use std::collections::HashSet;
use std::io;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};
use std::fs;

use notify::event::{CreateKind, ModifyKind, RemoveKind, RenameMode};
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use rusqlite::ErrorCode;

use crate::build_metadata;
use crate::constants::{DATA_DIR_NAME, DB_FILENAME, is_indexed_extension};
use crate::discovery;
use crate::indexing::{default_language_registry, make_relative, parse_file_strict, parse_files, parse_files_strict};
use crate::ignore::IgnoreRules;
use crate::incremental;
use crate::parser;
use crate::representative_rules::{load_workspace_representative_rules, set_active_representative_rules};
use crate::resolver;
use crate::storage::{self, Database};

use chrono::Utc;

const DEBOUNCE_MS: u64 = 500;
const BASE_WATCHER_BURST_THRESHOLD: usize = 64;
const WATCHER_BURST_WINDOW_MS: u64 = 5_000;
const BASE_WATCHER_BURST_WINDOW_THRESHOLD: usize = 96;
const IO_RETRY_BACKOFF_MS: &[u64] = &[0, 100, 250, 500];
const BASE_PROJECT_SIZE_WATCHER: usize = 5_000;

fn env_override_usize_watcher(var: &str, computed: usize) -> usize {
    match std::env::var(var) {
        Ok(val) => match val.parse::<usize>() {
            Ok(v) => v,
            Err(_) => computed,
        },
        Err(_) => computed,
    }
}

fn compute_watcher_burst_threshold(total_files: usize) -> usize {
    if total_files <= BASE_PROJECT_SIZE_WATCHER {
        return BASE_WATCHER_BURST_THRESHOLD;
    }
    let scale = ((total_files as f64) / (BASE_PROJECT_SIZE_WATCHER as f64)).sqrt();
    let computed = (BASE_WATCHER_BURST_THRESHOLD as f64 * scale).ceil() as usize;
    env_override_usize_watcher("CODEATLAS_BURST_THRESHOLD", computed)
}

