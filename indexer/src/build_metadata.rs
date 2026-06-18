use crate::constants;
use crate::cpp_context;
use crate::ignore::IgnoreRules;
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

/// Remove `entries_by_file` entries whose paths match `.codeatlasignore`.
/// Also updates `translation_unit_count` to reflect the filtered set.
/// This ensures the build-metadata lookup table is consistent with the file
/// discovery phase, which applies the same rules.
fn filter_by_ignore_rules(mut ctx: BuildMetadataContext, workspace_root: &Path) -> BuildMetadataContext {
    let ignore_rules = IgnoreRules::load(workspace_root);
    if ignore_rules.is_empty() {
        return ctx;
    }
    let before = ctx.entries_by_file.len();
    ctx.entries_by_file.retain(|path, _| !ignore_rules.is_ignored(path));
    let after = ctx.entries_by_file.len();
    if before != after {
        eprintln!(
            "[build_metadata] ignored {} entries matching .codeatlasignore ({} → {} TUs)",
            before - after, before, after
        );
    }
    ctx.translation_unit_count = ctx.entries_by_file.len();
    ctx
}

pub fn load_build_metadata(workspace_root: &Path) -> Result<Option<BuildMetadataContext>, String> {
    // Priority 1: Try .sln/.vcxproj (Visual Studio projects).
    // Only use the vcxproj result when it actually found translation units;
    // otherwise (e.g. the .sln is a sample/sub-project with no useful vcxproj
    // entries) fall through to compile_commands.json.
    if let Some(vcx_ctx) = vcxproj::load_build_context(workspace_root) {
        let meta = build_context_to_metadata(workspace_root, vcx_ctx);
        if meta.translation_unit_count > 0 {
            return Ok(Some(filter_by_ignore_rules(meta, workspace_root)));
        }
        // vcxproj found a .sln but yielded 0 TUs — try compile_commands.json instead.
    }

    // Priority 2: compile_commands.json
    if let Some(compile_commands_path) = find_compile_commands_path(workspace_root) {
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

        expand_unity_build_includes(workspace_root, &mut entries_by_file);

        // Supplement compile_commands.json coverage: add entries for source files
        // that appear in vcxproj projects but are absent from compile_commands.json.
        // This lets libclang handle those files instead of falling back to tree-sitter.
        expand_vcxproj_entries(workspace_root, &mut entries_by_file, &mut workspace_include_dirs);

        let ctx = BuildMetadataContext {
            source_path: compile_commands_path.to_string_lossy().replace('\\', "/"),
            translation_unit_count: entries_by_file.len(),
            workspace_include_dirs,
            entries_by_file,
        };
        return Ok(Some(filter_by_ignore_rules(ctx, workspace_root)));
    }

    // Priority 3: cpp_context.json (generated from vcxproj files, auto-refreshed when stale).
    // This is an independent alternative to compile_commands.json for MSBuild/vcxproj
    // workspaces that do not produce a compile-commands database.
    let data_dir = workspace_root.join(constants::DATA_DIR_NAME);
    if data_dir.is_dir() {
        if let Some(ctx) = cpp_context::load_or_generate(workspace_root, &data_dir) {
            let meta = build_metadata_from_cpp_context(ctx);
            return Ok(Some(filter_by_ignore_rules(meta, workspace_root)));
        }
    }

    // Priority 4: no build metadata available — indexer falls back to tree-sitter.
    Ok(None)
}

/// Convert a [`CppContext`] loaded from `cpp_context.json` into a
/// [`BuildMetadataContext`] that the indexer can use directly.
fn build_metadata_from_cpp_context(ctx: cpp_context::CppContext) -> BuildMetadataContext {
    let mut entries_by_file: HashMap<String, BuildMetadataEntry> = HashMap::new();
    let mut workspace_include_dirs: HashSet<String> = HashSet::new();

    for config in &ctx.configurations {
        for dir in &config.include_dirs {
            workspace_include_dirs.insert(dir.clone());
        }
        for src in &config.source_files {
            // First configuration that claims a source file wins.
            if entries_by_file.contains_key(src) {
                continue;
            }
            entries_by_file.insert(
                src.clone(),
                BuildMetadataEntry {
                    file_path: src.clone(),
                    output_path: None,
                    include_dirs: config.include_dirs.clone(),
                    defines: config.defines.clone(),
                },
            );
        }
    }

    eprintln!(
        "[build_metadata] cpp_context: {} entries, {} workspace include dirs",
        entries_by_file.len(),
        workspace_include_dirs.len(),
    );

    BuildMetadataContext {
        source_path: cpp_context::CPP_CONTEXT_FILENAME.to_string(),
        translation_unit_count: entries_by_file.len(),
        workspace_include_dirs,
        entries_by_file,
    }
}

