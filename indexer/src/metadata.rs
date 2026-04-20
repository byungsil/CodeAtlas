use crate::build_metadata::BuildMetadataContext;
use crate::models::{FileRecord, FileRiskSignals, Symbol};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DerivedMetadata {
    pub module: Option<String>,
    pub subsystem: Option<String>,
    pub project_area: Option<String>,
    pub artifact_kind: Option<String>,
    pub header_role: Option<String>,
}

pub fn apply_metadata_to_symbol_with_context(symbol: &mut Symbol, build_metadata: Option<&BuildMetadataContext>) {
    let metadata = derive_metadata_with_context(&symbol.file_path, build_metadata);
    symbol.module = metadata.module;
    symbol.subsystem = metadata.subsystem;
    symbol.project_area = metadata.project_area;
    symbol.artifact_kind = metadata.artifact_kind;
    symbol.header_role = metadata.header_role;
}

pub fn apply_metadata_to_file_record_with_context(
    file_record: &mut FileRecord,
    build_metadata: Option<&BuildMetadataContext>,
) {
    let metadata = derive_metadata_with_context(&file_record.path, build_metadata);
    file_record.module = metadata.module;
    file_record.subsystem = metadata.subsystem;
    file_record.project_area = metadata.project_area;
    file_record.artifact_kind = metadata.artifact_kind;
    file_record.header_role = metadata.header_role;
}

pub fn apply_risk_signals_to_symbol(symbol: &mut Symbol, signals: &FileRiskSignals) {
    symbol.parse_fragility = Some(match signals.parse_fragility {
        crate::models::ParseFragility::Low => "low".into(),
        crate::models::ParseFragility::Elevated => "elevated".into(),
    });
    symbol.macro_sensitivity = Some(match signals.macro_sensitivity {
        crate::models::MacroSensitivity::Low => "low".into(),
        crate::models::MacroSensitivity::High => "high".into(),
    });
    symbol.include_heaviness = Some(match signals.include_heaviness {
        crate::models::IncludeHeaviness::Light => "light".into(),
        crate::models::IncludeHeaviness::Heavy => "heavy".into(),
    });
}

pub fn apply_risk_signals_to_file_record(file_record: &mut FileRecord, signals: &FileRiskSignals) {
    file_record.parse_fragility = Some(match signals.parse_fragility {
        crate::models::ParseFragility::Low => "low".into(),
        crate::models::ParseFragility::Elevated => "elevated".into(),
    });
    file_record.macro_sensitivity = Some(match signals.macro_sensitivity {
        crate::models::MacroSensitivity::Low => "low".into(),
        crate::models::MacroSensitivity::High => "high".into(),
    });
    file_record.include_heaviness = Some(match signals.include_heaviness {
        crate::models::IncludeHeaviness::Light => "light".into(),
        crate::models::IncludeHeaviness::Heavy => "heavy".into(),
    });
}

pub fn derive_metadata(path: &str) -> DerivedMetadata {
    derive_metadata_with_context(path, None)
}

pub fn derive_metadata_with_context(path: &str, build_metadata: Option<&BuildMetadataContext>) -> DerivedMetadata {
    let normalized = path.replace('\\', "/");
    let trimmed = normalized.trim_matches('/');
    let lower = trimmed.to_ascii_lowercase();
    let parts: Vec<&str> = trimmed.split('/').filter(|segment| !segment.is_empty()).collect();
    let parts_lower: Vec<String> = parts.iter().map(|segment| segment.to_ascii_lowercase()).collect();
    let ext = trimmed.rsplit('.').next().unwrap_or("").to_ascii_lowercase();

    let subsystem = derive_subsystem(&parts_lower);
    let module = derive_module(&parts_lower);
    let artifact_kind = derive_artifact_kind(&parts_lower, &lower);
    let project_area = derive_project_area(&parts_lower, &lower);
    let header_role = derive_header_role(&parts_lower, &ext);

    let mut metadata = DerivedMetadata {
        module,
        subsystem,
        project_area,
        artifact_kind,
        header_role,
    };

    if let Some(build_metadata) = build_metadata {
        if let Some(entry) = build_metadata.entry_for_file(trimmed) {
            apply_compile_entry_overlay(&mut metadata, entry);
        }
        if build_metadata.marks_public_header(trimmed) {
            metadata.header_role = Some("public".into());
        }
    }

    metadata
}

