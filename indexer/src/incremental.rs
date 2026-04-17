use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::models::FileRecord;

#[derive(Debug, PartialEq)]
pub struct IncrementalPlan {
    pub to_index: Vec<String>,
    pub to_delete: Vec<String>,
    pub unchanged: Vec<String>,
}

pub fn plan(
    disk_files: &[String],
    stored_records: &[FileRecord],
    workspace_root: &Path,
) -> IncrementalPlan {
    let stored_map: HashMap<&str, &str> = stored_records
        .iter()
        .map(|r| (r.path.as_str(), r.content_hash.as_str()))
        .collect();

    let disk_set: HashSet<&str> = disk_files.iter().map(|s| s.as_str()).collect();

    let mut to_index = Vec::new();
    let mut unchanged = Vec::new();

    for rel_path in disk_files {
        let abs_path = workspace_root.join(rel_path.replace('/', std::path::MAIN_SEPARATOR_STR));
        let current_hash = match fs::read_to_string(&abs_path) {
            Ok(content) => format!("{:x}", Sha256::digest(content.as_bytes())),
            Err(_) => {
                to_index.push(rel_path.clone());
                continue;
            }
        };

        match stored_map.get(rel_path.as_str()) {
            Some(&stored_hash) if stored_hash == current_hash => {
                unchanged.push(rel_path.clone());
            }
            _ => {
                to_index.push(rel_path.clone());
            }
        }
    }

    let to_delete: Vec<String> = stored_records
        .iter()
        .filter(|r| !disk_set.contains(r.path.as_str()))
        .map(|r| r.path.clone())
        .collect();

    IncrementalPlan {
        to_index,
        to_delete,
        unchanged,
    }
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
}
