use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::time::Instant;

use tree_sitter::{Node, Parser, Tree};
use tree_sitter_graph::ast::File as GraphDslFile;
use tree_sitter_graph::functions::Functions as GraphFunctions;
use tree_sitter_graph::graph::Value as GraphValue;
use tree_sitter_graph::ExecutionConfig as GraphExecutionConfig;
use tree_sitter_graph::{NoCancellation, Variables as GraphVariables};
use crate::graph_rules::CPP_CALL_RELATIONS;
use crate::models::{
    CallableFlowSummary, FileRiskSignals, IncludeHeaviness, MacroSensitivity,
    NormalizedReference, ParseFragility, ParseResult, PropagationAnchor,
    PropagationAnchorKind, PropagationEvent, PropagationKind, PropagationRisk,
    ParseMetrics,
    compact_callable_flow_summary, compact_propagation_event,
    RawCallKind, RawCallSite, RawEventSource, RawExtractionConfidence,
    RawQualifierKind, RawReceiverKind, RawRelationEvent, RawRelationKind,
    ReferenceCategory, Symbol,
};

struct Ctx {
    file_path: String,
    source: Vec<u8>,
    symbols: Vec<Symbol>,
    raw_calls: Vec<RawCallSite>,
    ns_stack: Vec<String>,
    namespace_ids: HashSet<String>,
}

impl Ctx {
    fn qualify(&self, name: &str) -> String {
        let mut parts: Vec<&str> = self.ns_stack.iter().map(|s| s.as_str()).collect();
        parts.push(name);
        parts.join("::")
    }

    fn node_text(&self, node: Node) -> String {
        node.utf8_text(&self.source).unwrap_or("").to_string()
    }
}

enum WalkItem<'a> {
    Enter(Node<'a>),
    EnterClassMember(Node<'a>, String),
    ExitNamespace,
    ExitClass,
}

thread_local! {
    static CPP_GRAPH_FILE: RefCell<Option<Result<GraphDslFile, String>>> = const { RefCell::new(None) };
}

thread_local! {
    static LANG_GRAPH_FILES: RefCell<HashMap<&'static str, Option<Result<GraphDslFile, String>>>> =
        RefCell::new(HashMap::new());
}

/// Generic graph rule execution used by all language parsers.
/// Returns (events, compile_ms, execute_ms).
pub fn execute_graph_rules(
    language: tree_sitter::Language,
    language_name: &'static str,
    tsg_source: &str,
    tree: &Tree,
    source: &str,
    file_path: &str,
    symbols: &[Symbol],
) -> (Vec<RawRelationEvent>, u128, u128) {
    LANG_GRAPH_FILES.with(|slot| {
        let mut compile_ms = 0u128;
        {
            let mut map = slot.borrow_mut();
            if !map.contains_key(language_name) {
                let compile_start = Instant::now();
                let parsed = GraphDslFile::from_str(language.clone(), tsg_source)
                    .map_err(|err| err.to_string());
                compile_ms = compile_start.elapsed().as_millis();
                map.insert(language_name, Some(parsed));
            }
        }

        let functions = GraphFunctions::stdlib();
        let globals = GraphVariables::new();
        let config = GraphExecutionConfig::new(&functions, &globals).lazy(true);
        let borrowed = slot.borrow();
        let Some(Some(Ok(graph_file))) = borrowed.get(language_name) else {
            return (Vec::new(), compile_ms, 0);
        };

        let execute_start = Instant::now();
        let graph = match graph_file.execute(tree, source, &config, &NoCancellation) {
            Ok(graph) => graph,
            Err(_) => return (Vec::new(), compile_ms, execute_start.elapsed().as_millis()),
        };
        let execute_ms = execute_start.elapsed().as_millis();

        let mut events = Vec::new();
        for node_ref in graph.iter_nodes() {
            let attrs = &graph[node_ref].attributes;
            if !matches!(read_attr_str(attrs, "relation_kind"), Some("call" | "type_usage" | "inheritance")) {
                continue;
            }

            let relation_kind = match read_attr_str(attrs, "relation_kind") {
                Some("call") => RawRelationKind::Call,
                Some("type_usage") => RawRelationKind::TypeUsage,
                Some("inheritance") => RawRelationKind::Inheritance,
                _ => continue,
            };

            let target_name = read_attr_string(attrs, "target_name");
            let line = read_attr_u32(attrs, "line").map(|v| v as usize).unwrap_or(0);
            if relation_kind == RawRelationKind::Call && (target_name.is_none() || line == 0) {
                continue;
            }

            let qualifier = read_attr_string(attrs, "qualifier");
            let confidence = match relation_kind {
                RawRelationKind::Inheritance => RawExtractionConfidence::Partial,
                RawRelationKind::TypeUsage => RawExtractionConfidence::Partial,
                RawRelationKind::EnumValueUsage => RawExtractionConfidence::Partial,
                RawRelationKind::Call => match read_attr_str(attrs, "call_kind") {
                    Some("qualified") => RawExtractionConfidence::Partial,
                    _ => {
                        if read_attr_string(attrs, "receiver")
                            .as_deref()
                            .map(|r| r.contains('('))
                            .unwrap_or(false)
                        {
                            RawExtractionConfidence::Partial
                        } else {
                            RawExtractionConfidence::High
                        }
                    }
                },
            };
            let caller_id = read_enclosing_symbol_id_from_symbols(line, symbols);
            let receiver = read_attr_string(attrs, "receiver");
            let receiver_kind = receiver.as_deref().map(infer_receiver_kind_from_text);
            let event = RawRelationEvent {
                relation_kind: relation_kind.clone(),
                source: RawEventSource::TreeSitterGraph,
                confidence,
                caller_id,
                target_name,
                call_kind: read_attr_str(attrs, "call_kind").and_then(parse_call_kind),
                argument_count: read_attr_u32(attrs, "argument_count").map(|v| v as usize),
                receiver,
                receiver_kind,
                qualifier,
                qualifier_kind: None,
                file_path: file_path.to_string(),
                line,
            };
            events.push(event);
        }

        (events, compile_ms, execute_ms)
    })
}

fn read_enclosing_symbol_id_from_symbols(line: usize, symbols: &[Symbol]) -> Option<String> {
    if line == 0 {
        return None;
    }
    symbols
        .iter()
        .filter(|s| s.line <= line && line <= s.end_line)
        .min_by_key(|s| s.end_line.saturating_sub(s.line))
        .map(|s| s.id.clone())
}

const DEFAULT_CPP_PARSE_TIMEOUT_MICROS: u64 = 60_000_000;
const CPP_PARSE_TIMEOUT_MICROS_ENV: &str = "CODEATLAS_CPP_PARSE_TIMEOUT_MICROS";

fn graph_call_extraction_enabled() -> bool {
    !matches!(
        std::env::var("CODEATLAS_DISABLE_GRAPH_CALLS").ok().as_deref(),
        Some("1" | "true" | "TRUE" | "yes" | "YES")
    )
}

fn local_propagation_enabled() -> bool {
    !matches!(
        std::env::var("CODEATLAS_DISABLE_LOCAL_PROPAGATION").ok().as_deref(),
        Some("1" | "true" | "TRUE" | "yes" | "YES")
    )
}

pub fn parse_cpp_file(file_path: &str, source: &str) -> Result<ParseResult, String> {
    let mut parser = Parser::new();
    let lang = tree_sitter_cpp::LANGUAGE;
    parser
        .set_language(&lang.into())
        .map_err(|e| format!("Failed to set language: {}", e))?;
    let timeout_micros = configured_cpp_parse_timeout_micros();
    if let Some(timeout_micros) = timeout_micros {
        parser.set_timeout_micros(timeout_micros);
    }

    let tree_parse_start = Instant::now();
    let tree: Tree = parser
        .parse(source, None)
        .ok_or_else(|| match timeout_micros {
            Some(timeout_micros) => format!("Parse timed out after {}ms", timeout_micros / 1_000),
            None => "Failed to parse".to_string(),
        })?;
    let tree_sitter_parse_ms = tree_parse_start.elapsed().as_millis();

    let mut ctx = Ctx {
        file_path: file_path.to_string(),
        source: source.as_bytes().to_vec(),
        symbols: Vec::new(),
        raw_calls: Vec::new(),
        ns_stack: Vec::new(),
        namespace_ids: HashSet::new(),
    };

    let syntax_walk_start = Instant::now();
    visit_tree(tree.root_node(), &mut ctx);
    let syntax_walk_ms = syntax_walk_start.elapsed().as_millis();
    let file_risk_signals = derive_file_risk_signals(tree.root_node(), source);

    let local_flow_candidates_exist = ctx.symbols.iter().any(|symbol| {
        (symbol.symbol_type == "function" || symbol.symbol_type == "method")
            && matches!(
                symbol.symbol_role.as_deref(),
                Some("definition") | Some("inline_definition")
            )
    });
    let local_propagation_start = Instant::now();
    let (mut propagation_events, mut callable_flow_summaries, local_metrics) = if local_propagation_enabled()
        && local_flow_candidates_exist
    {
        extract_local_propagation_data(tree.root_node(), &ctx)
    } else {
        (Vec::new(), Vec::new(), LocalPropagationMetrics::default())
    };
    let local_propagation_ms = local_propagation_start.elapsed().as_millis();
    for event in &mut propagation_events {
        compact_propagation_event(event);
    }
    for summary in &mut callable_flow_summaries {
        compact_callable_flow_summary(summary);
    }
    let legacy_type_usage_events = extract_type_usage_relation_events(tree.root_node(), &ctx);
    let legacy_inheritance_events = extract_inheritance_relation_events(tree.root_node(), &ctx);
    let graph_relation_start = Instant::now();
    let (graph_relation_events, graph_rule_compile_ms, graph_rule_execute_ms) =
        if graph_call_extraction_enabled() {
            extract_graph_relation_events(file_path, source, &tree, &ctx)
        } else {
            (Vec::new(), 0, 0)
        };
    let graph_relation_ms = graph_relation_start.elapsed().as_millis();
    let enum_value_references = extract_enum_value_references(tree.root_node(), &ctx, &ctx.symbols);
    let enum_value_relation_events = extract_enum_value_relation_events(tree.root_node(), &ctx);
    let legacy_raw_calls = ctx.raw_calls;
    let legacy_relation_events: Vec<RawRelationEvent> = legacy_raw_calls
        .iter()
        .map(|raw_call| RawRelationEvent::from_raw_call_site(raw_call, RawEventSource::LegacyAst))
        .collect();
    // Separate field access events from method call events for parity comparison.
    // Field access events exist only in graph extraction (no legacy equivalent),
    // so they must be excluded from graph-vs-legacy parity checks and always kept.
    let (field_access_events, method_graph_events): (Vec<_>, Vec<_>) =
        graph_relation_events.into_iter().partition(|event| {
            matches!(
                event.call_kind,
                Some(RawCallKind::FieldAccess | RawCallKind::PointerFieldAccess | RawCallKind::ThisFieldAccess)
            )
        });
    let field_access_raw_calls: Vec<RawCallSite> = field_access_events
        .iter()
        .filter_map(RawRelationEvent::to_raw_call_site)
        .collect();
    let graph_raw_calls: Vec<RawCallSite> = method_graph_events
        .iter()
        .filter_map(RawRelationEvent::to_raw_call_site)
        .collect();

    let (mut relation_events, mut raw_calls) = if graph_matches_legacy_calls(&graph_raw_calls, &legacy_raw_calls)
    {
        let mut relation_events = method_graph_events;
        relation_events.extend(legacy_type_usage_events);
        relation_events.extend(legacy_inheritance_events);
        (
            relation_events,
            enrich_graph_raw_calls_with_legacy_details(graph_raw_calls, &legacy_raw_calls),
        )
    } else {
        let mut mixed_relation_events = legacy_relation_events;
        mixed_relation_events.extend(legacy_type_usage_events);
        mixed_relation_events.extend(legacy_inheritance_events);
        mixed_relation_events.extend(
            method_graph_events
                .into_iter()
                .filter(|event| event.relation_kind != RawRelationKind::Call),
        );
        (mixed_relation_events, legacy_raw_calls)
    };
    // Always include field access events and raw calls regardless of parity result.
    relation_events.extend(field_access_events);
    raw_calls.extend(field_access_raw_calls);
    relation_events.extend(enum_value_relation_events);
    let reference_normalization_start = Instant::now();
    let mut normalized_references = normalize_relation_events(&relation_events, &ctx.symbols);
    merge_normalized_references(&mut normalized_references, enum_value_references);
    let reference_normalization_ms = reference_normalization_start.elapsed().as_millis();

    Ok(ParseResult {
        symbols: ctx.symbols,
        file_risk_signals,
        relation_events,
        normalized_references,
        propagation_events,
        callable_flow_summaries,
        raw_calls,
        metrics: ParseMetrics {
            tree_sitter_parse_ms,
            syntax_walk_ms,
            local_propagation_ms,
            local_function_discovery_ms: local_metrics.function_discovery_ms,
            local_owner_lookup_ms: local_metrics.owner_lookup_ms,
            local_seed_ms: local_metrics.seed_ms,
            local_event_walk_ms: local_metrics.event_walk_ms,
            local_declaration_ms: local_metrics.declaration_ms,
            local_expression_statement_ms: local_metrics.expression_statement_ms,
            local_return_statement_ms: local_metrics.return_statement_ms,
            local_nested_block_ms: local_metrics.nested_block_ms,
            local_return_collection_ms: local_metrics.return_collection_ms,
            graph_relation_ms,
            graph_rule_compile_ms,
            graph_rule_execute_ms,
            reference_normalization_ms,
        },
    })
}

fn configured_cpp_parse_timeout_micros() -> Option<u64> {
    match std::env::var(CPP_PARSE_TIMEOUT_MICROS_ENV) {
        Ok(value) => parse_optional_timeout_micros(&value),
        Err(_) => Some(DEFAULT_CPP_PARSE_TIMEOUT_MICROS),
    }
}

fn parse_optional_timeout_micros(value: &str) -> Option<u64> {
    let trimmed = value.trim();
    let micros = trimmed.parse::<u64>().ok()?;
    if micros == 0 {
        return None;
    }
    Some(micros)
}

fn derive_file_risk_signals(root: Node, source: &str) -> FileRiskSignals {
    let mut include_count = 0usize;
    let mut macro_lines = 0usize;
    let mut conditional_macro_lines = 0usize;
    let mut function_like_defines = 0usize;

    for line in source.lines() {
        let trimmed = line.trim_start();
        if let Some(directive) = trimmed.strip_prefix('#') {
            macro_lines += 1;
            let directive = directive.trim_start();
            if directive.starts_with("include") {
                include_count += 1;
            }
            if directive.starts_with("if")
                || directive.starts_with("ifdef")
                || directive.starts_with("ifndef")
                || directive.starts_with("elif")
            {
                conditional_macro_lines += 1;
            }
            if let Some(rest) = directive.strip_prefix("define") {
                let rest = rest.trim_start();
                if rest.contains('(') {
                    function_like_defines += 1;
                }
            }
        }
    }

    let macro_sensitive = macro_lines >= 12 || conditional_macro_lines >= 4 || function_like_defines >= 3;
    let include_heavy = include_count >= 15;
    let parse_fragile = root.has_error() || macro_sensitive || include_count >= 25;

    FileRiskSignals {
        parse_fragility: if parse_fragile {
            ParseFragility::Elevated
        } else {
            ParseFragility::Low
        },
        macro_sensitivity: if macro_sensitive {
            MacroSensitivity::High
        } else {
            MacroSensitivity::Low
        },
        include_heaviness: if include_heavy {
            IncludeHeaviness::Heavy
        } else {
            IncludeHeaviness::Light
        },
    }
}

fn enrich_graph_raw_calls_with_legacy_details(
    graph_raw_calls: Vec<RawCallSite>,
    legacy_raw_calls: &[RawCallSite],
) -> Vec<RawCallSite> {
    let mut legacy_by_key: HashMap<String, Vec<&RawCallSite>> = HashMap::new();
    for raw_call in legacy_raw_calls {
        legacy_by_key
            .entry(raw_call_comparison_key(raw_call))
            .or_default()
            .push(raw_call);
    }

    let mut enriched = Vec::new();
    for mut raw_call in graph_raw_calls {
        if let Some(candidates) = legacy_by_key.get_mut(&raw_call_comparison_key(&raw_call)) {
            if let Some(legacy) = candidates.pop() {
                raw_call.argument_texts = legacy.argument_texts.clone();
                raw_call.result_target = legacy.result_target.clone();
            }
        }
        enriched.push(raw_call);
    }

    enriched
}

#[cfg(test)]
pub fn cpp_call_graph_rule_source() -> &'static str {
    CPP_CALL_RELATIONS
}

fn extract_graph_relation_events(
    file_path: &str,
    source: &str,
    tree: &Tree,
    ctx: &Ctx,
) -> (Vec<RawRelationEvent>, u128, u128) {
    CPP_GRAPH_FILE.with(|slot| {
        let mut compile_ms = 0;
        if slot.borrow().is_none() {
            let compile_start = Instant::now();
            let parsed = GraphDslFile::from_str(tree_sitter_cpp::LANGUAGE.into(), CPP_CALL_RELATIONS)
                .map_err(|err| err.to_string());
            compile_ms = compile_start.elapsed().as_millis();
            *slot.borrow_mut() = Some(parsed);
        }

        let functions = GraphFunctions::stdlib();
        let globals = GraphVariables::new();
        let config = GraphExecutionConfig::new(&functions, &globals).lazy(true);
        let borrowed = slot.borrow();
        let Some(Ok(graph_file)) = borrowed.as_ref() else {
            return (Vec::new(), compile_ms, 0);
        };

        let execute_start = Instant::now();
        let graph = match graph_file.execute(tree, source, &config, &NoCancellation) {
            Ok(graph) => graph,
            Err(_) => return (Vec::new(), compile_ms, execute_start.elapsed().as_millis()),
        };
        let execute_ms = execute_start.elapsed().as_millis();

        let mut events = Vec::new();
        for node_ref in graph.iter_nodes() {
            let attrs = &graph[node_ref].attributes;
            if !matches!(read_attr_str(attrs, "relation_kind"), Some("call" | "type_usage" | "inheritance")) {
                continue;
            }

            let relation_kind = match read_attr_str(attrs, "relation_kind") {
                Some("call") => RawRelationKind::Call,
                Some("type_usage") => RawRelationKind::TypeUsage,
                Some("inheritance") => RawRelationKind::Inheritance,
                _ => continue,
            };

            let target_name = read_attr_string(attrs, "target_name");
            let line = read_attr_u32(attrs, "line").map(|value| value as usize).unwrap_or(0);
            if relation_kind == RawRelationKind::Call && (target_name.is_none() || line == 0) {
                continue;
            }

            let qualifier = read_attr_string(attrs, "qualifier");
            let qualifier_kind = qualifier
                .as_deref()
                .and_then(|value| classify_qualifier_kind(value, ctx));
            let confidence = match relation_kind {
                RawRelationKind::Inheritance => RawExtractionConfidence::Partial,
                RawRelationKind::TypeUsage => RawExtractionConfidence::Partial,
                RawRelationKind::EnumValueUsage => RawExtractionConfidence::Partial,
                RawRelationKind::Call => match read_attr_str(attrs, "call_kind") {
                    Some("qualified") => RawExtractionConfidence::Partial,
                    _ => {
                        if read_attr_string(attrs, "receiver")
                            .as_deref()
                            .map(|receiver| receiver.contains('('))
                            .unwrap_or(false)
                        {
                            RawExtractionConfidence::Partial
                        } else {
                            RawExtractionConfidence::High
                        }
                    }
                },
            };
            let event = RawRelationEvent {
                relation_kind: relation_kind.clone(),
                source: RawEventSource::TreeSitterGraph,
                confidence,
                caller_id: if relation_kind == RawRelationKind::Call {
                    read_enclosing_caller_id(tree.root_node(), line, ctx)
                } else {
                    read_enclosing_symbol_id(line, ctx)
                },
                target_name,
                call_kind: read_attr_str(attrs, "call_kind").and_then(parse_call_kind),
                argument_count: read_attr_u32(attrs, "argument_count").map(|value| value as usize),
                receiver: read_attr_string(attrs, "receiver"),
                receiver_kind: graph_receiver_kind(read_attr_string(attrs, "receiver"), None),
                qualifier,
                qualifier_kind,
                file_path: file_path.to_string(),
                line,
            };
            events.push(event);
        }

        // Deduplicate: field access events that overlap with method call events
        // at the same (line, target_name, receiver) are suppressed. Method call
        // events carry richer data (argument_count) and take priority.
        let mut seen_method_calls: HashSet<(usize, String, Option<String>)> = HashSet::new();
        for event in &events {
            if matches!(
                event.call_kind,
                Some(RawCallKind::MemberAccess | RawCallKind::PointerMemberAccess | RawCallKind::ThisPointerAccess)
            ) {
                let key = (
                    event.line,
                    event.target_name.clone().unwrap_or_default(),
                    event.receiver.clone(),
                );
                seen_method_calls.insert(key);
            }
        }
        events.retain(|event| {
            if matches!(
                event.call_kind,
                Some(RawCallKind::FieldAccess | RawCallKind::PointerFieldAccess | RawCallKind::ThisFieldAccess)
            ) {
                let key = (
                    event.line,
                    event.target_name.clone().unwrap_or_default(),
                    event.receiver.clone(),
                );
                !seen_method_calls.contains(&key)
            } else {
                true
            }
        });

        (events, compile_ms, execute_ms)
    })
}

fn graph_receiver_kind(
    receiver: Option<String>,
    explicit_kind: Option<RawReceiverKind>,
) -> Option<RawReceiverKind> {
    explicit_kind.or_else(|| receiver.as_deref().map(infer_receiver_kind_from_text))
}

fn infer_receiver_kind_from_text(receiver: &str) -> RawReceiverKind {
    let trimmed = receiver.trim();
    if trimmed == "this" {
        RawReceiverKind::This
    } else if is_simple_identifier(trimmed) {
        RawReceiverKind::Identifier
    } else if trimmed.contains("->") {
        RawReceiverKind::PointerExpression
    } else if trimmed.contains('.') {
        RawReceiverKind::FieldExpression
    } else if trimmed.contains("::") {
        RawReceiverKind::QualifiedIdentifier
    } else {
        RawReceiverKind::Other
    }
}

