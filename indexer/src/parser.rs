use std::collections::{HashMap, HashSet};

use tree_sitter::{Node, Parser, Tree};
use tree_sitter_graph::ast::File as GraphDslFile;
use tree_sitter_graph::functions::Functions as GraphFunctions;
use tree_sitter_graph::graph::Value as GraphValue;
use tree_sitter_graph::{ExecutionConfig as GraphExecutionConfig, Identifier as GraphIdentifier};
use tree_sitter_graph::{NoCancellation, Variables as GraphVariables};
use crate::graph_rules::CPP_CALL_RELATIONS;
use crate::models::{
    CallableFlowSummary, NormalizedReference, ParseResult, PropagationAnchor,
    PropagationAnchorKind, PropagationEvent, PropagationKind, PropagationRisk,
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

pub fn parse_cpp_file(file_path: &str, source: &str) -> Result<ParseResult, String> {
    let mut parser = Parser::new();
    let lang = tree_sitter_cpp::LANGUAGE;
    parser
        .set_language(&lang.into())
        .map_err(|e| format!("Failed to set language: {}", e))?;

    let tree: Tree = parser
        .parse(source, None)
        .ok_or_else(|| "Failed to parse".to_string())?;

    let mut ctx = Ctx {
        file_path: file_path.to_string(),
        source: source.as_bytes().to_vec(),
        symbols: Vec::new(),
        raw_calls: Vec::new(),
        ns_stack: Vec::new(),
        namespace_ids: HashSet::new(),
    };

    visit_tree(tree.root_node(), &mut ctx);

    let (propagation_events, callable_flow_summaries) =
        extract_local_propagation_data(tree.root_node(), &ctx);
    let graph_relation_events = extract_graph_relation_events(file_path, source, &tree, &ctx);
    let legacy_raw_calls = ctx.raw_calls;
    let legacy_relation_events: Vec<RawRelationEvent> = legacy_raw_calls
        .iter()
        .map(|raw_call| RawRelationEvent::from_raw_call_site(raw_call, RawEventSource::LegacyAst))
        .collect();
    let graph_raw_calls: Vec<RawCallSite> = graph_relation_events
        .iter()
        .filter_map(RawRelationEvent::to_raw_call_site)
        .collect();

    let (relation_events, raw_calls) = if graph_matches_legacy_calls(&graph_raw_calls, &legacy_raw_calls)
    {
        (
            graph_relation_events,
            enrich_graph_raw_calls_with_legacy_details(graph_raw_calls, &legacy_raw_calls),
        )
    } else {
        let mut mixed_relation_events = legacy_relation_events;
        mixed_relation_events.extend(
            graph_relation_events
                .into_iter()
                .filter(|event| event.relation_kind != RawRelationKind::Call),
        );
        (mixed_relation_events, legacy_raw_calls)
    };
    let normalized_references = extract_normalized_references(&relation_events, &ctx.symbols);

    Ok(ParseResult {
        symbols: ctx.symbols,
        relation_events,
        normalized_references,
        propagation_events,
        callable_flow_summaries,
        raw_calls,
    })
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

pub fn cpp_call_graph_rule_source() -> &'static str {
    CPP_CALL_RELATIONS
}

fn extract_graph_relation_events(
    file_path: &str,
    source: &str,
    tree: &Tree,
    ctx: &Ctx,
) -> Vec<RawRelationEvent> {
    let graph_file = match GraphDslFile::from_str(tree_sitter_cpp::LANGUAGE.into(), CPP_CALL_RELATIONS) {
        Ok(file) => file,
        Err(_) => return Vec::new(),
    };

    let functions = GraphFunctions::stdlib();
    let mut globals = GraphVariables::new();
    if globals
        .add(GraphIdentifier::from("filepath"), file_path.into())
        .is_err()
    {
        return Vec::new();
    }

    let config = GraphExecutionConfig::new(&functions, &globals).lazy(true);
    let graph = match graph_file.execute(tree, source, &config, &NoCancellation) {
        Ok(graph) => graph,
        Err(_) => return Vec::new(),
    };

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
        let event = RawRelationEvent {
            relation_kind: relation_kind.clone(),
            source: match read_attr_str(attrs, "extraction_source") {
                Some("tree_sitter_graph") => RawEventSource::TreeSitterGraph,
                _ => RawEventSource::LegacyAst,
            },
            confidence: match read_attr_str(attrs, "confidence") {
                Some("partial") => RawExtractionConfidence::Partial,
                _ => RawExtractionConfidence::High,
            },
            caller_id: if relation_kind == RawRelationKind::Call {
                read_enclosing_caller_id(tree.root_node(), line, ctx)
            } else {
                read_enclosing_symbol_id(line, ctx)
            },
            target_name,
            call_kind: read_attr_str(attrs, "call_kind").and_then(parse_call_kind),
            argument_count: read_attr_u32(attrs, "argument_count").map(|value| value as usize),
            receiver: read_attr_string(attrs, "receiver"),
            receiver_kind: graph_receiver_kind(
                read_attr_string(attrs, "receiver"),
                read_attr_str(attrs, "receiver_kind").and_then(parse_receiver_kind),
            ),
            qualifier,
            qualifier_kind,
            file_path: read_attr_string(attrs, "file_path")
                .unwrap_or_else(|| file_path.to_string()),
            line,
        };
        events.push(event);
    }

    events
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

fn extract_normalized_references(
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
}

fn extract_local_propagation_data(
    root: Node,
    ctx: &Ctx,
) -> (Vec<PropagationEvent>, Vec<CallableFlowSummary>) {
    let mut events = Vec::new();
    let mut summaries = Vec::new();
    let mut stack = vec![root];

    while let Some(node) = stack.pop() {
        if node.kind() == "function_definition" {
            if let Some(extraction) = extract_local_propagation_events_for_function(node, ctx) {
                events.extend(extraction.events);
                summaries.push(extraction.summary);
            }
            continue;
        }

        for child in named_children(node) {
            stack.push(child);
        }
    }

    (events, summaries)
}

fn extract_local_propagation_events_for_function(
    node: Node,
    ctx: &Ctx,
) -> Option<FunctionPropagationExtraction> {
    let owner_symbol_id = match find_function_owner_symbol_id(node, ctx) {
        Some(id) => id,
        None => return None,
    };
    let body = match node.child_by_field_name("body") {
        Some(body) if body.kind() == "compound_statement" => body,
        _ => return None,
    };

    let mut scopes = vec![HashMap::new()];
    let parameter_anchors = seed_parameter_bindings(node, ctx, &owner_symbol_id, &mut scopes[0]);

    let mut events = Vec::new();
    let mut return_anchors = Vec::new();
    process_local_flow_block(body, ctx, &owner_symbol_id, &mut scopes, &mut events, false);
    collect_return_anchors(body, ctx, &owner_symbol_id, &scopes[0], &mut return_anchors);

    Some(FunctionPropagationExtraction {
        events,
        summary: CallableFlowSummary {
            callable_symbol_id: owner_symbol_id,
            parameter_anchors,
            return_anchors,
        },
    })
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
    create_scope: bool,
) {
    if create_scope {
        scopes.push(HashMap::new());
    }

    for child in named_children(block) {
        process_local_flow_node(child, ctx, owner_symbol_id, scopes, events);
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
) {
    match node.kind() {
        "compound_statement" => {
            process_local_flow_block(node, ctx, owner_symbol_id, scopes, events, true);
        }
        "declaration" => process_local_declaration(node, ctx, owner_symbol_id, scopes, events),
        "return_statement" => process_return_statement(node, ctx, owner_symbol_id, scopes, events),
        "expression_statement" => {
            if let Some(expression) = named_children(node).into_iter().next() {
                let _ = extract_expression_flow(expression, ctx, owner_symbol_id, scopes, events);
            }
        }
        _ => {
            for child in named_children(node) {
                process_local_flow_node(child, ctx, owner_symbol_id, scopes, events);
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
    let Some(expression) = named_children(node).into_iter().next() else {
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

    for child in named_children(block) {
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
                if let Some(expression) = named_children(child).into_iter().next() {
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
    for child in named_children(node) {
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

    for child in named_children(node) {
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
        "parenthesized_expression" => named_children(node)
            .into_iter()
            .next()
            .and_then(|child| extract_expression_flow(child, ctx, owner_symbol_id, scopes, events)),
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
        "parenthesized_expression" => named_children(node)
            .into_iter()
            .next()
            .and_then(|child| resolve_assignment_target(child, ctx, owner_symbol_id, scopes)),
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

fn declaration_contains_function_declarator(node: Node) -> bool {
    named_children(node).into_iter().any(|child| {
        child.kind() == "function_declarator"
            || find_descendant(child, "function_declarator").is_some()
    })
}

fn extract_identifier_node(node: Node) -> Option<Node> {
    if node.kind() == "identifier" {
        return Some(node);
    }

    for child in named_children(node) {
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

    named_children(node)
        .into_iter()
        .any(is_pointer_like_declarator)
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

fn resolve_reference_target_id(
    event: &RawRelationEvent,
    source_symbol_id: &str,
    symbol_index: &HashMap<String, Vec<Symbol>>,
) -> Option<String> {
    let target_name = event.target_name.as_deref()?;
    let direct = symbol_index.get(target_name)?;
    if direct.len() == 1 {
        return Some(direct[0].id.clone());
    }

    let source_namespace = namespace_scope(source_symbol_id);
    let mut scored: Vec<(i32, &Symbol)> = direct
        .iter()
        .map(|symbol| {
            let mut score = 0;
            if symbol.id == target_name || symbol.qualified_name == target_name {
                score += 100;
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
    if scored.iter().take_while(|candidate| candidate.0 == best_score).count() > 1 {
        return None;
    }

    Some(best.1.id.clone())
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
        _ => None,
    }
}

fn parse_receiver_kind(value: &str) -> Option<RawReceiverKind> {
    match value {
        "identifier" => Some(RawReceiverKind::Identifier),
        "this" => Some(RawReceiverKind::This),
        "pointer_expression" => Some(RawReceiverKind::PointerExpression),
        "field_expression" => Some(RawReceiverKind::FieldExpression),
        "qualified_identifier" => Some(RawReceiverKind::QualifiedIdentifier),
        "other" => Some(RawReceiverKind::Other),
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

    ctx.symbols.push(Symbol {
        id: enum_id.clone(),
        name,
        qualified_name: enum_id,
        symbol_type: "enum".to_string(),
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
    });
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
    }

    #[test]
    fn emits_member_state_field_write_and_field_read_events() {
        let source = include_str!("../../samples/propagation/src/member_state.cpp");
        let result = parse_cpp_file("samples/propagation/src/member_state.cpp", source).unwrap();

        assert!(result.propagation_events.iter().any(|event| {
            event.owner_symbol_id.as_deref() == Some("Game::Worker::SetFromParam")
                && event.propagation_kind == PropagationKind::FieldWrite
                && event.source_anchor.anchor_id.as_deref() == Some("Game::Worker::SetFromParam::param:value@4")
                && event.target_anchor.anchor_id.as_deref() == Some("Game::Worker::field:stored")
                && event.confidence == RawExtractionConfidence::High
        }));
        assert!(result.propagation_events.iter().any(|event| {
            event.owner_symbol_id.as_deref() == Some("Game::Worker::SetFromLocal")
                && event.propagation_kind == PropagationKind::FieldWrite
                && event.source_anchor.anchor_id.as_deref() == Some("Game::Worker::SetFromLocal::local:local@9")
                && event.target_anchor.anchor_id.as_deref() == Some("Game::Worker::field:cached")
        }));
        assert!(result.propagation_events.iter().any(|event| {
            event.owner_symbol_id.as_deref() == Some("Game::Worker::ReadToLocal")
                && event.propagation_kind == PropagationKind::FieldRead
                && event.source_anchor.anchor_id.as_deref() == Some("Game::Worker::field:stored")
                && event.target_anchor.anchor_id.as_deref() == Some("Game::Worker::ReadToLocal::local:local@14")
        }));
        assert!(result.propagation_events.iter().any(|event| {
            event.owner_symbol_id.as_deref() == Some("Game::Worker::ReadMember")
                && event.propagation_kind == PropagationKind::FieldRead
                && event.source_anchor.anchor_id.as_deref() == Some("Game::Worker::field:cached")
                && event.target_anchor.anchor_id.as_deref() == Some("Game::Worker::ReadMember::return@19")
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
        assert_eq!(result.symbols.len(), 1);
        assert_eq!(result.symbols[0].name, "AIState");
        assert_eq!(result.symbols[0].symbol_type, "enum");
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
        assert!(rules.contains("global filepath"));
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
    fn emits_type_usage_relation_events_from_graph() {
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
        assert!(type_usage_events.iter().all(|event| event.source == RawEventSource::TreeSitterGraph));
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
    fn emits_inheritance_relation_events_from_graph() {
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
        assert_eq!(inheritance_events[0].source, RawEventSource::TreeSitterGraph);
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
}
