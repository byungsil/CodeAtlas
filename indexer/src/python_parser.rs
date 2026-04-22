use regex::Regex;
use std::collections::HashMap;
use tree_sitter::{Node, Parser};

use crate::graph_rules::PYTHON_CALL_RELATIONS;
use crate::models::{
    FileRiskSignals, IncludeHeaviness, MacroSensitivity, NormalizedReference, ParseFragility,
    ParseMetrics, ParseResult, RawCallKind, RawCallSite, RawExtractionConfidence, RawQualifierKind,
    RawReceiverKind, ReferenceCategory, Symbol,
};
use crate::parser::execute_graph_rules;

pub fn parse_python_file(file_path: &str, source: &str) -> Result<ParseResult, String> {
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
    let mut known_classes = HashMap::<String, String>::new();

    let class_re = Regex::new(r#"^\s*class\s+([A-Za-z_][A-Za-z0-9_]*)\s*(?:\(([^)]*)\))?\s*:"#)
        .map_err(|e| format!("Python regex error: {}", e))?;
    let function_re = Regex::new(r#"^\s*def\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(([^)]*)\)\s*:"#)
        .map_err(|e| format!("Python regex error: {}", e))?;
    let import_re = Regex::new(r#"^\s*import\s+(.+)$"#).map_err(|e| format!("Python regex error: {}", e))?;
    let from_import_re =
        Regex::new(r#"^\s*from\s+([A-Za-z_][A-Za-z0-9_\.]*)\s+import\s+(.+)$"#)
            .map_err(|e| format!("Python regex error: {}", e))?;
    let qualified_call_re =
        Regex::new(r#"([A-Za-z_][A-Za-z0-9_]*)\.([A-Za-z_][A-Za-z0-9_]*)\s*\(([^()]*)\)"#)
            .map_err(|e| format!("Python regex error: {}", e))?;
    let unqualified_call_re =
        Regex::new(r#"([A-Za-z_][A-Za-z0-9_]*)\s*\(([^()]*)\)"#).map_err(|e| format!("Python regex error: {}", e))?;

    #[derive(Clone)]
    struct ClassContext {
        indent: usize,
        class_id: String,
        class_name: String,
    }

    #[derive(Clone)]
    struct FunctionContext {
        indent: usize,
        symbol_id: String,
        class_id: Option<String>,
        class_name: Option<String>,
    }

    let mut class_stack: Vec<ClassContext> = Vec::new();
    let mut function_stack: Vec<FunctionContext> = Vec::new();

    for (index, raw_line) in source.lines().enumerate() {
        let line_no = index + 1;
        let line = strip_python_comment(raw_line);
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let indent = indentation_width(line);
        while function_stack
            .last()
            .map(|ctx| indent <= ctx.indent)
            .unwrap_or(false)
        {
            function_stack.pop();
        }
        while class_stack
            .last()
            .map(|ctx| indent <= ctx.indent)
            .unwrap_or(false)
        {
            class_stack.pop();
        }

        let owner_symbol_id = function_stack
            .last()
            .map(|ctx| ctx.symbol_id.clone())
            .or_else(|| class_stack.last().map(|ctx| ctx.class_id.clone()))
            .unwrap_or_else(|| module_id.clone());

        if let Some(captures) = import_re.captures(trimmed) {
            let imports = captures.get(1).unwrap().as_str();
            for entry in imports.split(',').map(|part| part.trim()).filter(|part| !part.is_empty()) {
                let (module_name, alias) = split_import_alias(entry);
                let target_module_id = module_name_to_symbol_id(module_name);
                normalized_references.push(NormalizedReference {
                    source_symbol_id: owner_symbol_id.clone(),
                    target_symbol_id: target_module_id.clone(),
                    category: ReferenceCategory::ModuleImport,
                    file_path: file_path.into(),
                    line: line_no,
                    confidence: RawExtractionConfidence::High,
                });
                let alias_name = alias.unwrap_or_else(|| module_name.rsplit('.').next().unwrap_or(module_name));
                module_aliases.insert(alias_name.to_string(), target_module_id);
            }
            continue;
        }

        if let Some(captures) = from_import_re.captures(trimmed) {
            let module_name = captures.get(1).unwrap().as_str();
            let imported_items = captures.get(2).unwrap().as_str();
            let target_module_id = module_name_to_symbol_id(module_name);
            normalized_references.push(NormalizedReference {
                source_symbol_id: owner_symbol_id.clone(),
                target_symbol_id: target_module_id.clone(),
                category: ReferenceCategory::ModuleImport,
                file_path: file_path.into(),
                line: line_no,
                confidence: RawExtractionConfidence::High,
            });
            for entry in imported_items
                .trim_matches(|c| c == '(' || c == ')')
                .split(',')
                .map(|part| part.trim())
                .filter(|part| !part.is_empty() && *part != "*")
            {
                let (symbol_name, alias) = split_import_alias(entry);
                let alias_name = alias.unwrap_or(symbol_name);
                imported_symbol_aliases.insert(
                    alias_name.to_string(),
                    format!("{}::{}", target_module_id, symbol_name),
                );
            }
            continue;
        }

        if let Some(captures) = class_re.captures(trimmed) {
            let class_name = captures.get(1).unwrap().as_str();
            let bases = captures.get(2).map(|value| value.as_str()).unwrap_or("");
            let class_id = format!("{}::{}", module_id, class_name);
            known_classes.insert(class_name.to_string(), class_id.clone());
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

            for base in split_csv_like(bases) {
                if let Some(target_symbol_id) = resolve_python_name(
                    base.as_str(),
                    &module_id,
                    &known_classes,
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

            class_stack.push(ClassContext {
                indent,
                class_id,
                class_name: class_name.to_string(),
            });
            continue;
        }

        if let Some(captures) = function_re.captures(trimmed) {
            let function_name = captures.get(1).unwrap().as_str();
            let parameters = captures.get(2).unwrap().as_str();
            let current_class = class_stack.last().cloned();
            let (symbol_id, symbol_type, scope_qualified_name, parent_id, scope_kind) =
                if let Some(class_ctx) = &current_class {
                    (
                        format!("{}::{}", class_ctx.class_id, function_name),
                        "method",
                        Some(class_ctx.class_id.clone()),
                        Some(class_ctx.class_id.clone()),
                        Some("class"),
                    )
                } else {
                    (
                        format!("{}::{}", module_id, function_name),
                        "function",
                        Some(module_id.clone()),
                        None,
                        Some("namespace"),
                    )
                };

            symbols.push(Symbol {
                id: symbol_id.clone(),
                name: function_name.into(),
                qualified_name: symbol_id.clone(),
                symbol_type: symbol_type.into(),
                file_path: file_path.into(),
                line: line_no,
                end_line: line_no,
                signature: Some(format!("{}({})", function_name, parameters.trim())),
                parameter_count: Some(parameter_count(parameters)),
                scope_qualified_name,
                scope_kind: scope_kind.map(|kind| kind.into()),
                symbol_role: Some("definition".into()),
                declaration_file_path: None,
                declaration_line: None,
                declaration_end_line: None,
                definition_file_path: None,
                definition_line: None,
                definition_end_line: None,
                parent_id,
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
                indent,
                symbol_id,
                class_id: current_class.as_ref().map(|ctx| ctx.class_id.clone()),
                class_name: current_class.as_ref().map(|ctx| ctx.class_name.clone()),
            });
            continue;
        }

        if let Some(current) = function_stack.last() {
            for captures in qualified_call_re.captures_iter(trimmed) {
                let receiver_name = captures.get(1).unwrap().as_str();
                let called_name = captures.get(2).unwrap().as_str();
                let args = captures.get(3).unwrap().as_str();
                if is_python_keyword(receiver_name) {
                    continue;
                }

                let (call_kind, qualifier, qualifier_kind, receiver, receiver_kind) =
                    if receiver_name == "self" {
                        (
                            RawCallKind::MemberAccess,
                            current.class_id.clone(),
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
                    } else if let Some(class_id) = known_classes.get(receiver_name) {
                        (
                            RawCallKind::Qualified,
                            Some(class_id.clone()),
                            Some(RawQualifierKind::Type),
                            None,
                            None,
                        )
                    } else if current.class_name.as_deref() == Some(receiver_name) {
                        (
                            RawCallKind::Qualified,
                            current.class_id.clone(),
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
                if is_python_keyword(called_name)
                    || trimmed.starts_with("def ")
                    || trimmed.starts_with("class ")
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

fn split_csv_like(value: &str) -> Vec<String> {
    value.split(',')
        .map(|part| part.trim().to_string())
        .filter(|part| !part.is_empty())
        .collect()
}

fn resolve_python_name(
    name: &str,
    _module_id: &str,
    known_classes: &HashMap<String, String>,
    module_aliases: &HashMap<String, String>,
    imported_symbol_aliases: &HashMap<String, String>,
) -> Option<String> {
    if let Some(value) = known_classes.get(name) {
        return Some(value.clone());
    }
    if let Some(value) = imported_symbol_aliases.get(name) {
        return Some(value.clone());
    }
    if let Some(value) = module_aliases.get(name) {
        return Some(value.clone());
    }
    if name.contains('.') {
        let (head, tail) = name.split_once('.')?;
        if let Some(module_target) = module_aliases.get(head) {
            return Some(format!("{}::{}", module_target, tail.replace('.', "::")));
        }
        return None;
    }
    None
}

fn strip_python_comment(line: &str) -> &str {
    let mut in_single = false;
    let mut in_double = false;
    for (index, ch) in line.char_indices() {
        match ch {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '#' if !in_single && !in_double => return &line[..index],
            _ => {}
        }
    }
    line
}

fn indentation_width(line: &str) -> usize {
    line.chars()
        .take_while(|ch| ch.is_whitespace())
        .map(|ch| if ch == '\t' { 4 } else { 1 })
        .sum()
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

fn is_python_keyword(value: &str) -> bool {
    matches!(
        value,
        "if"
            | "for"
            | "while"
            | "return"
            | "def"
            | "class"
            | "lambda"
            | "with"
            | "raise"
            | "yield"
            | "assert"
            | "await"
            | "async"
    )
}

fn module_symbol_id(file_path: &str) -> String {
    let normalized = file_path.replace('\\', "/");
    let without_ext = normalized.strip_suffix(".py").unwrap_or(&normalized);
    let module_path = without_ext
        .strip_suffix("/__init__")
        .unwrap_or(without_ext)
        .replace('/', "::");
    format!("python::{}", module_path)
}

fn module_name_to_symbol_id(module_name: &str) -> String {
    format!("python::{}", module_name.replace('.', "::"))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_python_imports_functions_classes_and_calls() {
        let source = r#"
import tools.build as build
from tools.helpers import helper as run_helper

def orchestrate():
    build.run()
    run_helper()

class Worker:
    def refresh(self):
        self.sync()

    def sync(self):
        pass
"#;

        let result = parse_python_file("scripts/main.py", source).unwrap();
        assert!(result
            .symbols
            .iter()
            .any(|symbol| symbol.id == "python::scripts::main"));
        assert!(result
            .symbols
            .iter()
            .any(|symbol| symbol.id == "python::scripts::main::orchestrate"));
        assert!(result
            .symbols
            .iter()
            .any(|symbol| symbol.id == "python::scripts::main::Worker"));
        assert!(result
            .symbols
            .iter()
            .any(|symbol| symbol.id == "python::scripts::main::Worker::refresh"));
        assert!(result.normalized_references.iter().any(|reference| {
            reference.category == ReferenceCategory::ModuleImport
                && reference.target_symbol_id == "python::tools::build"
        }));
        assert!(result.raw_calls.iter().any(|call| {
            call.caller_id == "python::scripts::main::orchestrate"
                && call.called_name == "run"
                && call.qualifier.as_deref() == Some("python::tools::build")
        }));
        assert!(result.raw_calls.iter().any(|call| {
            call.caller_id == "python::scripts::main::orchestrate"
                && call.called_name == "helper"
                && call.qualifier.as_deref() == Some("python::tools::helpers")
        }));
        assert!(result.raw_calls.iter().any(|call| {
            call.caller_id == "python::scripts::main::Worker::refresh"
                && call.called_name == "sync"
                && call.qualifier.as_deref() == Some("python::scripts::main::Worker")
        }));
    }

    #[test]
    fn parses_python_base_classes_and_class_calls() {
        let source = r#"
from engine.base import BaseRunner

class Runner(BaseRunner):
    def create(self):
        return Runner()
"#;

        let result = parse_python_file("game/runner.py", source).unwrap();
        assert!(result.normalized_references.iter().any(|reference| {
            reference.category == ReferenceCategory::InheritanceMention
                && reference.source_symbol_id == "python::game::runner::Runner"
                && reference.target_symbol_id == "python::engine::base::BaseRunner"
        }));
        assert!(result.raw_calls.iter().any(|call| {
            call.caller_id == "python::game::runner::Runner::create"
                && call.called_name == "Runner"
                && matches!(call.call_kind, RawCallKind::Unqualified)
        }));
    }

    #[test]
    fn emits_external_python_base_classes_as_candidates_for_later_filtering() {
        let source = r#"
import unittest

class Runner(unittest.TestCase):
    pass

class Plain(object):
    pass
"#;

        let result = parse_python_file("game/test_runner.py", source).unwrap();
        assert!(result.normalized_references.iter().any(|reference| {
            reference.category == ReferenceCategory::InheritanceMention
                && reference.target_symbol_id == "python::unittest::TestCase"
        }));
    }
}

/// Tree-sitter based Python parser.
pub fn parse_python_file_treesitter(file_path: &str, source: &str) -> Result<ParseResult, String> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_python::LANGUAGE.into())
        .map_err(|e| format!("Failed to set Python language: {}", e))?;

    let tree = parser
        .parse(source, None)
        .ok_or_else(|| "Python parse returned None".to_string())?;

    let source_bytes = source.as_bytes();
    let module_id = module_symbol_id(file_path);
    let module_name = module_leaf_name(&module_id);
    let total_lines = source.lines().count().max(1);

    let mut symbols = vec![py_base_symbol(
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

    py_visit_node(
        tree.root_node(),
        source_bytes,
        file_path,
        &module_id,
        &mut scope_stack,
        &mut symbols,
        &mut normalized_references,
    );

    let (graph_events, _, _) = execute_graph_rules(
        tree_sitter_python::LANGUAGE.into(),
        "python",
        PYTHON_CALL_RELATIONS,
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

fn py_visit_node<'a>(
    node: Node<'a>,
    source: &[u8],
    file_path: &str,
    module_id: &str,
    scope_stack: &mut Vec<(String, &'static str)>,
    symbols: &mut Vec<Symbol>,
    references: &mut Vec<NormalizedReference>,
) {
    match node.kind() {
        "class_definition" => {
            if let Some(sym) = py_extract_class(node, source, file_path, module_id, scope_stack, symbols, references) {
                let class_id = sym.id.clone();
                scope_stack.push((class_id, "class"));
                py_visit_children(node, source, file_path, module_id, scope_stack, symbols, references);
                scope_stack.pop();
                return;
            }
        }
        "function_definition" => {
            if let Some(sym) = py_extract_function(node, source, file_path, module_id, scope_stack, symbols) {
                let fn_id = sym.id.clone();
                let fn_kind = if scope_stack.last().map(|(_, k)| *k) == Some("class") { "method" } else { "function" };
                scope_stack.push((fn_id, fn_kind));
                py_visit_children(node, source, file_path, module_id, scope_stack, symbols, references);
                scope_stack.pop();
                return;
            }
        }
        "import_statement" | "import_from_statement" => {
            py_extract_import(node, source, file_path, module_id, scope_stack, references);
            return;
        }
        _ => {}
    }
    py_visit_children(node, source, file_path, module_id, scope_stack, symbols, references);
}

fn py_visit_children<'a>(
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
        py_visit_node(child, source, file_path, module_id, scope_stack, symbols, references);
    }
}

fn py_node_text<'a>(node: Node, source: &'a [u8]) -> &'a str {
    node.utf8_text(source).unwrap_or("")
}

fn py_current_scope(module_id: &str, scope_stack: &[(String, &'static str)]) -> String {
    scope_stack.last().map(|(id, _)| id.clone()).unwrap_or_else(|| module_id.to_string())
}

fn py_qualify(name: &str, module_id: &str, scope_stack: &[(String, &'static str)]) -> String {
    format!("{}::{}", py_current_scope(module_id, scope_stack), name)
}

fn py_extract_class<'a>(
    node: Node<'a>,
    source: &[u8],
    file_path: &str,
    module_id: &str,
    scope_stack: &[(String, &'static str)],
    symbols: &mut Vec<Symbol>,
    references: &mut Vec<NormalizedReference>,
) -> Option<Symbol> {
    let name_node = node.child_by_field_name("name")?;
    let name = py_node_text(name_node, source);
    let id = py_qualify(name, module_id, scope_stack);
    let parent_id = py_current_scope(module_id, scope_stack);
    let line = node.start_position().row + 1;
    let end_line = node.end_position().row + 1;

    // Extract base classes
    if let Some(superclasses) = node.child_by_field_name("superclasses") {
        let mut cursor = superclasses.walk();
        for base in superclasses.children(&mut cursor) {
            let base_text = py_node_text(base, source).trim();
            if base_text.is_empty() || base_text == "(" || base_text == ")" || base_text == "," { continue; }
            if base_text == "object" { continue; }
            let target_id = if base_text.contains('.') {
                format!("python::{}", base_text.replace('.', "::"))
            } else {
                format!("{}::{}", module_id, base_text)
            };
            references.push(NormalizedReference {
                source_symbol_id: id.clone(),
                target_symbol_id: target_id,
                category: ReferenceCategory::InheritanceMention,
                file_path: file_path.to_string(),
                line,
                confidence: RawExtractionConfidence::Partial,
            });
        }
    }

    let sym = py_base_symbol(
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

fn py_extract_function<'a>(
    node: Node<'a>,
    source: &[u8],
    file_path: &str,
    module_id: &str,
    scope_stack: &[(String, &'static str)],
    symbols: &mut Vec<Symbol>,
) -> Option<Symbol> {
    let name_node = node.child_by_field_name("name")?;
    let name = py_node_text(name_node, source);
    let id = py_qualify(name, module_id, scope_stack);
    let parent_id = py_current_scope(module_id, scope_stack);
    let line = node.start_position().row + 1;
    let end_line = node.end_position().row + 1;
    let symbol_type = if scope_stack.last().map(|(_, k)| *k) == Some("class") { "method" } else { "function" };

    let param_count = node.child_by_field_name("parameters")
        .map(|p| p.named_child_count())
        .unwrap_or(0);

    let mut sym = py_base_symbol(
        &id,
        name,
        symbol_type,
        file_path,
        line,
        end_line,
        Some(parent_id.clone()),
        Some(symbol_type),
        Some(parent_id),
        Some(module_id.to_string()),
    );
    sym.parameter_count = Some(param_count);
    symbols.push(sym.clone());
    Some(sym)
}

fn py_extract_import(
    node: Node,
    source: &[u8],
    file_path: &str,
    module_id: &str,
    scope_stack: &[(String, &'static str)],
    references: &mut Vec<NormalizedReference>,
) {
    let caller_id = scope_stack.last().map(|(id, _)| id.as_str()).unwrap_or(module_id).to_string();
    let line = node.start_position().row + 1;

    match node.kind() {
        "import_statement" => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "dotted_name" {
                    let mod_name = py_node_text(child, source).replace('.', "::");
                    references.push(NormalizedReference {
                        source_symbol_id: caller_id.clone(),
                        target_symbol_id: format!("python::{}", mod_name),
                        category: ReferenceCategory::ModuleImport,
                        file_path: file_path.to_string(),
                        line,
                        confidence: RawExtractionConfidence::High,
                    });
                } else if child.kind() == "aliased_import" {
                    if let Some(name_node) = child.child_by_field_name("name") {
                        let mod_name = py_node_text(name_node, source).replace('.', "::");
                        references.push(NormalizedReference {
                            source_symbol_id: caller_id.clone(),
                            target_symbol_id: format!("python::{}", mod_name),
                            category: ReferenceCategory::ModuleImport,
                            file_path: file_path.to_string(),
                            line,
                            confidence: RawExtractionConfidence::High,
                        });
                    }
                }
            }
        }
        "import_from_statement" => {
            let mut cursor = node.walk();
            let mut module_name = String::new();
            for child in node.children(&mut cursor) {
                if child.kind() == "dotted_name" && module_name.is_empty() {
                    module_name = py_node_text(child, source).replace('.', "::");
                }
            }
            if !module_name.is_empty() {
                references.push(NormalizedReference {
                    source_symbol_id: caller_id.clone(),
                    target_symbol_id: format!("python::{}", module_name),
                    category: ReferenceCategory::ModuleImport,
                    file_path: file_path.to_string(),
                    line,
                    confidence: RawExtractionConfidence::High,
                });
            }
        }
        _ => {}
    }
}

fn py_base_symbol(
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
pub fn parse_python_file_dual(file_path: &str, source: &str) -> Result<ParseResult, String> {
    let legacy = parse_python_file(file_path, source)?;
    let ts_result = parse_python_file_treesitter(file_path, source)?;

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
    fn treesitter_extracts_basic_function_and_class() {
        let source = r#"
def greet(name):
    return "Hello " + name

class Greeter:
    def say_hi(self):
        greet("world")
"#;
        let result = parse_python_file_treesitter("scripts/greet.py", source).unwrap();
        assert!(result.symbols.iter().any(|s| s.id == "python::scripts::greet" && s.symbol_type == "namespace"));
        assert!(result.symbols.iter().any(|s| s.id == "python::scripts::greet::greet" && s.symbol_type == "function"),
            "greet function should exist; symbols: {:?}", result.symbols.iter().map(|s| &s.id).collect::<Vec<_>>());
        assert!(result.symbols.iter().any(|s| s.id == "python::scripts::greet::Greeter" && s.symbol_type == "class"));
        assert!(result.symbols.iter().any(|s| s.id == "python::scripts::greet::Greeter::say_hi" && s.symbol_type == "method"));
    }

    #[test]
    fn treesitter_extracts_import_references() {
        let source = r#"
import os.path
from tools.helpers import helper

def run():
    helper()
"#;
        let result = parse_python_file_treesitter("scripts/main.py", source).unwrap();
        assert!(result.normalized_references.iter().any(|r| {
            r.category == ReferenceCategory::ModuleImport
                && r.target_symbol_id == "python::os::path"
        }), "import os.path should produce ModuleImport");
        assert!(result.normalized_references.iter().any(|r| {
            r.category == ReferenceCategory::ModuleImport
                && r.target_symbol_id == "python::tools::helpers"
        }), "from tools.helpers import should produce ModuleImport");
    }

    #[test]
    fn treesitter_extracts_calls() {
        let source = r#"
def main():
    process()
    obj.update()
"#;
        let result = parse_python_file_treesitter("scripts/main.py", source).unwrap();
        assert!(result.raw_calls.iter().any(|c| c.called_name == "process"),
            "unqualified call 'process' should be extracted");
        assert!(result.raw_calls.iter().any(|c| c.called_name == "update"),
            "member call 'update' should be extracted");
    }

    #[test]
    fn treesitter_extracts_inheritance() {
        let source = r#"
from engine.base import BaseRunner

class Runner(BaseRunner):
    def run(self):
        pass
"#;
        let result = parse_python_file_treesitter("game/runner.py", source).unwrap();
        assert!(result.normalized_references.iter().any(|r| {
            r.category == ReferenceCategory::InheritanceMention
                && r.source_symbol_id == "python::game::runner::Runner"
        }), "Runner(BaseRunner) should produce InheritanceMention");
    }

    #[test]
    fn treesitter_dual_result_matches_or_exceeds_legacy() {
        let source = r#"
from tools.build import run_build

def orchestrate():
    run_build()

class Worker:
    def refresh(self):
        self.sync()

    def sync(self):
        pass
"#;
        let legacy = parse_python_file("scripts/main.py", source).unwrap();
        let ts = parse_python_file_treesitter("scripts/main.py", source).unwrap();
        assert!(
            ts.symbols.len() >= (legacy.symbols.len() as f64 * 0.9) as usize,
            "tree-sitter symbols {} should be >= 90% of legacy {}",
            ts.symbols.len(), legacy.symbols.len()
        );
    }
}
