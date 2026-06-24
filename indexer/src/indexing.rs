use std::fs;
use std::path::Path;
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use chrono::Utc;
use rayon::prelude::*;
use sha2::{Digest, Sha256};

use crate::build_metadata::BuildMetadataContext;
use crate::language::{DiscoveredSourceFile, SourceLanguage};
use crate::lua_parser;
use crate::python_parser;
use crate::rust_parser;
use crate::typescript_parser;
use crate::metadata::{
    apply_metadata_to_file_record_with_context, apply_metadata_to_symbol_with_context,
    apply_risk_signals_to_file_record, apply_risk_signals_to_symbol,
};
use crate::models::{
    CallableFlowSummary, FileRecord, FileRiskSignals, IncludeDependency, IncludeHeaviness, MacroSensitivity,
    ParseFragility, ParseMetrics, ParseResult, PropagationEvent, RawCallSite, RawRelationEvent,
    Symbol,
};
// removed crate::parser; as it's not used directly anymore
use std::collections::HashMap;

const DEFAULT_INDEXER_WORKER_STACK_BYTES: usize = 64 * 1024 * 1024;
const MIN_INDEXER_WORKER_STACK_BYTES: usize = 2 * 1024 * 1024;
const PARSE_PROGRESS_LOG_INTERVAL: Duration = Duration::from_secs(15);
const PARSE_PROGRESS_POLL_INTERVAL: Duration = Duration::from_millis(250);
const PARSE_SLOW_FILE_THRESHOLD: Duration = Duration::from_secs(20);
const DEFAULT_CPP_SKIP_THRESHOLD_BYTES: usize = 2 * 1024 * 1024;
const CPP_SKIP_CONTENT_SAMPLE_BYTES: usize = 64 * 1024;
static INDEXER_THREAD_POOL: OnceLock<rayon::ThreadPool> = OnceLock::new();
const SKIP_CPP_LARGER_THAN_BYTES_ENV: &str = "CODEATLAS_SKIP_CPP_LARGER_THAN_BYTES";
/// When set to a positive integer, caps the indexing thread pool size.
/// Used in watch/background mode to avoid starving the foreground process.
pub const BACKGROUND_THREADS_ENV: &str = "CODEATLAS_BACKGROUND_THREADS";

/// Caps the number of simultaneously active libclang translation units.
/// Defaults to min(4, thread_pool_size) — libclang TUs are memory-heavy
/// (200–500 MB each), so limiting concurrency prevents multi-GB RSS spikes
/// on large C++ projects. Override with CODEATLAS_CPP_PARSE_THREADS.
const CPP_PARSE_THREADS_ENV: &str = "CODEATLAS_CPP_PARSE_THREADS";
static CPP_PARSE_SEMAPHORE: OnceLock<CppParseSemaphore> = OnceLock::new();

struct CppParseSemaphore {
    permits: Mutex<usize>,
    cond: Condvar,
}

impl CppParseSemaphore {
    fn new(n: usize) -> Self {
        CppParseSemaphore { permits: Mutex::new(n), cond: Condvar::new() }
    }

    fn acquire(&self) {
        let mut avail = self.permits.lock().unwrap();
        while *avail == 0 {
            avail = self.cond.wait(avail).unwrap();
        }
        *avail -= 1;
    }

    fn release(&self) {
        let mut avail = self.permits.lock().unwrap();
        *avail += 1;
        self.cond.notify_one();
    }
}

/// RAII guard — releases one permit when dropped.
struct CppParsePermit;
impl Drop for CppParsePermit {
    fn drop(&mut self) {
        if let Some(sem) = CPP_PARSE_SEMAPHORE.get() {
            sem.release();
        }
    }
}

/// Acquires one C++ parse permit, blocking if the concurrency limit is reached.
/// Default: min(2, pool_size) — keeps peak RSS under ~1 GB on large C++ projects
/// (each libclang TU can consume 200–500 MB while active).
/// Override with CODEATLAS_CPP_PARSE_THREADS for higher throughput on
/// memory-rich machines (e.g. CODEATLAS_CPP_PARSE_THREADS=4 restores old behaviour).
fn acquire_cpp_parse_permit() -> CppParsePermit {
    let sem = CPP_PARSE_SEMAPHORE.get_or_init(|| {
        let pool_size = indexing_thread_pool().current_num_threads();
        let permits = std::env::var(CPP_PARSE_THREADS_ENV)
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|&n| n > 0)
            .unwrap_or_else(|| pool_size.min(8).max(1));
        eprintln!("  C++ parse concurrency: {} thread(s) (pool: {})", permits, pool_size);
        CppParseSemaphore::new(permits)
    });
    sem.acquire();
    CppParsePermit
}

#[derive(Clone)]
struct ActiveParseEntry {
    language: SourceLanguage,
    started_at: Instant,
}

pub trait LanguageAdapter: Send + Sync {
    fn language(&self) -> SourceLanguage;
    fn parse_file(&self, file_path: &str, source: &str, build_metadata: Option<&BuildMetadataContext>, workspace_root: Option<&std::path::Path>) -> Result<ParseResult, String>;
}

struct CppLanguageAdapter;
struct LuaLanguageAdapter;
struct PythonLanguageAdapter;
struct RustLanguageAdapter;
struct TypeScriptLanguageAdapter;

impl LanguageAdapter for CppLanguageAdapter {
    fn language(&self) -> SourceLanguage {
        SourceLanguage::Cpp
    }