fn apply_compile_entry_overlay(metadata: &mut DerivedMetadata, entry: &crate::build_metadata::BuildMetadataEntry) {
    if let Some(output_path) = &entry.output_path {
        let output_metadata = derive_metadata(output_path);
        if metadata.module.is_none() {
            metadata.module = output_metadata.module;
        }
        if metadata.subsystem.is_none() {
            metadata.subsystem = output_metadata.subsystem;
        }
        if metadata.project_area.is_none() {
            metadata.project_area = output_metadata.project_area;
        }
        if should_override_artifact_kind(metadata.artifact_kind.as_deref(), output_metadata.artifact_kind.as_deref()) {
            metadata.artifact_kind = output_metadata.artifact_kind;
        }
    }

    if should_mark_test_from_defines(&entry.defines) {
        metadata.artifact_kind = Some("test".into());
    }
}

fn should_override_artifact_kind(current: Option<&str>, candidate: Option<&str>) -> bool {
    matches!(
        (current, candidate),
        (None, Some(_))
            | (Some("runtime"), Some("editor" | "tool" | "test" | "generated"))
            | (Some("generated"), Some("test"))
    )
}

fn should_mark_test_from_defines(defines: &[String]) -> bool {
    defines.iter().any(|define| {
        let upper = define.to_ascii_uppercase();
        upper.contains("TEST")
            || upper.contains("GTEST")
            || upper.contains("BENCHMARK")
            || upper.contains("UNIT_TEST")
    })
}

fn derive_subsystem(parts_lower: &[String]) -> Option<String> {
    let first = parts_lower.first()?.as_str();
    let subsystem = match first {
        "src" | "source" | "include" | "public" | "private" | "internal" => "runtime",
        "editor" | "editors" => "editor",
        "tools" | "tool" => "tools",
        "tests" | "test" | "spec" | "specs" => "tests",
        "generated" | "gen" | "autogen" => "generated",
        "modules" | "module" => "modules",
        "plugins" | "plugin" => "plugins",
        other => other,
    };
    Some(subsystem.to_string())
}

fn derive_module(parts_lower: &[String]) -> Option<String> {
    if parts_lower.is_empty() {
        return None;
    }

    let module = match parts_lower[0].as_str() {
        "modules" | "module" | "plugins" | "plugin" if parts_lower.len() > 1 => parts_lower[1].as_str(),
        "src" | "source" | "include" | "public" | "private" | "internal" if parts_lower.len() > 1 => {
            parts_lower[1].as_str()
        }
        other => other,
    };

    Some(module.to_string())
}

fn derive_artifact_kind(parts_lower: &[String], lower: &str) -> Option<String> {
    if contains_any(parts_lower, &["generated", "gen", "autogen"]) {
        return Some("generated".into());
    }
    if contains_any(parts_lower, &["tests", "test", "spec", "specs"]) || lower.contains("_test.") || lower.contains(".test.") {
        return Some("test".into());
    }
    if contains_any(parts_lower, &["editor", "editors"]) {
        return Some("editor".into());
    }
    if contains_any(parts_lower, &["tools", "tool"]) {
        return Some("tool".into());
    }
    Some("runtime".into())
}

