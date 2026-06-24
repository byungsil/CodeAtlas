use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

/// Represents a parsed .sln file (Visual Studio Solution).
#[derive(Debug, Clone)]
pub struct Solution {
    /// Path to the solution file itself.
    pub path: PathBuf,
    /// List of project entries found in the solution.
    pub projects: Vec<ProjectEntry>,
}

/// A single project entry found in a .sln file.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ProjectEntry {
    /// Project name (e.g., "Core").
    pub name: String,
    /// Project GUID.
    pub guid: String,
    /// Path to the .vcxproj file (relative to solution directory).
    pub project_file: PathBuf,
    /// Project type kind.
    pub project_type: ProjectType,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectType {
    Cpp,
    Other,
}

/// Represents a parsed .vcxproj file (Visual Studio C++ Project).
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct VcxProject {
    /// Path to the vcxproj file itself.
    pub path: PathBuf,
    /// Project name from PropertyGroup/ProjectName.
    pub project_name: String,
    /// Configuration names found (e.g., "Debug", "Release").
    pub configurations: Vec<String>,
    /// Platform names found (e.g., "x64", "Win32").
    pub platforms: Vec<String>,
    /// Include directories for each configuration.
    pub include_dirs: HashMap<String, Vec<String>>, // config => dirs
    /// Preprocessor defines for each configuration.
    pub defines: HashMap<String, Vec<String>>,      // config => defines
    /// Output directory for each configuration.
    pub output_dir: HashMap<String, String>,         // config => dir
    /// Target name (OutputName) for each configuration.
    pub target_name: HashMap<String, String>,        // config => name
    /// Source files (.cpp) listed in the project.
    pub source_files: HashSet<PathBuf>,
    /// Header files (.h, .hpp) listed in the project.
    pub header_files: HashSet<PathBuf>,
}

/// Parsed build context combining solution + project data.
#[derive(Debug, Clone)]
pub struct BuildContext {
    pub solution: Option<Solution>,
    pub projects: Vec<VcxProject>,
    /// All include directories across all configurations (deduplicated).
    pub workspace_include_dirs: HashSet<String>,
    /// Map from relative source file path to its project name.
    pub file_to_project: HashMap<String, String>,
}

// ─── .sln Parsing ────────────────────────────────────────────────────────────

fn parse_sln(path: &Path) -> Option<Solution> {
    let raw = fs::read_to_string(path).ok()?;
    let solution_dir = path.parent()?.to_path_buf();

    let mut projects = Vec::new();
    let mut in_projects = false;

    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("Project(") && trimmed.contains("FAE04EC0-301F-11D3-BF4B-00C04F79EFBC") {
            in_projects = true;
        }
        if !in_projects {
            continue;
        }
        if trimmed == "EndProject" {
            in_projects = false;
            continue;
        }
        if let Some(entry) = parse_sln_project_line(trimmed, &solution_dir) {
            projects.push(entry);
        }
    }

    if projects.is_empty() {
        return None;
    }

    Some(Solution {
        path: path.to_path_buf(),
        projects,
    })
}