    fn parse_file(&self, file_path: &str, source: &str, build_metadata: Option<&BuildMetadataContext>, workspace_root: Option<&std::path::Path>) -> Result<ParseResult, String> {
        let mut args = Vec::new();
        let has_file_entry = build_metadata
            .and_then(|meta| meta.entry_for_file(file_path))
            .is_some();

        // Without compile_commands.json context, Clang must parse without
        // proper include paths. For large projects this is both slow (Clang
        // errors on every missing header) and lower quality than tree-sitter.
        // Fall back to the tree-sitter C++ parser in that case.
        if !has_file_entry {
            return crate::parser::parse_cpp_file(file_path, source);
        }

        // Always add workspace_root as an include search path so that
        // `#include "opencv2/..."` style relative-to-project-root includes
        // resolve even without compile_commands.json.
        if let Some(root) = workspace_root {
            args.push(format!("-I{}", root.to_string_lossy()));
        }

        if let Some(meta) = build_metadata {
            if let Some(entry) = meta.entry_for_file(file_path) {
                for dir in &entry.include_dirs {
                    args.push(format!("-I{}", dir));
                }
                for def in &entry.defines {
                    args.push(format!("-D{}", def));
                }
            }
        }

        // When the file is a C/C++ header (.h, .hpp, .hxx, .hh, .inl) and
        // no -x flag was provided by the build system, tell Clang to parse it
        // as C++ rather than letting it infer "C header" from the extension.
        // Without this, Clang silently ignores classes, namespaces, etc.
        let is_header = file_path.ends_with(".h")
            || file_path.ends_with(".hpp")
            || file_path.ends_with(".hxx")
            || file_path.ends_with(".hh")
            || file_path.ends_with(".inl");
        let has_x_flag = args.iter().any(|a| a == "-x" || a.starts_with("-x"));
        if is_header && !has_x_flag {
            args.push("-x".to_string());
            args.push("c++-header".to_string());
        }

        // MS22 parse cache gate. Compute a content-addressable key over the
        // normalized args + source + direct-include contents, and serve an
        // unchanged TU from cache — skipping the libclang parse AND its permit /
        // 200–500 MB RSS spike entirely. A miss falls through to a real parse
        // whose result is stored (unless macro-sensitive — see below). The cache
        // is a pure memoization: any change to args/source/direct-includes shifts
        // the key, forcing a real re-parse.
        let cache_key = compute_parse_cache_key(source, file_path, &args, workspace_root);
        if let Some(ref key) = cache_key {
            if let Some(cached) = crate::parse_cache::lookup(key) {
                return Ok(cached);
            }
        }

        // Acquire a permit to bound simultaneous libclang TUs and prevent
        // multi-GB RSS spikes. Released automatically when the guard is dropped.
        let _permit = acquire_cpp_parse_permit();

        let result = crate::clang_parser::parse_cpp_file(file_path, source, &args, workspace_root)?;

        // Store on a miss — but never cache macro-sensitive TUs. Their parse
        // output depends on preprocessor state our cheap direct-include key does
        // not fully capture, so they must always be parsed fresh (matches MS16's
        // macro fallback philosophy). Skipping the store means such a file never
        // has an entry and therefore always misses → always parses.
        if let Some(ref key) = cache_key {
            if result.file_risk_signals.macro_sensitivity != MacroSensitivity::High {
                crate::parse_cache::store(key, &result);
            }
        }

        Ok(result)
    }
}

/// Build the MS22 parse-cache key for a C++ translation unit, or `None` when the
/// key cannot be computed (in which case the caller parses without caching).
///
/// Resolves the TU's direct `#include`s against the build's `-I` search dirs and
/// hashes each resolved header's contents, so a change to any directly-included
/// header shifts the key. Resolution mirrors clang's default order: quote
/// includes (`"..."`) search the including file's own directory first, then the
/// `-I` dirs; angle includes (`<...>`) search only the `-I` dirs. An include
/// that resolves to no on-disk file contributes its path but no content hash
/// (its change is then caught by the includer's own re-parse under MS16 — see
/// MS22 Risk 1).
fn compute_parse_cache_key(
    source: &str,
    file_path: &str,
    args: &[String],
    workspace_root: Option<&Path>,
) -> Option<String> {
    let include_dirs: Vec<&str> = args
        .iter()
        .filter_map(|a| a.strip_prefix("-I"))
        .filter(|d| !d.is_empty())
        .collect();

    // Directory of the including file on disk (for quote-include resolution).
    let own_dir: Option<std::path::PathBuf> = workspace_root.map(|root| {
        root.join(file_path.replace('/', std::path::MAIN_SEPARATOR_STR))
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| root.to_path_buf())
    });

    let deps = crate::parser::extract_include_dependencies(source, file_path);
    let mut direct_includes: Vec<crate::parse_cache::DirectInclude> = Vec::with_capacity(deps.len());
    for dep in &deps {
        let header = dep.included_file.as_str();
        let resolved = resolve_include_path(header, dep.is_system_include, own_dir.as_deref(), &include_dirs);
        let content_hash = resolved
            .as_ref()
            .and_then(|p| fs::read(p).ok())
            .map(|bytes| format!("{:x}", Sha256::digest(&bytes)))
            .unwrap_or_default();
        direct_includes.push((header.to_string(), content_hash));
    }

    Some(crate::parse_cache::compute_key(args, source, &direct_includes))
}

