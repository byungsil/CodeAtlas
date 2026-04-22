use regex::Regex;
use tree_sitter::{Node, Parser};

use crate::graph_rules::LUA_CALL_RELATIONS;
use crate::models::{
    FileRiskSignals, IncludeHeaviness, MacroSensitivity, NormalizedReference, ParseFragility,
    ParseMetrics, ParseResult, RawCallKind, RawCallSite, RawExtractionConfidence, RawQualifierKind,
    ReferenceCategory, Symbol,
};
use crate::parser::execute_graph_rules;

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

/// Tree-sitter based Lua parser.
pub fn parse_lua_file_treesitter(file_path: &str, source: &str) -> Result<ParseResult, String> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_lua::LANGUAGE.into())
        .map_err(|e| format!("Failed to set Lua language: {}", e))?;

    let tree = parser
        .parse(source, None)
        .ok_or_else(|| "Lua parse returned None".to_string())?;

    let source_bytes = source.as_bytes();
    let module_id = module_symbol_id(file_path);
    let module_name = module_leaf_name(&module_id);
    let total_lines = source.lines().count().max(1);

    let mut symbols = vec![lua_base_symbol(
        &module_id,
        module_name,
        "namespace",
        file_path,
        1,
        total_lines,
        None,
        Some("namespace"),
        None,
        Some(module_id.clone()),
    )];
    let mut normalized_references: Vec<NormalizedReference> = Vec::new();
    let mut scope_stack: Vec<(String, &'static str)> = Vec::new();

    lua_visit_node(
        tree.root_node(),
        source_bytes,
        file_path,
        &module_id,
        &mut scope_stack,
        &mut symbols,
        &mut normalized_references,
    );

    let (graph_events, _, _) = execute_graph_rules(
        tree_sitter_lua::LANGUAGE.into(),
        "lua",
        LUA_CALL_RELATIONS,
        &tree,
        source,
        file_path,
        &symbols,
    );

    let raw_calls: Vec<RawCallSite> = graph_events
        .into_iter()
        .filter_map(|event| event.to_raw_call_site())
        .collect();

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

fn lua_visit_node<'a>(
    node: Node<'a>,
    source: &[u8],
    file_path: &str,
    module_id: &str,
    scope_stack: &mut Vec<(String, &'static str)>,
    symbols: &mut Vec<Symbol>,
    references: &mut Vec<NormalizedReference>,
) {
    match node.kind() {
        "function_declaration" => {
            lua_extract_function_decl(node, source, file_path, module_id, scope_stack, symbols);
            // still recurse for nested functions
        }
        "local_function" => {
            lua_extract_local_function(node, source, file_path, module_id, scope_stack, symbols);
        }
        "function_call" => {
            // Check for require("module") calls
            lua_extract_require(node, source, file_path, module_id, scope_stack, references);
        }
        _ => {}
    }
    lua_visit_children(node, source, file_path, module_id, scope_stack, symbols, references);
}

fn lua_visit_children<'a>(
    node: Node<'a>,
    source: &[u8],
    file_path: &str,
    module_id: &str,
    scope_stack: &mut Vec<(String, &'static str)>,
    symbols: &mut Vec<Symbol>,
    references: &mut Vec<NormalizedReference>,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        lua_visit_node(child, source, file_path, module_id, scope_stack, symbols, references);
    }
}

fn lua_node_text<'a>(node: Node, source: &'a [u8]) -> &'a str {
    node.utf8_text(source).unwrap_or("")
}

