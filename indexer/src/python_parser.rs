use regex::Regex;
use std::collections::HashMap;

use crate::models::{
    FileRiskSignals, IncludeHeaviness, MacroSensitivity, NormalizedReference, ParseFragility,
    ParseMetrics, ParseResult, RawCallKind, RawCallSite, RawExtractionConfidence, RawQualifierKind,
    RawReceiverKind, ReferenceCategory, Symbol,
};

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