fn is_simple_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) if first == '_' || first.is_ascii_alphabetic() => {}
        _ => return false,
    }
    chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn read_enclosing_symbol_id(line: usize, ctx: &Ctx) -> Option<String> {
    if line == 0 {
        return None;
    }

    ctx.symbols
        .iter()
        .filter(|symbol| symbol.line <= line && line <= symbol.end_line)
        .min_by_key(|symbol| symbol.end_line.saturating_sub(symbol.line))
        .map(|symbol| symbol.id.clone())
}

pub fn normalize_relation_events(
    relation_events: &[RawRelationEvent],
    symbols: &[Symbol],
) -> Vec<NormalizedReference> {
    let symbol_index = build_symbol_index(symbols);
    let mut references = Vec::new();
    let mut seen = HashSet::new();

    for event in relation_events {
        let category = match event.relation_kind {
            RawRelationKind::TypeUsage => ReferenceCategory::TypeUsage,
            RawRelationKind::Inheritance => ReferenceCategory::InheritanceMention,
            RawRelationKind::EnumValueUsage => ReferenceCategory::EnumValueUsage,
            RawRelationKind::Call => continue,
        };

        let source_symbol_id = match event.caller_id.as_deref() {
            Some(id) => id,
            None => continue,
        };
        let target_symbol_id = match resolve_reference_target_id(event, source_symbol_id, &symbol_index) {
            Some(id) => id,
            None => continue,
        };

        let normalized = NormalizedReference {
            source_symbol_id: source_symbol_id.to_string(),
            target_symbol_id: target_symbol_id.to_string(),
            category,
            file_path: event.file_path.clone(),
            line: event.line,
            confidence: event.confidence.clone(),
        };
        let key = format!(
            "{}|{}|{:?}|{}|{}",
            normalized.source_symbol_id,
            normalized.target_symbol_id,
            normalized.category,
            normalized.file_path,
            normalized.line
        );
        if seen.insert(key) {
            references.push(normalized);
        }
    }

    references
}

fn extract_type_usage_relation_events(root: Node, ctx: &Ctx) -> Vec<RawRelationEvent> {
    let mut events = Vec::new();
    let mut stack = vec![root];

    while let Some(node) = stack.pop() {
        match node.kind() {
            "declaration" | "field_declaration" | "parameter_declaration" => {
                if let Some(type_node) = node.child_by_field_name("type") {
                    push_type_usage_events_recursive(type_node, ctx, &mut events);
                }
                for child in named_children(node) {
                    stack.push(child);
                }
            }
            "function_definition" => {
                if let Some(type_node) = node.child_by_field_name("type") {
                    push_type_usage_events_recursive(type_node, ctx, &mut events);
                }
                for child in named_children(node) {
                    stack.push(child);
                }
            }
            _ => {
                for child in named_children(node) {
                    stack.push(child);
                }
            }
        }
    }

    events
}

fn extract_enum_value_relation_events(
    root: Node,
    ctx: &Ctx,
) -> Vec<RawRelationEvent> {
    let mut events = Vec::new();
    let mut seen = HashSet::new();
    let mut stack = vec![root];

    while let Some(node) = stack.pop() {
        if node.kind() == "qualified_identifier" || node.kind() == "identifier" {
            if let Some(reference) = enum_value_event_for_node(node, ctx) {
                let key = format!(
                    "{}|{}|{}|{}",
                    reference.caller_id.as_deref().unwrap_or_default(),
                    reference.target_name.as_deref().unwrap_or_default(),
                    reference.file_path,
                    reference.line
                );
                if seen.insert(key) {
                    events.push(reference);
                }
            }
        }

        for child in named_children(node) {
            stack.push(child);
        }
    }

    events
}

fn extract_enum_value_references(
    root: Node,
    ctx: &Ctx,
    symbols: &[Symbol],
) -> Vec<NormalizedReference> {
    let symbol_index = build_symbol_index(symbols);
    let mut references = Vec::new();
    let mut seen = HashSet::new();
    let mut stack = vec![root];

    while let Some(node) = stack.pop() {
        if node.kind() == "qualified_identifier" || node.kind() == "identifier" {
            if let Some(reference) = enum_value_reference_for_node(node, ctx, &symbol_index) {
                let key = format!(
                    "{}|{}|{:?}|{}|{}",
                    reference.source_symbol_id,
                    reference.target_symbol_id,
                    reference.category,
                    reference.file_path,
                    reference.line
                );
                if seen.insert(key) {
                    references.push(reference);
                }
            }
        }

        for child in named_children(node) {
            stack.push(child);
        }
    }

    references
}

fn enum_value_reference_for_node(
    node: Node,
    ctx: &Ctx,
    symbol_index: &HashMap<String, Vec<Symbol>>,
) -> Option<NormalizedReference> {
    if !enum_value_usage_context(node) {
        return None;
    }

    let line = node.start_position().row + 1;
    let source_symbol_id = read_enclosing_symbol_id(line, ctx)?;
    let target_name = ctx.node_text(node);
    let event = RawRelationEvent {
        relation_kind: RawRelationKind::EnumValueUsage,
        source: RawEventSource::LegacyAst,
        confidence: RawExtractionConfidence::Partial,
        caller_id: Some(source_symbol_id.clone()),
        target_name: Some(target_name),
        call_kind: None,
        argument_count: None,
        receiver: None,
        receiver_kind: None,
        qualifier: None,
        qualifier_kind: None,
        file_path: ctx.file_path.clone(),
        line,
    };
    let target_symbol_id = resolve_enum_value_target_id(
        node,
        &event,
        &source_symbol_id,
        ctx,
        symbol_index,
    )
    .or_else(|| resolve_reference_target_id(&event, &source_symbol_id, symbol_index))?;

    Some(NormalizedReference {
        source_symbol_id,
        target_symbol_id,
        category: ReferenceCategory::EnumValueUsage,
        file_path: ctx.file_path.clone(),
        line,
        confidence: RawExtractionConfidence::Partial,
    })
}

fn enum_value_event_for_node(
    node: Node,
    ctx: &Ctx,
) -> Option<RawRelationEvent> {
    if !enum_value_usage_context(node) {
        return None;
    }

    let line = node.start_position().row + 1;
    let source_symbol_id = read_enclosing_symbol_id(line, ctx)?;
    let target_name = ctx.node_text(node);
    Some(RawRelationEvent {
        relation_kind: RawRelationKind::EnumValueUsage,
        source: RawEventSource::LegacyAst,
        confidence: RawExtractionConfidence::Partial,
        caller_id: Some(source_symbol_id.clone()),
        target_name: Some(target_name),
        call_kind: None,
        argument_count: None,
        receiver: None,
        receiver_kind: None,
        qualifier: None,
        qualifier_kind: None,
        file_path: ctx.file_path.clone(),
        line,
    })
}

fn enum_value_usage_context(node: Node) -> bool {
    let mut current = node.parent();
    while let Some(parent) = current {
        match parent.kind() {
            "argument_list"
            | "assignment_expression"
            | "binary_expression"
            | "return_statement"
            | "conditional_expression"
            | "parenthesized_expression" => {
                return true;
            }
            "init_declarator" | "initializer_list" => {
                return true;
            }
            "function_definition" | "declaration" | "field_declaration" | "parameter_declaration" => {
                return false;
            }
            _ => {
                current = parent.parent();
            }
        }
    }

    false
}

fn merge_normalized_references(
    references: &mut Vec<NormalizedReference>,
    incoming: Vec<NormalizedReference>,
) {
    let mut seen: HashSet<String> = references
        .iter()
        .map(|reference| {
            format!(
                "{}|{}|{:?}|{}|{}",
                reference.source_symbol_id,
                reference.target_symbol_id,
                reference.category,
                reference.file_path,
                reference.line
            )
        })
        .collect();

    for reference in incoming {
        let key = format!(
            "{}|{}|{:?}|{}|{}",
            reference.source_symbol_id,
            reference.target_symbol_id,
            reference.category,
            reference.file_path,
            reference.line
        );
        if seen.insert(key) {
            references.push(reference);
        }
    }
}

fn extract_inheritance_relation_events(root: Node, ctx: &Ctx) -> Vec<RawRelationEvent> {
    let mut events = Vec::new();
    let mut stack = vec![root];

    while let Some(node) = stack.pop() {
        match node.kind() {
            "class_specifier" | "struct_specifier" => {
                if let Some(base_clause) = find_child(node, "base_class_clause") {
                    for child in named_children(base_clause) {
                        match child.kind() {
                            "type_identifier" | "qualified_identifier" => {
                                push_inheritance_event(node, child, ctx, &mut events);
                            }
                            _ => {}
                        }
                    }
                }
            }
            _ => {
                for child in named_children(node) {
                    stack.push(child);
                }
            }
        }
    }

    events
}

fn push_type_usage_event(node: Node, ctx: &Ctx, events: &mut Vec<RawRelationEvent>) {
    let Some((target_name, qualifier, qualifier_kind)) = type_usage_target(node, ctx) else {
        return;
    };
    let line = node.start_position().row + 1;
    events.push(RawRelationEvent {
        relation_kind: RawRelationKind::TypeUsage,
        source: RawEventSource::LegacyAst,
        confidence: RawExtractionConfidence::Partial,
        caller_id: read_enclosing_symbol_id(line, ctx),
        target_name: Some(target_name),
        call_kind: None,
        argument_count: None,
        receiver: None,
        receiver_kind: None,
        qualifier,
        qualifier_kind,
        file_path: ctx.file_path.clone(),
        line,
    });
}

fn push_type_usage_events_recursive(node: Node, ctx: &Ctx, events: &mut Vec<RawRelationEvent>) {
    match node.kind() {
        "type_identifier" | "qualified_identifier" => {
            push_type_usage_event(node, ctx, events);
        }
        "template_type" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                push_type_usage_events_recursive(name_node, ctx, events);
            }
            if let Some(args) = node.child_by_field_name("arguments") {
                for child in named_children(args) {
                    push_type_usage_events_recursive(child, ctx, events);
                }
            }
        }
        "type_descriptor" => {
            if let Some(type_node) = node.child_by_field_name("type") {
                push_type_usage_events_recursive(type_node, ctx, events);
            }
        }
        "pointer_declarator" | "reference_declarator" | "abstract_pointer_declarator"
        | "abstract_reference_declarator" => {
            for child in named_children(node) {
                push_type_usage_events_recursive(child, ctx, events);
            }
        }
        _ => {}
    }
}

fn push_inheritance_event(class_node: Node, base_node: Node, ctx: &Ctx, events: &mut Vec<RawRelationEvent>) {
    let Some((target_name, qualifier, qualifier_kind)) = inheritance_target(base_node, ctx) else {
        return;
    };
    let caller_id = class_node
        .child_by_field_name("name")
        .map(|name_node| {
            let name = ctx.node_text(name_node);
            read_enclosing_symbol_id(base_node.start_position().row + 1, ctx)
                .unwrap_or_else(|| name)
        });
    let Some(caller_id) = caller_id else {
        return;
    };
    events.push(RawRelationEvent {
        relation_kind: RawRelationKind::Inheritance,
        source: RawEventSource::LegacyAst,
        confidence: RawExtractionConfidence::Partial,
        caller_id: Some(caller_id),
        target_name: Some(target_name),
        call_kind: None,
        argument_count: None,
        receiver: None,
        receiver_kind: None,
        qualifier,
        qualifier_kind,
        file_path: ctx.file_path.clone(),
        line: base_node.start_position().row + 1,
    });
}

fn type_usage_target(
    node: Node,
    ctx: &Ctx,
) -> Option<(String, Option<String>, Option<RawQualifierKind>)> {
    match node.kind() {
        "type_identifier" => Some((ctx.node_text(node), None, None)),
        "qualified_identifier" => {
            let text = ctx.node_text(node);
            let qualifier_kind = classify_qualifier_kind(text.as_str(), ctx);
            Some((text.clone(), Some(text), qualifier_kind))
        }
        _ => None,
    }
}

fn inheritance_target(
    node: Node,
    ctx: &Ctx,
) -> Option<(String, Option<String>, Option<RawQualifierKind>)> {
    match node.kind() {
        "type_identifier" => Some((ctx.node_text(node), None, None)),
        "qualified_identifier" => {
            let text = ctx.node_text(node);
            Some((
                text.clone(),
                Some(text.clone()),
                classify_qualifier_kind(text.as_str(), ctx),
            ))
        }
        _ => None,
    }
}

#[derive(Clone)]
struct LocalBinding {
    anchor: PropagationAnchor,
    pointer_like: bool,
}

#[derive(Clone)]
struct FlowAnchorResolution {
    anchor: PropagationAnchor,
    pointer_like: bool,
    risks: Vec<PropagationRisk>,
}

struct FunctionPropagationExtraction {
    events: Vec<PropagationEvent>,
    summary: CallableFlowSummary,
    metrics: LocalPropagationMetrics,
}

#[derive(Default)]
struct LocalPropagationMetrics {
    function_discovery_ms: u128,
    owner_lookup_ms: u128,
    seed_ms: u128,
    event_walk_ms: u128,
    declaration_ms: u128,
    expression_statement_ms: u128,
    return_statement_ms: u128,
    nested_block_ms: u128,
    return_collection_ms: u128,
}

fn extract_local_propagation_data(
    root: Node,
    ctx: &Ctx,
) -> (Vec<PropagationEvent>, Vec<CallableFlowSummary>, LocalPropagationMetrics) {
    let mut events = Vec::new();
    let mut summaries = Vec::new();
    let mut metrics = LocalPropagationMetrics::default();
    let discovery_start = Instant::now();
    let mut function_nodes = Vec::new();
    let mut stack = vec![root];

    while let Some(node) = stack.pop() {
        if node.kind() == "function_definition" {
            function_nodes.push(node);
            continue;
        }

        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            stack.push(child);
        }
    }
    metrics.function_discovery_ms = discovery_start.elapsed().as_millis();

    for node in function_nodes {
        if let Some(extraction) =
            extract_local_propagation_events_for_function(node, None, ctx)
        {
            events.extend(extraction.events);
            summaries.push(extraction.summary);
            metrics.owner_lookup_ms += extraction.metrics.owner_lookup_ms;
            metrics.seed_ms += extraction.metrics.seed_ms;
            metrics.event_walk_ms += extraction.metrics.event_walk_ms;
            metrics.declaration_ms += extraction.metrics.declaration_ms;
            metrics.expression_statement_ms += extraction.metrics.expression_statement_ms;
            metrics.return_statement_ms += extraction.metrics.return_statement_ms;
            metrics.nested_block_ms += extraction.metrics.nested_block_ms;
            metrics.return_collection_ms += extraction.metrics.return_collection_ms;
        }
    }

    (events, summaries, metrics)
}

fn extract_local_propagation_events_for_function(
    node: Node,
    owner_symbol_id_override: Option<&str>,
    ctx: &Ctx,
) -> Option<FunctionPropagationExtraction> {
    let (owner_symbol_id, owner_lookup_ms) = if let Some(owner_symbol_id) = owner_symbol_id_override {
        (owner_symbol_id.to_string(), 0)
    } else {
        let owner_lookup_start = Instant::now();
        let owner_symbol_id = match find_function_owner_symbol_id(node, ctx) {
            Some(id) => id,
            None => return None,
        };
        (owner_symbol_id, owner_lookup_start.elapsed().as_millis())
    };
    let body = match node.child_by_field_name("body") {
        Some(body) if body.kind() == "compound_statement" => body,
        _ => return None,
    };

    let mut scopes = vec![HashMap::new()];
    let seed_start = Instant::now();
    let parameter_anchors = seed_parameter_bindings(node, ctx, &owner_symbol_id, &mut scopes[0]);
    let seed_ms = seed_start.elapsed().as_millis();

    let mut events = Vec::new();
    let mut return_anchors = Vec::new();
    extract_constructor_initializer_events(
        node,
        ctx,
        &owner_symbol_id,
        &mut scopes,
        &mut events,
    );
    let event_walk_start = Instant::now();
    let mut flow_metrics = LocalPropagationMetrics::default();
    process_local_flow_block(
        body,
        ctx,
        &owner_symbol_id,
        &mut scopes,
        &mut events,
        &mut flow_metrics,
        false,
    );
    let event_walk_ms = event_walk_start.elapsed().as_millis();
    let return_collection_start = Instant::now();
    collect_return_anchors(body, ctx, &owner_symbol_id, &scopes[0], &mut return_anchors);
    let return_collection_ms = return_collection_start.elapsed().as_millis();

    Some(FunctionPropagationExtraction {
        events,
        summary: CallableFlowSummary {
            callable_symbol_id: owner_symbol_id,
            parameter_anchors,
            return_anchors,
        },
        metrics: LocalPropagationMetrics {
            function_discovery_ms: 0,
            owner_lookup_ms,
            seed_ms,
            event_walk_ms,
            declaration_ms: flow_metrics.declaration_ms,
            expression_statement_ms: flow_metrics.expression_statement_ms,
            return_statement_ms: flow_metrics.return_statement_ms,
            nested_block_ms: flow_metrics.nested_block_ms,
            return_collection_ms,
        },
    })
}

fn extract_constructor_initializer_events(
    function_node: Node,
    ctx: &Ctx,
    owner_symbol_id: &str,
    scopes: &mut Vec<HashMap<String, LocalBinding>>,
    events: &mut Vec<PropagationEvent>,
) {
    let Some(initializer_list) = function_node
        .child_by_field_name("declarator")
        .and_then(|_| find_direct_child(function_node, "field_initializer_list"))
    else {
        return;
    };

    for initializer in named_children(initializer_list) {
        if initializer.kind() != "field_initializer" {
            continue;
        }

        let Some(target) = resolve_constructor_initializer_target(initializer, owner_symbol_id, ctx) else {
            continue;
        };
        let Some(value_node) = extract_constructor_initializer_value(initializer) else {
            continue;
        };
        let Some(source) = extract_expression_flow(value_node, ctx, owner_symbol_id, scopes, events) else {
            continue;
        };

        let combined_risks = merge_propagation_risks(source.risks.clone(), target.risks.clone());
        let (confidence, risks) = combine_local_flow_risk(
            target.pointer_like,
            source.pointer_like,
            combined_risks,
        );
        events.push(PropagationEvent {
            owner_symbol_id: Some(owner_symbol_id.to_string()),
            source_anchor: source.anchor,
            target_anchor: target.anchor,
            propagation_kind: PropagationKind::FieldWrite,
            file_path: ctx.file_path.clone(),
            line: initializer.start_position().row + 1,
            confidence,
            risks,
        });
    }
}

fn find_function_owner_symbol_id(node: Node, ctx: &Ctx) -> Option<String> {
    let line = node.start_position().row + 1;
    ctx.symbols
        .iter()
        .filter(|symbol| {
            (symbol.symbol_type == "function" || symbol.symbol_type == "method")
                && matches!(
                    symbol.symbol_role.as_deref(),
                    Some("definition") | Some("inline_definition")
                )
                && symbol.line == line
        })
        .min_by_key(|symbol| symbol.end_line.saturating_sub(symbol.line))
        .map(|symbol| symbol.id.clone())
}

fn seed_parameter_bindings(
    function_node: Node,
    ctx: &Ctx,
    owner_symbol_id: &str,
    scope: &mut HashMap<String, LocalBinding>,
) -> Vec<PropagationAnchor> {
    let func_decl = match find_descendant(function_node, "function_declarator") {
        Some(node) => node,
        None => return Vec::new(),
    };
    let parameter_list = match find_descendant(func_decl, "parameter_list") {
        Some(node) => node,
        None => return Vec::new(),
    };

    let mut anchors = Vec::new();
    for child in named_children(parameter_list) {
        if child.kind() != "parameter_declaration" {
            continue;
        }
        let declarator = match child.child_by_field_name("declarator") {
            Some(node) => node,
            None => continue,
        };
        let name_node = match extract_identifier_node(declarator) {
            Some(node) => node,
            None => continue,
        };
        let name = ctx.node_text(name_node);
        let line = name_node.start_position().row + 1;
        let binding = LocalBinding {
            anchor: PropagationAnchor {
                anchor_id: Some(format!("{}::param:{}@{}", owner_symbol_id, name, line)),
                symbol_id: None,
                expression_text: Some(name.clone()),
                anchor_kind: PropagationAnchorKind::Parameter,
            },
            pointer_like: is_pointer_like_declarator(declarator),
        };
        anchors.push(binding.anchor.clone());
        scope.insert(name, binding);
    }

    anchors
}

fn process_local_flow_block(
    block: Node,
    ctx: &Ctx,
    owner_symbol_id: &str,
    scopes: &mut Vec<HashMap<String, LocalBinding>>,
    events: &mut Vec<PropagationEvent>,
    metrics: &mut LocalPropagationMetrics,
    create_scope: bool,
) {
    if create_scope {
        scopes.push(HashMap::new());
    }

    let mut cursor = block.walk();
    for child in block.named_children(&mut cursor) {
        process_local_flow_node(child, ctx, owner_symbol_id, scopes, events, metrics);
    }

    if create_scope {
        scopes.pop();
    }
}

fn process_local_flow_node(
    node: Node,
    ctx: &Ctx,
    owner_symbol_id: &str,
    scopes: &mut Vec<HashMap<String, LocalBinding>>,
    events: &mut Vec<PropagationEvent>,
    metrics: &mut LocalPropagationMetrics,
) {
    match node.kind() {
        "compound_statement" => {
            let started = Instant::now();
            process_local_flow_block(node, ctx, owner_symbol_id, scopes, events, metrics, true);
            metrics.nested_block_ms += started.elapsed().as_millis();
        }
        "declaration" => {
            let started = Instant::now();
            process_local_declaration(node, ctx, owner_symbol_id, scopes, events);
            metrics.declaration_ms += started.elapsed().as_millis();
        }
        "return_statement" => {
            let started = Instant::now();
            process_return_statement(node, ctx, owner_symbol_id, scopes, events);
            metrics.return_statement_ms += started.elapsed().as_millis();
        }
        "expression_statement" => {
            let started = Instant::now();
            let mut cursor = node.walk();
            if let Some(expression) = node.named_children(&mut cursor).next() {
                let _ = extract_expression_flow(expression, ctx, owner_symbol_id, scopes, events);
            }
            metrics.expression_statement_ms += started.elapsed().as_millis();
        }
        _ => {
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                process_local_flow_node(child, ctx, owner_symbol_id, scopes, events, metrics);
            }
        }
    }
}

