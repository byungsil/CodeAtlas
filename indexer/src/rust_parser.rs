use regex::Regex;
use std::collections::HashMap;
use tree_sitter::{Node, Parser};

use crate::graph_rules::RUST_CALL_RELATIONS;
use crate::models::{
    FileRiskSignals, IncludeHeaviness, MacroSensitivity, NormalizedReference, ParseFragility,
    ParseMetrics, ParseResult, RawCallKind, RawCallSite, RawExtractionConfidence, RawQualifierKind,
    RawReceiverKind, ReferenceCategory, Symbol, TypeEvidence, TypeInferenceConfidence,
};
use crate::parser::execute_graph_rules;

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

    // MS20 type inference + pattern detection — gated by CODEATLAS_ENABLE_MS20 (off by default).
    let (type_inferences, analysis_results) = if crate::parser::ms20_semantic_enrichment_enabled() {
        (infer_rust_types_from_regex(source, file_path), detect_rust_patterns(source, file_path))
    } else {
        (Vec::new(), Vec::new())
    };

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
        include_dependencies: Vec::new(),
        macro_definitions: Vec::new(),
        conditional_blocks: Vec::new(),
        dependency_metrics: crate::models::DependencyMetrics::default(),
        conditional_symbols: Vec::new(),
        type_inferences,
        analysis_results,
    })
}

/// Infer types from Rust function signatures and variable bindings.
pub(crate) fn infer_rust_types_from_regex(
    source: &str,
    file_path: &str,
) -> Vec<crate::models::TypeInferenceResult> {
    let mut by_symbol = HashMap::<String, Vec<TypeEvidence>>::new();

    // 1. Extract return types from function signatures.
    for (line_no, line) in source.lines().enumerate() {
        let trimmed = line.trim();
        if !trimmed.starts_with("fn ") && !trimmed.starts_with("pub fn ") { continue; }
        if let Some(paren_end) = trimmed.find(')') {
            let after_paren = &trimmed[paren_end + 1..];
            if let Some(arrow_pos) = after_paren.find("->") {
                let ret_part: String = after_paren[arrow_pos + 2..]
                    .chars().take_while(|c| !"{; ".contains(*c)).collect();
                if !ret_part.is_empty() && !ret_part.starts_with('(') {
                    by_symbol.entry(format!("{}::function@L{}", file_path, line_no + 1))
                        .or_default()
                        .push(TypeEvidence { expression_text: ret_part.clone(), inferred_type_hint: Some(ret_part), confidence: TypeInferenceConfidence::High });
                }
            }
        }
    }

    // 2. Infer types from let bindings with explicit annotations.
    for (line_no, line) in source.lines().enumerate() {
        let trimmed = line.trim();
        if !trimmed.starts_with("let ") && !trimmed.starts_with("pub let ") { continue; }
        if let Some(colon_pos) = trimmed.find(':') {
            let after_colon = &trimmed[colon_pos + 1..];
            let type_hint: String = after_colon.chars().take_while(|c| !"; ".contains(*c)).collect();
            if !type_hint.is_empty() && !type_hint.starts_with('=') {
                by_symbol.entry(format!("{}::variable@L{}", file_path, line_no + 1))
                    .or_default()
                    .push(TypeEvidence { expression_text: type_hint.clone(), inferred_type_hint: Some(type_hint), confidence: TypeInferenceConfidence::High });
            }
        }
    }

    // 3. Infer types from return/Ok/Err expressions.
    for (line_no, line) in source.lines().enumerate() {
        let trimmed = line.trim();
        if !(trimmed.starts_with("return ") || trimmed == "true" || trimmed == "false") { continue; }
        let expr_text: String = if trimmed.starts_with("return ") {
            match trimmed[7..].find('(') {
                Some(p) => trimmed[p + 8..].trim().to_string(),
                None => trimmed[7..].replace(';', ""),
            }
        } else { trimmed.to_string() };
        if !expr_text.is_empty()
            && !(expr_text.starts_with('"') || expr_text.contains("return ")) {
            let inferred_type = infer_rust_expr_type(&expr_text);
            by_symbol.entry(format!("{}::expression@L{}", file_path, line_no + 1))
                .or_default()
                .push(TypeEvidence { expression_text: expr_text, inferred_type_hint: Some(inferred_type), confidence: TypeInferenceConfidence::Partial });
        }
    }

    by_symbol.into_iter().map(|(sym_id, evidences)| {
        let best = evidences.iter().filter_map(|e| e.inferred_type_hint.clone()).next();
        crate::models::TypeInferenceResult { symbol_id: sym_id, inferred_type: best,
            confidence: if evidences.len() > 1 { TypeInferenceConfidence::Partial } else { TypeInferenceConfidence::High }, evidence_sources: evidences }
    }).collect()
}

