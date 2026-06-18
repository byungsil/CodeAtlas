//! Generate and load a `cpp_context.json` file that describes the C++ compile
//! context (include directories, preprocessor definitions, source file list)
//! derived from `.vcxproj` files found in the workspace.
//!
//! This provides a project-agnostic alternative to `compile_commands.json` for
//! Visual Studio / MSBuild-based projects that do not emit a compile-commands
//! database.  The file is stored in the `.codeatlas` data directory and is
//! regenerated automatically whenever any `.vcxproj` file is newer than the
//! cached context.

use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use crate::vcxproj;

pub const CPP_CONTEXT_FILENAME: &str = "cpp_context.json";

const CPP_CONTEXT_VERSION: u32 = 1;

/// Maximum number of `.vcxproj` files to process during a single scan.
const MAX_VCXPROJS: usize = 2000;

// ---------------------------------------------------------------------------
// Public data model
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
pub struct CppContext {
    pub version: u32,
    pub generated_at: String,
    pub configurations: Vec<CppContextConfig>,
}

/// One entry per `.vcxproj` file (best-matching configuration).
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CppContextConfig {
    /// Human-readable label, e.g. `"aififa / Release"`.
    pub name: String,
    /// Workspace-relative path to the originating `.vcxproj`.
    pub vcxproj: String,
    /// Absolute include directories extracted from `AdditionalIncludeDirectories`.
    pub include_dirs: Vec<String>,
    /// Preprocessor definitions extracted from `PreprocessorDefinitions`.
    pub defines: Vec<String>,
    /// Workspace-relative paths of all `<ClCompile>` source files.
    pub source_files: Vec<String>,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Load `cpp_context.json` from `data_dir`, regenerating it when:
/// * the file does not exist, or
/// * any `.vcxproj` file under the well-known scan directories is newer.
///
/// Returns `None` when the workspace contains no `.vcxproj` files with source
/// files (i.e. the context cannot be built).
pub fn load_or_generate(workspace_root: &Path, data_dir: &Path) -> Option<CppContext> {
    let ctx_path = data_dir.join(CPP_CONTEXT_FILENAME);

    let needs_regen = if ctx_path.exists() {
        is_any_vcxproj_newer_than(workspace_root, &ctx_path)
    } else {
        true
    };

    if needs_regen {
        eprintln!("[cpp_context] Generating {} from vcxproj files...", CPP_CONTEXT_FILENAME);
        let ctx = generate(workspace_root)?;
        match serde_json::to_string_pretty(&ctx) {
            Ok(json) => {
                if let Err(e) = fs::write(&ctx_path, &json) {
                    eprintln!("[cpp_context] WARN: failed to save {}: {}", ctx_path.display(), e);
                } else {
                    eprintln!("[cpp_context] Saved -> {}", ctx_path.display());
                }
            }
            Err(e) => eprintln!("[cpp_context] WARN: failed to serialize context: {}", e),
        }
        Some(ctx)
    } else {
        eprintln!("[cpp_context] Loading cached {}", ctx_path.display());
        let raw = fs::read_to_string(&ctx_path).ok()?;
        serde_json::from_str::<CppContext>(&raw)
            .map_err(|e| eprintln!("[cpp_context] WARN: failed to parse {}: {}", ctx_path.display(), e))
            .ok()
    }
}

// ---------------------------------------------------------------------------
// Staleness check
// ---------------------------------------------------------------------------

/// Returns `true` if any `.vcxproj` file under the well-known build directories
/// has a modification time newer than `ctx_path`.
fn is_any_vcxproj_newer_than(workspace_root: &Path, ctx_path: &Path) -> bool {
    let ctx_mtime = fs::metadata(ctx_path)
        .and_then(|m| m.modified())
        .unwrap_or(SystemTime::UNIX_EPOCH);

    let scan_roots: Vec<PathBuf> = ["buildsys", "build", "projects"]
        .iter()
        .map(|s| workspace_root.join(s))
        .filter(|p| p.is_dir())
        .collect();

    for root in scan_roots {
        for entry in WalkDir::new(root).follow_links(false).into_iter().filter_map(|e| e.ok()) {
            if entry.path().extension().and_then(|e| e.to_str()) == Some("vcxproj") {
                if let Ok(mtime) = fs::metadata(entry.path()).and_then(|m| m.modified()) {
                    if mtime > ctx_mtime {
                        return true;
                    }
                }
            }
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Generation
// ---------------------------------------------------------------------------

/// Scan the workspace for `.vcxproj` files and build a [`CppContext`].
fn generate(workspace_root: &Path) -> Option<CppContext> {
    let scan_roots: Vec<PathBuf> = ["buildsys", "build", "projects", "intermediate"]
        .iter()
        .map(|s| workspace_root.join(s))
        .filter(|p| p.is_dir())
        .collect();

    if scan_roots.is_empty() {
        eprintln!("[cpp_context] No known build directories found; skipping generation");
        return None;
    }

    let mut configurations: Vec<CppContextConfig> = Vec::new();
    let mut vcxproj_count = 0usize;

    'outer: for root in &scan_roots {
        let walker = WalkDir::new(root)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| {
                let name = e.file_name().to_string_lossy();
                !name.starts_with('.') && name != "node_modules" && name != "target"
            });

        for entry in walker {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("vcxproj") {
                continue;
            }

            vcxproj_count += 1;
            if vcxproj_count > MAX_VCXPROJS {
                eprintln!("[cpp_context] Reached cap of {} vcxproj files, stopping scan", MAX_VCXPROJS);
                break 'outer;
            }

            let Some(vcx) = vcxproj::parse_vcxproj_file(path) else { continue };

            let config_name = pick_best_config(&vcx.configurations);
            let inc_dirs = vcx.include_dirs.get(&config_name).cloned().unwrap_or_default();
            let defs = vcx.defines.get(&config_name).cloned().unwrap_or_default();

            let source_files: Vec<String> = vcx.source_files
                .iter()
                .filter_map(|src| relativize_to_workspace(workspace_root, src))
                .collect();

            if source_files.is_empty() {
                continue;
            }

            let vcx_stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("unknown");
            let vcxproj_rel = relativize_to_workspace(workspace_root, path)
                .unwrap_or_else(|| path.to_string_lossy().replace('\\', "/"));

            configurations.push(CppContextConfig {
                name: format!("{} / {}", vcx_stem, config_name),
                vcxproj: vcxproj_rel,
                include_dirs: inc_dirs,
                defines: defs,
                source_files,
            });
        }
    }

    if configurations.is_empty() {
        eprintln!("[cpp_context] No vcxproj files with source files found; generation skipped");
        return None;
    }

    let total_sources: usize = configurations.iter().map(|c| c.source_files.len()).sum();
    eprintln!(
        "[cpp_context] Generated {} configurations, {} source file entries from {} vcxproj files",
        configurations.len(),
        total_sources,
        vcxproj_count,
    );

    Some(CppContext {
        version: CPP_CONTEXT_VERSION,
        generated_at: chrono::Utc::now().to_rfc3339(),
        configurations,
    })
}

// ---------------------------------------------------------------------------
// Helpers (local copies; avoids making build_metadata internals pub)
// ---------------------------------------------------------------------------

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
    // Strip Windows extended-length prefix (\\?\ or //?/).
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