fn process_return_statement(
    node: Node,
    ctx: &Ctx,
    owner_symbol_id: &str,
    scopes: &mut Vec<HashMap<String, LocalBinding>>,
    events: &mut Vec<PropagationEvent>,
) {
    let mut cursor = node.walk();
    let Some(expression) = node.named_children(&mut cursor).next() else {
        return;
    };
    let Some(source) = extract_expression_flow(expression, ctx, owner_symbol_id, scopes, events) else {
        return;
    };
    if source.anchor.anchor_kind != PropagationAnchorKind::Field {
        return;
    }

    let (confidence, risks) =
        combine_local_flow_risk(false, source.pointer_like, source.risks.clone());
    events.push(PropagationEvent {
        owner_symbol_id: Some(owner_symbol_id.to_string()),
        source_anchor: source.anchor.clone(),
        target_anchor: PropagationAnchor {
            anchor_id: Some(format!("{}::return@{}", owner_symbol_id, node.start_position().row + 1)),
            symbol_id: None,
            expression_text: source.anchor.expression_text.clone(),
            anchor_kind: PropagationAnchorKind::ReturnValue,
        },
        propagation_kind: PropagationKind::FieldRead,
        file_path: ctx.file_path.clone(),
        line: node.start_position().row + 1,
        confidence,
        risks,
    });
}

fn collect_return_anchors(
    block: Node,
    ctx: &Ctx,
    owner_symbol_id: &str,
    root_scope: &HashMap<String, LocalBinding>,
    return_anchors: &mut Vec<PropagationAnchor>,
) {
    let mut scopes = vec![root_scope.clone()];
    collect_return_anchors_in_block(
        block,
        ctx,
        owner_symbol_id,
        &mut scopes,
        return_anchors,
        false,
    );
}

fn collect_return_anchors_in_block(
    block: Node,
    ctx: &Ctx,
    owner_symbol_id: &str,
    scopes: &mut Vec<HashMap<String, LocalBinding>>,
    return_anchors: &mut Vec<PropagationAnchor>,
    create_scope: bool,
) {
    if create_scope {
        scopes.push(HashMap::new());
    }

    let mut cursor = block.walk();
    for child in block.named_children(&mut cursor) {
        match child.kind() {
            "compound_statement" => collect_return_anchors_in_block(
                child,
                ctx,
                owner_symbol_id,
                scopes,
                return_anchors,
                true,
            ),
            "declaration" => seed_declaration_bindings_for_return_scope(child, ctx, owner_symbol_id, scopes),
            "return_statement" => {
                let mut return_cursor = child.walk();
                let expression = child.named_children(&mut return_cursor).next();
                if let Some(expression) = expression {
                    if let Some(flow) =
                        extract_expression_flow(expression, ctx, owner_symbol_id, scopes, &mut Vec::new())
                    {
                        let mut anchor = flow.anchor;
                        anchor.anchor_kind = PropagationAnchorKind::ReturnValue;
                        if anchor.anchor_id.is_none() {
                            anchor.anchor_id = Some(format!(
                                "{}::return@{}",
                                owner_symbol_id,
                                child.start_position().row + 1
                            ));
                        }
                        if !return_anchors.contains(&anchor) {
                            return_anchors.push(anchor);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    if create_scope {
        scopes.pop();
    }
}

fn seed_declaration_bindings_for_return_scope(
    node: Node,
    ctx: &Ctx,
    owner_symbol_id: &str,
    scopes: &mut Vec<HashMap<String, LocalBinding>>,
) {
    if declaration_contains_function_declarator(node) {
        return;
    }

    let mut pending_bindings = Vec::new();
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        match child.kind() {
            "init_declarator" => {
                let Some(declarator) = child.child_by_field_name("declarator") else {
                    continue;
                };
                let Some(name_node) = extract_identifier_node(declarator) else {
                    continue;
                };
                let name = ctx.node_text(name_node);
                pending_bindings.push((
                    name.clone(),
                    make_local_binding(owner_symbol_id, &name, name_node.start_position().row + 1, declarator),
                ));
            }
            kind if kind.contains("declarator") && kind != "function_declarator" => {
                let Some(name_node) = extract_identifier_node(child) else {
                    continue;
                };
                let name = ctx.node_text(name_node);
                pending_bindings.push((
                    name.clone(),
                    make_local_binding(owner_symbol_id, &name, name_node.start_position().row + 1, child),
                ));
            }
            _ => {}
        }
    }

    if let Some(scope) = scopes.last_mut() {
        for (name, binding) in pending_bindings {
            scope.insert(name, binding);
        }
    }
}

fn process_local_declaration(
    node: Node,
    ctx: &Ctx,
    owner_symbol_id: &str,
    scopes: &mut Vec<HashMap<String, LocalBinding>>,
    events: &mut Vec<PropagationEvent>,
) {
    if declaration_contains_function_declarator(node) {
        return;
    }

    let mut pending_bindings = Vec::new();

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        match child.kind() {
            "init_declarator" => {
                let declarator = match child.child_by_field_name("declarator") {
                    Some(declarator) => declarator,
                    None => continue,
                };
                let name_node = match extract_identifier_node(declarator) {
                    Some(node) => node,
                    None => continue,
                };
                let name = ctx.node_text(name_node);
                let target_binding = make_local_binding(owner_symbol_id, &name, name_node.start_position().row + 1, declarator);

                if let Some(value) = child.child_by_field_name("value") {
                    if let Some(source) =
                        extract_expression_flow(value, ctx, owner_symbol_id, scopes, events)
                    {
                        let combined_risks = merge_propagation_risks(
                            source.risks.clone(),
                            Vec::new(),
                        );
                        let propagation_kind = propagation_kind_for_local_flow(
                            &source.anchor,
                            &target_binding.anchor,
                            PropagationKind::InitializerBinding,
                        );
                        let (confidence, risks) = combine_local_flow_risk(
                            target_binding.pointer_like,
                            source.pointer_like,
                            combined_risks,
                        );
                        events.push(PropagationEvent {
                            owner_symbol_id: Some(owner_symbol_id.to_string()),
                            source_anchor: source.anchor,
                            target_anchor: target_binding.anchor.clone(),
                            propagation_kind,
                            file_path: ctx.file_path.clone(),
                            line: child.start_position().row + 1,
                            confidence,
                            risks,
                        });
                    }
                }

                pending_bindings.push((name, target_binding));
            }
            kind if kind.contains("declarator") && kind != "function_declarator" => {
                let name_node = match extract_identifier_node(child) {
                    Some(node) => node,
                    None => continue,
                };
                let name = ctx.node_text(name_node);
                pending_bindings.push((
                    name.clone(),
                    make_local_binding(owner_symbol_id, &name, name_node.start_position().row + 1, child),
                ));
            }
            _ => {}
        }
    }

    if let Some(scope) = scopes.last_mut() {
        for (name, binding) in pending_bindings {
            scope.insert(name, binding);
        }
    }
}

fn extract_constructor_initializer_value(initializer: Node) -> Option<Node> {
    let mut cursor = initializer.walk();
    for child in initializer.named_children(&mut cursor) {
        match child.kind() {
            "argument_list" | "initializer_list" => {
                let mut value_cursor = child.walk();
                return child.named_children(&mut value_cursor).next();
            }
            _ => {}
        }
    }

    None
}

fn extract_expression_flow(
    node: Node,
    ctx: &Ctx,
    owner_symbol_id: &str,
    scopes: &mut Vec<HashMap<String, LocalBinding>>,
    events: &mut Vec<PropagationEvent>,
) -> Option<FlowAnchorResolution> {
    match node.kind() {
        "assignment_expression" => {
            let right = node.child_by_field_name("right")?;
            let left = node.child_by_field_name("left")?;
            let source = extract_expression_flow(right, ctx, owner_symbol_id, scopes, events)?;
            let target = resolve_assignment_target(left, ctx, owner_symbol_id, scopes)?;
            let combined_risks =
                merge_propagation_risks(source.risks.clone(), target.risks.clone());
            let propagation_kind = propagation_kind_for_local_flow(
                &source.anchor,
                &target.anchor,
                PropagationKind::Assignment,
            );
            let (confidence, risks) = combine_local_flow_risk(
                target.pointer_like,
                source.pointer_like,
                combined_risks,
            );
            events.push(PropagationEvent {
                owner_symbol_id: Some(owner_symbol_id.to_string()),
                source_anchor: source.anchor.clone(),
                target_anchor: target.anchor.clone(),
                propagation_kind,
                file_path: ctx.file_path.clone(),
                line: node.start_position().row + 1,
                confidence,
                risks: risks.clone(),
            });

            Some(FlowAnchorResolution {
                anchor: target.anchor,
                pointer_like: target.pointer_like,
                risks,
            })
        }
        "parenthesized_expression" => {
            let mut cursor = node.walk();
            let result = node.named_children(&mut cursor)
                .next()
                .and_then(|child| extract_expression_flow(child, ctx, owner_symbol_id, scopes, events));
            result
        }
        "identifier" => {
            let name = ctx.node_text(node);
            Some(resolve_identifier_anchor(&name, scopes).unwrap_or_else(|| FlowAnchorResolution {
                anchor: PropagationAnchor {
                    anchor_id: Some(format!(
                        "{}::expr:{}@{}",
                        owner_symbol_id,
                        name,
                        node.start_position().row + 1
                    )),
                    symbol_id: None,
                    expression_text: Some(name),
                    anchor_kind: PropagationAnchorKind::Expression,
                },
                pointer_like: false,
                risks: Vec::new(),
            }))
        }
        "field_expression" => resolve_field_expression_anchor(node, ctx, owner_symbol_id),
        _ => {
            let text = ctx.node_text(node);
            let pointer_like = expression_looks_pointer_heavy(node.kind(), &text);
            Some(FlowAnchorResolution {
                anchor: PropagationAnchor {
                    anchor_id: Some(format!(
                        "{}::expr:{}@{}",
                        owner_symbol_id,
                        sanitize_anchor_fragment(&text),
                        node.start_position().row + 1
                    )),
                    symbol_id: None,
                    expression_text: Some(text),
                    anchor_kind: PropagationAnchorKind::Expression,
                },
                pointer_like,
                risks: if pointer_like {
                    vec![PropagationRisk::PointerHeavyFlow]
                } else {
                    Vec::new()
                },
            })
        }
    }
}

fn resolve_assignment_target(
    node: Node,
    ctx: &Ctx,
    owner_symbol_id: &str,
    scopes: &[HashMap<String, LocalBinding>],
) -> Option<FlowAnchorResolution> {
    match node.kind() {
        "identifier" => {
            let name = ctx.node_text(node);
            resolve_identifier_anchor(&name, scopes)
        }
        "parenthesized_expression" => {
            let mut cursor = node.walk();
            let result = node.named_children(&mut cursor)
                .next()
                .and_then(|child| resolve_assignment_target(child, ctx, owner_symbol_id, scopes));
            result
        }
        "field_expression" => resolve_field_expression_anchor(node, ctx, owner_symbol_id),
        _ => None,
    }
}

fn propagation_kind_for_local_flow(
    source_anchor: &PropagationAnchor,
    target_anchor: &PropagationAnchor,
    default_kind: PropagationKind,
) -> PropagationKind {
    if target_anchor.anchor_kind == PropagationAnchorKind::Field {
        PropagationKind::FieldWrite
    } else if source_anchor.anchor_kind == PropagationAnchorKind::Field {
        PropagationKind::FieldRead
    } else {
        default_kind
    }
}

fn resolve_identifier_anchor(
    name: &str,
    scopes: &[HashMap<String, LocalBinding>],
) -> Option<FlowAnchorResolution> {
    for scope in scopes.iter().rev() {
        if let Some(binding) = scope.get(name) {
            return Some(FlowAnchorResolution {
                anchor: binding.anchor.clone(),
                pointer_like: binding.pointer_like,
                risks: if binding.pointer_like {
                    vec![PropagationRisk::PointerHeavyFlow]
                } else {
                    Vec::new()
                },
            });
        }
    }

    None
}

fn make_local_binding(
    owner_symbol_id: &str,
    name: &str,
    line: usize,
    declarator: Node,
) -> LocalBinding {
    LocalBinding {
        anchor: PropagationAnchor {
            anchor_id: Some(format!("{}::local:{}@{}", owner_symbol_id, name, line)),
            symbol_id: None,
            expression_text: Some(name.to_string()),
            anchor_kind: PropagationAnchorKind::LocalVariable,
        },
        pointer_like: is_pointer_like_declarator(declarator),
    }
}

fn combine_local_flow_risk(
    target_pointer_like: bool,
    source_pointer_like: bool,
    mut source_risks: Vec<PropagationRisk>,
) -> (RawExtractionConfidence, Vec<PropagationRisk>) {
    if target_pointer_like || source_pointer_like || !source_risks.is_empty() {
        if !source_risks.contains(&PropagationRisk::PointerHeavyFlow) {
            if target_pointer_like || source_pointer_like {
                source_risks.push(PropagationRisk::PointerHeavyFlow);
            }
        }
        (RawExtractionConfidence::Partial, source_risks)
    } else {
        (RawExtractionConfidence::High, source_risks)
    }
}

fn merge_propagation_risks(
    left: Vec<PropagationRisk>,
    right: Vec<PropagationRisk>,
) -> Vec<PropagationRisk> {
    let mut merged = left;
    for risk in right {
        if !merged.contains(&risk) {
            merged.push(risk);
        }
    }
    merged
}

fn resolve_field_expression_anchor(
    node: Node,
    ctx: &Ctx,
    owner_symbol_id: &str,
) -> Option<FlowAnchorResolution> {
    let receiver_node = node.child_by_field_name("argument")?;
    let field_node = node.child_by_field_name("field")?;
    let receiver_text = ctx.node_text(receiver_node);
    let field_name = ctx.node_text(field_node);
    let operator = field_expr_separator(node, ctx);
    let full_text = ctx.node_text(node);

    let mut risks = Vec::new();
    let (anchor_id, pointer_like) = if receiver_text == "this" {
        let owner_parent = owner_symbol_id
            .rsplit_once("::")
            .map(|(parent, _)| parent.to_string())
            .unwrap_or_else(|| owner_symbol_id.to_string());
        (format!("{}::field:{}", owner_parent, field_name), false)
    } else if operator == "->" {
        risks.push(PropagationRisk::PointerHeavyFlow);
        risks.push(PropagationRisk::ReceiverAmbiguity);
        (
            format!(
                "{}::fieldexpr:{}_{}@{}",
                owner_symbol_id,
                sanitize_anchor_fragment(&receiver_text),
                field_name,
                node.start_position().row + 1
            ),
            true,
        )
    } else {
        risks.push(PropagationRisk::ReceiverAmbiguity);
        (
            format!(
                "{}::fieldexpr:{}_{}@{}",
                owner_symbol_id,
                sanitize_anchor_fragment(&receiver_text),
                field_name,
                node.start_position().row + 1
            ),
            false,
        )
    };

    Some(FlowAnchorResolution {
        anchor: PropagationAnchor {
            anchor_id: Some(anchor_id),
            symbol_id: None,
            expression_text: Some(full_text),
            anchor_kind: PropagationAnchorKind::Field,
        },
        pointer_like,
        risks,
    })
}

fn resolve_constructor_initializer_target(
    initializer: Node,
    owner_symbol_id: &str,
    ctx: &Ctx,
) -> Option<FlowAnchorResolution> {
    let mut cursor = initializer.walk();
    for child in initializer.named_children(&mut cursor) {
        let field_name = match child.kind() {
            "field_identifier" => ctx.node_text(child),
            "qualified_identifier" => parse_qualified_id(child, ctx).1,
            _ => continue,
        };
        let owner_parent = owner_symbol_id
            .rsplit_once("::")
            .map(|(parent, _)| parent.to_string())
            .unwrap_or_else(|| owner_symbol_id.to_string());
        return Some(FlowAnchorResolution {
            anchor: PropagationAnchor {
                anchor_id: Some(format!("{}::field:{}", owner_parent, field_name)),
                symbol_id: None,
                expression_text: Some(field_name),
                anchor_kind: PropagationAnchorKind::Field,
            },
            pointer_like: false,
            risks: Vec::new(),
        });
    }

    None
}

fn declaration_contains_function_declarator(node: Node) -> bool {
    let mut cursor = node.walk();
    let contains = node.named_children(&mut cursor).any(|child| {
        child.kind() == "function_declarator"
            || find_descendant(child, "function_declarator").is_some()
    });
    contains
}

fn find_direct_child<'tree>(node: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
    let mut cursor = node.walk();
    let child = node
        .named_children(&mut cursor)
        .find(|child| child.kind() == kind);
    child
}

fn extract_identifier_node(node: Node) -> Option<Node> {
    if node.kind() == "identifier" {
        return Some(node);
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if let Some(identifier) = extract_identifier_node(child) {
            return Some(identifier);
        }
    }

    None
}

fn is_pointer_like_declarator(node: Node) -> bool {
    if matches!(node.kind(), "pointer_declarator" | "reference_declarator") {
        return true;
    }

    let mut cursor = node.walk();
    let pointer_like = node.named_children(&mut cursor).any(is_pointer_like_declarator);
    pointer_like
}

fn expression_looks_pointer_heavy(kind: &str, text: &str) -> bool {
    kind == "pointer_expression"
        || kind == "reference_expression"
        || text.contains("->")
        || text.starts_with('&')
        || text.starts_with('*')
}

fn sanitize_anchor_fragment(text: &str) -> String {
    let mut sanitized = String::new();
    for ch in text.chars() {
        if ch == '_' || ch.is_ascii_alphanumeric() {
            sanitized.push(ch);
        } else if !sanitized.ends_with('_') {
            sanitized.push('_');
        }
        if sanitized.len() >= 48 {
            break;
        }
    }
    if sanitized.is_empty() {
        "expr".to_string()
    } else {
        sanitized
    }
}

fn build_symbol_index(symbols: &[Symbol]) -> HashMap<String, Vec<Symbol>> {
    let mut index: HashMap<String, Vec<Symbol>> = HashMap::new();
    for symbol in symbols {
        index
            .entry(symbol.name.clone())
            .or_default()
            .push(symbol.clone());
        index
            .entry(symbol.id.clone())
            .or_default()
            .push(symbol.clone());
        if symbol.qualified_name != symbol.id {
            index
                .entry(symbol.qualified_name.clone())
                .or_default()
                .push(symbol.clone());
        }
    }
    index
}

fn resolve_enum_value_target_id(
    node: Node,
    event: &RawRelationEvent,
    source_symbol_id: &str,
    ctx: &Ctx,
    symbol_index: &HashMap<String, Vec<Symbol>>,
) -> Option<String> {
    let target_name = event.target_name.as_deref()?;
    let direct = enum_member_candidates_for_lookup(target_name, symbol_index);
    if direct.is_empty() {
        return None;
    }
    if direct.len() == 1 {
        return Some(direct[0].id.clone());
    }

    let preferred_enum_ids = collect_enum_parent_hints(node, source_symbol_id, ctx, symbol_index);
    let source_namespace = namespace_scope(source_symbol_id);
    let explicit_namespace = enum_member_explicit_namespace(node, ctx);
    let mut scored: Vec<(i32, &Symbol)> = direct
        .iter()
        .map(|symbol| {
            let symbol = *symbol;
            let mut score = 0;
            if let Some(parent_id) = symbol.parent_id.as_deref() {
                if preferred_enum_ids.contains(parent_id) {
                    score += 250;
                }
            }
            if let Some(namespace) = explicit_namespace.as_deref() {
                if enum_member_namespace(symbol) == Some(namespace) {
                    score += 180;
                }
            }
            if let Some(namespace) = source_namespace {
                if symbol_namespace(symbol) == Some(namespace) {
                    score += 50;
                }
            }
            (score, symbol)
        })
        .collect();
    scored.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.id.cmp(&right.1.id)));

    let best = scored.first()?;
    let best_score = best.0;
    if best_score == 0 {
        return None;
    }
    if scored
        .iter()
        .take_while(|candidate| candidate.0 == best_score)
        .count()
        > 1
    {
        return None;
    }

    Some(best.1.id.clone())
}

fn enum_member_candidates_for_lookup<'a>(
    target_name: &str,
    symbol_index: &'a HashMap<String, Vec<Symbol>>,
) -> Vec<&'a Symbol> {
    let mut candidates = Vec::new();
    let mut seen = HashSet::new();

    if let Some(direct) = symbol_index.get(target_name) {
        for symbol in direct {
            if is_enum_member_symbol(symbol) && seen.insert(symbol.id.clone()) {
                candidates.push(symbol);
            }
        }
    }

    if let Some((_, leaf_name)) = target_name.rsplit_once("::") {
        if let Some(by_leaf) = symbol_index.get(leaf_name) {
            for symbol in by_leaf {
                if is_enum_member_symbol(symbol) && seen.insert(symbol.id.clone()) {
                    candidates.push(symbol);
                }
            }
        }
    }

    candidates
}

fn enum_member_explicit_namespace(node: Node, ctx: &Ctx) -> Option<String> {
    if node.kind() != "qualified_identifier" {
        return None;
    }

    let text = ctx.node_text(node);
    let (qualifier, _) = text.rsplit_once("::")?;
    Some(qualifier.to_string())
}

fn enum_member_namespace<'a>(symbol: &'a Symbol) -> Option<&'a str> {
    symbol.parent_id.as_deref().and_then(namespace_scope)
}

