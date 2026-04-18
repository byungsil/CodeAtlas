mod constants;
mod discovery;
mod graph_rules;
mod ignore;
mod incremental;
mod indexing;
mod metadata;
mod models;
mod parser;
mod resolver;
mod storage;
mod watcher;

use std::collections::HashSet;
use std::fs;
use std::io;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;
use std::time::Instant;

use constants::{DATA_DIR_NAME, DB_FILENAME};
use indexing::{make_relative, parse_file_strict, parse_files, parse_files_strict};
use rusqlite::ErrorCode;

const IO_RETRY_BACKOFF_MS: &[u64] = &[0, 100, 250, 500];

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        print_help();
        return;
    }

    let watch_mode = args.get(1).map(|s| s.as_str()) == Some("watch");
    let requested_full_mode = args.iter().any(|a| a == "--full");
    let json_mode = args.iter().any(|a| a == "--json");
    let verbose_mode = args.iter().any(|a| a == "--verbose");
    let positional: Vec<&String> = args.iter().skip(1).filter(|a| !a.starts_with('-') && a.as_str() != "watch").collect();

    let workspace_root = match positional.first() {
        Some(p) => PathBuf::from(p),
        None => {
            eprintln!("Usage: codeatlas-indexer [watch] <workspace-root> [--full] [--json]");
            eprintln!("Try `codeatlas-indexer --help` for more information.");
            std::process::exit(1);
        }
    };

    if !workspace_root.exists() {
        eprintln!("Directory not found: {}", workspace_root.display());
        std::process::exit(1);
    }

    if watch_mode {
        if let Err(e) = watcher::watch(&workspace_root, verbose_mode) {
            eprintln!("Watch error: {}", e);
            std::process::exit(1);
        }
        return;
    }

    let data_dir = workspace_root.join(DATA_DIR_NAME);
    fs::create_dir_all(&data_dir).expect("Failed to create data directory");
    mark_codeatlas_artifacts(&data_dir, None);

    let db_path = data_dir.join(DB_FILENAME);
    let mut effective_full_mode = determine_effective_full_mode(&db_path, requested_full_mode);
    let staging_db_path = make_staging_db_path(&db_path, "main");
    if let Err(err) = prepare_staging_db(&db_path, &staging_db_path, effective_full_mode) {
        if !effective_full_mode {
            eprintln!(
                "Failed to prepare incremental staging database ({}). Falling back to full rebuild.",
                err
            );
            effective_full_mode = true;
            prepare_staging_db(&db_path, &staging_db_path, true)
                .expect("Failed to prepare staging SQLite database after full-rebuild fallback");
        } else {
            panic!("Failed to prepare staging SQLite database: {}", err);
        }
    }
    {
        let db = open_database_with_retry(&staging_db_path, "open staging sqlite database")
            .expect("Failed to open SQLite database");

        println!("Indexing: {}", workspace_root.display());
        let start = Instant::now();

        let all_files = discovery::find_cpp_files_with_feedback(&workspace_root, verbose_mode);
        let all_relative: Vec<String> = all_files
            .iter()
            .map(|p| make_relative(&workspace_root, p))
            .collect();
        println!("Found {} C++ files", all_relative.len());

        if effective_full_mode {
            println!("Mode: full rebuild");
            run_full(&db, &workspace_root, &all_relative, json_mode, verbose_mode, &data_dir);
        } else {
            let stored = db.read_file_records().unwrap_or_default();
            let plan = incremental::plan(&all_relative, &stored, &workspace_root);
            let escalation = incremental::assess_escalation(all_relative.len(), &plan);

            println!(
                "Mode: incremental ({} to index, {} unchanged, {} to delete)",
                plan.to_index.len(),
                plan.unchanged.len(),
                plan.to_delete.len()
            );
            log_incremental_plan(&plan, verbose_mode);

            if let Some(reason) = &escalation.reason {
                println!("  Escalation: {}", reason);
            }

            if escalation.level == incremental::EscalationLevel::FullRebuild {
                println!("Mode override: full rebuild");
                run_full(&db, &workspace_root, &all_relative, json_mode, verbose_mode, &data_dir);
            } else {
                if plan.to_index.is_empty() && plan.to_delete.is_empty() {
                    let elapsed = start.elapsed();
                    println!("\nNothing to do.");
                    println!("Done in {}", format_elapsed(elapsed.as_millis()));
                    println!("  Output: {}", data_dir.display());
                    return;
                }

                if let Err(e) = run_incremental(&db, &workspace_root, &plan, verbose_mode) {
                    eprintln!("Incremental indexing failed: {}", e);
                    std::process::exit(1);
                }
            }
        }

        db.checkpoint().expect("Failed to checkpoint SQLite database");

        let elapsed = start.elapsed();
        println!("\nDone in {}", format_elapsed(elapsed.as_millis()));
        println!("  Output: {}", data_dir.display());
    }
    publish_staging_db(&staging_db_path, &db_path).expect("Failed to publish SQLite database");
    mark_codeatlas_artifacts(&data_dir, Some(&db_path));

}

