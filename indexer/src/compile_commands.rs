/// compile_commands.rs — shared logic for generating and patching
/// compile_commands.json from MSBuild (vcxproj / sln).
///
/// Used by:
///   - `main.rs`  (the `generate-compile-commands` subcommand)
///   - `watcher.rs` (incremental patch when a .vcxproj changes)

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

// ─── MSBuild targets XML injected via ForceImportBeforeCppTargets ────────────
// Supported by v160 (VS2019) and v170 (VS2022) toolsets.
pub const CODEATLAS_CC_TARGETS: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<Project xmlns="http://schemas.microsoft.com/developer/msbuild/2003">
  <Target Name="CodeAtlasGetCompileItems">
    <Message Condition="'%(ClCompile.Identity)' != '' and '%(ClCompile.ExcludedFromBuild)' != 'true'"
             Text="CODEATLAS_CC:%(ClCompile.FullPath)|%(ClCompile.AdditionalIncludeDirectories)|%(ClCompile.PreprocessorDefinitions)"
             Importance="high" />
  </Target>
</Project>"#;

// ─── MSBuild discovery ───────────────────────────────────────────────────────

/// Locate MSBuild.exe (VS2017 – VS2022, including BuildTools editions).
pub fn find_msbuild() -> Option<PathBuf> {
    // 1. Already in PATH (developer command prompt / CI)
    if let Ok(out) = std::process::Command::new("where").args(["MSBuild"]).output() {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout);
            if let Some(first) = s.lines().next() {
                let p = PathBuf::from(first.trim());
                if p.exists() {
                    return Some(p);
                }
            }
        }
    }

    // 2. vswhere (ships with VS2017+ installer)
    let vswhere = PathBuf::from(
        r"C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe",
    );
    if vswhere.exists() {
        let out = std::process::Command::new(&vswhere)
            .args([
                "-latest",
                "-requires",
                "Microsoft.Component.MSBuild",
                "-find",
                r"MSBuild\**\Bin\MSBuild.exe",
            ])
            .output()
            .ok()?;
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout);
            if let Some(line) = s.lines().next() {
                let p = PathBuf::from(line.trim());
                if p.exists() {
                    return Some(p);
                }
            }
        }
    }

    // 3. Hardcoded fallbacks
    let candidates = [
        r"C:\Program Files\Microsoft Visual Studio\2022\Professional\MSBuild\Current\Bin\MSBuild.exe",
        r"C:\Program Files\Microsoft Visual Studio\2022\Community\MSBuild\Current\Bin\MSBuild.exe",
        r"C:\Program Files\Microsoft Visual Studio\2022\BuildTools\MSBuild\Current\Bin\MSBuild.exe",
        r"C:\Program Files (x86)\Microsoft Visual Studio\2019\Professional\MSBuild\Current\Bin\MSBuild.exe",
        r"C:\Program Files (x86)\Microsoft Visual Studio\2019\Community\MSBuild\Current\Bin\MSBuild.exe",
        r"C:\Program Files (x86)\Microsoft Visual Studio\2017\Professional\MSBuild\15.0\Bin\MSBuild.exe",
        r"C:\Program Files (x86)\Microsoft Visual Studio\2017\Community\MSBuild\15.0\Bin\MSBuild.exe",
    ];
    for c in &candidates {
        let p = PathBuf::from(c);
        if p.exists() {
            return Some(p);
        }
    }

    None
}

// ─── Output parsing ──────────────────────────────────────────────────────────

/// Parse `CODEATLAS_CC:file|includes|defines` lines from MSBuild stdout into
/// compile_commands.json-style JSON objects.
pub fn parse_codeatlas_cc_output(
    output: &str,
    workspace_root: &Path,
) -> Vec<serde_json::Value> {
    let workspace_str = workspace_root.to_string_lossy().replace('\\', "/");
    let mut entries = Vec::new();
    let mut seen = HashSet::new();

    for line in output.lines() {
        let trimmed = line.trim();
        let rest = match trimmed.find("CODEATLAS_CC:") {
            Some(pos) => &trimmed[pos + "CODEATLAS_CC:".len()..],
            None => continue,
        };

        let parts: Vec<&str> = rest.splitn(3, '|').collect();
        if parts.is_empty() {
            continue;
        }

        let file_path = parts[0].trim().replace('\\', "/");
        if file_path.is_empty() || !seen.insert(file_path.clone()) {
            continue;
        }

        let includes_raw = parts.get(1).copied().unwrap_or("").trim();
        let defines_raw = parts.get(2).copied().unwrap_or("").trim();

        let mut arguments = vec!["clang-cl".to_string()];

        for dir in includes_raw.split(';') {
            let dir = dir.trim();
            if !dir.is_empty() && !dir.contains("$(") && !dir.contains("%(") {
                arguments.push(format!("-I{}", dir.replace('\\', "/")));
            }
        }
        for def in defines_raw.split(';') {
            let def = def.trim();
            if !def.is_empty() && !def.contains("$(") && !def.contains("%(") {
                arguments.push(format!("-D{}", def));
            }
        }

        arguments.push(file_path.clone());

        entries.push(serde_json::json!({
            "directory": workspace_str,
            "file": file_path,
            "arguments": arguments
        }));
    }

    entries
}