/// Resolve one `#include` to an on-disk path using clang's basic search order.
/// Returns `None` when no candidate exists.
fn resolve_include_path(
    header: &str,
    is_system_include: bool,
    own_dir: Option<&Path>,
    include_dirs: &[&str],
) -> Option<std::path::PathBuf> {
    let rel = header.replace('/', std::path::MAIN_SEPARATOR_STR);
    // Quote includes search the including file's own directory first.
    if !is_system_include {
        if let Some(dir) = own_dir {
            let candidate = dir.join(&rel);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    for dir in include_dirs {
        let candidate = Path::new(dir).join(&rel);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

impl LanguageAdapter for LuaLanguageAdapter {
    fn language(&self) -> SourceLanguage {
        SourceLanguage::Lua
    }

    fn parse_file(&self, file_path: &str, source: &str, _build_metadata: Option<&BuildMetadataContext>, _workspace_root: Option<&std::path::Path>) -> Result<ParseResult, String> {
        lua_parser::parse_lua_file_dual(file_path, source)
    }
}

impl LanguageAdapter for PythonLanguageAdapter {
    fn language(&self) -> SourceLanguage {
        SourceLanguage::Python
    }

    fn parse_file(&self, file_path: &str, source: &str, _build_metadata: Option<&BuildMetadataContext>, _workspace_root: Option<&std::path::Path>) -> Result<ParseResult, String> {
        python_parser::parse_python_file_dual(file_path, source)
    }
}

impl LanguageAdapter for TypeScriptLanguageAdapter {
    fn language(&self) -> SourceLanguage {
        SourceLanguage::TypeScript
    }

    fn parse_file(&self, file_path: &str, source: &str, _build_metadata: Option<&BuildMetadataContext>, _workspace_root: Option<&std::path::Path>) -> Result<ParseResult, String> {
        typescript_parser::parse_typescript_file_dual(file_path, source)
    }
}

impl LanguageAdapter for RustLanguageAdapter {
    fn language(&self) -> SourceLanguage {
        SourceLanguage::Rust
    }

    fn parse_file(&self, file_path: &str, source: &str, _build_metadata: Option<&BuildMetadataContext>, _workspace_root: Option<&std::path::Path>) -> Result<ParseResult, String> {
        rust_parser::parse_rust_file_dual(file_path, source)
    }
}

pub struct LanguageRegistry {
    adapters: HashMap<SourceLanguage, Box<dyn LanguageAdapter>>,
}

impl LanguageRegistry {
    pub fn new() -> Self {
        Self {
            adapters: HashMap::new(),
        }
    }

    pub fn register<A: LanguageAdapter + 'static>(&mut self, adapter: A) {
        self.adapters.insert(adapter.language(), Box::new(adapter));
    }

    #[allow(dead_code)]
    pub fn supported_languages(&self) -> Vec<SourceLanguage> {
        let mut languages: Vec<_> = self.adapters.keys().copied().collect();
        languages.sort_by_key(|language| language.display_name());
        languages
    }

    pub fn language_for_path(&self, path: &str) -> Option<SourceLanguage> {
        let language = SourceLanguage::from_path(Path::new(path))?;
        self.adapters.contains_key(&language).then_some(language)
    }

    pub fn parse_file(
        &self,
        language: SourceLanguage,
        file_path: &str,
        source: &str,
        build_metadata: Option<&BuildMetadataContext>,
        workspace_root: Option<&std::path::Path>,
    ) -> Result<ParseResult, String> {
        let adapter = self
            .adapters
            .get(&language)
            .ok_or_else(|| format!("No language adapter registered for {}", language.display_name()))?;
        adapter.parse_file(file_path, source, build_metadata, workspace_root)
    }
}

pub fn default_language_registry() -> LanguageRegistry {
    let mut registry = LanguageRegistry::new();
    registry.register(CppLanguageAdapter);
    registry.register(LuaLanguageAdapter);
    registry.register(PythonLanguageAdapter);
    registry.register(RustLanguageAdapter);
    registry.register(TypeScriptLanguageAdapter);
    registry
}

fn indexing_thread_pool() -> &'static rayon::ThreadPool {
    INDEXER_THREAD_POOL.get_or_init(|| {
        let mut builder = rayon::ThreadPoolBuilder::new()
            .thread_name(|index| format!("codeatlas-index-{}", index))
            .stack_size(configured_indexer_worker_stack_bytes());
        // Apply background thread cap if set.
        let thread_cap = configured_indexer_thread_count();
        if thread_cap > 0 {
            builder = builder.num_threads(thread_cap);
        }
        builder
            .build()
            .expect("Failed to build CodeAtlas indexing thread pool")
    })
}

/// Returns the thread count for the indexing pool.
///
/// Priority:
///   1. `CODEATLAS_BACKGROUND_THREADS` env var (explicit override; 0 = all cores)
///   2. Default: half the available logical CPUs, clamped to [4, 16]
///
/// The default cap prevents each rayon worker from holding its own CXIndex
/// (libclang parse context) from exhausting system memory on wide machines.
/// Raise the cap with `CODEATLAS_BACKGROUND_THREADS=<n>` when you need more
/// throughput and have sufficient RAM.
fn configured_indexer_thread_count() -> usize {
    if let Ok(val) = std::env::var(BACKGROUND_THREADS_ENV) {
        // Explicit override: 0 means "all logical cores" (rayon default).
        return val.trim().parse::<usize>().unwrap_or(0);
    }

    // Default: half of available logical CPUs, clamped between 4 and 16.
    let logical_cpus = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    (logical_cpus / 2).clamp(4, 16)
}

fn configured_indexer_worker_stack_bytes() -> usize {
    read_stack_size_from_env("CODEATLAS_INDEXER_STACK_BYTES")
        .or_else(|| read_stack_size_from_env("RUST_MIN_STACK"))
        .unwrap_or(DEFAULT_INDEXER_WORKER_STACK_BYTES)
}

fn read_stack_size_from_env(name: &str) -> Option<usize> {
    let value = std::env::var(name).ok()?;
    parse_stack_size_bytes(&value)
}

fn configured_cpp_skip_threshold_bytes() -> Option<usize> {
    match std::env::var(SKIP_CPP_LARGER_THAN_BYTES_ENV) {
        Ok(value) => parse_optional_threshold_bytes(&value),
        Err(_) => Some(DEFAULT_CPP_SKIP_THRESHOLD_BYTES),
    }
}

fn parse_stack_size_bytes(value: &str) -> Option<usize> {
    let trimmed = value.trim();
    let bytes = trimmed.parse::<usize>().ok()?;
    (bytes >= MIN_INDEXER_WORKER_STACK_BYTES).then_some(bytes)
}

fn parse_optional_threshold_bytes(value: &str) -> Option<usize> {
    let trimmed = value.trim();
    let bytes = trimmed.parse::<usize>().ok()?;
    if bytes == 0 {
        return None;
    }
    Some(bytes)
}

fn skip_reason_before_parse(language: SourceLanguage, content: &str) -> Option<&'static str> {
    match language {
        SourceLanguage::Cpp => classify_cpp_skip_reason(content),
        _ => None,
    }
}

fn classify_cpp_skip_reason(content: &str) -> Option<&'static str> {
    let sample = sample_prefix(content, CPP_SKIP_CONTENT_SAMPLE_BYTES);
    let source_structure_markers = count_cpp_source_markers(sample);

    if configured_cpp_skip_threshold_bytes()
        .map(|threshold| content.len() > threshold)
        .unwrap_or(false)
        && source_structure_markers < 6
    {
        return Some("exceeds oversized-file threshold");
    }

    let invalid_marker_count = sample
        .chars()
        .filter(|ch| *ch == '\0' || *ch == '\u{FFFD}')
        .count();
    if invalid_marker_count >= 128 && source_structure_markers < 6 {
        return Some("binary-like content signature");
    }

    let numeric_blob_lines = sample
        .lines()
        .take(64)
        .filter(|line| is_numeric_blob_line(line))
        .count();
    if numeric_blob_lines >= 8 && source_structure_markers < 6 {
        return Some("embedded numeric blob signature");
    }

    None
}

