mod build_metadata;
mod constants;
mod discovery;
mod graph_rules;
mod ignore;
mod incremental;
mod indexing;
mod language;
mod lua_parser;
mod metadata;
mod models;
mod parser;
mod python_parser;
mod representative_rules;
mod resolver;
mod rust_parser;
mod storage;
mod typescript_parser;
mod watcher;

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;
use std::time::Instant;

use constants::{DATA_DIR_NAME, DB_FILENAME, EXTENSIONS, INDEX_EXTENSIONS_ENV, parse_extension_list};
use indexing::{
    default_language_registry, make_relative, parse_discovered_files_with_progress, parse_file_strict,
    parse_files_strict,
};
use models::ParseMetrics;
use representative_rules::{load_workspace_representative_rules, set_active_representative_rules};
use rusqlite::ErrorCode;

const IO_RETRY_BACKOFF_MS: &[u64] = &[0, 100, 250, 500];
const FULL_REBUILD_PARSE_BATCH_SIZE: usize = 2048;
const FULL_REBUILD_PARSE_BATCH_SIZE_ENV: &str = "CODEATLAS_FULL_REBUILD_PARSE_BATCH_SIZE";
const LOG_DIAGNOSTICS_ENV: &str = "CODEATLAS_LOG_DIAGNOSTICS";

#[derive(Debug, Clone, Copy)]
struct MemorySnapshot {
    working_set_bytes: u64,
    private_bytes: u64,
}

#[derive(Debug, Default, Clone, Copy)]
struct IndexStageTimings {
    discovery_ms: u128,
    parse_ms: u128,
    resolve_ms: u128,
    persist_ms: u128,
    json_ms: u128,
    checkpoint_ms: u128,
}

#[derive(Debug, Default, Clone, Copy)]
struct ResolveBreakdownTimings {
    merge_symbols_ms: u128,
    resolve_calls_ms: u128,
    boundary_propagation_ms: u128,
    propagation_merge_ms: u128,
}

#[derive(Debug, Default, Clone, Copy)]
struct IncrementalStageTimings {
    initial_parse_ms: u128,
    cleanup_refresh_ms: u128,
    affected_reparse_ms: u128,
    resolve_calls_ms: u128,
    summary_load_ms: u128,
    boundary_propagation_ms: u128,
    propagation_merge_ms: u128,
    persist_ms: u128,
    commit_ms: u128,
}

#[derive(Debug, Clone)]
struct CliOptions {
    workspace_root: PathBuf,
    watch_mode: bool,
    requested_full_mode: bool,
    json_mode: bool,
    verbose_mode: bool,
    index_extensions: Option<Vec<String>>,
    parse_timeout_ms: Option<u64>,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        print_help();
        return;
    }

    let options = match parse_cli_args(&args) {
        Ok(options) => options,
        Err(error) => {
            eprintln!("{}", error);
            eprintln!("Try `codeatlas-indexer --help` for more information.");
            std::process::exit(1);
        }
    };
    let CliOptions {
        workspace_root,
        watch_mode,
        requested_full_mode,
        json_mode,
        verbose_mode,
        index_extensions,
        parse_timeout_ms,
    } = options;

    if !workspace_root.exists() {
        eprintln!("Directory not found: {}", workspace_root.display());
        std::process::exit(1);
    }

    if let Some(extensions) = index_extensions {
        std::env::set_var(INDEX_EXTENSIONS_ENV, extensions.join(","));
        println!("Index extensions: {}", extensions.join(", "));
    }

    if let Some(timeout_ms) = parse_timeout_ms {
        std::env::set_var(
            "CODEATLAS_CPP_PARSE_TIMEOUT_MICROS",
            timeout_ms.saturating_mul(1_000).to_string(),
        );
        println!("C/C++ parse timeout: {}ms", timeout_ms);
    }

    match load_workspace_representative_rules(&workspace_root) {
        Ok(config) => set_active_representative_rules(config),
        Err(error) => eprintln!("Warning: {}", error),
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
    let build_metadata = match build_metadata::load_build_metadata(&workspace_root) {
        Ok(metadata) => metadata,
        Err(err) => {
            eprintln!("Build metadata disabled: {}", err);
            None
        }
    };
    if let Some(metadata) = &build_metadata {
        println!(
            "Build metadata: {} translation unit(s), {} workspace include dir(s)",
            metadata.translation_unit_count,
            metadata.workspace_include_dirs.len()
        );
    }
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

        let registry = default_language_registry();
        let log_diagnostics = should_log_diagnostics(verbose_mode);
        let supported_languages = registry.supported_languages();
        let discovery_start = Instant::now();
        let discovered_files =
            discovery::find_source_files_with_feedback(&workspace_root, verbose_mode, &supported_languages);
        let discovery_elapsed = discovery_start.elapsed().as_millis();
        if log_diagnostics {
            print_memory_snapshot("after discovery");
        }
        let all_relative: Vec<String> = discovered_files
            .iter()
            .map(|entry| make_relative(&workspace_root, &entry.path))
            .collect();
        println!("Found {} source files", all_relative.len());

        if effective_full_mode {
            println!("Mode: full rebuild");
            let mut timings =
                run_full(
                    &db,
                    &workspace_root,
                    &discovered_files,
                    json_mode,
                    verbose_mode,
                    log_diagnostics,
                    &data_dir,
                    build_metadata.as_ref(),
                );
            timings.discovery_ms = discovery_elapsed;
            let checkpoint_start = Instant::now();
            db.checkpoint().expect("Failed to checkpoint SQLite database");
            timings.checkpoint_ms = checkpoint_start.elapsed().as_millis();
            print_stage_timings(&timings, json_mode);
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
                run_full(
                    &db,
                    &workspace_root,
                    &discovered_files,
                    json_mode,
                    verbose_mode,
                    log_diagnostics,
                    &data_dir,
                    build_metadata.as_ref(),
                );
            } else {
                if plan.to_index.is_empty() && plan.to_delete.is_empty() {
                    let elapsed = start.elapsed();
                    println!("\nNothing to do.");
                    println!("Done in {}", format_elapsed(elapsed.as_millis()));
                    println!("  Output: {}", data_dir.display());
                    return;
                }

                if let Err(e) = run_incremental(&db, &workspace_root, &plan, verbose_mode, log_diagnostics, build_metadata.as_ref()) {
                    eprintln!("Incremental indexing failed: {}", e);
                    std::process::exit(1);
                }
            }

            let checkpoint_start = Instant::now();
            db.checkpoint().expect("Failed to checkpoint SQLite database");
            let checkpoint_ms = checkpoint_start.elapsed().as_millis();
            println!(
                "  Timings: discovery {} | checkpoint {}",
                format_elapsed(discovery_elapsed),
                format_elapsed(checkpoint_ms)
            );
        }

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