fn convert_rust_type_map_to_results(
    by_symbol: HashMap<String, Vec<TypeEvidence>>,
) -> Vec<crate::models::TypeInferenceResult> {
    by_symbol.into_iter().map(|(sym_id, evidences)| {
        let best = evidences.iter().filter_map(|e| e.inferred_type_hint.clone()).next();
        crate::models::TypeInferenceResult { symbol_id: sym_id, inferred_type: best,
            confidence: if evidences.len() > 1 { TypeInferenceConfidence::Partial } else { TypeInferenceConfidence::High }, evidence_sources: evidences }
    }).collect()
}

fn infer_rust_expr_type(expr: &str) -> String {
    let e = expr.trim();
    if e.starts_with("Some(") { return "Option".into(); }
    if e.starts_with("Ok(") || e.starts_with("Err(") { return "Result".into(); }
    if e == "true" || e == "false" { return "bool".into(); }
    if let Some(s) = e.strip_prefix('"') {
        if !s.is_empty() && s.contains(|c: char| c != '"' && c != '\\') { return "String".into(); }
    }
    if let Some(first) = e.chars().next() {
        if first.is_uppercase() {
            let t: String = e.split(|c: char| !c.is_alphanumeric() && c != '_' && c != '<' && c != '>' && c != ':').take(1).collect();
            return if t.len() > 1 { t } else { "unknown".into() };
        }
    }
    let ns: String = e.chars().take_while(|c| !c.is_alphabetic()).collect();
    if !ns.trim().is_empty() { return "i32".into(); }
    "unknown".into()
}