fn compute_watcher_burst_window_threshold(total_files: usize) -> usize {
    if total_files <= BASE_PROJECT_SIZE_WATCHER {
        return BASE_WATCHER_BURST_WINDOW_THRESHOLD;
    }
    let scale = ((total_files as f64) / (BASE_PROJECT_SIZE_WATCHER as f64)).sqrt();
    let computed = (BASE_WATCHER_BURST_WINDOW_THRESHOLD as f64 * scale).ceil() as usize;
    env_override_usize_watcher("CODEATLAS_BURST_WINDOW_THRESHOLD", computed)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NormalizedWatchEvent {
    kind_label: &'static str,
    paths: Vec<PathBuf>,
}

#[derive(Debug, Default)]
struct WatchRunState {
    pending: HashSet<PathBuf>,
    in_flight: HashSet<PathBuf>,
    dirty_during_run: HashSet<PathBuf>,
    last_event: Option<Instant>,
    immediate_follow_up: bool,
}

#[derive(Debug, Default)]
struct BurstWindow {
    entries: Vec<(Instant, usize)>,
}

impl BurstWindow {
    fn record(&mut self, count: usize, now: Instant) -> usize {
        self.entries.push((now, count));
        self.prune(now);
        self.total()
    }

    fn total(&self) -> usize {
        self.entries.iter().map(|(_, count)| *count).sum()
    }

    fn prune(&mut self, now: Instant) {
        let window = Duration::from_millis(WATCHER_BURST_WINDOW_MS);
        self.entries
            .retain(|(timestamp, _)| now.saturating_duration_since(*timestamp) <= window);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WatcherBurstDecision {
    rebuild_from_scratch: bool,
    reason: Option<String>,
}

fn watcher_burst_decision(changed_count: usize, recent_window_total: usize, total_files: usize) -> WatcherBurstDecision {
    let burst_threshold = compute_watcher_burst_threshold(total_files);
    if changed_count >= burst_threshold {
        return WatcherBurstDecision {
            rebuild_from_scratch: true,
            reason: Some(format!(
                "watcher burst exceeded threshold ({} >= {} for {} files)",
                changed_count, burst_threshold, total_files
            )),
        };
    }

    let window_threshold = compute_watcher_burst_window_threshold(total_files);
    if recent_window_total >= window_threshold {
        return WatcherBurstDecision {
            rebuild_from_scratch: true,
            reason: Some(format!(
                "sustained watcher churn exceeded recent-window threshold ({} >= {} over {}ms for {} files)",
                recent_window_total,
                window_threshold,
                WATCHER_BURST_WINDOW_MS,
                total_files
            )),
        };
    }

    WatcherBurstDecision {
        rebuild_from_scratch: false,
        reason: None,
    }
}

impl WatchRunState {
    fn record_paths(&mut self, paths: Vec<PathBuf>, indexing_active: bool) {
        let target = if indexing_active {
            &mut self.dirty_during_run
        } else {
            &mut self.pending
        };
        for path in paths {
            target.insert(path);
        }
        self.last_event = Some(Instant::now());
    }

    fn take_ready_batch(&mut self, debounce: Duration) -> Option<Vec<PathBuf>> {
        if self.pending.is_empty() || !self.is_ready(debounce) {
            return None;
        }

        self.immediate_follow_up = false;
        self.in_flight = std::mem::take(&mut self.pending);
        let mut changed: Vec<PathBuf> = self.in_flight.iter().cloned().collect();
        changed.sort();
        Some(changed)
    }

    fn complete_batch(&mut self) -> usize {
        self.in_flight.clear();
        let dirty_count = self.dirty_during_run.len();
        if dirty_count > 0 {
            self.pending.extend(self.dirty_during_run.drain());
            self.immediate_follow_up = true;
        }
        dirty_count
    }

    fn is_ready(&self, debounce: Duration) -> bool {
        if self.immediate_follow_up {
            return true;
        }
        self.last_event
            .map(|last| last.elapsed() >= debounce)
            .unwrap_or(false)
    }
}

fn is_tracked(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(is_indexed_extension)
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

fn is_relevant_event_kind(kind: &EventKind) -> bool {
    matches!(
        kind,
        EventKind::Create(CreateKind::Any | CreateKind::File)
            | EventKind::Modify(ModifyKind::Any | ModifyKind::Data(_) | ModifyKind::Name(_))
            | EventKind::Remove(RemoveKind::Any | RemoveKind::File)
    )
}

fn event_kind_label(kind: &EventKind) -> &'static str {
    match kind {
        EventKind::Create(_) => "create",
        EventKind::Modify(ModifyKind::Name(RenameMode::Any | RenameMode::Both | RenameMode::From | RenameMode::To)) => "rename",
        EventKind::Modify(_) => "modify",
        EventKind::Remove(_) => "remove",
        _ => "other",
    }
}

fn normalize_watch_event(
    event: &Event,
    workspace_root: &Path,
    ignore_rules: &IgnoreRules,
) -> Option<NormalizedWatchEvent> {
    if !is_relevant_event_kind(&event.kind) {
        return None;
    }

    let mut normalized = HashSet::new();
    for path in &event.paths {
        if let Some(candidate) = normalize_event_path(path, workspace_root, ignore_rules) {
            normalized.insert(candidate);
        }
    }

    if normalized.is_empty() {
        return None;
    }

    let mut paths: Vec<PathBuf> = normalized.into_iter().collect();
    paths.sort();
    Some(NormalizedWatchEvent {
        kind_label: event_kind_label(&event.kind),
        paths,
    })
}

fn normalize_event_path(
    path: &Path,
    workspace_root: &Path,
    ignore_rules: &IgnoreRules,
) -> Option<PathBuf> {
    if !is_in_workspace(path, workspace_root) || is_in_codeatlas_dir(path) {
        return None;
    }

    let normalized = normalize_editor_temp_path(path);
    if is_ignored_path(&normalized, workspace_root, ignore_rules) || !is_tracked(&normalized) {
        return None;
    }

    Some(normalized)
}

fn normalize_editor_temp_path(path: &Path) -> PathBuf {
    let file_name = path.file_name().and_then(|name| name.to_str()).unwrap_or_default();
    let trimmed_name = strip_editor_temp_suffix(file_name);
    if trimmed_name == file_name {
        return path.to_path_buf();
    }

    path.with_file_name(trimmed_name)
}

fn strip_editor_temp_suffix(name: &str) -> &str {
    const TEMP_SUFFIXES: &[&str] = &[".__jb_tmp__", ".tmp", ".TMP", ".temp", ".swp", ".swx", "~"];

    for suffix in TEMP_SUFFIXES {
        if let Some(base) = name.strip_suffix(suffix) {
            if Path::new(base)
                .extension()
                .and_then(|ext| ext.to_str())
                .map(is_indexed_extension)
                .unwrap_or(false)
            {
                return base;
            }
        }
    }

    name
}

pub fn watch(workspace_root: &Path, workspace_name: &str, verbose: bool) -> Result<(), String> {
    let workspace_root = workspace_root
        .canonicalize()
        .map_err(|e| format!("Failed to resolve workspace root: {}", e))?;
    match load_workspace_representative_rules(&workspace_root) {
        Ok(config) => set_active_representative_rules(config),
        Err(error) => eprintln!("Warning: {}", error),
    }

    let data_dir = workspace_root.join(DATA_DIR_NAME);
    fs::create_dir_all(&data_dir).map_err(|e| format!("Failed to create data dir: {}", e))?;
    let expected_index_metadata = storage::expected_index_metadata(&workspace_root, workspace_name);

    let ignore_rules = IgnoreRules::load(&workspace_root);

    println!("Watch mode: {}", workspace_root.display());
    println!("Press Ctrl-C to stop.\n");

    let active_db_path = match storage::resolve_active_database_path(&data_dir) {
        Ok(path) => path,
        Err(issue) => {
            println!(
                "Existing index is unhealthy (active DB resolution failed: {}). Rebuilding from scratch...",
                issue
            );
            run_full_index(&workspace_root, workspace_name, &data_dir, verbose)?;
            storage::resolve_active_database_path(&data_dir)?
        }
    };

    match active_db_path.as_deref() {
        Some(db_path) => match storage::validate_existing_database(db_path) {
            Ok(()) => {
                if let Some(reason) = storage::existing_database_metadata_issue(db_path, &expected_index_metadata)? {
                    println!("Existing index is outdated ({}). Rebuilding from scratch...", reason);
                    run_full_index(&workspace_root, workspace_name, &data_dir, verbose)?;
                } else {
                    let db = open_database_with_retry(db_path, "watch open sqlite database")?;
                    if db.has_data() {
                        println!("Existing index found, running incremental catch-up...");
                        run_incremental_index(&workspace_root, workspace_name, &data_dir, verbose, None)?;
                    } else {
                        println!("No existing index, running full initial index...");
                        run_full_index(&workspace_root, workspace_name, &data_dir, verbose)?;
                    }
                }
            }
            Err(issue) => {
                println!("Existing index is unhealthy ({}). Rebuilding from scratch...", issue);
                run_full_index(&workspace_root, workspace_name, &data_dir, verbose)?;
            }
        },
        None => {
            println!("No existing index, running full initial index...");
            run_full_index(&workspace_root, workspace_name, &data_dir, verbose)?;
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

    let mut state = WatchRunState::default();
    let debounce = Duration::from_millis(DEBOUNCE_MS);
    let mut burst_window = BurstWindow::default();

    let mut cached_total_files: usize = storage::resolve_active_database_path(&data_dir)
        .ok()
        .flatten()
        .and_then(|db_path| open_database_with_retry(&db_path, "watch read total_files").ok())
        .and_then(|db| db.count_files().ok())
        .map(|n| n as usize)
        .unwrap_or(0);

    loop {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(event) => {
                if let Some(normalized) = normalize_watch_event(&event, &workspace_root, &ignore_rules) {
                    if verbose {
                        let joined = normalized
                            .paths
                            .iter()
                            .map(|path| {
                                path.strip_prefix(&workspace_root)
                                    .unwrap_or(path)
                                    .to_string_lossy()
                                    .replace('\\', "/")
                            })
                            .collect::<Vec<_>>()
                            .join(", ");
                        println!("  WATCH: {} [{}]", normalized.kind_label, joined);
                    }
                    let indexing_active = !state.in_flight.is_empty();
                    state.record_paths(normalized.paths, indexing_active);
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }

        if let Some(changed) = state.take_ready_batch(debounce) {
            let now = Instant::now();
            let recent_window_total = burst_window.record(changed.len(), now);
            println!(
                "[{}] {} file(s) changed, re-indexing...",
                Utc::now().format("%H:%M:%S"),
                changed.len()
            );
            let burst_decision = watcher_burst_decision(changed.len(), recent_window_total, cached_total_files);

            let result = if burst_decision.rebuild_from_scratch {
                if let Some(reason) = &burst_decision.reason {
                    println!("  Escalation: {}, rebuilding from scratch", reason);
                }
                let r = run_full_index(&workspace_root, workspace_name, &data_dir, verbose);
                if r.is_ok() {
                    cached_total_files = storage::resolve_active_database_path(&data_dir)
                        .ok()
                        .flatten()
                        .and_then(|db_path| open_database_with_retry(&db_path, "watch refresh total_files").ok())
                        .and_then(|db| db.count_files().ok())
                        .map(|n| n as usize)
                        .unwrap_or(cached_total_files);
                }
                r
            } else {
                run_incremental_index(&workspace_root, workspace_name, &data_dir, verbose, Some(&changed))
            };

            if let Err(e) = result {
                eprintln!("  Indexing error: {}", e);
            }

            drain_queued_events(
                &rx,
                &workspace_root,
                &ignore_rules,
                verbose,
                &mut state,
            );

            let dirty_count = state.complete_batch();
            if dirty_count > 0 {
                println!(
                    "  Watch follow-up: {} file(s) changed during indexing, scheduling compressed rerun",
                    dirty_count
                );
            }
        }
    }

    Ok(())
}

fn drain_queued_events(
    rx: &mpsc::Receiver<Event>,
    workspace_root: &Path,
    ignore_rules: &IgnoreRules,
    verbose: bool,
    state: &mut WatchRunState,
) {
    while let Ok(event) = rx.try_recv() {
        if let Some(normalized) = normalize_watch_event(&event, workspace_root, ignore_rules) {
            if verbose {
                let joined = normalized
                    .paths
                    .iter()
                    .map(|path| {
                        path.strip_prefix(workspace_root)
                            .unwrap_or(path)
                            .to_string_lossy()
                            .replace('\\', "/")
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                println!("  WATCH: {} [{}]", normalized.kind_label, joined);
            }
            state.record_paths(normalized.paths, true);
        }
    }
}

fn run_full_index(
    workspace_root: &Path,
    workspace_name: &str,
    data_dir: &Path,
    verbose: bool,
) -> Result<(), String> {
    let build_metadata = match build_metadata::load_build_metadata(workspace_root) {
        Ok(metadata) => metadata,
        Err(err) => {
            eprintln!("Build metadata disabled in watch mode: {}", err);
            None
        }
    };
    let expected_index_metadata = storage::expected_index_metadata(workspace_root, workspace_name);
    let staging_db_path = make_watch_staging_db_path(data_dir, "full");
    if staging_db_path.exists() {
        retry_io("watch staging cleanup", || fs::remove_file(&staging_db_path))
            .map_err(|e| format!("{}", e))?;
    }

    let db = open_database_with_retry(&staging_db_path, "watch open staging sqlite database")?;
    let registry = default_language_registry();
    let supported_languages = registry.supported_languages();
    let files = discovery::find_source_files_with_feedback(workspace_root, verbose, &supported_languages);
    let all_relative: Vec<String> = files
        .iter()
        .map(|entry| make_relative(workspace_root, &entry.path))
        .collect();

    println!("Initial index: {} files", all_relative.len());
    let start = Instant::now();

    let (
        raw_symbols,
        raw_calls,
        relation_events,
        local_propagation_events,
        callable_flow_summaries,
        file_records,
        _parse_metrics,
    ) = parse_files(workspace_root, &all_relative, verbose, build_metadata.as_ref());
    let symbols = resolver::merge_symbols(&raw_symbols);
    let calls = resolver::resolve_calls(&raw_calls, &symbols);
    let normalized_references = parser::normalize_relation_events(&relation_events, &symbols);
    let boundary_propagation_events = resolver::derive_function_boundary_propagation_events(
        &raw_calls,
        &calls,
        &callable_flow_summaries,
        &symbols,
    );
    let propagation_events = resolver::merge_propagation_events(
        &local_propagation_events,
        &boundary_propagation_events,
    );
    db.write_all(
        &raw_symbols,
        &symbols,
        &calls,
        &normalized_references,
        &propagation_events,
        &callable_flow_summaries,
        &file_records,
    )
        .map_err(|e| format!("DB write: {}", e))?;
    db.write_index_metadata(&expected_index_metadata)
        .map_err(|e| format!("DB write metadata: {}", e))?;
    db.checkpoint().map_err(|e| format!("DB checkpoint: {}", e))?;

    publish_watch_db(&staging_db_path, data_dir)?;

    println!(
        "  Done in {}ms: {} symbols, {} calls",
        start.elapsed().as_millis(),
        symbols.len(),
        calls.len()
    );
    Ok(())
}

fn publish_watch_db(staging_db_path: &Path, data_dir: &Path) -> Result<(), String> {
    let previous_active_filename = storage::read_active_db_pointer(data_dir)
        .ok()
        .flatten()
        .map(|pointer| pointer.active_db_filename);
    let generation_filename = storage::create_versioned_db_generation_filename();
    let publish_path = data_dir.join(&generation_filename);
    retry_io("watch staging copy", || fs::copy(staging_db_path, &publish_path).map(|_| ()))
        .map_err(|e| format!("{}", e))?;
    let pointer = storage::ActiveDbPointer {
        active_db_filename: generation_filename,
        published_at: chrono::Utc::now().to_rfc3339(),
        format_version: storage::current_index_format_version(),
    };
    retry_io("watch active db pointer update", || {
        storage::write_active_db_pointer(data_dir, &pointer)
    })
        .map_err(|e| format!("{}", e))?;
    let _ = storage::cleanup_inactive_generations(
        data_dir,
        previous_active_filename.as_deref(),
        1,
    );
    retry_io("watch staging remove", || fs::remove_file(staging_db_path))
        .map_err(|e| format!("{}", e))?;
    Ok(())
}

fn make_watch_staging_db_path(data_dir: &Path, tag: &str) -> PathBuf {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    data_dir.hash(&mut hasher);
    let key = hasher.finish();
    std::env::temp_dir().join(format!("codeatlas-watch-{}-{}-{}.db", tag, key, DB_FILENAME))
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

fn open_database_with_retry(path: &Path, operation: &str) -> Result<Database, String> {
    let mut last_error = None;

    for (attempt, delay_ms) in IO_RETRY_BACKOFF_MS.iter().enumerate() {
        if *delay_ms > 0 {
            thread::sleep(Duration::from_millis(*delay_ms));
        }

        match Database::open(path) {
            Ok(db) => return Ok(db),
            Err(err) if should_retry_sqlite_open(&err) && attempt + 1 < IO_RETRY_BACKOFF_MS.len() => {
                last_error = Some(err.to_string());
            }
            Err(err) => return Err(format!("{} failed: {}", operation, err)),
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

fn load_missing_callable_summaries(
    db: &Database,
    calls: &[crate::models::Call],
    in_memory: &[crate::models::CallableFlowSummary],
) -> rusqlite::Result<Vec<crate::models::CallableFlowSummary>> {
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
    primary: &[crate::models::CallableFlowSummary],
    secondary: &[crate::models::CallableFlowSummary],
) -> Vec<crate::models::CallableFlowSummary> {
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

fn run_incremental_index(
    workspace_root: &Path,
    workspace_name: &str,
    data_dir: &Path,
    verbose: bool,
    changed_paths: Option<&[PathBuf]>,
) -> Result<(), String> {
    let build_metadata = match build_metadata::load_build_metadata(workspace_root) {
        Ok(metadata) => metadata,
        Err(err) => {
            eprintln!("Build metadata disabled in watch mode: {}", err);
            None
        }
    };
    let expected_index_metadata = storage::expected_index_metadata(workspace_root, workspace_name);
    let db_path = storage::resolve_active_database_path(data_dir)?
        .ok_or_else(|| "no active database available for watch incremental update".to_string())?;
    let db = open_database_with_retry(&db_path, "watch open incremental sqlite database")?;
    let stored = db.read_file_records().unwrap_or_default();
    let (plan, total_files) = if let Some(changed_paths) = changed_paths {
        let changed_relative: Vec<String> = changed_paths
            .iter()
            .map(|path| make_relative(workspace_root, path))
            .collect();
        match incremental::plan_from_changed_paths(&changed_relative, &stored, workspace_root, Some(&db)) {
            incremental::ChangedSetPlanResult::Narrow(plan) => {
                if verbose {
                    println!(
                        "  Planner: changed-set mode ({} tracked path(s))",
                        changed_relative.len()
                    );
                }
                (plan, stored.len())
            }
            incremental::ChangedSetPlanResult::RequiresFullDiscovery { reason } => {
                println!("  Planner fallback: {}", reason);
                let registry = default_language_registry();
                let supported_languages = registry.supported_languages();
                let files = discovery::find_source_files_with_feedback(
                    workspace_root,
                    verbose,
                    &supported_languages,
                );
                let all_relative: Vec<String> = files
                    .iter()
                    .map(|entry| make_relative(workspace_root, &entry.path))
                    .collect();
                let total_files = all_relative.len();
                (incremental::plan(&all_relative, &stored, workspace_root), total_files)
            }
        }
    } else {
        let registry = default_language_registry();
        let supported_languages = registry.supported_languages();
        let files = discovery::find_source_files_with_feedback(workspace_root, verbose, &supported_languages);
        let all_relative: Vec<String> = files
            .iter()
            .map(|entry| make_relative(workspace_root, &entry.path))
            .collect();
        let total_files = all_relative.len();
        (incremental::plan(&all_relative, &stored, workspace_root), total_files)
    };
    let escalation = incremental::assess_escalation(total_files, &plan);

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

    if let Some(reason) = &escalation.reason {
        println!("  Escalation: {}", reason);
    }

    if plan.to_index.is_empty() && plan.to_delete.is_empty() {
        println!("  No effective changes.");
        return Ok(());
    }

    if escalation.level == incremental::EscalationLevel::FullRebuild {
        println!("  Escalation action: running full rebuild");
        return run_full_index(workspace_root, workspace_name, data_dir, verbose);
    }

    let start = Instant::now();

    let (
        parsed_symbols,
        new_raw_calls,
        new_relation_events,
        new_local_propagation,
        new_callable_summaries,
        new_files,
        _parse_metrics,
    ) = if !plan.to_index.is_empty() {
        parse_files_strict(workspace_root, &plan.to_index, verbose, build_metadata.as_ref())?
    } else {
        (
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            crate::models::ParseMetrics::default(),
        )
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
            db.delete_references_for_file(path)
                .map_err(|e| format!("DB delete references: {}", e))?;
            db.delete_propagation_for_file(path)
                .map_err(|e| format!("DB delete propagation: {}", e))?;
            db.delete_callable_flow_summaries_for_file(path)
                .map_err(|e| format!("DB delete callable summaries: {}", e))?;
            db.delete_raw_symbols_for_file(path)
                .map_err(|e| format!("DB delete raw symbols: {}", e))?;
            db.delete_file_record(path)
                .map_err(|e| format!("DB delete file record: {}", e))?;
        }
        for path in &plan.to_index {
            db.delete_calls_for_file(path)
                .map_err(|e| format!("DB delete calls: {}", e))?;
            db.delete_references_for_file(path)
                .map_err(|e| format!("DB delete references: {}", e))?;
            db.delete_propagation_for_file(path)
                .map_err(|e| format!("DB delete propagation: {}", e))?;
            db.delete_callable_flow_summaries_for_file(path)
                .map_err(|e| format!("DB delete callable summaries: {}", e))?;
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
        let valid_symbol_ids: HashSet<String> = db
            .read_all_symbol_ids()
            .map_err(|e| format!("DB read symbol ids: {}", e))?
            .into_iter()
            .collect();
        let symbol_types = db
            .read_all_symbol_types()
            .map_err(|e| format!("DB read symbol types: {}", e))?;

        let affected_calls = db.cleanup_dangling_calls()
            .map_err(|e| format!("DB cleanup: {}", e))?;
        let affected_references = db.cleanup_dangling_references()
            .map_err(|e| format!("DB cleanup references: {}", e))?;

        let mut all_raw_calls = new_raw_calls;
        let mut all_relation_events = new_relation_events;
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
        let affected_propagation = db.cleanup_dangling_propagation()
            .map_err(|e| format!("DB cleanup propagation: {}", e))?;
        for path in affected_propagation {
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

            let (result, _, lossy, skipped) = parse_file_strict(workspace_root, path, build_metadata.as_ref())?;
            if skipped {
                println!("  SKIP: {}: oversized file", path);
                continue;
            }
            if verbose && lossy {
                println!("  LOSSY: {}: non-UTF8 bytes replaced during parsing", path);
            }
            db.delete_calls_for_file(path)
                .map_err(|e| format!("DB delete calls: {}", e))?;
            db.delete_references_for_file(path)
                .map_err(|e| format!("DB delete references: {}", e))?;
            db.delete_propagation_for_file(path)
                .map_err(|e| format!("DB delete propagation: {}", e))?;
            all_raw_calls.extend(result.raw_calls);
            all_relation_events.extend(
                result
                    .relation_events
                    .into_iter()
                    .filter(|event| event.relation_kind != crate::models::RawRelationKind::Call),
            );
            all_local_propagation.extend(result.propagation_events);
            all_callable_summaries.extend(result.callable_flow_summaries);
        }
        let mut all_references =
            parser::normalize_relation_events(&all_relation_events, &refreshed_symbols);
        let dropped_references =
            storage::filter_persistable_references(&mut all_references, &valid_symbol_ids, &symbol_types);
        if dropped_references > 0 && verbose {
            println!(
                "  FILTER: dropped {} unresolved reference(s)",
                dropped_references
            );
        }

        let resolved = resolver::resolve_calls_with_db(&all_raw_calls, &refreshed_symbols, &db);
        let callable_summaries = merge_callable_summaries(
            &all_callable_summaries,
            &load_missing_callable_summaries(&db, &resolved, &all_callable_summaries)
                .map_err(|e| format!("DB read callable summaries: {}", e))?,
        );
        let boundary_propagation = resolver::derive_function_boundary_propagation_events(
            &all_raw_calls,
            &resolved,
            &callable_summaries,
            &refreshed_symbols,
        );
        let propagation_events =
            resolver::merge_propagation_events(&all_local_propagation, &boundary_propagation);
        if !resolved.is_empty() {
            db.write_calls(&resolved)
                .map_err(|e| format!("DB write calls: {}", e))?;
        }
        if !all_references.is_empty() {
            db.write_references(&all_references)
                .map_err(|e| format!("DB write references: {}", e))?;
        }
        if !propagation_events.is_empty() {
            db.write_propagation_events(&propagation_events)
                .map_err(|e| format!("DB write propagation: {}", e))?;
        }
        if !all_callable_summaries.is_empty() {
            db.write_callable_flow_summaries(&all_callable_summaries, &refreshed_symbols)
                .map_err(|e| format!("DB write callable summaries: {}", e))?;
        }
        if !new_files.is_empty() {
            db.write_files(&new_files)
                .map_err(|e| format!("DB write files: {}", e))?;
        }
        db.refresh_fts_for_symbol_ids(&affected_symbol_ids)
            .map_err(|e| format!("DB refresh FTS: {}", e))?;

        let total_syms = db.count_symbols().unwrap_or(0);
        let total_calls = db.count_calls().unwrap_or(0);
        let total_references = db.count_references().unwrap_or(0);
        let total_propagation = db.count_propagation_events().unwrap_or(0);
        let total_files = db.count_files().unwrap_or(0);
        println!(
            "  Done in {}ms: {} symbols, {} calls, {} references, {} propagation, {} files, {} file(s) re-resolved",
            start.elapsed().as_millis(),
            total_syms,
            total_calls,
            total_references,
            total_propagation,
            total_files,
            files_to_reresolve.len()
        );
        Ok(())
    })();

    if let Err(err) = tx_result {
        let _ = db.rollback();
        return Err(err);
    }

    db.write_index_metadata(&expected_index_metadata)
        .map_err(|e| format!("DB write metadata: {}", e))?;
    db.commit().map_err(|e| format!("DB commit: {}", e))?;

    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::mpsc;

    #[test]
    fn is_tracked_accepts_cpp_extensions() {
        assert!(is_tracked(Path::new("foo.c")));
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

    #[test]
    fn strip_editor_temp_suffix_normalizes_common_save_patterns() {
        assert_eq!(strip_editor_temp_suffix("foo.cpp.__jb_tmp__"), "foo.cpp");
        assert_eq!(strip_editor_temp_suffix("foo.cpp.tmp"), "foo.cpp");
        assert_eq!(strip_editor_temp_suffix("foo.cpp~"), "foo.cpp");
        assert_eq!(strip_editor_temp_suffix("foo.tmp"), "foo.tmp");
    }

    #[test]
    fn normalize_watch_event_maps_temp_replacement_to_real_source_path() {
        let root = PathBuf::from("/project");
        let ignore_rules = IgnoreRules::from_patterns(vec![]);
        let event = Event {
            kind: EventKind::Modify(ModifyKind::Name(RenameMode::Both)),
            paths: vec![
                root.join("src").join("foo.cpp.__jb_tmp__"),
                root.join("src").join("foo.cpp"),
            ],
            attrs: Default::default(),
        };

        let normalized = normalize_watch_event(&event, &root, &ignore_rules).unwrap();
        assert_eq!(normalized.kind_label, "rename");
        assert_eq!(normalized.paths, vec![root.join("src").join("foo.cpp")]);
    }

    #[test]
    fn normalize_watch_event_filters_ignored_and_codeatlas_paths() {
        let root = PathBuf::from("/project");
        let ignore_rules = IgnoreRules::from_patterns(vec![regex::Regex::new(r"^generated/").unwrap()]);
        let event = Event {
            kind: EventKind::Modify(ModifyKind::Data(notify::event::DataChange::Any)),
            paths: vec![
                root.join(".codeatlas").join("index.db"),
                root.join("generated").join("skip.cpp"),
                root.join("src").join("keep.cpp"),
            ],
            attrs: Default::default(),
        };

        let normalized = normalize_watch_event(&event, &root, &ignore_rules).unwrap();
        assert_eq!(normalized.kind_label, "modify");
        assert_eq!(normalized.paths, vec![root.join("src").join("keep.cpp")]);
    }

    #[test]
    fn retry_io_retries_permission_denied_then_succeeds() {
        use std::cell::Cell;

        let attempts = Cell::new(0usize);
        let result = retry_io("watch retry test", || {
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

    #[test]
    fn publish_watch_db_creates_versioned_generation_and_pointer() {
        let temp = tempfile::tempdir().unwrap();
        let data_dir = temp.path().join(DATA_DIR_NAME);
        std::fs::create_dir_all(&data_dir).unwrap();
        let staging_db_path = temp.path().join("watch-staging.db");
        std::fs::write(&staging_db_path, b"sqlite placeholder").unwrap();

        publish_watch_db(&staging_db_path, &data_dir).unwrap();

        let pointer = storage::read_active_db_pointer(&data_dir).unwrap().unwrap();
        let published = data_dir.join(&pointer.active_db_filename);
        assert!(published.exists());
        assert!(pointer.active_db_filename.starts_with("index-"));
        let resolved = storage::resolve_active_database_path(&data_dir).unwrap().unwrap();
        assert_eq!(resolved, published);
        assert!(!staging_db_path.exists());
    }

    #[test]
    fn watch_run_state_coalesces_repeated_paths_during_active_run() {
        let debounce = Duration::from_millis(500);
        let first = PathBuf::from("/project/src/foo.cpp");
        let second = PathBuf::from("/project/src/bar.cpp");
        let mut state = WatchRunState::default();

        state.record_paths(vec![first.clone(), second.clone()], false);
        state.immediate_follow_up = true;
        let batch = state.take_ready_batch(debounce).unwrap();
        assert_eq!(batch, vec![second.clone(), first.clone()]);
        assert!(state.pending.is_empty());
        assert_eq!(state.in_flight.len(), 2);

        state.record_paths(vec![first.clone(), first.clone()], true);
        state.record_paths(vec![second.clone()], true);
        let dirty_count = state.complete_batch();

        assert_eq!(dirty_count, 2);
        assert!(state.in_flight.is_empty());
        assert!(state.immediate_follow_up);
        assert_eq!(state.pending.len(), 2);
        assert!(state.pending.contains(&first));
        assert!(state.pending.contains(&second));
    }

    #[test]
    fn watch_run_state_immediate_follow_up_bypasses_debounce() {
        let path = PathBuf::from("/project/src/foo.cpp");
        let mut state = WatchRunState::default();

        state.record_paths(vec![path.clone()], false);
        assert!(!state.is_ready(Duration::from_secs(60)));

        state.immediate_follow_up = true;
        let batch = state.take_ready_batch(Duration::from_secs(60)).unwrap();

        assert_eq!(batch, vec![path]);
    }

    #[test]
    fn burst_window_prunes_old_entries() {
        let mut window = BurstWindow::default();
        let base = Instant::now();

        assert_eq!(window.record(10, base), 10);
        assert_eq!(
            window.record(5, base + Duration::from_millis(WATCHER_BURST_WINDOW_MS + 1)),
            5
        );
    }

    #[test]
    fn watcher_burst_decision_rebuilds_for_sustained_recent_churn() {
        let decision = watcher_burst_decision(20, BASE_WATCHER_BURST_WINDOW_THRESHOLD, 0);
        assert!(decision.rebuild_from_scratch);
        assert!(decision
            .reason
            .unwrap()
            .contains("sustained watcher churn exceeded recent-window threshold"));
    }

    #[test]
    fn compute_watcher_burst_threshold_uses_base_for_small_projects() {
        assert_eq!(compute_watcher_burst_threshold(0), 64);
        assert_eq!(compute_watcher_burst_threshold(5000), 64);
    }

    #[test]
    fn compute_watcher_burst_threshold_scales_for_large_projects() {
        // 35K: 64 * sqrt(35000/5000) ≈ 169.3 → ceil = 170
        let t35k = compute_watcher_burst_threshold(35_000);
        assert!(t35k >= 169 && t35k <= 171, "35K burst threshold was {}", t35k);
    }

    #[test]
    fn watcher_burst_decision_stays_incremental_for_large_project_100_changes() {
        // 100 changes in a 35K project should stay incremental (burst threshold ~170)
        let decision = watcher_burst_decision(100, 100, 35_000);
        assert!(!decision.rebuild_from_scratch);
    }

    #[test]
    fn watcher_burst_decision_triggers_rebuild_for_small_project_at_base_threshold() {
        // BASE_WATCHER_BURST_THRESHOLD (64) changes in a 5K project should rebuild
        let decision = watcher_burst_decision(BASE_WATCHER_BURST_THRESHOLD, 0, 5_000);
        assert!(decision.rebuild_from_scratch);
        assert!(decision.reason.unwrap().contains("watcher burst exceeded threshold"));
    }

    #[test]
    fn watcher_burst_decision_reason_includes_project_size() {
        let decision = watcher_burst_decision(200, 0, 35_000);
        assert!(decision.rebuild_from_scratch);
        let reason = decision.reason.unwrap();
        assert!(reason.contains("35000"), "reason missing project size: {}", reason);
    }

    #[test]
    fn drain_queued_events_compresses_same_path_burst_into_dirty_set() {
        let root = PathBuf::from("/project");
        let ignore_rules = IgnoreRules::from_patterns(vec![]);
        let (tx, rx) = mpsc::channel();
        let mut state = WatchRunState::default();
        let tracked = root.join("src").join("foo.cpp");

        state.record_paths(vec![tracked.clone()], false);
        state.immediate_follow_up = true;
        assert!(state.take_ready_batch(Duration::from_millis(1)).is_some());

        let modify = Event {
            kind: EventKind::Modify(ModifyKind::Data(notify::event::DataChange::Any)),
            paths: vec![tracked.clone()],
            attrs: Default::default(),
        };
        tx.send(modify.clone()).unwrap();
        tx.send(modify).unwrap();

        drain_queued_events(&rx, &root, &ignore_rules, false, &mut state);

        assert_eq!(state.dirty_during_run.len(), 1);
        assert!(state.dirty_during_run.contains(&tracked));
    }
}