fn print_stage_timings(timings: &IndexStageTimings, json_mode: bool) {
    let mut parts = vec![
        format!("discovery {}", format_elapsed(timings.discovery_ms)),
        format!("parse {}", format_elapsed(timings.parse_ms)),
        format!("resolve {}", format_elapsed(timings.resolve_ms)),
        format!("persist {}", format_elapsed(timings.persist_ms)),
    ];

    if json_mode || timings.json_ms > 0 {
        parts.push(format!("json {}", format_elapsed(timings.json_ms)));
    }

    parts.push(format!("checkpoint {}", format_elapsed(timings.checkpoint_ms)));

    println!("  Timings: {}", parts.join(" | "));
}

fn accumulate_parse_metrics(total: &mut ParseMetrics, batch: &ParseMetrics) {
    total.tree_sitter_parse_ms += batch.tree_sitter_parse_ms;
    total.syntax_walk_ms += batch.syntax_walk_ms;
    total.local_propagation_ms += batch.local_propagation_ms;
    total.local_function_discovery_ms += batch.local_function_discovery_ms;
    total.local_owner_lookup_ms += batch.local_owner_lookup_ms;
    total.local_seed_ms += batch.local_seed_ms;
    total.local_event_walk_ms += batch.local_event_walk_ms;
    total.local_declaration_ms += batch.local_declaration_ms;
    total.local_expression_statement_ms += batch.local_expression_statement_ms;
    total.local_return_statement_ms += batch.local_return_statement_ms;
    total.local_nested_block_ms += batch.local_nested_block_ms;
    total.local_return_collection_ms += batch.local_return_collection_ms;
    total.graph_relation_ms += batch.graph_relation_ms;
    total.graph_rule_compile_ms += batch.graph_rule_compile_ms;
    total.graph_rule_execute_ms += batch.graph_rule_execute_ms;
    total.reference_normalization_ms += batch.reference_normalization_ms;
}

fn format_memory_bytes(bytes: u64) -> String {
    const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
    const MIB: f64 = 1024.0 * 1024.0;
    if bytes >= GIB as u64 {
        format!("{:.2} GiB", bytes as f64 / GIB)
    } else {
        format!("{:.1} MiB", bytes as f64 / MIB)
    }
}

fn should_log_diagnostics(verbose: bool) -> bool {
    if verbose {
        return true;
    }

    match std::env::var(LOG_DIAGNOSTICS_ENV) {
        Ok(value) => matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
        Err(_) => false,
    }
}

fn configured_full_rebuild_parse_batch_size() -> usize {
    std::env::var(FULL_REBUILD_PARSE_BATCH_SIZE_ENV)
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|size| *size > 0)
        .unwrap_or(FULL_REBUILD_PARSE_BATCH_SIZE)
}

fn print_memory_snapshot(label: &str) {
    if let Some(snapshot) = current_process_memory_snapshot() {
        println!(
            "  Memory: {} | working-set {} | private {}",
            label,
            format_memory_bytes(snapshot.working_set_bytes),
            format_memory_bytes(snapshot.private_bytes)
        );
    }
}