fn detect_rust_patterns(source: &str, file_path: &str) -> Vec<crate::models::AnalysisResult> {
    let mut results = Vec::new();
    let all_lines: Vec<&str> = source.lines().collect();
    for (line_no, line) in all_lines.iter().enumerate() {
        let trimmed = line.trim();
        if !trimmed.contains("vec![") || trimmed.starts_with('#') { continue; }
        // Check context: is this inside a function?
        let start = line_no.saturating_sub(20);
        let preceding: Vec<&str> = all_lines[start..=line_no].iter().rev().copied().collect();
        if preceding.iter().any(|l| l.trim().starts_with("fn ") || l.trim().starts_with("pub fn ")) {
            results.push(crate::models::AnalysisResult { rule_id: "rust-mutable-default-arg".into(), file_path: file_path.into(), line_start: line_no + 1, line_end: line_no + 1, match_text: Some(trimmed.to_string()), symbol_id: None });
        }
    }
    results
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

/// Infer types from tree-sitter AST nodes (return annotations, let bindings).
pub(crate) fn infer_rust_types_from_treesitter(
    root: tree_sitter::Node<'_>,
    symbols: &Vec<Symbol>,
    source_bytes: &[u8],
) -> Vec<crate::models::TypeInferenceResult> {
    let mut by_symbol = HashMap::<String, Vec<TypeEvidence>>::new();

    fn node_text(node: &tree_sitter::Node<'_>, src: &[u8]) -> String {
        node.utf8_text(src).unwrap_or("").trim().to_string()
    }
   let mut sym_by_qualified: HashMap<String, String> = HashMap::new();
    for s in symbols {
        if (s.symbol_type == "function" || s.symbol_type == "method") && !s.id.is_empty() {
            sym_by_qualified.insert(s.qualified_name.clone(), s.id.clone());
        }
    }

    fn walk(
        node: tree_sitter::Node<'_>,
        src: &[u8],
        sym_lookup: &HashMap<String, String>,
        results: &mut HashMap<String, Vec<TypeEvidence>>,
    ) {
        match node.kind() {
            "function_item" => {
                let fn_name = extract_fn_name(&node, src);

                if !fn_name.is_empty() {
                    let sym_id = sym_lookup.get(&fn_name)
                        .cloned()
                        .or_else(|| {
                            let target = format!("::{fn_name}");
                            sym_lookup.iter().find(|(qn, _)| qn.ends_with(&target)).map(|(_, id)| id.clone())
                        });

                    if let Some(sym_id) = sym_id {
                        collect_return_type_evidence(node, &sym_id, src, results);
                    } else {
                        fallback_inference(&fn_name, node, src, results);
                    }
                } else {
                    fallback_inference("", node, src, results);
                }
            }
            _ => {}
        }

        for child in node.children(&mut node.walk()) {
            walk(child, src, sym_lookup, results);
        }
    }

    fn extract_fn_name<'a>(node: &'a tree_sitter::Node<'_>, src: &[u8]) -> String {
        if let Some(name_node) = node.child_by_field_name("name") {
            return name_node.utf8_text(src).unwrap_or("").trim().to_string();
        }
        for ch in node.named_children(&mut node.walk()) {
            if ch.kind() == "identifier" || ch.kind() == "qualified_identifier" {
                let text = ch.utf8_text(src).unwrap_or("").trim().to_string();
                return text.strip_prefix("::").map(|s| s.to_string()).unwrap_or(text);
            }
        }
        String::new()
    }

   fn collect_return_type_evidence(
        func_node: tree_sitter::Node<'_>,
        owner_id: &str,
        src: &[u8],
        results: &mut HashMap<String, Vec<TypeEvidence>>,
    ) {
        if let Some(ret_type) = find_return_annotation(&func_node) {
            let ret_str = node_text(&ret_type, src);
            if !ret_str.is_empty() && !(ret_str.starts_with('(') || ret_str.contains("fn") || ret_str == "Self" || ret_str == "()") {
                results.entry(owner_id.to_string()).or_default().push(TypeEvidence { expression_text: ret_str.clone(), inferred_type_hint: Some(ret_str), confidence: TypeInferenceConfidence::High });
            }
        }
    }


    fn fallback_inference(fn_name: &str, node: tree_sitter::Node<'_>, src: &[u8], results: &mut HashMap<String, Vec<TypeEvidence>>,) {
        if let Some(ret_type) = find_return_annotation(&node) {
            let ret_str = node_text(&ret_type, src);
            if !ret_str.is_empty() && !(ret_str.starts_with('(') || ret_str.contains("fn") || ret_str == "Self" || ret_str == "()") {
                let line_no = count_newlines(src, node.byte_range().start) + 1;
                results.entry(format!("{}::function@L{}", fn_name.replace('/', "_"), line_no))
                    .or_default()
                    .push(TypeEvidence { expression_text: ret_str.clone(), inferred_type_hint: Some(ret_str), confidence: TypeInferenceConfidence::High });
            }
        }
    }

    fn find_return_annotation<'a>(node: &'a tree_sitter::Node<'_>) -> Option<tree_sitter::Node<'a>> {
        node.child_by_field_name("return_type")
    }

    walk(root.clone(), source_bytes, &sym_by_qualified, &mut by_symbol);
    convert_rust_type_map_to_results(by_symbol)
}

