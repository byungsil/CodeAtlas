use regex::Regex;

use crate::models::{
    FileRiskSignals, IncludeHeaviness, MacroSensitivity, NormalizedReference, ParseFragility,
    ParseMetrics, ParseResult, RawCallKind, RawCallSite, RawExtractionConfidence, RawQualifierKind,
    ReferenceCategory, Symbol,
};

pub fn parse_lua_file(file_path: &str, source: &str) -> Result<ParseResult, String> {
    let module_id = module_symbol_id(file_path);
    let module_name = module_leaf_name(&module_id);
    let total_lines = source.lines().count().max(1);

    let mut symbols = vec![Symbol {
        id: module_id.clone(),
        name: module_name.to_string(),
        qualified_name: module_id.clone(),
        symbol_type: "namespace".into(),
        file_path: file_path.into(),
        line: 1,
        end_line: total_lines,
        signature: None,
        parameter_count: None,
        scope_qualified_name: module_parent_scope(&module_id),
        scope_kind: Some("namespace".into()),
        symbol_role: Some("definition".into()),
        declaration_file_path: None,
        declaration_line: None,
        declaration_end_line: None,
        definition_file_path: None,
        definition_line: None,
        definition_end_line: None,
        parent_id: None,
        module: Some(module_id.clone()),
        subsystem: None,
        project_area: None,
        artifact_kind: None,
        header_role: None,
        parse_fragility: Some("low".into()),
        macro_sensitivity: Some("low".into()),
        include_heaviness: Some("light".into()),
    }];

    let mut normalized_references = Vec::new();
    let mut raw_calls = Vec::new();
    let mut module_aliases = std::collections::HashMap::<String, String>::new();
    let mut known_tables = std::collections::HashSet::<String>::new();
    let mut known_table_ids = std::collections::HashMap::<String, String>::new();

    let local_function_re = Regex::new(r#"^\s*local\s+function\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(([^)]*)\)"#)
        .map_err(|e| format!("Lua regex error: {}", e))?;
    let global_function_re = Regex::new(r#"^\s*function\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(([^)]*)\)"#)
        .map_err(|e| format!("Lua regex error: {}", e))?;
    let table_function_re =
        Regex::new(r#"^\s*function\s+([A-Za-z_][A-Za-z0-9_]*)[\.:]([A-Za-z_][A-Za-z0-9_]*)\s*\(([^)]*)\)"#)
            .map_err(|e| format!("Lua regex error: {}", e))?;
    let require_re = Regex::new(
        r#"(?x)
        (?:
            local \s+ ([A-Za-z_][A-Za-z0-9_]*) \s* = \s*
        )?
        require \s* \( \s* ["']([A-Za-z0-9_./-]+)["'] \s* \)
        "#,
    )
    .map_err(|e| format!("Lua regex error: {}", e))?;
    let qualified_call_re =
        Regex::new(r#"([A-Za-z_][A-Za-z0-9_]*)[\.:]([A-Za-z_][A-Za-z0-9_]*)\s*\(([^)]*)\)"#)
            .map_err(|e| format!("Lua regex error: {}", e))?;
    let unqualified_call_re =
        Regex::new(r#"([A-Za-z_][A-Za-z0-9_]*)\s*\(([^)]*)\)"#).map_err(|e| format!("Lua regex error: {}", e))?;
    let block_open_if_re = Regex::new(r#"^\s*if\b.*\bthen\b"#).unwrap();
    let block_open_for_re = Regex::new(r#"^\s*for\b.*\bdo\b"#).unwrap();
    let block_open_while_re = Regex::new(r#"^\s*while\b.*\bdo\b"#).unwrap();
    let block_open_repeat_re = Regex::new(r#"^\s*repeat\b"#).unwrap();
    let block_open_do_re = Regex::new(r#"^\s*do\b"#).unwrap();
    let block_close_end_re = Regex::new(r#"^\s*end\b"#).unwrap();
    let block_close_until_re = Regex::new(r#"^\s*until\b"#).unwrap();

    #[derive(Clone)]
    struct FunctionContext {
        caller_id: String,
        block_depth: i32,
    }

    let mut contexts: Vec<FunctionContext> = Vec::new();

    for (index, raw_line) in source.lines().enumerate() {
        let line_no = index + 1;
        let line = strip_lua_comment(raw_line);
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let owner_symbol_id = contexts
            .last()
            .map(|ctx| ctx.caller_id.clone())
            .unwrap_or_else(|| module_id.clone());

        for captures in require_re.captures_iter(trimmed) {
            let alias = captures.get(1).map(|value| value.as_str().to_string());
            let required_module = captures.get(2).unwrap().as_str();
            let target_module_id = module_name_to_symbol_id(required_module);

            if let Some(alias_name) = alias {
                module_aliases.insert(alias_name, target_module_id.clone());
            }

            normalized_references.push(NormalizedReference {
                source_symbol_id: owner_symbol_id.clone(),
                target_symbol_id: target_module_id,
                category: ReferenceCategory::ModuleImport,
                file_path: file_path.into(),
                line: line_no,
                confidence: RawExtractionConfidence::High,
            });
        }

        if let Some(captures) = table_function_re.captures(trimmed) {
            let table_name = captures.get(1).unwrap().as_str();
            let function_name = captures.get(2).unwrap().as_str();
            let parameters = captures.get(3).unwrap().as_str();
            let table_id = format!("{}::{}", module_id, table_name);

            if known_tables.insert(table_name.to_string()) {
                known_table_ids.insert(table_name.to_string(), table_id.clone());
                symbols.push(Symbol {
                    id: table_id.clone(),
                    name: table_name.into(),
                    qualified_name: table_id.clone(),
                    symbol_type: "namespace".into(),
                    file_path: file_path.into(),
                    line: line_no,
                    end_line: line_no,
                    signature: None,
                    parameter_count: None,
                    scope_qualified_name: Some(module_id.clone()),
                    scope_kind: Some("namespace".into()),
                    symbol_role: Some("definition".into()),
                    declaration_file_path: None,
                    declaration_line: None,
                    declaration_end_line: None,
                    definition_file_path: None,
                    definition_line: None,
                    definition_end_line: None,
                    parent_id: None,
                    module: Some(module_id.clone()),
                    subsystem: None,
                    project_area: None,
                    artifact_kind: None,
                    header_role: None,
                    parse_fragility: Some("low".into()),
                    macro_sensitivity: Some("low".into()),
                    include_heaviness: Some("light".into()),
                });
            }

            let function_id = format!("{}::{}", table_id, function_name);
            symbols.push(Symbol {
                id: function_id.clone(),
                name: function_name.into(),
                qualified_name: function_id.clone(),
                symbol_type: "method".into(),
                file_path: file_path.into(),
                line: line_no,
                end_line: line_no,
                signature: Some(format!("{}({})", function_name, parameters.trim())),
                parameter_count: Some(parameter_count(parameters)),
                scope_qualified_name: Some(table_id.clone()),
                scope_kind: Some("namespace".into()),
                symbol_role: Some("definition".into()),
                declaration_file_path: None,
                declaration_line: None,
                declaration_end_line: None,
                definition_file_path: None,
                definition_line: None,
                definition_end_line: None,
                parent_id: Some(table_id.clone()),
                module: Some(module_id.clone()),
                subsystem: None,
                project_area: None,
                artifact_kind: None,
                header_role: None,
                parse_fragility: Some("low".into()),
                macro_sensitivity: Some("low".into()),
                include_heaviness: Some("light".into()),
            });
            contexts.push(FunctionContext {
                caller_id: function_id,
                block_depth: 1,
            });
            continue;
        }

        if let Some(captures) = local_function_re.captures(trimmed).or_else(|| global_function_re.captures(trimmed)) {
            let function_name = captures.get(1).unwrap().as_str();
            let parameters = captures.get(2).unwrap().as_str();
            let function_id = format!("{}::{}", module_id, function_name);

            symbols.push(Symbol {
                id: function_id.clone(),
                name: function_name.into(),
                qualified_name: function_id.clone(),
                symbol_type: "function".into(),
                file_path: file_path.into(),
                line: line_no,
                end_line: line_no,
                signature: Some(format!("{}({})", function_name, parameters.trim())),
                parameter_count: Some(parameter_count(parameters)),
                scope_qualified_name: Some(module_id.clone()),
                scope_kind: Some("namespace".into()),
                symbol_role: Some("definition".into()),
                declaration_file_path: None,
                declaration_line: None,
                declaration_end_line: None,
                definition_file_path: None,
                definition_line: None,
                definition_end_line: None,
                parent_id: None,
                module: Some(module_id.clone()),
                subsystem: None,
                project_area: None,
                artifact_kind: None,
                header_role: None,
                parse_fragility: Some("low".into()),
                macro_sensitivity: Some("low".into()),
                include_heaviness: Some("light".into()),
            });
            contexts.push(FunctionContext {
                caller_id: function_id,
                block_depth: 1,
            });
            continue;
        }

        if let Some(current) = contexts.last() {
            for captures in qualified_call_re.captures_iter(trimmed) {
                let qualifier_name = captures.get(1).unwrap().as_str();
                let called_name = captures.get(2).unwrap().as_str();
                let args = captures.get(3).unwrap().as_str();
                if is_lua_keyword(qualifier_name) {
                    continue;
                }

                let qualifier = module_aliases
                    .get(qualifier_name)
                    .cloned()
                    .or_else(|| known_table_ids.get(qualifier_name).cloned())
                    .unwrap_or_else(|| qualifier_name.to_string());
                let qualifier_kind = if module_aliases.contains_key(qualifier_name) {
                    Some(RawQualifierKind::Namespace)
                } else {
                    Some(RawQualifierKind::Type)
                };

                raw_calls.push(RawCallSite {
                    caller_id: current.caller_id.clone(),
                    called_name: called_name.into(),
                    call_kind: RawCallKind::Qualified,
                    argument_count: Some(argument_count(args)),
                    argument_texts: split_arguments(args),
                    result_target: None,
                    receiver: None,
                    receiver_kind: None,
                    qualifier: Some(qualifier),
                    qualifier_kind,
                    file_path: file_path.into(),
                    line: line_no,
                });
            }

            for captures in unqualified_call_re.captures_iter(trimmed) {
                let called_name = captures.get(1).unwrap().as_str();
                let args = captures.get(2).unwrap().as_str();
                if is_lua_keyword(called_name) || trimmed.starts_with("require") || trimmed.contains(&format!("function {}", called_name)) {
                    continue;
                }
                if qualified_call_re.is_match(trimmed) && trimmed.contains(&format!("{}(", called_name)) {
                    if trimmed.contains('.') || trimmed.contains(':') {
                        continue;
                    }
                }

                raw_calls.push(RawCallSite {
                    caller_id: current.caller_id.clone(),
                    called_name: called_name.into(),
                    call_kind: RawCallKind::Unqualified,
                    argument_count: Some(argument_count(args)),
                    argument_texts: split_arguments(args),
                    result_target: None,
                    receiver: None,
                    receiver_kind: None,
                    qualifier: None,
                    qualifier_kind: None,
                    file_path: file_path.into(),
                    line: line_no,
                });
            }
        }

        if let Some(current) = contexts.last_mut() {
            if block_open_if_re.is_match(trimmed)
                || block_open_for_re.is_match(trimmed)
                || block_open_while_re.is_match(trimmed)
                || block_open_repeat_re.is_match(trimmed)
                || block_open_do_re.is_match(trimmed)
            {
                current.block_depth += 1;
            }
            if block_close_end_re.is_match(trimmed) || block_close_until_re.is_match(trimmed) {
                current.block_depth -= 1;
                if current.block_depth <= 0 {
                    contexts.pop();
                }
            }
        }
    }

    Ok(ParseResult {
        symbols,
        file_risk_signals: FileRiskSignals {
            parse_fragility: ParseFragility::Low,
            macro_sensitivity: MacroSensitivity::Low,
            include_heaviness: IncludeHeaviness::Light,
        },
        relation_events: Vec::new(),
        normalized_references,
        propagation_events: Vec::new(),
        callable_flow_summaries: Vec::new(),
        raw_calls,
        metrics: ParseMetrics::default(),
    })
}

fn strip_lua_comment(line: &str) -> &str {
    line.split("--").next().unwrap_or(line)
}

fn parameter_count(parameters: &str) -> usize {
    let trimmed = parameters.trim();
    if trimmed.is_empty() {
        return 0;
    }
    trimmed.split(',').count()
}

fn argument_count(arguments: &str) -> usize {
    let trimmed = arguments.trim();
    if trimmed.is_empty() {
        return 0;
    }
    trimmed.split(',').count()
}

fn split_arguments(arguments: &str) -> Vec<String> {
    let trimmed = arguments.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    trimmed
        .split(',')
        .map(|arg| arg.trim().to_string())
        .filter(|arg| !arg.is_empty())
        .collect()
}

fn is_lua_keyword(value: &str) -> bool {
    matches!(
        value,
        "if"
            | "for"
            | "while"
            | "repeat"
            | "until"
            | "return"
            | "local"
            | "function"
            | "end"
            | "do"
            | "then"
            | "elseif"
            | "require"
    )
}

fn module_symbol_id(file_path: &str) -> String {
    let normalized = file_path.replace('\\', "/");
    let without_ext = normalized.strip_suffix(".lua").unwrap_or(&normalized);
    let module_path = without_ext
        .strip_suffix("/init")
        .unwrap_or(without_ext)
        .replace('/', "::");
    format!("lua::{}", module_path)
}

fn module_name_to_symbol_id(module_name: &str) -> String {
    format!("lua::{}", module_name.replace(['.', '/'], "::"))
}

fn module_leaf_name(module_id: &str) -> &str {
    module_id.rsplit("::").next().unwrap_or(module_id)
}

fn module_parent_scope(module_id: &str) -> Option<String> {
    module_id.rsplit_once("::").map(|(parent, _)| parent.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lua_module_functions_requires_and_calls() {
        let source = r#"
local util = require("game.util")

function update()
  util.tick()
  refresh()
end

function refresh()
end
"#;

        let result = parse_lua_file("game/main.lua", source).unwrap();
        assert!(result.symbols.iter().any(|symbol| symbol.id == "lua::game::main"));
        assert!(result.symbols.iter().any(|symbol| symbol.id == "lua::game::main::update"));
        assert!(result.symbols.iter().any(|symbol| symbol.id == "lua::game::main::refresh"));
        assert!(result.normalized_references.iter().any(|reference| {
            reference.category == ReferenceCategory::ModuleImport
                && reference.target_symbol_id == "lua::game::util"
        }));
        assert!(result.raw_calls.iter().any(|call| {
            call.caller_id == "lua::game::main::update"
                && call.called_name == "tick"
                && call.qualifier.as_deref() == Some("lua::game::util")
        }));
        assert!(result.raw_calls.iter().any(|call| {
            call.caller_id == "lua::game::main::update"
                && call.called_name == "refresh"
                && matches!(call.call_kind, RawCallKind::Unqualified)
        }));
    }

    #[test]
    fn parses_lua_table_attached_functions() {
        let source = r#"
function State.enter(player)
  State.exit(player)
end

function State.exit(player)
end
"#;

        let result = parse_lua_file("game/state.lua", source).unwrap();
        assert!(result.symbols.iter().any(|symbol| symbol.id == "lua::game::state::State"));
        assert!(result.symbols.iter().any(|symbol| symbol.id == "lua::game::state::State::enter"));
        assert!(result.raw_calls.iter().any(|call| {
            call.caller_id == "lua::game::state::State::enter"
                && call.called_name == "exit"
                && call.qualifier.as_deref() == Some("lua::game::state::State")
        }));
    }
}
