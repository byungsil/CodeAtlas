use std::fs;
use std::path::Path;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};

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
    CallableFlowSummary, FileRecord, NormalizedReference, ParseMetrics, ParseResult, PropagationEvent,
    RawCallSite, Symbol,
};
use crate::parser;
use std::collections::HashMap;

const DEFAULT_INDEXER_WORKER_STACK_BYTES: usize = 64 * 1024 * 1024;
const MIN_INDEXER_WORKER_STACK_BYTES: usize = 2 * 1024 * 1024;
static INDEXER_THREAD_POOL: OnceLock<rayon::ThreadPool> = OnceLock::new();

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

fn parse_stack_size_bytes(value: &str) -> Option<usize> {
    let trimmed = value.trim();
    let bytes = trimmed.parse::<usize>().ok()?;
    (bytes >= MIN_INDEXER_WORKER_STACK_BYTES).then_some(bytes)
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
    let total = discovered_files.len();
    let progress = AtomicUsize::new(0);

    let results: Vec<(String, Result<ParseResult, String>, String, bool)> = indexing_thread_pool().install(|| {
        discovered_files
            .par_iter()
            .map(|discovered| {
                let rel_path = make_relative(workspace_root, &discovered.path);
                let (content, hash, lossy) = match read_source_file(&discovered.path) {
                    Ok(result) => result,
                    Err(e) => return (rel_path.clone(), Err(format!("Read error: {}", e)), String::new(), false),
                };
                let result = registry.parse_file(discovered.language, &rel_path, &content);
                if verbose {
                    let current = progress.fetch_add(1, Ordering::Relaxed) + 1;
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
                            println!("  [{}/{}] FAILED: {}: {}", current, total, rel_path, e);
                        }
                    }
                }
                (rel_path, result, hash, lossy)
            })
            .collect()
    });

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
                    eprintln!("  FAILED: {}: {}", rel_path, e);
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

pub fn parse_file_strict(
    workspace_root: &Path,
    rel_path: &str,
    build_metadata: Option<&BuildMetadataContext>,
) -> Result<(ParseResult, String, bool), String> {
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
) -> Result<(ParseResult, String, bool), String> {
    let rel_path = make_relative(workspace_root, &discovered.path);
    let (content, hash, lossy) = read_source_file(&discovered.path)
        .map_err(|e| format!("Read error for {}: {}", rel_path, e))?;
    let result = registry
        .parse_file(discovered.language, &rel_path, &content)
        .map_err(|e| format!("Parse error for {}: {}", rel_path, e))?;
    let mut result = result;
    for symbol in &mut result.symbols {
        apply_metadata_to_symbol_with_context(symbol, build_metadata);
        apply_risk_signals_to_symbol(symbol, &result.file_risk_signals);
    }
    Ok((result, hash, lossy))
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
        let (result, hash, lossy) =
            parse_discovered_file_strict(workspace_root, discovered, build_metadata, registry)?;
        if verbose {
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
}