fn count_newlines(data: &[u8], pos: usize) -> usize {
    data[..pos].iter().filter(|&&b| b == b'\n').count()
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

/// Tree-sitter based Rust parser.
pub fn parse_rust_file_treesitter(file_path: &str, source: &str) -> Result<ParseResult, String> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .map_err(|e| format!("Failed to set Rust language: {}", e))?;

    let tree = parser
        .parse(source, None)
        .ok_or_else(|| "Rust parse returned None".to_string())?;

    // Clone root node early so it survives the rs_visit_node call which consumes the original.
    let root_for_type_inference = tree.root_node();

    let source_bytes = source.as_bytes();
    let module_id = module_symbol_id(file_path);
    let module_name = module_leaf_name(&module_id);
    let total_lines = source.lines().count().max(1);

    let mut symbols = vec![rs_base_symbol(
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
    // scope_stack: (qualified_id, symbol_type, impl_owner)
    // impl_owner is set when we're inside an impl block
    let mut scope_stack: Vec<(String, &'static str, Option<String>)> = Vec::new();

    rs_visit_node(
        tree.root_node(),
        source_bytes,
        file_path,
        &module_id,
        &mut scope_stack,
        &mut symbols,
        &mut normalized_references,
    );

    let (graph_events, _, _) = execute_graph_rules(
        tree_sitter_rust::LANGUAGE.into(),
        "rust",
        RUST_CALL_RELATIONS,
        &tree,
        source,
        file_path,
        &symbols,
    );

    let raw_calls: Vec<RawCallSite> = graph_events
        .into_iter()
        .filter_map(|event| event.to_raw_call_site())
        .collect();

    // MS20 type inference + pattern detection — gated by CODEATLAS_ENABLE_MS20 (off by default).
    let (type_inferences, analysis_results) = if crate::parser::ms20_semantic_enrichment_enabled() {
        (
            infer_rust_types_from_treesitter(root_for_type_inference.clone(), &symbols, source.as_bytes()),
            detect_rust_patterns(source, file_path),
        )
    } else {
        (Vec::new(), Vec::new())
    };

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
        include_dependencies: Vec::new(),
        macro_definitions: Vec::new(),
        conditional_blocks: Vec::new(),
        dependency_metrics: crate::models::DependencyMetrics::default(),
        conditional_symbols: Vec::new(),
        type_inferences,
        analysis_results,
    })
}

fn rs_visit_node<'a>(
    node: Node<'a>,
    source: &[u8],
    file_path: &str,
    module_id: &str,
    scope_stack: &mut Vec<(String, &'static str, Option<String>)>,
    symbols: &mut Vec<Symbol>,
    references: &mut Vec<NormalizedReference>,
) {
    match node.kind() {
        "mod_item" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                let name = rs_node_text(name_node, source);
                let id = rs_qualify(name, module_id, scope_stack);
                let line = node.start_position().row + 1;
                let end_line = node.end_position().row + 1;
                let parent_id = rs_current_scope(module_id, scope_stack);
                let sym = rs_base_symbol(&id, name, "namespace", file_path, line, end_line,
                    Some(parent_id.clone()), Some("namespace"), Some(parent_id), Some(module_id.to_string()));
                symbols.push(sym);
                scope_stack.push((id, "namespace", None));
                rs_visit_children(node, source, file_path, module_id, scope_stack, symbols, references);
                scope_stack.pop();
                return;
            }
        }
        "struct_item" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                let name = rs_node_text(name_node, source);
                let id = rs_qualify(name, module_id, scope_stack);
                let parent_id = rs_current_scope(module_id, scope_stack);
                let line = node.start_position().row + 1;
                let end_line = node.end_position().row + 1;
                let sym = rs_base_symbol(&id, name, "struct", file_path, line, end_line,
                    Some(parent_id.clone()), Some("struct"), Some(parent_id), Some(module_id.to_string()));
                symbols.push(sym);
            }
        }
        "enum_item" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                let name = rs_node_text(name_node, source);
                let id = rs_qualify(name, module_id, scope_stack);
                let parent_id = rs_current_scope(module_id, scope_stack);
                let line = node.start_position().row + 1;
                let end_line = node.end_position().row + 1;
                let sym = rs_base_symbol(&id, name, "enum", file_path, line, end_line,
                    Some(parent_id.clone()), Some("enum"), Some(parent_id), Some(module_id.to_string()));
                symbols.push(sym);
            }
        }
        "trait_item" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                let name = rs_node_text(name_node, source);
                let id = rs_qualify(name, module_id, scope_stack);
                let parent_id = rs_current_scope(module_id, scope_stack);
                let line = node.start_position().row + 1;
                let end_line = node.end_position().row + 1;
                let sym = rs_base_symbol(&id, name, "trait", file_path, line, end_line,
                    Some(parent_id.clone()), Some("trait"), Some(parent_id), Some(module_id.to_string()));
                symbols.push(sym);
                scope_stack.push((id, "trait", None));
                rs_visit_children(node, source, file_path, module_id, scope_stack, symbols, references);
                scope_stack.pop();
                return;
            }
        }
        "impl_item" => {
            // Determine the type being implemented
            let type_name = node.child_by_field_name("type")
                .map(|n| rs_node_text(n, source).to_string());
            let impl_owner = type_name.as_ref().map(|t| {
                rs_qualify(t, module_id, scope_stack)
            });
            scope_stack.push((rs_current_scope(module_id, scope_stack), "impl", impl_owner));
            rs_visit_children(node, source, file_path, module_id, scope_stack, symbols, references);
            scope_stack.pop();
            return;
        }
        "function_item" => {
            rs_extract_function(node, source, file_path, module_id, scope_stack, symbols);
        }
        "use_declaration" => {
            rs_extract_use(node, source, file_path, module_id, scope_stack, references);
            return;
        }
        _ => {}
    }
    rs_visit_children(node, source, file_path, module_id, scope_stack, symbols, references);
}