fn format_elapsed(elapsed_ms: u128) -> String {
    if elapsed_ms >= 1_000 {
        format!("{}ms ({:.2}s)", elapsed_ms, elapsed_ms as f64 / 1_000.0)
    } else {
        format!("{}ms", elapsed_ms)
    }
}

fn print_help() {
    println!("CodeAtlas Indexer");
    println!();
    println!("Usage:");
    println!("  codeatlas-indexer <workspace-root>");
    println!("  codeatlas-indexer <workspace-root> --full");
    println!("  codeatlas-indexer <workspace-root> --full --json");
    println!("  codeatlas-indexer watch <workspace-root>");
    println!("  codeatlas-indexer --help");
    println!();
    println!("Modes:");
    println!("  incremental  Re-index only changed files and remove deleted files");
    println!("  --full       Rebuild the entire index from scratch");
    println!("  watch        Monitor the workspace and re-index on file changes");
    println!();
    println!("Options:");
    println!("  --verbose    Show discovery spinner, per-file progress, and lossy-read warnings");
    println!("  --json       Write JSON snapshots alongside the SQLite database");
    println!("  -h, --help   Show this help message");
    println!();
    println!("Output:");
    println!("  The index is stored in <workspace-root>/.codeatlas/index.db");
    println!("  Supported file extensions: .cpp, .h, .hpp, .cc, .cxx, .inl, .inc");
}

fn prepare_staging_db(final_db_path: &Path, staging_db_path: &Path, full_mode: bool) -> std::io::Result<()> {
    if staging_db_path.exists() {
        retry_io("remove stale staging db", || fs::remove_file(staging_db_path))?;
    }

    if !full_mode && final_db_path.exists() {
        retry_io("copy published db to staging", || {
            fs::copy(final_db_path, staging_db_path).map(|_| ())
        })?;
    }

    Ok(())
}

fn determine_effective_full_mode(db_path: &Path, requested_full_mode: bool) -> bool {
    if requested_full_mode {
        return true;
    }

    match storage::validate_existing_database(db_path) {
        Ok(()) => false,
        Err(issue) => {
            eprintln!("Existing index is unhealthy ({}). Forcing full rebuild.", issue);
            true
        }
    }
}

fn make_staging_db_path(final_db_path: &Path, tag: &str) -> PathBuf {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    final_db_path.hash(&mut hasher);
    let key = hasher.finish();
    std::env::temp_dir().join(format!("codeatlas-{}-{}-{}.db", tag, key, DB_FILENAME))
}

fn publish_staging_db(staging_db_path: &Path, final_db_path: &Path) -> std::io::Result<()> {
    let publish_path = final_db_path.with_file_name(format!("{}.next", DB_FILENAME));

    if publish_path.exists() {
        retry_io("remove stale publish db", || fs::remove_file(&publish_path))?;
    }

    retry_io("copy staging db to publish db", || {
        fs::copy(staging_db_path, &publish_path).map(|_| ())
    })?;

    if final_db_path.exists() {
        retry_io("replace published db", || fs::remove_file(final_db_path))?;
    }

    retry_io("rename publish db into place", || fs::rename(&publish_path, final_db_path))?;
    retry_io("remove staging db after publish", || fs::remove_file(staging_db_path))?;
    Ok(())
}

