use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::models::FileRecord;

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
    pub fn entry_for_path(&self, path: &str) -> Option<&PlanEntry> {
        self.entries.iter().find(|entry| entry.path == path)
    }
}

const MASS_CHANGE_ABSOLUTE_THRESHOLD: usize = 200;
const MASS_CHANGE_PERCENT_THRESHOLD: usize = 40;
const MASS_CHANGE_MIN_TOTAL_FILES: usize = 20;
const RENAME_HEAVY_THRESHOLD: usize = 50;

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

pub fn assess_escalation(total_files: usize, plan: &IncrementalPlan) -> EscalationDecision {
    let changed = plan.to_index.len() + plan.to_delete.len();
    if changed == 0 {
        return EscalationDecision {
            level: EscalationLevel::Incremental,
            reason: None,
        };
    }

    if plan.rename_hints.len() >= RENAME_HEAVY_THRESHOLD {
        return EscalationDecision {
            level: EscalationLevel::FullRebuild,
            reason: Some(format!(
                "rename-heavy churn detected ({} rename/move hints)",
                plan.rename_hints.len()
            )),
        };
    }

    if changed >= MASS_CHANGE_ABSOLUTE_THRESHOLD {
        return EscalationDecision {
            level: EscalationLevel::FullRebuild,
            reason: Some(format!(
                "mass change detected ({} files changed, threshold {})",
                changed, MASS_CHANGE_ABSOLUTE_THRESHOLD
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
            rename_hints: (0..RENAME_HEAVY_THRESHOLD)
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
}
