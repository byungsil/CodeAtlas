use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::Duration;
use walkdir::WalkDir;
use crate::constants::{DATA_DIR_NAME, is_indexed_extension};
use crate::ignore::IgnoreRules;
use crate::language::{DiscoveredSourceFile, SourceLanguage};

pub fn find_cpp_files(root: &Path) -> Vec<PathBuf> {
    find_cpp_files_with_ignore(root, &IgnoreRules::load(root))
}

#[allow(dead_code)]
pub fn find_cpp_files_with_feedback(root: &Path, verbose: bool) -> Vec<PathBuf> {
    if !verbose {
        return find_cpp_files(root);
    }

    let running = Arc::new(AtomicBool::new(true));
    let spinner_running = Arc::clone(&running);
    let spinner = thread::spawn(move || {
        let frames = ["|", "/", "-", "\\"];
        let mut index = 0usize;
        while spinner_running.load(Ordering::Relaxed) {
            print!("\rSearching files... {}", frames[index % frames.len()]);
            let _ = io::stdout().flush();
            index += 1;
            thread::sleep(Duration::from_millis(100));
        }
    });

    let files = find_cpp_files(root);

    running.store(false, Ordering::Relaxed);
    let _ = spinner.join();
    println!("\rSearching files... done");
    files
}

pub fn find_cpp_files_with_ignore(root: &Path, ignore: &IgnoreRules) -> Vec<PathBuf> {
    let root_canonical = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());

    let entries: Vec<PathBuf> = {
        let rc = root_canonical.clone();
        WalkDir::new(root)
            .into_iter()
            .filter_entry(move |e| {
                if e.path().canonicalize().unwrap_or_else(|_| e.path().to_path_buf()) == rc {
                    return true;
                }
                if !e.file_type().is_dir() {
                    return true;
                }
                let name = e.file_name().to_string_lossy();
                !name.starts_with('.') && name != DATA_DIR_NAME && name != "node_modules" && name != "target"
            })
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .filter(|e| {
                e.path()
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .map(is_indexed_extension)
                    .unwrap_or(false)
            })
            .map(|e| e.into_path())
            .collect()
    };

    entries
        .into_iter()
        .filter(|p| {
            let rel = p.strip_prefix(root)
                .unwrap_or(p)
                .to_string_lossy()
                .replace('\\', "/");
            !ignore.is_ignored(&rel)
        })
        .collect()
}

pub fn find_source_files(root: &Path, languages: &[SourceLanguage]) -> Vec<DiscoveredSourceFile> {
    find_source_files_with_ignore(root, &IgnoreRules::load(root), languages)
}

#[allow(dead_code)]
pub fn find_source_files_with_feedback(
    root: &Path,
    verbose: bool,
    languages: &[SourceLanguage],
) -> Vec<DiscoveredSourceFile> {
    if !verbose {
        return find_source_files(root, languages);
    }

    let running = Arc::new(AtomicBool::new(true));
    let spinner_running = Arc::clone(&running);
    let spinner = thread::spawn(move || {
        let frames = ["|", "/", "-", "\\"];
        let mut index = 0usize;
        while spinner_running.load(Ordering::Relaxed) {
            print!("\rSearching files... {}", frames[index % frames.len()]);
            let _ = io::stdout().flush();
            index += 1;
            thread::sleep(Duration::from_millis(100));
        }
    });

    let files = find_source_files(root, languages);

    running.store(false, Ordering::Relaxed);
    let _ = spinner.join();
    println!("\rSearching files... done");
    files
}