fn retry_io<T, F>(operation: &str, mut action: F) -> io::Result<T>
where
    F: FnMut() -> io::Result<T>,
{
    let mut last_error = None;

    for (attempt, delay_ms) in IO_RETRY_BACKOFF_MS.iter().enumerate() {
        if *delay_ms > 0 {
            thread::sleep(Duration::from_millis(*delay_ms));
        }

        match action() {
            Ok(value) => return Ok(value),
            Err(err) if should_retry_io_error(&err) && attempt + 1 < IO_RETRY_BACKOFF_MS.len() => {
                last_error = Some(err);
            }
            Err(err) => {
                return Err(io::Error::new(
                    err.kind(),
                    format!("{} failed: {}", operation, err),
                ));
            }
        }
    }

    let err = last_error.unwrap_or_else(|| io::Error::other("retry failed without an error"));
    Err(io::Error::new(
        err.kind(),
        format!("{} failed after retries: {}", operation, err),
    ))
}

fn should_retry_io_error(err: &io::Error) -> bool {
    matches!(err.kind(), io::ErrorKind::PermissionDenied | io::ErrorKind::WouldBlock)
}

fn open_database_with_retry(path: &Path, operation: &str) -> Result<storage::Database, String> {
    let mut last_error = None;

    for (attempt, delay_ms) in IO_RETRY_BACKOFF_MS.iter().enumerate() {
        if *delay_ms > 0 {
            thread::sleep(Duration::from_millis(*delay_ms));
        }

        match storage::Database::open(path) {
            Ok(db) => return Ok(db),
            Err(err) if should_retry_sqlite_open(&err) && attempt + 1 < IO_RETRY_BACKOFF_MS.len() => {
                last_error = Some(err.to_string());
            }
            Err(err) => {
                return Err(format!("{} failed: {}", operation, err));
            }
        }
    }

    Err(format!(
        "{} failed after retries: {}",
        operation,
        last_error.unwrap_or_else(|| "unknown error".to_string())
    ))
}

fn should_retry_sqlite_open(err: &rusqlite::Error) -> bool {
    matches!(
        err,
        rusqlite::Error::SqliteFailure(code, _) if code.code == ErrorCode::CannotOpen || code.code == ErrorCode::DatabaseBusy
    )
}

fn mark_codeatlas_artifacts(data_dir: &Path, db_path: Option<&Path>) {
    #[cfg(windows)]
    {
        let _ = run_attrib(&["+H", "+I"], data_dir);
        if let Some(path) = db_path {
            let _ = run_attrib(&["+I"], path);
        }
    }

    #[cfg(not(windows))]
    {
        let _ = data_dir;
        let _ = db_path;
    }
}

#[cfg(windows)]
fn run_attrib(flags: &[&str], path: &Path) -> std::io::Result<()> {
    let status = Command::new("attrib")
        .args(flags)
        .arg(path)
        .status()?;

    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other("attrib command failed"))
    }
}


fn run_full(
    db: &storage::Database,
    workspace_root: &Path,
    all_relative: &[String],
    json_mode: bool,
    verbose: bool,
    data_dir: &Path,
) {
    let (raw_symbols, raw_calls, normalized_references, file_records) = parse_files(workspace_root, all_relative, verbose);
    let symbols = resolver::merge_symbols(&raw_symbols);
    let calls = resolver::resolve_calls(&raw_calls, &symbols);

    let raw_count: usize = all_relative.len();
    db.write_all(&raw_symbols, &symbols, &calls, &normalized_references, &file_records)
        .expect("Failed to write to SQLite");

    if json_mode {
        write_json(&data_dir.join("symbols.json"), &symbols);
        write_json(&data_dir.join("calls.json"), &calls);
        write_json(&data_dir.join("references.json"), &normalized_references);
        write_json(&data_dir.join("files.json"), &file_records);
    }

    println!("  Symbols: {} | Calls: {} | Files: {}", symbols.len(), calls.len(), raw_count);
}

