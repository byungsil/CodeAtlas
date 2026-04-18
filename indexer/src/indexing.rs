use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use chrono::Utc;
use rayon::prelude::*;
use sha2::{Digest, Sha256};

use crate::metadata::{apply_metadata_to_file_record, apply_metadata_to_symbol};
use crate::models::{FileRecord, NormalizedReference, ParseResult, RawCallSite, Symbol};
use crate::parser;

pub fn parse_files(
    workspace_root: &Path,
    relative_paths: &[String],
    verbose: bool,
) -> (Vec<Symbol>, Vec<RawCallSite>, Vec<NormalizedReference>, Vec<FileRecord>) {
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
    let mut file_records = Vec::new();

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
                };
                apply_metadata_to_file_record(&mut file_record);
                file_records.push(file_record);
                normalized_references.extend(pr.normalized_references);
                let mut enriched_symbols = pr.symbols;
                for symbol in &mut enriched_symbols {
                    apply_metadata_to_symbol(symbol);
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

    (symbols, raw_calls, normalized_references, file_records)
}

pub fn parse_file_strict(workspace_root: &Path, rel_path: &str) -> Result<(ParseResult, String, bool), String> {
    let abs_path = workspace_root.join(rel_path.replace('/', std::path::MAIN_SEPARATOR_STR));
    let (content, hash, lossy) = read_source_file(&abs_path)
        .map_err(|e| format!("Read error for {}: {}", rel_path, e))?;
    let result = parser::parse_cpp_file(rel_path, &content)
        .map_err(|e| format!("Parse error for {}: {}", rel_path, e))?;
    let mut result = result;
    for symbol in &mut result.symbols {
        apply_metadata_to_symbol(symbol);
    }
    Ok((result, hash, lossy))
}

pub fn parse_files_strict(
    workspace_root: &Path,
    relative_paths: &[String],
    verbose: bool,
) -> Result<(Vec<Symbol>, Vec<RawCallSite>, Vec<NormalizedReference>, Vec<FileRecord>), String> {
    let mut symbols = Vec::new();
    let mut raw_calls = Vec::new();
    let mut normalized_references = Vec::new();
    let mut file_records = Vec::new();
    let total = relative_paths.len();

    for (index, rel_path) in relative_paths.iter().enumerate() {
        let (result, hash, lossy) = parse_file_strict(workspace_root, rel_path)?;
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
        };
        apply_metadata_to_file_record(&mut file_record);
        file_records.push(file_record);
        normalized_references.extend(result.normalized_references);
        symbols.extend(result.symbols);
        raw_calls.extend(result.raw_calls);
    }

    Ok((symbols, raw_calls, normalized_references, file_records))
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