fn collect_enum_parent_hints(
    node: Node,
    source_symbol_id: &str,
    ctx: &Ctx,
    symbol_index: &HashMap<String, Vec<Symbol>>,
) -> HashSet<String> {
    let mut hints = HashSet::new();

    if node.kind() == "qualified_identifier" {
        let text = ctx.node_text(node);
        if let Some((qualifier, _)) = text.rsplit_once("::") {
            for enum_id in resolve_enum_ids_for_type_text(qualifier, symbol_index) {
                hints.insert(enum_id);
            }
        }
    }

    let mut current = node.parent();
    while let Some(parent) = current {
        match parent.kind() {
            "init_declarator" => {
                hints.extend(enum_hints_from_typed_declaration(parent, ctx, symbol_index));
            }
            "argument_list" => {
                hints.extend(enum_hints_from_argument_list(parent, node, ctx, symbol_index));
            }
            "return_statement" => {
                hints.extend(enum_hints_from_return_context(source_symbol_id, symbol_index));
            }
            _ => {}
        }
        if !hints.is_empty() {
            break;
        }
        current = parent.parent();
    }

    hints
}

fn enum_hints_from_typed_declaration(
    init_declarator: Node,
    ctx: &Ctx,
    symbol_index: &HashMap<String, Vec<Symbol>>,
) -> HashSet<String> {
    let mut hints = HashSet::new();
    let Some(declaration) = init_declarator.parent() else {
        return hints;
    };
    let mut cursor = declaration.walk();
    for child in declaration.children(&mut cursor) {
        match child.kind() {
            "type_identifier" | "qualified_identifier" | "enum_specifier" => {
                let type_text = declaration_type_text(child, ctx);
                hints.extend(resolve_enum_ids_for_type_text(&type_text, symbol_index));
            }
            kind if kind.contains("declarator") || kind == "init_declarator" => break,
            _ => {}
        }
    }
    hints
}

fn declaration_type_text(node: Node, ctx: &Ctx) -> String {
    if node.kind() != "enum_specifier" {
        return ctx.node_text(node);
    }
    node.child_by_field_name("name")
        .map(|name| ctx.node_text(name))
        .unwrap_or_default()
}

fn enum_hints_from_argument_list(
    argument_list: Node,
    target_node: Node,
    ctx: &Ctx,
    symbol_index: &HashMap<String, Vec<Symbol>>,
) -> HashSet<String> {
    let mut hints = HashSet::new();
    let Some(call_expression) = argument_list.parent() else {
        return hints;
    };
    let Some(function_node) = call_expression.child_by_field_name("function") else {
        return hints;
    };
    let Some(called_name) = callable_leaf_name(function_node, ctx) else {
        return hints;
    };
    let Some(argument_index) = argument_index(argument_list, target_node) else {
        return hints;
    };

    let callable_symbols = symbol_index
        .get(called_name.as_str())
        .into_iter()
        .flat_map(|symbols| symbols.iter())
        .filter(|symbol| matches!(symbol.symbol_type.as_str(), "function" | "method"));

    for symbol in callable_symbols {
        let Some(parameter_type) =
            signature_parameter_type(symbol.signature.as_deref().unwrap_or_default(), argument_index)
        else {
            continue;
        };
        hints.extend(resolve_enum_ids_for_type_text(&parameter_type, symbol_index));
    }

    hints
}

fn callable_leaf_name(node: Node, ctx: &Ctx) -> Option<String> {
    match node.kind() {
        "identifier" | "field_identifier" => Some(ctx.node_text(node)),
        "qualified_identifier" => ctx
            .node_text(node)
            .rsplit("::")
            .next()
            .map(|name| name.to_string()),
        "field_expression" => node
            .child_by_field_name("field")
            .map(|field| ctx.node_text(field)),
        _ => None,
    }
}

fn argument_index(argument_list: Node, target_node: Node) -> Option<usize> {
    let mut argument_node = target_node;
    while let Some(parent) = argument_node.parent() {
        if parent == argument_list {
            break;
        }
        argument_node = parent;
    }

    let mut cursor = argument_list.walk();
    let mut index = 0;
    for child in argument_list.named_children(&mut cursor) {
        if child == argument_node {
            return Some(index);
        }
        index += 1;
    }
    None
}

fn signature_parameter_type(signature: &str, argument_index: usize) -> Option<String> {
    let start = signature.find('(')?;
    let end = signature.rfind(')')?;
    if end <= start {
        return None;
    }
    let params = &signature[start + 1..end];
    let parameters = split_top_level_commas(params);
    let parameter = parameters.get(argument_index)?.trim();
    if parameter.is_empty() {
        return None;
    }
    let without_default = parameter.split('=').next()?.trim();
    let tokens: Vec<&str> = without_default.split_whitespace().collect();
    if tokens.is_empty() {
        return None;
    }
    if tokens.len() == 1 {
        return Some(clean_type_token(tokens[0]));
    }
    Some(clean_type_token(&tokens[..tokens.len() - 1].join(" ")))
}

fn split_top_level_commas(text: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut angle_depth = 0i32;
    let mut paren_depth = 0i32;

    for ch in text.chars() {
        match ch {
            '<' => angle_depth += 1,
            '>' if angle_depth > 0 => angle_depth -= 1,
            '(' => paren_depth += 1,
            ')' if paren_depth > 0 => paren_depth -= 1,
            ',' if angle_depth == 0 && paren_depth == 0 => {
                parts.push(current.trim().to_string());
                current.clear();
                continue;
            }
            _ => {}
        }
        current.push(ch);
    }

    if !current.trim().is_empty() {
        parts.push(current.trim().to_string());
    }

    parts
}

fn clean_type_token(text: &str) -> String {
    text.replace('&', "")
        .replace('*', "")
        .replace("const ", "")
        .replace(" const", "")
        .trim()
        .to_string()
}

fn enum_hints_from_return_context(
    source_symbol_id: &str,
    symbol_index: &HashMap<String, Vec<Symbol>>,
) -> HashSet<String> {
    let mut hints = HashSet::new();
    let Some(symbol) = symbol_index
        .get(source_symbol_id)
        .and_then(|symbols| symbols.first())
    else {
        return hints;
    };
    let Some(signature) = symbol.signature.as_deref() else {
        return hints;
    };
    let Some(paren_index) = signature.find('(') else {
        return hints;
    };
    let prefix = signature[..paren_index].trim();
    let mut parts: Vec<&str> = prefix.split_whitespace().collect();
    if parts.len() < 2 {
        return hints;
    }
    parts.pop();
    let return_type = clean_type_token(&parts.join(" "));
    hints.extend(resolve_enum_ids_for_type_text(&return_type, symbol_index));
    hints
}

fn resolve_enum_ids_for_type_text(
    type_text: &str,
    symbol_index: &HashMap<String, Vec<Symbol>>,
) -> HashSet<String> {
    let mut hints = HashSet::new();
    for token in extract_type_lookup_tokens(type_text) {
        if let Some(symbols) = symbol_index.get(token.as_str()) {
            for symbol in symbols {
                if symbol.symbol_type == "enum" {
                    hints.insert(symbol.id.clone());
                }
            }
        }
    }
    hints
}

fn extract_type_lookup_tokens(type_text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    for ch in type_text.chars() {
        if ch.is_alphanumeric() || ch == '_' || ch == ':' {
            current.push(ch);
        } else if !current.is_empty() {
            tokens.push(current.clone());
            current.clear();
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }

    let mut expanded = Vec::new();
    for token in tokens {
        expanded.push(token.clone());
        if let Some((_, leaf)) = token.rsplit_once("::") {
            expanded.push(leaf.to_string());
        }
    }
    expanded.sort();
    expanded.dedup();
    expanded
}

fn resolve_reference_target_id(
    event: &RawRelationEvent,
    source_symbol_id: &str,
    symbol_index: &HashMap<String, Vec<Symbol>>,
) -> Option<String> {
    let target_name = event.target_name.as_deref()?;
    let direct = reference_candidates_for_lookup(event, target_name, symbol_index);
    let direct: Vec<&Symbol> = direct
        .into_iter()
        .filter(|symbol| reference_target_allowed(event, symbol))
        .collect();
    if direct.is_empty() {
        return None;
    }
    if direct.len() == 1 {
        return Some(direct[0].id.clone());
    }

    let source_namespace = namespace_scope(source_symbol_id);
    let target_namespace = target_name.rsplit_once("::").map(|(namespace, _)| namespace);
    let mut scored: Vec<(i32, &Symbol)> = direct
        .iter()
        .map(|symbol| {
            let symbol = *symbol;
            let mut score = 0;
            if symbol.id == target_name || symbol.qualified_name == target_name {
                score += 100;
            }
            if let Some(namespace) = source_namespace {
                if symbol_namespace(symbol) == Some(namespace) {
                    score += 50;
                }
            }
            if let Some(namespace) = target_namespace {
                if symbol_namespace(symbol) == Some(namespace)
                    || enum_member_namespace(symbol) == Some(namespace)
                {
                    score += 120;
                }
            }
            (score, symbol)
        })
        .collect();
    scored.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.id.cmp(&right.1.id)));

    let best = scored.first()?;
    let best_score = best.0;
    if best_score == 0 {
        return None;
    }
    if scored.iter().take_while(|candidate| candidate.0 == best_score).count() > 1 {
        return None;
    }

    Some(best.1.id.clone())
}

fn reference_candidates_for_lookup<'a>(
    event: &RawRelationEvent,
    target_name: &str,
    symbol_index: &'a HashMap<String, Vec<Symbol>>,
) -> Vec<&'a Symbol> {
    if event.relation_kind == RawRelationKind::EnumValueUsage {
        return enum_member_candidates_for_lookup(target_name, symbol_index);
    }

    symbol_index
        .get(target_name)
        .map(|direct| direct.iter().collect())
        .unwrap_or_default()
}

fn reference_target_allowed(event: &RawRelationEvent, symbol: &Symbol) -> bool {
    match event.relation_kind {
        RawRelationKind::Inheritance => is_type_like_symbol(symbol),
        RawRelationKind::EnumValueUsage => is_enum_member_symbol(symbol),
        RawRelationKind::TypeUsage | RawRelationKind::Call => true,
    }
}

fn is_type_like_symbol(symbol: &Symbol) -> bool {
    matches!(symbol.symbol_type.as_str(), "class" | "struct" | "enum")
}

fn is_enum_member_symbol(symbol: &Symbol) -> bool {
    symbol.symbol_type == "enumMember"
}

fn symbol_namespace<'a>(symbol: &'a Symbol) -> Option<&'a str> {
    namespace_scope(symbol.id.as_str())
}

fn namespace_scope(qualified_id: &str) -> Option<&str> {
    qualified_id.rsplit_once("::").map(|(namespace, _)| namespace)
}

fn read_attr_string(
    attrs: &tree_sitter_graph::graph::Attributes,
    name: &str,
) -> Option<String> {
    attrs.get(name).and_then(|value| match value {
        GraphValue::String(value) => Some(value.clone()),
        _ => None,
    })
}

fn read_attr_str<'a>(
    attrs: &'a tree_sitter_graph::graph::Attributes,
    name: &str,
) -> Option<&'a str> {
    attrs.get(name).and_then(|value| match value {
        GraphValue::String(value) => Some(value.as_str()),
        _ => None,
    })
}

fn read_attr_u32(attrs: &tree_sitter_graph::graph::Attributes, name: &str) -> Option<u32> {
    attrs.get(name).and_then(|value| match value {
        GraphValue::Integer(value) => Some(*value),
        _ => None,
    })
}

fn parse_call_kind(value: &str) -> Option<RawCallKind> {
    match value {
        "unqualified" => Some(RawCallKind::Unqualified),
        "member_access" => Some(RawCallKind::MemberAccess),
        "pointer_member_access" => Some(RawCallKind::PointerMemberAccess),
        "this_pointer_access" => Some(RawCallKind::ThisPointerAccess),
        "qualified" => Some(RawCallKind::Qualified),
        "field_access" => Some(RawCallKind::FieldAccess),
        "pointer_field_access" => Some(RawCallKind::PointerFieldAccess),
        "this_field_access" => Some(RawCallKind::ThisFieldAccess),
        _ => None,
    }
}

fn graph_matches_legacy_calls(graph_calls: &[RawCallSite], legacy_calls: &[RawCallSite]) -> bool {
    if graph_calls.len() != legacy_calls.len() || graph_calls.is_empty() {
        return false;
    }

    let mut graph_keys: Vec<String> = graph_calls.iter().map(raw_call_comparison_key).collect();
    let mut legacy_keys: Vec<String> = legacy_calls.iter().map(raw_call_comparison_key).collect();
    graph_keys.sort();
    legacy_keys.sort();
    graph_keys == legacy_keys
}

fn raw_call_comparison_key(raw_call: &RawCallSite) -> String {
    format!(
        "{}|{}|{}|{}|{}|{}|{}|{}",
        raw_call.caller_id,
        raw_call.called_name,
        raw_call.line,
        raw_call_kind_key(&raw_call.call_kind),
        raw_call.argument_count.map(|value| value.to_string()).unwrap_or_default(),
        raw_call.receiver.as_deref().unwrap_or_default(),
        raw_call.receiver_kind
            .as_ref()
            .map(raw_receiver_kind_key)
            .unwrap_or_default(),
        raw_call.qualifier.as_deref().unwrap_or_default(),
    )
}

#[cfg(test)]
fn raw_call_comparison_keys(raw_calls: &[RawCallSite]) -> Vec<String> {
    let mut keys: Vec<String> = raw_calls.iter().map(raw_call_comparison_key).collect();
    keys.sort();
    keys
}

fn raw_call_kind_key(kind: &RawCallKind) -> &'static str {
    match kind {
        RawCallKind::Unqualified => "unqualified",
        RawCallKind::MemberAccess => "member_access",
        RawCallKind::PointerMemberAccess => "pointer_member_access",
        RawCallKind::ThisPointerAccess => "this_pointer_access",
        RawCallKind::Qualified => "qualified",
        RawCallKind::FieldAccess => "field_access",
        RawCallKind::PointerFieldAccess => "pointer_field_access",
        RawCallKind::ThisFieldAccess => "this_field_access",
    }
}

fn raw_receiver_kind_key(kind: &RawReceiverKind) -> &'static str {
    match kind {
        RawReceiverKind::Identifier => "identifier",
        RawReceiverKind::This => "this",
        RawReceiverKind::PointerExpression => "pointer_expression",
        RawReceiverKind::FieldExpression => "field_expression",
        RawReceiverKind::QualifiedIdentifier => "qualified_identifier",
        RawReceiverKind::Other => "other",
    }
}

fn read_enclosing_caller_id(_root: Node, line: usize, ctx: &Ctx) -> Option<String> {
    if line == 0 {
        return None;
    }

    ctx.symbols
        .iter()
        .filter(|symbol| {
            (symbol.symbol_type == "function" || symbol.symbol_type == "method")
                && matches!(
                    symbol.symbol_role.as_deref(),
                    Some("definition") | Some("inline_definition")
                )
                && symbol.line <= line
                && line <= symbol.end_line
        })
        .min_by_key(|symbol| symbol.end_line.saturating_sub(symbol.line))
        .map(|symbol| symbol.id.clone())
}

fn visit_tree(root: Node, ctx: &mut Ctx) {
    let mut stack = vec![WalkItem::Enter(root)];

    while let Some(item) = stack.pop() {
        match item {
            WalkItem::Enter(node) => match node.kind() {
                "namespace_definition" => enter_namespace(node, ctx, &mut stack),
                "class_specifier" => enter_class(node, ctx, "class", &mut stack),
                "struct_specifier" => enter_class(node, ctx, "struct", &mut stack),
                "enum_specifier" => visit_enum(node, ctx),
                "function_definition" => visit_function_definition(node, ctx),
                "declaration" => visit_declaration(node, ctx),
                "template_declaration" => enter_template_declaration(node, &mut stack),
                _ => push_children(node, &mut stack),
            },
            WalkItem::EnterClassMember(node, parent_id) => match node.kind() {
                "field_declaration" => visit_field_declaration(node, ctx, &parent_id),
                "function_definition" => visit_inline_method(node, ctx, &parent_id),
                "declaration" => visit_member_declaration(node, ctx, &parent_id),
                "class_specifier" => enter_class(node, ctx, "class", &mut stack),
                "struct_specifier" => enter_class(node, ctx, "struct", &mut stack),
                "template_declaration" => enter_template_declaration_in_class(node, parent_id, &mut stack),
                _ => {}
            },
            WalkItem::ExitNamespace | WalkItem::ExitClass => {
                ctx.ns_stack.pop();
            }
        }
    }
}

fn enter_namespace<'a>(node: Node<'a>, ctx: &mut Ctx, stack: &mut Vec<WalkItem<'a>>) {
    let name_node = node.child_by_field_name("name");
    let body_node = node.child_by_field_name("body");

    if let (Some(name), Some(body)) = (name_node, body_node) {
        let ns_name = ctx.node_text(name);
        let ns_id = ctx.qualify(&ns_name);
        ctx.namespace_ids.insert(ns_id);
        ctx.ns_stack.push(ns_name);
        stack.push(WalkItem::ExitNamespace);
        push_children(body, stack);
    }
}

fn enter_class<'a>(node: Node<'a>, ctx: &mut Ctx, kind: &str, stack: &mut Vec<WalkItem<'a>>) {
    let name_node = node.child_by_field_name("name");
    let name = match name_node {
        Some(n) => ctx.node_text(n),
        None => return,
    };

    let class_id = ctx.qualify(&name);
    ctx.symbols.push(Symbol {
        id: class_id.clone(),
        name: name.clone(),
        qualified_name: class_id.clone(),
        symbol_type: kind.to_string(),
        file_path: ctx.file_path.clone(),
        line: node.start_position().row + 1,
        end_line: node.end_position().row + 1,
        signature: None,
        parameter_count: None,
        scope_qualified_name: None,
        scope_kind: None,
        symbol_role: None,
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
        artifact_kind: None,
        header_role: None,
        parse_fragility: None,
        macro_sensitivity: None,
        include_heaviness: None,
    });

    let body = match node.child_by_field_name("body") {
        Some(b) => b,
        None => return,
    };

    ctx.ns_stack.push(name);
    stack.push(WalkItem::ExitClass);
    push_class_body(body, &class_id, stack);
}

fn push_children<'a>(node: Node<'a>, stack: &mut Vec<WalkItem<'a>>) {
    let mut cursor = node.walk();
    let children: Vec<Node<'a>> = node.children(&mut cursor).collect();
    for child in children.into_iter().rev() {
        stack.push(WalkItem::Enter(child));
    }
}

fn push_class_body<'a>(body: Node<'a>, parent_id: &str, stack: &mut Vec<WalkItem<'a>>) {
    let mut cursor = body.walk();
    let children: Vec<Node<'a>> = body.children(&mut cursor).collect();
    for child in children.into_iter().rev() {
        match child.kind() {
            "field_declaration"
            | "function_definition"
            | "declaration"
            | "class_specifier"
            | "struct_specifier"
            | "template_declaration" => {
                stack.push(WalkItem::EnterClassMember(child, parent_id.to_string()))
            }
            _ => {}
        }
    }
}

fn visit_field_declaration(node: Node, ctx: &mut Ctx, parent_id: &str) {
    let func_decl = match find_child(node, "function_declarator") {
        Some(fd) => fd,
        None => return,
    };
    let name_node = match func_decl.child_by_field_name("declarator") {
        Some(n) => n,
        None => return,
    };
    let method_name = ctx.node_text(name_node);
    let method_id = ctx.qualify(&method_name);
    let sig = build_decl_signature(node, ctx);
    let parameter_count = count_parameters(func_decl, ctx);

    ctx.symbols.push(Symbol {
        id: method_id.clone(),
        name: method_name,
        qualified_name: method_id,
        symbol_type: "method".to_string(),
        file_path: ctx.file_path.clone(),
        line: node.start_position().row + 1,
        end_line: node.end_position().row + 1,
        signature: Some(sig),
        parameter_count,
        scope_qualified_name: None,
        scope_kind: None,
        symbol_role: Some("declaration".to_string()),
        declaration_file_path: None,
        declaration_line: None,
        declaration_end_line: None,
        definition_file_path: None,
        definition_line: None,
        definition_end_line: None,
        parent_id: Some(parent_id.to_string()),
        module: None,
        subsystem: None,
        project_area: None,
        artifact_kind: None,
        header_role: None,
        parse_fragility: None,
        macro_sensitivity: None,
        include_heaviness: None,
    });
}

fn visit_member_declaration(node: Node, ctx: &mut Ctx, parent_id: &str) {
    let func_decl = match find_descendant(node, "function_declarator") {
        Some(fd) => fd,
        None => return,
    };
    let name_node = match func_decl.child_by_field_name("declarator") {
        Some(n) => n,
        None => return,
    };
    let name = ctx.node_text(name_node);
    if name.contains("::") {
        return;
    }
    let method_id = ctx.qualify(&name);
    let sig = build_decl_signature(node, ctx);
    let parameter_count = count_parameters(func_decl, ctx);

    ctx.symbols.push(Symbol {
        id: method_id.clone(),
        name,
        qualified_name: method_id,
        symbol_type: "method".to_string(),
        file_path: ctx.file_path.clone(),
        line: node.start_position().row + 1,
        end_line: node.end_position().row + 1,
        signature: Some(sig),
        parameter_count,
        scope_qualified_name: None,
        scope_kind: None,
        symbol_role: Some("declaration".to_string()),
        declaration_file_path: None,
        declaration_line: None,
        declaration_end_line: None,
        definition_file_path: None,
        definition_line: None,
        definition_end_line: None,
        parent_id: Some(parent_id.to_string()),
        module: None,
        subsystem: None,
        project_area: None,
        artifact_kind: None,
        header_role: None,
        parse_fragility: None,
        macro_sensitivity: None,
        include_heaviness: None,
    });
}

