use crate::constants;
use crate::vcxproj;

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;
use walkdir::WalkDir;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildMetadataContext {
    pub source_path: String,
    pub translation_unit_count: usize,
    pub workspace_include_dirs: HashSet<String>,
    pub entries_by_file: HashMap<String, BuildMetadataEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildMetadataEntry {
    pub file_path: String,
    pub output_path: Option<String>,
    pub include_dirs: Vec<String>,
    pub defines: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct CompileCommandRecord {
    directory: String,
    file: String,
    command: Option<String>,
    arguments: Option<Vec<String>>,
    output: Option<String>,
}

pub fn load_build_metadata(workspace_root: &Path) -> Result<Option<BuildMetadataContext>, String> {
    // Priority 1: Try .sln/.vcxproj (Visual Studio projects).
    // Only use the vcxproj result when it actually found translation units;
    // otherwise (e.g. the .sln is a sample/sub-project with no useful vcxproj
    // entries) fall through to compile_commands.json.
    if let Some(vcx_ctx) = vcxproj::load_build_context(workspace_root) {
        let meta = build_context_to_metadata(workspace_root, vcx_ctx);
        if meta.translation_unit_count > 0 {
            return Ok(Some(meta));
        }
        // vcxproj found a .sln but yielded 0 TUs — try compile_commands.json instead.
    }

    // Priority 2: Fall back to compile_commands.json
    let Some(compile_commands_path) = find_compile_commands_path(workspace_root) else {
        return Ok(None);
    };

    let raw = fs::read_to_string(&compile_commands_path)
        .map_err(|err| format!("Failed to read {}: {}", compile_commands_path.display(), err))?;
    let records: Vec<CompileCommandRecord> = serde_json::from_str(&raw)
        .map_err(|err| format!("Failed to parse {}: {}", compile_commands_path.display(), err))?;

    let mut workspace_include_dirs = HashSet::new();
    let mut entries_by_file = HashMap::new();

    for record in records {
        let directory = PathBuf::from(&record.directory);
        let Some(file_path) = relativize_to_workspace(workspace_root, &directory.join(&record.file)) else {
            continue;
        };

        let args = match (record.arguments, record.command) {
            (Some(arguments), _) => arguments,
            (None, Some(command)) => split_shell_words(&command),
            (None, None) => Vec::new(),
        };
        let parsed = parse_compile_arguments(workspace_root, &directory, &args);
        for include_dir in &parsed.workspace_include_dirs {
            workspace_include_dirs.insert(include_dir.clone());
        }

        let output_path = record
            .output
            .as_deref()
            .and_then(|output| relativize_to_workspace(workspace_root, &directory.join(output)));

        entries_by_file.insert(
            file_path.clone(),
            BuildMetadataEntry {
                file_path,
                output_path,
                include_dirs: parsed.workspace_include_dirs,
                defines: parsed.defines,
            },
        );
    }

    Ok(Some(BuildMetadataContext {
        source_path: compile_commands_path.to_string_lossy().replace('\\', "/"),
        translation_unit_count: entries_by_file.len(),
        workspace_include_dirs,
        entries_by_file,
    }))
}

/// Convert vcxproj BuildContext to BuildMetadataContext.
fn build_context_to_metadata(
    _workspace_root: &Path,
    vcx_ctx: vcxproj::BuildContext,
) -> BuildMetadataContext {
    let mut entries_by_file = HashMap::new();

    for proj in &vcx_ctx.projects {
        // Use first configuration (typically "Development" or "Debug")
        let config = proj.configurations.first().unwrap_or(&"Development".to_string()).clone();

        for file_path in vcx_ctx.file_to_project.keys() {
            entries_by_file.insert(
                file_path.clone(),
                BuildMetadataEntry {
                    file_path: file_path.clone(),
                    output_path: proj.output_dir.get(&config).cloned(),
                    include_dirs: proj.include_dirs.get(&config).cloned().unwrap_or_default(),
                    defines: proj.defines.get(&config).cloned().unwrap_or_default(),
                },
            );
        }
    }

    BuildMetadataContext {
        source_path: vcx_ctx.solution
            .map(|s| s.path.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|| "vcxproj".to_string()),
        translation_unit_count: entries_by_file.len(),
        workspace_include_dirs: vcx_ctx.workspace_include_dirs,
        entries_by_file,
    }
}

impl BuildMetadataContext {
    pub fn entry_for_file(&self, file_path: &str) -> Option<&BuildMetadataEntry> {
        self.entries_by_file.get(file_path)
    }

    pub fn marks_public_header(&self, file_path: &str) -> bool {
        let path = Path::new(file_path);
        self.workspace_include_dirs.iter().any(|include_dir| {
            let include_path = Path::new(include_dir);
            path.starts_with(include_path)
        })
    }
}

fn find_compile_commands_path(workspace_root: &Path) -> Option<PathBuf> {
    let direct_candidates = [
        workspace_root.join("compile_commands.json"),
        workspace_root.join("build").join("compile_commands.json"),
        workspace_root.join(constants::DATA_DIR_NAME).join("compile_commands.json"),
    ];
    for candidate in direct_candidates {
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    let mut discovered: Vec<PathBuf> = WalkDir::new(workspace_root)
        .max_depth(4)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file() && entry.file_name() == "compile_commands.json")
        .map(|entry| entry.into_path())
        .collect();
    discovered.sort_by_key(|path| path.components().count());
    discovered.into_iter().next()
}

struct ParsedCompileArguments {
    workspace_include_dirs: Vec<String>,
    defines: Vec<String>,
}

fn parse_compile_arguments(
    workspace_root: &Path,
    directory: &Path,
    args: &[String],
) -> ParsedCompileArguments {
    let mut include_dirs = Vec::new();
    let mut defines = Vec::new();
    let mut index = 0usize;

    while index < args.len() {
        let arg = &args[index];
        if arg == "-I" || arg == "/I" || arg == "-isystem" {
            if let Some(next) = args.get(index + 1) {
                if let Some(rel) = relativize_to_workspace(workspace_root, &directory.join(next)) {
                    include_dirs.push(rel);
                }
                index += 2;
                continue;
            }
        }
        if let Some(value) = arg.strip_prefix("-I").filter(|value| !value.is_empty()) {
            if let Some(rel) = relativize_to_workspace(workspace_root, &directory.join(value)) {
                include_dirs.push(rel);
            }
            index += 1;
            continue;
        }
        if let Some(value) = arg.strip_prefix("/I").filter(|value| !value.is_empty()) {
            if let Some(rel) = relativize_to_workspace(workspace_root, &directory.join(value)) {
                include_dirs.push(rel);
            }
            index += 1;
            continue;
        }
        if let Some(value) = arg.strip_prefix("-isystem").filter(|value| !value.is_empty()) {
            if let Some(rel) = relativize_to_workspace(workspace_root, &directory.join(value)) {
                include_dirs.push(rel);
            }
            index += 1;
            continue;
        }
        if arg == "-D" || arg == "/D" {
            if let Some(next) = args.get(index + 1) {
                defines.push(trim_define_value(next));
                index += 2;
                continue;
            }
        }
        if let Some(value) = arg.strip_prefix("-D").filter(|value| !value.is_empty()) {
            defines.push(trim_define_value(value));
            index += 1;
            continue;
        }
        if let Some(value) = arg.strip_prefix("/D").filter(|value| !value.is_empty()) {
            defines.push(trim_define_value(value));
            index += 1;
            continue;
        }
        index += 1;
    }

    ParsedCompileArguments {
        workspace_include_dirs: include_dirs,
        defines,
    }
}

fn trim_define_value(value: &str) -> String {
    value.split('=').next().unwrap_or(value).trim().to_string()
}

fn relativize_to_workspace(workspace_root: &Path, path: &Path) -> Option<String> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        workspace_root.join(path)
    };
    let normalized = normalize_path(&absolute);
    let normalized_root = normalize_path(workspace_root);
    normalized
        .strip_prefix(&normalized_root)
        .ok()
        .map(|rel| rel.to_string_lossy().replace('\\', "/"))
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

fn split_shell_words(command: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut quote_char = '\0';
    let mut escaped = false;

    for ch in command.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }
        match ch {
            '\\' if in_quotes => {
                escaped = true;
            }
            '"' | '\'' => {
                if in_quotes && ch == quote_char {
                    in_quotes = false;
                    quote_char = '\0';
                } else if !in_quotes {
                    in_quotes = true;
                    quote_char = ch;
                } else {
                    current.push(ch);
                }
            }
            c if c.is_whitespace() && !in_quotes => {
                if !current.is_empty() {
                    args.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(ch),
        }
    }

    if !current.is_empty() {
        args.push(current);
    }
    args
}

// ─── Build-config mtime helper ────────────────────────────────────────────────

/// Return the last-modified time (seconds since Unix epoch) of the primary
/// build configuration file in `workspace_root`.
///
/// Checks in order:
/// 1. Any `.sln` file directly inside `workspace_root` (shallowest match first).
/// 2. `compile_commands.json` at the workspace root or inside a `build/` sub-dir.
///
/// Returns `None` if no configuration file is found or the mtime cannot be read.
pub fn find_build_config_mtime(workspace_root: &Path) -> Option<u64> {
    // 1. Shallow scan for .sln files (max depth 1).
    let sln_path = WalkDir::new(workspace_root)
        .max_depth(1)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| {
            e.file_type().is_file()
                && e.path()
                    .extension()
                    .map(|ext| ext.eq_ignore_ascii_case("sln"))
                    .unwrap_or(false)
        })
        .map(|e| e.into_path())
        .next();

    let config_path = sln_path.or_else(|| find_compile_commands_path(workspace_root))?;

    mtime_secs(&config_path)
}