fn sample_prefix(content: &str, max_bytes: usize) -> &str {
    if content.len() <= max_bytes {
        return content;
    }

    let mut end = 0usize;
    for (index, ch) in content.char_indices() {
        let next = index + ch.len_utf8();
        if next > max_bytes {
            break;
        }
        end = next;
    }

    if end == 0 {
        ""
    } else {
        &content[..end]
    }
}

fn count_cpp_source_markers(sample: &str) -> usize {
    const MARKERS: [&str; 18] = [
        "#include",
        "namespace ",
        "class ",
        "struct ",
        "enum ",
        "typedef ",
        "template<",
        "template <",
        "using ",
        "extern ",
        "::",
        "{",
        "}",
        ";",
        "if(",
        "for(",
        "while(",
        "switch(",
    ];

    MARKERS
        .iter()
        .map(|marker| sample.matches(marker).count())
        .sum()
}

fn is_numeric_blob_line(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.len() < 48 {
        return false;
    }

    let mut digits = 0usize;
    let mut commas = 0usize;
    let mut alpha = 0usize;
    let mut allowed = 0usize;

    for ch in trimmed.chars() {
        if ch.is_ascii_digit() {
            digits += 1;
            allowed += 1;
        } else if matches!(ch, ',' | ' ' | '\t' | '-' | '+') {
            if ch == ',' {
                commas += 1;
            }
            allowed += 1;
        } else if ch.is_ascii_alphabetic() {
            alpha += 1;
        }
    }

    commas >= 12
        && digits >= 24
        && alpha <= 2
        && allowed * 100 / trimmed.len().max(1) >= 90
}

fn empty_parse_result() -> ParseResult {
    ParseResult {
        symbols: Vec::new(),
        file_risk_signals: FileRiskSignals {
            parse_fragility: ParseFragility::Low,
            macro_sensitivity: MacroSensitivity::Low,
            include_heaviness: IncludeHeaviness::Light,
        },
        relation_events: Vec::new(),
        normalized_references: Vec::new(),
        propagation_events: Vec::new(),
        callable_flow_summaries: Vec::new(),
        raw_calls: Vec::new(),
        metrics: ParseMetrics::default(),
        include_dependencies: Vec::new(),
        macro_definitions: Vec::new(),
        conditional_blocks: Vec::new(),
        dependency_metrics: crate::models::DependencyMetrics::default(),
        conditional_symbols: Vec::new(),
    }
}

fn discard_runtime_only_parse_payload(parse_result: &mut ParseResult) {
    // Full/incremental indexing rebuilds references from retained relation events later,
    // so keeping per-file normalized references alive in collector buffers only inflates memory.
    parse_result.normalized_references.clear();
}

fn extend_non_call_relation_events(
    destination: &mut Vec<RawRelationEvent>,
    relation_events: Vec<RawRelationEvent>,
) {
    destination.extend(
        relation_events
            .into_iter()
            .filter(|event| event.relation_kind != crate::models::RawRelationKind::Call),
    );
}

#[allow(dead_code)]
pub fn parse_discovered_files(
    workspace_root: &Path,
    discovered_files: &[DiscoveredSourceFile],
    verbose: bool,
    build_metadata: Option<&BuildMetadataContext>,
    registry: &LanguageRegistry,
) -> (
    Vec<Symbol>,
    Vec<RawCallSite>,
    Vec<RawRelationEvent>,
    Vec<PropagationEvent>,
    Vec<CallableFlowSummary>,
    Vec<FileRecord>,
    Vec<IncludeDependency>,
    Vec<crate::models::MacroDefinition>,
    Vec<crate::models::ConditionalBlock>,
    Vec<crate::models::ConditionalSymbol>,
    ParseMetrics,
) {
    parse_discovered_files_with_progress(
        workspace_root,
        discovered_files,
        verbose,
        build_metadata,
        registry,
        0,
        discovered_files.len(),
    )
}