// ─── Single-vcxproj MSBuild run ──────────────────────────────────────────────

/// Run MSBuild on a single vcxproj and return the parsed compile_commands
/// entries.  Returns an empty Vec on failure (warnings are printed to stderr).
pub fn extract_entries_for_vcxproj(
    msbuild: &Path,
    vcxproj: &Path,
    config: &str,
    platform: &str,
    targets_path: &Path,
    workspace_root: &Path,
) -> Vec<serde_json::Value> {
    let proj_name = vcxproj
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();

    let result = std::process::Command::new(msbuild)
        .args([
            vcxproj.to_str().unwrap_or(""),
            "/t:CodeAtlasGetCompileItems",
            &format!("/p:ForceImportBeforeCppTargets={}", targets_path.display()),
            &format!("/p:Configuration={}", config),
            &format!("/p:Platform={}", platform),
            "/nologo",
            "/verbosity:minimal",
        ])
        .output();

    match result {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let entries = parse_codeatlas_cc_output(&stdout, workspace_root);
            if entries.is_empty() && !out.status.success() {
                let stderr = String::from_utf8_lossy(&out.stderr);
                eprintln!("  [WARN] MSBuild failed for {} (exit {:?})", proj_name, out.status.code());
                if !stderr.is_empty() {
                    let tail: String = stderr
                        .lines()
                        .rev()
                        .take(5)
                        .collect::<Vec<_>>()
                        .into_iter()
                        .rev()
                        .collect::<Vec<_>>()
                        .join("\n");
                    eprintln!("  stderr: {}", tail);
                }
            }
            entries
        }
        Err(e) => {
            eprintln!("  [WARN] Failed to launch MSBuild for {}: {}", proj_name, e);
            vec![]
        }
    }
}

// ─── Temp-targets file helper ────────────────────────────────────────────────

/// Write the shared targets XML to a temp file and return its path.
/// Returns `Err` if writing fails.
pub fn write_temp_targets_file() -> Result<PathBuf, String> {
    let path = std::env::temp_dir().join("codeatlas_cc.targets");
    fs::write(&path, CODEATLAS_CC_TARGETS)
        .map_err(|e| format!("Failed to write temp targets file: {}", e))?;
    Ok(path)
}

// ─── compile_commands.json patch ─────────────────────────────────────────────

/// Read the existing compile_commands.json, replace all entries whose "file"
/// field matches one of `new_entries`, append entries for files that were not
/// present before, and write the result back atomically.
///
/// Returns the list of file paths that were added or replaced (so the caller
/// can schedule them for re-indexing).
pub fn patch_compile_commands(
    compile_commands_path: &Path,
    new_entries: &[serde_json::Value],
) -> Result<Vec<String>, String> {
    if new_entries.is_empty() {
        return Ok(vec![]);
    }

    // Build index: file → new entry
    let new_by_file: std::collections::HashMap<String, &serde_json::Value> = new_entries
        .iter()
        .filter_map(|e| {
            e.get("file")
                .and_then(|f| f.as_str())
                .map(|f| (f.to_string(), e))
        })
        .collect();

    // Load existing entries (empty Vec if file doesn't exist yet)
    let mut existing: Vec<serde_json::Value> = if compile_commands_path.exists() {
        let raw = fs::read_to_string(compile_commands_path)
            .map_err(|e| format!("Failed to read compile_commands.json: {}", e))?;
        serde_json::from_str(&raw)
            .map_err(|e| format!("Failed to parse compile_commands.json: {}", e))?
    } else {
        vec![]
    };

    // Replace existing entries for files that appear in new_by_file
    let mut replaced_files: HashSet<String> = HashSet::new();
    for entry in &mut existing {
        let file_key = entry.get("file").and_then(|f| f.as_str()).map(|s| s.to_string());
        if let Some(file) = file_key {
            if let Some(new_entry) = new_by_file.get(&file) {
                *entry = (*new_entry).clone();
                replaced_files.insert(file);
            }
        }
    }

    // Append entries for files not previously present
    let mut changed_files: Vec<String> = replaced_files.into_iter().collect();
    for (file, entry) in &new_by_file {
        if !changed_files.contains(file) {
            existing.push((*entry).clone());
            changed_files.push(file.clone());
        }
    }

    // Write back atomically (temp file → rename)
    let tmp_path = compile_commands_path.with_extension("json.tmp");
    let json = serde_json::to_string_pretty(&existing)
        .map_err(|e| format!("Failed to serialize compile_commands: {}", e))?;
    fs::write(&tmp_path, &json)
        .map_err(|e| format!("Failed to write temp compile_commands: {}", e))?;
    fs::rename(&tmp_path, compile_commands_path)
        .map_err(|e| format!("Failed to rename compile_commands: {}", e))?;

    changed_files.sort();
    Ok(changed_files)
}
