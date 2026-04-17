mod constants;
mod discovery;
mod ignore;
mod incremental;
mod indexing;
mod models;
mod parser;
mod resolver;
mod storage;
mod watcher;

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use constants::{DATA_DIR_NAME, DB_FILENAME};
use indexing::{make_relative, parse_file_strict, parse_files, parse_files_strict};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        print_help();
        return;
    }

    let watch_mode = args.get(1).map(|s| s.as_str()) == Some("watch");
    let full_mode = args.iter().any(|a| a == "--full");
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

    let db_path = data_dir.join(DB_FILENAME);
    let db = storage::Database::open(&db_path).expect("Failed to open SQLite database");

    println!("Indexing: {}", workspace_root.display());
    let start = Instant::now();

    let all_files = discovery::find_cpp_files(&workspace_root);
    let all_relative: Vec<String> = all_files
        .iter()
        .map(|p| make_relative(&workspace_root, p))
        .collect();
    println!("Found {} C++ files", all_relative.len());

    if full_mode {
        println!("Mode: full rebuild");
        run_full(&db, &workspace_root, &all_relative, json_mode, verbose_mode, &data_dir);
    } else {
        let stored = db.read_file_records().unwrap_or_default();
        let plan = incremental::plan(&all_relative, &stored, &workspace_root);

        println!(
            "Mode: incremental ({} to index, {} unchanged, {} to delete)",
            plan.to_index.len(),
            plan.unchanged.len(),
            plan.to_delete.len()
        );

        if plan.to_index.is_empty() && plan.to_delete.is_empty() {
            println!("\nNothing to do.");
            return;
        }

        if let Err(e) = run_incremental(&db, &workspace_root, &plan, verbose_mode) {
            eprintln!("Incremental indexing failed: {}", e);
            std::process::exit(1);
        }
    }

    let elapsed = start.elapsed();
    println!("\nDone in {}ms", elapsed.as_millis());
    println!("  Output: {}", data_dir.display());
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
    println!("  --verbose    Print per-file indexing logs and lossy-read warnings");
    println!("  --json       Write JSON snapshots alongside the SQLite database");
    println!("  -h, --help   Show this help message");
    println!();
    println!("Output:");
    println!("  The index is stored in <workspace-root>/.codeatlas/index.db");
    println!("  Supported file extensions: .cpp, .h, .hpp, .cc, .cxx, .inl, .inc");
}

fn run_full(
    db: &storage::Database,
    workspace_root: &Path,
    all_relative: &[String],
    json_mode: bool,
    verbose: bool,
    data_dir: &Path,
) {
    let (raw_symbols, raw_calls, file_records) = parse_files(workspace_root, all_relative, verbose);
    let symbols = resolver::merge_symbols(&raw_symbols);
    let calls = resolver::resolve_calls(&raw_calls, &symbols);

    let raw_count: usize = all_relative.len();
    db.write_all(&raw_symbols, &symbols, &calls, &file_records)
        .expect("Failed to write to SQLite");

    if json_mode {
        write_json(&data_dir.join("symbols.json"), &symbols);
        write_json(&data_dir.join("calls.json"), &calls);
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
    let (parsed_symbols, new_raw_calls, new_files) = if !plan.to_index.is_empty() {
        parse_files_strict(workspace_root, &plan.to_index, verbose)?
    } else {
        (Vec::new(), Vec::new(), Vec::new())
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
            db.delete_raw_symbols_for_file(path)
                .map_err(|e| format!("Failed to delete raw symbols for {}: {}", path, e))?;
            db.delete_file_record(path)
                .map_err(|e| format!("Failed to delete file record for {}: {}", path, e))?;
        }
        for path in &plan.to_index {
            db.delete_calls_for_file(path)
                .map_err(|e| format!("Failed to delete calls for {}: {}", path, e))?;
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

        let affected = db
            .cleanup_dangling_calls()
            .map_err(|e| format!("Failed to cleanup dangling calls: {}", e))?;

        let mut all_calls = new_raw_calls;
        let plan_to_index_set: HashSet<&str> = plan.to_index.iter().map(|p| p.as_str()).collect();
        let mut files_to_reresolve: Vec<String> = plan.to_index.clone();

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
            all_calls.extend(result.raw_calls);
        }

        let resolved = resolver::resolve_calls_with_db(&all_calls, &refreshed_symbols, db);
        if !resolved.is_empty() {
            db.write_calls(&resolved)
                .map_err(|e| format!("Failed to write calls: {}", e))?;
        }

        if !new_files.is_empty() {
            db.write_files(&new_files)
                .map_err(|e| format!("Failed to write files: {}", e))?;
        }

        db.refresh_fts_for_symbol_ids(&affected_symbol_ids)
            .map_err(|e| format!("Failed to refresh FTS: {}", e))?;

        let total_syms: i64 = db.count_symbols().unwrap_or(0);
        let total_calls: i64 = db.count_calls().unwrap_or(0);
        let total_files: i64 = db.count_files().unwrap_or(0);
        println!(
            "  Symbols: {} | Calls: {} | Files: {} | Re-resolved: {} file(s)",
            total_syms, total_calls, total_files, files_to_reresolve.len(),
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