fn parse_sln_project_line(line: &str, solution_dir: &Path) -> Option<ProjectEntry> {
    // Format: Project("{GUID1}") = "Name", "path\to\project.vcxproj", "{GUID2}"
    let project_guid_start = line.find(r#"Project("{"#)? + 9;
    let project_guid_end = line[project_guid_start..].find('}')? + project_guid_start;
    let project_guid = &line[project_guid_start..project_guid_end];

    // Extract name and project file path
    // After GUID: `) = "Name", "path\to\file.vcxproj", "{GUID2}"
    let rest = &line[project_guid_end + 1..];
    let eq_pos = rest.find('=')?;
    let after_eq = rest[eq_pos + 1..].trim();

    // Split by comma to get name, file path, and project GUID
    // after_eq = "\"Core\", \"src\\Core.vcxproj\", \"{GUID}\""
    let first_comma = after_eq.find(',')?;
    let name_part = after_eq[..first_comma].trim().trim_matches('"');

    let rest_after_name = after_eq[first_comma + 1..].trim();
    let second_comma = rest_after_name.find(',')?;
    let project_file_str = rest_after_name[..second_comma].trim().trim_matches('"');

    // Determine type by GUID (with or without braces)
    let guid_compare = project_guid.trim_matches('{').trim_matches('}');
    let project_type = if guid_compare == "FAE04EC0-301F-11D3-BF4B-00C04F79EFBC" {
        ProjectType::Cpp
    } else {
        ProjectType::Other
    };

    Some(ProjectEntry {
        name: name_part.to_string(),
        guid: project_guid.to_string(),
        project_file: solution_dir.join(project_file_str),
        project_type,
    })
}

// ─── .vcxproj Parsing ────────────────────────────────────────────────────────

fn parse_vcxproj(path: &Path) -> Option<VcxProject> {
    let raw = fs::read_to_string(path).ok()?;

    // Parse XML manually for efficiency (no external crate dependency)
    let mut project_name = String::new();
    let mut configurations = Vec::new();
    let mut platforms = Vec::new();
    let mut include_dirs: HashMap<String, Vec<String>> = HashMap::new();
    let mut defines: HashMap<String, Vec<String>> = HashMap::new();
    let mut output_dir: HashMap<String, String> = HashMap::new();
    let mut target_name: HashMap<String, String> = HashMap::new();
    let mut source_files = HashSet::new();
    let mut header_files = HashSet::new();

    // Extract ProjectName
    if let Some(start) = raw.find("<ProjectName>") {
        let after_tag = &raw[start + 13..];
        if let Some(end) = after_tag.find("</ProjectName>") {
            project_name = after_tag[..end].trim().to_string();
        }
    }

    // Extract configurations and platforms from PropertyGroup (Condition)
    for line in raw.lines() {
        let trimmed = line.trim();

        // Configuration/Platform from PropertyGroup
        // Format: Condition="'$(Configuration)|$(Platform)'=='Debug|Win32'"
        if trimmed.starts_with("<PropertyGroup") && trimmed.contains("Condition") {
            // Extract configuration name after the second ==
            if let Some(eq_eq_pos) = trimmed.find("=='") {
                let after_eq_eq = &trimmed[eq_eq_pos + 3..];
                if let Some(end) = after_eq_eq.find('\'') {
                    let config_platform = after_eq_eq[..end].trim();
                    // Split by | to get config and platform
                    if let Some(pipe_pos) = config_platform.find('|') {
                        let config = config_platform[..pipe_pos].trim().to_string();
                        let platform = config_platform[pipe_pos + 1..].trim().to_string();
                        configurations.push(config);
                        platforms.push(platform);
                    } else {
                        configurations.push(config_platform.to_string());
                    }
                }
            }
        }

        // IncludeDirectories
        if trimmed.starts_with("<AdditionalIncludeDirectories>") {
            let after_tag = &trimmed[trimmed.find(">").unwrap() + 1..];
            if let Some(end) = after_tag.find("</AdditionalIncludeDirectories>") {
                let value = after_tag[..end].trim();
                // Find which configuration this belongs to
                let config = find_current_config(&raw, trimmed);
                if !value.is_empty() {
                    let dirs: Vec<String> = value
                        .split(';')
                        .map(|d| d.trim().trim_matches('"').to_string())
                        .filter(|d| !d.is_empty() && !d.starts_with('%'))
                        .collect();
                    include_dirs.entry(config).or_default().extend(dirs);
                }
            }
        }

        // PreprocessorDefinitions
        if trimmed.starts_with("<PreprocessorDefinitions>") {
            let after_tag = &trimmed[trimmed.find(">").unwrap() + 1..];
            if let Some(end) = after_tag.find("</PreprocessorDefinitions>") {
                let value = after_tag[..end].trim();
                let config = find_current_config(&raw, trimmed);
                if !value.is_empty() {
                    let defs: Vec<String> = value
                        .split(';')
                        .map(|d| d.trim().trim_matches('"').to_string())
                        .filter(|d| !d.is_empty() && !d.starts_with('%'))
                        .collect();
                    defines.entry(config).or_default().extend(defs);
                }
            }
        }

        // OutDir
        if trimmed.starts_with("<OutDir>") {
            let after_tag = &trimmed[trimmed.find(">").unwrap() + 1..];
            if let Some(end) = after_tag.find("</OutDir>") {
                let value = after_tag[..end].trim().to_string();
                let config = find_current_config(&raw, trimmed);
                output_dir.insert(config, value);
            }
        }

        // TargetName / OutputFileName
        if trimmed.starts_with("<TargetName>") {
            let after_tag = &trimmed[trimmed.find(">").unwrap() + 1..];
            if let Some(end) = after_tag.find("</TargetName>") {
                let value = after_tag[..end].trim().to_string();
                let config = find_current_config(&raw, trimmed);
                target_name.insert(config, value);
            }
        }

        // Include CompileItems (ClCompile)
        if trimmed.starts_with("<ClCompile Include=\"") {
            if let Some(find_pos) = trimmed.find("Include=\"") {
                let start = find_pos + 9;
                if let Some(end) = trimmed[start..].find('"') {
                    let file_path = PathBuf::from(&trimmed[start..start + end]);
                    let ext = file_path.extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("");
                    match ext {
                        "cpp" | "cxx" | "cc" | "c" => {
                            source_files.insert(file_path);
                        }
                        "h" | "hpp" | "hxx" => {
                            header_files.insert(file_path);
                        }
                        _ => {}
                    }
                }
            }
        }

        // Include ClInclude items
        if trimmed.starts_with("<ClInclude Include=\"") {
            if let Some(find_pos) = trimmed.find("Include=\"") {
                let start = find_pos + 9;
                if let Some(end) = trimmed[start..].find('"') {
                    let file_path = PathBuf::from(&trimmed[start..start + end]);
                    header_files.insert(file_path);
                }
            }
        }

        // Include None items (sometimes headers are listed here)
        if trimmed.starts_with("<None Include=\"") && path.extension().and_then(|e| e.to_str()) == Some("vcxproj") {
            if let Some(find_pos) = trimmed.find("Include=\"") {
                let start = find_pos + 9;
                if let Some(end) = trimmed[start..].find('"') {
                    let file_path = PathBuf::from(&trimmed[start..start + end]);
                    let ext = file_path.extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("");
                    if ext == "h" || ext == "hpp" {
                        header_files.insert(file_path);
                    }
                }
            }
        }
    }

    // Also scan subdirectories for items not explicitly listed in vcxproj
    let project_dir = path.parent()?.to_path_buf();
    discover_project_files(&project_dir, &mut source_files, &mut header_files);

    if project_name.is_empty() {
        // Try to get from filename
        project_name = path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();
    }

    Some(VcxProject {
        path: PathBuf::from(path),
        project_name,
        configurations,
        platforms,
        include_dirs,
        defines,
        output_dir,
        target_name,
        source_files,
        header_files,
    })
}

fn find_current_config(raw: &str, search_line: &str) -> String {
    // Look backwards from the search_line position to find <PropertyGroup Condition="...Configuration=='...'">
    let line_pos = raw.find(search_line).unwrap_or(0);
    let before = &raw[..line_pos];

    // Find last PropertyGroup with Configuration condition
    for line in before.lines().rev() {
        if line.contains("<PropertyGroup") && line.contains("Configuration") {
            // Format: Condition="'$(Configuration)|$(Platform)'=='Debug|Win32'"
            if let Some(eq_eq_pos) = line.find("=='") {
                let after_eq_eq = &line[eq_eq_pos + 3..];
                if let Some(end) = after_eq_eq.find('\'') {
                    let config_platform = after_eq_eq[..end].trim();
                    if let Some(pipe_pos) = config_platform.find('|') {
                        return config_platform[..pipe_pos].trim().to_string();
                    }
                    return config_platform.to_string();
                }
            }
            // Also handle explicit Configuration='Debug' pattern
            if let Some(start) = line.find("Configuration='") {
                let rest = &line[start + 15..];
                if let Some(end) = rest.find('\'') {
                    return rest[..end].trim().to_string();
                }
            }
        }
    }
    "Development".to_string() // Default VS configuration
}

fn discover_project_files(
    project_dir: &Path,
    source_files: &mut HashSet<PathBuf>,
    header_files: &mut HashSet<PathBuf>,
) {
    if !project_dir.exists() {
        return;
    }

    let extensions = ["cpp", "cxx", "cc", "c", "h", "hpp", "hxx"];
    for entry in fs::read_dir(project_dir).ok().into_iter().flatten() {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();

        if !path.is_file() {
            // Recurse into subdirectories (skip build output dirs)
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str == "Debug" || name_str == "Release" || name_str == "x64"
                || name_str == "Win32" || name_str.starts_with('.') {
                continue;
            }
            discover_project_files(&path, source_files, header_files);
            continue;
        }

        let ext = path.extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        if extensions.contains(&ext) {
            if matches!(ext, "cpp" | "cxx" | "cc" | "c") {
                source_files.insert(path);
            } else {
                header_files.insert(path);
            }
        }
    }
}

// ─── Public API ──────────────────────────────────────────────────────────────

/// Find .sln files in the workspace root.
pub fn find_solution_files(workspace_root: &Path) -> Vec<PathBuf> {
    let mut solutions = Vec::new();
    collect_sln_files(workspace_root, &mut solutions);
    solutions.sort_by_key(|p| p.components().count());
    solutions
}

fn collect_sln_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).ok().into_iter().flatten() {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();
        if path.is_dir() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.starts_with('.') || name_str == "node_modules" || name_str == "target" {
                continue;
            }
            collect_sln_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("sln") {
            out.push(path);
        }
    }
}