pub fn parse_discovered_files_with_progress(
    workspace_root: &Path,
    discovered_files: &[DiscoveredSourceFile],
    verbose: bool,
    build_metadata: Option<&BuildMetadataContext>,
    registry: &LanguageRegistry,
    progress_offset: usize,
    overall_total: usize,
) -> (
    Vec<Symbol>,
    Vec<RawCallSite>,
    Vec<RawRelationEvent>,
    Vec<PropagationEvent>,
    Vec<CallableFlowSummary>,
    Vec<FileRecord>,
    Vec<IncludeDependency>,
    Vec<crate::models::MacroDefinition>,
    Vec<crate::models::ConditionalBlock>,
    Vec<crate::models::ConditionalSymbol>,
    ParseMetrics,
) {
    let batch_total = discovered_files.len();
    let total = overall_total.max(batch_total);
    let progress = Arc::new(AtomicUsize::new(0));
    let active_files = Arc::new(Mutex::new(HashMap::<String, ActiveParseEntry>::new()));
    let parsing_complete = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let monitor_handle = if !verbose && batch_total > 0 {
        Some(spawn_parse_progress_monitor(
            total,
            Arc::clone(&active_files),
            Arc::clone(&parsing_complete),
            Arc::clone(&progress),
            progress_offset,
        ))
    } else {
        None
    };

    let results: Vec<(String, Result<ParseResult, String>, String, bool)> = indexing_thread_pool().install(|| {
        discovered_files
            .par_iter()
            .map(|discovered| {
                let rel_path = make_relative(workspace_root, &discovered.path);
                {
                    let mut active = active_files.lock().expect("active parse tracker poisoned");
                    active.insert(
                        rel_path.clone(),
                        ActiveParseEntry {
                            language: discovered.language,
                            started_at: Instant::now(),
                        },
                    );
                }
                let (content, hash, lossy) = match read_source_file(&discovered.path) {
                    Ok(result) => result,
                    Err(e) => {
                        let _ = active_files
                            .lock()
                            .map(|mut active| active.remove(&rel_path));
                        let current = progress_offset + progress.fetch_add(1, Ordering::Relaxed) + 1;
                        if !verbose {
                            eprintln!(
                                "  Parsing: {}/{} | read error in {}",
                                current, total, rel_path
                            );
                        }
                        return (rel_path.clone(), Err(format!("Read error: {}", e)), String::new(), false);
                    }
                };
                if let Some(skip_reason) = skip_reason_before_parse(discovered.language, &content) {
                    let _ = active_files
                        .lock()
                        .map(|mut active| active.remove(&rel_path));
                    let current = progress_offset + progress.fetch_add(1, Ordering::Relaxed) + 1;
                    if verbose {
                        println!(
                            "  [{}/{}] SKIP: {}: {} ({} bytes)",
                            current,
                            total,
                            rel_path,
                            skip_reason,
                            content.len()
                        );
                    } else {
                        eprintln!(
                            "  Parsing: {}/{} | skipped file {}: {} ({} bytes)",
                            current,
                            total,
                            rel_path,
                            skip_reason,
                            content.len()
                        );
                    }
                    return (rel_path, Ok(empty_parse_result()), hash, lossy);
                }
                let mut result = registry.parse_file(discovered.language, &rel_path, &content, build_metadata, Some(workspace_root));
                let elapsed = {
                    let mut active = active_files.lock().expect("active parse tracker poisoned");
                    active
                        .remove(&rel_path)
                        .map(|entry| entry.started_at.elapsed())
                        .unwrap_or_default()
                };
                let current = progress_offset + progress.fetch_add(1, Ordering::Relaxed) + 1;
                if verbose {
                    match &result {
                        Ok(pr) => {
                            if lossy {
                                println!("  [{}/{}] LOSSY: {}: non-UTF8 bytes replaced during parsing", current, total, rel_path);
                            }
                            println!(
                                "  [{}/{}] INDEX: {}: {} symbols, {} raw calls",
                                current,
                                total,
                                rel_path,
                                pr.symbols.len(),
                                pr.raw_calls.len()
                            );
                        }
                        Err(e) => {
                            if is_parse_timeout_error(e) {
                                println!("  [{}/{}] TIMEOUT: {}: {}", current, total, rel_path, e);
                            } else {
                                println!("  [{}/{}] FAILED: {}: {}", current, total, rel_path, e);
                            }
                        }
                    }
                } else if let Err(err) = &result {
                    if is_parse_timeout_error(err) {
                        eprintln!(
                            "  Parse timeout: {}/{} after {} | {} ({:?}) | {}",
                            current,
                            total,
                            format_elapsed_ms(elapsed),
                            rel_path,
                            discovered.language,
                            err
                        );
                    } else if elapsed >= PARSE_SLOW_FILE_THRESHOLD {
                        eprintln!(
                            "  Slow failure: {}/{} after {} | {} ({:?}) | {}",
                            current,
                            total,
                            format_elapsed_ms(elapsed),
                            rel_path,
                            discovered.language,
                            err
                        );
                    }
                } else if elapsed >= PARSE_SLOW_FILE_THRESHOLD {
                    match &result {
                        Ok(pr) => {
                            eprintln!(
                                "  Slow parse: {}/{} done in {} | {} ({:?}) | {} symbols, {} raw calls | {}",
                                current,
                                total,
                                format_elapsed_ms(elapsed),
                                rel_path,
                                discovered.language,
                                pr.symbols.len(),
                                pr.raw_calls.len(),
                                summarize_parse_metrics(&pr.metrics),
                            );
                        }
                        Err(_) => {}
                    }
                }
                if let Ok(pr) = &mut result {
                    discard_runtime_only_parse_payload(pr);
                }
                (rel_path, result, hash, lossy)
            })
            .collect()
    });
    parsing_complete.store(true, Ordering::Relaxed);
    if let Some(handle) = monitor_handle {
        let _ = handle.join();
    }

    let mut symbols = Vec::new();
    let mut raw_calls = Vec::new();
    let mut relation_events = Vec::new();
    let mut propagation_events = Vec::new();
    let mut callable_flow_summaries = Vec::new();
    let mut include_dependencies = Vec::new();
    let mut macro_definitions = Vec::new();
    let mut conditional_blocks = Vec::new();
    let mut conditional_symbols = Vec::new();
    let mut file_records = Vec::new();
    let mut metrics = ParseMetrics::default();

    for (rel_path, result, hash, _lossy) in results {
        match result {
            Ok(pr) => {
                let mut file_record = FileRecord {
                    path: rel_path,
                    content_hash: hash,
                    last_indexed: Utc::now().to_rfc3339(),
                    symbol_count: pr.symbols.len(),
                    module: None,
                    subsystem: None,
                    project_area: None,
                    artifact_kind: None,
                    header_role: None,
                    parse_fragility: None,
                    macro_sensitivity: None,
                    include_heaviness: None,
                };
                apply_metadata_to_file_record_with_context(&mut file_record, build_metadata);
                apply_risk_signals_to_file_record(&mut file_record, &pr.file_risk_signals);
                file_records.push(file_record);
                extend_non_call_relation_events(&mut relation_events, pr.relation_events);
                propagation_events.extend(pr.propagation_events);
                callable_flow_summaries.extend(pr.callable_flow_summaries);
                metrics.tree_sitter_parse_ms += pr.metrics.tree_sitter_parse_ms;
                metrics.syntax_walk_ms += pr.metrics.syntax_walk_ms;
                metrics.local_propagation_ms += pr.metrics.local_propagation_ms;
                metrics.local_function_discovery_ms += pr.metrics.local_function_discovery_ms;
                metrics.local_owner_lookup_ms += pr.metrics.local_owner_lookup_ms;
                metrics.local_seed_ms += pr.metrics.local_seed_ms;
                metrics.local_event_walk_ms += pr.metrics.local_event_walk_ms;
                metrics.local_declaration_ms += pr.metrics.local_declaration_ms;
                metrics.local_expression_statement_ms += pr.metrics.local_expression_statement_ms;
                metrics.local_return_statement_ms += pr.metrics.local_return_statement_ms;
                metrics.local_nested_block_ms += pr.metrics.local_nested_block_ms;
                metrics.local_return_collection_ms += pr.metrics.local_return_collection_ms;
                metrics.graph_relation_ms += pr.metrics.graph_relation_ms;
                metrics.graph_rule_compile_ms += pr.metrics.graph_rule_compile_ms;
                metrics.graph_rule_execute_ms += pr.metrics.graph_rule_execute_ms;
                metrics.reference_normalization_ms += pr.metrics.reference_normalization_ms;
                let mut enriched_symbols = pr.symbols;
                for symbol in &mut enriched_symbols {
                    apply_metadata_to_symbol_with_context(symbol, build_metadata);
                    apply_risk_signals_to_symbol(symbol, &pr.file_risk_signals);
                }
                symbols.extend(enriched_symbols);
                raw_calls.extend(pr.raw_calls);
                include_dependencies.extend(pr.include_dependencies);
                macro_definitions.extend(pr.macro_definitions);
                conditional_blocks.extend(pr.conditional_blocks);
                conditional_symbols.extend(pr.conditional_symbols);
            }
            Err(e) => {
                if !verbose {
                    if is_parse_timeout_error(&e) {
                        eprintln!("  TIMEOUT: {}: {}", rel_path, e);
                    } else {
                        eprintln!("  FAILED: {}: {}", rel_path, e);
                    }
                }
            }
        }
    }

    (
        symbols,
        raw_calls,
        relation_events,
        propagation_events,
        callable_flow_summaries,
        file_records,
        include_dependencies,
        macro_definitions,
        conditional_blocks,
        conditional_symbols,
        metrics,
    )
}