pub fn find_source_files_with_ignore(
    root: &Path,
    ignore: &IgnoreRules,
    languages: &[SourceLanguage],
) -> Vec<DiscoveredSourceFile> {
    let root_canonical = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());

    let entries: Vec<DiscoveredSourceFile> = {
        let rc = root_canonical.clone();
        WalkDir::new(root)
            .into_iter()
            .filter_entry(move |e| {
                if e.path().canonicalize().unwrap_or_else(|_| e.path().to_path_buf()) == rc {
                    return true;
                }
                if !e.file_type().is_dir() {
                    return true;
                }
                let name = e.file_name().to_string_lossy();
                !name.starts_with('.') && name != DATA_DIR_NAME && name != "node_modules" && name != "target"
            })
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .filter_map(|e| {
                let extension = e
                    .path()
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .map(|ext| ext.to_ascii_lowercase())?;
                if !is_indexed_extension(&extension) {
                    return None;
                }
                let language = languages
                    .iter()
                    .copied()
                    .find(|language| language.extensions().contains(&extension.as_str()))?;
                Some(DiscoveredSourceFile {
                    path: e.into_path(),
                    language,
                })
            })
            .collect()
    };

    entries
        .into_iter()
        .filter(|entry| {
            let rel = entry
                .path
                .strip_prefix(root)
                .unwrap_or(&entry.path)
                .to_string_lossy()
                .replace('\\', "/");
            !ignore.is_ignored(&rel)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use regex::Regex;

    #[test]
    fn finds_cpp_and_h_files() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("foo.c"), "").unwrap();
        fs::write(dir.path().join("foo.cpp"), "").unwrap();
        fs::write(dir.path().join("bar.h"), "").unwrap();
        fs::write(dir.path().join("baz.txt"), "").unwrap();
        fs::create_dir(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("sub/nested.hpp"), "").unwrap();

        let files = find_cpp_files(dir.path());
        assert_eq!(files.len(), 4);
    }

    #[test]
    fn skips_hidden_dirs() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join(".hidden")).unwrap();
        fs::write(dir.path().join(".hidden/secret.cpp"), "").unwrap();
        fs::write(dir.path().join("visible.cpp"), "").unwrap();

        let files = find_cpp_files(dir.path());
        assert_eq!(files.len(), 1);
    }

    #[test]
    fn ignores_files_matching_rules() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::create_dir_all(dir.path().join("tools")).unwrap();
        fs::write(dir.path().join("src/main.cpp"), "").unwrap();
        fs::write(dir.path().join("tools/helper.cpp"), "").unwrap();

        let ignore = IgnoreRules::from_patterns(vec![Regex::new(r"^tools/").unwrap()]);
        let files = find_cpp_files_with_ignore(dir.path(), &ignore);
        assert_eq!(files.len(), 1);
        assert!(files[0].to_string_lossy().contains("main.cpp"));
    }

    #[test]
    fn loads_codeatlasignore_from_workspace() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::create_dir_all(dir.path().join("testbed")).unwrap();
        fs::write(dir.path().join("src/main.cpp"), "").unwrap();
        fs::write(dir.path().join("testbed/test.cpp"), "").unwrap();
        fs::write(dir.path().join(".codeatlasignore"), "^testbed/\n").unwrap();

        let files = find_cpp_files(dir.path());
        assert_eq!(files.len(), 1);
        assert!(files[0].to_string_lossy().contains("main.cpp"));
    }

    #[test]
    fn finds_source_files_for_multiple_languages() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("engine.cpp"), "").unwrap();
        fs::write(dir.path().join("script.lua"), "").unwrap();
        fs::write(dir.path().join("tool.py"), "").unwrap();
        fs::write(dir.path().join("ui.ts"), "").unwrap();
        fs::write(dir.path().join("core.rs"), "").unwrap();

        let files = find_source_files(
            dir.path(),
            &[
                SourceLanguage::Cpp,
                SourceLanguage::Lua,
                SourceLanguage::Python,
                SourceLanguage::TypeScript,
                SourceLanguage::Rust,
            ],
        );

        assert_eq!(files.len(), 5);
        assert!(files.iter().any(|entry| entry.language == SourceLanguage::Lua));
        assert!(files.iter().any(|entry| entry.language == SourceLanguage::Python));
        assert!(files.iter().any(|entry| entry.language == SourceLanguage::TypeScript));
        assert!(files.iter().any(|entry| entry.language == SourceLanguage::Rust));
    }

    #[test]
    fn source_discovery_respects_requested_languages() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("script.lua"), "").unwrap();
        fs::write(dir.path().join("main.cpp"), "").unwrap();

        let files = find_source_files(dir.path(), &[SourceLanguage::Cpp]);

        assert_eq!(files.len(), 1);
        assert_eq!(files[0].language, SourceLanguage::Cpp);
        assert!(files[0].path.to_string_lossy().contains("main.cpp"));
    }
}
