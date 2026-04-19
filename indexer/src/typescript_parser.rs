use regex::Regex;
use std::collections::HashMap;

use crate::models::{
    FileRiskSignals, IncludeHeaviness, MacroSensitivity, NormalizedReference, ParseFragility,
    ParseMetrics, ParseResult, RawCallKind, RawCallSite, RawExtractionConfidence, RawQualifierKind,
    RawReceiverKind, ReferenceCategory, Symbol,
};

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
}
