use regex::Regex;
use std::collections::HashMap;

use crate::models::{
    FileRiskSignals, IncludeHeaviness, MacroSensitivity, NormalizedReference, ParseFragility,
    ParseMetrics, ParseResult, RawCallKind, RawCallSite, RawExtractionConfidence, RawQualifierKind,
    RawReceiverKind, ReferenceCategory, Symbol,
};

pub fn parse_rust_file(file_path: &str, source: &str) -> Result<ParseResult, String> {
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

    let mod_re = Regex::new(r#"^\s*(?:pub\s+)?mod\s+([A-Za-z_][A-Za-z0-9_]*)\s*(?:;|\{)"#)
        .map_err(|e| format!("Rust regex error: {}", e))?;
    let use_re = Regex::new(r#"^\s*use\s+([^;]+);"#).map_err(|e| format!("Rust regex error: {}", e))?;
    let struct_re = Regex::new(r#"^\s*(?:pub\s+)?struct\s+([A-Za-z_][A-Za-z0-9_]*)"#)
        .map_err(|e| format!("Rust regex error: {}", e))?;
    let enum_re = Regex::new(r#"^\s*(?:pub\s+)?enum\s+([A-Za-z_][A-Za-z0-9_]*)"#)
        .map_err(|e| format!("Rust regex error: {}", e))?;
    let trait_re = Regex::new(r#"^\s*(?:pub\s+)?trait\s+([A-Za-z_][A-Za-z0-9_]*)"#)
        .map_err(|e| format!("Rust regex error: {}", e))?;
    let impl_trait_re = Regex::new(
        r#"^\s*impl(?:<[^>]+>)?\s+([A-Za-z_][A-Za-z0-9_:<>]*)\s+for\s+([A-Za-z_][A-Za-z0-9_:<>]*)\s*\{"#,
    )
    .map_err(|e| format!("Rust regex error: {}", e))?;
    let impl_type_re = Regex::new(r#"^\s*impl(?:<[^>]+>)?\s+([A-Za-z_][A-Za-z0-9_:<>]*)\s*\{"#)
        .map_err(|e| format!("Rust regex error: {}", e))?;
    let fn_re = Regex::new(r#"^\s*(?:pub\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(([^)]*)\)"#)
        .map_err(|e| format!("Rust regex error: {}", e))?;
    let qualified_call_re =
        Regex::new(r#"([A-Za-z_][A-Za-z0-9_:]*)::([A-Za-z_][A-Za-z0-9_]*)\s*\(([^()]*)\)"#)
            .map_err(|e| format!("Rust regex error: {}", e))?;
    let member_call_re =
        Regex::new(r#"([A-Za-z_][A-Za-z0-9_]*)\.([A-Za-z_][A-Za-z0-9_]*)\s*\(([^()]*)\)"#)
            .map_err(|e| format!("Rust regex error: {}", e))?;
    let unqualified_call_re =
        Regex::new(r#"([A-Za-z_][A-Za-z0-9_]*)\s*\(([^()]*)\)"#).map_err(|e| format!("Rust regex error: {}", e))?;

    #[derive(Clone)]
    struct ModuleContext {
        brace_depth: i32,
        module_id: String,
    }

    #[derive(Clone)]
    struct ImplContext {
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

    let mut module_stack: Vec<ModuleContext> = Vec::new();
    let mut impl_stack: Vec<ImplContext> = Vec::new();
    let mut function_stack: Vec<FunctionContext> = Vec::new();
    let mut brace_depth = 0i32;

    for (index, raw_line) in source.lines().enumerate() {
        let line_no = index + 1;
        let line = strip_rust_comment(raw_line);
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
        while impl_stack
            .last()
            .map(|ctx| brace_depth < ctx.brace_depth)
            .unwrap_or(false)
        {
            impl_stack.pop();
        }
        while module_stack
            .last()
            .map(|ctx| brace_depth < ctx.brace_depth)
            .unwrap_or(false)
        {
            module_stack.pop();
        }

        let current_module_id = module_stack
            .last()
            .map(|ctx| ctx.module_id.clone())
            .unwrap_or_else(|| module_id.clone());
        let owner_symbol_id = function_stack
            .last()
            .map(|ctx| ctx.symbol_id.clone())
            .or_else(|| impl_stack.last().map(|ctx| ctx.type_id.clone()))
            .or_else(|| module_stack.last().map(|ctx| ctx.module_id.clone()))
            .unwrap_or_else(|| module_id.clone());

        if let Some(captures) = mod_re.captures(trimmed) {
            let mod_name = captures.get(1).unwrap().as_str();
            let child_module_id = format!("{}::{}", current_module_id, mod_name);
            symbols.push(base_symbol(
                &child_module_id,
                mod_name,
                "namespace",
                file_path,
                line_no,
                line_no,
                Some(current_module_id.clone()),
                Some("namespace"),
                None,
                Some(module_id.clone()),
            ));
            if trimmed.ends_with('{') {
                module_stack.push(ModuleContext {
                    brace_depth: brace_depth + open_brace_delta(trimmed),
                    module_id: child_module_id,
                });
            }
        }

        if let Some(captures) = use_re.captures(trimmed) {
            let use_expr = captures.get(1).unwrap().as_str();
            for use_target in expand_use_targets(use_expr) {
                let target_symbol_id = rust_path_to_symbol_id(&normalize_rust_use_path(&use_target));
                normalized_references.push(NormalizedReference {
                    source_symbol_id: owner_symbol_id.clone(),
                    target_symbol_id: target_symbol_id.clone(),
                    category: ReferenceCategory::ModuleImport,
                    file_path: file_path.into(),
                    line: line_no,
                    confidence: RawExtractionConfidence::High,
                });

                let alias_name = import_alias_name(&use_target);
                if let Some(last_segment) = alias_name {
                    if target_symbol_id.rsplit("::").count() > 1 {
                        imported_symbol_aliases.insert(last_segment.to_string(), target_symbol_id.clone());
                    }
                    module_aliases.insert(last_segment.to_string(), target_symbol_id);
                }
            }
        }

        if let Some(captures) = struct_re.captures(trimmed) {
            let type_name = captures.get(1).unwrap().as_str();
            let type_id = format!("{}::{}", current_module_id, type_name);
            known_types.insert(type_name.to_string(), type_id.clone());
            symbols.push(base_symbol(
                &type_id,
                type_name,
                "struct",
                file_path,
                line_no,
                line_no,
                Some(current_module_id.clone()),
                Some("namespace"),
                None,
                Some(module_id.clone()),
            ));
        }

        if let Some(captures) = enum_re.captures(trimmed) {
            let type_name = captures.get(1).unwrap().as_str();
            let type_id = format!("{}::{}", current_module_id, type_name);
            known_types.insert(type_name.to_string(), type_id.clone());
            symbols.push(base_symbol(
                &type_id,
                type_name,
                "enum",
                file_path,
                line_no,
                line_no,
                Some(current_module_id.clone()),
                Some("namespace"),
                None,
                Some(module_id.clone()),
            ));
        }

        if let Some(captures) = trait_re.captures(trimmed) {
            let trait_name = captures.get(1).unwrap().as_str();
            let trait_id = format!("{}::{}", current_module_id, trait_name);
            known_types.insert(trait_name.to_string(), trait_id.clone());
            symbols.push(base_symbol(
                &trait_id,
                trait_name,
                "trait",
                file_path,
                line_no,
                line_no,
                Some(current_module_id.clone()),
                Some("namespace"),
                None,
                Some(module_id.clone()),
            ));
            if trimmed.ends_with('{') {
                impl_stack.push(ImplContext {
                    brace_depth: brace_depth + open_brace_delta(trimmed),
                    type_id: trait_id,
                    type_name: trait_name.to_string(),
                });
            }
        }

        if let Some(captures) = impl_trait_re.captures(trimmed) {
            let trait_name = normalize_rust_type_token(captures.get(1).unwrap().as_str());
            let target_type_name = normalize_rust_type_token(captures.get(2).unwrap().as_str());
            let trait_id = resolve_rust_name(
                &trait_name,
                &current_module_id,
                &known_types,
                &module_aliases,
                &imported_symbol_aliases,
            );
            let type_id = resolve_rust_name(
                &target_type_name,
                &current_module_id,
                &known_types,
                &module_aliases,
                &imported_symbol_aliases,
            )
            .unwrap_or_else(|| format!("{}::{}", current_module_id, target_type_name));

            if let Some(resolved_trait_id) = trait_id {
                normalized_references.push(NormalizedReference {
                    source_symbol_id: type_id.clone(),
                    target_symbol_id: resolved_trait_id,
                    category: ReferenceCategory::InheritanceMention,
                    file_path: file_path.into(),
                    line: line_no,
                    confidence: RawExtractionConfidence::High,
                });
            }

            impl_stack.push(ImplContext {
                brace_depth: brace_depth + open_brace_delta(trimmed),
                type_id,
                type_name: target_type_name,
            });
        } else if let Some(captures) = impl_type_re.captures(trimmed) {
            let target_type_name = normalize_rust_type_token(captures.get(1).unwrap().as_str());
            let type_id = resolve_rust_name(
                &target_type_name,
                &current_module_id,
                &known_types,
                &module_aliases,
                &imported_symbol_aliases,
            )
            .unwrap_or_else(|| format!("{}::{}", current_module_id, target_type_name));
            impl_stack.push(ImplContext {
                brace_depth: brace_depth + open_brace_delta(trimmed),
                type_id,
                type_name: target_type_name,
            });
        }

        if let Some(captures) = fn_re.captures(trimmed) {
            let fn_name = captures.get(1).unwrap().as_str();
            let parameters = captures.get(2).unwrap().as_str();
            let current_impl = impl_stack.last().cloned();
            let (symbol_id, symbol_type, scope_qualified_name, parent_id, scope_kind) =
                if let Some(impl_ctx) = &current_impl {
                    (
                        format!("{}::{}", impl_ctx.type_id, fn_name),
                        "method",
                        Some(impl_ctx.type_id.clone()),
                        Some(impl_ctx.type_id.clone()),
                        Some("type"),
                    )
                } else {
                    (
                        format!("{}::{}", current_module_id, fn_name),
                        "function",
                        Some(current_module_id.clone()),
                        None,
                        Some("namespace"),
                    )
                };

            symbols.push(Symbol {
                id: symbol_id.clone(),
                name: fn_name.into(),
                qualified_name: symbol_id.clone(),
                symbol_type: symbol_type.into(),
                file_path: file_path.into(),
                line: line_no,
                end_line: line_no,
                signature: Some(format!("{}({})", fn_name, parameters.trim())),
                parameter_count: Some(parameter_count(parameters)),
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
                module: Some(module_id.clone()),
                subsystem: None,
                project_area: None,
                artifact_kind: None,
                header_role: None,
                parse_fragility: Some("low".into()),
                macro_sensitivity: Some("low".into()),
                include_heaviness: Some("light".into()),
            });

            if trimmed.ends_with('{') {
                function_stack.push(FunctionContext {
                    brace_depth: brace_depth + open_brace_delta(trimmed),
                    symbol_id,
                    type_id: current_impl.as_ref().map(|ctx| ctx.type_id.clone()),
                    type_name: current_impl.as_ref().map(|ctx| ctx.type_name.clone()),
                });
            }
        }

        if let Some(current) = function_stack.last() {
            for captures in qualified_call_re.captures_iter(trimmed) {
                let qualifier_name = captures.get(1).unwrap().as_str();
                let called_name = captures.get(2).unwrap().as_str();
                let args = captures.get(3).unwrap().as_str();
                if is_rust_control_keyword(qualifier_name) {
                    continue;
                }

                let normalized_qualifier = normalize_rust_type_token(qualifier_name);
                let qualifier = resolve_rust_name(
                    &normalized_qualifier,
                    &current_module_id,
                    &known_types,
                    &module_aliases,
                    &imported_symbol_aliases,
                )
                .unwrap_or_else(|| rust_path_to_symbol_id(&normalized_qualifier));
                let qualifier_kind = if known_types.contains_key(&normalized_qualifier)
                    || current.type_name.as_deref() == Some(normalized_qualifier.as_str())
                {
                    Some(RawQualifierKind::Type)
                } else {
                    Some(RawQualifierKind::Namespace)
                };

                raw_calls.push(RawCallSite {
                    caller_id: current.symbol_id.clone(),
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

            for captures in member_call_re.captures_iter(trimmed) {
                let receiver_name = captures.get(1).unwrap().as_str();
                let called_name = captures.get(2).unwrap().as_str();
                let args = captures.get(3).unwrap().as_str();
                if is_rust_control_keyword(receiver_name) {
                    continue;
                }

                let (qualifier, qualifier_kind, receiver_kind) = if receiver_name == "self" {
                    (current.type_id.clone(), Some(RawQualifierKind::Type), Some(RawReceiverKind::This))
                } else {
                    (None, None, Some(RawReceiverKind::Identifier))
                };

                raw_calls.push(RawCallSite {
                    caller_id: current.symbol_id.clone(),
                    called_name: called_name.into(),
                    call_kind: RawCallKind::MemberAccess,
                    argument_count: Some(argument_count(args)),
                    argument_texts: split_arguments(args),
                    result_target: None,
                    receiver: Some(receiver_name.to_string()),
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
                if is_rust_control_keyword(called_name)
                    || trimmed.starts_with("fn ")
                    || trimmed.starts_with("pub fn ")
                {
                    continue;
                }
                if trimmed.contains(&format!("::{}(", called_name)) || trimmed.contains(&format!(".{}(", called_name)) {
                    continue;
                }

                if let Some(imported_symbol_id) = imported_symbol_aliases
                    .get(called_name)
                    .or_else(|| module_aliases.get(called_name))
                {
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

fn expand_use_targets(value: &str) -> Vec<String> {
    let trimmed = value.trim();
    if let Some((prefix, rest)) = trimmed.split_once("::{") {
        let prefix = prefix.trim();
        let items = rest.trim_end_matches('}');
        return items
            .split(',')
            .map(|item| item.trim())
            .filter(|item| !item.is_empty() && *item != "self")
            .map(|item| format!("{}::{}", prefix, item))
            .collect();
    }
    vec![trimmed.to_string()]
}

fn import_alias_name(target: &str) -> Option<&str> {
    if let Some((_, alias)) = target.split_once(" as ") {
        Some(alias.trim())
    } else {
        target.rsplit("::").next()
    }
}

fn normalize_rust_use_path(value: &str) -> String {
    value
        .split_once(" as ")
        .map(|(base, _)| base.trim().to_string())
        .unwrap_or_else(|| value.trim().to_string())
}

fn resolve_rust_name(
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
    if name.contains("::") {
        return Some(rust_path_to_symbol_id(name));
    }
    Some(format!("{}::{}", module_id, name))
}

fn strip_rust_comment(line: &str) -> &str {
    if let Some(index) = line.find("//") {
        &line[..index]
    } else {
        line
    }
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

fn is_rust_control_keyword(value: &str) -> bool {
    matches!(
        value,
        "if"
            | "for"
            | "while"
            | "match"
            | "loop"
            | "return"
            | "fn"
            | "struct"
            | "enum"
            | "trait"
            | "impl"
            | "Some"
            | "Ok"
    )
}

fn normalize_rust_type_token(value: &str) -> String {
    value
        .trim()
        .trim_start_matches('&')
        .split('<')
        .next()
        .unwrap_or(value)
        .trim()
        .to_string()
}

fn module_symbol_id(file_path: &str) -> String {
    let normalized = file_path.replace('\\', "/");
    let without_ext = normalized.strip_suffix(".rs").unwrap_or(&normalized);
    let module_path = without_ext
        .strip_suffix("/mod")
        .or_else(|| without_ext.strip_suffix("/lib"))
        .or_else(|| without_ext.strip_suffix("/main"))
        .unwrap_or(without_ext)
        .replace('/', "::");
    format!("rust::{}", module_path)
}

fn rust_path_to_symbol_id(path: &str) -> String {
    format!("rust::{}", normalize_rust_type_token(path))
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
    fn parses_rust_use_trait_impl_and_calls() {
        let source = r#"
use crate::core::BaseWorker;
use crate::util::helper as run_helper;

pub trait Runnable {
    fn run(&self);
}

pub struct Worker;

impl Runnable for Worker {
    fn run(&self) {
        self.sync();
        run_helper();
        crate::util::build();
    }

    fn sync(&self) {}
}
"#;

        let result = parse_rust_file("src/worker.rs", source).unwrap();
        assert!(result.symbols.iter().any(|symbol| symbol.id == "rust::src::worker"));
        assert!(result.symbols.iter().any(|symbol| symbol.id == "rust::src::worker::Runnable"));
        assert!(result.symbols.iter().any(|symbol| symbol.id == "rust::src::worker::Worker"));
        assert!(result.symbols.iter().any(|symbol| symbol.id == "rust::src::worker::Worker::run"));
        assert!(result.normalized_references.iter().any(|reference| {
            reference.category == ReferenceCategory::ModuleImport
                && reference.target_symbol_id == "rust::crate::core::BaseWorker"
        }));
        assert!(result.normalized_references.iter().any(|reference| {
            reference.category == ReferenceCategory::InheritanceMention
                && reference.source_symbol_id == "rust::src::worker::Worker"
                && reference.target_symbol_id == "rust::src::worker::Runnable"
        }));
        assert!(result.raw_calls.iter().any(|call| {
            call.caller_id == "rust::src::worker::Worker::run"
                && call.called_name == "sync"
                && call.qualifier.as_deref() == Some("rust::src::worker::Worker")
        }));
        assert!(result.raw_calls.iter().any(|call| {
            call.caller_id == "rust::src::worker::Worker::run"
                && call.called_name == "helper"
                && call.qualifier.as_deref() == Some("rust::crate::util")
        }));
        assert!(result.raw_calls.iter().any(|call| {
            call.caller_id == "rust::src::worker::Worker::run"
                && call.called_name == "build"
                && call.qualifier.as_deref() == Some("rust::crate::util")
        }));
    }

    #[test]
    fn parses_rust_modules_and_impl_attached_methods() {
        let source = r#"
pub mod nested {
    pub struct Engine;

    impl Engine {
        pub fn new() -> Self {
            Engine
        }
    }
}
"#;

        let result = parse_rust_file("src/lib.rs", source).unwrap();
        assert!(result.symbols.iter().any(|symbol| symbol.id == "rust::src"));
        assert!(result.symbols.iter().any(|symbol| symbol.id == "rust::src::nested"));
        assert!(result.symbols.iter().any(|symbol| symbol.id == "rust::src::nested::Engine"));
        assert!(result
            .symbols
            .iter()
            .any(|symbol| symbol.id == "rust::src::nested::Engine::new"));
    }
}