fn spawn_parse_progress_monitor(
    total: usize,
    active_files: Arc<Mutex<HashMap<String, ActiveParseEntry>>>,
    parsing_complete: Arc<std::sync::atomic::AtomicBool>,
    progress: Arc<AtomicUsize>,
    progress_offset: usize,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut waited = Duration::ZERO;
        loop {
            if parsing_complete.load(Ordering::Relaxed) {
                break;
            }
            thread::sleep(PARSE_PROGRESS_POLL_INTERVAL);
            waited += PARSE_PROGRESS_POLL_INTERVAL;
            if waited < PARSE_PROGRESS_LOG_INTERVAL {
                continue;
            }
            waited = Duration::ZERO;

            let completed = (progress_offset + progress.load(Ordering::Relaxed)).min(total);
            let snapshot = {
                let active = active_files.lock().expect("active parse tracker poisoned");
                let mut entries: Vec<_> = active
                    .iter()
                    .map(|(path, entry)| {
                        (
                            path.clone(),
                            entry.language,
                            entry.started_at.elapsed(),
                        )
                    })
                    .collect();
                entries.sort_by(|a, b| b.2.cmp(&a.2));
                entries.truncate(3);
                entries
            };

            if snapshot.is_empty() {
                eprintln!("  Parsing: {}/{} | waiting for worker completion...", completed, total);
                continue;
            }

            let active_summary = snapshot
                .iter()
                .map(|(path, language, elapsed)| {
                    format!("{} ({:?}, {})", path, language, format_elapsed_ms(*elapsed))
                })
                .collect::<Vec<_>>()
                .join(" | ");
            eprintln!(
                "  Parsing: {}/{} | active slow files: {}",
                completed, total, active_summary
            );
        }
    })
}

fn summarize_parse_metrics(metrics: &ParseMetrics) -> String {
    format!(
        "tree-sitter {} | syntax-walk {} | local-propagation {} | graph-relations {} | graph-execute {}",
        format_elapsed_u128_ms(metrics.tree_sitter_parse_ms),
        format_elapsed_u128_ms(metrics.syntax_walk_ms),
        format_elapsed_u128_ms(metrics.local_propagation_ms),
        format_elapsed_u128_ms(metrics.graph_relation_ms),
        format_elapsed_u128_ms(metrics.graph_rule_execute_ms),
    )
}

fn format_elapsed_ms(elapsed: Duration) -> String {
    let elapsed_ms = elapsed.as_millis();
    if elapsed_ms >= 1_000 {
        format!("{}ms ({:.2}s)", elapsed_ms, elapsed_ms as f64 / 1_000.0)
    } else {
        format!("{}ms", elapsed_ms)
    }
}

fn is_parse_timeout_error(message: &str) -> bool {
    message.contains("Parse timed out after")
}

fn format_elapsed_u128_ms(elapsed_ms: u128) -> String {
    if elapsed_ms >= 1_000 {
        format!("{}ms ({:.2}s)", elapsed_ms, elapsed_ms as f64 / 1_000.0)
    } else {
        format!("{}ms", elapsed_ms)
    }
}

/// Parses `paths` in parallel using the shared indexing thread pool.
///
/// Each entry in the returned `Vec` corresponds to one input path and contains
/// `(rel_path, ParseResult, lossy, skipped)`.  The order of entries matches the
/// order of `paths`.  If any file fails to parse the whole batch is aborted and
/// the error is returned.
///
/// Callers are responsible for applying any subsequent DB mutations
/// sequentially after this call returns.
pub fn parse_paths_parallel(
    workspace_root: &Path,
    paths: &[&str],
    build_metadata: Option<&BuildMetadataContext>,
) -> Result<Vec<(String, ParseResult, bool, bool)>, String> {
    if paths.is_empty() {
        return Ok(Vec::new());
    }
    indexing_thread_pool().install(|| {
        paths
            .par_iter()
            .map(|path| {
                parse_file_strict(workspace_root, path, build_metadata)
                    .map(|(result, _hash, lossy, skipped)| (path.to_string(), result, lossy, skipped))
            })
            .collect::<Result<Vec<_>, _>>()
    })
}