/// Parse a solution and its associated projects.
pub fn parse_solution(workspace_root: &Path, sln_path: &Path) -> Option<BuildContext> {
    let solution = parse_sln(sln_path)?;

    // Filter to C++ projects only
    let cpp_projects: Vec<&ProjectEntry> = solution
        .projects
        .iter()
        .filter(|p| p.project_type == ProjectType::Cpp)
        .collect();

    if cpp_projects.is_empty() {
        return None;
    }

    // Parse each vcxproj
    let mut projects = Vec::new();
    let mut workspace_include_dirs = HashSet::new();
    let mut file_to_project: HashMap<String, String> = HashMap::new();

    for entry in &cpp_projects {
        if let Some(vcx) = parse_vcxproj(&entry.project_file) {
            // Collect include dirs
            for (_, dirs) in &vcx.include_dirs {
                for dir in dirs {
                    workspace_include_dirs.insert(dir.clone());
                }
            }

            // Map source/header files to project name
            let proj_name = vcx.project_name.clone();
            for src in &vcx.source_files {
                if let Ok(rel) = src.strip_prefix(workspace_root) {
                    file_to_project.insert(
                        rel.to_string_lossy().replace('\\', "/"),
                        proj_name.clone(),
                    );
                }
            }
            for hdr in &vcx.header_files {
                if let Ok(rel) = hdr.strip_prefix(workspace_root) {
                    file_to_project.entry(
                        rel.to_string_lossy().replace('\\', "/"),
                    ).or_insert_with(|| proj_name.clone());
                }
            }

            projects.push(vcx);
        }
    }

    if projects.is_empty() {
        return None;
    }

    Some(BuildContext {
        solution: Some(solution),
        projects,
        workspace_include_dirs,
        file_to_project,
    })
}

