use std::fs;
use std::path::Path;
use regex::Regex;

const IGNORE_FILENAME: &str = ".codeatlasignore";

pub struct IgnoreRules {
    patterns: Vec<Regex>,
}

impl IgnoreRules {
    pub fn load(workspace_root: &Path) -> Self {
        let ignore_path = workspace_root.join(IGNORE_FILENAME);
        let patterns = if ignore_path.exists() {
            parse_ignore_file(&fs::read_to_string(&ignore_path).unwrap_or_default())
        } else {
            Vec::new()
        };
        IgnoreRules { patterns }
    }

    pub fn from_patterns(patterns: Vec<Regex>) -> Self {
        IgnoreRules { patterns }
    }

    pub fn is_ignored(&self, workspace_relative_path: &str) -> bool {
        let normalized = workspace_relative_path.replace('\\', "/");
        self.patterns.iter().any(|p| p.is_match(&normalized))
    }

    pub fn is_empty(&self) -> bool {
        self.patterns.is_empty()
    }
}

fn parse_ignore_file(content: &str) -> Vec<Regex> {
    content
        .lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| Regex::new(line).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_rules_ignore_nothing() {
        let rules = IgnoreRules::from_patterns(vec![]);
        assert!(!rules.is_ignored("src/foo.cpp"));
        assert!(rules.is_empty());
    }

    #[test]
    fn matches_directory_pattern() {
        let rules = IgnoreRules::from_patterns(vec![Regex::new(r"^tools/").unwrap()]);
        assert!(rules.is_ignored("tools/debug/helper.cpp"));
        assert!(rules.is_ignored("tools/test.h"));
        assert!(!rules.is_ignored("src/tools.cpp"));
    }

    #[test]
    fn matches_file_pattern() {
        let rules = IgnoreRules::from_patterns(vec![Regex::new(r"_test\.cpp$").unwrap()]);
        assert!(rules.is_ignored("src/foo_test.cpp"));
        assert!(!rules.is_ignored("src/foo.cpp"));
    }

    #[test]
    fn matches_multiple_patterns() {
        let rules = IgnoreRules::from_patterns(vec![
            Regex::new(r"^tools/").unwrap(),
            Regex::new(r"^testbed/").unwrap(),
        ]);
        assert!(rules.is_ignored("tools/a.cpp"));
        assert!(rules.is_ignored("testbed/b.h"));
        assert!(!rules.is_ignored("src/c.cpp"));
    }

    #[test]
    fn normalizes_backslashes() {
        let rules = IgnoreRules::from_patterns(vec![Regex::new(r"^tools/").unwrap()]);
        assert!(rules.is_ignored("tools\\debug\\helper.cpp"));
    }

    #[test]
    fn parse_ignore_file_skips_blanks_and_comments() {
        let content = "
# This is a comment
^tools/

^testbed/
# another comment

";
        let patterns = parse_ignore_file(content);
        assert_eq!(patterns.len(), 2);
    }

    #[test]
    fn parse_ignore_file_skips_invalid_regex() {
        let content = "^valid/\n[invalid((\n^also_valid/";
        let patterns = parse_ignore_file(content);
        assert_eq!(patterns.len(), 2);
    }

    #[test]
    fn load_from_workspace() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(".codeatlasignore"), "^tools/\n^testbed/\n").unwrap();
        let rules = IgnoreRules::load(dir.path());
        assert!(rules.is_ignored("tools/foo.cpp"));
        assert!(rules.is_ignored("testbed/bar.h"));
        assert!(!rules.is_ignored("src/main.cpp"));
    }

    #[test]
    fn load_returns_empty_when_no_file() {
        let dir = tempfile::tempdir().unwrap();
        let rules = IgnoreRules::load(dir.path());
        assert!(rules.is_empty());
    }
}
