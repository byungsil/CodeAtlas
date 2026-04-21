use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::indexing::parse_file_strict;
use crate::models::{FileRecord, Symbol};
use crate::storage::Database;

const BASE_PROJECT_SIZE: usize = 5_000;
const BASE_MASS_CHANGE_ABSOLUTE_THRESHOLD: usize = 200;
const BASE_RENAME_HEAVY_THRESHOLD: usize = 50;

fn compute_mass_change_threshold(total_files: usize) -> usize {
    if total_files <= BASE_PROJECT_SIZE {
        return BASE_MASS_CHANGE_ABSOLUTE_THRESHOLD;
    }
    let scale = ((total_files as f64) / (BASE_PROJECT_SIZE as f64)).sqrt();
    let computed = (BASE_MASS_CHANGE_ABSOLUTE_THRESHOLD as f64 * scale).ceil() as usize;
    env_override_usize("CODEATLAS_ESCALATION_ABSOLUTE", computed)
}

fn compute_rename_heavy_threshold(total_files: usize) -> usize {
    if total_files <= BASE_PROJECT_SIZE {
        return BASE_RENAME_HEAVY_THRESHOLD;
    }
    let scale = ((total_files as f64) / (BASE_PROJECT_SIZE as f64)).sqrt();
    let computed = (BASE_RENAME_HEAVY_THRESHOLD as f64 * scale).ceil() as usize;
    env_override_usize("CODEATLAS_ESCALATION_RENAME", computed)
}

