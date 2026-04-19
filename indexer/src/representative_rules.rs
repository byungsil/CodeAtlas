use std::fs;
use std::path::Path;
use std::sync::{LazyLock, RwLock};

use serde::Deserialize;

use crate::models::Symbol;

pub const REPRESENTATIVE_RULES_FILENAME: &str = ".codeatlasrepresentative.json";

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RepresentativeRuleConfig {
    #[serde(default)]
    pub preferred_path_prefixes: Vec<String>,
    #[serde(default)]
    pub demoted_path_prefixes: Vec<String>,
    #[serde(default)]
    pub favored_artifact_kinds: Vec<String>,
    #[serde(default)]
    pub favored_header_roles: Vec<String>,
}

static ACTIVE_RULES: LazyLock<RwLock<RepresentativeRuleConfig>> =
    LazyLock::new(|| RwLock::new(RepresentativeRuleConfig::default()));

pub fn set_active_representative_rules(config: RepresentativeRuleConfig) {
    if let Ok(mut guard) = ACTIVE_RULES.write() {
        *guard = config;
    }
}

pub fn active_representative_rules() -> RepresentativeRuleConfig {
    ACTIVE_RULES
        .read()
        .map(|guard| guard.clone())
        .unwrap_or_default()
}

pub fn load_workspace_representative_rules(
    workspace_root: &Path,
) -> Result<RepresentativeRuleConfig, String> {
    let path = workspace_root.join(REPRESENTATIVE_RULES_FILENAME);
    if !path.exists() {
        return Ok(RepresentativeRuleConfig::default());
    }

    let raw = fs::read_to_string(&path)
        .map_err(|error| format!("Failed to read {}: {}", path.display(), error))?;
    let mut config: RepresentativeRuleConfig = serde_json::from_str(&raw)
        .map_err(|error| format!("Failed to parse {}: {}", path.display(), error))?;
    normalize_representative_rule_config(&mut config);
    Ok(config)
}

pub fn repository_rule_score(symbol: &Symbol, rules: &RepresentativeRuleConfig) -> i32 {
    if is_empty_config(rules) {
        return 0;
    }

    let mut score = 0;
    let normalized_path = normalize_path(symbol.file_path.as_str());

    if rules
        .preferred_path_prefixes
        .iter()
        .any(|prefix| normalized_path.starts_with(prefix))
    {
        score += 40;
    }

    if rules
        .demoted_path_prefixes
        .iter()
        .any(|prefix| normalized_path.starts_with(prefix))
    {
        score -= 40;
    }

    if symbol
        .artifact_kind
        .as_deref()
        .map(normalize_token)
        .as_deref()
        .is_some_and(|artifact_kind| rules.favored_artifact_kinds.iter().any(|kind| kind == artifact_kind))
    {
        score += 20;
    }

    if symbol
        .header_role
        .as_deref()
        .map(normalize_token)
        .as_deref()
        .is_some_and(|header_role| rules.favored_header_roles.iter().any(|role| role == header_role))
    {
        score += 15;
    }

    score
}

fn normalize_representative_rule_config(config: &mut RepresentativeRuleConfig) {
    config.preferred_path_prefixes = config
        .preferred_path_prefixes
        .iter()
        .map(|value| normalize_path(value))
        .filter(|value| !value.is_empty())
        .collect();
    config.demoted_path_prefixes = config
        .demoted_path_prefixes
        .iter()
        .map(|value| normalize_path(value))
        .filter(|value| !value.is_empty())
        .collect();
    config.favored_artifact_kinds = config
        .favored_artifact_kinds
        .iter()
        .map(|value| normalize_token(value))
        .filter(|value| !value.is_empty())
        .collect();
    config.favored_header_roles = config
        .favored_header_roles
        .iter()
        .map(|value| normalize_token(value))
        .filter(|value| !value.is_empty())
        .collect();
}

fn normalize_path(value: impl AsRef<str>) -> String {
    value
        .as_ref()
        .trim()
        .replace('\\', "/")
        .trim_matches('/')
        .to_ascii_lowercase()
}

fn normalize_token(value: impl AsRef<str>) -> String {
    value.as_ref().trim().to_ascii_lowercase()
}

fn is_empty_config(config: &RepresentativeRuleConfig) -> bool {
    config.preferred_path_prefixes.is_empty()
        && config.demoted_path_prefixes.is_empty()
        && config.favored_artifact_kinds.is_empty()
        && config.favored_header_roles.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Symbol;
    use tempfile::tempdir;

    fn make_symbol(file_path: &str) -> Symbol {
        Symbol {
            id: "Game::Widget".into(),
            name: "Widget".into(),
            qualified_name: "Game::Widget".into(),
            symbol_type: "class".into(),
            file_path: file_path.into(),
            line: 10,
            end_line: 20,
            signature: None,
            parameter_count: None,
            scope_qualified_name: None,
            scope_kind: None,
            symbol_role: Some("declaration".into()),
            declaration_file_path: None,
            declaration_line: None,
            declaration_end_line: None,
            definition_file_path: None,
            definition_line: None,
            definition_end_line: None,
            parent_id: None,
            module: None,
            subsystem: None,
            project_area: None,
            artifact_kind: Some("runtime".into()),
            header_role: Some("public".into()),
            parse_fragility: None,
            macro_sensitivity: None,
            include_heaviness: None,
        }
    }

    #[test]
    fn loads_and_normalizes_workspace_representative_rules() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join(REPRESENTATIVE_RULES_FILENAME),
            r#"{
  "preferredPathPrefixes": ["Engine\\Source\\Runtime"],
  "demotedPathPrefixes": ["Engine/Source/Editor"],
  "favoredArtifactKinds": ["Runtime"],
  "favoredHeaderRoles": ["Public"]
}"#,
        )
        .unwrap();

        let config = load_workspace_representative_rules(dir.path()).unwrap();
        assert_eq!(config.preferred_path_prefixes, vec!["engine/source/runtime"]);
        assert_eq!(config.demoted_path_prefixes, vec!["engine/source/editor"]);
        assert_eq!(config.favored_artifact_kinds, vec!["runtime"]);
        assert_eq!(config.favored_header_roles, vec!["public"]);
    }

    #[test]
    fn repository_rule_score_prefers_favored_prefixes_and_demotes_editor_paths() {
        let runtime_symbol = make_symbol("Engine/Source/Runtime/Core/Public/String.h");
        let editor_symbol = make_symbol("Engine/Source/Editor/Core/Public/String.h");
        let rules = RepresentativeRuleConfig {
            preferred_path_prefixes: vec!["engine/source/runtime".into()],
            demoted_path_prefixes: vec!["engine/source/editor".into()],
            favored_artifact_kinds: vec!["runtime".into()],
            favored_header_roles: vec!["public".into()],
        };

        assert!(repository_rule_score(&runtime_symbol, &rules) > repository_rule_score(&editor_symbol, &rules));
    }
}
