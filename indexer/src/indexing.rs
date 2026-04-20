use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};
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
    CallableFlowSummary, FileRecord, FileRiskSignals, IncludeHeaviness, MacroSensitivity,
    NormalizedReference, ParseFragility, ParseMetrics, ParseResult, PropagationEvent, RawCallSite,
    Symbol,
};
use crate::parser;
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

#[derive(Clone)]
struct ActiveParseEntry {
    language: SourceLanguage,
    started_at: Instant,
}

pub trait LanguageAdapter: Send + Sync {
    fn language(&self) -> SourceLanguage;
    fn parse_file(&self, file_path: &str, source: &str) -> Result<ParseResult, String>;
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

    fn parse_file(&self, file_path: &str, source: &str) -> Result<ParseResult, String> {
        parser::parse_cpp_file(file_path, source)
    }
}

impl LanguageAdapter for LuaLanguageAdapter {
    fn language(&self) -> SourceLanguage {
        SourceLanguage::Lua
    }

    fn parse_file(&self, file_path: &str, source: &str) -> Result<ParseResult, String> {
        lua_parser::parse_lua_file(file_path, source)
    }
}

impl LanguageAdapter for PythonLanguageAdapter {
    fn language(&self) -> SourceLanguage {
        SourceLanguage::Python
    }

    fn parse_file(&self, file_path: &str, source: &str) -> Result<ParseResult, String> {
        python_parser::parse_python_file(file_path, source)
    }
}

impl LanguageAdapter for TypeScriptLanguageAdapter {
    fn language(&self) -> SourceLanguage {
        SourceLanguage::TypeScript
    }

    fn parse_file(&self, file_path: &str, source: &str) -> Result<ParseResult, String> {
        typescript_parser::parse_typescript_file(file_path, source)
    }
}

impl LanguageAdapter for RustLanguageAdapter {
    fn language(&self) -> SourceLanguage {
        SourceLanguage::Rust
    }

    fn parse_file(&self, file_path: &str, source: &str) -> Result<ParseResult, String> {
        rust_parser::parse_rust_file(file_path, source)
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
    ) -> Result<ParseResult, String> {
        let adapter = self
            .adapters
            .get(&language)
            .ok_or_else(|| format!("No language adapter registered for {}", language.display_name()))?;
        adapter.parse_file(file_path, source)
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
        rayon::ThreadPoolBuilder::new()
            .thread_name(|index| format!("codeatlas-index-{}", index))
            .stack_size(configured_indexer_worker_stack_bytes())
            .build()
            .expect("Failed to build CodeAtlas indexing thread pool")
    })
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
    }
}

pub fn parse_files(
    workspace_root: &Path,
    relative_paths: &[String],
    verbose: bool,
    build_metadata: Option<&BuildMetadataContext>,
) -> (
    Vec<Symbol>,
    Vec<RawCallSite>,
    Vec<NormalizedReference>,
    Vec<PropagationEvent>,
    Vec<CallableFlowSummary>,
    Vec<FileRecord>,
    ParseMetrics,
) {
    let registry = default_language_registry();
    let discovered_files: Vec<_> = relative_paths
        .iter()
        .map(|rel_path| DiscoveredSourceFile {
            path: workspace_root.join(rel_path.replace('/', std::path::MAIN_SEPARATOR_STR)),
            language: SourceLanguage::Cpp,
        })
        .collect();
    parse_discovered_files(workspace_root, &discovered_files, verbose, build_metadata, &registry)
}

pub fn parse_discovered_files(
    workspace_root: &Path,
    discovered_files: &[DiscoveredSourceFile],
    verbose: bool,
    build_metadata: Option<&BuildMetadataContext>,
    registry: &LanguageRegistry,
) -> (
    Vec<Symbol>,
    Vec<RawCallSite>,
    Vec<NormalizedReference>,
    Vec<PropagationEvent>,
    Vec<CallableFlowSummary>,
    Vec<FileRecord>,
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
    Vec<NormalizedReference>,
    Vec<PropagationEvent>,
    Vec<CallableFlowSummary>,
    Vec<FileRecord>,
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
                let result = registry.parse_file(discovered.language, &rel_path, &content);
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
    let mut normalized_references = Vec::new();
    let mut propagation_events = Vec::new();
    let mut callable_flow_summaries = Vec::new();
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
                normalized_references.extend(pr.normalized_references);
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
        normalized_references,
        propagation_events,
        callable_flow_summaries,
        file_records,
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
    let result = match registry.parse_file(discovered.language, &rel_path, &content) {
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
    Vec<NormalizedReference>,
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
    Vec<NormalizedReference>,
    Vec<PropagationEvent>,
    Vec<CallableFlowSummary>,
    Vec<FileRecord>,
    ParseMetrics,
), String> {
    let mut symbols = Vec::new();
    let mut raw_calls = Vec::new();
    let mut normalized_references = Vec::new();
    let mut propagation_events = Vec::new();
    let mut callable_flow_summaries = Vec::new();
    let mut file_records = Vec::new();
    let mut metrics = ParseMetrics::default();
    let total = discovered_files.len();

    for (index, discovered) in discovered_files.iter().enumerate() {
        let rel_path = make_relative(workspace_root, &discovered.path);
        let (result, hash, lossy, skipped) =
            parse_discovered_file_strict(workspace_root, discovered, build_metadata, registry)?;
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
        normalized_references.extend(result.normalized_references);
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
        normalized_references,
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

        fn parse_file(&self, file_path: &str, _source: &str) -> Result<ParseResult, String> {
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

        let (symbols, raw_calls, references, propagation, summaries, files, metrics) =
            parse_discovered_files(dir.path(), &discovered, false, None, &registry);

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
