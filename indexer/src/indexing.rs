use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use chrono::Utc;
use rayon::prelude::*;
use sha2::{Digest, Sha256};

use crate::build_metadata::BuildMetadataContext;
use crate::metadata::{
    apply_metadata_to_file_record_with_context, apply_metadata_to_symbol_with_context,
    apply_risk_signals_to_file_record, apply_risk_signals_to_symbol,
};
use crate::models::{
    CallableFlowSummary, FileRecord, NormalizedReference, ParseMetrics, ParseResult, PropagationEvent,
    RawCallSite, Symbol,
};
use crate::parser;

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
    let total = relative_paths.len();
    let progress = AtomicUsize::new(0);

    let results: Vec<(String, Result<ParseResult, String>, String, bool)> = relative_paths
        .par_iter()
        .map(|rel_path| {
            let abs_path = workspace_root.join(rel_path.replace('/', std::path::MAIN_SEPARATOR_STR));
            let (content, hash, lossy) = match read_source_file(&abs_path) {
                Ok(result) => result,
                Err(e) => return (rel_path.clone(), Err(format!("Read error: {}", e)), String::new(), false),
            };
            let result = parser::parse_cpp_file(rel_path, &content);
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
            (rel_path.clone(), result, hash, lossy)
        })
        .collect();

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
    let abs_path = workspace_root.join(rel_path.replace('/', std::path::MAIN_SEPARATOR_STR));
    let (content, hash, lossy) = read_source_file(&abs_path)
        .map_err(|e| format!("Read error for {}: {}", rel_path, e))?;
    let result = parser::parse_cpp_file(rel_path, &content)
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
    let mut symbols = Vec::new();
    let mut raw_calls = Vec::new();
    let mut normalized_references = Vec::new();
    let mut propagation_events = Vec::new();
    let mut callable_flow_summaries = Vec::new();
    let mut file_records = Vec::new();
    let mut metrics = ParseMetrics::default();
    let total = relative_paths.len();

    for (index, rel_path) in relative_paths.iter().enumerate() {
        let (result, hash, lossy) = parse_file_strict(workspace_root, rel_path, build_metadata)?;
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