fn env_override_usize(var: &str, computed: usize) -> usize {
    match std::env::var(var) {
        Ok(val) => match val.parse::<usize>() {
            Ok(v) => v,
            Err(_) => computed,
        },
        Err(_) => computed,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct SymbolSignature {
    name: String,
    qualified_name: String,
    symbol_type: String,
    signature: Option<String>,
    parameter_count: Option<usize>,
}

impl SymbolSignature {
    fn from_symbol(symbol: &Symbol) -> Self {
        SymbolSignature {
            name: symbol.name.clone(),
            qualified_name: symbol.qualified_name.clone(),
            symbol_type: symbol.symbol_type.clone(),
            signature: symbol.signature.clone(),
            parameter_count: symbol.parameter_count,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeaderChangeKind {
    BodyOnly,
    SymbolsChanged {
        changed_ids: Vec<String>,
        added_ids: Vec<String>,
        removed_ids: Vec<String>,
    },
    MacroSensitive,
    Unknown,
}

pub fn analyze_header_change(
    old_symbols: &[Symbol],
    new_symbols: &[Symbol],
    macro_sensitivity: Option<&str>,
) -> HeaderChangeKind {
    if macro_sensitivity == Some("high") {
        return HeaderChangeKind::MacroSensitive;
    }

    if new_symbols.iter().any(|s| s.symbol_type.contains("template"))
        || old_symbols.iter().any(|s| s.symbol_type.contains("template"))
    {
        let old_sigs: HashSet<SymbolSignature> =
            old_symbols.iter().map(SymbolSignature::from_symbol).collect();
        let new_sigs: HashSet<SymbolSignature> =
            new_symbols.iter().map(SymbolSignature::from_symbol).collect();
        if old_sigs != new_sigs {
            return HeaderChangeKind::MacroSensitive;
        }
    }

    let old_by_id: HashMap<&str, SymbolSignature> = old_symbols
        .iter()
        .map(|s| (s.id.as_str(), SymbolSignature::from_symbol(s)))
        .collect();
    let new_by_id: HashMap<&str, SymbolSignature> = new_symbols
        .iter()
        .map(|s| (s.id.as_str(), SymbolSignature::from_symbol(s)))
        .collect();

    let mut changed_ids = Vec::new();
    let mut added_ids = Vec::new();
    let mut removed_ids = Vec::new();

    for (id, new_sig) in &new_by_id {
        match old_by_id.get(id) {
            Some(old_sig) if old_sig != new_sig => changed_ids.push(id.to_string()),
            None => added_ids.push(id.to_string()),
            _ => {}
        }
    }
    for id in old_by_id.keys() {
        if !new_by_id.contains_key(id) {
            removed_ids.push(id.to_string());
        }
    }

    if changed_ids.is_empty() && added_ids.is_empty() && removed_ids.is_empty() {
        HeaderChangeKind::BodyOnly
    } else {
        HeaderChangeKind::SymbolsChanged {
            changed_ids,
            added_ids,
            removed_ids,
        }
    }
}

pub fn detect_define_changes(old_source: &str, new_source: &str) -> bool {
    fn extract_defines(source: &str) -> HashSet<String> {
        source
            .lines()
            .filter(|line| {
                let trimmed = line.trim();
                trimmed.starts_with("#define") || trimmed.starts_with("# define")
                    || trimmed.starts_with("#undef") || trimmed.starts_with("# undef")
            })
            .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
            .collect()
    }
    extract_defines(old_source) != extract_defines(new_source)
}

enum HeaderAnalysisResult {
    RequiresFullDiscovery { reason: String },
    Narrow {
        headers_to_index: Vec<String>,
        extra_files_to_index: Vec<String>,
    },
}

fn analyze_changed_headers(
    header_paths: &[String],
    workspace_root: &Path,
    db: &Database,
) -> Result<HeaderAnalysisResult, String> {
    let mut headers_to_index: Vec<String> = Vec::new();
    let mut extra_files_to_index: Vec<String> = Vec::new();

    for header_path in header_paths {
        let abs_path = workspace_root.join(header_path.replace('/', std::path::MAIN_SEPARATOR_STR));

        // If the header was deleted, treat as normal delete — no fanout needed
        if !abs_path.exists() {
            continue;
        }

        // Read new source to check for #define changes
        let new_source = fs::read_to_string(&abs_path)
            .map_err(|e| format!("Failed to read header {}: {}", header_path, e))?;

        // Read old source from DB hash (not available directly, get from file record)
        // We can only detect define changes by reading old source if we stored it.
        // We don't store raw source, so we compare new source to detect if defines exist at all.
        // Instead: re-parse header and compare symbol signatures with DB.

        // Get macro_sensitivity from DB file record
        let macro_sensitivity = db
            .read_file_records()
            .ok()
            .and_then(|records| records.into_iter().find(|r| r.path == *header_path))
            .and_then(|r| r.macro_sensitivity);

        if macro_sensitivity.as_deref() == Some("high") {
            return Ok(HeaderAnalysisResult::RequiresFullDiscovery {
                reason: format!("header {} has high macro sensitivity", header_path),
            });
        }

        // Get old symbols from DB
        let old_symbols = db
            .read_raw_symbols_for_file(header_path)
            .map_err(|e| format!("Failed to read old symbols for {}: {}", header_path, e))?;

        // Re-parse header to get new symbols
        let parse_result = parse_file_strict(workspace_root, header_path, None);
        let new_symbols = match parse_result {
            Ok((result, _, _, _)) => result.symbols,
            Err(_) => {
                return Ok(HeaderAnalysisResult::RequiresFullDiscovery {
                    reason: format!("failed to parse header {}", header_path),
                });
            }
        };

        // Check for #define changes (conservative: if new source has any #define, check carefully)
        // We compare old source via reading if we have it. Since we don't store old source,
        // we use a heuristic: if any symbols changed AND defines exist in the file, fallback.
        let has_defines = new_source
            .lines()
            .any(|l| l.trim().starts_with("#define") || l.trim().starts_with("#undef"));

        let change_kind = analyze_header_change(&old_symbols, &new_symbols, macro_sensitivity.as_deref());

        match change_kind {
            HeaderChangeKind::BodyOnly => {
                // Body-only: re-index the header itself but no fanout
                headers_to_index.push(header_path.clone());
            }
            HeaderChangeKind::SymbolsChanged { changed_ids, added_ids, removed_ids } => {
                if has_defines {
                    // Conservative: if header has #defines and symbols changed, full discovery
                    return Ok(HeaderAnalysisResult::RequiresFullDiscovery {
                        reason: format!(
                            "header {} has #define directives and symbol changes",
                            header_path
                        ),
                    });
                }
                headers_to_index.push(header_path.clone());
                // Find files that reference the changed/removed symbols
                let affected_ids: Vec<String> = changed_ids
                    .into_iter()
                    .chain(removed_ids)
                    .collect();
                if !affected_ids.is_empty() {
                    let referencing = db
                        .read_files_referencing_symbols(&affected_ids)
                        .map_err(|e| format!("Failed to query referencing files: {}", e))?;
                    for f in referencing {
                        if f != *header_path && !extra_files_to_index.contains(&f) {
                            extra_files_to_index.push(f);
                        }
                    }
                }
                // added_ids: new symbols — no existing references, skip
                let _ = added_ids;
            }
            HeaderChangeKind::MacroSensitive | HeaderChangeKind::Unknown => {
                return Ok(HeaderAnalysisResult::RequiresFullDiscovery {
                    reason: format!("header {} requires conservative fanout", header_path),
                });
            }
        }
    }

    Ok(HeaderAnalysisResult::Narrow {
        headers_to_index,
        extra_files_to_index,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanDisposition {
    Index,
    Delete,
    Unchanged,
}

impl PlanDisposition {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Index => "index",
            Self::Delete => "delete",
            Self::Unchanged => "unchanged",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanReason {
    NewPath,
    ContentChanged,
    ReadFailed,
    RemovedPath,
    UnchangedContent,
    HeaderFanout,
    RenameMoveSource,
    RenameMoveDestination,
}

impl PlanReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::NewPath => "new_path",
            Self::ContentChanged => "content_changed",
            Self::ReadFailed => "read_failed",
            Self::RemovedPath => "removed_path",
            Self::UnchangedContent => "unchanged_content",
            Self::HeaderFanout => "header_fanout",
            Self::RenameMoveSource => "rename_move_source",
            Self::RenameMoveDestination => "rename_move_destination",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanEntry {
    pub path: String,
    pub disposition: PlanDisposition,
    pub reason: PlanReason,
    pub content_hash: Option<String>,
    pub matched_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenameHint {
    pub from_path: String,
    pub to_path: String,
    pub content_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EscalationLevel {
    Incremental,
    FullRebuild,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EscalationDecision {
    pub level: EscalationLevel,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncrementalPlan {
    pub to_index: Vec<String>,
    pub to_delete: Vec<String>,
    pub unchanged: Vec<String>,
    pub entries: Vec<PlanEntry>,
    pub rename_hints: Vec<RenameHint>,
}

impl IncrementalPlan {
    #[cfg(test)]
    pub fn entry_for_path(&self, path: &str) -> Option<&PlanEntry> {
        self.entries.iter().find(|entry| entry.path == path)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangedSetPlanResult {
    Narrow(IncrementalPlan),
    RequiresFullDiscovery { reason: String },
}

const MASS_CHANGE_PERCENT_THRESHOLD: usize = 40;
const MASS_CHANGE_MIN_TOTAL_FILES: usize = 20;

pub fn plan(
    disk_files: &[String],
    stored_records: &[FileRecord],
    workspace_root: &Path,
) -> IncrementalPlan {
    let stored_map: HashMap<&str, &FileRecord> = stored_records
        .iter()
        .map(|r| (r.path.as_str(), r))
        .collect();

    let disk_set: HashSet<&str> = disk_files.iter().map(|s| s.as_str()).collect();

    let mut to_index = Vec::new();
    let mut unchanged = Vec::new();
    let mut entries = Vec::new();
    let mut disk_hashes: HashMap<String, String> = HashMap::new();

    for rel_path in disk_files {
        let abs_path = workspace_root.join(rel_path.replace('/', std::path::MAIN_SEPARATOR_STR));
        let current_hash = match hash_file_bytes(&abs_path) {
            Ok(hash) => hash,
            Err(_) => {
                to_index.push(rel_path.clone());
                entries.push(PlanEntry {
                    path: rel_path.clone(),
                    disposition: PlanDisposition::Index,
                    reason: PlanReason::ReadFailed,
                    content_hash: None,
                    matched_path: None,
                });
                continue;
            }
        };
        disk_hashes.insert(rel_path.clone(), current_hash.clone());

        match stored_map.get(rel_path.as_str()) {
            Some(stored_record) if stored_record.content_hash == current_hash => {
                unchanged.push(rel_path.clone());
                entries.push(PlanEntry {
                    path: rel_path.clone(),
                    disposition: PlanDisposition::Unchanged,
                    reason: PlanReason::UnchangedContent,
                    content_hash: Some(current_hash),
                    matched_path: None,
                });
            }
            _ => {
                let reason = if stored_map.contains_key(rel_path.as_str()) {
                    PlanReason::ContentChanged
                } else {
                    PlanReason::NewPath
                };
                to_index.push(rel_path.clone());
                entries.push(PlanEntry {
                    path: rel_path.clone(),
                    disposition: PlanDisposition::Index,
                    reason,
                    content_hash: Some(current_hash),
                    matched_path: None,
                });
            }
        }
    }

    let to_delete: Vec<String> = stored_records
        .iter()
        .filter(|r| !disk_set.contains(r.path.as_str()))
        .map(|r| r.path.clone())
        .collect();

    for path in &to_delete {
        let content_hash = stored_map.get(path.as_str()).map(|record| record.content_hash.clone());
        entries.push(PlanEntry {
            path: path.clone(),
            disposition: PlanDisposition::Delete,
            reason: PlanReason::RemovedPath,
            content_hash,
            matched_path: None,
        });
    }

    let rename_hints = attach_rename_hints(&mut entries, &disk_hashes, stored_records);
    apply_header_fanout(&mut to_index, &mut unchanged, &mut entries, disk_files, workspace_root);
    to_index.sort();
    to_index.dedup();
    unchanged.sort();
    unchanged.dedup();
    entries.sort_by(|a, b| a.path.cmp(&b.path));

    IncrementalPlan {
        to_index,
        to_delete,
        unchanged,
        entries,
        rename_hints,
    }
}

pub fn plan_from_changed_paths(
    changed_paths: &[String],
    stored_records: &[FileRecord],
    workspace_root: &Path,
    db: Option<&Database>,
) -> ChangedSetPlanResult {
    let mut unique_paths: Vec<String> = changed_paths.to_vec();
    unique_paths.sort();
    unique_paths.dedup();

    let (header_paths, non_header_paths): (Vec<String>, Vec<String>) =
        unique_paths.into_iter().partition(|p| is_header_path(p));

    // For headers, attempt smart analysis if DB is available
    if !header_paths.is_empty() {
        match db {
            None => {
                return ChangedSetPlanResult::RequiresFullDiscovery {
                    reason: "header change requires wider incremental discovery".into(),
                };
            }
            Some(db) => {
                match analyze_changed_headers(&header_paths, workspace_root, db) {
                    Ok(HeaderAnalysisResult::RequiresFullDiscovery { reason }) => {
                        return ChangedSetPlanResult::RequiresFullDiscovery { reason };
                    }
                    Ok(HeaderAnalysisResult::Narrow {
                        headers_to_index,
                        extra_files_to_index,
                    }) => {
                        // Combine header results with non-header paths and proceed
                        let all_paths: Vec<String> = headers_to_index
                            .into_iter()
                            .chain(extra_files_to_index)
                            .chain(non_header_paths.clone())
                            .collect();
                        return plan_from_changed_paths(&all_paths, stored_records, workspace_root, None);
                    }
                    Err(_) => {
                        return ChangedSetPlanResult::RequiresFullDiscovery {
                            reason: "header analysis failed, falling back to full discovery".into(),
                        };
                    }
                }
            }
        }
    }

    let unique_paths = non_header_paths;

    let stored_map: HashMap<&str, &FileRecord> = stored_records
        .iter()
        .map(|record| (record.path.as_str(), record))
        .collect();

    let mut to_index = Vec::new();
    let mut to_delete = Vec::new();
    let mut unchanged = Vec::new();
    let mut entries = Vec::new();
    let mut disk_hashes: HashMap<String, String> = HashMap::new();

    for rel_path in unique_paths {
        let abs_path = workspace_root.join(rel_path.replace('/', std::path::MAIN_SEPARATOR_STR));
        if abs_path.exists() {
            let current_hash = match hash_file_bytes(&abs_path) {
                Ok(hash) => hash,
                Err(_) => {
                    to_index.push(rel_path.clone());
                    entries.push(PlanEntry {
                        path: rel_path,
                        disposition: PlanDisposition::Index,
                        reason: PlanReason::ReadFailed,
                        content_hash: None,
                        matched_path: None,
                    });
                    continue;
                }
            };
            disk_hashes.insert(rel_path.clone(), current_hash.clone());

            match stored_map.get(rel_path.as_str()) {
                Some(stored_record) if stored_record.content_hash == current_hash => {
                    unchanged.push(rel_path.clone());
                    entries.push(PlanEntry {
                        path: rel_path,
                        disposition: PlanDisposition::Unchanged,
                        reason: PlanReason::UnchangedContent,
                        content_hash: Some(current_hash),
                        matched_path: None,
                    });
                }
                Some(_) => {
                    to_index.push(rel_path.clone());
                    entries.push(PlanEntry {
                        path: rel_path,
                        disposition: PlanDisposition::Index,
                        reason: PlanReason::ContentChanged,
                        content_hash: Some(current_hash),
                        matched_path: None,
                    });
                }
                None => {
                    to_index.push(rel_path.clone());
                    entries.push(PlanEntry {
                        path: rel_path,
                        disposition: PlanDisposition::Index,
                        reason: PlanReason::NewPath,
                        content_hash: Some(current_hash),
                        matched_path: None,
                    });
                }
            }
        } else if let Some(stored_record) = stored_map.get(rel_path.as_str()) {
            to_delete.push(rel_path.clone());
            entries.push(PlanEntry {
                path: rel_path,
                disposition: PlanDisposition::Delete,
                reason: PlanReason::RemovedPath,
                content_hash: Some(stored_record.content_hash.clone()),
                matched_path: None,
            });
        }
    }

    let rename_hints = attach_rename_hints(&mut entries, &disk_hashes, stored_records);

    ChangedSetPlanResult::Narrow(IncrementalPlan {
        to_index,
        to_delete,
        unchanged,
        entries,
        rename_hints,
    })
}

pub fn assess_escalation(total_files: usize, plan: &IncrementalPlan) -> EscalationDecision {
    let changed = plan.to_index.len() + plan.to_delete.len();
    if changed == 0 {
        return EscalationDecision {
            level: EscalationLevel::Incremental,
            reason: None,
        };
    }

    let rename_threshold = compute_rename_heavy_threshold(total_files);
    if plan.rename_hints.len() >= rename_threshold {
        return EscalationDecision {
            level: EscalationLevel::FullRebuild,
            reason: Some(format!(
                "rename-heavy churn detected ({} rename/move hints, threshold {} for {} files)",
                plan.rename_hints.len(), rename_threshold, total_files
            )),
        };
    }

    let mass_change_threshold = compute_mass_change_threshold(total_files);
    if changed >= mass_change_threshold {
        return EscalationDecision {
            level: EscalationLevel::FullRebuild,
            reason: Some(format!(
                "mass change detected ({} files changed, threshold {} for {} files)",
                changed, mass_change_threshold, total_files
            )),
        };
    }

    if total_files >= MASS_CHANGE_MIN_TOTAL_FILES {
        let changed_percent = changed * 100 / total_files.max(1);
        if changed_percent >= MASS_CHANGE_PERCENT_THRESHOLD {
            return EscalationDecision {
                level: EscalationLevel::FullRebuild,
                reason: Some(format!(
                    "branch-like churn detected ({} of {} files changed, {}%)",
                    changed, total_files, changed_percent
                )),
            };
        }
    }

    EscalationDecision {
        level: EscalationLevel::Incremental,
        reason: None,
    }
}

fn hash_file_bytes(path: &Path) -> std::io::Result<String> {
    let bytes = fs::read(path)?;
    Ok(format!("{:x}", Sha256::digest(&bytes)))
}

fn attach_rename_hints(
    entries: &mut [PlanEntry],
    disk_hashes: &HashMap<String, String>,
    stored_records: &[FileRecord],
) -> Vec<RenameHint> {
    let mut deleted_by_hash: HashMap<String, Vec<String>> = HashMap::new();
    for entry in entries.iter() {
        if entry.disposition == PlanDisposition::Delete {
            if let Some(hash) = entry.content_hash.as_deref() {
                deleted_by_hash
                    .entry(hash.to_string())
                    .or_default()
                    .push(entry.path.clone());
            }
        }
    }

    let stored_paths: HashSet<&str> = stored_records.iter().map(|record| record.path.as_str()).collect();
    let mut rename_hints = Vec::new();
    let mut claimed_sources: HashSet<String> = HashSet::new();

    for entry in entries.iter_mut() {
        if entry.disposition != PlanDisposition::Index || stored_paths.contains(entry.path.as_str()) {
            continue;
        }

        let Some(hash) = disk_hashes.get(&entry.path) else {
            continue;
        };
        let Some(candidates) = deleted_by_hash.get(hash) else {
            continue;
        };

        let Some(source_path) = candidates.iter().find(|path| !claimed_sources.contains(path.as_str())) else {
            continue;
        };

        claimed_sources.insert(source_path.clone());
        entry.reason = PlanReason::RenameMoveDestination;
        entry.matched_path = Some(source_path.clone());
        rename_hints.push(RenameHint {
            from_path: source_path.clone(),
            to_path: entry.path.clone(),
            content_hash: hash.clone(),
        });
    }

    for hint in &rename_hints {
        if let Some(source_entry) = entries.iter_mut().find(|entry| entry.path == hint.from_path) {
            source_entry.reason = PlanReason::RenameMoveSource;
            source_entry.matched_path = Some(hint.to_path.clone());
        }
    }

    rename_hints.sort_by(|a, b| a.from_path.cmp(&b.from_path).then(a.to_path.cmp(&b.to_path)));
    rename_hints
}

fn apply_header_fanout(
    to_index: &mut Vec<String>,
    unchanged: &mut Vec<String>,
    entries: &mut [PlanEntry],
    disk_files: &[String],
    workspace_root: &Path,
) {
    let changed_headers: Vec<String> = entries
        .iter()
        .filter(|entry| {
            entry.disposition != PlanDisposition::Unchanged && is_header_path(&entry.path)
        })
        .map(|entry| entry.path.clone())
        .collect();

    if changed_headers.is_empty() {
        return;
    }

    let reverse_include_graph = build_reverse_include_graph(disk_files, workspace_root);
    let mut queue = changed_headers.clone();
    let mut visited: HashSet<String> = changed_headers.into_iter().collect();
    let mut fanout_sources: HashMap<String, String> = HashMap::new();

    while let Some(current) = queue.pop() {
        let Some(includers) = reverse_include_graph.get(&current) else {
            continue;
        };

        for includer in includers {
            if visited.insert(includer.clone()) {
                fanout_sources.insert(includer.clone(), current.clone());
                queue.push(includer.clone());
            }
        }
    }

    for entry in entries.iter_mut() {
        if entry.disposition != PlanDisposition::Unchanged {
            continue;
        }
        let Some(source_path) = fanout_sources.get(&entry.path) else {
            continue;
        };

        entry.disposition = PlanDisposition::Index;
        entry.reason = PlanReason::HeaderFanout;
        entry.matched_path = Some(source_path.clone());
        to_index.push(entry.path.clone());
    }

    let fanout_paths: HashSet<&str> = fanout_sources.keys().map(|path| path.as_str()).collect();
    unchanged.retain(|path| !fanout_paths.contains(path.as_str()));
}

fn build_reverse_include_graph(
    disk_files: &[String],
    workspace_root: &Path,
) -> HashMap<String, Vec<String>> {
    let disk_set: HashSet<&str> = disk_files.iter().map(|path| path.as_str()).collect();
    let mut reverse_graph: HashMap<String, Vec<String>> = HashMap::new();

    for rel_path in disk_files {
        let abs_path = workspace_root.join(rel_path.replace('/', std::path::MAIN_SEPARATOR_STR));
        let Ok(source) = fs::read_to_string(&abs_path) else {
            continue;
        };

        for included in scan_local_includes(rel_path, &source) {
            if disk_set.contains(included.as_str()) {
                reverse_graph.entry(included).or_default().push(rel_path.clone());
            }
        }
    }

    for includers in reverse_graph.values_mut() {
        includers.sort();
        includers.dedup();
    }

    reverse_graph
}

fn scan_local_includes(rel_path: &str, source: &str) -> Vec<String> {
    let mut includes = Vec::new();
    let base_dir = Path::new(rel_path).parent().unwrap_or_else(|| Path::new(""));

    for line in source.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("#include \"") {
            continue;
        }
        let Some(rest) = trimmed.strip_prefix("#include \"") else {
            continue;
        };
        let Some(include_target) = rest.split('"').next() else {
            continue;
        };

        let normalized = normalize_relative_like(base_dir.join(include_target).to_string_lossy().as_ref());
        includes.push(normalized);
    }

    includes
}

fn normalize_relative_like(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    let mut normalized_parts = Vec::new();
    for part in normalized.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                normalized_parts.pop();
            }
            other => normalized_parts.push(other),
        }
    }
    normalized_parts.join("/")
}

fn is_header_path(path: &str) -> bool {
    matches!(
        Path::new(path).extension().and_then(|ext| ext.to_str()),
        Some("h" | "hpp" | "inl" | "inc")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_record(path: &str, hash: &str) -> FileRecord {
        FileRecord {
            path: path.into(),
            content_hash: hash.into(),
            last_indexed: "2026-01-01T00:00:00Z".into(),
            symbol_count: 1,
            module: None,
            subsystem: None,
            project_area: None,
            artifact_kind: None,
            header_role: None,
            parse_fragility: None,
            macro_sensitivity: None,
            include_heaviness: None,
        }
    }

    #[test]
    fn detects_new_files() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("new.cpp"), "void f() {}").unwrap();

        let disk = vec!["new.cpp".to_string()];
        let stored: Vec<FileRecord> = vec![];
        let plan = plan(&disk, &stored, dir.path());

        assert_eq!(plan.to_index, vec!["new.cpp"]);
        assert!(plan.to_delete.is_empty());
        assert!(plan.unchanged.is_empty());
        assert_eq!(
            plan.entry_for_path("new.cpp").map(|entry| &entry.reason),
            Some(&PlanReason::NewPath)
        );
    }

    #[test]
    fn detects_unchanged_files() {
        let dir = tempfile::tempdir().unwrap();
        let content = "void f() {}";
        fs::write(dir.path().join("same.cpp"), content).unwrap();
        let hash = format!("{:x}", Sha256::digest(content.as_bytes()));

        let disk = vec!["same.cpp".to_string()];
        let stored = vec![make_record("same.cpp", &hash)];
        let plan = plan(&disk, &stored, dir.path());

        assert!(plan.to_index.is_empty());
        assert!(plan.to_delete.is_empty());
        assert_eq!(plan.unchanged, vec!["same.cpp"]);
        assert_eq!(
            plan.entry_for_path("same.cpp").map(|entry| &entry.reason),
            Some(&PlanReason::UnchangedContent)
        );
    }

    #[test]
    fn detects_changed_files() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("changed.cpp"), "void g() {}").unwrap();

        let disk = vec!["changed.cpp".to_string()];
        let stored = vec![make_record("changed.cpp", "old_hash")];
        let plan = plan(&disk, &stored, dir.path());

        assert_eq!(plan.to_index, vec!["changed.cpp"]);
        assert!(plan.to_delete.is_empty());
        assert!(plan.unchanged.is_empty());
        assert_eq!(
            plan.entry_for_path("changed.cpp").map(|entry| &entry.reason),
            Some(&PlanReason::ContentChanged)
        );
    }

    #[test]
    fn detects_deleted_files() {
        let dir = tempfile::tempdir().unwrap();

        let disk: Vec<String> = vec![];
        let stored = vec![make_record("gone.cpp", "hash")];
        let plan = plan(&disk, &stored, dir.path());

        assert!(plan.to_index.is_empty());
        assert_eq!(plan.to_delete, vec!["gone.cpp"]);
        assert!(plan.unchanged.is_empty());
        assert_eq!(
            plan.entry_for_path("gone.cpp").map(|entry| &entry.reason),
            Some(&PlanReason::RemovedPath)
        );
    }

    #[test]
    fn mixed_scenario() {
        let dir = tempfile::tempdir().unwrap();
        let unchanged_content = "// unchanged";
        let unchanged_hash = format!("{:x}", Sha256::digest(unchanged_content.as_bytes()));
        fs::write(dir.path().join("keep.cpp"), unchanged_content).unwrap();
        fs::write(dir.path().join("edit.cpp"), "// edited").unwrap();
        fs::write(dir.path().join("add.cpp"), "// new").unwrap();

        let disk = vec!["keep.cpp".into(), "edit.cpp".into(), "add.cpp".into()];
        let stored = vec![
            make_record("keep.cpp", &unchanged_hash),
            make_record("edit.cpp", "stale_hash"),
            make_record("removed.cpp", "dead_hash"),
        ];
        let plan = plan(&disk, &stored, dir.path());

        assert_eq!(plan.unchanged, vec!["keep.cpp"]);
        assert!(plan.to_index.contains(&"edit.cpp".to_string()));
        assert!(plan.to_index.contains(&"add.cpp".to_string()));
        assert_eq!(plan.to_delete, vec!["removed.cpp"]);
    }

    #[test]
    fn detects_content_assisted_rename_hints() {
        let dir = tempfile::tempdir().unwrap();
        let content = "void f() {}\n";
        fs::create_dir_all(dir.path().join("next")).unwrap();
        fs::write(dir.path().join("next").join("renamed.cpp"), content).unwrap();
        let hash = format!("{:x}", Sha256::digest(content.as_bytes()));

        let disk = vec!["next/renamed.cpp".to_string()];
        let stored = vec![make_record("old.cpp", &hash)];
        let plan = plan(&disk, &stored, dir.path());

        assert_eq!(plan.to_index, vec!["next/renamed.cpp"]);
        assert_eq!(plan.to_delete, vec!["old.cpp"]);
        assert_eq!(
            plan.entry_for_path("next/renamed.cpp").map(|entry| (&entry.reason, entry.matched_path.as_deref())),
            Some((&PlanReason::RenameMoveDestination, Some("old.cpp")))
        );
        assert_eq!(
            plan.entry_for_path("old.cpp").map(|entry| (&entry.reason, entry.matched_path.as_deref())),
            Some((&PlanReason::RenameMoveSource, Some("next/renamed.cpp")))
        );
        assert_eq!(
            plan.rename_hints,
            vec![RenameHint {
                from_path: "old.cpp".into(),
                to_path: "next/renamed.cpp".into(),
                content_hash: hash,
            }]
        );
    }

    #[test]
    fn hashes_non_utf8_files_by_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let bytes = vec![0xff, 0xfe, b'c', b'p', b'p'];
        fs::write(dir.path().join("lossy.cpp"), &bytes).unwrap();
        let hash = format!("{:x}", Sha256::digest(&bytes));

        let disk = vec!["lossy.cpp".to_string()];
        let stored = vec![make_record("lossy.cpp", &hash)];
        let plan = plan(&disk, &stored, dir.path());

        assert!(plan.to_index.is_empty());
        assert_eq!(plan.unchanged, vec!["lossy.cpp"]);
        assert_eq!(
            plan.entry_for_path("lossy.cpp")
                .and_then(|entry| entry.content_hash.as_deref()),
            Some(hash.as_str())
        );
    }

    #[test]
    fn header_change_fans_out_to_transitive_includers() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("api.h"), "void Stable();\n").unwrap();
        fs::write(dir.path().join("sub").join("wrapper.h"), "#include \"../api.h\"\n").unwrap();
        fs::write(
            dir.path().join("main.cpp"),
            "#include \"sub/wrapper.h\"\nint main() { return 0; }\n",
        )
        .unwrap();

        let old_hash = "stale_hash";
        let new_hash = hash_file_bytes(&dir.path().join("api.h")).unwrap();
        let wrapper_hash = hash_file_bytes(&dir.path().join("sub").join("wrapper.h")).unwrap();
        let main_hash = hash_file_bytes(&dir.path().join("main.cpp")).unwrap();

        let disk = vec![
            "api.h".to_string(),
            "sub/wrapper.h".to_string(),
            "main.cpp".to_string(),
        ];
        let stored = vec![
            make_record("api.h", old_hash),
            make_record("sub/wrapper.h", &wrapper_hash),
            make_record("main.cpp", &main_hash),
        ];
        let plan = plan(&disk, &stored, dir.path());

        assert!(plan.to_index.contains(&"api.h".to_string()));
        assert!(plan.to_index.contains(&"sub/wrapper.h".to_string()));
        assert!(plan.to_index.contains(&"main.cpp".to_string()));
        assert_eq!(
            plan.entry_for_path("sub/wrapper.h").map(|entry| (&entry.reason, entry.matched_path.as_deref())),
            Some((&PlanReason::HeaderFanout, Some("api.h")))
        );
        assert_eq!(
            plan.entry_for_path("main.cpp").map(|entry| (&entry.reason, entry.matched_path.as_deref())),
            Some((&PlanReason::HeaderFanout, Some("sub/wrapper.h")))
        );
        assert_eq!(
            plan.entry_for_path("api.h").and_then(|entry| entry.content_hash.as_deref()),
            Some(new_hash.as_str())
        );
    }

    #[test]
    fn escalation_prefers_full_rebuild_for_branch_like_percentage_churn() {
        let plan = IncrementalPlan {
            to_index: (0..10).map(|i| format!("changed_{i}.cpp")).collect(),
            to_delete: Vec::new(),
            unchanged: (0..10).map(|i| format!("same_{i}.cpp")).collect(),
            entries: Vec::new(),
            rename_hints: Vec::new(),
        };

        let decision = assess_escalation(20, &plan);
        assert_eq!(decision.level, EscalationLevel::FullRebuild);
        assert!(decision.reason.unwrap().contains("branch-like churn"));
    }

    #[test]
    fn escalation_prefers_full_rebuild_for_rename_heavy_churn() {
        let plan = IncrementalPlan {
            to_index: vec!["new.cpp".into()],
            to_delete: vec!["old.cpp".into()],
            unchanged: vec![],
            entries: Vec::new(),
            rename_hints: (0..BASE_RENAME_HEAVY_THRESHOLD)
                .map(|i| RenameHint {
                    from_path: format!("from_{i}.cpp"),
                    to_path: format!("to_{i}.cpp"),
                    content_hash: format!("hash_{i}"),
                })
                .collect(),
        };

        let decision = assess_escalation(200, &plan);
        assert_eq!(decision.level, EscalationLevel::FullRebuild);
        assert!(decision.reason.unwrap().contains("rename-heavy churn"));
    }

    #[test]
    fn escalation_keeps_small_changes_incremental() {
        let plan = IncrementalPlan {
            to_index: vec!["edit.cpp".into()],
            to_delete: Vec::new(),
            unchanged: (0..50).map(|i| format!("same_{i}.cpp")).collect(),
            entries: Vec::new(),
            rename_hints: Vec::new(),
        };

        let decision = assess_escalation(51, &plan);
        assert_eq!(decision.level, EscalationLevel::Incremental);
        assert!(decision.reason.is_none());
    }

    #[test]
    fn changed_set_plan_uses_only_touched_cpp_paths() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("keep.cpp"), "void keep() {}\n").unwrap();
        fs::write(dir.path().join("edit.cpp"), "void newer() {}\n").unwrap();
        let keep_hash = hash_file_bytes(&dir.path().join("keep.cpp")).unwrap();

        let stored = vec![
            make_record("keep.cpp", &keep_hash),
            make_record("edit.cpp", "stale_hash"),
        ];

        let result = plan_from_changed_paths(&["edit.cpp".into()], &stored, dir.path(), None);
        let ChangedSetPlanResult::Narrow(plan) = result else {
            panic!("expected narrow plan");
        };

        assert_eq!(plan.to_index, vec!["edit.cpp"]);
        assert!(plan.to_delete.is_empty());
        assert!(plan.unchanged.is_empty());
    }

    #[test]
    fn changed_set_plan_falls_back_for_header_changes_without_db() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("api.h"), "void f();\n").unwrap();

        // Without DB, header changes always fall back to full discovery
        let result = plan_from_changed_paths(&["api.h".into()], &[], dir.path(), None);
        assert!(matches!(result, ChangedSetPlanResult::RequiresFullDiscovery { .. }));
    }

    #[test]
    fn changed_set_plan_supports_cpp_rename_without_full_discovery() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("next")).unwrap();
        let content = "void f() {}\n";
        fs::write(dir.path().join("next").join("renamed.cpp"), content).unwrap();
        let hash = format!("{:x}", Sha256::digest(content.as_bytes()));
        let stored = vec![make_record("old.cpp", &hash)];

        let result = plan_from_changed_paths(
            &["old.cpp".into(), "next/renamed.cpp".into()],
            &stored,
            dir.path(),
            None,
        );
        let ChangedSetPlanResult::Narrow(plan) = result else {
            panic!("expected narrow plan");
        };

        assert_eq!(plan.to_index, vec!["next/renamed.cpp"]);
        assert_eq!(plan.to_delete, vec!["old.cpp"]);
        assert_eq!(plan.rename_hints.len(), 1);
    }

    #[test]
    fn compute_mass_change_threshold_uses_base_for_small_projects() {
        assert_eq!(compute_mass_change_threshold(0), 200);
        assert_eq!(compute_mass_change_threshold(1000), 200);
        assert_eq!(compute_mass_change_threshold(5000), 200);
    }

    #[test]
    fn compute_mass_change_threshold_scales_for_large_projects() {
        // 10K: 200 * sqrt(10000/5000) = 200 * 1.414 = 282.8 → ceil = 283
        assert_eq!(compute_mass_change_threshold(10_000), 283);
        // 35K: 200 * sqrt(35000/5000) = 200 * 2.646 = 529.1 → ceil = 530
        let t35k = compute_mass_change_threshold(35_000);
        assert!(t35k >= 529 && t35k <= 531, "35K threshold was {}", t35k);
        // 100K: 200 * sqrt(100000/5000) = 200 * 4.472 = 894.4 → ceil = 895
        let t100k = compute_mass_change_threshold(100_000);
        assert!(t100k >= 894 && t100k <= 896, "100K threshold was {}", t100k);
    }

    #[test]
    fn compute_rename_heavy_threshold_uses_base_for_small_projects() {
        assert_eq!(compute_rename_heavy_threshold(0), 50);
        assert_eq!(compute_rename_heavy_threshold(5000), 50);
    }

    #[test]
    fn compute_rename_heavy_threshold_scales_for_large_projects() {
        // 35K: 50 * sqrt(35000/5000) ≈ 132.3 → ceil = 133
        let t35k = compute_rename_heavy_threshold(35_000);
        assert!(t35k >= 132 && t35k <= 134, "35K rename threshold was {}", t35k);
    }

    #[test]
    fn assess_escalation_stays_incremental_for_large_project_500_changes() {
        let plan = IncrementalPlan {
            to_index: (0..499).map(|i| format!("file_{i}.cpp")).collect(),
            to_delete: vec!["old.cpp".into()],
            unchanged: vec![],
            entries: vec![],
            rename_hints: vec![],
        };
        // 500 changes in a 35K project should stay incremental (threshold ~530)
        let decision = assess_escalation(35_000, &plan);
        assert_eq!(decision.level, EscalationLevel::Incremental);
    }

    #[test]
    fn assess_escalation_triggers_rebuild_for_small_project_200_changes() {
        let plan = IncrementalPlan {
            to_index: (0..199).map(|i| format!("file_{i}.cpp")).collect(),
            to_delete: vec!["old.cpp".into()],
            unchanged: vec![],
            entries: vec![],
            rename_hints: vec![],
        };
        // 200 changes in a 5K project should trigger rebuild (threshold = 200)
        let decision = assess_escalation(5_000, &plan);
        assert_eq!(decision.level, EscalationLevel::FullRebuild);
    }

    #[test]
    fn assess_escalation_reason_includes_threshold_and_project_size() {
        let plan = IncrementalPlan {
            to_index: (0..600).map(|i| format!("file_{i}.cpp")).collect(),
            to_delete: vec![],
            unchanged: vec![],
            entries: vec![],
            rename_hints: vec![],
        };
        let decision = assess_escalation(35_000, &plan);
        assert_eq!(decision.level, EscalationLevel::FullRebuild);
        let reason = decision.reason.unwrap();
        assert!(reason.contains("35000"), "reason missing project size: {}", reason);
    }

    fn make_test_symbol(id: &str, name: &str, qualified: &str, sig: Option<&str>) -> Symbol {
        Symbol {
            id: id.into(),
            name: name.into(),
            qualified_name: qualified.into(),
            symbol_type: "function".into(),
            file_path: "test.h".into(),
            line: 1,
            end_line: 5,
            signature: sig.map(|s| s.into()),
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
        }
    }

    #[test]
    fn analyze_header_change_body_only_when_signatures_identical() {
        let sym = make_test_symbol("id1", "Foo", "ns::Foo", Some("(int x)"));
        let old = vec![sym.clone()];
        let new = vec![sym.clone()];
        assert_eq!(analyze_header_change(&old, &new, None), HeaderChangeKind::BodyOnly);
    }

    #[test]
    fn analyze_header_change_symbols_changed_when_signature_differs() {
        let old_sym = make_test_symbol("id1", "Foo", "ns::Foo", Some("(int x)"));
        let mut new_sym = old_sym.clone();
        new_sym.signature = Some("(int x, int y)".into());
        let result = analyze_header_change(&[old_sym], &[new_sym], None);
        assert!(matches!(result, HeaderChangeKind::SymbolsChanged { .. }));
        if let HeaderChangeKind::SymbolsChanged { changed_ids, added_ids, removed_ids } = result {
            assert_eq!(changed_ids, vec!["id1"]);
            assert!(added_ids.is_empty());
            assert!(removed_ids.is_empty());
        }
    }

    #[test]
    fn analyze_header_change_symbols_changed_when_symbol_added() {
        let old_sym = make_test_symbol("id1", "Foo", "ns::Foo", None);
        let new_sym1 = old_sym.clone();
        let new_sym2 = make_test_symbol("id2", "Bar", "ns::Bar", None);
        let result = analyze_header_change(&[old_sym], &[new_sym1, new_sym2], None);
        assert!(matches!(result, HeaderChangeKind::SymbolsChanged { .. }));
        if let HeaderChangeKind::SymbolsChanged { changed_ids, added_ids, removed_ids } = result {
            assert!(changed_ids.is_empty());
            assert_eq!(added_ids, vec!["id2"]);
            assert!(removed_ids.is_empty());
        }
    }

    #[test]
    fn analyze_header_change_symbols_changed_when_symbol_removed() {
        let sym = make_test_symbol("id1", "Foo", "ns::Foo", None);
        let result = analyze_header_change(&[sym], &[], None);
        assert!(matches!(result, HeaderChangeKind::SymbolsChanged { .. }));
        if let HeaderChangeKind::SymbolsChanged { changed_ids, added_ids, removed_ids } = result {
            assert!(changed_ids.is_empty());
            assert!(added_ids.is_empty());
            assert_eq!(removed_ids, vec!["id1"]);
        }
    }

    #[test]
    fn analyze_header_change_macro_sensitive_when_sensitivity_high() {
        let sym = make_test_symbol("id1", "Foo", "ns::Foo", None);
        assert_eq!(
            analyze_header_change(&[sym.clone()], &[sym], Some("high")),
            HeaderChangeKind::MacroSensitive
        );
    }

    #[test]
    fn analyze_header_change_macro_sensitive_for_template_change() {
        let mut sym = make_test_symbol("id1", "Foo", "ns::Foo", None);
        sym.symbol_type = "template_function".into();
        let mut changed_sym = sym.clone();
        changed_sym.signature = Some("(T x, U y)".into());
        let result = analyze_header_change(&[sym], &[changed_sym], None);
        assert_eq!(result, HeaderChangeKind::MacroSensitive);
    }

    #[test]
    fn detect_define_changes_returns_false_for_identical_source() {
        let src = "#define FOO 1\n#define BAR 2\n";
        assert!(!detect_define_changes(src, src));
    }

    #[test]
    fn detect_define_changes_returns_true_when_define_added() {
        let old = "#define FOO 1\n";
        let new = "#define FOO 1\n#define BAR 2\n";
        assert!(detect_define_changes(old, new));
    }

    #[test]
    fn detect_define_changes_returns_true_when_define_changed() {
        let old = "#define FOO 1\n";
        let new = "#define FOO 2\n";
        assert!(detect_define_changes(old, new));
    }

    #[test]
    fn detect_define_changes_returns_false_when_only_comments_change() {
        let old = "#define FOO 1\n// comment\n";
        let new = "#define FOO 1\n// different comment\n";
        assert!(!detect_define_changes(old, new));
    }
}