fn run_incremental(
    db: &storage::Database,
    workspace_root: &Path,
    plan: &incremental::IncrementalPlan,
    verbose: bool,
) -> Result<(), String> {
    let (parsed_symbols, new_raw_calls, new_references, new_files) = if !plan.to_index.is_empty() {
        parse_files_strict(workspace_root, &plan.to_index, verbose)?
    } else {
        (Vec::new(), Vec::new(), Vec::new(), Vec::new())
    };

    let mut replaced_paths = plan.to_delete.clone();
    replaced_paths.extend(plan.to_index.clone());
    let old_symbols = db
        .read_symbols_for_paths(&replaced_paths)
        .map_err(|e| format!("Failed to read existing symbols: {}", e))?;
    let mut affected_symbol_ids: Vec<String> = old_symbols.iter().map(|s| s.id.clone()).collect();

    for id in parsed_symbols.iter().map(|s| s.id.clone()) {
        if !affected_symbol_ids.contains(&id) {
            affected_symbol_ids.push(id);
        }
    }
    affected_symbol_ids.sort();
    affected_symbol_ids.dedup();

    db.begin().map_err(|e| format!("Failed to begin transaction: {}", e))?;

    let tx_result: Result<(), String> = (|| {
        for path in &plan.to_delete {
            println!("  DELETE: {}", path);
            db.delete_calls_for_file(path)
                .map_err(|e| format!("Failed to delete calls for {}: {}", path, e))?;
            db.delete_references_for_file(path)
                .map_err(|e| format!("Failed to delete references for {}: {}", path, e))?;
            db.delete_raw_symbols_for_file(path)
                .map_err(|e| format!("Failed to delete raw symbols for {}: {}", path, e))?;
            db.delete_file_record(path)
                .map_err(|e| format!("Failed to delete file record for {}: {}", path, e))?;
        }
        for path in &plan.to_index {
            db.delete_calls_for_file(path)
                .map_err(|e| format!("Failed to delete calls for {}: {}", path, e))?;
            db.delete_references_for_file(path)
                .map_err(|e| format!("Failed to delete references for {}: {}", path, e))?;
            db.delete_raw_symbols_for_file(path)
                .map_err(|e| format!("Failed to delete raw symbols for {}: {}", path, e))?;
            db.delete_file_record(path)
                .map_err(|e| format!("Failed to delete file record for {}: {}", path, e))?;
        }

        if !parsed_symbols.is_empty() {
            db.write_raw_symbols(&parsed_symbols)
                .map_err(|e| format!("Failed to write raw symbols: {}", e))?;
        }
        db.refresh_symbols_for_ids(&affected_symbol_ids)
            .map_err(|e| format!("Failed to refresh symbols: {}", e))?;

        let refreshed_symbols = db
            .find_symbols_by_ids(&affected_symbol_ids)
            .map_err(|e| format!("Failed to read refreshed symbols: {}", e))?;

        let affected_calls = db
            .cleanup_dangling_calls()
            .map_err(|e| format!("Failed to cleanup dangling calls: {}", e))?;
        let affected_references = db
            .cleanup_dangling_references()
            .map_err(|e| format!("Failed to cleanup dangling references: {}", e))?;

        let mut all_calls = new_raw_calls;
        let mut all_references = new_references;
        let plan_to_index_set: HashSet<&str> = plan.to_index.iter().map(|p| p.as_str()).collect();
        let mut files_to_reresolve: Vec<String> = plan.to_index.clone();

        let mut affected = affected_calls;
        for path in affected_references {
            if !affected.contains(&path) {
                affected.push(path);
            }
        }

        for path in &affected {
            if !files_to_reresolve.contains(path) {
                files_to_reresolve.push(path.clone());
            }
            if plan_to_index_set.contains(path.as_str()) {
                continue;
            }

            let (result, _, lossy) = parse_file_strict(workspace_root, path)?;
            if verbose && lossy {
                println!("  LOSSY: {}: non-UTF8 bytes replaced during parsing", path);
            }
            db.delete_calls_for_file(path)
                .map_err(|e| format!("Failed to delete calls for {}: {}", path, e))?;
            db.delete_references_for_file(path)
                .map_err(|e| format!("Failed to delete references for {}: {}", path, e))?;
            all_calls.extend(result.raw_calls);
            all_references.extend(result.normalized_references);
        }

        let resolved = resolver::resolve_calls_with_db(&all_calls, &refreshed_symbols, db);
        if !resolved.is_empty() {
            db.write_calls(&resolved)
                .map_err(|e| format!("Failed to write calls: {}", e))?;
        }
        if !all_references.is_empty() {
            db.write_references(&all_references)
                .map_err(|e| format!("Failed to write references: {}", e))?;
        }

        if !new_files.is_empty() {
            db.write_files(&new_files)
                .map_err(|e| format!("Failed to write files: {}", e))?;
        }

        db.refresh_fts_for_symbol_ids(&affected_symbol_ids)
            .map_err(|e| format!("Failed to refresh FTS: {}", e))?;

        let total_syms: i64 = db.count_symbols().unwrap_or(0);
        let total_calls: i64 = db.count_calls().unwrap_or(0);
        let total_references: i64 = db.count_references().unwrap_or(0);
        let total_files: i64 = db.count_files().unwrap_or(0);
        println!(
            "  Symbols: {} | Calls: {} | References: {} | Files: {} | Re-resolved: {} file(s)",
            total_syms, total_calls, total_references, total_files, files_to_reresolve.len(),
        );
        Ok(())
    })();

    if let Err(err) = tx_result {
        let _ = db.rollback();
        return Err(err);
    }

    db.commit()
        .map_err(|e| format!("Failed to commit transaction: {}", e))?;

    if plan.to_index.is_empty() && !plan.to_delete.is_empty() {
        println!("  Deleted {} file(s)", plan.to_delete.len());
    }
    Ok(())
}