/// For each compile_commands entry that looks like a unity/bbsource file
/// (i.e. a `.cpp` whose content is mostly `#include "*.cpp"` directives),
/// synthesise BuildMetadataEntry records for the individual included `.cpp`
/// files so that the indexer can run libclang on them with proper flags.
///
/// Existing entries are never overwritten — if an individual file already has
/// its own compile_commands entry it keeps its own flags.
fn expand_unity_build_includes(
    workspace_root: &Path,
    entries_by_file: &mut HashMap<String, BuildMetadataEntry>,
) {
    // Collect the unity entries first to avoid borrowing issues.
    let unity_entries: Vec<BuildMetadataEntry> = entries_by_file
        .values()
        .filter(|e| is_likely_unity_file(&e.file_path))
        .cloned()
        .collect();

    if unity_entries.is_empty() {
        eprintln!("[build_metadata] No unity/bbsource files detected in compile_commands.json");
        return;
    }

    eprintln!("[build_metadata] Found {} unity/bbsource file(s) in compile_commands.json:", unity_entries.len());
    for u in &unity_entries {
        eprintln!("[build_metadata]   {}", u.file_path);
    }

    let mut total_expanded = 0usize;

    for unity in unity_entries {
        let abs_unity = workspace_root.join(unity.file_path.replace('/', std::path::MAIN_SEPARATOR_STR));
        let source = match fs::read_to_string(&abs_unity) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[build_metadata]   WARN: cannot read {}: {}", unity.file_path, e);
                continue;
            }
        };

        let unity_dir = abs_unity.parent().unwrap_or(workspace_root);
        let included = extract_cpp_includes(&source, unity_dir, workspace_root);

        eprintln!("[build_metadata]   {} -> {} .cpp includes found", unity.file_path, included.len());
        for rel in &included {
            eprintln!("[build_metadata]     include: {}", rel);
        }

        for included_rel in included {
            let is_new = !entries_by_file.contains_key(&included_rel);
            entries_by_file.entry(included_rel.clone()).or_insert_with(|| BuildMetadataEntry {
                file_path: included_rel.clone(),
                output_path: unity.output_path.clone(),
                include_dirs: unity.include_dirs.clone(),
                defines: unity.defines.clone(),
            });
            if is_new {
                total_expanded += 1;
                eprintln!("[build_metadata]     + added entry for: {}", included_rel);
            }
        }
    }

    eprintln!("[build_metadata] Unity expansion complete: {} new entries added", total_expanded);
}

/// After compile_commands.json coverage has been established (including unity-build
/// expansion), scan for `*.vcxproj` files under well-known build subdirectories
/// (e.g. `buildsys/`, `build/`) and add `BuildMetadataEntry` records for source
/// files that are not yet in `entries_by_file`.  This gives libclang-level
/// analysis to files like `shotnormal.cpp` that are compiled via vcxproj but
/// absent from compile_commands.json.
fn expand_vcxproj_entries(
    workspace_root: &Path,
    entries_by_file: &mut HashMap<String, BuildMetadataEntry>,
    workspace_include_dirs: &mut HashSet<String>,
) {
    // Only scan directories that are likely to contain generated vcxproj files.
    let scan_roots: Vec<PathBuf> = ["buildsys", "build", "projects", "intermediate"]
        .iter()
        .map(|s| workspace_root.join(s))
        .filter(|p| p.is_dir())
        .collect();

    if scan_roots.is_empty() {
        return;
    }

    const MAX_VCXPROJS: usize = 2000;
    let mut vcxproj_count = 0usize;
    let mut new_entries = 0usize;

    'outer: for scan_root in &scan_roots {
        let walker = WalkDir::new(scan_root)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| {
                let name = e.file_name().to_string_lossy();
                // Skip hidden dirs and known irrelevant directories.
                !name.starts_with('.') && name != "node_modules" && name != "target"
            });

        for entry in walker {
            let entry = match entry { Ok(e) => e, Err(_) => continue };
            if !entry.file_type().is_file() { continue; }
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("vcxproj") { continue; }

            vcxproj_count += 1;
            if vcxproj_count > MAX_VCXPROJS {
                eprintln!("[build_metadata] vcxproj expansion: reached cap of {} files, stopping", MAX_VCXPROJS);
                break 'outer;
            }

            let Some(vcx) = vcxproj::parse_vcxproj_file(path) else { continue };

            // Choose the most release-like configuration available.
            let config = pick_best_config(&vcx.configurations);
            let inc_dirs = vcx.include_dirs.get(&config).cloned().unwrap_or_default();
            let defs    = vcx.defines.get(&config).cloned().unwrap_or_default();
            let out_dir = vcx.output_dir.get(&config).and_then(|d| {
                relativize_to_workspace(workspace_root, Path::new(d))
            });

            for dir in &inc_dirs {
                workspace_include_dirs.insert(dir.clone());
            }

            for src_path in &vcx.source_files {
                let Some(rel) = relativize_to_workspace(workspace_root, src_path) else { continue };
                if entries_by_file.contains_key(&rel) { continue; }

                eprintln!("[build_metadata] vcxproj: + {}", rel);
                entries_by_file.insert(rel.clone(), BuildMetadataEntry {
                    file_path: rel,
                    output_path: out_dir.clone(),
                    include_dirs: inc_dirs.clone(),
                    defines: defs.clone(),
                });
                new_entries += 1;
            }
        }
    }

    if vcxproj_count > 0 {
        eprintln!(
            "[build_metadata] vcxproj expansion: {} .vcxproj files scanned, {} new entries added",
            vcxproj_count, new_entries
        );
    }
}

