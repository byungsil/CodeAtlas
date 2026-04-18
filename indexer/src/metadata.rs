use crate::models::{FileRecord, Symbol};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DerivedMetadata {
    pub module: Option<String>,
    pub subsystem: Option<String>,
    pub project_area: Option<String>,
    pub artifact_kind: Option<String>,
    pub header_role: Option<String>,
}

pub fn apply_metadata_to_symbol(symbol: &mut Symbol) {
    let metadata = derive_metadata(&symbol.file_path);
    symbol.module = metadata.module;
    symbol.subsystem = metadata.subsystem;
    symbol.project_area = metadata.project_area;
    symbol.artifact_kind = metadata.artifact_kind;
    symbol.header_role = metadata.header_role;
}

pub fn apply_metadata_to_file_record(file_record: &mut FileRecord) {
    let metadata = derive_metadata(&file_record.path);
    file_record.module = metadata.module;
    file_record.subsystem = metadata.subsystem;
    file_record.project_area = metadata.project_area;
    file_record.artifact_kind = metadata.artifact_kind;
    file_record.header_role = metadata.header_role;
}

pub fn derive_metadata(path: &str) -> DerivedMetadata {
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

    DerivedMetadata {
        module,
        subsystem,
        project_area,
        artifact_kind,
        header_role,
    }
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
}