#[cfg(target_os = "windows")]
fn current_process_memory_snapshot() -> Option<MemorySnapshot> {
    use std::mem::{size_of, zeroed};
    use windows_sys::Win32::System::ProcessStatus::{
        GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;

    let process = unsafe { GetCurrentProcess() };
    let mut counters: PROCESS_MEMORY_COUNTERS_EX = unsafe { zeroed() };
    let ok = unsafe {
        GetProcessMemoryInfo(
            process,
            &mut counters as *mut PROCESS_MEMORY_COUNTERS_EX as *mut _,
            size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
        )
    };
    if ok == 0 {
        return None;
    }

    Some(MemorySnapshot {
        working_set_bytes: counters.WorkingSetSize as u64,
        private_bytes: counters.PrivateUsage as u64,
    })
}

#[cfg(not(target_os = "windows"))]
fn current_process_memory_snapshot() -> Option<MemorySnapshot> {
    None
}

fn print_resolve_breakdown(label: &str, timings: &ResolveBreakdownTimings) {
    println!(
        "  {}: merge-symbols {} | resolve-calls {} | boundary-propagation {} | propagation-merge {}",
        label,
        format_elapsed(timings.merge_symbols_ms),
        format_elapsed(timings.resolve_calls_ms),
        format_elapsed(timings.boundary_propagation_ms),
        format_elapsed(timings.propagation_merge_ms)
    );
}

fn print_incremental_timings(timings: &IncrementalStageTimings) {
    println!(
        "  Incremental timings: initial-parse {} | cleanup-refresh {} | affected-reparse {} | resolve-calls {} | summary-load {} | boundary-propagation {} | propagation-merge {} | persist {} | commit {}",
        format_elapsed(timings.initial_parse_ms),
        format_elapsed(timings.cleanup_refresh_ms),
        format_elapsed(timings.affected_reparse_ms),
        format_elapsed(timings.resolve_calls_ms),
        format_elapsed(timings.summary_load_ms),
        format_elapsed(timings.boundary_propagation_ms),
        format_elapsed(timings.propagation_merge_ms),
        format_elapsed(timings.persist_ms),
        format_elapsed(timings.commit_ms)
    );
}

fn print_parse_breakdown(label: &str, metrics: &ParseMetrics) {
    println!(
        "  {}: tree-sitter {} | syntax-walk {} | local-propagation {} | local-function-discovery {} | local-owner-lookup {} | local-seed {} | local-event-walk {} | local-declaration {} | local-expression {} | local-return {} | local-nested-block {} | local-return-collection {} | graph-relations {} | graph-compile {} | graph-execute {} | reference-normalization {}",
        label,
        format_elapsed(metrics.tree_sitter_parse_ms),
        format_elapsed(metrics.syntax_walk_ms),
        format_elapsed(metrics.local_propagation_ms),
        format_elapsed(metrics.local_function_discovery_ms),
        format_elapsed(metrics.local_owner_lookup_ms),
        format_elapsed(metrics.local_seed_ms),
        format_elapsed(metrics.local_event_walk_ms),
        format_elapsed(metrics.local_declaration_ms),
        format_elapsed(metrics.local_expression_statement_ms),
        format_elapsed(metrics.local_return_statement_ms),
        format_elapsed(metrics.local_nested_block_ms),
        format_elapsed(metrics.local_return_collection_ms),
        format_elapsed(metrics.graph_relation_ms),
        format_elapsed(metrics.graph_rule_compile_ms),
        format_elapsed(metrics.graph_rule_execute_ms),
        format_elapsed(metrics.reference_normalization_ms)
    );
}

fn print_help() {
    println!("CodeAtlas Indexer");
    println!();
    println!("Usage:");
    println!("  codeatlas-indexer <workspace-root>");
    println!("  codeatlas-indexer <workspace-root> --full");
    println!("  codeatlas-indexer <workspace-root> --full --json");
    println!("  codeatlas-indexer <workspace-root> --extensions cpp,h,hpp --parse-timeout-ms 60000");
    println!("  codeatlas-indexer watch <workspace-root>");
    println!("  codeatlas-indexer watch <workspace-root> --extensions cpp,h,hpp --parse-timeout-ms 60000");
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
    println!("  --extensions Comma-separated extension allowlist, e.g. cpp,h,hpp,py");
    println!("  --parse-timeout-ms  Per-file C/C++ parse timeout in milliseconds");
    println!("               Applies to watch mode too.");
    println!("  Diagnostics such as memory snapshots and parse/resolve breakdowns are");
    println!("  shown for `--verbose` or when `CODEATLAS_LOG_DIAGNOSTICS=1` is set.");
    println!("  -h, --help   Show this help message");
    println!();
    println!("Large-repository stack safety:");
    println!("  CodeAtlas uses a larger internal worker-thread stack by default for indexing.");
    println!("  Override with `CODEATLAS_INDEXER_STACK_BYTES=<bytes>` if a larger stack is needed.");
    println!("  If `CODEATLAS_INDEXER_STACK_BYTES` is unset, `RUST_MIN_STACK` is honored when present.");
    println!("  C/C++ files larger than 2097152 bytes are skipped by default to avoid");
    println!("  pathological embedded-data headers blocking indexing.");
    println!("  Override with `CODEATLAS_SKIP_CPP_LARGER_THAN_BYTES=<bytes>` or set it to `0` to disable.");
    println!();
    println!("Optional build metadata:");
    println!("  If a workspace contains `compile_commands.json`, CodeAtlas auto-detects it");
    println!("  and uses include directories, output paths, and cheap define hints to");
    println!("  refine metadata classification without requiring an LSP or compile DB.");
    println!();
    println!("Output:");
    println!("  The index is stored in <workspace-root>/.codeatlas/index.db");
    println!(
        "  Supported file extensions: {}",
        EXTENSIONS
            .iter()
            .map(|ext| format!(".{}", ext))
            .collect::<Vec<_>>()
            .join(", ")
    );
}

fn parse_cli_args(args: &[String]) -> Result<CliOptions, String> {
    let mut watch_mode = false;
    let mut requested_full_mode = false;
    let mut json_mode = false;
    let mut verbose_mode = false;
    let mut workspace_root: Option<PathBuf> = None;
    let mut index_extensions: Option<Vec<String>> = None;
    let mut parse_timeout_ms: Option<u64> = None;

    let mut index = 1usize;
    if args.get(1).map(|arg| arg.as_str()) == Some("watch") {
        watch_mode = true;
        index = 2;
    }

    while index < args.len() {
        let arg = &args[index];
        match arg.as_str() {
            "--full" => requested_full_mode = true,
            "--json" => json_mode = true,
            "--verbose" => verbose_mode = true,
            "--extensions" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| "Missing value for --extensions".to_string())?;
                index_extensions = Some(parse_cli_extensions(value)?);
            }
            "--parse-timeout-ms" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| "Missing value for --parse-timeout-ms".to_string())?;
                parse_timeout_ms = Some(parse_timeout_ms_arg(value)?);
            }
            _ if let Some(value) = arg.strip_prefix("--extensions=") => {
                index_extensions = Some(parse_cli_extensions(value)?);
            }
            _ if let Some(value) = arg.strip_prefix("--parse-timeout-ms=") => {
                parse_timeout_ms = Some(parse_timeout_ms_arg(value)?);
            }
            _ if arg.starts_with('-') => {
                return Err(format!("Unknown option: {}", arg));
            }
            _ => {
                if workspace_root.is_some() {
                    return Err(format!("Unexpected extra positional argument: {}", arg));
                }
                workspace_root = Some(PathBuf::from(arg));
            }
        }
        index += 1;
    }

    let workspace_root = workspace_root.ok_or_else(|| {
        "Usage: codeatlas-indexer [watch] <workspace-root> [--full] [--json]".to_string()
    })?;

    Ok(CliOptions {
        workspace_root,
        watch_mode,
        requested_full_mode,
        json_mode,
        verbose_mode,
        index_extensions,
        parse_timeout_ms,
    })
}

