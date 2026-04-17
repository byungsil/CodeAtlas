use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};
use std::fs;

use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

use crate::constants::{DATA_DIR_NAME, DB_FILENAME, EXTENSIONS};
use crate::discovery;
use crate::indexing::{make_relative, parse_file_strict, parse_files, parse_files_strict};
use crate::ignore::IgnoreRules;
use crate::incremental;
use crate::resolver;
use crate::storage::Database;

use chrono::Utc;

const DEBOUNCE_MS: u64 = 500;


fn is_tracked(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| EXTENSIONS.contains(&e))
        .unwrap_or(false)
}

fn is_in_workspace(path: &Path, workspace_root: &Path) -> bool {
    path.starts_with(workspace_root)
}

fn is_ignored_path(path: &Path, workspace_root: &Path, ignore_rules: &IgnoreRules) -> bool {
    if ignore_rules.is_empty() {
        return false;
    }
    let rel = path.strip_prefix(workspace_root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/");
    ignore_rules.is_ignored(&rel)
}

fn is_in_codeatlas_dir(path: &Path) -> bool {
    path.components().any(|c| c.as_os_str() == DATA_DIR_NAME)
}

pub fn watch(workspace_root: &Path, verbose: bool) -> Result<(), String> {
    let workspace_root = workspace_root
        .canonicalize()
        .map_err(|e| format!("Failed to resolve workspace root: {}", e))?;

    let data_dir = workspace_root.join(DATA_DIR_NAME);
    fs::create_dir_all(&data_dir).map_err(|e| format!("Failed to create data dir: {}", e))?;
    let db_path = data_dir.join(DB_FILENAME);

    let ignore_rules = IgnoreRules::load(&workspace_root);

    println!("Watch mode: {}", workspace_root.display());
    println!("Press Ctrl-C to stop.\n");

    {
        let db = Database::open(&db_path).map_err(|e| format!("DB open: {}", e))?;
        if db.has_data() {
            println!("Existing index found, running incremental catch-up...");
            run_incremental_index(&workspace_root, &db_path, verbose)?;
        } else {
            println!("No existing index, running full initial index...");
            run_full_index(&workspace_root, &db_path, verbose)?;
        }
    }

    let (tx, rx) = mpsc::channel::<Event>();

    let mut watcher = RecommendedWatcher::new(
        move |res: Result<Event, notify::Error>| {
            if let Ok(event) = res {
                let _ = tx.send(event);
            }
        },
        Config::default(),
    )
    .map_err(|e| format!("Failed to create watcher: {}", e))?;

    watcher
        .watch(&workspace_root, RecursiveMode::Recursive)
        .map_err(|e| format!("Failed to watch directory: {}", e))?;

    let mut pending: HashSet<PathBuf> = HashSet::new();
    let mut last_event = Instant::now();
    let debounce = Duration::from_millis(DEBOUNCE_MS);

    loop {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(event) => {
                let dominated = matches!(
                    event.kind,
                    EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
                );
                if dominated {
                    for path in &event.paths {
                        if is_tracked(path)
                            && is_in_workspace(path, &workspace_root)
                            && !is_in_codeatlas_dir(path)
                            && !is_ignored_path(path, &workspace_root, &ignore_rules)
                        {
                            pending.insert(path.clone());
                            last_event = Instant::now();
                        }
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }

        if !pending.is_empty() && last_event.elapsed() >= debounce {
            let changed: Vec<PathBuf> = pending.drain().collect();
            println!(
                "[{}] {} file(s) changed, re-indexing...",
                Utc::now().format("%H:%M:%S"),
                changed.len()
            );

            if let Err(e) = run_incremental_index(&workspace_root, &db_path, verbose) {
                eprintln!("  Indexing error: {}", e);
            }
        }
    }

    Ok(())
}

fn run_full_index(workspace_root: &Path, db_path: &Path, verbose: bool) -> Result<(), String> {
    let db = Database::open(db_path).map_err(|e| format!("DB open: {}", e))?;
    let files = discovery::find_cpp_files(workspace_root);
    let all_relative: Vec<String> = files
        .iter()
        .map(|p| make_relative(workspace_root, p))
        .collect();

    println!("Initial index: {} files", all_relative.len());
    let start = Instant::now();

    let (raw_symbols, raw_calls, file_records) = parse_files(workspace_root, &all_relative, verbose);
    let symbols = resolver::merge_symbols(&raw_symbols);
    let calls = resolver::resolve_calls(&raw_calls, &symbols);
    db.write_all(&raw_symbols, &symbols, &calls, &file_records)
        .map_err(|e| format!("DB write: {}", e))?;

    println!(
        "  Done in {}ms: {} symbols, {} calls",
        start.elapsed().as_millis(),
        symbols.len(),
        calls.len()
    );
    Ok(())
}

fn run_incremental_index(workspace_root: &Path, db_path: &Path, verbose: bool) -> Result<(), String> {
    let db = Database::open(db_path).map_err(|e| format!("DB open: {}", e))?;
    let files = discovery::find_cpp_files(workspace_root);
    let all_relative: Vec<String> = files
        .iter()
        .map(|p| make_relative(workspace_root, p))
        .collect();

    let stored = db.read_file_records().unwrap_or_default();
    let plan = incremental::plan(&all_relative, &stored, workspace_root);

    if plan.to_index.is_empty() && plan.to_delete.is_empty() {
        println!("  No effective changes.");
        return Ok(());
    }

    let start = Instant::now();

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

    db.begin().map_err(|e| format!("DB begin: {}", e))?;

    let tx_result: Result<(), String> = (|| {
        for path in &plan.to_delete {
            db.delete_calls_for_file(path)
                .map_err(|e| format!("DB delete calls: {}", e))?;
            db.delete_raw_symbols_for_file(path)
                .map_err(|e| format!("DB delete raw symbols: {}", e))?;
            db.delete_file_record(path)
                .map_err(|e| format!("DB delete file record: {}", e))?;
        }
        for path in &plan.to_index {
            db.delete_calls_for_file(path)
                .map_err(|e| format!("DB delete calls: {}", e))?;
            db.delete_raw_symbols_for_file(path)
                .map_err(|e| format!("DB delete raw symbols: {}", e))?;
            db.delete_file_record(path)
                .map_err(|e| format!("DB delete file record: {}", e))?;
        }

        if !parsed_symbols.is_empty() {
            db.write_raw_symbols(&parsed_symbols)
                .map_err(|e| format!("DB write raw symbols: {}", e))?;
        }
        db.refresh_symbols_for_ids(&affected_symbol_ids)
            .map_err(|e| format!("DB refresh symbols: {}", e))?;

        let refreshed_symbols = db
            .find_symbols_by_ids(&affected_symbol_ids)
            .map_err(|e| format!("DB read refreshed symbols: {}", e))?;

        let affected = db.cleanup_dangling_calls()
            .map_err(|e| format!("DB cleanup: {}", e))?;

        let mut all_raw_calls = new_raw_calls;
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
                .map_err(|e| format!("DB delete calls: {}", e))?;
            all_raw_calls.extend(result.raw_calls);
        }

        let resolved = resolver::resolve_calls_with_db(&all_raw_calls, &refreshed_symbols, &db);
        if !resolved.is_empty() {
            db.write_calls(&resolved)
                .map_err(|e| format!("DB write calls: {}", e))?;
        }
        if !new_files.is_empty() {
            db.write_files(&new_files)
                .map_err(|e| format!("DB write files: {}", e))?;
        }
        db.refresh_fts_for_symbol_ids(&affected_symbol_ids)
            .map_err(|e| format!("DB refresh FTS: {}", e))?;

        let total_syms = db.count_symbols().unwrap_or(0);
        let total_calls = db.count_calls().unwrap_or(0);
        let total_files = db.count_files().unwrap_or(0);
        println!(
            "  Done in {}ms: {} symbols, {} calls, {} files, {} file(s) re-resolved",
            start.elapsed().as_millis(),
            total_syms,
            total_calls,
            total_files,
            files_to_reresolve.len()
        );
        Ok(())
    })();

    if let Err(err) = tx_result {
        let _ = db.rollback();
        return Err(err);
    }

    db.commit().map_err(|e| format!("DB commit: {}", e))?;

    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn is_tracked_accepts_cpp_extensions() {
        assert!(is_tracked(Path::new("foo.cpp")));
        assert!(is_tracked(Path::new("bar.h")));
        assert!(is_tracked(Path::new("baz.inl")));
        assert!(is_tracked(Path::new("qux.inc")));
        assert!(!is_tracked(Path::new("readme.md")));
        assert!(!is_tracked(Path::new("data.json")));
    }

    #[test]
    fn is_in_workspace_filters_correctly() {
        let root = PathBuf::from("/project/src");
        assert!(is_in_workspace(Path::new("/project/src/foo.cpp"), &root));
        assert!(is_in_workspace(Path::new("/project/src/sub/bar.h"), &root));
        assert!(!is_in_workspace(Path::new("/other/place/foo.cpp"), &root));
        assert!(!is_in_workspace(Path::new("/project/foo.cpp"), &root));
    }

    #[test]
    fn is_in_codeatlas_dir_filters_correctly() {
        assert!(is_in_codeatlas_dir(Path::new("/project/.codeatlas/index.db")));
        assert!(is_in_codeatlas_dir(Path::new("/project/.codeatlas/symbols.json")));
        assert!(!is_in_codeatlas_dir(Path::new("/project/src/foo.cpp")));
        assert!(!is_in_codeatlas_dir(Path::new("/project/codeatlas/foo.cpp")));
    }
}