/// Pick the best configuration from a vcxproj: prefer release variants, then
/// development, then whatever is available.
fn pick_best_config(configurations: &[String]) -> String {
    for cfg in configurations {
        if cfg.to_lowercase().contains("release") {
            return cfg.clone();
        }
    }
    for cfg in configurations {
        let lower = cfg.to_lowercase();
        if lower.contains("develop") || lower == "development" {
            return cfg.clone();
        }
    }
    configurations.first().cloned().unwrap_or_else(|| "Development".to_string())
}


/// Heuristic: a file is a unity/bbsource file when its name contains one of
/// the known unity-build markers.  This avoids reading every source file.
fn is_likely_unity_file(file_path: &str) -> bool {
    let lower = file_path.to_lowercase();
    let name = lower.rsplit('/').next().unwrap_or(&lower);
    // Frostbite bulk-build: BB_runtime.<module>.AutoNN.cpp
    // or BB_runtime.cpp (single-unit)
    name.starts_with("bb_runtime")
        // Other common unity build patterns
        || name.contains("bbsource")
        || name.contains("unitybuild")
        || name.contains("unity_build")
        || name.contains("amalgam")
}

/// Scan `source` text for `#include "*.cpp"` directives and return
/// workspace-relative paths for each resolved included file.
fn extract_cpp_includes(
    source: &str,
    file_dir: &Path,
    workspace_root: &Path,
) -> Vec<String> {
    let mut result = Vec::new();
    for line in source.lines() {
        let trimmed = line.trim();
        // Match: #include "something.cpp"  (double-quote includes only)
        if !trimmed.starts_with("#include") {
            continue;
        }
        let after = trimmed.trim_start_matches("#include").trim();
        if !after.starts_with('"') {
            continue;
        }
        let inner = after.trim_matches('"').trim_matches('"');
        // Only expand .cpp includes (not headers)
        if !inner.to_lowercase().ends_with(".cpp") {
            continue;
        }
        // Resolve relative to the including file's directory
        let candidate = file_dir.join(inner.replace('/', std::path::MAIN_SEPARATOR_STR));
        if let Some(rel) = relativize_to_workspace(workspace_root, &candidate) {
            result.push(rel);
        }
    }
    result
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
    // Strip Windows extended-length prefix: \\?\ or //?/ before normalizing.
    // These prefixes appear in compile_commands.json generated by MSVC toolchains
    // (e.g. //?/D:/dev/project/... or \\?\D:\dev\project\...).
    let path_str = path.to_string_lossy();
    let stripped: &str = if path_str.starts_with("//?/") || path_str.starts_with("//?\\") {
        &path_str[4..]
    } else if path_str.starts_with("\\\\?\\") {
        &path_str[4..]
    } else {
        &path_str
    };

    let effective = Path::new(stripped);
    let mut normalized = PathBuf::new();
    for component in effective.components() {
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