fn lua_current_scope(module_id: &str, scope_stack: &[(String, &'static str)]) -> String {
    scope_stack.last().map(|(id, _)| id.clone()).unwrap_or_else(|| module_id.to_string())
}

fn lua_qualify(name: &str, module_id: &str, scope_stack: &[(String, &'static str)]) -> String {
    format!("{}::{}", lua_current_scope(module_id, scope_stack), name)
}

fn lua_extract_function_decl(
    node: Node,
    source: &[u8],
    file_path: &str,
    module_id: &str,
    scope_stack: &[(String, &'static str)],
    symbols: &mut Vec<Symbol>,
) {
    // function name() -- global or table.method or table:method
    let Some(name_node) = node.child_by_field_name("name") else { return };
    let name_text = lua_node_text(name_node, source);
    let line = node.start_position().row + 1;
    let end_line = node.end_position().row + 1;
    let parent_id = lua_current_scope(module_id, scope_stack);

    if name_text.contains('.') || name_text.contains(':') {
        // Table method: State.enter or State:enter
        let sep = if name_text.contains(':') { ':' } else { '.' };
        let parts: Vec<&str> = name_text.splitn(2, sep).collect();
        if parts.len() == 2 {
            let table_name = parts[0];
            let method_name = parts[1];
            let table_id = format!("{}::{}", parent_id, table_name);
            let method_id = format!("{}::{}", table_id, method_name);

            // Ensure namespace/table symbol exists
            if !symbols.iter().any(|s| s.id == table_id) {
                let table_sym = lua_base_symbol(&table_id, table_name, "namespace", file_path, line, end_line,
                    Some(parent_id.clone()), Some("namespace"), Some(parent_id.clone()), Some(module_id.to_string()));
                symbols.push(table_sym);
            }
            let sym = lua_base_symbol(&method_id, method_name, "function", file_path, line, end_line,
                Some(table_id.clone()), Some("function"), Some(table_id), Some(module_id.to_string()));
            symbols.push(sym);
        }
    } else {
        let id = lua_qualify(name_text, module_id, scope_stack);
        let sym = lua_base_symbol(&id, name_text, "function", file_path, line, end_line,
            Some(parent_id.clone()), Some("function"), Some(parent_id), Some(module_id.to_string()));
        symbols.push(sym);
    }
}

fn lua_extract_local_function(
    node: Node,
    source: &[u8],
    file_path: &str,
    module_id: &str,
    scope_stack: &[(String, &'static str)],
    symbols: &mut Vec<Symbol>,
) {
    let Some(name_node) = node.child_by_field_name("name") else { return };
    let name = lua_node_text(name_node, source);
    let id = lua_qualify(name, module_id, scope_stack);
    let parent_id = lua_current_scope(module_id, scope_stack);
    let line = node.start_position().row + 1;
    let end_line = node.end_position().row + 1;
    let sym = lua_base_symbol(&id, name, "function", file_path, line, end_line,
        Some(parent_id.clone()), Some("function"), Some(parent_id), Some(module_id.to_string()));
    symbols.push(sym);
}

fn lua_extract_require(
    node: Node,
    source: &[u8],
    file_path: &str,
    module_id: &str,
    scope_stack: &[(String, &'static str)],
    references: &mut Vec<NormalizedReference>,
) {
    // Check if this is a require() call
    let name_node = node.child_by_field_name("name");
    let is_require = name_node.map(|n| lua_node_text(n, source) == "require").unwrap_or(false);
    if !is_require { return; }

    let caller_id = lua_current_scope(module_id, scope_stack);
    let line = node.start_position().row + 1;

    // Get the string argument
    if let Some(args_node) = node.child_by_field_name("arguments") {
        let mut cursor = args_node.walk();
        for child in args_node.children(&mut cursor) {
            let text = lua_node_text(child, source);
            let trimmed = text.trim_matches(|c| c == '"' || c == '\'' || c == '(' || c == ')').trim();
            if !trimmed.is_empty() && !trimmed.starts_with('(') {
                let target_id = module_name_to_symbol_id(trimmed);
                references.push(NormalizedReference {
                    source_symbol_id: caller_id.clone(),
                    target_symbol_id: target_id,
                    category: ReferenceCategory::ModuleImport,
                    file_path: file_path.to_string(),
                    line,
                    confidence: RawExtractionConfidence::High,
                });
                break;
            }
        }
    }
}

fn lua_base_symbol(
    id: &str,
    name: &str,
    symbol_type: &str,
    file_path: &str,
    line: usize,
    end_line: usize,
    scope_qualified_name: Option<String>,
    scope_kind: Option<&str>,
    parent_id: Option<String>,
    module: Option<String>,
) -> Symbol {
    Symbol {
        id: id.into(),
        name: name.into(),
        qualified_name: id.into(),
        symbol_type: symbol_type.into(),
        file_path: file_path.into(),
        line,
        end_line,
        signature: None,
        parameter_count: None,
        scope_qualified_name,
        scope_kind: scope_kind.map(|v| v.into()),
        symbol_role: Some("definition".into()),
        declaration_file_path: None,
        declaration_line: None,
        declaration_end_line: None,
        definition_file_path: None,
        definition_line: None,
        definition_end_line: None,
        parent_id,
        module,
        subsystem: None,
        project_area: None,
        artifact_kind: None,
        header_role: None,
        parse_fragility: Some("low".into()),
        macro_sensitivity: Some("low".into()),
        include_heaviness: Some("light".into()),
    }
}

/// Dual-extraction for Lua.
pub fn parse_lua_file_dual(file_path: &str, source: &str) -> Result<ParseResult, String> {
    let legacy = parse_lua_file(file_path, source)?;
    let ts_result = parse_lua_file_treesitter(file_path, source)?;

    let symbol_threshold = legacy.symbols.len() as f64 * 0.9;
    let call_threshold = legacy.raw_calls.len() as f64 * 0.8;

    if ts_result.symbols.len() as f64 >= symbol_threshold
        && ts_result.raw_calls.len() as f64 >= call_threshold
    {
        Ok(ts_result)
    } else {
        Ok(legacy)
    }
}

#[cfg(test)]
mod treesitter_tests {
    use super::*;

    #[test]
    fn treesitter_extracts_global_functions() {
        let source = r#"
function update()
  refresh()
end

function refresh()
end
"#;
        let result = parse_lua_file_treesitter("game/main.lua", source).unwrap();
        assert!(result.symbols.iter().any(|s| s.id == "lua::game::main" && s.symbol_type == "namespace"));
        assert!(result.symbols.iter().any(|s| s.id == "lua::game::main::update" && s.symbol_type == "function"),
            "update should be extracted; symbols: {:?}", result.symbols.iter().map(|s| &s.id).collect::<Vec<_>>());
        assert!(result.symbols.iter().any(|s| s.id == "lua::game::main::refresh" && s.symbol_type == "function"));
    }

    #[test]
    fn treesitter_extracts_table_methods() {
        let source = r#"
function State.enter(player)
  State.exit(player)
end

function State.exit(player)
end
"#;
        let result = parse_lua_file_treesitter("game/state.lua", source).unwrap();
        assert!(result.symbols.iter().any(|s| s.name == "State"),
            "State table namespace should be extracted; symbols: {:?}",
            result.symbols.iter().map(|s| format!("{}:{}", s.name, s.symbol_type)).collect::<Vec<_>>());
        assert!(result.symbols.iter().any(|s| s.name == "enter"),
            "enter method should be extracted");
    }

    #[test]
    fn treesitter_extracts_require_as_module_import() {
        let source = r#"
local util = require("game.util")

function main()
  util.tick()
end
"#;
        let result = parse_lua_file_treesitter("game/main.lua", source).unwrap();
        assert!(result.normalized_references.iter().any(|r| {
            r.category == ReferenceCategory::ModuleImport
                && r.target_symbol_id == "lua::game::util"
        }), "require('game.util') should produce ModuleImport; refs: {:?}",
            result.normalized_references.iter().map(|r| &r.target_symbol_id).collect::<Vec<_>>());
    }

    #[test]
    fn treesitter_extracts_dot_and_colon_calls() {
        let source = r#"
function main()
  util.tick()
  obj:update()
  process()
end
"#;
        let result = parse_lua_file_treesitter("game/main.lua", source).unwrap();
        assert!(result.raw_calls.iter().any(|c| c.called_name == "tick"),
            "dot call 'util.tick' should be extracted");
        assert!(result.raw_calls.iter().any(|c| c.called_name == "update"),
            "colon call 'obj:update' should be extracted");
        assert!(result.raw_calls.iter().any(|c| c.called_name == "process"),
            "unqualified call 'process' should be extracted");
    }

    #[test]
    fn treesitter_dual_result_matches_or_exceeds_legacy() {
        let source = r#"
local util = require("game.util")

function update()
  util.tick()
  refresh()
end

function refresh()
end
"#;
        let legacy = parse_lua_file("game/main.lua", source).unwrap();
        let ts = parse_lua_file_treesitter("game/main.lua", source).unwrap();
        assert!(
            ts.symbols.len() >= (legacy.symbols.len() as f64 * 0.9) as usize,
            "tree-sitter symbols {} should be >= 90% of legacy {}",
            ts.symbols.len(), legacy.symbols.len()
        );
    }
}