fn parse_cli_extensions(raw: &str) -> Result<Vec<String>, String> {
    let parsed = parse_extension_list(raw).ok_or_else(|| {
        format!(
            "Invalid --extensions value: {}. Supported extensions: {}",
            raw,
            EXTENSIONS.join(",")
        )
    })?;
    let mut values: Vec<String> = parsed.into_iter().collect();
    values.sort();
    Ok(values)
}

fn parse_timeout_ms_arg(raw: &str) -> Result<u64, String> {
    raw.parse::<u64>()
        .map_err(|_| format!("Invalid --parse-timeout-ms value: {}", raw))
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
    discovered_files: &[language::DiscoveredSourceFile],
    json_mode: bool,
    verbose: bool,
    log_diagnostics: bool,
    data_dir: &Path,
    build_metadata: Option<&build_metadata::BuildMetadataContext>,
) -> IndexStageTimings {
    let mut timings = IndexStageTimings::default();
    let mut resolve_breakdown = ResolveBreakdownTimings::default();
    let registry = default_language_registry();
    let batch_size = configured_full_rebuild_parse_batch_size();
    if log_diagnostics {
        print_memory_snapshot("full rebuild start");
    }

    println!("  Stage: parse files");
    let parse_start = Instant::now();
    let mut parse_metrics = ParseMetrics::default();
    db.begin().expect("Failed to begin full rebuild transaction");
    db.clear().expect("Failed to clear SQLite tables before full rebuild");
    for (batch_index, batch) in discovered_files
        .chunks(batch_size)
        .enumerate()
    {
        let batch_start = batch_index * batch_size;
        let (
            raw_symbols,
            raw_calls,
            normalized_references,
            local_propagation_events,
            callable_flow_summaries,
            file_records,
            batch_metrics,
        ) = parse_discovered_files_with_progress(
            workspace_root,
            batch,
            verbose,
            build_metadata,
            &registry,
            batch_start,
            discovered_files.len(),
        );
        db.write_raw_symbols(&raw_symbols)
            .expect("Failed to write batch raw symbols");
        db.write_raw_calls(&raw_calls)
            .expect("Failed to write batch raw calls");
        db.write_references(&normalized_references)
            .expect("Failed to write batch references");
        db.write_propagation_events(&local_propagation_events)
            .expect("Failed to write batch local propagation");
        db.write_callable_flow_summaries(&callable_flow_summaries, &raw_symbols)
            .expect("Failed to write batch callable summaries");
        db.write_files(&file_records)
            .expect("Failed to write batch file records");
        accumulate_parse_metrics(&mut parse_metrics, &batch_metrics);
        if !verbose {
            println!(
                "  Parse batch {}/{} committed ({} files)",
                batch_index + 1,
                discovered_files.len().div_ceil(batch_size),
                batch.len()
            );
            if log_diagnostics {
                print_memory_snapshot("after parse batch flush");
            }
        }
    }
    timings.parse_ms = parse_start.elapsed().as_millis();
    println!(
        "  Stage complete: parse files in {}",
        format_elapsed(timings.parse_ms)
    );
    if log_diagnostics {
        print_memory_snapshot("after parse files");
    }

    println!("  Stage: merge symbols");
    let resolve_start = Instant::now();
    let merge_symbols_start = Instant::now();
    let raw_symbols = db
        .read_all_raw_symbols()
        .expect("Failed to read staged raw symbols");
    let symbols = resolver::merge_symbols(&raw_symbols);
    drop(raw_symbols);
    db.write_symbols(&symbols)
        .expect("Failed to write merged representative symbols");
    db.rebuild_fts()
        .expect("Failed to rebuild symbols FTS");
    let symbol_count = symbols.len();
    let caller_parent_by_id: HashMap<String, String> = symbols
        .iter()
        .filter_map(|symbol| {
            symbol
                .parent_id
                .as_ref()
                .map(|parent_id| (symbol.id.clone(), parent_id.clone()))
        })
        .collect();
    let callee_name_by_id: HashMap<String, String> = symbols
        .iter()
        .map(|symbol| (symbol.id.clone(), symbol.name.clone()))
        .collect();
    let valid_symbol_ids: HashSet<String> = symbols.iter().map(|symbol| symbol.id.clone()).collect();
    let symbol_types: HashMap<String, String> = symbols
        .iter()
        .map(|symbol| (symbol.id.clone(), symbol.symbol_type.clone()))
        .collect();
    let mut staged_references = db
        .read_all_references()
        .expect("Failed to read staged references");
    let dropped_references =
        storage::filter_persistable_references(&mut staged_references, &valid_symbol_ids, &symbol_types);
    if dropped_references > 0 {
        db.replace_references(&staged_references)
            .expect("Failed to rewrite filtered references");
        if verbose {
            println!(
                "  FILTER: dropped {} unresolved reference(s)",
                dropped_references
            );
        }
    }
    let cleaned_references = db
        .cleanup_dangling_references()
        .expect("Failed to cleanup invalid references after symbol merge");
    if !cleaned_references.is_empty() && verbose {
        println!(
            "  FILTER: removed invalid references from {} file(s) after symbol merge",
            cleaned_references.len()
        );
    }
    resolve_breakdown.merge_symbols_ms = merge_symbols_start.elapsed().as_millis();
    println!(
        "  Stage complete: merge symbols in {}",
        format_elapsed(resolve_breakdown.merge_symbols_ms)
    );
    if log_diagnostics {
        print_memory_snapshot("after merge symbols");
    }

    println!("  Stage: resolve calls");
    let resolve_calls_start = Instant::now();
    let file_paths: Vec<String> = db
        .read_file_records()
        .expect("Failed to read staged file records for batching")
        .into_iter()
        .map(|record| record.path)
        .collect();
    for (batch_index, file_batch) in file_paths.chunks(batch_size).enumerate() {
        let raw_calls = db
            .read_raw_calls_for_paths(file_batch)
            .expect("Failed to read staged raw calls for resolve batch");
        let calls = resolver::resolve_calls(&raw_calls, &symbols);
        if !calls.is_empty() {
            db.write_calls(&calls)
                .expect("Failed to write resolved call batch");
        }
        if !verbose {
            println!(
                "  Resolve batch {}/{} committed ({} files, {} calls)",
                batch_index + 1,
                file_paths.len().div_ceil(batch_size),
                file_batch.len(),
                calls.len()
            );
        }
    }
    resolve_breakdown.resolve_calls_ms = resolve_calls_start.elapsed().as_millis();
    println!(
        "  Stage complete: resolve calls in {}",
        format_elapsed(resolve_breakdown.resolve_calls_ms)
    );
    if log_diagnostics {
        print_memory_snapshot("after resolve calls");
    }
    let symbols_for_json = if json_mode { Some(symbols.clone()) } else { None };
    drop(symbols);

    println!("  Stage: derive boundary propagation");
    let boundary_start = Instant::now();
    let mut propagation_keys = db
        .read_all_propagation_event_keys()
        .expect("Failed to read staged propagation keys");
    let mut appended_boundary_propagation = 0usize;
    for (batch_index, file_batch) in file_paths.chunks(batch_size).enumerate() {
        let raw_calls = db
            .read_raw_calls_for_paths(file_batch)
            .expect("Failed to read staged raw calls for boundary batch");
        let calls = db
            .read_calls_for_paths(file_batch)
            .expect("Failed to read resolved calls for boundary batch");
        let callee_ids: Vec<String> = calls.iter().map(|call| call.callee_id.clone()).collect();
        let callable_flow_summaries = db
            .read_callable_flow_summaries_for_ids(&callee_ids)
            .expect("Failed to read callable summaries for boundary batch");
        let boundary_propagation_events =
            resolver::derive_function_boundary_propagation_events_with_indexes(
            &raw_calls,
            &calls,
            &callable_flow_summaries,
            &caller_parent_by_id,
            &callee_name_by_id,
        );
        let mut unique_boundary_events = Vec::new();
        for event in boundary_propagation_events {
            let key = storage::propagation_event_storage_key(&event);
            if propagation_keys.insert(key) {
                unique_boundary_events.push(event);
            }
        }
        appended_boundary_propagation += unique_boundary_events.len();
        if !unique_boundary_events.is_empty() {
            db.write_propagation_events(&unique_boundary_events)
                .expect("Failed to write boundary propagation batch");
        }
        if !verbose {
            println!(
                "  Boundary batch {}/{} appended {} event(s)",
                batch_index + 1,
                file_paths.len().div_ceil(batch_size),
                unique_boundary_events.len()
            );
        }
    }
    resolve_breakdown.boundary_propagation_ms = boundary_start.elapsed().as_millis();
    println!(
        "  Stage complete: derive boundary propagation in {}",
        format_elapsed(resolve_breakdown.boundary_propagation_ms)
    );
    if log_diagnostics {
        print_memory_snapshot("after boundary propagation");
    }

    println!("  Stage: merge propagation");
    let propagation_merge_start = Instant::now();
    resolve_breakdown.propagation_merge_ms = propagation_merge_start.elapsed().as_millis();
    timings.resolve_ms = resolve_start.elapsed().as_millis();
    println!(
        "  Stage complete: merge propagation in {}",
        format_elapsed(resolve_breakdown.propagation_merge_ms)
    );
    println!(
        "  Propagation merge: appended {} boundary event(s)",
        appended_boundary_propagation
    );
    if log_diagnostics {
        print_memory_snapshot("after merge propagation");
    }

    let raw_count: usize = discovered_files.len();
    println!("  Stage: persist sqlite");
    let persist_start = Instant::now();
    db.clear_raw_calls()
        .expect("Failed to clear staged raw calls");
    timings.persist_ms = persist_start.elapsed().as_millis();
    println!(
        "  Stage complete: persist sqlite in {}",
        format_elapsed(timings.persist_ms)
    );
    if log_diagnostics {
        print_memory_snapshot("after persist sqlite");
    }

    if json_mode {
        println!("  Stage: write json");
        let json_start = Instant::now();
        let calls = db
            .read_all_calls()
            .expect("Failed to read persisted calls");
        let normalized_references = db
            .read_all_references()
            .expect("Failed to read staged references");
        let propagation_events = db
            .read_all_propagation_events()
            .expect("Failed to read persisted propagation events");
        let file_records = db
            .read_file_records()
            .expect("Failed to read staged file records");
        write_json(
            &data_dir.join("symbols.json"),
            symbols_for_json.as_ref().expect("symbols must be present for json mode"),
        );
        write_json(&data_dir.join("calls.json"), &calls);
        write_json(&data_dir.join("references.json"), &normalized_references);
        write_json(&data_dir.join("propagation.json"), &propagation_events);
        write_json(&data_dir.join("files.json"), &file_records);
        timings.json_ms = json_start.elapsed().as_millis();
        println!(
            "  Stage complete: write json in {}",
            format_elapsed(timings.json_ms)
        );
        if log_diagnostics {
            print_memory_snapshot("after write json");
        }
    }
    db.commit()
        .expect("Failed to commit full rebuild transaction");

    println!(
        "  Symbols: {} | Calls: {} | Propagation: {} | Files: {}",
        symbol_count,
        db.count_calls().unwrap_or(0),
        db.count_propagation_events().unwrap_or(0),
        raw_count
    );
    if log_diagnostics {
        print_parse_breakdown("Parse breakdown", &parse_metrics);
        print_resolve_breakdown("Resolve breakdown", &resolve_breakdown);
    }

    timings
}