/// Load build context from workspace. Tries .sln/.vcxproj first, falls back to compile_commands.json.
pub fn load_build_context(workspace_root: &Path) -> Option<BuildContext> {
    let solutions = find_solution_files(workspace_root);
    for sln in &solutions {
        if let Some(ctx) = parse_solution(workspace_root, sln) {
            return Some(ctx);
        }
    }

    // Fallback: try compile_commands.json (existing logic via build_metadata.rs)
    None
}

/// Parse a single vcxproj file. Public API so build_metadata.rs can supplement
/// compile_commands.json coverage with entries from individual vcxproj projects.
pub fn parse_vcxproj_file(path: &Path) -> Option<VcxProject> {
    parse_vcxproj(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn parses_simple_sln() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();

        let sln_content = r#"Microsoft Visual Studio Solution File, Format Version 12.00
# Visual Studio Version 17
VisualStudioVersion = 17.0.31903.59
MinimumVisualStudioVersion = 10.0.40219.1
Project("{FAE04EC0-301F-11D3-BF4B-00C04F79EFBC}") = "Core", "src\Core.vcxproj", "{12345678-1234-1234-1234-123456789012}"
EndProject
Global
EndGlobal
"#;
        fs::write(dir.path().join("test.sln"), sln_content).unwrap();

        let vcx_content = r#"<?xml version="1.0" encoding="utf-8"?>
<Project DefaultTargets="Build" xmlns="http://schemas.microsoft.com/developer/msbuild/2003">
  <ItemGroup>
    <ClCompile Include="src\main.cpp" />
  </ItemGroup>
  <ItemGroup>
    <ClInclude Include="src\core.h" />
  </ItemGroup>
</Project>
"#;
        fs::write(dir.path().join("src/Core.vcxproj"), vcx_content).unwrap();

        let ctx = parse_solution(dir.path(), &dir.path().join("test.sln"));
        assert!(ctx.is_some(), "Expected solution to be parsed");
        let ctx = ctx.unwrap();
        assert_eq!(ctx.projects.len(), 1);
        assert_eq!(ctx.projects[0].project_name, "Core");
    }

    #[test]
    fn extracts_include_directories() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("include")).unwrap();

        let sln_content = r#"Microsoft Visual Studio Solution File, Format Version 12.00