fn visit_inline_method(node: Node, ctx: &mut Ctx, parent_id: &str) {
    let decl_node = match node.child_by_field_name("declarator") {
        Some(d) => d,
        None => return,
    };
    let name_node = match decl_node.child_by_field_name("declarator") {
        Some(n) => n,
        None => return,
    };
    let method_name = ctx.node_text(name_node);
    let method_id = ctx.qualify(&method_name);
    let sig = build_func_signature(node, ctx);
    let parameter_count = count_parameters(decl_node, ctx);

    ctx.symbols.push(Symbol {
        id: method_id.clone(),
        name: method_name,
        qualified_name: method_id.clone(),
        symbol_type: "method".to_string(),
        file_path: ctx.file_path.clone(),
        line: node.start_position().row + 1,
        end_line: node.end_position().row + 1,
        signature: Some(sig),
        parameter_count,
        scope_qualified_name: None,
        scope_kind: None,
        symbol_role: Some("inline_definition".to_string()),
        declaration_file_path: None,
        declaration_line: None,
        declaration_end_line: None,
        definition_file_path: None,
        definition_line: None,
        definition_end_line: None,
        parent_id: Some(parent_id.to_string()),
        module: None,
        subsystem: None,
        project_area: None,
        artifact_kind: None,
        header_role: None,
        parse_fragility: None,
        macro_sensitivity: None,
        include_heaviness: None,
    });

    if let Some(body) = node.child_by_field_name("body") {
        extract_call_sites(body, &method_id, ctx);
    }
}

fn visit_enum(node: Node, ctx: &mut Ctx) {
    let name_node = match node.child_by_field_name("name") {
        Some(n) => n,
        None => return,
    };
    let name = ctx.node_text(name_node);
    let enum_id = ctx.qualify(&name);
    let line = node.start_position().row + 1;
    let end_line = node.end_position().row + 1;

    ctx.symbols.push(Symbol {
        id: enum_id.clone(),
        name,
        qualified_name: enum_id.clone(),
        symbol_type: "enum".to_string(),
        file_path: ctx.file_path.clone(),
        line,
        end_line,
        signature: None,
        parameter_count: None,
        scope_qualified_name: None,
        scope_kind: None,
        symbol_role: None,
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
        artifact_kind: None,
        header_role: None,
        parse_fragility: None,
        macro_sensitivity: None,
        include_heaviness: None,
    });

    if let Some(body) = find_child(node, "enumerator_list") {
        for enumerator in named_children(body) {
            if enumerator.kind() != "enumerator" {
                continue;
            }
            let Some(member_name_node) = enumerator.child_by_field_name("name") else {
                continue;
            };
            let member_name = ctx.node_text(member_name_node);
            let member_id = format!("{}::{}", enum_id, member_name);
            ctx.symbols.push(Symbol {
                id: member_id.clone(),
                name: member_name,
                qualified_name: member_id,
                symbol_type: "enumMember".to_string(),
                file_path: ctx.file_path.clone(),
                line: enumerator.start_position().row + 1,
                end_line: enumerator.end_position().row + 1,
                signature: None,
                parameter_count: None,
                scope_qualified_name: None,
                scope_kind: None,
                symbol_role: None,
                declaration_file_path: None,
                declaration_line: None,
                declaration_end_line: None,
                definition_file_path: None,
                definition_line: None,
                definition_end_line: None,
                parent_id: Some(enum_id.clone()),
                module: None,
                subsystem: None,
                project_area: None,
                artifact_kind: None,
                header_role: None,
                parse_fragility: None,
                macro_sensitivity: None,
                include_heaviness: None,
            });
        }
    }
}

fn visit_function_definition(node: Node, ctx: &mut Ctx) {
    let decl_node = match node.child_by_field_name("declarator") {
        Some(d) => d,
        None => return,
    };

    let func_decl = if decl_node.kind() == "function_declarator" {
        decl_node
    } else {
        match find_descendant(decl_node, "function_declarator") {
            Some(fd) => fd,
            None => return,
        }
    };

    let name_node = match func_decl.child_by_field_name("declarator") {
        Some(n) => n,
        None => return,
    };

    let (func_name, func_id, parent_id) = if name_node.kind() == "qualified_identifier" {
        let (class_name, member_name) = parse_qualified_id(name_node, ctx);
        if !class_name.is_empty() {
            let class_id = ctx.qualify(&class_name);
            let full_id = format!("{}::{}", class_id, member_name);
            (member_name, full_id, Some(class_id))
        } else {
            let text = ctx.node_text(name_node);
            let id = ctx.qualify(&text);
            (text, id, None)
        }
    } else if name_node.kind() == "destructor_name" {
        let text = ctx.node_text(name_node);
        let id = ctx.qualify(&text);
        (text, id, None)
    } else {
        let text = ctx.node_text(name_node);
        let id = ctx.qualify(&text);
        (text, id, None)
    };

    let sig = build_func_signature(node, ctx);
    let parameter_count = count_parameters(func_decl, ctx);
    let sym_type = if parent_id.is_some() { "method" } else { "function" };

    ctx.symbols.push(Symbol {
        id: func_id.clone(),
        name: func_name,
        qualified_name: func_id.clone(),
        symbol_type: sym_type.to_string(),
        file_path: ctx.file_path.clone(),
        line: node.start_position().row + 1,
        end_line: node.end_position().row + 1,
        signature: Some(sig),
        parameter_count,
        scope_qualified_name: None,
        scope_kind: None,
        symbol_role: Some("definition".to_string()),
        declaration_file_path: None,
        declaration_line: None,
        declaration_end_line: None,
        definition_file_path: None,
        definition_line: None,
        definition_end_line: None,
        parent_id,
        module: None,
        subsystem: None,
        project_area: None,
        artifact_kind: None,
        header_role: None,
        parse_fragility: None,
        macro_sensitivity: None,
        include_heaviness: None,
    });

    if let Some(body) = node.child_by_field_name("body") {
        extract_call_sites(body, &func_id, ctx);
    }
}

fn visit_declaration(node: Node, ctx: &mut Ctx) {
    let func_decl = match find_descendant(node, "function_declarator") {
        Some(fd) => fd,
        None => return,
    };
    let name_node = match func_decl.child_by_field_name("declarator") {
        Some(n) => n,
        None => return,
    };
    let name = ctx.node_text(name_node);
    if name.contains("::") {
        return;
    }
    let func_id = ctx.qualify(&name);
    if ctx.symbols.iter().any(|s| s.id == func_id) {
        return;
    }
    let sig = build_decl_signature(node, ctx);
    let parameter_count = count_parameters(func_decl, ctx);

    ctx.symbols.push(Symbol {
        id: func_id.clone(),
        name,
        qualified_name: func_id,
        symbol_type: "function".to_string(),
        file_path: ctx.file_path.clone(),
        line: node.start_position().row + 1,
        end_line: node.end_position().row + 1,
        signature: Some(sig),
        parameter_count,
        scope_qualified_name: None,
        scope_kind: None,
        symbol_role: Some("declaration".to_string()),
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
        artifact_kind: None,
        header_role: None,
        parse_fragility: None,
        macro_sensitivity: None,
        include_heaviness: None,
    });
}

fn enter_template_declaration<'a>(node: Node<'a>, stack: &mut Vec<WalkItem<'a>>) {
    let mut cursor = node.walk();
    let children: Vec<Node<'a>> = node.children(&mut cursor).collect();
    for child in children.into_iter().rev() {
        match child.kind() {
            "class_specifier" | "struct_specifier" | "function_definition" | "declaration" => {
                stack.push(WalkItem::Enter(child));
            }
            _ => {}
        }
    }
}

fn enter_template_declaration_in_class<'a>(
    node: Node<'a>,
    parent_id: String,
    stack: &mut Vec<WalkItem<'a>>,
) {
    let mut cursor = node.walk();
    let children: Vec<Node<'a>> = node.children(&mut cursor).collect();
    for child in children.into_iter().rev() {
        match child.kind() {
            "function_definition"
            | "field_declaration"
            | "declaration"
            | "class_specifier"
            | "struct_specifier"
            | "template_declaration" => {
                stack.push(WalkItem::EnterClassMember(child, parent_id.clone()));
            }
            _ => {}
        }
    }
}

fn extract_call_sites(node: Node, caller_id: &str, ctx: &mut Ctx) {
    let mut stack = vec![node];

    while let Some(current) = stack.pop() {
        if current.kind() == "call_expression" {
            if let Some(func_node) = current.child_by_field_name("function") {
                if let Some(raw_call) = parse_call_expr(func_node, caller_id, current, ctx) {
                    ctx.raw_calls.push(RawCallSite {
                        file_path: ctx.file_path.clone(),
                        line: current.start_position().row + 1,
                        ..raw_call
                    });
                }
            }
        }

        let mut cursor = current.walk();
        let children: Vec<Node> = current.children(&mut cursor).collect();
        for child in children.into_iter().rev() {
            stack.push(child);
        }
    }
}

fn parse_call_expr(func_node: Node, caller_id: &str, call_node: Node, ctx: &Ctx) -> Option<RawCallSite> {
    let argument_count = call_node
        .child_by_field_name("arguments")
        .map(|args| count_arguments(args, ctx));
    let argument_texts = extract_argument_texts(call_node, ctx);
    let result_target = extract_call_result_target(call_node, caller_id, ctx);

    match func_node.kind() {
        "identifier" => Some(RawCallSite {
            caller_id: caller_id.to_string(),
            called_name: ctx.node_text(func_node),
            call_kind: RawCallKind::Unqualified,
            argument_count,
            argument_texts,
            result_target,
            receiver: None,
            receiver_kind: None,
            qualifier: None,
            qualifier_kind: None,
            file_path: String::new(),
            line: call_node.start_position().row + 1,
        }),
        "field_expression" => {
            let field = func_node.child_by_field_name("field")?;
            let arg = func_node.child_by_field_name("argument");
            let receiver = arg.map(|a| extract_receiver(a, ctx));
            let receiver_kind = arg.map(detect_receiver_kind);
            let separator = field_expr_separator(func_node, ctx);
            let call_kind = match (receiver.as_deref(), separator.as_str()) {
                (Some("this"), "->") => RawCallKind::ThisPointerAccess,
                (_, "->") => RawCallKind::PointerMemberAccess,
                _ => RawCallKind::MemberAccess,
            };
            Some(RawCallSite {
                caller_id: caller_id.to_string(),
                called_name: ctx.node_text(field),
                call_kind,
                argument_count,
                argument_texts,
                result_target,
                receiver,
                receiver_kind,
                qualifier: None,
                qualifier_kind: None,
                file_path: String::new(),
                line: call_node.start_position().row + 1,
            })
        }
        "qualified_identifier" => {
            let count = func_node.named_child_count();
            let last = func_node.named_child(count.checked_sub(1)?)?;
            let mut parts = Vec::new();
            for i in 0..count.saturating_sub(1) {
                if let Some(child) = func_node.named_child(i) {
                    parts.push(ctx.node_text(child));
                }
            }
            let qualifier = if parts.is_empty() {
                None
            } else {
                Some(parts.join("::"))
            };
            let qualifier_kind = qualifier
                .as_deref()
                .and_then(|value| classify_qualifier_kind(value, ctx));
            Some(RawCallSite {
                caller_id: caller_id.to_string(),
                called_name: ctx.node_text(last),
                call_kind: RawCallKind::Qualified,
                argument_count,
                argument_texts,
                result_target,
                receiver: None,
                receiver_kind: None,
                qualifier,
                qualifier_kind,
                file_path: String::new(),
                line: call_node.start_position().row + 1,
            })
        }
        _ => None,
    }
}

fn extract_argument_texts(call_node: Node, ctx: &Ctx) -> Vec<String> {
    let Some(arguments) = call_node.child_by_field_name("arguments") else {
        return Vec::new();
    };

    named_children(arguments)
        .into_iter()
        .map(|child| ctx.node_text(child))
        .collect()
}

fn extract_call_result_target(
    call_node: Node,
    caller_id: &str,
    ctx: &Ctx,
) -> Option<PropagationAnchor> {
    let parent = call_node.parent()?;
    match parent.kind() {
        "assignment_expression" => {
            let left = parent.child_by_field_name("left")?;
            anchor_from_target_expression(left, caller_id, ctx)
        }
        "init_declarator" => {
            let declarator = parent.child_by_field_name("declarator")?;
            let identifier = extract_identifier_node(declarator)?;
            let name = ctx.node_text(identifier);
            Some(PropagationAnchor {
                anchor_id: Some(format!(
                    "{}::local:{}@{}",
                    caller_id,
                    name,
                    identifier.start_position().row + 1
                )),
                symbol_id: None,
                expression_text: Some(name),
                anchor_kind: PropagationAnchorKind::LocalVariable,
            })
        }
        _ => None,
    }
}

fn anchor_from_target_expression(
    node: Node,
    owner_symbol_id: &str,
    ctx: &Ctx,
) -> Option<PropagationAnchor> {
    match node.kind() {
        "identifier" => {
            let name = ctx.node_text(node);
            Some(PropagationAnchor {
                anchor_id: Some(format!(
                    "{}::local:{}@{}",
                    owner_symbol_id,
                    name,
                    node.start_position().row + 1
                )),
                symbol_id: None,
                expression_text: Some(name),
                anchor_kind: PropagationAnchorKind::LocalVariable,
            })
        }
        "parenthesized_expression" => named_children(node)
            .into_iter()
            .next()
            .and_then(|child| anchor_from_target_expression(child, owner_symbol_id, ctx)),
        _ => None,
    }
}

fn extract_receiver(node: Node, ctx: &Ctx) -> String {
    match node.kind() {
        "identifier" | "this" => ctx.node_text(node),
        "pointer_expression" => {
            if let Some(arg) = node.child_by_field_name("argument") {
                extract_receiver(arg, ctx)
            } else {
                ctx.node_text(node)
            }
        }
        _ => ctx.node_text(node),
    }
}

fn detect_receiver_kind(node: Node) -> RawReceiverKind {
    match node.kind() {
        "identifier" => RawReceiverKind::Identifier,
        "this" => RawReceiverKind::This,
        "pointer_expression" => RawReceiverKind::PointerExpression,
        "field_expression" => RawReceiverKind::FieldExpression,
        "qualified_identifier" => RawReceiverKind::QualifiedIdentifier,
        _ => RawReceiverKind::Other,
    }
}

fn field_expr_separator(node: Node, ctx: &Ctx) -> String {
    let argument = match node.child_by_field_name("argument") {
        Some(arg) => arg,
        None => return ".".to_string(),
    };
    let field = match node.child_by_field_name("field") {
        Some(field) => field,
        None => return ".".to_string(),
    };
    let start = argument.end_byte();
    let end = field.start_byte();
    if start >= end || end > ctx.source.len() {
        return ".".to_string();
    }
    let between = std::str::from_utf8(&ctx.source[start..end]).unwrap_or("");
    if between.contains("->") {
        "->".to_string()
    } else {
        ".".to_string()
    }
}

fn classify_qualifier_kind(qualifier: &str, ctx: &Ctx) -> Option<RawQualifierKind> {
    if is_known_type_qualifier(qualifier, ctx) {
        return Some(RawQualifierKind::Type);
    }

    if is_known_namespace_qualifier(qualifier, ctx) {
        return Some(RawQualifierKind::Namespace);
    }

    None
}

fn is_known_type_qualifier(qualifier: &str, ctx: &Ctx) -> bool {
    let contextual = ctx.qualify(qualifier);
    ctx.symbols.iter().any(|sym| {
        (sym.symbol_type == "class" || sym.symbol_type == "struct")
            && (sym.qualified_name == qualifier
                || sym.qualified_name == contextual
                || sym.name == qualifier)
    })
}

fn is_known_namespace_qualifier(qualifier: &str, ctx: &Ctx) -> bool {
    ctx.namespace_ids.contains(qualifier) || ctx.namespace_ids.contains(&ctx.qualify(qualifier))
}

fn parse_qualified_id(node: Node, ctx: &Ctx) -> (String, String) {
    let count = node.named_child_count();
    if count >= 2 {
        let mut parts = Vec::new();
        for i in 0..count {
            if let Some(child) = node.named_child(i) {
                parts.push(ctx.node_text(child));
            }
        }
        let member = parts.pop().unwrap_or_default();
        let class = parts.join("::");
        (class, member)
    } else {
        (String::new(), ctx.node_text(node))
    }
}

fn build_func_signature(node: Node, ctx: &Ctx) -> String {
    let return_type = build_return_type(node, ctx);
    let decl_node = match node.child_by_field_name("declarator") {
        Some(d) => d,
        None => return String::new(),
    };

    let func_decl = if decl_node.kind() == "function_declarator" {
        decl_node
    } else {
        match find_descendant(decl_node, "function_declarator") {
            Some(fd) => fd,
            None => return format!("{} {}", return_type, ctx.node_text(decl_node)).trim().to_string(),
        }
    };

    let name = func_decl
        .child_by_field_name("declarator")
        .map(|n| ctx.node_text(n))
        .unwrap_or_default();
    let params = func_decl
        .child_by_field_name("parameters")
        .map(|n| ctx.node_text(n))
        .unwrap_or_else(|| "()".to_string());

    let mut qualifiers = Vec::new();
    let mut cursor = func_decl.walk();
    for child in func_decl.children(&mut cursor) {
        if child.kind() == "type_qualifier" {
            qualifiers.push(ctx.node_text(child));
        }
    }

    let qual = if qualifiers.is_empty() {
        String::new()
    } else {
        format!(" {}", qualifiers.join(" "))
    };

    let prefix = if return_type.is_empty() {
        String::new()
    } else {
        format!("{} ", return_type)
    };

    format!("{}{}{}{}", prefix, name, params, qual).trim().to_string()
}

fn build_return_type(node: Node, ctx: &Ctx) -> String {
    let mut parts = Vec::new();
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "type_qualifier" | "primitive_type" | "type_identifier"
            | "qualified_identifier" | "sized_type_specifier" | "template_type" => {
                parts.push(ctx.node_text(child));
            }
            _ => {
                if child.kind().contains("declarator") || child.kind() == "compound_statement"
                    || child.kind() == "field_initializer_list"
                {
                    break;
                }
            }
        }
    }

    let decl_node = node.child_by_field_name("declarator");
    if let Some(d) = decl_node {
        match d.kind() {
            "reference_declarator" => {
                if find_descendant(d, "function_declarator").is_some() {
                    parts.push("&".to_string());
                }
            }
            "pointer_declarator" => {
                parts.push("*".to_string());
            }
            _ => {}
        }
    }

    parts.join(" ")
}

fn build_decl_signature(node: Node, ctx: &Ctx) -> String {
    let text = ctx.node_text(node);
    let trimmed = text.trim_end_matches(';').trim_end();
    if let Some(brace) = trimmed.find('{') {
        trimmed[..brace].trim().to_string()
    } else {
        trimmed.to_string()
    }
}

fn count_parameters(func_decl: Node, ctx: &Ctx) -> Option<usize> {
    let params = func_decl.child_by_field_name("parameters")?;
    Some(count_parameter_like_list(params, ctx))
}

fn count_arguments(args_node: Node, ctx: &Ctx) -> usize {
    count_parameter_like_list(args_node, ctx)
}

fn count_parameter_like_list(list_node: Node, ctx: &Ctx) -> usize {
    let named_count = list_node.named_child_count();
    if named_count == 0 {
        return 0;
    }

    if named_count == 1 {
        if let Some(child) = list_node.named_child(0) {
            let text = ctx.node_text(child);
            if child.kind() == "primitive_type" && text == "void" {
                return 0;
            }
        }
    }

    named_count
}

fn find_child<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    let result = node.children(&mut cursor).find(|c| c.kind() == kind);
    result
}

fn named_children<'a>(node: Node<'a>) -> Vec<Node<'a>> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .filter(|child| child.is_named())
        .collect()
}