fn run_incremental(
    db: &storage::Database,
    workspace_root: &Path,
    plan: &incremental::IncrementalPlan,
    verbose: bool,
    log_diagnostics: bool,
    build_metadata: Option<&build_metadata::BuildMetadataContext>,
) -> Result<(), String> {
    let mut timings = IncrementalStageTimings::default();
    if log_diagnostics {
        print_memory_snapshot("incremental start");
    }
    let initial_parse_start = Instant::now();
    let (
        parsed_symbols,
        new_raw_calls,
        new_references,
        new_local_propagation,
        new_callable_summaries,
        new_files,
        parse_metrics,
    ) = if !plan.to_index.is_empty() {
        parse_files_strict(workspace_root, &plan.to_index, verbose, build_metadata)?
    } else {
        (
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            ParseMetrics::default(),
        )
    };
    timings.initial_parse_ms = initial_parse_start.elapsed().as_millis();
    if log_diagnostics {
        print_memory_snapshot("after incremental initial parse");
    }
    if log_diagnostics && parse_metrics != ParseMetrics::default() {
        print_parse_breakdown("Incremental parse breakdown", &parse_metrics);
    }

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
        let cleanup_refresh_start = Instant::now();
        for path in &plan.to_delete {
            println!("  DELETE: {}", path);
            db.delete_calls_for_file(path)
                .map_err(|e| format!("Failed to delete calls for {}: {}", path, e))?;
            db.delete_references_for_file(path)
                .map_err(|e| format!("Failed to delete references for {}: {}", path, e))?;
            db.delete_propagation_for_file(path)
                .map_err(|e| format!("Failed to delete propagation for {}: {}", path, e))?;
            db.delete_callable_flow_summaries_for_file(path)
                .map_err(|e| format!("Failed to delete callable summaries for {}: {}", path, e))?;
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
            db.delete_propagation_for_file(path)
                .map_err(|e| format!("Failed to delete propagation for {}: {}", path, e))?;
            db.delete_callable_flow_summaries_for_file(path)
                .map_err(|e| format!("Failed to delete callable summaries for {}: {}", path, e))?;
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
        let valid_symbol_ids: HashSet<String> = db
            .read_all_symbol_ids()
            .map_err(|e| format!("Failed to read symbol ids: {}", e))?
            .into_iter()
            .collect();
        let symbol_types = db
            .read_all_symbol_types()
            .map_err(|e| format!("Failed to read symbol types: {}", e))?;

        let affected_calls = db
            .cleanup_dangling_calls()
            .map_err(|e| format!("Failed to cleanup dangling calls: {}", e))?;
        let affected_references = db
            .cleanup_dangling_references()
            .map_err(|e| format!("Failed to cleanup dangling references: {}", e))?;
        timings.cleanup_refresh_ms = cleanup_refresh_start.elapsed().as_millis();
        if log_diagnostics {
            print_memory_snapshot("after incremental cleanup/refresh");
        }

        let mut all_calls = new_raw_calls;
        let mut all_references = new_references;
        let mut all_local_propagation = new_local_propagation;
        let mut all_callable_summaries = new_callable_summaries;
        let plan_to_index_set: HashSet<&str> = plan.to_index.iter().map(|p| p.as_str()).collect();
        let mut files_to_reresolve: Vec<String> = plan.to_index.clone();

        let mut affected = affected_calls;
        for path in affected_references {
            if !affected.contains(&path) {
                affected.push(path);
            }
        }
        let affected_propagation = db
            .cleanup_dangling_propagation()
            .map_err(|e| format!("Failed to cleanup dangling propagation: {}", e))?;
        for path in affected_propagation {
            if !affected.contains(&path) {
                affected.push(path);
            }
        }

        let affected_reparse_start = Instant::now();
        for path in &affected {
            if !files_to_reresolve.contains(path) {
                files_to_reresolve.push(path.clone());
            }
            if plan_to_index_set.contains(path.as_str()) {
                continue;
            }

            let (result, _, lossy, skipped) = parse_file_strict(workspace_root, path, build_metadata)?;
            if skipped {
                println!("  SKIP: {}: oversized file", path);
                continue;
            }
            if verbose && lossy {
                println!("  LOSSY: {}: non-UTF8 bytes replaced during parsing", path);
            }
            db.delete_calls_for_file(path)
                .map_err(|e| format!("Failed to delete calls for {}: {}", path, e))?;
            db.delete_references_for_file(path)
                .map_err(|e| format!("Failed to delete references for {}: {}", path, e))?;
            db.delete_propagation_for_file(path)
                .map_err(|e| format!("Failed to delete propagation for {}: {}", path, e))?;
            all_calls.extend(result.raw_calls);
            all_references.extend(result.normalized_references);
            all_local_propagation.extend(result.propagation_events);
            all_callable_summaries.extend(result.callable_flow_summaries);
        }
        let dropped_references =
            storage::filter_persistable_references(&mut all_references, &valid_symbol_ids, &symbol_types);
        if dropped_references > 0 && verbose {
            println!(
                "  FILTER: dropped {} unresolved reference(s)",
                dropped_references
            );
        }
        timings.affected_reparse_ms = affected_reparse_start.elapsed().as_millis();
        if log_diagnostics {
            print_memory_snapshot("after incremental affected reparse");
        }

        let resolve_calls_start = Instant::now();
        let resolved = resolver::resolve_calls_with_db(&all_calls, &refreshed_symbols, db);
        timings.resolve_calls_ms = resolve_calls_start.elapsed().as_millis();
        if log_diagnostics {
            print_memory_snapshot("after incremental resolve calls");
        }

        let summary_load_start = Instant::now();
        let callable_summaries = merge_callable_summaries(
            &all_callable_summaries,
            &load_missing_callable_summaries(db, &resolved, &all_callable_summaries)
                .map_err(|e| format!("Failed to read callable summaries: {}", e))?,
        );
        timings.summary_load_ms = summary_load_start.elapsed().as_millis();
        if log_diagnostics {
            print_memory_snapshot("after incremental summary load");
        }

        let boundary_start = Instant::now();
        let boundary_propagation = resolver::derive_function_boundary_propagation_events(
            &all_calls,
            &resolved,
            &callable_summaries,
            &refreshed_symbols,
        );
        timings.boundary_propagation_ms = boundary_start.elapsed().as_millis();
        if log_diagnostics {
            print_memory_snapshot("after incremental boundary propagation");
        }

        let propagation_merge_start = Instant::now();
        let propagation_events =
            resolver::merge_propagation_events(&all_local_propagation, &boundary_propagation);
        timings.propagation_merge_ms = propagation_merge_start.elapsed().as_millis();
        if log_diagnostics {
            print_memory_snapshot("after incremental propagation merge");
        }

        let persist_start = Instant::now();
        if !resolved.is_empty() {
            db.write_calls(&resolved)
                .map_err(|e| format!("Failed to write calls: {}", e))?;
        }
        if !all_references.is_empty() {
            db.write_references(&all_references)
                .map_err(|e| format!("Failed to write references: {}", e))?;
        }
        if !propagation_events.is_empty() {
            db.write_propagation_events(&propagation_events)
                .map_err(|e| format!("Failed to write propagation: {}", e))?;
        }
        if !all_callable_summaries.is_empty() {
            db.write_callable_flow_summaries(&all_callable_summaries, &refreshed_symbols)
                .map_err(|e| format!("Failed to write callable summaries: {}", e))?;
        }

        if !new_files.is_empty() {
            db.write_files(&new_files)
                .map_err(|e| format!("Failed to write files: {}", e))?;
        }

        db.refresh_fts_for_symbol_ids(&affected_symbol_ids)
            .map_err(|e| format!("Failed to refresh FTS: {}", e))?;
        timings.persist_ms = persist_start.elapsed().as_millis();
        if log_diagnostics {
            print_memory_snapshot("after incremental persist");
        }

        let total_syms: i64 = db.count_symbols().unwrap_or(0);
        let total_calls: i64 = db.count_calls().unwrap_or(0);
        let total_references: i64 = db.count_references().unwrap_or(0);
        let total_propagation: i64 = db.count_propagation_events().unwrap_or(0);
        let total_files: i64 = db.count_files().unwrap_or(0);
        println!(
            "  Symbols: {} | Calls: {} | References: {} | Propagation: {} | Files: {} | Re-resolved: {} file(s)",
            total_syms, total_calls, total_references, total_propagation, total_files, files_to_reresolve.len(),
        );
        Ok(())
    })();

    if let Err(err) = tx_result {
        let _ = db.rollback();
        return Err(err);
    }

    let commit_start = Instant::now();
    db.commit()
        .map_err(|e| format!("Failed to commit transaction: {}", e))?;
    timings.commit_ms = commit_start.elapsed().as_millis();
    if log_diagnostics {
        print_memory_snapshot("after incremental commit");
        print_incremental_timings(&timings);
    }

    if plan.to_index.is_empty() && !plan.to_delete.is_empty() {
        println!("  Deleted {} file(s)", plan.to_delete.len());
    }
    Ok(())
}