fn derive_project_area(parts_lower: &[String], lower: &str) -> Option<String> {
    let mapping = [
        ("gameplay", "gameplay"),
        ("ui", "ui"),
        ("network", "networking"),
        ("net", "networking"),
        ("ai", "ai"),
        ("render", "rendering"),
        ("renderer", "rendering"),
        ("graphics", "rendering"),
        ("audio", "audio"),
        ("sound", "audio"),
        ("physics", "physics"),
        ("editor", "editor"),
        ("tool", "tools"),
        ("tools", "tools"),
        ("test", "tests"),
        ("tests", "tests"),
        ("generated", "generated"),
        ("engine", "engine"),
        ("core", "core"),
    ];

    for (needle, area) in mapping {
        if contains_any(parts_lower, &[needle]) || lower.contains(needle) {
            return Some(area.to_string());
        }
    }

    None
}

fn derive_header_role(parts_lower: &[String], ext: &str) -> Option<String> {
    if !matches!(ext, "h" | "hh" | "hpp" | "hxx" | "inl" | "inc") {
        return None;
    }

    if contains_any(parts_lower, &["public", "include"]) {
        return Some("public".into());
    }
    if contains_any(parts_lower, &["private", "src", "source"]) {
        return Some("private".into());
    }
    Some("internal".into())
}

fn contains_any(parts_lower: &[String], needles: &[&str]) -> bool {
    parts_lower
        .iter()
        .any(|part| needles.iter().any(|needle| part == needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_module_and_public_header_role_from_module_include_path() {
        let metadata = derive_metadata("modules/imgproc/include/opencv2/imgproc.hpp");
        assert_eq!(metadata.subsystem.as_deref(), Some("modules"));
        assert_eq!(metadata.module.as_deref(), Some("imgproc"));
        assert_eq!(metadata.header_role.as_deref(), Some("public"));
        assert_eq!(metadata.artifact_kind.as_deref(), Some("runtime"));
    }

    #[test]
    fn derives_test_and_project_area_from_test_path() {
        let metadata = derive_metadata("tests/ai/controller_update_test.cpp");
        assert_eq!(metadata.subsystem.as_deref(), Some("tests"));
        assert_eq!(metadata.module.as_deref(), Some("tests"));
        assert_eq!(metadata.artifact_kind.as_deref(), Some("test"));
        assert_eq!(metadata.project_area.as_deref(), Some("ai"));
        assert_eq!(metadata.header_role, None);
    }

    #[test]
    fn derives_editor_and_private_header_role_from_source_tree_path() {
        let metadata = derive_metadata("src/editor/viewport/private/panel.h");
        assert_eq!(metadata.subsystem.as_deref(), Some("runtime"));
        assert_eq!(metadata.module.as_deref(), Some("editor"));
        assert_eq!(metadata.artifact_kind.as_deref(), Some("editor"));
        assert_eq!(metadata.project_area.as_deref(), Some("editor"));
        assert_eq!(metadata.header_role.as_deref(), Some("private"));
    }

    #[test]
    fn compile_metadata_can_promote_header_role_to_public() {
        let context = BuildMetadataContext {
            source_path: "compile_commands.json".into(),
            translation_unit_count: 1,
            workspace_include_dirs: std::collections::HashSet::from(["sdk/api".to_string()]),
            entries_by_file: std::collections::HashMap::new(),
        };

        let metadata = derive_metadata_with_context("sdk/api/demo.hpp", Some(&context));
        assert_eq!(metadata.header_role.as_deref(), Some("public"));
    }

    #[test]
    fn compile_metadata_can_promote_runtime_file_to_test_from_output_path_and_defines() {
        let mut entries = std::collections::HashMap::new();
        entries.insert(
            "src/demo.cpp".to_string(),
            crate::build_metadata::BuildMetadataEntry {
                file_path: "src/demo.cpp".into(),
                output_path: Some("tests/demo_test.o".into()),
                include_dirs: vec!["include".into()],
                defines: vec!["UNIT_TEST".into()],
            },
        );
        let context = BuildMetadataContext {
            source_path: "compile_commands.json".into(),
            translation_unit_count: 1,
            workspace_include_dirs: std::collections::HashSet::new(),
            entries_by_file: entries,
        };

        let metadata = derive_metadata_with_context("src/demo.cpp", Some(&context));
        assert_eq!(metadata.artifact_kind.as_deref(), Some("test"));
    }
}