/// Read the mtime of `path` as seconds since the Unix epoch.
fn mtime_secs(path: &Path) -> Option<u64> {
    path.metadata()
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn loads_compile_commands_and_relativizes_workspace_paths() {
        let temp = tempdir().unwrap();
        let root = temp.path();
        fs::create_dir_all(root.join("build")).unwrap();
        fs::create_dir_all(root.join("include/public")).unwrap();
        fs::write(
            root.join("build/compile_commands.json"),
            r#"[
              {
                "directory": "ROOT",
                "file": "src/demo.cpp",
                "arguments": ["clang++", "-I", "include/public", "-DUNIT_TEST=1"],
                "output": "tests/demo_test.o"
              }
            ]"#
            .replace("ROOT", &root.to_string_lossy().replace('\\', "\\\\")),
        )
        .unwrap();

        let metadata = load_build_metadata(root).unwrap().unwrap();
        let entry = metadata.entry_for_file("src/demo.cpp").unwrap();
        assert_eq!(metadata.translation_unit_count, 1);
        assert!(metadata.workspace_include_dirs.contains("include/public"));
        assert_eq!(entry.output_path.as_deref(), Some("tests/demo_test.o"));
        assert_eq!(entry.defines, vec!["UNIT_TEST"]);
    }

    #[test]
    fn marks_headers_under_workspace_include_dirs_as_public_candidates() {
        let context = BuildMetadataContext {
            source_path: "compile_commands.json".into(),
            translation_unit_count: 1,
            workspace_include_dirs: HashSet::from(["include/public".to_string()]),
            entries_by_file: HashMap::new(),
        };

        assert!(context.marks_public_header("include/public/demo/api.hpp"));
        assert!(!context.marks_public_header("src/private_impl/api.hpp"));
    }

    #[test]
    fn splits_shell_words_with_quoted_arguments() {
        let args = split_shell_words(r#"clang++ -I "include/public api" -DUNIT_TEST src/demo.cpp"#);
        assert_eq!(args[0], "clang++");
        assert_eq!(args[1], "-I");
        assert_eq!(args[2], "include/public api");
        assert_eq!(args[3], "-DUNIT_TEST");
        assert_eq!(args[4], "src/demo.cpp");
    }
}