fn write_json<T: serde::Serialize>(path: &Path, data: &T) {
    let json = serde_json::to_string_pretty(data).expect("Failed to serialize JSON");
    fs::write(path, json).expect("Failed to write JSON file");
}

fn load_missing_callable_summaries(
    db: &storage::Database,
    calls: &[models::Call],
    in_memory: &[models::CallableFlowSummary],
) -> rusqlite::Result<Vec<models::CallableFlowSummary>> {
    let existing: HashSet<&str> = in_memory
        .iter()
        .map(|summary| summary.callable_symbol_id.as_str())
        .collect();
    let mut missing: Vec<String> = calls
        .iter()
        .map(|call| call.callee_id.clone())
        .filter(|callee_id| !existing.contains(callee_id.as_str()))
        .collect();
    missing.sort();
    missing.dedup();
    db.read_callable_flow_summaries_for_ids(&missing)
}

fn merge_callable_summaries(
    primary: &[models::CallableFlowSummary],
    secondary: &[models::CallableFlowSummary],
) -> Vec<models::CallableFlowSummary> {
    let mut merged = primary.to_vec();
    let mut seen: HashSet<String> = merged
        .iter()
        .map(|summary| summary.callable_symbol_id.clone())
        .collect();
    for summary in secondary {
        if seen.insert(summary.callable_symbol_id.clone()) {
            merged.push(summary.clone());
        }
    }
    merged
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
        let registry = default_language_registry();
        let supported_languages = registry.supported_languages();
        discovery::find_source_files_with_feedback(workspace_root, false, &supported_languages)
            .iter()
            .map(|entry| make_relative(workspace_root, &entry.path))
            .collect()
    }

    fn full_index_fixture(workspace_root: &Path, db_path: &Path) {
        let data_dir = workspace_root.join(DATA_DIR_NAME);
        fs::create_dir_all(&data_dir).unwrap();
        let db = storage::Database::open(db_path).unwrap();
        let registry = default_language_registry();
        let supported_languages = registry.supported_languages();
        let discovered =
            discovery::find_source_files_with_feedback(workspace_root, false, &supported_languages);
        run_full(&db, workspace_root, &discovered, false, false, false, &data_dir, None);
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
        run_incremental(&db, workspace_root, &plan, false, false, None).unwrap();
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
    fn parse_cli_args_accepts_extensions_and_timeout() {
        let args = vec![
            "codeatlas-indexer".to_string(),
            "watch".to_string(),
            "workspace".to_string(),
            "--extensions=cpp,h,hpp".to_string(),
            "--parse-timeout-ms".to_string(),
            "60000".to_string(),
            "--verbose".to_string(),
        ];

        let parsed = parse_cli_args(&args).unwrap();

        assert!(parsed.watch_mode);
        assert!(parsed.verbose_mode);
        assert_eq!(parsed.workspace_root, PathBuf::from("workspace"));
        assert_eq!(parsed.parse_timeout_ms, Some(60_000));
        assert_eq!(
            parsed.index_extensions,
            Some(vec!["cpp".into(), "h".into(), "hpp".into()])
        );
    }

    #[test]
    fn parse_cli_args_rejects_unknown_extension() {
        let args = vec![
            "codeatlas-indexer".to_string(),
            "workspace".to_string(),
            "--extensions".to_string(),
            "cpp,md".to_string(),
        ];

        let err = parse_cli_args(&args).unwrap_err();
        assert!(err.contains("Invalid --extensions value"));
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