pub fn parse_file_strict(
    workspace_root: &Path,
    rel_path: &str,
    build_metadata: Option<&BuildMetadataContext>,
) -> Result<(ParseResult, String, bool, bool), String> {
    let registry = default_language_registry();
    let language = registry
        .language_for_path(rel_path)
        .ok_or_else(|| format!("No language adapter registered for {}", rel_path))?;
    let discovered = DiscoveredSourceFile {
        path: workspace_root.join(rel_path.replace('/', std::path::MAIN_SEPARATOR_STR)),
        language,
    };
    parse_discovered_file_strict(workspace_root, &discovered, build_metadata, &registry)
}

pub fn parse_discovered_file_strict(
    workspace_root: &Path,
    discovered: &DiscoveredSourceFile,
    build_metadata: Option<&BuildMetadataContext>,
    registry: &LanguageRegistry,
) -> Result<(ParseResult, String, bool, bool), String> {
    let rel_path = make_relative(workspace_root, &discovered.path);
    let (content, hash, lossy) = read_source_file(&discovered.path)
        .map_err(|e| format!("Read error for {}: {}", rel_path, e))?;
    if skip_reason_before_parse(discovered.language, &content).is_some() {
        return Ok((empty_parse_result(), hash, lossy, true));
    }
    let result = match registry.parse_file(discovered.language, &rel_path, &content, build_metadata, Some(workspace_root)) {
        Ok(result) => result,
        Err(e) => {
            let message = format!("Parse error for {}: {}", rel_path, e);
            if is_parse_timeout_error(&e) {
                eprintln!("  TIMEOUT: {}: {}", rel_path, e);
            }
            return Err(message);
        }
    };
    let mut result = result;
    for symbol in &mut result.symbols {
        apply_metadata_to_symbol_with_context(symbol, build_metadata);
        apply_risk_signals_to_symbol(symbol, &result.file_risk_signals);
    }
    Ok((result, hash, lossy, false))
}

pub fn parse_files_strict(
    workspace_root: &Path,
    relative_paths: &[String],
    verbose: bool,
    build_metadata: Option<&BuildMetadataContext>,
) -> Result<(
    Vec<Symbol>,
    Vec<RawCallSite>,
    Vec<RawRelationEvent>,
    Vec<PropagationEvent>,
    Vec<CallableFlowSummary>,
    Vec<FileRecord>,
    ParseMetrics,
), String> {
    let registry = default_language_registry();
    let discovered_files: Result<Vec<_>, String> = relative_paths
        .iter()
        .map(|rel_path| {
            let language = registry
                .language_for_path(rel_path)
                .ok_or_else(|| format!("No language adapter registered for {}", rel_path))?;
            Ok(DiscoveredSourceFile {
                path: workspace_root.join(rel_path.replace('/', std::path::MAIN_SEPARATOR_STR)),
                language,
            })
        })
        .collect();
    parse_discovered_files_strict(
        workspace_root,
        &discovered_files?,
        verbose,
        build_metadata,
        &registry,
    )
}

pub fn parse_discovered_files_strict(
    workspace_root: &Path,
    discovered_files: &[DiscoveredSourceFile],
    verbose: bool,
    build_metadata: Option<&BuildMetadataContext>,
    registry: &LanguageRegistry,
) -> Result<(
    Vec<Symbol>,
    Vec<RawCallSite>,
    Vec<RawRelationEvent>,
    Vec<PropagationEvent>,
    Vec<CallableFlowSummary>,
    Vec<FileRecord>,
    ParseMetrics,
), String> {
    let total = discovered_files.len();

    // Parse in parallel using the shared indexing thread pool.
    // Each worker produces its result independently; errors are collected and
    // checked during the sequential aggregation pass below.
    let raw_results: Vec<(String, Result<(ParseResult, String, bool, bool), String>)> =
        indexing_thread_pool().install(|| {
            discovered_files
                .par_iter()
                .map(|discovered| {
                    let rel_path = make_relative(workspace_root, &discovered.path);
                    let result = parse_discovered_file_strict(
                        workspace_root,
                        discovered,
                        build_metadata,
                        registry,
                    );
                    (rel_path, result)
                })
                .collect()
        });

    // Sequential aggregation — fast, preserves deterministic output order.
    let mut symbols = Vec::new();
    let mut raw_calls = Vec::new();
    let mut relation_events = Vec::new();
    let mut propagation_events = Vec::new();
    let mut callable_flow_summaries = Vec::new();
    let mut file_records = Vec::new();
    let mut metrics = ParseMetrics::default();

    for (index, (rel_path, parse_result)) in raw_results.into_iter().enumerate() {
        let (mut result, hash, lossy, skipped) = parse_result?;
        if verbose {
            if skipped {
                println!(
                    "  [{}/{}] SKIP: {}: oversized file",
                    index + 1,
                    total,
                    rel_path
                );
                continue;
            }
            if lossy {
                println!(
                    "  [{}/{}] LOSSY: {}: non-UTF8 bytes replaced during parsing",
                    index + 1,
                    total,
                    rel_path
                );
            }
            println!(
                "  [{}/{}] INDEX: {}: {} symbols, {} raw calls",
                index + 1,
                total,
                rel_path,
                result.symbols.len(),
                result.raw_calls.len()
            );
        }
        let mut file_record = FileRecord {
            path: rel_path.clone(),
            content_hash: hash,
            last_indexed: Utc::now().to_rfc3339(),
            symbol_count: result.symbols.len(),
            module: None,
            subsystem: None,
            project_area: None,
            artifact_kind: None,
            header_role: None,
            parse_fragility: None,
            macro_sensitivity: None,
            include_heaviness: None,
        };
        apply_metadata_to_file_record_with_context(&mut file_record, build_metadata);
        apply_risk_signals_to_file_record(&mut file_record, &result.file_risk_signals);
        file_records.push(file_record);
        discard_runtime_only_parse_payload(&mut result);
        extend_non_call_relation_events(&mut relation_events, result.relation_events);
        propagation_events.extend(result.propagation_events);
        callable_flow_summaries.extend(result.callable_flow_summaries);
        metrics.tree_sitter_parse_ms += result.metrics.tree_sitter_parse_ms;
        metrics.syntax_walk_ms += result.metrics.syntax_walk_ms;
        metrics.local_propagation_ms += result.metrics.local_propagation_ms;
        metrics.local_function_discovery_ms += result.metrics.local_function_discovery_ms;
        metrics.local_owner_lookup_ms += result.metrics.local_owner_lookup_ms;
        metrics.local_seed_ms += result.metrics.local_seed_ms;
        metrics.local_event_walk_ms += result.metrics.local_event_walk_ms;
        metrics.local_declaration_ms += result.metrics.local_declaration_ms;
        metrics.local_expression_statement_ms += result.metrics.local_expression_statement_ms;
        metrics.local_return_statement_ms += result.metrics.local_return_statement_ms;
        metrics.local_nested_block_ms += result.metrics.local_nested_block_ms;
        metrics.local_return_collection_ms += result.metrics.local_return_collection_ms;
        metrics.graph_relation_ms += result.metrics.graph_relation_ms;
        metrics.graph_rule_compile_ms += result.metrics.graph_rule_compile_ms;
        metrics.graph_rule_execute_ms += result.metrics.graph_rule_execute_ms;
        metrics.reference_normalization_ms += result.metrics.reference_normalization_ms;
        symbols.extend(result.symbols);
        raw_calls.extend(result.raw_calls);
    }

    Ok((
        symbols,
        raw_calls,
        relation_events,
        propagation_events,
        callable_flow_summaries,
        file_records,
        metrics,
    ))
}