fn rs_visit_children<'a>(
    node: Node<'a>,
    source: &[u8],
    file_path: &str,
    module_id: &str,
    scope_stack: &mut Vec<(String, &'static str, Option<String>)>,
    symbols: &mut Vec<Symbol>,
    references: &mut Vec<NormalizedReference>,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        rs_visit_node(child, source, file_path, module_id, scope_stack, symbols, references);
    }
}

fn rs_node_text<'a>(node: Node, source: &'a [u8]) -> &'a str {
    node.utf8_text(source).unwrap_or("")
}

fn rs_current_scope(module_id: &str, scope_stack: &[(String, &'static str, Option<String>)]) -> String {
    // For impl blocks, the scope is the impl owner (e.g., MyStruct)
    for (id, kind, impl_owner) in scope_stack.iter().rev() {
        if *kind == "impl" {
            return impl_owner.clone().unwrap_or_else(|| id.clone());
        }
        if *kind != "impl" {
            return id.clone();
        }
    }
    module_id.to_string()
}

fn rs_qualify(name: &str, module_id: &str, scope_stack: &[(String, &'static str, Option<String>)]) -> String {
    format!("{}::{}", rs_current_scope(module_id, scope_stack), name)
}

fn rs_extract_function(
    node: Node,
    source: &[u8],
    file_path: &str,
    module_id: &str,
    scope_stack: &[(String, &'static str, Option<String>)],
    symbols: &mut Vec<Symbol>,
) {
    let Some(name_node) = node.child_by_field_name("name") else { return };
    let name = rs_node_text(name_node, source);
    let id = rs_qualify(name, module_id, scope_stack);
    let parent_id = rs_current_scope(module_id, scope_stack);
    let line = node.start_position().row + 1;
    let end_line = node.end_position().row + 1;

    let is_method = scope_stack.iter().rev().any(|(_, k, _)| *k == "impl" || *k == "trait");
    let symbol_type = if is_method { "method" } else { "function" };

    let param_count = node.child_by_field_name("parameters")
        .map(|p| p.named_child_count())
        .unwrap_or(0);

    let mut sym = rs_base_symbol(
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
    symbols.push(sym);
}

fn rs_extract_use(
    node: Node,
    source: &[u8],
    file_path: &str,
    module_id: &str,
    scope_stack: &[(String, &'static str, Option<String>)],
    references: &mut Vec<NormalizedReference>,
) {
    let caller_id = rs_current_scope(module_id, scope_stack);
    let line = node.start_position().row + 1;

    // Extract the use path text
    if let Some(arg) = node.child_by_field_name("argument") {
        let path_text = rs_node_text(arg, source);
        let target_id = format!("rust::{}", path_text.replace("::", "::"));
        references.push(NormalizedReference {
            source_symbol_id: caller_id,
            target_symbol_id: target_id,
            category: ReferenceCategory::ModuleImport,
            file_path: file_path.to_string(),
            line,
            confidence: RawExtractionConfidence::High,
        });
    }
}

fn rs_base_symbol(
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

/// Dual-extraction for Rust.
pub fn parse_rust_file_dual(file_path: &str, source: &str) -> Result<ParseResult, String> {
    let legacy = parse_rust_file(file_path, source)?;
    let ts_result = parse_rust_file_treesitter(file_path, source)?;

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
    fn treesitter_extracts_struct_enum_fn() {
        let source = r#"
struct Point {
    x: f64,
    y: f64,
}

enum Color {
    Red,
    Green,
    Blue,
}

fn distance(a: &Point, b: &Point) -> f64 {
    0.0
}
"#;
        let result = parse_rust_file_treesitter("src/geo.rs", source).unwrap();
        assert!(result.symbols.iter().any(|s| s.symbol_type == "struct" && s.name == "Point"),
            "Point struct should be extracted");
        assert!(result.symbols.iter().any(|s| s.symbol_type == "enum" && s.name == "Color"),
            "Color enum should be extracted");
        assert!(result.symbols.iter().any(|s| s.symbol_type == "function" && s.name == "distance"),
            "distance function should be extracted");
    }

    #[test]
    fn treesitter_extracts_impl_methods() {
        let source = r#"
struct Engine {}

impl Engine {
    pub fn new() -> Self {
        Engine {}
    }

    pub fn start(&self) {
        self.run();
    }
}
"#;
        let result = parse_rust_file_treesitter("src/engine.rs", source).unwrap();
        assert!(result.symbols.iter().any(|s| s.symbol_type == "struct" && s.name == "Engine"),
            "Engine struct should be extracted");
        assert!(result.symbols.iter().any(|s| s.symbol_type == "method" && s.name == "new"),
            "new method should be extracted; symbols: {:?}",
            result.symbols.iter().map(|s| format!("{}({})", s.name, s.symbol_type)).collect::<Vec<_>>());
        assert!(result.symbols.iter().any(|s| s.symbol_type == "method" && s.name == "start"),
            "start method should be extracted");
    }

    #[test]
    fn treesitter_extracts_calls() {
        let source = r#"
fn main() {
    process();
    obj.update();
    self.refresh();
    Engine::new();
}
"#;
        let result = parse_rust_file_treesitter("src/main.rs", source).unwrap();
        assert!(result.raw_calls.iter().any(|c| c.called_name == "process"),
            "unqualified call 'process' should be extracted");
        assert!(result.raw_calls.iter().any(|c| c.called_name == "update"),
            "member call 'update' should be extracted");
        assert!(result.raw_calls.iter().any(|c| c.called_name == "new"),
            "qualified call 'Engine::new' should be extracted");
    }

    #[test]
    fn treesitter_extracts_use_as_module_import() {
        let source = r#"
use std::collections::HashMap;
use crate::models::Symbol;

fn main() {}
"#;
        let result = parse_rust_file_treesitter("src/main.rs", source).unwrap();
        assert!(result.normalized_references.iter().any(|r| r.category == ReferenceCategory::ModuleImport),
            "use declarations should produce ModuleImport references");
    }

    #[test]
    fn treesitter_dual_result_matches_or_exceeds_legacy() {
        let source = r#"
pub mod nested {
    pub struct Engine {
        pub name: String,
    }

    impl Engine {
        pub fn new(name: &str) -> Self {
            Engine { name: name.into() }
        }
    }
}
"#;
        let legacy = parse_rust_file("src/lib.rs", source).unwrap();
        let ts = parse_rust_file_treesitter("src/lib.rs", source).unwrap();
        assert!(
            ts.symbols.len() >= (legacy.symbols.len() as f64 * 0.9) as usize,
            "tree-sitter symbols {} should be >= 90% of legacy {}",
            ts.symbols.len(), legacy.symbols.len()
        );
    }
}
