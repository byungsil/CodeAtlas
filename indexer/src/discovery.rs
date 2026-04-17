use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::Duration;
use walkdir::WalkDir;
use crate::constants::{EXTENSIONS, DATA_DIR_NAME};
use crate::ignore::IgnoreRules;

pub fn find_cpp_files(root: &Path) -> Vec<PathBuf> {
    find_cpp_files_with_ignore(root, &IgnoreRules::load(root))
}

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
                    .map(|ext| EXTENSIONS.contains(&ext))
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use regex::Regex;

    #[test]
    fn finds_cpp_and_h_files() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("foo.cpp"), "").unwrap();
        fs::write(dir.path().join("bar.h"), "").unwrap();
        fs::write(dir.path().join("baz.txt"), "").unwrap();
        fs::create_dir(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("sub/nested.hpp"), "").unwrap();

        let files = find_cpp_files(dir.path());
        assert_eq!(files.len(), 3);
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
}
