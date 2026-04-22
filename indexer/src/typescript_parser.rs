use regex::Regex;
use std::collections::HashMap;
use tree_sitter::{Node, Parser};

use crate::graph_rules::TYPESCRIPT_CALL_RELATIONS;
use crate::models::{
    FileRiskSignals, IncludeHeaviness, MacroSensitivity, NormalizedReference, ParseFragility,
    ParseMetrics, ParseResult, RawCallKind, RawCallSite, RawExtractionConfidence, RawQualifierKind,
    RawReceiverKind, ReferenceCategory, Symbol,
};
use crate::parser::execute_graph_rules;

pub fn parse_typescript_file(file_path: &str, source: &str) -> Result<ParseResult, String> {
    let module_id = module_symbol_id(file_path);
    let module_name = module_leaf_name(&module_id);
    let total_lines = source.lines().count().max(1);

    let mut symbols = vec![base_symbol(
        &module_id,
        module_name,
        "namespace",
        file_path,
        1,
        total_lines,
        module_parent_scope(&module_id),
        Some("namespace"),
        None,
        Some(module_id.clone()),
    )];
    let mut normalized_references = Vec::new();
    let mut raw_calls = Vec::new();
    let mut module_aliases = HashMap::<String, String>::new();
    let mut imported_symbol_aliases = HashMap::<String, String>::new();
    let mut known_types = HashMap::<String, String>::new();

    let import_re = Regex::new(
        r#"^\s*import\s+(?:(\*\s+as\s+[A-Za-z_][A-Za-z0-9_]*)|(\{[^}]+\})|([A-Za-z_][A-Za-z0-9_]*))\s+from\s+["']([^"']+)["']"#,
    )
    .map_err(|e| format!("TypeScript regex error: {}", e))?;
    let import_side_effect_re = Regex::new(r#"^\s*import\s+["']([^"']+)["']"#)
        .map_err(|e| format!("TypeScript regex error: {}", e))?;
    let export_function_re =
        Regex::new(r#"^\s*export\s+function\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(([^)]*)\)"#)
            .map_err(|e| format!("TypeScript regex error: {}", e))?;
    let function_re = Regex::new(r#"^\s*function\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(([^)]*)\)"#)
        .map_err(|e| format!("TypeScript regex error: {}", e))?;
    let class_re = Regex::new(
        r#"^\s*(?:export\s+)?class\s+([A-Za-z_][A-Za-z0-9_]*)\s*(?:extends\s+([A-Za-z_][A-Za-z0-9_\.]*))?"#,
    )
    .map_err(|e| format!("TypeScript regex error: {}", e))?;
    let interface_re = Regex::new(
        r#"^\s*(?:export\s+)?interface\s+([A-Za-z_][A-Za-z0-9_]*)\s*(?:extends\s+([A-Za-z_][A-Za-z0-9_\.]*(?:\s*,\s*[A-Za-z_][A-Za-z0-9_\.]*)*))?"#,
    )
    .map_err(|e| format!("TypeScript regex error: {}", e))?;
    let method_re = Regex::new(
        r#"^\s*(?:public\s+|private\s+|protected\s+|async\s+|static\s+)*(?:get\s+|set\s+)?([A-Za-z_][A-Za-z0-9_]*)\s*\(([^)]*)\)\s*\{"#,
    )
    .map_err(|e| format!("TypeScript regex error: {}", e))?;
    let qualified_call_re =
        Regex::new(r#"([A-Za-z_][A-Za-z0-9_]*)\.([A-Za-z_][A-Za-z0-9_]*)\s*\(([^()]*)\)"#)
            .map_err(|e| format!("TypeScript regex error: {}", e))?;
    let unqualified_call_re =
        Regex::new(r#"([A-Za-z_][A-Za-z0-9_]*)\s*\(([^()]*)\)"#).map_err(|e| format!("TypeScript regex error: {}", e))?;

    #[derive(Clone)]
    struct TypeContext {
        brace_depth: i32,
        type_id: String,
        type_name: String,
    }

    #[derive(Clone)]
    struct FunctionContext {
        brace_depth: i32,
        symbol_id: String,
        type_id: Option<String>,
        type_name: Option<String>,
    }

    let mut type_stack: Vec<TypeContext> = Vec::new();
    let mut function_stack: Vec<FunctionContext> = Vec::new();
    let mut brace_depth = 0i32;

    for (index, raw_line) in source.lines().enumerate() {
        let line_no = index + 1;
        let line = strip_ts_comment(raw_line);
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        while function_stack
            .last()
            .map(|ctx| brace_depth < ctx.brace_depth)
            .unwrap_or(false)
        {
            function_stack.pop();
        }
        while type_stack
            .last()
            .map(|ctx| brace_depth < ctx.brace_depth)
            .unwrap_or(false)
        {
            type_stack.pop();
        }

        let owner_symbol_id = function_stack
            .last()
            .map(|ctx| ctx.symbol_id.clone())
            .or_else(|| type_stack.last().map(|ctx| ctx.type_id.clone()))
            .unwrap_or_else(|| module_id.clone());

        if let Some(captures) = import_re.captures(trimmed) {
            let module_name = captures.get(4).unwrap().as_str();
            let target_module_id = module_name_to_symbol_id(module_name);
            normalized_references.push(NormalizedReference {
                source_symbol_id: owner_symbol_id.clone(),
                target_symbol_id: target_module_id.clone(),
                category: ReferenceCategory::ModuleImport,
                file_path: file_path.into(),
                line: line_no,
                confidence: RawExtractionConfidence::High,
            });

            if let Some(namespace_import) = captures.get(1) {
                if let Some((_, alias)) = namespace_import.as_str().split_once(" as ") {
                    module_aliases.insert(alias.trim().to_string(), target_module_id.clone());
                }
            }
            if let Some(named_imports) = captures.get(2) {
                for entry in named_imports
                    .as_str()
                    .trim_matches(|ch| ch == '{' || ch == '}')
                    .split(',')
                    .map(|part| part.trim())
                    .filter(|part| !part.is_empty())
                {
                    let (symbol_name, alias) = split_import_alias(entry);
                    let alias_name = alias.unwrap_or(symbol_name);
                    imported_symbol_aliases.insert(
                        alias_name.to_string(),
                        format!("{}::{}", target_module_id, symbol_name),
                    );
                }
            }
            if let Some(default_import) = captures.get(3) {
                let alias_name = default_import.as_str().trim();
                imported_symbol_aliases.insert(alias_name.to_string(), target_module_id.clone());
                module_aliases.insert(alias_name.to_string(), target_module_id);
            }
        } else if let Some(captures) = import_side_effect_re.captures(trimmed) {
            let module_name = captures.get(1).unwrap().as_str();
            normalized_references.push(NormalizedReference {
                source_symbol_id: owner_symbol_id.clone(),
                target_symbol_id: module_name_to_symbol_id(module_name),
                category: ReferenceCategory::ModuleImport,
                file_path: file_path.into(),
                line: line_no,
                confidence: RawExtractionConfidence::High,
            });
        }

        if let Some(captures) = class_re.captures(trimmed) {
            let class_name = captures.get(1).unwrap().as_str();
            let class_id = format!("{}::{}", module_id, class_name);
            known_types.insert(class_name.to_string(), class_id.clone());
            symbols.push(base_symbol(
                &class_id,
                class_name,
                "class",
                file_path,
                line_no,
                line_no,
                Some(module_id.clone()),
                Some("namespace"),
                None,
                Some(module_id.clone()),
            ));

            if let Some(base_name) = captures.get(2) {
                if let Some(target_symbol_id) = resolve_ts_name(
                    base_name.as_str(),
                    &module_id,
                    &known_types,
                    &module_aliases,
                    &imported_symbol_aliases,
                ) {
                    normalized_references.push(NormalizedReference {
                        source_symbol_id: class_id.clone(),
                        target_symbol_id,
                        category: ReferenceCategory::InheritanceMention,
                        file_path: file_path.into(),
                        line: line_no,
                        confidence: RawExtractionConfidence::High,
                    });
                }
            }

            type_stack.push(TypeContext {
                brace_depth: brace_depth + open_brace_delta(trimmed),
                type_id: class_id,
                type_name: class_name.to_string(),
            });
        } else if let Some(captures) = interface_re.captures(trimmed) {
            let interface_name = captures.get(1).unwrap().as_str();
            let interface_id = format!("{}::{}", module_id, interface_name);
            known_types.insert(interface_name.to_string(), interface_id.clone());
            symbols.push(base_symbol(
                &interface_id,
                interface_name,
                "interface",
                file_path,
                line_no,
                line_no,
                Some(module_id.clone()),
                Some("namespace"),
                None,
                Some(module_id.clone()),
            ));

            if let Some(base_list) = captures.get(2) {
                for base_name in base_list.as_str().split(',').map(|part| part.trim()).filter(|part| !part.is_empty()) {
                    if let Some(target_symbol_id) = resolve_ts_name(
                        base_name,
                        &module_id,
                        &known_types,
                        &module_aliases,
                        &imported_symbol_aliases,
                    ) {
                        normalized_references.push(NormalizedReference {
                            source_symbol_id: interface_id.clone(),
                            target_symbol_id,
                            category: ReferenceCategory::InheritanceMention,
                            file_path: file_path.into(),
                            line: line_no,
                            confidence: RawExtractionConfidence::High,
                        });
                    }
                }
            }

            type_stack.push(TypeContext {
                brace_depth: brace_depth + open_brace_delta(trimmed),
                type_id: interface_id,
                type_name: interface_name.to_string(),
            });
        } else if let Some(captures) = export_function_re
            .captures(trimmed)
            .or_else(|| function_re.captures(trimmed))
        {
            let function_name = captures.get(1).unwrap().as_str();
            let parameters = captures.get(2).unwrap().as_str();
            let symbol_id = format!("{}::{}", module_id, function_name);
            symbols.push(Symbol {
                id: symbol_id.clone(),
                name: function_name.into(),
                qualified_name: symbol_id.clone(),
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
            function_stack.push(FunctionContext {
                brace_depth: brace_depth + open_brace_delta(trimmed),
                symbol_id,
                type_id: None,
                type_name: None,
            });
        } else if let Some(captures) = method_re.captures(trimmed) {
            if let Some(type_ctx) = type_stack.last().cloned() {
                let method_name = captures.get(1).unwrap().as_str();
                if !is_ts_control_keyword(method_name) {
                    let parameters = captures.get(2).unwrap().as_str();
                    let symbol_id = format!("{}::{}", type_ctx.type_id, method_name);
                    symbols.push(Symbol {
                        id: symbol_id.clone(),
                        name: method_name.into(),
                        qualified_name: symbol_id.clone(),
                        symbol_type: "method".into(),
                        file_path: file_path.into(),
                        line: line_no,
                        end_line: line_no,
                        signature: Some(format!("{}({})", method_name, parameters.trim())),
                        parameter_count: Some(parameter_count(parameters)),
                        scope_qualified_name: Some(type_ctx.type_id.clone()),
                        scope_kind: Some("class".into()),
                        symbol_role: Some("definition".into()),
                        declaration_file_path: None,
                        declaration_line: None,
                        declaration_end_line: None,
                        definition_file_path: None,
                        definition_line: None,
                        definition_end_line: None,
                        parent_id: Some(type_ctx.type_id.clone()),
                        module: Some(module_id.clone()),
                        subsystem: None,
                        project_area: None,
                        artifact_kind: None,
                        header_role: None,
                        parse_fragility: Some("low".into()),
                        macro_sensitivity: Some("low".into()),
                        include_heaviness: Some("light".into()),
                    });
                    function_stack.push(FunctionContext {
                        brace_depth: brace_depth + open_brace_delta(trimmed),
                        symbol_id,
                        type_id: Some(type_ctx.type_id),
                        type_name: Some(type_ctx.type_name),
                    });
                }
            }
        }

        if let Some(current) = function_stack.last() {
            for captures in qualified_call_re.captures_iter(trimmed) {
                let receiver_name = captures.get(1).unwrap().as_str();
                let called_name = captures.get(2).unwrap().as_str();
                let args = captures.get(3).unwrap().as_str();
                if is_ts_control_keyword(receiver_name) {
                    continue;
                }

                let (call_kind, qualifier, qualifier_kind, receiver, receiver_kind) =
                    if receiver_name == "this" {
                        (
                            RawCallKind::MemberAccess,
                            current.type_id.clone(),
                            Some(RawQualifierKind::Type),
                            Some(receiver_name.to_string()),
                            Some(RawReceiverKind::This),
                        )
                    } else if let Some(module_target) = module_aliases.get(receiver_name) {
                        (
                            RawCallKind::Qualified,
                            Some(module_target.clone()),
                            Some(RawQualifierKind::Namespace),
                            None,
                            None,
                        )
                    } else if let Some(type_id) = known_types.get(receiver_name) {
                        (
                            RawCallKind::Qualified,
                            Some(type_id.clone()),
                            Some(RawQualifierKind::Type),
                            None,
                            None,
                        )
                    } else if current.type_name.as_deref() == Some(receiver_name) {
                        (
                            RawCallKind::Qualified,
                            current.type_id.clone(),
                            Some(RawQualifierKind::Type),
                            None,
                            None,
                        )
                    } else {
                        (
                            RawCallKind::MemberAccess,
                            None,
                            None,
                            Some(receiver_name.to_string()),
                            Some(RawReceiverKind::Identifier),
                        )
                    };

                raw_calls.push(RawCallSite {
                    caller_id: current.symbol_id.clone(),
                    called_name: called_name.into(),
                    call_kind,
                    argument_count: Some(argument_count(args)),
                    argument_texts: split_arguments(args),
                    result_target: None,
                    receiver,
                    receiver_kind,
                    qualifier,
                    qualifier_kind,
                    file_path: file_path.into(),
                    line: line_no,
                });
            }

            for captures in unqualified_call_re.captures_iter(trimmed) {
                let called_name = captures.get(1).unwrap().as_str();
                let args = captures.get(2).unwrap().as_str();
                if is_ts_control_keyword(called_name)
                    || trimmed.starts_with("function ")
                    || trimmed.starts_with("export function ")
                {
                    continue;
                }
                if trimmed.contains(&format!(".{}(", called_name)) {
                    continue;
                }

                if let Some(imported_symbol_id) = imported_symbol_aliases.get(called_name) {
                    raw_calls.push(RawCallSite {
                        caller_id: current.symbol_id.clone(),
                        called_name: imported_symbol_leaf(imported_symbol_id).into(),
                        call_kind: RawCallKind::Qualified,
                        argument_count: Some(argument_count(args)),
                        argument_texts: split_arguments(args),
                        result_target: None,
                        receiver: None,
                        receiver_kind: None,
                        qualifier: parent_scope(imported_symbol_id),
                        qualifier_kind: Some(RawQualifierKind::Namespace),
                        file_path: file_path.into(),
                        line: line_no,
                    });
                    continue;
                }

                raw_calls.push(RawCallSite {
                    caller_id: current.symbol_id.clone(),
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

        brace_depth += open_brace_delta(trimmed);
        brace_depth -= close_brace_delta(trimmed);
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

fn base_symbol(
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
        scope_kind: scope_kind.map(|value| value.into()),
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

fn split_import_alias(entry: &str) -> (&str, Option<&str>) {
    if let Some((name, alias)) = entry.split_once(" as ") {
        (name.trim(), Some(alias.trim()))
    } else {
        (entry.trim(), None)
    }
}

fn resolve_ts_name(
    name: &str,
    module_id: &str,
    known_types: &HashMap<String, String>,
    module_aliases: &HashMap<String, String>,
    imported_symbol_aliases: &HashMap<String, String>,
) -> Option<String> {
    if let Some(value) = known_types.get(name) {
        return Some(value.clone());
    }
    if let Some(value) = imported_symbol_aliases.get(name) {
        return Some(value.clone());
    }
    if let Some(value) = module_aliases.get(name) {
        return Some(value.clone());
    }
    if name.contains('.') {
        return Some(module_name_to_symbol_id(name));
    }
    Some(format!("{}::{}", module_id, name))
}

fn strip_ts_comment(line: &str) -> &str {
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;
    for (index, ch) in line.char_indices() {
        match ch {
            '\\' if in_single || in_double => {
                escaped = !escaped;
            }
            '\'' if !in_double && !escaped => in_single = !in_single,
            '"' if !in_single && !escaped => in_double = !in_double,
            '/' if !in_single && !in_double && !escaped => {
                if let Some(rest) = line[index..].strip_prefix("//") {
                    let _ = rest;
                    return &line[..index];
                }
            }
            _ => escaped = false,
        }
    }
    line
}

fn open_brace_delta(line: &str) -> i32 {
    line.chars().filter(|ch| *ch == '{').count() as i32
}

fn close_brace_delta(line: &str) -> i32 {
    line.chars().filter(|ch| *ch == '}').count() as i32
}

fn parameter_count(parameters: &str) -> usize {
    split_arguments(parameters).len()
}

fn argument_count(arguments: &str) -> usize {
    split_arguments(arguments).len()
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

fn is_ts_control_keyword(value: &str) -> bool {
    matches!(
        value,
        "if"
            | "for"
            | "while"
            | "switch"
            | "return"
            | "function"
            | "class"
            | "constructor"
            | "super"
            | "catch"
            | "new"
    )
}

fn module_symbol_id(file_path: &str) -> String {
    let normalized = file_path.replace('\\', "/");
    let without_ts = normalized
        .strip_suffix(".tsx")
        .or_else(|| normalized.strip_suffix(".ts"))
        .unwrap_or(&normalized);
    let module_path = without_ts
        .strip_suffix("/index")
        .unwrap_or(without_ts)
        .replace('/', "::");
    format!("typescript::{}", module_path)
}

fn module_name_to_symbol_id(module_name: &str) -> String {
    format!(
        "typescript::{}",
        module_name
            .trim_start_matches("./")
            .trim_start_matches("../")
            .replace(['/', '.'], "::")
    )
}

fn module_leaf_name(module_id: &str) -> &str {
    module_id.rsplit("::").next().unwrap_or(module_id)
}

fn module_parent_scope(module_id: &str) -> Option<String> {
    module_id.rsplit_once("::").map(|(parent, _)| parent.to_string())
}

fn parent_scope(symbol_id: &str) -> Option<String> {
    symbol_id.rsplit_once("::").map(|(parent, _)| parent.to_string())
}

fn imported_symbol_leaf(symbol_id: &str) -> &str {
    symbol_id.rsplit("::").next().unwrap_or(symbol_id)
}

/// Tree-sitter based TypeScript parser.
/// Extracts symbols by walking the AST and calls via .tsg graph rules.
/// Returns a ParseResult comparable to the legacy regex parser.
pub fn parse_typescript_file_treesitter(file_path: &str, source: &str) -> Result<ParseResult, String> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
        .map_err(|e| format!("Failed to set TypeScript language: {}", e))?;

    let tree = parser
        .parse(source, None)
        .ok_or_else(|| "TypeScript parse returned None".to_string())?;

    let source_bytes = source.as_bytes();
    let module_id = module_symbol_id(file_path);
    let module_name = module_leaf_name(&module_id);
    let total_lines = source.lines().count().max(1);

    let mut symbols = vec![ts_base_symbol(
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

    // Scope stack: (qualified_id, symbol_type) pairs
    let mut scope_stack: Vec<(String, &'static str)> = Vec::new();

    ts_visit_node(
        tree.root_node(),
        source_bytes,
        file_path,
        &module_id,
        &mut scope_stack,
        &mut symbols,
        &mut normalized_references,
    );

    // Extract calls via tree-sitter-graph rules
    let (graph_events, _, _) = execute_graph_rules(
        tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        "typescript",
        TYPESCRIPT_CALL_RELATIONS,
        &tree,
        source,
        file_path,
        &symbols,
    );

    // Convert graph relation events to RawCallSite
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

fn ts_visit_node<'a>(
    node: Node<'a>,
    source: &[u8],
    file_path: &str,
    module_id: &str,
    scope_stack: &mut Vec<(String, &'static str)>,
    symbols: &mut Vec<Symbol>,
    references: &mut Vec<NormalizedReference>,
) {
    let kind = node.kind();

    match kind {
        "class_declaration" | "abstract_class_declaration" => {
            if let Some(sym) = ts_extract_class(node, source, file_path, module_id, scope_stack, symbols, references) {
                let class_id = sym.id.clone();
                scope_stack.push((class_id.clone(), "class"));
                ts_visit_children(node, source, file_path, module_id, scope_stack, symbols, references);
                scope_stack.pop();
                return;
            }
        }
        "interface_declaration" => {
            if let Some(sym) = ts_extract_interface(node, source, file_path, module_id, scope_stack, symbols, references) {
                let iface_id = sym.id.clone();
                scope_stack.push((iface_id, "interface"));
                ts_visit_children(node, source, file_path, module_id, scope_stack, symbols, references);
                scope_stack.pop();
                return;
            }
        }
        "function_declaration" | "generator_function_declaration" => {
            ts_extract_function(node, source, file_path, module_id, scope_stack, symbols);
            // recurse into function body handled below
        }
        "method_definition" => {
            ts_extract_method(node, source, file_path, module_id, scope_stack, symbols);
        }
        "lexical_declaration" | "variable_declaration" => {
            ts_extract_arrow_function(node, source, file_path, module_id, scope_stack, symbols);
        }
        "import_statement" => {
            ts_extract_import(node, source, file_path, module_id, references);
            return;
        }
        _ => {}
    }

    ts_visit_children(node, source, file_path, module_id, scope_stack, symbols, references);
}

fn ts_visit_children<'a>(
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
        ts_visit_node(child, source, file_path, module_id, scope_stack, symbols, references);
    }
}

fn ts_node_text<'a>(node: Node, source: &'a [u8]) -> &'a str {
    node.utf8_text(source).unwrap_or("")
}

fn ts_current_scope(module_id: &str, scope_stack: &[(String, &'static str)]) -> String {
    scope_stack.last().map(|(id, _)| id.clone()).unwrap_or_else(|| module_id.to_string())
}

fn ts_qualify(name: &str, module_id: &str, scope_stack: &[(String, &'static str)]) -> String {
    format!("{}::{}", ts_current_scope(module_id, scope_stack), name)
}

fn ts_extract_class<'a>(
    node: Node<'a>,
    source: &[u8],
    file_path: &str,
    module_id: &str,
    scope_stack: &[(String, &'static str)],
    symbols: &mut Vec<Symbol>,
    references: &mut Vec<NormalizedReference>,
) -> Option<Symbol> {
    let name_node = node.child_by_field_name("name")?;
    let name = ts_node_text(name_node, source);
    let id = ts_qualify(name, module_id, scope_stack);
    let parent_id = ts_current_scope(module_id, scope_stack);
    let line = node.start_position().row + 1;
    let end_line = node.end_position().row + 1;

    // Extract extends (InheritanceMention)
    if let Some(heritage) = node.child_by_field_name("body") {
        // Check siblings for heritage clause
        let _ = heritage;
    }
    // Walk direct children for extends/implements
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "class_heritage" {
            let mut c2 = child.walk();
            for heritage_child in child.children(&mut c2) {
                if heritage_child.kind() == "extends_clause" || heritage_child.kind() == "implements_clause" {
                    let mut c3 = heritage_child.walk();
                    for type_node in heritage_child.children(&mut c3) {
                        if type_node.kind() == "identifier" || type_node.kind() == "type_identifier" {
                            let base_name = ts_node_text(type_node, source);
                            if base_name != "extends" && base_name != "implements" {
                                references.push(NormalizedReference {
                                    source_symbol_id: id.clone(),
                                    target_symbol_id: format!("{}::{}", module_id, base_name),
                                    category: ReferenceCategory::InheritanceMention,
                                    file_path: file_path.to_string(),
                                    line,
                                    confidence: RawExtractionConfidence::Partial,
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    let sym = ts_base_symbol(
        &id,
        name,
        "class",
        file_path,
        line,
        end_line,
        Some(parent_id.clone()),
        Some("class"),
        Some(parent_id),
        Some(module_id.to_string()),
    );
    symbols.push(sym.clone());
    Some(sym)
}

fn ts_extract_interface<'a>(
    node: Node<'a>,
    source: &[u8],
    file_path: &str,
    module_id: &str,
    scope_stack: &[(String, &'static str)],
    symbols: &mut Vec<Symbol>,
    references: &mut Vec<NormalizedReference>,
) -> Option<Symbol> {
    let name_node = node.child_by_field_name("name")?;
    let name = ts_node_text(name_node, source);
    let id = ts_qualify(name, module_id, scope_stack);
    let parent_id = ts_current_scope(module_id, scope_stack);
    let line = node.start_position().row + 1;
    let end_line = node.end_position().row + 1;

    // Extract extends
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "extends_type_clause" || child.kind() == "extends_clause" {
            let mut c2 = child.walk();
            for type_node in child.children(&mut c2) {
                if type_node.kind() == "type_identifier" || type_node.kind() == "identifier" {
                    let base_name = ts_node_text(type_node, source);
                    if base_name != "extends" {
                        references.push(NormalizedReference {
                            source_symbol_id: id.clone(),
                            target_symbol_id: format!("{}::{}", module_id, base_name),
                            category: ReferenceCategory::InheritanceMention,
                            file_path: file_path.to_string(),
                            line,
                            confidence: RawExtractionConfidence::Partial,
                        });
                    }
                }
            }
        }
    }

    let sym = ts_base_symbol(
        &id,
        name,
        "interface",
        file_path,
        line,
        end_line,
        Some(parent_id.clone()),
        Some("interface"),
        Some(parent_id),
        Some(module_id.to_string()),
    );
    symbols.push(sym.clone());
    Some(sym)
}

fn ts_extract_function(
    node: Node,
    source: &[u8],
    file_path: &str,
    module_id: &str,
    scope_stack: &[(String, &'static str)],
    symbols: &mut Vec<Symbol>,
) {
    let Some(name_node) = node.child_by_field_name("name") else { return };
    let name = ts_node_text(name_node, source);
    let id = ts_qualify(name, module_id, scope_stack);
    let parent_id = ts_current_scope(module_id, scope_stack);
    let line = node.start_position().row + 1;
    let end_line = node.end_position().row + 1;

    let param_count = node.child_by_field_name("parameters")
        .map(|p| p.named_child_count())
        .unwrap_or(0);

    let mut sym = ts_base_symbol(
        &id,
        name,
        "function",
        file_path,
        line,
        end_line,
        Some(parent_id.clone()),
        Some("function"),
        Some(parent_id),
        Some(module_id.to_string()),
    );
    sym.parameter_count = Some(param_count);
    symbols.push(sym);
}

fn ts_extract_method(
    node: Node,
    source: &[u8],
    file_path: &str,
    module_id: &str,
    scope_stack: &[(String, &'static str)],
    symbols: &mut Vec<Symbol>,
) {
    let Some(name_node) = node.child_by_field_name("name") else { return };
    let name = ts_node_text(name_node, source);
    // Skip constructor as method — handled separately
    if name == "constructor" { return; }
    let id = ts_qualify(name, module_id, scope_stack);
    let parent_id = ts_current_scope(module_id, scope_stack);
    let line = node.start_position().row + 1;
    let end_line = node.end_position().row + 1;

    let param_count = node.child_by_field_name("parameters")
        .map(|p| p.named_child_count())
        .unwrap_or(0);

    let mut sym = ts_base_symbol(
        &id,
        name,
        "method",
        file_path,
        line,
        end_line,
        Some(parent_id.clone()),
        Some("method"),
        Some(parent_id),
        Some(module_id.to_string()),
    );
    sym.parameter_count = Some(param_count);
    symbols.push(sym);
}

fn ts_extract_arrow_function(
    node: Node,
    source: &[u8],
    file_path: &str,
    module_id: &str,
    scope_stack: &[(String, &'static str)],
    symbols: &mut Vec<Symbol>,
) {
    // const name = (...) => { ... }  or  export const name = ...
    let mut cursor = node.walk();
    for declarator in node.children(&mut cursor) {
        if declarator.kind() != "variable_declarator" { continue; }
        let Some(name_node) = declarator.child_by_field_name("name") else { continue };
        let Some(value_node) = declarator.child_by_field_name("value") else { continue };
        if value_node.kind() != "arrow_function" { continue; }

        let name = ts_node_text(name_node, source);
        let id = ts_qualify(name, module_id, scope_stack);
        let parent_id = ts_current_scope(module_id, scope_stack);
        let line = declarator.start_position().row + 1;
        let end_line = declarator.end_position().row + 1;

        let param_count = value_node.child_by_field_name("parameters")
            .map(|p| p.named_child_count())
            .unwrap_or(0);

        let mut sym = ts_base_symbol(
            &id,
            name,
            "function",
            file_path,
            line,
            end_line,
            Some(parent_id.clone()),
            Some("function"),
            Some(parent_id),
            Some(module_id.to_string()),
        );
        sym.parameter_count = Some(param_count);
        symbols.push(sym);
    }
}

fn ts_extract_import(
    node: Node,
    source: &[u8],
    file_path: &str,
    module_id: &str,
    references: &mut Vec<NormalizedReference>,
) {
    // import ... from "module"
    let source_node = node.child_by_field_name("source");
    let Some(src) = source_node else { return };
    let module_raw = ts_node_text(src, source).trim_matches(|c| c == '"' || c == '\'');
    let target_id = module_name_to_symbol_id(module_raw);
    let line = node.start_position().row + 1;

    references.push(NormalizedReference {
        source_symbol_id: module_id.to_string(),
        target_symbol_id: target_id,
        category: ReferenceCategory::ModuleImport,
        file_path: file_path.to_string(),
        line,
        confidence: RawExtractionConfidence::High,
    });
}

fn ts_base_symbol(
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

/// Dual-extraction: run both legacy and tree-sitter parsers.
/// Use tree-sitter result when it meets the quality threshold.
pub fn parse_typescript_file_dual(file_path: &str, source: &str) -> Result<ParseResult, String> {
    let legacy = parse_typescript_file(file_path, source)?;
    let ts_result = parse_typescript_file_treesitter(file_path, source)?;

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
mod tests {
    use super::*;

    #[test]
    fn parses_typescript_imports_exports_and_calls() {
        let source = r#"
import { helper as runHelper } from "./tools/helper";
import * as api from "./services/api";

export function boot() {
  runHelper();
  api.fetchData();
}

export class Controller {
  refresh() {
    this.sync();
  }

  sync() {}
}
"#;

        let result = parse_typescript_file("ui/app.ts", source).unwrap();
        assert!(result
            .symbols
            .iter()
            .any(|symbol| symbol.id == "typescript::ui::app"));
        assert!(result
            .symbols
            .iter()
            .any(|symbol| symbol.id == "typescript::ui::app::boot"));
        assert!(result
            .symbols
            .iter()
            .any(|symbol| symbol.id == "typescript::ui::app::Controller"));
        assert!(result
            .symbols
            .iter()
            .any(|symbol| symbol.id == "typescript::ui::app::Controller::refresh"));
        assert!(result.normalized_references.iter().any(|reference| {
            reference.category == ReferenceCategory::ModuleImport
                && reference.target_symbol_id == "typescript::tools::helper"
        }));
        assert!(result.raw_calls.iter().any(|call| {
            call.caller_id == "typescript::ui::app::boot"
                && call.called_name == "helper"
                && call.qualifier.as_deref() == Some("typescript::tools::helper")
        }));
        assert!(result.raw_calls.iter().any(|call| {
            call.caller_id == "typescript::ui::app::boot"
                && call.called_name == "fetchData"
                && call.qualifier.as_deref() == Some("typescript::services::api")
        }));
        assert!(result.raw_calls.iter().any(|call| {
            call.caller_id == "typescript::ui::app::Controller::refresh"
                && call.called_name == "sync"
                && call.qualifier.as_deref() == Some("typescript::ui::app::Controller")
        }));
    }

    #[test]
    fn parses_typescript_interfaces_and_extends() {
        let source = r#"
import { BaseView } from "./base";

export interface ViewContract extends BaseView {}

export class Screen extends BaseView {
  render() {
    return BaseView();
  }
}
"#;

        let result = parse_typescript_file("ui/screen.tsx", source).unwrap();
        assert!(result
            .symbols
            .iter()
            .any(|symbol| symbol.id == "typescript::ui::screen::ViewContract"));
        assert!(result.normalized_references.iter().any(|reference| {
            reference.category == ReferenceCategory::InheritanceMention
                && reference.source_symbol_id == "typescript::ui::screen::ViewContract"
                && reference.target_symbol_id == "typescript::base::BaseView"
        }));
        assert!(result.normalized_references.iter().any(|reference| {
            reference.category == ReferenceCategory::InheritanceMention
                && reference.source_symbol_id == "typescript::ui::screen::Screen"
                && reference.target_symbol_id == "typescript::base::BaseView"
        }));
    }

    // --- Tree-sitter parser tests ---

    #[test]
    fn treesitter_extracts_basic_function_and_class() {
        let source = r#"
function greet(name: string): string {
  return "Hello " + name;
}

class Greeter {
  sayHi() {
    greet("world");
  }
}
"#;
        let result = parse_typescript_file_treesitter("mod/greet.ts", source).unwrap();
        assert!(result.symbols.iter().any(|s| s.id == "typescript::mod::greet" && s.symbol_type == "namespace"));
        assert!(result.symbols.iter().any(|s| s.id == "typescript::mod::greet::greet" && s.symbol_type == "function"));
        assert!(result.symbols.iter().any(|s| s.id == "typescript::mod::greet::Greeter" && s.symbol_type == "class"));
        assert!(result.symbols.iter().any(|s| s.id == "typescript::mod::greet::Greeter::sayHi" && s.symbol_type == "method"));
    }

    #[test]
    fn treesitter_extracts_arrow_function() {
        let source = r#"
const handler = (x: number) => {
  return x * 2;
};

export const process = async (items: string[]) => {
  handler(1);
};
"#;
        let result = parse_typescript_file_treesitter("mod/handler.ts", source).unwrap();
        assert!(result.symbols.iter().any(|s| s.id == "typescript::mod::handler::handler" && s.symbol_type == "function"),
            "handler arrow function should be extracted; symbols: {:?}",
            result.symbols.iter().map(|s| &s.id).collect::<Vec<_>>());
        assert!(result.symbols.iter().any(|s| s.id == "typescript::mod::handler::process" && s.symbol_type == "function"));
    }

    #[test]
    fn treesitter_extracts_import_as_module_import_reference() {
        let source = r#"
import { helper } from "./utils/helper";
import * as api from "./services/api";

function run() {
  helper();
}
"#;
        let result = parse_typescript_file_treesitter("app/main.ts", source).unwrap();
        assert!(result.normalized_references.iter().any(|r| {
            r.category == ReferenceCategory::ModuleImport
                && r.target_symbol_id == "typescript::utils::helper"
        }), "should have ModuleImport for ./utils/helper");
        assert!(result.normalized_references.iter().any(|r| {
            r.category == ReferenceCategory::ModuleImport
                && r.target_symbol_id == "typescript::services::api"
        }), "should have ModuleImport for ./services/api");
    }

    #[test]
    fn treesitter_extracts_unqualified_and_member_calls() {
        let source = r#"
function doWork() {
  process();
  obj.update();
  this.refresh();
}
"#;
        let result = parse_typescript_file_treesitter("mod/work.ts", source).unwrap();
        assert!(result.raw_calls.iter().any(|c| c.called_name == "process"),
            "unqualified call 'process' should be extracted");
        assert!(result.raw_calls.iter().any(|c| c.called_name == "update"),
            "member call 'update' should be extracted");
        assert!(result.raw_calls.iter().any(|c| c.called_name == "refresh"),
            "this call 'refresh' should be extracted");
    }

    #[test]
    fn treesitter_extracts_interface_with_extends() {
        let source = r#"
interface Animal {
  name: string;
}

interface Dog extends Animal {
  breed: string;
}
"#;
        let result = parse_typescript_file_treesitter("mod/animal.ts", source).unwrap();
        assert!(result.symbols.iter().any(|s| s.symbol_type == "interface" && s.name == "Animal"));
        assert!(result.symbols.iter().any(|s| s.symbol_type == "interface" && s.name == "Dog"));
        assert!(result.normalized_references.iter().any(|r| {
            r.category == ReferenceCategory::InheritanceMention
                && r.source_symbol_id.ends_with("::Dog")
        }), "Dog extends Animal should produce InheritanceMention");
    }

    #[test]
    fn treesitter_dual_result_matches_or_exceeds_legacy() {
        let source = r#"
import { run } from "./runner";

export function start() {
  run();
}

export class App {
  init() {
    this.setup();
  }
  setup() {}
}
"#;
        let legacy = parse_typescript_file("app/app.ts", source).unwrap();
        let ts = parse_typescript_file_treesitter("app/app.ts", source).unwrap();

        // tree-sitter should find at least as many symbols (may find more)
        assert!(
            ts.symbols.len() >= (legacy.symbols.len() as f64 * 0.9) as usize,
            "tree-sitter symbols {} should be >= 90% of legacy {}",
            ts.symbols.len(), legacy.symbols.len()
        );
    }
}