fn find_descendant<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
    let mut stack = vec![node];

    while let Some(current) = stack.pop() {
        if current.kind() == kind {
            return Some(current);
        }

        let mut cursor = current.walk();
        let children: Vec<Node> = current.children(&mut cursor).collect();
        for child in children.into_iter().rev() {
            stack.push(child);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn extract_legacy_raw_calls(file_path: &str, source: &str) -> Vec<RawCallSite> {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_cpp::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();
        let mut ctx = Ctx {
            file_path: file_path.to_string(),
            source: source.as_bytes().to_vec(),
            symbols: Vec::new(),
            raw_calls: Vec::new(),
            ns_stack: Vec::new(),
            namespace_ids: HashSet::new(),
        };
        visit_tree(tree.root_node(), &mut ctx);
        ctx.raw_calls
    }

    fn extract_graph_raw_calls(file_path: &str, source: &str) -> Vec<RawCallSite> {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_cpp::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();
        let mut ctx = Ctx {
            file_path: file_path.to_string(),
            source: source.as_bytes().to_vec(),
            symbols: Vec::new(),
            raw_calls: Vec::new(),
            ns_stack: Vec::new(),
            namespace_ids: HashSet::new(),
        };
        visit_tree(tree.root_node(), &mut ctx);
        extract_graph_relation_events(file_path, source, &tree, &ctx)
            .0
            .into_iter()
            .filter_map(|event| event.to_raw_call_site())
            .collect()
    }

    fn assert_graph_parity(file_path: &str, source: &str) {
        let legacy = extract_legacy_raw_calls(file_path, source);
        let graph = extract_graph_raw_calls(file_path, source);
        assert_eq!(
            raw_call_comparison_keys(&graph),
            raw_call_comparison_keys(&legacy),
            "graph extraction diverged from legacy extraction for {}",
            file_path
        );
    }

    fn relation_event_key(event: &RawRelationEvent) -> String {
        format!(
            "{}|{}|{}|{}|{}|{}|{}",
            match event.relation_kind {
                RawRelationKind::Call => "call",
                RawRelationKind::TypeUsage => "type_usage",
                RawRelationKind::Inheritance => "inheritance",
                RawRelationKind::EnumValueUsage => "enum_value_usage",
            },
            event.caller_id.as_deref().unwrap_or_default(),
            event.target_name.as_deref().unwrap_or_default(),
            event.file_path,
            event.line,
            event
                .qualifier
                .as_deref()
                .unwrap_or_default(),
            match event.source {
                RawEventSource::LegacyAst => "legacy",
                RawEventSource::TreeSitterGraph => "graph",
            }
        )
    }

    fn propagation_key(event: &PropagationEvent) -> String {
        format!(
            "{}|{}|{}|{}|{}",
            event
                .source_anchor
                .anchor_id
                .as_deref()
                .or(event.source_anchor.expression_text.as_deref())
                .unwrap_or_default(),
            event
                .target_anchor
                .anchor_id
                .as_deref()
                .or(event.target_anchor.expression_text.as_deref())
                .unwrap_or_default(),
            match event.propagation_kind {
                PropagationKind::Assignment => "assignment",
                PropagationKind::InitializerBinding => "initializerBinding",
                PropagationKind::ArgumentToParameter => "argumentToParameter",
                PropagationKind::ReturnValue => "returnValue",
                PropagationKind::FieldWrite => "fieldWrite",
                PropagationKind::FieldRead => "fieldRead",
            },
            event.file_path,
            event.line,
        )
    }

    #[test]
    fn emits_local_initializer_and_assignment_propagation_events() {
        let source = include_str!("../../samples/propagation/src/local_flows.cpp");
        let result = parse_cpp_file("samples/propagation/src/local_flows.cpp", source).unwrap();

        let propagation_keys: Vec<_> = result
            .propagation_events
            .iter()
            .map(propagation_key)
            .collect();

        assert!(propagation_keys.iter().any(|key| {
            key.contains("Game::Tick::param:input@2")
                && key.contains("Game::Tick::local:first@3")
                && key.contains("initializerBinding")
        }));
        assert!(propagation_keys.iter().any(|key| {
            key.contains("Game::Tick::local:first@3")
                && key.contains("Game::Tick::local:second@4")
                && key.contains("initializerBinding")
        }));
        assert!(propagation_keys.iter().any(|key| {
            key.contains("Game::Tick::param:input@2")
                && key.contains("Game::Tick::local:second@4")
                && key.contains("assignment")
        }));
        assert!(propagation_keys.iter().any(|key| {
            key.contains("Game::Tick::local:first@3")
                && key.contains("Game::Tick::local:second@4")
                && key.contains("assignment")
        }));
        assert!(propagation_keys.iter().any(|key| {
            key.contains("Game::Tick::local:third@6")
                && key.contains("initializerBinding")
        }));
        assert!(propagation_keys.iter().any(|key| {
            key.contains("Game::Tick::local:copy@7")
                && key.contains("initializerBinding")
        }));
    }

    #[test]
    fn marks_pointer_heavy_local_flow_as_partial() {
        let source = include_str!("../../samples/propagation/src/local_flows.cpp");
        let result = parse_cpp_file("samples/propagation/src/local_flows.cpp", source).unwrap();

        let pointer_heavy = result
            .propagation_events
            .iter()
            .find(|event| {
                event.target_anchor.anchor_id.as_deref() == Some("Game::Tick::local:alias@9")
                    && event.propagation_kind == PropagationKind::InitializerBinding
            })
            .unwrap();

        assert_eq!(pointer_heavy.confidence, RawExtractionConfidence::Partial);
        assert!(pointer_heavy
            .risks
            .contains(&PropagationRisk::PointerHeavyFlow));
        assert_eq!(
            pointer_heavy.source_anchor.expression_text.as_deref(),
            Some("*ptr")
        );
    }

    #[test]
    fn keeps_shadowed_locals_distinct_in_propagation_events() {
        let source = include_str!("../../samples/propagation/src/shadowing.cpp");
        let result = parse_cpp_file("samples/propagation/src/shadowing.cpp", source).unwrap();

        let mirror_event = result
            .propagation_events
            .iter()
            .find(|event| event.target_anchor.anchor_id.as_deref() == Some("Game::Tick::local:mirror@6"))
            .unwrap();
        let after_event = result
            .propagation_events
            .iter()
            .find(|event| event.target_anchor.anchor_id.as_deref() == Some("Game::Tick::local:after@8"))
            .unwrap();

        assert_eq!(
            mirror_event.source_anchor.anchor_id.as_deref(),
            Some("Game::Tick::local:value@5")
        );
        assert_eq!(
            after_event.source_anchor.anchor_id.as_deref(),
            Some("Game::Tick::local:value@3")
        );
    }

    #[test]
    fn emits_callable_flow_summaries_with_parameters_and_returns() {
        let source = include_str!("../../samples/propagation/src/function_boundary.cpp");
        let result = parse_cpp_file("samples/propagation/src/function_boundary.cpp", source).unwrap();

        let forward = result
            .callable_flow_summaries
            .iter()
            .find(|summary| summary.callable_symbol_id == "Game::Forward")
            .unwrap();
        assert_eq!(forward.parameter_anchors.len(), 1);
        assert_eq!(
            forward.parameter_anchors[0].anchor_id.as_deref(),
            Some("Game::Forward::param:value@4")
        );
        assert_eq!(forward.return_anchors.len(), 1);
        assert_eq!(
            forward.return_anchors[0].anchor_id.as_deref(),
            Some("Game::Forward::local:local@5")
        );

        let consume = result
            .callable_flow_summaries
            .iter()
            .find(|summary| summary.callable_symbol_id == "Game::Consume")
            .unwrap();
        assert_eq!(consume.parameter_anchors.len(), 1);
        assert!(consume.return_anchors.is_empty());

        let make_hint = result
            .callable_flow_summaries
            .iter()
            .find(|summary| summary.callable_symbol_id == "Game::MakeHint")
            .unwrap();
        assert_eq!(make_hint.parameter_anchors.len(), 1);
        assert_eq!(
            make_hint.parameter_anchors[0].anchor_id.as_deref(),
            Some("Game::MakeHint::param:value@13")
        );
        assert_eq!(make_hint.return_anchors.len(), 1);
        assert_eq!(
            make_hint.return_anchors[0].anchor_id.as_deref(),
            Some("Game::MakeHint::local:hint@14")
        );

        let apply_hint = result
            .callable_flow_summaries
            .iter()
            .find(|summary| summary.callable_symbol_id == "Game::BoundaryWorker::ApplyHint")
            .unwrap();
        assert_eq!(apply_hint.parameter_anchors.len(), 1);
        assert_eq!(
            apply_hint.parameter_anchors[0].anchor_id.as_deref(),
            Some("Game::BoundaryWorker::ApplyHint::param:hint@20")
        );
        assert!(apply_hint.return_anchors.is_empty());

        assert!(result.propagation_events.iter().any(|event| {
            event.owner_symbol_id.as_deref() == Some("Game::BoundaryWorker::ApplyHint")
                && event.propagation_kind == PropagationKind::FieldWrite
                && event.source_anchor.expression_text.as_deref() == Some("hint.power")
                && event.target_anchor.anchor_id.as_deref()
                    == Some("Game::BoundaryWorker::field:stored")
                && event.confidence == RawExtractionConfidence::Partial
                && event.risks.contains(&PropagationRisk::ReceiverAmbiguity)
        }));

        let make_envelope = result
            .callable_flow_summaries
            .iter()
            .find(|summary| summary.callable_symbol_id == "Game::MakeEnvelope")
            .unwrap();
        assert_eq!(make_envelope.parameter_anchors.len(), 1);
        assert_eq!(
            make_envelope.parameter_anchors[0].anchor_id.as_deref(),
            Some("Game::MakeEnvelope::param:value@43")
        );
        assert_eq!(make_envelope.return_anchors.len(), 1);
        assert_eq!(
            make_envelope.return_anchors[0].anchor_id.as_deref(),
            Some("Game::MakeEnvelope::local:envelope@44")
        );

        let envelope_apply_hint = result
            .callable_flow_summaries
            .iter()
            .find(|summary| summary.callable_symbol_id == "Game::EnvelopeWorker::ApplyHint")
            .unwrap();
        assert_eq!(envelope_apply_hint.parameter_anchors.len(), 1);
        assert_eq!(
            envelope_apply_hint.parameter_anchors[0].anchor_id.as_deref(),
            Some("Game::EnvelopeWorker::ApplyHint::param:hint@67")
        );
        assert!(envelope_apply_hint.return_anchors.is_empty());

        assert!(result.propagation_events.iter().any(|event| {
            event.owner_symbol_id.as_deref() == Some("Game::EnvelopeWorker::ApplyHint")
                && event.propagation_kind == PropagationKind::FieldWrite
                && event.source_anchor.expression_text.as_deref() == Some("hint.power")
                && event.target_anchor.anchor_id.as_deref()
                    == Some("Game::EnvelopeWorker::field:stored")
                && event.confidence == RawExtractionConfidence::Partial
                && event.risks.contains(&PropagationRisk::ReceiverAmbiguity)
        }));

        let make_nested_envelope = result
            .callable_flow_summaries
            .iter()
            .find(|summary| summary.callable_symbol_id == "Game::MakeNestedEnvelope")
            .unwrap();
        assert_eq!(make_nested_envelope.parameter_anchors.len(), 1);
        assert_eq!(
            make_nested_envelope.parameter_anchors[0].anchor_id.as_deref(),
            Some("Game::MakeNestedEnvelope::param:value@52")
        );
        assert_eq!(make_nested_envelope.return_anchors.len(), 1);
        assert_eq!(
            make_nested_envelope.return_anchors[0].anchor_id.as_deref(),
            Some("Game::MakeNestedEnvelope::local:nested@53")
        );

        let nested_envelope_apply_hint = result
            .callable_flow_summaries
            .iter()
            .find(|summary| summary.callable_symbol_id == "Game::NestedEnvelopeWorker::ApplyHint")
            .unwrap();
        assert_eq!(nested_envelope_apply_hint.parameter_anchors.len(), 1);
        assert_eq!(
            nested_envelope_apply_hint.parameter_anchors[0]
                .anchor_id
                .as_deref(),
            Some("Game::NestedEnvelopeWorker::ApplyHint::param:hint@82")
        );
        assert!(nested_envelope_apply_hint.return_anchors.is_empty());

        assert!(result.propagation_events.iter().any(|event| {
            event.owner_symbol_id.as_deref() == Some("Game::NestedEnvelopeWorker::ApplyHint")
                && event.propagation_kind == PropagationKind::FieldWrite
                && event.source_anchor.expression_text.as_deref() == Some("hint.power")
                && event.target_anchor.anchor_id.as_deref()
                    == Some("Game::NestedEnvelopeWorker::field:stored")
                && event.confidence == RawExtractionConfidence::Partial
                && event.risks.contains(&PropagationRisk::ReceiverAmbiguity)
        }));

        let extract_hint_power = result
            .callable_flow_summaries
            .iter()
            .find(|summary| summary.callable_symbol_id == "Game::ExtractHintPower")
            .unwrap();
        assert_eq!(extract_hint_power.parameter_anchors.len(), 1);
        assert_eq!(
            extract_hint_power.parameter_anchors[0].anchor_id.as_deref(),
            Some("Game::ExtractHintPower::param:hint@57")
        );
        assert_eq!(extract_hint_power.return_anchors.len(), 1);
        assert_eq!(
            extract_hint_power.return_anchors[0].anchor_id.as_deref(),
            Some("Game::ExtractHintPower::fieldexpr:hint_power@58")
        );

        let emit_power = result
            .callable_flow_summaries
            .iter()
            .find(|summary| summary.callable_symbol_id == "Game::EmitPower")
            .unwrap();
        assert_eq!(emit_power.parameter_anchors.len(), 1);
        assert_eq!(
            emit_power.parameter_anchors[0].anchor_id.as_deref(),
            Some("Game::EmitPower::param:power@61")
        );
        assert!(emit_power.return_anchors.is_empty());

        let member_relay_seed = result
            .callable_flow_summaries
            .iter()
            .find(|summary| summary.callable_symbol_id == "Game::MemberRelayWorker::Seed")
            .unwrap();
        assert_eq!(member_relay_seed.parameter_anchors.len(), 1);
        assert_eq!(
            member_relay_seed.parameter_anchors[0].anchor_id.as_deref(),
            Some("Game::MemberRelayWorker::Seed::param:hint@97")
        );
        assert!(member_relay_seed.return_anchors.is_empty());

        let member_relay_emit = result
            .callable_flow_summaries
            .iter()
            .find(|summary| summary.callable_symbol_id == "Game::MemberRelayWorker::EmitStored")
            .unwrap();
        assert!(member_relay_emit.parameter_anchors.is_empty());
        assert!(member_relay_emit.return_anchors.is_empty());

        assert!(result.propagation_events.iter().any(|event| {
            event.owner_symbol_id.as_deref() == Some("Game::MemberRelayWorker::Seed")
                && event.propagation_kind == PropagationKind::FieldWrite
                && event.source_anchor.expression_text.as_deref() == Some("hint.power")
                && event.target_anchor.anchor_id.as_deref()
                    == Some("Game::MemberRelayWorker::field:stored")
                && event.confidence == RawExtractionConfidence::Partial
                && event.risks.contains(&PropagationRisk::ReceiverAmbiguity)
        }));
    }

    #[test]
    fn emits_member_state_field_write_and_field_read_events() {
        let source = include_str!("../../samples/propagation/src/member_state.cpp");
        let result = parse_cpp_file("samples/propagation/src/member_state.cpp", source).unwrap();

        assert!(result.propagation_events.iter().any(|event| {
                event.owner_symbol_id.as_deref() == Some("Game::Worker::SetFromParam")
                    && event.propagation_kind == PropagationKind::FieldWrite
                && event.source_anchor.anchor_id.as_deref()
                    == Some("Game::Worker::SetFromParam::param:value@23")
                && event.target_anchor.anchor_id.as_deref() == Some("Game::Worker::field:stored")
                && event.confidence == RawExtractionConfidence::High
        }));
        assert!(result.propagation_events.iter().any(|event| {
                event.owner_symbol_id.as_deref() == Some("Game::Worker::SetFromLocal")
                    && event.propagation_kind == PropagationKind::FieldWrite
                && event.source_anchor.anchor_id.as_deref()
                    == Some("Game::Worker::SetFromLocal::local:local@28")
                && event.target_anchor.anchor_id.as_deref() == Some("Game::Worker::field:cached")
        }));
        assert!(result.propagation_events.iter().any(|event| {
                event.owner_symbol_id.as_deref() == Some("Game::Worker::ReadToLocal")
                    && event.propagation_kind == PropagationKind::FieldRead
                && event.source_anchor.anchor_id.as_deref() == Some("Game::Worker::field:stored")
                && event.target_anchor.anchor_id.as_deref()
                    == Some("Game::Worker::ReadToLocal::local:local@33")
        }));
        assert!(result.propagation_events.iter().any(|event| {
                event.owner_symbol_id.as_deref() == Some("Game::Worker::ReadMember")
                    && event.propagation_kind == PropagationKind::FieldRead
                && event.source_anchor.anchor_id.as_deref() == Some("Game::Worker::field:cached")
                && event.target_anchor.anchor_id.as_deref()
                    == Some("Game::Worker::ReadMember::return@38")
        }));
        assert!(result.propagation_events.iter().any(|event| {
            event.owner_symbol_id.as_deref() == Some("Game::Worker::PullFromCarrier")
                && event.propagation_kind == PropagationKind::FieldWrite
                && event.source_anchor.expression_text.as_deref() == Some("carrier.power")
                && event.target_anchor.anchor_id.as_deref() == Some("Game::Worker::field:stored")
        }));
        assert!(result.propagation_events.iter().any(|event| {
            event.owner_symbol_id.as_deref() == Some("Game::Worker::PushToCarrier")
                && event.propagation_kind == PropagationKind::FieldWrite
                && event.source_anchor.anchor_id.as_deref() == Some("Game::Worker::field:cached")
                && event.target_anchor.expression_text.as_deref() == Some("carrier.mirrored")
        }));
        assert!(result.propagation_events.iter().any(|event| {
                event.owner_symbol_id.as_deref() == Some("Game::Worker::StageThroughLocalCarrier")
                    && event.propagation_kind == PropagationKind::FieldWrite
                && event.source_anchor.anchor_id.as_deref()
                    == Some("Game::Worker::StageThroughLocalCarrier::param:value@58")
                && event.target_anchor.expression_text.as_deref() == Some("stage.power")
                && event.confidence == RawExtractionConfidence::Partial
                && event.risks.contains(&PropagationRisk::ReceiverAmbiguity)
        }));
        assert!(result.propagation_events.iter().any(|event| {
            event.owner_symbol_id.as_deref() == Some("Game::Worker::StageThroughLocalCarrier")
                && event.propagation_kind == PropagationKind::FieldWrite
                && event.source_anchor.expression_text.as_deref() == Some("stage.power")
                && event.target_anchor.anchor_id.as_deref() == Some("Game::Worker::field:stored")
                && event.confidence == RawExtractionConfidence::Partial
                && event.risks.contains(&PropagationRisk::ReceiverAmbiguity)
        }));
        assert!(result.propagation_events.iter().any(|event| {
                event.owner_symbol_id.as_deref() == Some("Game::ConstructedWorker::ConstructedWorker")
                    && event.propagation_kind == PropagationKind::FieldWrite
                && event.source_anchor.anchor_id.as_deref()
                    == Some("Game::ConstructedWorker::ConstructedWorker::param:initialStored@72")
                && event.target_anchor.anchor_id.as_deref()
                    == Some("Game::ConstructedWorker::field:stored")
                && event.confidence == RawExtractionConfidence::High
        }));
        assert!(result.propagation_events.iter().any(|event| {
                event.owner_symbol_id.as_deref() == Some("Game::ConstructedWorker::ConstructedWorker")
                    && event.propagation_kind == PropagationKind::FieldWrite
                && event.source_anchor.anchor_id.as_deref()
                    == Some("Game::ConstructedWorker::ConstructedWorker::param:initialCached@72")
                && event.target_anchor.anchor_id.as_deref()
                    == Some("Game::ConstructedWorker::field:cached")
                && event.confidence == RawExtractionConfidence::High
        }));
        assert!(result.propagation_events.iter().any(|event| {
            event.owner_symbol_id.as_deref()
                == Some("Game::CarrierConstructedWorker::CarrierConstructedWorker")
                && event.propagation_kind == PropagationKind::FieldWrite
                && event.source_anchor.expression_text.as_deref() == Some("carrier.power")
                && event.target_anchor.anchor_id.as_deref()
                    == Some("Game::CarrierConstructedWorker::field:stored")
                && event.confidence == RawExtractionConfidence::Partial
                && event.risks.contains(&PropagationRisk::ReceiverAmbiguity)
        }));
        assert!(result.propagation_events.iter().any(|event| {
            event.owner_symbol_id.as_deref()
                == Some("Game::CarrierConstructedWorker::CarrierConstructedWorker")
                && event.propagation_kind == PropagationKind::FieldWrite
                && event.source_anchor.expression_text.as_deref() == Some("carrier.mirrored")
                && event.target_anchor.anchor_id.as_deref()
                    == Some("Game::CarrierConstructedWorker::field:cached")
                && event.confidence == RawExtractionConfidence::Partial
                && event.risks.contains(&PropagationRisk::ReceiverAmbiguity)
        }));
        assert!(result.propagation_events.iter().any(|event| {
            event.owner_symbol_id.as_deref()
                == Some("Game::PointerCarrierConstructedWorker::PointerCarrierConstructedWorker")
                && event.propagation_kind == PropagationKind::FieldWrite
                && event.source_anchor.expression_text.as_deref() == Some("carrier->power")
                && event.target_anchor.anchor_id.as_deref()
                    == Some("Game::PointerCarrierConstructedWorker::field:stored")
                && event.confidence == RawExtractionConfidence::Partial
                && event.risks.contains(&PropagationRisk::PointerHeavyFlow)
                && event.risks.contains(&PropagationRisk::ReceiverAmbiguity)
        }));
        assert!(result.propagation_events.iter().any(|event| {
            event.owner_symbol_id.as_deref()
                == Some("Game::PointerCarrierConstructedWorker::PointerCarrierConstructedWorker")
                && event.propagation_kind == PropagationKind::FieldWrite
                && event.source_anchor.expression_text.as_deref() == Some("carrier->mirrored")
                && event.target_anchor.anchor_id.as_deref()
                    == Some("Game::PointerCarrierConstructedWorker::field:cached")
                && event.confidence == RawExtractionConfidence::Partial
                && event.risks.contains(&PropagationRisk::PointerHeavyFlow)
                && event.risks.contains(&PropagationRisk::ReceiverAmbiguity)
        }));
        assert!(result.propagation_events.iter().any(|event| {
            event.owner_symbol_id.as_deref()
                == Some("Game::HelperCarrierConstructedWorker::HelperCarrierConstructedWorker")
                && event.propagation_kind == PropagationKind::FieldWrite
                && event.source_anchor.expression_text.as_deref()
                    == Some("MakeCarrier(value).power")
                && event.target_anchor.anchor_id.as_deref()
                    == Some("Game::HelperCarrierConstructedWorker::field:stored")
                && event.confidence == RawExtractionConfidence::Partial
                && event.risks.contains(&PropagationRisk::ReceiverAmbiguity)
        }));
        assert!(result.propagation_events.iter().any(|event| {
            event.owner_symbol_id.as_deref()
                == Some("Game::HelperCarrierPipelineWorker::HelperCarrierPipelineWorker")
                && event.propagation_kind == PropagationKind::FieldWrite
                && event.source_anchor.expression_text.as_deref()
                    == Some("MakeCarrier(value).power")
                && event.target_anchor.anchor_id.as_deref()
                    == Some("Game::HelperCarrierPipelineWorker::field:stored")
                && event.confidence == RawExtractionConfidence::Partial
                && event.risks.contains(&PropagationRisk::ReceiverAmbiguity)
        }));
        assert!(result.propagation_events.iter().any(|event| {
            event.owner_symbol_id.as_deref()
                == Some("Game::HelperCarrierPipelineWorker::EmitStored")
                && event.propagation_kind == PropagationKind::FieldRead
                && event.source_anchor.anchor_id.as_deref()
                    == Some("Game::HelperCarrierPipelineWorker::field:stored")
                && event.target_anchor.anchor_id.as_deref()
                    == Some("Game::HelperCarrierPipelineWorker::EmitStored::local:current@116")
                && event.confidence == RawExtractionConfidence::High
        }));
        assert!(result.propagation_events.iter().any(|event| {
            event.owner_symbol_id.as_deref()
                == Some("Game::HelperCarrierConstructedWorker::HelperCarrierConstructedWorker")
                && event.propagation_kind == PropagationKind::FieldWrite
                && event.source_anchor.expression_text.as_deref()
                    == Some("MakeCarrier(value).mirrored")
                && event.target_anchor.anchor_id.as_deref()
                    == Some("Game::HelperCarrierConstructedWorker::field:cached")
                && event.confidence == RawExtractionConfidence::Partial
                && event.risks.contains(&PropagationRisk::ReceiverAmbiguity)
        }));
        assert!(result.propagation_events.iter().any(|event| {
            event.owner_symbol_id.as_deref()
                == Some("Game::NestedHelperCarrierPipelineWorker::NestedHelperCarrierPipelineWorker")
                && event.propagation_kind == PropagationKind::FieldWrite
                && event.source_anchor.expression_text.as_deref()
                    == Some("MakeCarrierEnvelope(value).carrier.power")
                && event.target_anchor.anchor_id.as_deref()
                    == Some("Game::NestedHelperCarrierPipelineWorker::field:stored")
                && event.confidence == RawExtractionConfidence::Partial
                && event.risks.contains(&PropagationRisk::ReceiverAmbiguity)
        }));
        assert!(result.propagation_events.iter().any(|event| {
            event.owner_symbol_id.as_deref()
                == Some("Game::NestedHelperCarrierPipelineWorker::EmitStored")
                && event.propagation_kind == PropagationKind::FieldRead
                && event.source_anchor.anchor_id.as_deref()
                    == Some("Game::NestedHelperCarrierPipelineWorker::field:stored")
                && event.target_anchor.anchor_id.as_deref()
                    == Some("Game::NestedHelperCarrierPipelineWorker::EmitStored::local:current@130")
                && event.confidence == RawExtractionConfidence::High
        }));
    }

    #[test]
    fn marks_object_and_pointer_member_state_flows_as_partial_when_receiver_identity_is_weaker() {
        let source = include_str!("../../samples/propagation/src/member_state.cpp");
        let result = parse_cpp_file("samples/propagation/src/member_state.cpp", source).unwrap();

        let object_write = result
            .propagation_events
            .iter()
            .find(|event| {
                event.owner_symbol_id.as_deref() == Some("Game::Worker::CopyToPeer")
                    && event.propagation_kind == PropagationKind::FieldWrite
                    && event.target_anchor.expression_text.as_deref() == Some("other.shared")
            })
            .unwrap();
        assert_eq!(object_write.confidence, RawExtractionConfidence::Partial);
        assert!(object_write
            .risks
            .contains(&PropagationRisk::ReceiverAmbiguity));

        let pointer_read = result
            .propagation_events
            .iter()
            .find(|event| {
                event.owner_symbol_id.as_deref() == Some("Game::Worker::ReadFromPointer")
                    && event.propagation_kind == PropagationKind::FieldRead
                    && event.source_anchor.expression_text.as_deref() == Some("other->shared")
            })
            .unwrap();
        assert_eq!(pointer_read.confidence, RawExtractionConfidence::Partial);
        assert!(pointer_read
            .risks
            .contains(&PropagationRisk::PointerHeavyFlow));
        assert!(pointer_read
            .risks
            .contains(&PropagationRisk::ReceiverAmbiguity));
    }

    #[test]
    fn parses_simple_function() {
        let src = "void foo(int x) { bar(x); }";
        let result = parse_cpp_file("test.cpp", src).unwrap();
        assert_eq!(result.symbols.len(), 1);
        assert_eq!(result.symbols[0].name, "foo");
        assert_eq!(result.symbols[0].symbol_type, "function");
        assert_eq!(result.symbols[0].parameter_count, Some(1));
        assert!(result.raw_calls.iter().any(|c| {
            c.called_name == "bar"
                && matches!(c.call_kind, RawCallKind::Unqualified)
                && c.argument_count == Some(1)
        }));
    }

    #[test]
    fn parses_namespace_and_class() {
        let src = r#"
namespace Game {
class Player {
public:
    void Update(float dt);
};
}
"#;
        let result = parse_cpp_file("test.h", src).unwrap();
        let class = result.symbols.iter().find(|s| s.name == "Player").unwrap();
        assert_eq!(class.id, "Game::Player");
        let method = result.symbols.iter().find(|s| s.name == "Update").unwrap();
        assert_eq!(method.id, "Game::Player::Update");
        assert_eq!(method.parent_id.as_deref(), Some("Game::Player"));
    }

    #[test]
    fn parses_method_definition() {
        let src = r#"
namespace Game {
void Player::Update(float dt) {
    Render();
}
}
"#;
        let result = parse_cpp_file("test.cpp", src).unwrap();
        let method = result.symbols.iter().find(|s| s.name == "Update").unwrap();
        assert_eq!(method.id, "Game::Player::Update");
        assert_eq!(method.symbol_type, "method");
        assert_eq!(method.parameter_count, Some(1));
        assert!(result.raw_calls.iter().any(|c| {
            c.called_name == "Render" && matches!(c.call_kind, RawCallKind::Unqualified)
        }));
    }

    #[test]
    fn parses_enum_class() {
        let src = "enum class AIState { Idle, Patrol, Chase };";
        let result = parse_cpp_file("test.h", src).unwrap();
        assert!(result.symbols.iter().any(|symbol| {
            symbol.name == "AIState" && symbol.symbol_type == "enum"
        }));
        assert!(result.symbols.iter().any(|symbol| {
            symbol.qualified_name == "AIState::Idle" && symbol.symbol_type == "enumMember"
        }));
        assert!(result.symbols.iter().any(|symbol| {
            symbol.qualified_name == "AIState::Patrol" && symbol.symbol_type == "enumMember"
        }));
        assert!(result.symbols.iter().any(|symbol| {
            symbol.qualified_name == "AIState::Chase" && symbol.symbol_type == "enumMember"
        }));
    }

    #[test]
    fn emits_enum_value_usage_references_for_qualified_and_plain_members() {
        let src = r#"
namespace Game {
enum class AIState { Idle, Patrol, Chase };
enum LegacyMode { Off, On };

class Controller {
public:
    void Update() {
        AIState state = AIState::Idle;
        if (state == AIState::Patrol) {
            state = AIState::Chase;
        }
        LegacyMode mode = On;
    }
};
}
"#;
        let result = parse_cpp_file("enum_usage.cpp", src).unwrap();
        let enum_refs: Vec<_> = result
            .normalized_references
            .iter()
            .filter(|reference| reference.category == ReferenceCategory::EnumValueUsage)
            .collect();

        assert!(result.symbols.iter().any(|symbol| {
            symbol.qualified_name == "Game::AIState::Idle" && symbol.symbol_type == "enumMember"
        }));
        assert!(result.symbols.iter().any(|symbol| {
            symbol.qualified_name == "Game::LegacyMode::On" && symbol.symbol_type == "enumMember"
        }));
        assert!(enum_refs.iter().any(|reference| {
            reference.target_symbol_id == "Game::AIState::Idle"
                && reference.source_symbol_id == "Game::Controller::Update"
        }));
        assert!(enum_refs.iter().any(|reference| {
            reference.target_symbol_id == "Game::AIState::Patrol"
                && reference.source_symbol_id == "Game::Controller::Update"
        }));
        assert!(enum_refs.iter().any(|reference| {
            reference.target_symbol_id == "Game::AIState::Chase"
                && reference.source_symbol_id == "Game::Controller::Update"
        }));
        assert!(enum_refs.iter().any(|reference| {
            reference.target_symbol_id == "Game::LegacyMode::On"
                && reference.source_symbol_id == "Game::Controller::Update"
        }));
    }

    #[test]
    fn emits_enum_value_usage_references_for_flag_composition_arguments_and_returns() {
        let src = r#"
namespace Game {
enum ShotFlags {
    SHOTFLAG_NONE = 0,
    SHOTFLAG_CHIP = 1 << 0,
    SHOTFLAG_FINESSE = 1 << 1
};

enum OtherFlags {
    OTHER_NONE = 0,
    SHOTFLAG_CHIP = 1 << 0,
    SHOTFLAG_FINESSE = 1 << 1
};

class ShotController {
public:
    void ApplyFlags(ShotFlags flags) {
        ShotFlags localFlags = SHOTFLAG_CHIP | SHOTFLAG_FINESSE;
        ApplyFlags(SHOTFLAG_CHIP | SHOTFLAG_FINESSE);
        localFlags = BuildFlags();
    }

    ShotFlags BuildFlags() {
        return SHOTFLAG_CHIP | SHOTFLAG_FINESSE;
    }
};
}
"#;
        let result = parse_cpp_file("shot_flags.cpp", src).unwrap();
        let enum_refs: Vec<_> = result
            .normalized_references
            .iter()
            .filter(|reference| reference.category == ReferenceCategory::EnumValueUsage)
            .collect();

        let apply_hits: Vec<_> = enum_refs
            .iter()
            .filter(|reference| reference.source_symbol_id == "Game::ShotController::ApplyFlags")
            .collect();
        let build_hits: Vec<_> = enum_refs
            .iter()
            .filter(|reference| reference.source_symbol_id == "Game::ShotController::BuildFlags")
            .collect();

        assert!(apply_hits
            .iter()
            .filter(|reference| reference.target_symbol_id == "Game::ShotFlags::SHOTFLAG_CHIP")
            .count()
            >= 2);
        assert!(apply_hits
            .iter()
            .filter(|reference| reference.target_symbol_id == "Game::ShotFlags::SHOTFLAG_FINESSE")
            .count()
            >= 2);
        assert!(build_hits.iter().any(|reference| {
            reference.target_symbol_id == "Game::ShotFlags::SHOTFLAG_CHIP"
        }));
        assert!(build_hits.iter().any(|reference| {
            reference.target_symbol_id == "Game::ShotFlags::SHOTFLAG_FINESSE"
        }));
        assert!(!enum_refs.iter().any(|reference| {
            reference.target_symbol_id == "Game::OtherFlags::SHOTFLAG_CHIP"
                || reference.target_symbol_id == "Game::OtherFlags::SHOTFLAG_FINESSE"
        }));
    }

    #[test]
    fn resolves_namespace_qualified_unscoped_enum_members_in_realistic_flag_checks() {
        let src = r#"
namespace Gameplay {
enum ShotFlags {
    SHOTFLAG_NONE = 0,
    SHOTFLAG_CHIP = 1 << 4,
    SHOTFLAG_FINESSE = 1 << 11,
};

class ShotContext {
public:
    bool IsShotFlag(ShotFlags flags) const;
};
}

namespace Audio {
bool UsesChip(const Gameplay::ShotContext& msg, Gameplay::ShotFlags shotFlags) {
    if (msg.IsShotFlag(Gameplay::SHOTFLAG_CHIP)) {
        return true;
    }
    return (shotFlags & Gameplay::SHOTFLAG_FINESSE) != 0;
}
}
"#;
        let result = parse_cpp_file("namespace_scoped_flags.cpp", src).unwrap();
        let enum_refs: Vec<_> = result
            .normalized_references
            .iter()
            .filter(|reference| reference.category == ReferenceCategory::EnumValueUsage)
            .collect();

        assert!(enum_refs.iter().any(|reference| {
            reference.source_symbol_id == "Audio::UsesChip"
                && reference.target_symbol_id == "Gameplay::ShotFlags::SHOTFLAG_CHIP"
        }));
        assert!(enum_refs.iter().any(|reference| {
            reference.source_symbol_id == "Audio::UsesChip"
                && reference.target_symbol_id == "Gameplay::ShotFlags::SHOTFLAG_FINESSE"
        }));
    }

    #[test]
    fn extracts_unqualified_base_method_calls_from_out_of_line_method_definitions() {
        let src = r#"
namespace Gameplay {
class ShotSubSystem {
public:
    void SetShotFlags(int flags);
};

class ShotNormal : public ShotSubSystem {
public:
    void CalcShotInformation(int flags);
};

void ShotNormal::CalcShotInformation(int flags) {
    SetShotFlags(flags);
}
}
"#;
        let result = parse_cpp_file("shotnormal_like.cpp", src).unwrap();
        assert!(result.raw_calls.iter().any(|call| {
            call.caller_id == "Gameplay::ShotNormal::CalcShotInformation"
                && call.called_name == "SetShotFlags"
                && matches!(call.call_kind, RawCallKind::Unqualified)
                && call.argument_count == Some(1)
        }));
    }

    #[test]
    fn parses_template_class() {
        let src = r#"
template <typename T>
class Container {
public:
    void Add(const T& item);
    T Get(int index) const;
};
"#;
        let result = parse_cpp_file("test.h", src).unwrap();
        let class = result.symbols.iter().find(|s| s.name == "Container").unwrap();
        assert_eq!(class.symbol_type, "class");
        assert!(result.symbols.iter().any(|s| s.name == "Add"));
        assert!(result.symbols.iter().any(|s| s.name == "Get"));
    }

    #[test]
    fn parses_template_function() {
        let src = r#"
template <typename T>
T Max(T a, T b) { return a > b ? a : b; }
"#;
        let result = parse_cpp_file("test.h", src).unwrap();
        assert!(result.symbols.iter().any(|s| {
            s.name == "Max"
                && s.symbol_type == "function"
                && s.symbol_role.as_deref() == Some("definition")
        }));
    }

    #[test]
    fn exposes_cpp_call_graph_rules_for_initial_shapes() {
        let rules = cpp_call_graph_rule_source();
        assert!(rules.contains("relation_kind = \"call\""));
        assert!(rules.contains("call_kind = \"unqualified\""));
        assert!(rules.contains("call_kind = \"qualified\""));
        assert!(rules.contains("call_kind = \"member_access\""));
        assert!(rules.contains("call_kind = \"pointer_member_access\""));
        assert!(rules.contains("call_kind = \"this_pointer_access\""));
    }

    #[test]
    fn prefers_graph_relation_events_when_they_match_legacy_calls() {
        let src = "void foo() { bar(); }";
        let result = parse_cpp_file("test.cpp", src).unwrap();
        assert_eq!(result.raw_calls.len(), 1);
        assert_eq!(result.relation_events.len(), 1);
        assert_eq!(result.relation_events[0].source, RawEventSource::TreeSitterGraph);
        assert_eq!(result.relation_events[0].relation_kind, RawRelationKind::Call);
        assert_eq!(result.relation_events[0].target_name.as_deref(), Some("bar"));
    }

    #[test]
    fn graph_matches_legacy_on_namespace_dupes_fixture() {
        let source = include_str!("../../samples/ambiguity/src/namespace_dupes.cpp");
        assert_graph_parity("samples/ambiguity/src/namespace_dupes.cpp", source);
    }

    #[test]
    fn graph_matches_legacy_on_overloads_fixture() {
        let source = include_str!("../../samples/ambiguity/src/overloads.cpp");
        assert_graph_parity("samples/ambiguity/src/overloads.cpp", source);
    }

    #[test]
    fn graph_matches_legacy_on_sibling_methods_fixture() {
        let source = include_str!("../../samples/ambiguity/src/sibling_methods.cpp");
        assert_graph_parity("samples/ambiguity/src/sibling_methods.cpp", source);
    }

    #[test]
    fn graph_matches_legacy_on_split_update_fixture() {
        let source = include_str!("../../samples/ambiguity/src/split_update.cpp");
        assert_graph_parity("samples/ambiguity/src/split_update.cpp", source);
    }

    #[test]
    fn graph_matches_legacy_on_complex_receivers_fixture() {
        let source = include_str!("../../samples/ambiguity/src/complex_receivers.cpp");
        assert_graph_parity("samples/ambiguity/src/complex_receivers.cpp", source);
    }

    #[test]
    fn emits_type_usage_relation_events_from_ast_extraction() {
        let src = r#"
namespace Game {
class Actor {};

class Controller {
public:
    Actor* actor;
    void Tick(Actor value) {
        Actor local;
    }
};
}
"#;
        let result = parse_cpp_file("type_usage.cpp", src).unwrap();
        let type_usage_events: Vec<_> = result
            .relation_events
            .iter()
            .filter(|event| event.relation_kind == RawRelationKind::TypeUsage)
            .collect();
        assert!(!type_usage_events.is_empty());
        assert!(type_usage_events.iter().all(|event| event.source == RawEventSource::LegacyAst));
        assert!(type_usage_events
            .iter()
            .any(|event| event.target_name.as_deref() == Some("Actor")));
        assert!(type_usage_events.iter().any(|event| {
            event.caller_id.as_deref() == Some("Game::Controller")
                || event.caller_id.as_deref() == Some("Game::Controller::Tick")
        }));
        let normalized_refs: Vec<_> = result
            .normalized_references
            .iter()
            .filter(|reference| reference.category == ReferenceCategory::TypeUsage)
            .collect();
        assert!(!normalized_refs.is_empty());
        assert!(normalized_refs
            .iter()
            .all(|reference| reference.target_symbol_id == "Game::Actor"));
    }

    #[test]
    fn emits_inheritance_relation_events_from_ast_extraction() {
        let src = r#"
namespace Game {
class Actor {};
class Player : public Actor {};
}
"#;
        let result = parse_cpp_file("inheritance.cpp", src).unwrap();
        let inheritance_events: Vec<_> = result
            .relation_events
            .iter()
            .filter(|event| event.relation_kind == RawRelationKind::Inheritance)
            .collect();
        assert_eq!(inheritance_events.len(), 1);
        assert_eq!(inheritance_events[0].source, RawEventSource::LegacyAst);
        assert_eq!(inheritance_events[0].target_name.as_deref(), Some("Actor"));
        assert_eq!(inheritance_events[0].caller_id.as_deref(), Some("Game::Player"));
        assert_eq!(result.normalized_references.len(), 1);
        assert_eq!(
            result.normalized_references[0],
            NormalizedReference {
                source_symbol_id: "Game::Player".into(),
                target_symbol_id: "Game::Actor".into(),
                category: ReferenceCategory::InheritanceMention,
                file_path: "inheritance.cpp".into(),
                line: 4,
                confidence: RawExtractionConfidence::Partial,
            }
        );
    }

    #[test]
    fn emits_multiple_inheritance_references_for_interfaces_and_abstract_bases() {
        let src = r#"
namespace Game {
class ISystem {
public:
    virtual void Tick() = 0;
};

class Actor {
public:
    virtual void Update() = 0;
};

class SystemAdapter : public ISystem {};
class Player : public Actor {};
class Enemy : public Actor {};
}
"#;
        let result = parse_cpp_file("hierarchy.cpp", src).unwrap();
        let inheritance_refs: Vec<_> = result
            .normalized_references
            .iter()
            .filter(|reference| reference.category == ReferenceCategory::InheritanceMention)
            .collect();

        assert_eq!(inheritance_refs.len(), 3);
        assert!(inheritance_refs.iter().any(|reference| {
            reference.source_symbol_id == "Game::SystemAdapter"
                && reference.target_symbol_id == "Game::ISystem"
        }));
        assert!(inheritance_refs.iter().any(|reference| {
            reference.source_symbol_id == "Game::Player"
                && reference.target_symbol_id == "Game::Actor"
        }));
        assert!(inheritance_refs.iter().any(|reference| {
            reference.source_symbol_id == "Game::Enemy"
                && reference.target_symbol_id == "Game::Actor"
        }));
    }

    #[test]
    fn inheritance_normalization_rejects_constructor_like_targets() {
        let symbols = vec![
            Symbol {
                id: "Game::ThreadPoolProvider".into(),
                name: "ThreadPoolProvider".into(),
                qualified_name: "Game::ThreadPoolProvider".into(),
                symbol_type: "class".into(),
                file_path: "provider.h".into(),
                line: 1,
                end_line: 20,
                signature: None,
                parameter_count: None,
                scope_qualified_name: Some("Game".into()),
                scope_kind: Some("namespace".into()),
                symbol_role: Some("definition".into()),
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
                artifact_kind: None,
                header_role: None,
                parse_fragility: None,
                macro_sensitivity: None,
                include_heaviness: None,
            },
            Symbol {
                id: "Game::ThreadPoolProvider::ThreadPoolProvider".into(),
                name: "ThreadPoolProvider".into(),
                qualified_name: "Game::ThreadPoolProvider::ThreadPoolProvider".into(),
                symbol_type: "method".into(),
                file_path: "provider.cpp".into(),
                line: 22,
                end_line: 22,
                signature: Some("ThreadPoolProvider()".into()),
                parameter_count: Some(0),
                scope_qualified_name: Some("Game::ThreadPoolProvider".into()),
                scope_kind: Some("class".into()),
                symbol_role: Some("definition".into()),
                declaration_file_path: None,
                declaration_line: None,
                declaration_end_line: None,
                definition_file_path: None,
                definition_line: None,
                definition_end_line: None,
                parent_id: Some("Game::ThreadPoolProvider".into()),
                module: None,
                subsystem: None,
                project_area: None,
                artifact_kind: None,
                header_role: None,
                parse_fragility: None,
                macro_sensitivity: None,
                include_heaviness: None,
            },
            Symbol {
                id: "Game::WorkerPool".into(),
                name: "WorkerPool".into(),
                qualified_name: "Game::WorkerPool".into(),
                symbol_type: "class".into(),
                file_path: "worker_pool.h".into(),
                line: 30,
                end_line: 50,
                signature: None,
                parameter_count: None,
                scope_qualified_name: Some("Game".into()),
                scope_kind: Some("namespace".into()),
                symbol_role: Some("definition".into()),
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
                artifact_kind: None,
                header_role: None,
                parse_fragility: None,
                macro_sensitivity: None,
                include_heaviness: None,
            },
        ];
        let relation_events = vec![RawRelationEvent {
            relation_kind: RawRelationKind::Inheritance,
            source: RawEventSource::LegacyAst,
            confidence: RawExtractionConfidence::Partial,
            caller_id: Some("Game::WorkerPool".into()),
            target_name: Some("Game::ThreadPoolProvider::ThreadPoolProvider".into()),
            call_kind: None,
            argument_count: None,
            receiver: None,
            receiver_kind: None,
            qualifier: Some("Game::ThreadPoolProvider::ThreadPoolProvider".into()),
            qualifier_kind: Some(RawQualifierKind::Type),
            file_path: "worker_pool.h".into(),
            line: 30,
        }];

        let normalized = normalize_relation_events(&relation_events, &symbols);

        assert!(normalized.is_empty());
    }

    #[test]
    fn ambiguity_fixtures_prefer_graph_sourced_call_relation_events() {
        let fixtures = [
            (
                "samples/ambiguity/src/namespace_dupes.cpp",
                include_str!("../../samples/ambiguity/src/namespace_dupes.cpp"),
            ),
            (
                "samples/ambiguity/src/overloads.cpp",
                include_str!("../../samples/ambiguity/src/overloads.cpp"),
            ),
            (
                "samples/ambiguity/src/sibling_methods.cpp",
                include_str!("../../samples/ambiguity/src/sibling_methods.cpp"),
            ),
            (
                "samples/ambiguity/src/split_update.cpp",
                include_str!("../../samples/ambiguity/src/split_update.cpp"),
            ),
        ];

        for (path, source) in fixtures {
            let result = parse_cpp_file(path, source).unwrap();
            let call_events: Vec<_> = result
                .relation_events
                .iter()
                .filter(|event| event.relation_kind == RawRelationKind::Call)
                .collect();
            assert!(!call_events.is_empty(), "expected call events for {}", path);
            assert!(
                call_events
                    .iter()
                    .all(|event| event.source == RawEventSource::TreeSitterGraph),
                "expected graph-sourced call events for {} but got {:?}",
                path,
                call_events
                    .iter()
                    .map(|event| relation_event_key(event))
                    .collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn complex_member_receiver_shape_is_supported_by_graph_calls() {
        let src = r#"
namespace Game {
class Worker {
public:
    void Update();
};

Worker MakeWorker();

void Tick() {
    MakeWorker().Update();
}
}
"#;
        let result = parse_cpp_file("unsupported_receiver.cpp", src).unwrap();
        assert!(result
            .raw_calls
            .iter()
            .any(|call| call.called_name == "MakeWorker" && matches!(call.call_kind, RawCallKind::Unqualified)));
        let update_call = result
            .raw_calls
            .iter()
            .find(|call| call.called_name == "Update")
            .unwrap();
        assert!(matches!(update_call.call_kind, RawCallKind::MemberAccess));
        assert_eq!(update_call.receiver.as_deref(), Some("MakeWorker()"));
        assert_eq!(update_call.receiver_kind, Some(RawReceiverKind::Other));
        assert!(result
            .relation_events
            .iter()
            .filter(|event| event.relation_kind == RawRelationKind::Call)
            .all(|event| event.source == RawEventSource::TreeSitterGraph));
        assert!(result
            .relation_events
            .iter()
            .any(|event| event.target_name.as_deref() == Some("Update")));
    }

    #[test]
    fn graph_integration_stays_tolerant_for_template_and_macro_bearing_code() {
        let src = r#"
#define DECLARE_WORKER(TypeName) class TypeName {};
DECLARE_WORKER(GeneratedWorker)

template <typename T>
class Holder {
public:
    T value;
};

class Base {};
class Derived : public Base {};

void Tick() {
    Holder<Derived> holder;
}
"#;
        let result = parse_cpp_file("macro_template.cpp", src).unwrap();
        assert!(result.symbols.iter().any(|symbol| symbol.name == "Holder"));
        assert!(result.symbols.iter().any(|symbol| symbol.name == "Derived"));
        assert!(result
            .relation_events
            .iter()
            .any(|event| event.relation_kind == RawRelationKind::Inheritance));
    }

    #[test]
    fn tolerates_macro_heavy_code() {
        let src = r#"
#define DECLARE_CLASS(name) class name##Impl {};
#ifdef SOME_FLAG
void conditionalFunc() {}
#endif
#if defined(OTHER_FLAG)
void otherFunc() {}
#else
void fallbackFunc() {}
#endif
void alwaysPresent() {}
"#;
        let result = parse_cpp_file("test.cpp", src).unwrap();
        assert!(result.symbols.iter().any(|s| s.name == "alwaysPresent"));
        assert_eq!(result.file_risk_signals.macro_sensitivity, MacroSensitivity::Low);
    }

    #[test]
    fn marks_include_heavy_file_risk_signals() {
        let includes = (0..16)
            .map(|index| format!("#include \"header{}.h\"", index))
            .collect::<Vec<_>>()
            .join("\n");
        let src = format!(
            "{}\nnamespace Demo {{\nclass Widget {{}};\n}}\n",
            includes
        );

        let result = parse_cpp_file("include_heavy.cpp", &src).unwrap();
        assert_eq!(result.file_risk_signals.include_heaviness, IncludeHeaviness::Heavy);
        assert_eq!(result.file_risk_signals.macro_sensitivity, MacroSensitivity::High);
    }

    #[test]
    fn marks_parse_fragility_when_tree_contains_errors() {
        let src = "class Broken { void Tick( }";
        let result = parse_cpp_file("broken.cpp", src).unwrap();
        assert_eq!(result.file_risk_signals.parse_fragility, ParseFragility::Elevated);
    }

    #[test]
    fn per_file_failure_isolation() {
        let malformed = "class { void broken( {}";
        let result = parse_cpp_file("bad.cpp", malformed);
        assert!(result.is_ok());
    }

    #[test]
    fn parses_deep_nested_namespace() {
        let src = r#"
namespace EA {
namespace Ant {
namespace Controllers {
class ControllerAsset {
public:
    void Load();
};
}}}
"#;
        let result = parse_cpp_file("test.h", src).unwrap();
        let class = result.symbols.iter().find(|s| s.name == "ControllerAsset").unwrap();
        assert_eq!(class.id, "EA::Ant::Controllers::ControllerAsset");
        let method = result.symbols.iter().find(|s| s.name == "Load").unwrap();
        assert_eq!(method.parent_id.as_deref(), Some("EA::Ant::Controllers::ControllerAsset"));
    }

    #[test]
    fn parses_member_call_shapes() {
        let src = r#"
namespace Game {
class Worker {
public:
    void Update();
    void Tick(Worker* other) {
        this->Update();
        other->Update();
    }
};
}
"#;
        let result = parse_cpp_file("test.cpp", src).unwrap();
        let this_call = result
            .raw_calls
            .iter()
            .find(|c| matches!(c.call_kind, RawCallKind::ThisPointerAccess))
            .unwrap();
        assert_eq!(this_call.called_name, "Update");
        assert_eq!(this_call.receiver.as_deref(), Some("this"));
        assert_eq!(this_call.receiver_kind, Some(RawReceiverKind::This));

        let pointer_call = result
            .raw_calls
            .iter()
            .find(|c| matches!(c.call_kind, RawCallKind::PointerMemberAccess))
            .unwrap();
        assert_eq!(pointer_call.called_name, "Update");
        assert_eq!(pointer_call.receiver.as_deref(), Some("other"));
        assert_eq!(pointer_call.receiver_kind, Some(RawReceiverKind::Identifier));
    }

    #[test]
    fn parses_dot_member_call_shape() {
        let src = r#"
namespace Game {
class Worker {
public:
    void Update();
};

void Tick() {
    Worker local;
    local.Update();
}
}
"#;
        let result = parse_cpp_file("test.cpp", src).unwrap();
        let member_call = result
            .raw_calls
            .iter()
            .find(|c| matches!(c.call_kind, RawCallKind::MemberAccess))
            .unwrap();
        assert_eq!(member_call.called_name, "Update");
        assert_eq!(member_call.argument_count, Some(0));
        assert_eq!(member_call.receiver.as_deref(), Some("local"));
        assert_eq!(member_call.receiver_kind, Some(RawReceiverKind::Identifier));
    }

    #[test]
    fn parses_namespace_qualified_call_shape() {
        let src = r#"
namespace Gameplay {
void Update();
}

namespace AI {
class Controller {
public:
    void Tick() {
        Gameplay::Update();
    }
};
}
"#;
        let result = parse_cpp_file("test.cpp", src).unwrap();
        let qualified_call = result
            .raw_calls
            .iter()
            .find(|c| matches!(c.call_kind, RawCallKind::Qualified))
            .unwrap();
        assert_eq!(qualified_call.called_name, "Update");
        assert_eq!(qualified_call.argument_count, Some(0));
        assert_eq!(qualified_call.qualifier.as_deref(), Some("Gameplay"));
        assert_eq!(qualified_call.qualifier_kind, Some(RawQualifierKind::Namespace));
    }

    #[test]
    fn parses_type_qualified_call_shape() {
        let src = r#"
namespace Game {
class Worker {
public:
    static void Update();
};

void Tick() {
    Worker::Update();
}
}
"#;
        let result = parse_cpp_file("test.cpp", src).unwrap();
        let qualified_call = result
            .raw_calls
            .iter()
            .find(|c| matches!(c.call_kind, RawCallKind::Qualified))
            .unwrap();
        assert_eq!(qualified_call.called_name, "Update");
        assert_eq!(qualified_call.argument_count, Some(0));
        assert_eq!(qualified_call.qualifier.as_deref(), Some("Worker"));
        assert_eq!(qualified_call.qualifier_kind, Some(RawQualifierKind::Type));
    }

    #[test]
    fn parses_parameter_and_argument_counts() {
        let src = r#"
namespace Math {
void Blend(int a);
void Compose(float a, float b);

void Tick() {
    Blend(1);
    Compose(1.0f, 2.0f);
}
}
"#;
        let result = parse_cpp_file("test.cpp", src).unwrap();
        let one_arg = result
            .symbols
            .iter()
            .find(|s| s.signature.as_deref() == Some("void Blend(int a)"))
            .unwrap();
        let two_arg = result
            .symbols
            .iter()
            .find(|s| s.signature.as_deref() == Some("void Compose(float a, float b)"))
            .unwrap();
        assert_eq!(one_arg.parameter_count, Some(1));
        assert_eq!(two_arg.parameter_count, Some(2));

        assert!(result
            .raw_calls
            .iter()
            .any(|c| c.called_name == "Blend" && c.argument_count == Some(1)));
        assert!(result
            .raw_calls
            .iter()
            .any(|c| c.called_name == "Compose" && c.argument_count == Some(2)));
    }

    #[test]
    fn tags_free_function_declaration_and_definition_roles() {
        let decl_src = "void Tick();";
        let decl_result = parse_cpp_file("test.h", decl_src).unwrap();
        let decl = decl_result.symbols.iter().find(|s| s.name == "Tick").unwrap();
        assert_eq!(decl.symbol_role.as_deref(), Some("declaration"));

        let def_src = "void Tick() {}";
        let def_result = parse_cpp_file("test.cpp", def_src).unwrap();
        let def = def_result.symbols.iter().find(|s| s.name == "Tick").unwrap();
        assert_eq!(def.symbol_role.as_deref(), Some("definition"));
    }

    #[test]
    fn tags_method_declaration_definition_and_inline_definition_roles() {
        let decl_src = r#"
namespace Game {
class Worker {
public:
    void Update();
};
}
"#;
        let decl_result = parse_cpp_file("worker.h", decl_src).unwrap();
        let decl = decl_result
            .symbols
            .iter()
            .find(|s| s.id == "Game::Worker::Update")
            .unwrap();
        assert_eq!(decl.symbol_role.as_deref(), Some("declaration"));

        let def_src = r#"
namespace Game {
void Worker::Update() {}
}
"#;
        let def_result = parse_cpp_file("worker.cpp", def_src).unwrap();
        let def = def_result
            .symbols
            .iter()
            .find(|s| s.id == "Game::Worker::Update")
            .unwrap();
        assert_eq!(def.symbol_role.as_deref(), Some("definition"));

        let inline_src = r#"
namespace Game {
class Worker {
public:
    void Update() {}
};
}
"#;
        let inline_result = parse_cpp_file("worker_inline.h", inline_src).unwrap();
        let inline = inline_result
            .symbols
            .iter()
            .find(|s| s.id == "Game::Worker::Update")
            .unwrap();
        assert_eq!(inline.symbol_role.as_deref(), Some("inline_definition"));
    }

    #[test]
    fn ambiguity_namespace_fixture_parses_qualified_calls() {
        let src = include_str!("../../samples/ambiguity/src/namespace_dupes.cpp");
        let result = parse_cpp_file("samples/ambiguity/src/namespace_dupes.cpp", src).unwrap();

        let controller = result
            .symbols
            .iter()
            .find(|s| s.id == "AI::Controller::Update")
            .unwrap();
        assert_eq!(controller.symbol_role.as_deref(), Some("definition"));

        let qualified_calls: Vec<_> = result
            .raw_calls
            .iter()
            .filter(|c| matches!(c.call_kind, RawCallKind::Qualified))
            .collect();
        assert_eq!(qualified_calls.len(), 2);
        assert!(qualified_calls.iter().any(|c| {
            c.called_name == "Update"
                && c.qualifier.as_deref() == Some("Gameplay")
                && c.qualifier_kind == Some(RawQualifierKind::Namespace)
        }));
        assert!(qualified_calls.iter().any(|c| {
            c.called_name == "Update"
                && c.qualifier.as_deref() == Some("UI")
                && c.qualifier_kind == Some(RawQualifierKind::Namespace)
        }));
    }

    #[test]
    fn ambiguity_overloads_fixture_parses_arity_metadata() {
        let header = include_str!("../../samples/ambiguity/src/overloads.h");
        let header_result = parse_cpp_file("samples/ambiguity/src/overloads.h", header).unwrap();

        let decls: Vec<_> = header_result
            .symbols
            .iter()
            .filter(|s| s.name == "Blend")
            .collect();
        assert_eq!(decls.len(), 2);
        assert!(decls.iter().any(|s| {
            s.id == "Math::Blend"
                && s.parameter_count == Some(1)
                && s.symbol_role.as_deref() == Some("declaration")
        }));
        assert!(decls.iter().any(|s| {
            s.id == "Renderer::Blend"
                && s.parameter_count == Some(2)
                && s.symbol_role.as_deref() == Some("declaration")
        }));

        let source = include_str!("../../samples/ambiguity/src/overloads.cpp");
        let source_result = parse_cpp_file("samples/ambiguity/src/overloads.cpp", source).unwrap();
        let blend_call = source_result
            .raw_calls
            .iter()
            .find(|c| c.caller_id == "Renderer::Blend" && c.called_name == "Blend")
            .unwrap();
        assert_eq!(blend_call.argument_count, Some(1));
        assert!(matches!(blend_call.call_kind, RawCallKind::Qualified));
        assert_eq!(blend_call.qualifier.as_deref(), Some("Math"));
        assert_eq!(blend_call.qualifier_kind, Some(RawQualifierKind::Namespace));
    }

    #[test]
    fn ambiguity_sibling_methods_fixture_parses_this_calls() {
        let header = include_str!("../../samples/ambiguity/src/sibling_methods.h");
        let header_result = parse_cpp_file("samples/ambiguity/src/sibling_methods.h", header).unwrap();
        assert!(header_result.symbols.iter().any(|s| {
            s.id == "Game::Player::Process" && s.symbol_role.as_deref() == Some("declaration")
        }));
        assert!(header_result.symbols.iter().any(|s| {
            s.id == "Game::Enemy::Run" && s.symbol_role.as_deref() == Some("declaration")
        }));

        let source = include_str!("../../samples/ambiguity/src/sibling_methods.cpp");
        let source_result = parse_cpp_file("samples/ambiguity/src/sibling_methods.cpp", source).unwrap();
        let this_calls: Vec<_> = source_result
            .raw_calls
            .iter()
            .filter(|c| matches!(c.call_kind, RawCallKind::ThisPointerAccess))
            .collect();
        assert_eq!(this_calls.len(), 2);
        assert!(this_calls.iter().all(|c| c.called_name == "Run"));
        assert!(this_calls.iter().all(|c| c.receiver.as_deref() == Some("this")));
        assert!(this_calls.iter().all(|c| c.receiver_kind == Some(RawReceiverKind::This)));
    }

    #[test]
    fn ambiguity_split_update_fixture_parses_split_roles_and_pointer_calls() {
        let header = include_str!("../../samples/ambiguity/src/split_update.h");
        let header_result = parse_cpp_file("samples/ambiguity/src/split_update.h", header).unwrap();
        assert!(header_result.symbols.iter().any(|s| {
            s.id == "Game::Worker::Update"
                && s.symbol_role.as_deref() == Some("declaration")
                && s.parent_id.as_deref() == Some("Game::Worker")
        }));
        assert!(header_result.symbols.iter().any(|s| {
            s.id == "Game::Worker::Tick"
                && s.parameter_count == Some(1)
                && s.symbol_role.as_deref() == Some("declaration")
        }));

        let source = include_str!("../../samples/ambiguity/src/split_update.cpp");
        let source_result = parse_cpp_file("samples/ambiguity/src/split_update.cpp", source).unwrap();
        assert!(source_result.symbols.iter().any(|s| {
            s.id == "Game::Worker::Update" && s.symbol_role.as_deref() == Some("definition")
        }));

        let this_call = source_result
            .raw_calls
            .iter()
            .find(|c| matches!(c.call_kind, RawCallKind::ThisPointerAccess))
            .unwrap();
        assert_eq!(this_call.called_name, "Update");
        assert_eq!(this_call.receiver.as_deref(), Some("this"));

        let pointer_call = source_result
            .raw_calls
            .iter()
            .find(|c| matches!(c.call_kind, RawCallKind::PointerMemberAccess))
            .unwrap();
        assert_eq!(pointer_call.called_name, "Update");
        assert_eq!(pointer_call.receiver.as_deref(), Some("other"));
        assert_eq!(pointer_call.receiver_kind, Some(RawReceiverKind::Identifier));
    }

    #[test]
    fn field_access_extraction_and_dedup() {
        let source = r#"
class Foo {
public:
    int mValue;
    void doStuff();
};

void bar(Foo* ptr, Foo obj) {
    int x = ptr->mValue;
    int y = obj.mValue;
    ptr->doStuff();
    obj.doStuff();
}
"#;
        let result = parse_cpp_file("test_field_access.cpp", source).unwrap();

        // Field access raw calls should be present for standalone field reads
        let field_accesses: Vec<_> = result
            .raw_calls
            .iter()
            .filter(|c| {
                matches!(
                    c.call_kind,
                    RawCallKind::FieldAccess | RawCallKind::PointerFieldAccess | RawCallKind::ThisFieldAccess
                )
            })
            .collect();
        assert!(
            !field_accesses.is_empty(),
            "expected field access raw calls, found none"
        );

        // ptr->mValue should produce PointerFieldAccess
        let ptr_field = field_accesses
            .iter()
            .find(|c| c.called_name == "mValue" && c.receiver.as_deref() == Some("ptr"))
            .expect("expected PointerFieldAccess for ptr->mValue");
        assert!(matches!(ptr_field.call_kind, RawCallKind::PointerFieldAccess));

        // obj.mValue should produce FieldAccess
        let obj_field = field_accesses
            .iter()
            .find(|c| c.called_name == "mValue" && c.receiver.as_deref() == Some("obj"))
            .expect("expected FieldAccess for obj.mValue");
        assert!(matches!(obj_field.call_kind, RawCallKind::FieldAccess));

        // Method calls should still be member/pointer_member, NOT field access
        let method_calls: Vec<_> = result
            .raw_calls
            .iter()
            .filter(|c| c.called_name == "doStuff")
            .collect();
        assert!(
            method_calls.len() >= 2,
            "expected at least 2 method calls for doStuff"
        );
        for mc in &method_calls {
            assert!(
                matches!(mc.call_kind, RawCallKind::MemberAccess | RawCallKind::PointerMemberAccess),
                "doStuff() should be MemberAccess/PointerMemberAccess, got {:?}",
                mc.call_kind
            );
        }

        // Deduplication: no field access event at the same line as a method call for doStuff
        let dedup_violations: Vec<_> = field_accesses
            .iter()
            .filter(|fa| fa.called_name == "doStuff")
            .collect();
        assert!(
            dedup_violations.is_empty(),
            "field access events should be deduplicated against method calls: {:?}",
            dedup_violations
        );
    }

    #[test]
    fn this_field_access_extraction() {
        let source = r#"
class Widget {
public:
    int mSize;
    void update() {
        int s = this->mSize;
    }
};
"#;
        let result = parse_cpp_file("test_this_field.cpp", source).unwrap();

        let this_field: Vec<_> = result
            .raw_calls
            .iter()
            .filter(|c| matches!(c.call_kind, RawCallKind::ThisFieldAccess))
            .collect();
        assert!(
            !this_field.is_empty(),
            "expected ThisFieldAccess for this->mSize"
        );
        assert_eq!(this_field[0].called_name, "mSize");
        assert_eq!(this_field[0].receiver.as_deref(), Some("this"));
    }

    fn extract_type_usage_events(source: &str) -> Vec<RawRelationEvent> {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_cpp::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();
        let mut ctx = Ctx {
            file_path: "test_type_usage.cpp".to_string(),
            source: source.as_bytes().to_vec(),
            symbols: Vec::new(),
            raw_calls: Vec::new(),
            ns_stack: Vec::new(),
            namespace_ids: HashSet::new(),
        };
        visit_tree(tree.root_node(), &mut ctx);
        extract_type_usage_relation_events(tree.root_node(), &ctx)
    }

    #[test]
    fn template_type_argument_extraction() {
        let source = r#"
class ShotFlags {};
class MyVec {};

void foo(MyVec<ShotFlags> param);
ShotFlags bar();
void baz(MyVec<MyVec<ShotFlags>> nested);
"#;
        let events = extract_type_usage_events(source);
        let type_names: Vec<String> = events
            .iter()
            .filter(|e| e.relation_kind == RawRelationKind::TypeUsage)
            .filter_map(|e| e.target_name.clone())
            .collect();

        assert!(
            type_names.contains(&"ShotFlags".to_string()),
            "ShotFlags should appear as typeUsage, got: {:?}",
            type_names
        );
        assert!(
            type_names.contains(&"MyVec".to_string()),
            "MyVec should appear as typeUsage, got: {:?}",
            type_names
        );
        let sf_count = type_names.iter().filter(|n| *n == "ShotFlags").count();
        assert!(
            sf_count >= 3,
            "ShotFlags should appear at least 3 times (param, return, nested), got {}",
            sf_count
        );
    }

    #[test]
    fn return_type_in_function_definition() {
        let source = r#"
class Widget {};

Widget createWidget() {
    return Widget();
}
"#;
        let events = extract_type_usage_events(source);
        let type_names: Vec<String> = events
            .iter()
            .filter(|e| e.relation_kind == RawRelationKind::TypeUsage)
            .filter_map(|e| e.target_name.clone())
            .collect();

        assert!(
            type_names.contains(&"Widget".to_string()),
            "Widget return type should be captured as typeUsage, got: {:?}",
            type_names
        );
    }
}