Project("{FAE04EC0-301F-11D3-BF4B-00C04F79EFBC}") = "MyLib", "MyLib.vcxproj", "{GUID}"
EndProject
Global
EndGlobal
"#;
        fs::write(dir.path().join("test.sln"), sln_content).unwrap();

        let vcx_content = r#"<?xml version="1.0" encoding="utf-8"?>
<Project DefaultTargets="Build" xmlns="http://schemas.microsoft.com/developer/msbuild/2003">
  <PropertyGroup Condition="'$(Configuration)|$(Platform)'=='Debug|Win32'">
    <AdditionalIncludeDirectories>include;..\third_party\boost</AdditionalIncludeDirectories>
    <PreprocessorDefinitions>WIN32;_DEBUG;MY_DEFINE=1</PreprocessorDefinitions>
    <OutDir>$(SolutionDir)build\</OutDir>
  </PropertyGroup>
  <ItemGroup>
    <ClCompile Include="main.cpp" />
  </ItemGroup>
</Project>
"#;
        fs::write(dir.path().join("MyLib.vcxproj"), vcx_content).unwrap();

        let ctx = parse_solution(dir.path(), &dir.path().join("test.sln"));
        assert!(ctx.is_some(), "Expected solution to be parsed");
        let ctx = ctx.unwrap();
        assert_eq!(ctx.projects.len(), 1);
        let proj = &ctx.projects[0];
        assert!(proj.include_dirs.contains_key("Debug"), "Should have Debug config include dirs");
        let dirs = &proj.include_dirs["Debug"];
        assert!(dirs.iter().any(|d| d.contains("include")), "Should contain 'include' dir");
    }

    #[test]
    fn skips_non_cpp_projects() {
        let dir = tempdir().unwrap();

        let sln_content = r#"Microsoft Visual Studio Solution File, Format Version 12.00
Project("{8BC9CEB8-8B4A-11D0-8D11-00A0C91BC942}") = "CSharpProj", "CSharp.csproj", "{OTHERGUID}"
EndProject
Project("{FAE04EC0-301F-11D3-BF4B-00C04F79EFBC}") = "CppProj", "Cpp.vcxproj", "{CPPGUID}"
EndProject
Global
EndGlobal
"#;
        fs::write(dir.path().join("test.sln"), sln_content).unwrap();

        let vcx_content = r#"<?xml version="1.0" encoding="utf-8"?>
<Project DefaultTargets="Build" xmlns="http://schemas.microsoft.com/developer/msbuild/2003">
  <PropertyGroup Label="Configuration">
    <ProjectName>CppProj</ProjectName>
  </PropertyGroup>
  <ItemGroup>
    <ClCompile Include="main.cpp" />
  </ItemGroup>
</Project>
"#;
        fs::write(dir.path().join("Cpp.vcxproj"), vcx_content).unwrap();

        let ctx = parse_solution(dir.path(), &dir.path().join("test.sln"));
        assert!(ctx.is_some(), "Expected solution to be parsed");
        let ctx = ctx.unwrap();
        assert_eq!(ctx.projects.len(), 1);
        assert_eq!(ctx.projects[0].project_name, "CppProj");
    }

    // Regression: the include/define/outdir/targetname extractors previously
    // sliced the *whole file* (`raw`) using a `>` offset computed from the
    // current *line* (`trimmed`). That made every extracted value start right
    // after the file's first `>` (the `<?xml ... ?>` declaration), so values
    // were polluted with the leading XML text instead of holding the tag body.
    // These assertions check the *exact* extracted values, not a `contains`.
    #[test]
    fn extracts_clean_tag_values_without_leading_xml_pollution() {
        let dir = tempdir().unwrap();

        let vcx_content = r#"<?xml version="1.0" encoding="utf-8"?>
<Project DefaultTargets="Build" xmlns="http://schemas.microsoft.com/developer/msbuild/2003">
  <PropertyGroup Condition="'$(Configuration)|$(Platform)'=='Release|x64'">
    <AdditionalIncludeDirectories>src;include</AdditionalIncludeDirectories>
    <PreprocessorDefinitions>NDEBUG;DEMO_BUILD=1</PreprocessorDefinitions>
    <OutDir>build\out\</OutDir>
    <TargetName>app</TargetName>
  </PropertyGroup>
  <ItemGroup>
    <ClCompile Include="src\main.cpp" />
  </ItemGroup>
</Project>
"#;
        let vcx_path = dir.path().join("app.vcxproj");
        fs::write(&vcx_path, vcx_content).unwrap();

        let proj = parse_vcxproj_file(&vcx_path).expect("Expected vcxproj to parse");

        let dirs = proj.include_dirs.get("Release").expect("Release include dirs");
        assert_eq!(dirs, &vec!["src".to_string(), "include".to_string()]);

        let defs = proj.defines.get("Release").expect("Release defines");
        assert_eq!(defs, &vec!["NDEBUG".to_string(), "DEMO_BUILD=1".to_string()]);

        assert_eq!(proj.output_dir.get("Release").map(String::as_str), Some("build\\out\\"));
        assert_eq!(proj.target_name.get("Release").map(String::as_str), Some("app"));

        // No value may contain leaked XML markup from the file header.
        for value in dirs.iter().chain(defs.iter()) {
            assert!(!value.contains('<'), "value leaked XML markup: {value:?}");
            assert!(!value.contains("xml"), "value leaked XML declaration: {value:?}");
        }
    }
}

