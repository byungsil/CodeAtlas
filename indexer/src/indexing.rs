use std::fs;
use std::path::Path;

use chrono::Utc;
use rayon::prelude::*;
use sha2::{Digest, Sha256};

use crate::models::{FileRecord, ParseResult, RawCallSite, Symbol};
use crate::parser;

pub fn parse_files(
    workspace_root: &Path,
    relative_paths: &[String],
) -> (Vec<Symbol>, Vec<RawCallSite>, Vec<FileRecord>) {
    let results: Vec<(String, Result<ParseResult, String>, String)> = relative_paths
        .par_iter()
        .map(|rel_path| {
            let abs_path = workspace_root.join(rel_path.replace('/', std::path::MAIN_SEPARATOR_STR));
            let content = match fs::read_to_string(&abs_path) {
                Ok(c) => c,
                Err(e) => return (rel_path.clone(), Err(format!("Read error: {}", e)), String::new()),
            };
            let hash = format!("{:x}", Sha256::digest(content.as_bytes()));
            let result = parser::parse_cpp_file(rel_path, &content);
            (rel_path.clone(), result, hash)
        })
        .collect();

    let mut symbols = Vec::new();
    let mut raw_calls = Vec::new();
    let mut file_records = Vec::new();

    for (rel_path, result, hash) in results {
        match result {
            Ok(pr) => {
                println!("  INDEX: {}: {} symbols, {} raw calls", rel_path, pr.symbols.len(), pr.raw_calls.len());
                file_records.push(FileRecord {
                    path: rel_path,
                    content_hash: hash,
                    last_indexed: Utc::now().to_rfc3339(),
                    symbol_count: pr.symbols.len(),
                });
                symbols.extend(pr.symbols);
                raw_calls.extend(pr.raw_calls);
            }
            Err(e) => {
                eprintln!("  FAILED: {}: {}", rel_path, e);
            }
        }
    }

    (symbols, raw_calls, file_records)
}

pub fn parse_file_strict(workspace_root: &Path, rel_path: &str) -> Result<(ParseResult, String), String> {
    let abs_path = workspace_root.join(rel_path.replace('/', std::path::MAIN_SEPARATOR_STR));
    let content = fs::read_to_string(&abs_path)
        .map_err(|e| format!("Read error for {}: {}", rel_path, e))?;
    let hash = format!("{:x}", Sha256::digest(content.as_bytes()));
    let result = parser::parse_cpp_file(rel_path, &content)
        .map_err(|e| format!("Parse error for {}: {}", rel_path, e))?;
    Ok((result, hash))
}

pub fn parse_files_strict(
    workspace_root: &Path,
    relative_paths: &[String],
) -> Result<(Vec<Symbol>, Vec<RawCallSite>, Vec<FileRecord>), String> {
    let mut symbols = Vec::new();
    let mut raw_calls = Vec::new();
    let mut file_records = Vec::new();

    for rel_path in relative_paths {
        let (result, hash) = parse_file_strict(workspace_root, rel_path)?;
        println!(
            "  INDEX: {}: {} symbols, {} raw calls",
            rel_path,
            result.symbols.len(),
            result.raw_calls.len()
        );
        file_records.push(FileRecord {
            path: rel_path.clone(),
            content_hash: hash,
            last_indexed: Utc::now().to_rfc3339(),
            symbol_count: result.symbols.len(),
        });
        symbols.extend(result.symbols);
        raw_calls.extend(result.raw_calls);
    }

    Ok((symbols, raw_calls, file_records))
}

pub fn make_relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}