fn read_source_file(path: &Path) -> Result<(String, String, bool), std::io::Error> {
    let bytes = fs::read(path)?;
    let hash = format!("{:x}", Sha256::digest(&bytes));
    match String::from_utf8(bytes) {
        Ok(content) => Ok((content, hash, false)),
        Err(err) => {
            let bytes = err.into_bytes();
            let content = String::from_utf8_lossy(&bytes).into_owned();
            Ok((content, hash, true))
        }
    }
}

pub fn make_relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{
        FileRiskSignals, ParseFragility, ParseMetrics, ParseResult, RawCallSite, Symbol,
        MacroSensitivity, IncludeHeaviness,
    };
    use tempfile::tempdir;

    struct MockLuaAdapter;

    impl LanguageAdapter for MockLuaAdapter {
        fn language(&self) -> SourceLanguage {
            SourceLanguage::Lua
        }

        fn parse_file(&self, file_path: &str, _source: &str, _build_metadata: Option<&BuildMetadataContext>, _workspace_root: Option<&std::path::Path>) -> Result<ParseResult, String> {
            Ok(ParseResult {
                symbols: vec![Symbol {
                    id: "game.update".into(),
                    name: "update".into(),
                    qualified_name: "game.update".into(),
                    symbol_type: "function".into(),
                    file_path: file_path.into(),
                    line: 1,
                    end_line: 1,
                    signature: None,
                    parameter_count: None,
                    scope_qualified_name: None,
                    scope_kind: None,
                    symbol_role: None,
                    declaration_file_path: None,
                    declaration_line: None,
                    declaration_end_line: None,
                    definition_file_path: None,
                    definition_line: None,
                    definition_end_line: None,
                    parent_id: None,
                    module: None,
                    subsystem: None,
                    project_area: None,
                    artifact_kind: None,
                    header_role: None,
                    parse_fragility: None,
                    macro_sensitivity: None,
                    include_heaviness: None,
                }],
                file_risk_signals: FileRiskSignals {
                    parse_fragility: ParseFragility::Low,
                    macro_sensitivity: MacroSensitivity::Low,
                    include_heaviness: IncludeHeaviness::Light,
                },
                relation_events: Vec::new(),
                normalized_references: Vec::new(),
                propagation_events: Vec::new(),
                callable_flow_summaries: Vec::new(),
                raw_calls: Vec::<RawCallSite>::new(),
                metrics: ParseMetrics::default(),
                include_dependencies: Vec::new(),
                macro_definitions: Vec::new(),
                conditional_blocks: Vec::new(),
                dependency_metrics: crate::models::DependencyMetrics::default(),
                conditional_symbols: Vec::new(),
            })
        }
    }

    #[test]
    fn parse_discovered_files_uses_registered_language_adapter() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("game.lua");
        fs::write(&path, "function update() end").unwrap();

        let mut registry = LanguageRegistry::new();
        registry.register(MockLuaAdapter);

        let discovered = vec![DiscoveredSourceFile {
            path: path.clone(),
            language: SourceLanguage::Lua,
        }];

        let (symbols, raw_calls, references, propagation, summaries, files, include_deps, macro_defs, cond_blocks, cond_symbols, metrics) =
            parse_discovered_files(dir.path(), &discovered, false, None, &registry);
        assert!(include_deps.is_empty());
        assert!(macro_defs.is_empty());
        assert!(cond_blocks.is_empty());
        assert!(cond_symbols.is_empty());

        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].file_path, "game.lua");
        assert!(raw_calls.is_empty());
        assert!(references.is_empty());
        assert!(propagation.is_empty());
        assert!(summaries.is_empty());
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "game.lua");
        assert_eq!(metrics, ParseMetrics::default());
    }

    #[test]
    fn parses_stack_size_bytes_with_minimum_guard() {
        assert_eq!(parse_stack_size_bytes("67108864"), Some(67_108_864));
        assert_eq!(parse_stack_size_bytes("2097152"), Some(2_097_152));
        assert_eq!(parse_stack_size_bytes("1048576"), None);
        assert_eq!(parse_stack_size_bytes("abc"), None);
    }

    #[test]
    fn sample_prefix_respects_utf8_boundaries() {
        let content = format!("{}…tail", "a".repeat(CPP_SKIP_CONTENT_SAMPLE_BYTES - 2));
        let sample = sample_prefix(&content, CPP_SKIP_CONTENT_SAMPLE_BYTES);
        assert!(sample.is_char_boundary(sample.len()));
        assert!(!sample.ends_with('…'));
    }

}