fn write_json<T: serde::Serialize>(path: &Path, data: &T) {
    let json = serde_json::to_string_pretty(data).expect("Failed to serialize JSON");
    fs::write(path, json).expect("Failed to write JSON file");
}

fn log_incremental_plan(plan: &incremental::IncrementalPlan, verbose: bool) {
    if !plan.rename_hints.is_empty() {
        println!("  Planner: {} rename/move hint(s)", plan.rename_hints.len());
        if verbose {
            for hint in &plan.rename_hints {
                println!("    MOVE?: {} -> {}", hint.from_path, hint.to_path);
            }
        }
    }

    if verbose {
        for entry in &plan.entries {
            let matched = entry
                .matched_path
                .as_deref()
                .map(|path| format!(" -> {}", path))
                .unwrap_or_default();
            println!(
                "  PLAN: {} {} ({}){}",
                entry.disposition.as_str(),
                entry.path,
                entry.reason.as_str(),
                matched
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::{params, Connection};
    use tempfile::tempdir;

    fn copy_snapshot(src: &Path, dst: &Path) {
        fs::create_dir_all(dst).unwrap();
        for entry in fs::read_dir(src).unwrap() {
            let entry = entry.unwrap();
            let src_path = entry.path();
            let dst_path = dst.join(entry.file_name());
            if src_path.is_dir() {
                copy_snapshot(&src_path, &dst_path);
            } else {
                if let Some(parent) = dst_path.parent() {
                    fs::create_dir_all(parent).unwrap();
                }
                fs::copy(&src_path, &dst_path).unwrap();
            }
        }
    }

    fn clear_workspace_except_codeatlas(workspace_root: &Path) {
        if !workspace_root.exists() {
            return;
        }

        for entry in fs::read_dir(workspace_root).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.file_name().and_then(|n| n.to_str()) == Some(DATA_DIR_NAME) {
                continue;
            }
            if path.is_dir() {
                fs::remove_dir_all(path).unwrap();
            } else {
                fs::remove_file(path).unwrap();
            }
        }
    }

    fn apply_fixture_snapshot(workspace_root: &Path, fixture_root: &Path, snapshot: &str) {
        clear_workspace_except_codeatlas(workspace_root);
        copy_snapshot(&fixture_root.join(snapshot), workspace_root);
    }

    fn fixture_root(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../samples/incremental")
            .join(name)
    }

    fn discover_relative_paths(workspace_root: &Path) -> Vec<String> {
        discovery::find_cpp_files_with_feedback(workspace_root, false)
            .iter()
            .map(|p| make_relative(workspace_root, p))
            .collect()
    }

    fn full_index_fixture(workspace_root: &Path, db_path: &Path) {
        let data_dir = workspace_root.join(DATA_DIR_NAME);
        fs::create_dir_all(&data_dir).unwrap();
        let db = storage::Database::open(db_path).unwrap();
        let all_relative = discover_relative_paths(workspace_root);
        run_full(&db, workspace_root, &all_relative, false, false, &data_dir);
        db.checkpoint().unwrap();
    }

    fn incremental_reindex_fixture(
        workspace_root: &Path,
        db_path: &Path,
    ) -> incremental::IncrementalPlan {
        let db = storage::Database::open(db_path).unwrap();
        let all_relative = discover_relative_paths(workspace_root);
        let stored = db.read_file_records().unwrap();
        let plan = incremental::plan(&all_relative, &stored, workspace_root);
        run_incremental(&db, workspace_root, &plan, false).unwrap();
        db.checkpoint().unwrap();
        plan
    }

    fn symbol_exists(db_path: &Path, qualified_name: &str) -> bool {
        let conn = Connection::open(db_path).unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM symbols WHERE qualified_name = ?1",
                params![qualified_name],
                |row| row.get(0),
            )
            .unwrap();
        count > 0
    }

    fn call_count_for_file(db_path: &Path, file_path: &str) -> i64 {
        let conn = Connection::open(db_path).unwrap();
        conn.query_row(
            "SELECT COUNT(*) FROM calls WHERE file_path = ?1",
            params![file_path],
            |row| row.get(0),
        )
        .unwrap()
    }

    fn file_record_exists(db_path: &Path, file_path: &str) -> bool {
        let conn = Connection::open(db_path).unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM files WHERE path = ?1",
                params![file_path],
                |row| row.get(0),
            )
            .unwrap();
        count > 0
    }

    fn file_record_hash(db_path: &Path, file_path: &str) -> String {
        let conn = Connection::open(db_path).unwrap();
        conn.query_row(
            "SELECT content_hash FROM files WHERE path = ?1",
            params![file_path],
            |row| row.get(0),
        )
        .unwrap()
    }

    fn definition_file_path(db_path: &Path, qualified_name: &str) -> Option<String> {
        let conn = Connection::open(db_path).unwrap();
        conn.query_row(
            "SELECT definition_file_path FROM symbols WHERE qualified_name = ?1 LIMIT 1",
            params![qualified_name],
            |row| row.get(0),
        )
        .ok()
    }

    fn count_dangling_calls(db_path: &Path) -> i64 {
        let conn = Connection::open(db_path).unwrap();
        conn.query_row(
            "SELECT COUNT(*) FROM calls WHERE caller_id NOT IN (SELECT id FROM symbols) OR callee_id NOT IN (SELECT id FROM symbols)",
            [],
            |row| row.get(0),
        )
        .unwrap()
    }

    fn count_dangling_references(db_path: &Path) -> i64 {
        let conn = Connection::open(db_path).unwrap();
        conn.query_row(
            "SELECT COUNT(*) FROM symbol_references WHERE source_symbol_id NOT IN (SELECT id FROM symbols) OR target_symbol_id NOT IN (SELECT id FROM symbols)",
            [],
            |row| row.get(0),
        )
        .unwrap()
    }

    #[test]
    fn incremental_fixture_add_file_adds_new_symbols_and_records() {
        let fixture = fixture_root("add_file");
        let temp = tempdir().unwrap();
        let workspace_root = temp.path();
        let db_path = workspace_root.join(DATA_DIR_NAME).join(DB_FILENAME);

        apply_fixture_snapshot(workspace_root, &fixture, "before");
        full_index_fixture(workspace_root, &db_path);

        apply_fixture_snapshot(workspace_root, &fixture, "after");
        let plan = incremental_reindex_fixture(workspace_root, &db_path);

        assert_eq!(plan.to_delete.len(), 0);
        assert!(plan.to_index.contains(&"added.cpp".to_string()));
        assert!(plan.to_index.contains(&"added.h".to_string()));
        assert!(file_record_exists(&db_path, "added.cpp"));
        assert!(file_record_exists(&db_path, "added.h"));
        assert!(symbol_exists(&db_path, "demo::Added"));
        assert_eq!(count_dangling_calls(&db_path), 0);
        assert_eq!(count_dangling_references(&db_path), 0);
    }

    #[test]
    fn incremental_fixture_delete_file_removes_stale_symbols_and_calls() {
        let fixture = fixture_root("delete_file");
        let temp = tempdir().unwrap();
        let workspace_root = temp.path();
        let db_path = workspace_root.join(DATA_DIR_NAME).join(DB_FILENAME);

        apply_fixture_snapshot(workspace_root, &fixture, "before");
        full_index_fixture(workspace_root, &db_path);
        assert!(symbol_exists(&db_path, "demo::Gone"));
        assert_eq!(call_count_for_file(&db_path, "main.cpp"), 1);

        apply_fixture_snapshot(workspace_root, &fixture, "after");
        let plan = incremental_reindex_fixture(workspace_root, &db_path);

        assert!(plan.to_delete.contains(&"gone.cpp".to_string()));
        assert!(plan.to_delete.contains(&"gone.h".to_string()));
        assert!(!file_record_exists(&db_path, "gone.cpp"));
        assert!(!file_record_exists(&db_path, "gone.h"));
        assert!(!symbol_exists(&db_path, "demo::Gone"));
        assert_eq!(call_count_for_file(&db_path, "main.cpp"), 0);
        assert_eq!(count_dangling_calls(&db_path), 0);
    }

    #[test]
    fn incremental_fixture_edit_symbol_rename_cleans_up_stale_relations() {
        let fixture = fixture_root("edit_symbol_rename");
        let temp = tempdir().unwrap();
        let workspace_root = temp.path();
        let db_path = workspace_root.join(DATA_DIR_NAME).join(DB_FILENAME);

        apply_fixture_snapshot(workspace_root, &fixture, "before");
        full_index_fixture(workspace_root, &db_path);
        assert!(symbol_exists(&db_path, "demo::Update"));
        assert_eq!(call_count_for_file(&db_path, "main.cpp"), 1);

        apply_fixture_snapshot(workspace_root, &fixture, "after");
        let plan = incremental_reindex_fixture(workspace_root, &db_path);

        assert!(plan.to_index.contains(&"worker.cpp".to_string()));
        assert!(plan.to_index.contains(&"worker.h".to_string()));
        assert!(!symbol_exists(&db_path, "demo::Update"));
        assert!(symbol_exists(&db_path, "demo::Refresh"));
        assert_eq!(call_count_for_file(&db_path, "main.cpp"), 0);
        assert_eq!(count_dangling_calls(&db_path), 0);
        assert_eq!(count_dangling_references(&db_path), 0);
    }

    #[test]
    fn incremental_fixture_rename_move_rewrites_file_records_without_losing_symbol() {
        let fixture = fixture_root("rename_move");
        let temp = tempdir().unwrap();
        let workspace_root = temp.path();
        let db_path = workspace_root.join(DATA_DIR_NAME).join(DB_FILENAME);

        apply_fixture_snapshot(workspace_root, &fixture, "before");
        full_index_fixture(workspace_root, &db_path);

        apply_fixture_snapshot(workspace_root, &fixture, "after");
        let plan = incremental_reindex_fixture(workspace_root, &db_path);

        assert!(plan.to_delete.contains(&"helper.cpp".to_string()));
        assert!(plan.to_index.contains(&"impl/helper_impl.cpp".to_string()));
        assert!(!file_record_exists(&db_path, "helper.cpp"));
        assert!(file_record_exists(&db_path, "impl/helper_impl.cpp"));
        assert!(symbol_exists(&db_path, "demo::Helper"));
        assert_eq!(
            definition_file_path(&db_path, "demo::Helper").as_deref(),
            Some("impl/helper_impl.cpp")
        );
        assert_eq!(call_count_for_file(&db_path, "main.cpp"), 1);
    }

    #[test]
    fn incremental_fixture_header_comment_change_updates_hash_without_changing_relations() {
        let fixture = fixture_root("header_comment_change");
        let temp = tempdir().unwrap();
        let workspace_root = temp.path();
        let db_path = workspace_root.join(DATA_DIR_NAME).join(DB_FILENAME);

        apply_fixture_snapshot(workspace_root, &fixture, "before");
        full_index_fixture(workspace_root, &db_path);
        let before_hash = file_record_hash(&db_path, "api.h");

        apply_fixture_snapshot(workspace_root, &fixture, "after");
        let plan = incremental_reindex_fixture(workspace_root, &db_path);

        assert_eq!(plan.to_delete.len(), 0);
        assert!(plan.to_index.contains(&"api.h".to_string()));
        assert_ne!(before_hash, file_record_hash(&db_path, "api.h"));
        assert!(symbol_exists(&db_path, "demo::Stable"));
        assert_eq!(call_count_for_file(&db_path, "main.cpp"), 1);
        assert_eq!(count_dangling_calls(&db_path), 0);
    }

    #[test]
    fn incremental_fixture_mass_churn_keeps_database_consistent() {
        let fixture = fixture_root("mass_churn");
        let temp = tempdir().unwrap();
        let workspace_root = temp.path();
        let db_path = workspace_root.join(DATA_DIR_NAME).join(DB_FILENAME);

        apply_fixture_snapshot(workspace_root, &fixture, "before");
        full_index_fixture(workspace_root, &db_path);

        apply_fixture_snapshot(workspace_root, &fixture, "after");
        let plan = incremental_reindex_fixture(workspace_root, &db_path);

        assert!(plan.to_delete.contains(&"a.cpp".to_string()));
        assert!(plan.to_delete.contains(&"a.h".to_string()));
        assert!(plan.to_index.contains(&"b.cpp".to_string()));
        assert!(plan.to_index.contains(&"b.h".to_string()));
        assert!(plan.to_index.contains(&"c.cpp".to_string()));
        assert!(plan.to_index.contains(&"c.h".to_string()));
        assert!(!symbol_exists(&db_path, "demo::A"));
        assert!(!symbol_exists(&db_path, "demo::B"));
        assert!(symbol_exists(&db_path, "demo::B2"));
        assert!(symbol_exists(&db_path, "demo::C"));
        assert_eq!(call_count_for_file(&db_path, "main.cpp"), 0);
        assert_eq!(count_dangling_calls(&db_path), 0);
        assert_eq!(count_dangling_references(&db_path), 0);
    }

    #[test]
    fn retry_io_retries_permission_denied_then_succeeds() {
        use std::cell::Cell;

        let attempts = Cell::new(0usize);
        let result = retry_io("test op", || {
            let current = attempts.get();
            attempts.set(current + 1);
            if current == 0 {
                Err(io::Error::new(io::ErrorKind::PermissionDenied, "locked"))
            } else {
                Ok("ok")
            }
        })
        .unwrap();

        assert_eq!(result, "ok");
        assert_eq!(attempts.get(), 2);
    }

    #[test]
    fn retry_io_does_not_retry_non_retryable_errors() {
        use std::cell::Cell;

        let attempts = Cell::new(0usize);
        let err = retry_io("test op", || {
            attempts.set(attempts.get() + 1);
            Err::<(), _>(io::Error::new(io::ErrorKind::NotFound, "missing"))
        })
        .unwrap_err();

        assert_eq!(attempts.get(), 1);
        assert!(err.to_string().contains("test op failed"));
    }

    #[test]
    fn should_retry_sqlite_open_matches_cannot_open() {
        let err = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                code: ErrorCode::CannotOpen,
                extended_code: ErrorCode::CannotOpen as i32,
            },
            None,
        );

        assert!(should_retry_sqlite_open(&err));
    }
}
