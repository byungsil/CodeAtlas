use std::collections::{HashMap, HashSet};

use crate::models::{
    Call, CallableFlowSummary, InheritanceEdge, OverrideCandidate, OverrideMatchReason,
    PropagationAnchor, PropagationAnchorKind, PropagationEvent, PropagationKind, PropagationRisk,
    RawCallKind, RawCallSite, RawExtractionConfidence, RawQualifierKind, Symbol,
};
use crate::storage::Database;

#[derive(Debug, Clone, PartialEq, Eq)]
struct RankingReason {
    kind: &'static str,
    score: i32,
}

#[derive(Debug, Clone)]
struct RankedCandidate<'a> {
    symbol: &'a Symbol,
    score: i32,
    reasons: Vec<RankingReason>,
}

#[derive(Debug, Clone)]
struct ResolutionDecision<'a> {
    ranked: Vec<RankedCandidate<'a>>,
    status: ResolutionStatus,
    chosen: Option<&'a Symbol>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ResolutionStatus {
    Resolved,
    Ambiguous,
    Unresolved,
}

#[derive(Debug, Clone, Copy)]
struct ResolutionContext<'a> {
    caller_parent: Option<&'a str>,
    caller_namespace: Option<&'a str>,
}

pub fn resolve_calls(raw_calls: &[RawCallSite], symbols: &[Symbol]) -> Vec<Call> {
    let by_name = build_callable_index(symbols);
    let parent_of = build_parent_index(symbols);

    let mut calls = Vec::new();
    let mut seen = HashSet::new();

    for raw in raw_calls {
        let candidates = collect_candidates(raw, &by_name);
        let context = ResolutionContext {
            caller_parent: parent_of.get(raw.caller_id.as_str()).copied(),
            caller_namespace: namespace_scope(raw.caller_id.as_str(), parent_of.get(raw.caller_id.as_str()).copied()),
        };
        let decision = resolve_one(raw, candidates, context);

        if let Some(callee) = decision.chosen {
            if callee.id == raw.caller_id {
                continue;
            }
            let key = format!("{}->{}@{}:{}", raw.caller_id, callee.id, raw.file_path, raw.line);
            if seen.insert(key) {
                calls.push(Call {
                    caller_id: raw.caller_id.clone(),
                    callee_id: callee.id.clone(),
                    file_path: raw.file_path.clone(),
                    line: raw.line,
                });
            }
        }
    }

    calls
}

fn build_callable_index<'a>(symbols: &'a [Symbol]) -> HashMap<&'a str, Vec<&'a Symbol>> {
    let mut map: HashMap<&str, Vec<&Symbol>> = HashMap::new();
    for sym in symbols {
        if sym.symbol_type == "function" || sym.symbol_type == "method" {
            map.entry(sym.name.as_str()).or_default().push(sym);
        }
    }
    map
}

fn build_parent_index<'a>(symbols: &'a [Symbol]) -> HashMap<&'a str, &'a str> {
    symbols
        .iter()
        .filter_map(|s| s.parent_id.as_deref().map(|p| (s.id.as_str(), p)))
        .collect()
}

fn collect_candidates<'a>(
    raw: &RawCallSite,
    by_name: &HashMap<&'a str, Vec<&'a Symbol>>,
) -> Vec<&'a Symbol> {
    by_name
        .get(raw.called_name.as_str())
        .cloned()
        .unwrap_or_default()
}

fn resolve_one<'a>(
    raw: &RawCallSite,
    candidates: Vec<&'a Symbol>,
    context: ResolutionContext<'_>,
) -> ResolutionDecision<'a> {
    if candidates.is_empty() {
        return ResolutionDecision {
            ranked: Vec::new(),
            status: ResolutionStatus::Unresolved,
            chosen: None,
        };
    }

    let ranked = score_candidates(raw, candidates, context);
    let (status, chosen) = tie_break(&ranked);

    ResolutionDecision {
        ranked,
        status,
        chosen,
    }
}

fn score_candidates<'a>(
    raw: &RawCallSite,
    candidates: Vec<&'a Symbol>,
    context: ResolutionContext<'_>,
) -> Vec<RankedCandidate<'a>> {
    let mut ranked: Vec<RankedCandidate<'a>> = candidates
        .into_iter()
        .map(|symbol| {
            let mut reasons = Vec::with_capacity(4);

            if !matches!(raw.call_kind, RawCallKind::Qualified) {
                if let Some(parent) = context.caller_parent {
                    if symbol.parent_id.as_deref() == Some(parent) {
                        reasons.push(RankingReason {
                            kind: "same_parent",
                            score: 100,
                        });
                    }
                }
            }

            if let Some(namespace) = context.caller_namespace {
                if candidate_namespace(symbol) == Some(namespace) {
                    reasons.push(RankingReason {
                        kind: "same_namespace",
                        score: 50,
                    });
                }
            }

            push_receiver_aware_reasons(raw, symbol, context, &mut reasons);

            push_arity_hint_reasons(raw, symbol, &mut reasons);

            let score = reasons.iter().map(|reason| reason.score).sum();

            RankedCandidate {
                symbol,
                score,
                reasons,
            }
        })
        .collect();

    ranked.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.symbol.id.cmp(&right.symbol.id))
    });

    ranked
}

fn tie_break<'a>(ranked: &[RankedCandidate<'a>]) -> (ResolutionStatus, Option<&'a Symbol>) {
    let first = match ranked.first() {
        Some(first) => first,
        None => return (ResolutionStatus::Unresolved, None),
    };

    let top_score = first.score;
    let top_count = ranked.iter().take_while(|candidate| candidate.score == top_score).count();

    if top_count > 1 {
        return (ResolutionStatus::Ambiguous, None);
    }

    (ResolutionStatus::Resolved, Some(first.symbol))
}

fn push_receiver_aware_reasons(
    raw: &RawCallSite,
    symbol: &Symbol,
    context: ResolutionContext<'_>,
    reasons: &mut Vec<RankingReason>,
) {
    match raw.call_kind {
        RawCallKind::ThisPointerAccess => {
            if let Some(parent) = context.caller_parent {
                if symbol.parent_id.as_deref() == Some(parent) {
                    reasons.push(RankingReason {
                        kind: "this_receiver_match",
                        score: 80,
                    });
                }
            }
        }
        RawCallKind::MemberAccess | RawCallKind::PointerMemberAccess => {
            if symbol.symbol_type == "method" {
                reasons.push(RankingReason {
                    kind: "member_call_prefers_method",
                    score: 30,
                });
            }
        }
        RawCallKind::Qualified => {
            if let Some(qualifier) = raw.qualifier.as_deref() {
                match raw.qualifier_kind {
                    Some(RawQualifierKind::Type) => {
                        if symbol.parent_id.as_deref() == Some(qualifier)
                            || type_name_of_parent(symbol.parent_id.as_deref()) == Some(qualifier)
                        {
                            reasons.push(RankingReason {
                                kind: "qualified_type_match",
                                score: 90,
                            });
                        }
                    }
                    Some(RawQualifierKind::Namespace) => {
                        if candidate_namespace(symbol) == Some(qualifier) {
                            reasons.push(RankingReason {
                                kind: "qualified_namespace_match",
                                score: 70,
                            });
                        }
                    }
                    None => {}
                }
            }
        }
        RawCallKind::Unqualified => {}
    }
}

fn push_arity_hint_reasons(
    raw: &RawCallSite,
    symbol: &Symbol,
    reasons: &mut Vec<RankingReason>,
) {
    let argument_count = match raw.argument_count {
        Some(count) => count,
        None => return,
    };

    if let Some(parameter_count) = symbol.parameter_count {
        if parameter_count == argument_count {
            reasons.push(RankingReason {
                kind: "parameter_count_match",
                score: 60,
            });
        }
        return;
    }

    if let Some(signature_arity) = infer_parameter_count_from_signature(symbol.signature.as_deref()) {
        if signature_arity == argument_count {
            reasons.push(RankingReason {
                kind: "signature_arity_hint",
                score: 40,
            });
        }
    }
}

pub fn merge_symbols(all_symbols: &[Symbol]) -> Vec<Symbol> {
    let mut by_id: HashMap<String, Symbol> = HashMap::new();

    for sym in all_symbols {
        let entry = by_id.entry(sym.id.clone());
        entry
            .and_modify(|existing| {
                merge_symbol_variant(existing, sym);
            })
            .or_insert_with(|| sym.clone());
    }

    by_id.into_values().collect()
}

fn merge_symbol_variant(existing: &mut Symbol, incoming: &Symbol) {
    merge_dual_locations(existing, incoming);

    if incoming_replaces_representative(existing, incoming) {
        existing.name = incoming.name.clone();
        existing.qualified_name = incoming.qualified_name.clone();
        existing.symbol_type = incoming.symbol_type.clone();
        existing.file_path = incoming.file_path.clone();
        existing.line = incoming.line;
        existing.end_line = incoming.end_line;
        existing.signature = incoming.signature.clone();
        existing.parameter_count = incoming.parameter_count;
        existing.scope_qualified_name = incoming.scope_qualified_name.clone();
        existing.scope_kind = incoming.scope_kind.clone();
        existing.symbol_role = incoming.symbol_role.clone();
        existing.parent_id = incoming.parent_id.clone();
        existing.module = incoming.module.clone();
        existing.subsystem = incoming.subsystem.clone();
        existing.project_area = incoming.project_area.clone();
        existing.artifact_kind = incoming.artifact_kind.clone();
        existing.header_role = incoming.header_role.clone();
    } else {
        if existing.signature.is_none() && incoming.signature.is_some() {
            existing.signature = incoming.signature.clone();
        }
        if existing.parameter_count.is_none() && incoming.parameter_count.is_some() {
            existing.parameter_count = incoming.parameter_count;
        }
        if existing.scope_qualified_name.is_none() && incoming.scope_qualified_name.is_some() {
            existing.scope_qualified_name = incoming.scope_qualified_name.clone();
        }
        if existing.scope_kind.is_none() && incoming.scope_kind.is_some() {
            existing.scope_kind = incoming.scope_kind.clone();
        }
        if existing.parent_id.is_none() && incoming.parent_id.is_some() {
            existing.parent_id = incoming.parent_id.clone();
        }
        if existing.module.is_none() && incoming.module.is_some() {
            existing.module = incoming.module.clone();
        }
        if existing.subsystem.is_none() && incoming.subsystem.is_some() {
            existing.subsystem = incoming.subsystem.clone();
        }
        if existing.project_area.is_none() && incoming.project_area.is_some() {
            existing.project_area = incoming.project_area.clone();
        }
        if existing.artifact_kind.is_none() && incoming.artifact_kind.is_some() {
            existing.artifact_kind = incoming.artifact_kind.clone();
        }
        if existing.header_role.is_none() && incoming.header_role.is_some() {
            existing.header_role = incoming.header_role.clone();
        }
    }
}

fn merge_dual_locations(existing: &mut Symbol, incoming: &Symbol) {
    if existing.declaration_file_path.is_none() && incoming.declaration_file_path.is_some() {
        existing.declaration_file_path = incoming.declaration_file_path.clone();
        existing.declaration_line = incoming.declaration_line;
        existing.declaration_end_line = incoming.declaration_end_line;
    }
    if existing.definition_file_path.is_none() && incoming.definition_file_path.is_some() {
        existing.definition_file_path = incoming.definition_file_path.clone();
        existing.definition_line = incoming.definition_line;
        existing.definition_end_line = incoming.definition_end_line;
    }

    match incoming.symbol_role.as_deref() {
        Some("declaration") => {
            if existing.declaration_file_path.is_none() {
                existing.declaration_file_path = Some(incoming.file_path.clone());
                existing.declaration_line = Some(incoming.line);
                existing.declaration_end_line = Some(incoming.end_line);
            }
        }
        Some("definition") => {
            existing.definition_file_path = Some(incoming.file_path.clone());
            existing.definition_line = Some(incoming.line);
            existing.definition_end_line = Some(incoming.end_line);
        }
        Some("inline_definition") => {
            if existing.definition_file_path.is_none() {
                existing.definition_file_path = Some(incoming.file_path.clone());
                existing.definition_line = Some(incoming.line);
                existing.definition_end_line = Some(incoming.end_line);
            }
        }
        _ => {
            if incoming.file_path.ends_with(".h") && existing.declaration_file_path.is_none() {
                existing.declaration_file_path = Some(incoming.file_path.clone());
                existing.declaration_line = Some(incoming.line);
                existing.declaration_end_line = Some(incoming.end_line);
            }
            if incoming.file_path.ends_with(".cpp") && existing.definition_file_path.is_none() {
                existing.definition_file_path = Some(incoming.file_path.clone());
                existing.definition_line = Some(incoming.line);
                existing.definition_end_line = Some(incoming.end_line);
            }
        }
    }
}

fn incoming_replaces_representative(existing: &Symbol, incoming: &Symbol) -> bool {
    let existing_rank = representative_rank(existing);
    let incoming_rank = representative_rank(incoming);

    if incoming_rank != existing_rank {
        return incoming_rank > existing_rank;
    }

    let existing_cpp = existing.file_path.ends_with(".cpp");
    let incoming_cpp = incoming.file_path.ends_with(".cpp");
    if incoming_cpp != existing_cpp {
        return incoming_cpp;
    }

    false
}

fn representative_rank(symbol: &Symbol) -> i32 {
    match symbol.symbol_role.as_deref() {
        Some("definition") => 3,
        Some("inline_definition") => 2,
        Some("declaration") => 1,
        _ => {
            if symbol.file_path.ends_with(".cpp") {
                2
            } else {
                1
            }
        }
    }
}

pub fn resolve_calls_with_db(raw_calls: &[RawCallSite], new_symbols: &[Symbol], db: &Database) -> Vec<Call> {
    let new_by_name = build_callable_index(new_symbols);
    let new_parent_of = build_parent_index(new_symbols);

    let mut caller_ids: Vec<String> = raw_calls
        .iter()
        .map(|raw| raw.caller_id.clone())
        .filter(|id| !new_parent_of.contains_key(id.as_str()))
        .collect();
    caller_ids.sort();
    caller_ids.dedup();

    let db_parent_of = db.find_parent_ids(&caller_ids).unwrap_or_default();

    let mut calls = Vec::new();
    let mut seen = HashSet::new();

    for raw in raw_calls {
        let caller_parent = new_parent_of
            .get(raw.caller_id.as_str())
            .copied()
            .or_else(|| db_parent_of.get(raw.caller_id.as_str()).map(|p| p.as_str()));

        let owned_candidates = collect_candidates_with_db(raw, &new_by_name, db);
        let candidates: Vec<&Symbol> = owned_candidates.iter().collect();
        let context = ResolutionContext {
            caller_parent,
            caller_namespace: namespace_scope(raw.caller_id.as_str(), caller_parent),
        };
        let decision = resolve_one(raw, candidates, context);

        if let Some(callee) = decision.chosen {
            if callee.id == raw.caller_id {
                continue;
            }
            let key = format!("{}->{}@{}:{}", raw.caller_id, callee.id, raw.file_path, raw.line);
            if seen.insert(key) {
                calls.push(Call {
                    caller_id: raw.caller_id.clone(),
                    callee_id: callee.id.clone(),
                    file_path: raw.file_path.clone(),
                    line: raw.line,
                });
            }
        }
    }

    calls
}

fn candidate_namespace<'a>(symbol: &'a Symbol) -> Option<&'a str> {
    namespace_scope(symbol.qualified_name.as_str(), symbol.parent_id.as_deref())
}

fn namespace_scope<'a>(qualified_id: &'a str, parent_id: Option<&'a str>) -> Option<&'a str> {
    if let Some(parent) = parent_id {
        return parent.rsplit_once("::").map(|(namespace, _)| namespace);
    }

    qualified_id.rsplit_once("::").map(|(namespace, _)| namespace)
}

fn type_name_of_parent(parent_id: Option<&str>) -> Option<&str> {
    parent_id
        .and_then(|parent| parent.rsplit_once("::").map(|(_, type_name)| type_name).or(Some(parent)))
}

fn infer_parameter_count_from_signature(signature: Option<&str>) -> Option<usize> {
    let signature = signature?;
    let start = signature.find('(')?;
    let end = signature.rfind(')')?;
    if end <= start {
        return None;
    }

    let params = signature[start + 1..end].trim();
    if params.is_empty() || params == "void" {
        return Some(0);
    }

    Some(params.split(',').count())
}

pub fn find_override_candidates(
    symbols: &[Symbol],
    inheritance_edges: &[InheritanceEdge],
) -> Vec<OverrideCandidate> {
    let methods_by_parent = build_methods_by_parent(symbols);
    let mut candidates = Vec::new();
    let mut seen = HashSet::new();

    for edge in inheritance_edges {
        let Some(derived_methods) = methods_by_parent.get(edge.derived_symbol_id.as_str()) else {
            continue;
        };
        let Some(base_methods) = methods_by_parent.get(edge.base_symbol_id.as_str()) else {
            continue;
        };

        for derived_method in derived_methods {
            for base_method in base_methods {
                if derived_method.name != base_method.name {
                    continue;
                }

                let mut reasons = vec![
                    OverrideMatchReason::InheritanceEdge,
                    OverrideMatchReason::MatchingMethodName,
                ];
                let mut confidence = RawExtractionConfidence::Partial;

                if derived_method.parameter_count.is_some()
                    && derived_method.parameter_count == base_method.parameter_count
                {
                    reasons.push(OverrideMatchReason::ParameterCountMatch);
                    confidence = RawExtractionConfidence::High;
                } else {
                    let derived_arity =
                        infer_parameter_count_from_signature(derived_method.signature.as_deref());
                    let base_arity =
                        infer_parameter_count_from_signature(base_method.signature.as_deref());
                    if derived_arity.is_some() && derived_arity == base_arity {
                        reasons.push(OverrideMatchReason::SignatureArityMatch);
                        confidence = RawExtractionConfidence::High;
                    }
                }

                let key = format!("{}->{}", derived_method.id, base_method.id);
                if seen.insert(key) {
                    candidates.push(OverrideCandidate {
                        derived_method_id: derived_method.id.clone(),
                        base_method_id: base_method.id.clone(),
                        confidence,
                        reasons,
                    });
                }
            }
        }
    }

    candidates.sort_by(|left, right| {
        left
            .derived_method_id
            .cmp(&right.derived_method_id)
            .then_with(|| left.base_method_id.cmp(&right.base_method_id))
    });

    candidates
}

pub fn derive_function_boundary_propagation_events(
    raw_calls: &[RawCallSite],
    calls: &[Call],
    callable_summaries: &[CallableFlowSummary],
    symbols: &[Symbol],
) -> Vec<PropagationEvent> {
    let mut raw_calls_by_site: HashMap<String, Vec<&RawCallSite>> = HashMap::new();
    for raw in raw_calls {
        raw_calls_by_site
            .entry(raw_call_site_key(&raw.caller_id, &raw.file_path, raw.line))
            .or_default()
            .push(raw);
    }
    let summary_by_callable: HashMap<&str, &CallableFlowSummary> = callable_summaries
        .iter()
        .map(|summary| (summary.callable_symbol_id.as_str(), summary))
        .collect();
    let symbol_by_id: HashMap<&str, &Symbol> = symbols
        .iter()
        .map(|symbol| (symbol.id.as_str(), symbol))
        .collect();

    let mut events = Vec::new();

    for call in calls {
        let key = raw_call_site_key(&call.caller_id, &call.file_path, call.line);
        let Some(raw_calls_at_site) = raw_calls_by_site.get(key.as_str()) else {
            continue;
        };
        let callee_name = symbol_by_id
            .get(call.callee_id.as_str())
            .map(|symbol| symbol.name.as_str());
        let Some(raw_call) = raw_calls_at_site
            .iter()
            .copied()
            .find(|raw| callee_name == Some(raw.called_name.as_str()))
            .or_else(|| raw_calls_at_site.first().copied())
        else {
            continue;
        };
        let Some(summary) = summary_by_callable.get(call.callee_id.as_str()) else {
            continue;
        };

        for (index, parameter_anchor) in summary.parameter_anchors.iter().enumerate() {
            let Some(argument_text) = raw_call.argument_texts.get(index) else {
                continue;
            };
            let source_anchor = PropagationAnchor {
                anchor_id: Some(format!(
                    "{}::arg{}@{}",
                    raw_call.caller_id,
                    index,
                    raw_call.line
                )),
                symbol_id: None,
                expression_text: Some(argument_text.clone()),
                anchor_kind: PropagationAnchorKind::Expression,
            };
            let (confidence, risks) = propagation_confidence_for_text(argument_text);
            events.push(PropagationEvent {
                owner_symbol_id: Some(call.callee_id.clone()),
                source_anchor,
                target_anchor: parameter_anchor.clone(),
                propagation_kind: PropagationKind::ArgumentToParameter,
                file_path: call.file_path.clone(),
                line: call.line,
                confidence,
                risks,
            });
        }

        if let Some(result_target) = raw_call.result_target.clone() {
            for return_anchor in &summary.return_anchors {
                let (confidence, risks) = propagation_confidence_for_anchor(return_anchor);
                events.push(PropagationEvent {
                    owner_symbol_id: Some(call.callee_id.clone()),
                    source_anchor: return_anchor.clone(),
                    target_anchor: result_target.clone(),
                    propagation_kind: PropagationKind::ReturnValue,
                    file_path: call.file_path.clone(),
                    line: call.line,
                    confidence,
                    risks,
                });
            }
        } else if symbol_by_id.contains_key(call.callee_id.as_str()) {
            continue;
        }
    }

    dedupe_propagation_events(events)
}

pub fn merge_propagation_events(
    local_events: &[PropagationEvent],
    boundary_events: &[PropagationEvent],
) -> Vec<PropagationEvent> {
    let mut combined = Vec::with_capacity(local_events.len() + boundary_events.len());
    combined.extend(local_events.iter().cloned());
    combined.extend(boundary_events.iter().cloned());
    dedupe_propagation_events(combined)
}

fn propagation_confidence_for_text(text: &str) -> (RawExtractionConfidence, Vec<PropagationRisk>) {
    if text.contains("->") || text.starts_with('&') || text.starts_with('*') {
        (
            RawExtractionConfidence::Partial,
            vec![PropagationRisk::PointerHeavyFlow],
        )
    } else {
        (RawExtractionConfidence::High, Vec::new())
    }
}

fn propagation_confidence_for_anchor(
    anchor: &PropagationAnchor,
) -> (RawExtractionConfidence, Vec<PropagationRisk>) {
    if anchor
        .expression_text
        .as_deref()
        .map(|text| text.contains("->") || text.starts_with('&') || text.starts_with('*'))
        .unwrap_or(false)
    {
        (
            RawExtractionConfidence::Partial,
            vec![PropagationRisk::PointerHeavyFlow],
        )
    } else {
        (RawExtractionConfidence::High, Vec::new())
    }
}

fn dedupe_propagation_events(events: Vec<PropagationEvent>) -> Vec<PropagationEvent> {
    let mut seen = HashSet::new();
    let mut deduped = Vec::new();

    for event in events {
        let key = format!(
            "{}|{}|{}|{}|{}|{}",
            event.owner_symbol_id.as_deref().unwrap_or_default(),
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
        );
        if seen.insert(key) {
            deduped.push(event);
        }
    }

    deduped
}

fn raw_call_site_key(caller_id: &str, file_path: &str, line: usize) -> String {
    format!("{}|{}|{}", caller_id, file_path, line)
}

fn build_methods_by_parent<'a>(symbols: &'a [Symbol]) -> HashMap<&'a str, Vec<&'a Symbol>> {
    let mut methods_by_parent: HashMap<&str, Vec<&Symbol>> = HashMap::new();
    for symbol in symbols {
        if symbol.symbol_type != "method" {
            continue;
        }
        let Some(parent_id) = symbol.parent_id.as_deref() else {
            continue;
        };
        methods_by_parent.entry(parent_id).or_default().push(symbol);
    }
    methods_by_parent
}

fn collect_candidates_with_db<'a>(
    raw: &RawCallSite,
    new_by_name: &HashMap<&'a str, Vec<&'a Symbol>>,
    db: &Database,
) -> Vec<Symbol> {
    let mut candidates: Vec<Symbol> = collect_candidates(raw, new_by_name)
        .into_iter()
        .cloned()
        .collect();
    let db_candidates = db.find_symbols_by_name(&raw.called_name).unwrap_or_default();

    for db_sym in db_candidates {
        if !candidates.iter().any(|candidate| candidate.id == db_sym.id) {
            candidates.push(db_sym);
        }
    }

    candidates
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{RawCallKind, RawQualifierKind};
    use crate::parser::parse_cpp_file;
    use std::path::Path;

    fn make_sym(id: &str, name: &str, stype: &str, parent: Option<&str>) -> Symbol {
        Symbol {
            id: id.to_string(),
            name: name.to_string(),
            qualified_name: id.to_string(),
            symbol_type: stype.to_string(),
            file_path: "test.cpp".to_string(),
            line: 1,
            end_line: 1,
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
            parent_id: parent.map(|p| p.to_string()),
            module: None,
            subsystem: None,
            project_area: None,
            artifact_kind: None,
            header_role: None,
            parse_fragility: None,
            macro_sensitivity: None,
            include_heaviness: None,
        }
    }

    fn make_raw(caller_id: &str, called_name: &str) -> RawCallSite {
        RawCallSite {
            caller_id: caller_id.to_string(),
            called_name: called_name.to_string(),
            call_kind: RawCallKind::Unqualified,
            argument_count: None,
            argument_texts: Vec::new(),
            result_target: None,
            receiver: None,
            receiver_kind: None,
            qualifier: None,
            qualifier_kind: None,
            file_path: "test.cpp".to_string(),
            line: 5,
        }
    }

    fn make_member_raw(caller_id: &str, called_name: &str, call_kind: RawCallKind) -> RawCallSite {
        RawCallSite {
            caller_id: caller_id.to_string(),
            called_name: called_name.to_string(),
            call_kind,
            argument_count: None,
            argument_texts: Vec::new(),
            result_target: None,
            receiver: Some("this".to_string()),
            receiver_kind: None,
            qualifier: None,
            qualifier_kind: None,
            file_path: "test.cpp".to_string(),
            line: 5,
        }
    }

    fn resolve_fixture_source(path: &str, source: &str) -> (Vec<Symbol>, Vec<RawCallSite>, Vec<Call>) {
        let parsed = parse_cpp_file(path, source).unwrap();
        let calls = resolve_calls(&parsed.raw_calls, &parsed.symbols);
        (parsed.symbols, parsed.raw_calls, calls)
    }

    #[test]
    fn derives_argument_and_return_boundary_propagation_events() {
        let source = include_str!("../../samples/propagation/src/function_boundary.cpp");
        let parsed = parse_cpp_file("samples/propagation/src/function_boundary.cpp", source).unwrap();
        let calls = resolve_calls(&parsed.raw_calls, &parsed.symbols);
        let events = derive_function_boundary_propagation_events(
            &parsed.raw_calls,
            &calls,
            &parsed.callable_flow_summaries,
            &parsed.symbols,
        );

        assert!(events.iter().any(|event| {
            event.propagation_kind == PropagationKind::ArgumentToParameter
                && event.owner_symbol_id.as_deref() == Some("Game::Consume")
                && event.source_anchor.expression_text.as_deref() == Some("current")
                && event.target_anchor.anchor_id.as_deref() == Some("Game::Consume::param:value@2")
        }));
        assert!(events.iter().any(|event| {
            event.propagation_kind == PropagationKind::ArgumentToParameter
                && event.owner_symbol_id.as_deref() == Some("Game::Forward")
                && event.source_anchor.expression_text.as_deref() == Some("current")
                && event.target_anchor.anchor_id.as_deref() == Some("Game::Forward::param:value@4")
        }));
        assert!(events.iter().any(|event| {
            event.propagation_kind == PropagationKind::ReturnValue
                && event.owner_symbol_id.as_deref() == Some("Game::Forward")
                && event.source_anchor.anchor_id.as_deref() == Some("Game::Forward::local:local@5")
                && event.target_anchor.anchor_id.as_deref() == Some("Game::Tick::local:out@12")
        }));
    }

    fn resolve_fixture_decision<'a>(
        raw_calls: &'a [RawCallSite],
        symbols: &'a [Symbol],
        caller_id: &str,
        called_name: &str,
        qualifier: Option<&str>,
    ) -> ResolutionDecision<'a> {
        let raw = raw_calls
            .iter()
            .find(|raw| {
                raw.caller_id == caller_id
                    && raw.called_name == called_name
                    && raw.qualifier.as_deref() == qualifier
            })
            .unwrap();
        let by_name = build_callable_index(symbols);
        let parent_of = build_parent_index(symbols);
        let candidates = collect_candidates(raw, &by_name);
        let caller_parent = parent_of.get(raw.caller_id.as_str()).copied();
        let context = ResolutionContext {
            caller_parent,
            caller_namespace: namespace_scope(raw.caller_id.as_str(), caller_parent),
        };
        resolve_one(raw, candidates, context)
    }

    #[test]
    fn resolves_simple_call() {
        let symbols = vec![
            make_sym("main", "main", "function", None),
            make_sym("foo", "foo", "function", None),
        ];
        let raw = vec![make_raw("main", "foo")];
        let calls = resolve_calls(&raw, &symbols);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].callee_id, "foo");
    }

    #[test]
    fn deduplicates_calls() {
        let symbols = vec![
            make_sym("main", "main", "function", None),
            make_sym("foo", "foo", "function", None),
        ];
        let raw = vec![make_raw("main", "foo"), make_raw("main", "foo")];
        let calls = resolve_calls(&raw, &symbols);
        assert_eq!(calls.len(), 1);
    }

    #[test]
    fn skips_self_calls() {
        let symbols = vec![make_sym("foo", "foo", "function", None)];
        let raw = vec![make_raw("foo", "foo")];
        let calls = resolve_calls(&raw, &symbols);
        assert_eq!(calls.len(), 0);
    }

    #[test]
    fn merge_prefers_cpp_over_h() {
        let syms = vec![
            Symbol {
                id: "Foo::Bar".into(),
                name: "Bar".into(),
                qualified_name: "Foo::Bar".into(),
                symbol_type: "method".into(),
                file_path: "foo.h".into(),
                line: 5,
                end_line: 5,
                signature: Some("void Bar()".into()),
                parameter_count: None,
                scope_qualified_name: None,
                scope_kind: None,
                symbol_role: Some("declaration".into()),
                declaration_file_path: Some("foo.h".into()),
                declaration_line: Some(5),
                declaration_end_line: Some(5),
                definition_file_path: None,
                definition_line: None,
                definition_end_line: None,
                parent_id: Some("Foo".into()),
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
                id: "Foo::Bar".into(),
                name: "Bar".into(),
                qualified_name: "Foo::Bar".into(),
                symbol_type: "method".into(),
                file_path: "foo.cpp".into(),
                line: 10,
                end_line: 15,
                signature: Some("void Foo::Bar()".into()),
                parameter_count: None,
                scope_qualified_name: None,
                scope_kind: None,
                symbol_role: Some("definition".into()),
                declaration_file_path: None,
                declaration_line: None,
                declaration_end_line: None,
                definition_file_path: Some("foo.cpp".into()),
                definition_line: Some(10),
                definition_end_line: Some(15),
                parent_id: Some("Foo".into()),
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
        let merged = merge_symbols(&syms);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].file_path, "foo.cpp");
        assert_eq!(merged[0].line, 10);
        assert_eq!(merged[0].symbol_role.as_deref(), Some("definition"));
        assert_eq!(merged[0].declaration_file_path.as_deref(), Some("foo.h"));
        assert_eq!(merged[0].definition_file_path.as_deref(), Some("foo.cpp"));
    }

    #[test]
    fn merge_combines_declaration_and_definition_locations() {
        let declaration = Symbol {
            id: "Game::Worker::Update".into(),
            name: "Update".into(),
            qualified_name: "Game::Worker::Update".into(),
            symbol_type: "method".into(),
            file_path: "worker.h".into(),
            line: 4,
            end_line: 4,
            signature: Some("void Update()".into()),
            parameter_count: Some(0),
            scope_qualified_name: None,
            scope_kind: None,
            symbol_role: Some("declaration".into()),
            declaration_file_path: Some("worker.h".into()),
            declaration_line: Some(4),
            declaration_end_line: Some(4),
            definition_file_path: None,
            definition_line: None,
            definition_end_line: None,
            parent_id: Some("Game::Worker".into()),
            module: None,
            subsystem: None,
            project_area: None,
            artifact_kind: None,
            header_role: None,
            parse_fragility: None,
            macro_sensitivity: None,
            include_heaviness: None,
        };
        let definition = Symbol {
            id: "Game::Worker::Update".into(),
            name: "Update".into(),
            qualified_name: "Game::Worker::Update".into(),
            symbol_type: "method".into(),
            file_path: "worker.cpp".into(),
            line: 12,
            end_line: 18,
            signature: Some("void Worker::Update()".into()),
            parameter_count: Some(0),
            scope_qualified_name: None,
            scope_kind: None,
            symbol_role: Some("definition".into()),
            declaration_file_path: None,
            declaration_line: None,
            declaration_end_line: None,
            definition_file_path: Some("worker.cpp".into()),
            definition_line: Some(12),
            definition_end_line: Some(18),
            parent_id: Some("Game::Worker".into()),
            module: None,
            subsystem: None,
            project_area: None,
            artifact_kind: None,
            header_role: None,
            parse_fragility: None,
            macro_sensitivity: None,
            include_heaviness: None,
        };

        let merged = merge_symbols(&[declaration, definition]);

        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].file_path, "worker.cpp");
        assert_eq!(merged[0].line, 12);
        assert_eq!(merged[0].declaration_file_path.as_deref(), Some("worker.h"));
        assert_eq!(merged[0].declaration_line, Some(4));
        assert_eq!(merged[0].definition_file_path.as_deref(), Some("worker.cpp"));
        assert_eq!(merged[0].definition_line, Some(12));
    }

    #[test]
    fn merge_prefers_definition_over_inline_definition() {
        let inline = Symbol {
            id: "Game::Worker::Tick".into(),
            name: "Tick".into(),
            qualified_name: "Game::Worker::Tick".into(),
            symbol_type: "method".into(),
            file_path: "worker.h".into(),
            line: 6,
            end_line: 8,
            signature: Some("void Tick()".into()),
            parameter_count: Some(0),
            scope_qualified_name: None,
            scope_kind: None,
            symbol_role: Some("inline_definition".into()),
            declaration_file_path: None,
            declaration_line: None,
            declaration_end_line: None,
            definition_file_path: Some("worker.h".into()),
            definition_line: Some(6),
            definition_end_line: Some(8),
            parent_id: Some("Game::Worker".into()),
            module: None,
            subsystem: None,
            project_area: None,
            artifact_kind: None,
            header_role: None,
            parse_fragility: None,
            macro_sensitivity: None,
            include_heaviness: None,
        };
        let definition = Symbol {
            id: "Game::Worker::Tick".into(),
            name: "Tick".into(),
            qualified_name: "Game::Worker::Tick".into(),
            symbol_type: "method".into(),
            file_path: "worker.cpp".into(),
            line: 20,
            end_line: 24,
            signature: Some("void Worker::Tick()".into()),
            parameter_count: Some(0),
            scope_qualified_name: None,
            scope_kind: None,
            symbol_role: Some("definition".into()),
            declaration_file_path: None,
            declaration_line: None,
            declaration_end_line: None,
            definition_file_path: Some("worker.cpp".into()),
            definition_line: Some(20),
            definition_end_line: Some(24),
            parent_id: Some("Game::Worker".into()),
            module: None,
            subsystem: None,
            project_area: None,
            artifact_kind: None,
            header_role: None,
            parse_fragility: None,
            macro_sensitivity: None,
            include_heaviness: None,
        };

        let merged = merge_symbols(&[inline, definition]);

        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].file_path, "worker.cpp");
        assert_eq!(merged[0].symbol_role.as_deref(), Some("definition"));
        assert_eq!(merged[0].definition_file_path.as_deref(), Some("worker.cpp"));
    }

    #[test]
    fn resolve_calls_with_db_uses_db_caller_parent_for_unchanged_callers() {
        let db = Database::open(Path::new(":memory:")).unwrap();
        db.write_symbols(&[
            make_sym("Alpha::Caller", "Caller", "method", Some("Alpha")),
            make_sym("Alpha::Target", "Target", "method", Some("Alpha")),
        ])
        .unwrap();

        let new_symbols = vec![make_sym("Beta::Target", "Target", "method", Some("Beta"))];

        let raw = vec![make_raw("Alpha::Caller", "Target")];

        let calls = resolve_calls_with_db(&raw, &new_symbols, &db);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].callee_id, "Alpha::Target");
    }

    #[test]
    fn same_parent_score_is_visible_in_ranking_decision() {
        let raw = make_raw("Alpha::Caller", "Target");
        let alpha = make_sym("Alpha::Target", "Target", "method", Some("Alpha"));
        let beta = make_sym("Beta::Target", "Target", "method", Some("Beta"));
        let candidates = vec![&beta, &alpha];
        let context = ResolutionContext {
            caller_parent: Some("Alpha"),
            caller_namespace: None,
        };

        let decision = resolve_one(&raw, candidates, context);

        assert_eq!(decision.chosen.map(|symbol| symbol.id.as_str()), Some("Alpha::Target"));
        assert_eq!(decision.status, ResolutionStatus::Resolved);
        assert_eq!(decision.ranked.len(), 2);
        assert_eq!(decision.ranked[0].symbol.id, "Alpha::Target");
        assert!(decision.ranked[0]
            .reasons
            .iter()
            .any(|reason| reason.kind == "same_parent" && reason.score == 100));
    }

    #[test]
    fn same_namespace_score_is_visible_in_ranking_decision() {
        let raw = make_raw("Gameplay::Tick", "Update");
        let gameplay = make_sym("Gameplay::Update", "Update", "function", None);
        let ui = make_sym("UI::Update", "Update", "function", None);
        let candidates = vec![&ui, &gameplay];
        let context = ResolutionContext {
            caller_parent: None,
            caller_namespace: Some("Gameplay"),
        };

        let decision = resolve_one(&raw, candidates, context);

        assert_eq!(decision.chosen.map(|symbol| symbol.id.as_str()), Some("Gameplay::Update"));
        assert_eq!(decision.status, ResolutionStatus::Resolved);
        assert_eq!(decision.ranked[0].symbol.id, "Gameplay::Update");
        assert!(decision.ranked[0]
            .reasons
            .iter()
            .any(|reason| reason.kind == "same_namespace" && reason.score == 50));
    }

    #[test]
    fn same_parent_outranks_same_namespace() {
        let raw = make_raw("Game::Player::Process", "Run");
        let sibling = make_sym("Game::Player::Run", "Run", "method", Some("Game::Player"));
        let same_namespace = make_sym("Game::Enemy::Run", "Run", "method", Some("Game::Enemy"));
        let candidates = vec![&same_namespace, &sibling];
        let context = ResolutionContext {
            caller_parent: Some("Game::Player"),
            caller_namespace: Some("Game"),
        };

        let decision = resolve_one(&raw, candidates, context);

        assert_eq!(decision.chosen.map(|symbol| symbol.id.as_str()), Some("Game::Player::Run"));
        assert_eq!(decision.status, ResolutionStatus::Resolved);
        assert_eq!(decision.ranked[0].score, 150);
        assert_eq!(decision.ranked[1].score, 50);
    }

    #[test]
    fn this_receiver_match_is_visible_in_ranking_decision() {
        let raw = make_member_raw("Game::Player::Process", "Run", RawCallKind::ThisPointerAccess);
        let sibling = make_sym("Game::Player::Run", "Run", "method", Some("Game::Player"));
        let other = make_sym("Game::Enemy::Run", "Run", "method", Some("Game::Enemy"));
        let candidates = vec![&other, &sibling];
        let context = ResolutionContext {
            caller_parent: Some("Game::Player"),
            caller_namespace: Some("Game"),
        };

        let decision = resolve_one(&raw, candidates, context);

        assert_eq!(decision.chosen.map(|symbol| symbol.id.as_str()), Some("Game::Player::Run"));
        assert_eq!(decision.status, ResolutionStatus::Resolved);
        assert!(decision.ranked[0]
            .reasons
            .iter()
            .any(|reason| reason.kind == "this_receiver_match" && reason.score == 80));
    }

    #[test]
    fn member_call_prefers_method_over_free_function() {
        let raw = RawCallSite {
            caller_id: "Game::Tick".to_string(),
            called_name: "Update".to_string(),
            call_kind: RawCallKind::MemberAccess,
            argument_count: Some(0),
            argument_texts: Vec::new(),
            result_target: None,
            receiver: Some("actor".to_string()),
            receiver_kind: None,
            qualifier: None,
            qualifier_kind: None,
            file_path: "test.cpp".to_string(),
            line: 5,
        };
        let method = make_sym("Game::Actor::Update", "Update", "method", Some("Game::Actor"));
        let function = make_sym("Game::Update", "Update", "function", None);
        let candidates = vec![&function, &method];
        let context = ResolutionContext {
            caller_parent: None,
            caller_namespace: Some("Game"),
        };

        let decision = resolve_one(&raw, candidates, context);

        assert_eq!(decision.chosen.map(|symbol| symbol.id.as_str()), Some("Game::Actor::Update"));
        assert_eq!(decision.status, ResolutionStatus::Resolved);
        assert!(decision.ranked[0]
            .reasons
            .iter()
            .any(|reason| reason.kind == "member_call_prefers_method" && reason.score == 30));
    }

    #[test]
    fn qualified_type_match_is_visible_in_ranking_decision() {
        let raw = RawCallSite {
            caller_id: "Game::Tick".to_string(),
            called_name: "Update".to_string(),
            call_kind: RawCallKind::Qualified,
            argument_count: Some(0),
            argument_texts: Vec::new(),
            result_target: None,
            receiver: None,
            receiver_kind: None,
            qualifier: Some("Worker".to_string()),
            qualifier_kind: Some(RawQualifierKind::Type),
            file_path: "test.cpp".to_string(),
            line: 5,
        };
        let method = make_sym("Game::Worker::Update", "Update", "method", Some("Game::Worker"));
        let function = make_sym("Game::Update", "Update", "function", None);
        let candidates = vec![&function, &method];
        let context = ResolutionContext {
            caller_parent: None,
            caller_namespace: Some("Game"),
        };

        let decision = resolve_one(&raw, candidates, context);

        assert_eq!(decision.chosen.map(|symbol| symbol.id.as_str()), Some("Game::Worker::Update"));
        assert_eq!(decision.status, ResolutionStatus::Resolved);
        assert!(decision.ranked[0]
            .reasons
            .iter()
            .any(|reason| reason.kind == "qualified_type_match" && reason.score == 90));
    }

    #[test]
    fn qualified_namespace_match_is_visible_in_ranking_decision() {
        let raw = RawCallSite {
            caller_id: "AI::Controller::Update".to_string(),
            called_name: "Update".to_string(),
            call_kind: RawCallKind::Qualified,
            argument_count: Some(0),
            argument_texts: Vec::new(),
            result_target: None,
            receiver: None,
            receiver_kind: None,
            qualifier: Some("Gameplay".to_string()),
            qualifier_kind: Some(RawQualifierKind::Namespace),
            file_path: "test.cpp".to_string(),
            line: 5,
        };
        let gameplay = make_sym("Gameplay::Update", "Update", "function", None);
        let ui = make_sym("UI::Update", "Update", "function", None);
        let candidates = vec![&ui, &gameplay];
        let context = ResolutionContext {
            caller_parent: Some("AI::Controller"),
            caller_namespace: Some("AI"),
        };

        let decision = resolve_one(&raw, candidates, context);

        assert_eq!(decision.chosen.map(|symbol| symbol.id.as_str()), Some("Gameplay::Update"));
        assert_eq!(decision.status, ResolutionStatus::Resolved);
        assert!(decision.ranked[0]
            .reasons
            .iter()
            .any(|reason| reason.kind == "qualified_namespace_match" && reason.score == 70));
    }

    #[test]
    fn explicit_qualifier_overrides_same_parent_locality() {
        let raw = RawCallSite {
            caller_id: "AI::Controller::Update".to_string(),
            called_name: "Update".to_string(),
            call_kind: RawCallKind::Qualified,
            argument_count: Some(0),
            argument_texts: Vec::new(),
            result_target: None,
            receiver: None,
            receiver_kind: None,
            qualifier: Some("Gameplay".to_string()),
            qualifier_kind: Some(RawQualifierKind::Namespace),
            file_path: "test.cpp".to_string(),
            line: 5,
        };
        let self_method = make_sym("AI::Controller::Update", "Update", "method", Some("AI::Controller"));
        let gameplay = make_sym("Gameplay::Update", "Update", "function", None);
        let candidates = vec![&self_method, &gameplay];
        let context = ResolutionContext {
            caller_parent: Some("AI::Controller"),
            caller_namespace: Some("AI"),
        };

        let decision = resolve_one(&raw, candidates, context);

        assert_eq!(decision.status, ResolutionStatus::Resolved);
        assert_eq!(decision.chosen.map(|symbol| symbol.id.as_str()), Some("Gameplay::Update"));
        assert!(decision.ranked[0]
            .reasons
            .iter()
            .all(|reason| reason.kind != "same_parent"));
    }

    #[test]
    fn parameter_count_match_is_visible_in_ranking_decision() {
        let raw = RawCallSite {
            caller_id: "Math::Tick".to_string(),
            called_name: "Blend".to_string(),
            call_kind: RawCallKind::Unqualified,
            argument_count: Some(2),
            argument_texts: vec!["a".to_string(), "b".to_string()],
            result_target: None,
            receiver: None,
            receiver_kind: None,
            qualifier: None,
            qualifier_kind: None,
            file_path: "test.cpp".to_string(),
            line: 5,
        };
        let one = Symbol {
            parameter_count: Some(1),
            signature: Some("void Blend(int a)".to_string()),
            ..make_sym("Math::Blend#1", "Blend", "function", None)
        };
        let two = Symbol {
            parameter_count: Some(2),
            signature: Some("void Blend(float a, float b)".to_string()),
            ..make_sym("Math::Blend#2", "Blend", "function", None)
        };
        let candidates = vec![&one, &two];
        let context = ResolutionContext {
            caller_parent: None,
            caller_namespace: Some("Math"),
        };

        let decision = resolve_one(&raw, candidates, context);

        assert_eq!(decision.chosen.map(|symbol| symbol.id.as_str()), Some("Math::Blend#2"));
        assert_eq!(decision.status, ResolutionStatus::Resolved);
        assert!(decision.ranked[0]
            .reasons
            .iter()
            .any(|reason| reason.kind == "parameter_count_match" && reason.score == 60));
    }

    #[test]
    fn signature_arity_hint_is_used_when_parameter_count_is_missing() {
        let raw = RawCallSite {
            caller_id: "Math::Tick".to_string(),
            called_name: "Blend".to_string(),
            call_kind: RawCallKind::Unqualified,
            argument_count: Some(2),
            argument_texts: vec!["a".to_string(), "b".to_string()],
            result_target: None,
            receiver: None,
            receiver_kind: None,
            qualifier: None,
            qualifier_kind: None,
            file_path: "test.cpp".to_string(),
            line: 5,
        };
        let one = Symbol {
            signature: Some("void Blend(int a)".to_string()),
            ..make_sym("Math::Blend#1", "Blend", "function", None)
        };
        let two = Symbol {
            signature: Some("void Blend(float a, float b)".to_string()),
            ..make_sym("Math::Blend#2", "Blend", "function", None)
        };
        let candidates = vec![&one, &two];
        let context = ResolutionContext {
            caller_parent: None,
            caller_namespace: Some("Math"),
        };

        let decision = resolve_one(&raw, candidates, context);

        assert_eq!(decision.chosen.map(|symbol| symbol.id.as_str()), Some("Math::Blend#2"));
        assert_eq!(decision.status, ResolutionStatus::Resolved);
        assert!(decision.ranked[0]
            .reasons
            .iter()
            .any(|reason| reason.kind == "signature_arity_hint" && reason.score == 40));
    }

    #[test]
    fn no_candidates_is_unresolved() {
        let raw = make_raw("Game::Tick", "Missing");
        let context = ResolutionContext {
            caller_parent: None,
            caller_namespace: Some("Game"),
        };

        let decision = resolve_one(&raw, Vec::new(), context);

        assert_eq!(decision.status, ResolutionStatus::Unresolved);
        assert!(decision.chosen.is_none());
        assert!(decision.ranked.is_empty());
    }

    #[test]
    fn top_score_tie_is_ambiguous() {
        let raw = make_raw("Game::Tick", "Update");
        let left = make_sym("Gameplay::Update", "Update", "function", None);
        let right = make_sym("UI::Update", "Update", "function", None);
        let candidates = vec![&left, &right];
        let context = ResolutionContext {
            caller_parent: None,
            caller_namespace: None,
        };

        let decision = resolve_one(&raw, candidates, context);

        assert_eq!(decision.status, ResolutionStatus::Ambiguous);
        assert!(decision.chosen.is_none());
        assert_eq!(decision.ranked.len(), 2);
        assert_eq!(decision.ranked[0].score, decision.ranked[1].score);
    }

    #[test]
    fn ambiguous_edges_are_not_emitted() {
        let symbols = vec![
            make_sym("Gameplay::Update", "Update", "function", None),
            make_sym("UI::Update", "Update", "function", None),
        ];
        let raw = vec![make_raw("Game::Tick", "Update")];

        let calls = resolve_calls(&raw, &symbols);

        assert!(calls.is_empty());
    }

    #[test]
    fn sibling_methods_fixture_prefers_containing_class_method() {
        let source = include_str!("../../samples/ambiguity/src/sibling_methods.cpp");
        let (symbols, raw_calls, calls) =
            resolve_fixture_source("samples/ambiguity/src/sibling_methods.cpp", source);

        let player_decision = resolve_fixture_decision(
            &raw_calls,
            &symbols,
            "Game::Player::Process",
            "Run",
            None,
        );
        assert_eq!(player_decision.status, ResolutionStatus::Resolved);
        assert_eq!(
            player_decision.chosen.map(|symbol| symbol.id.as_str()),
            Some("Game::Player::Run")
        );

        let enemy_decision = resolve_fixture_decision(
            &raw_calls,
            &symbols,
            "Game::Enemy::Process",
            "Run",
            None,
        );
        assert_eq!(enemy_decision.status, ResolutionStatus::Resolved);
        assert_eq!(
            enemy_decision.chosen.map(|symbol| symbol.id.as_str()),
            Some("Game::Enemy::Run")
        );

        assert!(calls.iter().any(|call| {
            call.caller_id == "Game::Player::Process" && call.callee_id == "Game::Player::Run"
        }));
        assert!(calls.iter().any(|call| {
            call.caller_id == "Game::Enemy::Process" && call.callee_id == "Game::Enemy::Run"
        }));
    }

    #[test]
    fn namespace_dupes_fixture_resolves_qualified_targets() {
        let source = include_str!("../../samples/ambiguity/src/namespace_dupes.cpp");
        let (symbols, raw_calls, calls) =
            resolve_fixture_source("samples/ambiguity/src/namespace_dupes.cpp", source);

        let gameplay_decision = resolve_fixture_decision(
            &raw_calls,
            &symbols,
            "AI::Controller::Update",
            "Update",
            Some("Gameplay"),
        );
        assert_eq!(gameplay_decision.status, ResolutionStatus::Resolved);
        assert_eq!(
            gameplay_decision.chosen.map(|symbol| symbol.id.as_str()),
            Some("Gameplay::Update")
        );

        let ui_decision = resolve_fixture_decision(
            &raw_calls,
            &symbols,
            "AI::Controller::Update",
            "Update",
            Some("UI"),
        );
        assert_eq!(ui_decision.status, ResolutionStatus::Resolved);
        assert_eq!(
            ui_decision.chosen.map(|symbol| symbol.id.as_str()),
            Some("UI::Update")
        );

        assert!(calls.iter().any(|call| {
            call.caller_id == "AI::Controller::Update" && call.callee_id == "Gameplay::Update"
        }));
        assert!(calls.iter().any(|call| {
            call.caller_id == "AI::Controller::Update" && call.callee_id == "UI::Update"
        }));
    }

    #[test]
    fn split_update_fixture_resolves_this_and_pointer_member_calls() {
        let source = include_str!("../../samples/ambiguity/src/split_update.cpp");
        let (symbols, raw_calls, calls) =
            resolve_fixture_source("samples/ambiguity/src/split_update.cpp", source);

        let this_decision = resolve_fixture_decision(
            &raw_calls,
            &symbols,
            "Game::Worker::Tick",
            "Update",
            None,
        );
        assert_eq!(this_decision.status, ResolutionStatus::Resolved);
        assert_eq!(
            this_decision.chosen.map(|symbol| symbol.id.as_str()),
            Some("Game::Worker::Update")
        );

        let matched_edges: Vec<&Call> = calls
            .iter()
            .filter(|call| {
                call.caller_id == "Game::Worker::Tick" && call.callee_id == "Game::Worker::Update"
            })
            .collect();
        assert_eq!(matched_edges.len(), 2);
    }

    #[test]
    fn overloads_fixture_keeps_same_arity_namespace_call_ambiguous() {
        let source = include_str!("../../samples/ambiguity/src/overloads.cpp");
        let (symbols, raw_calls, calls) =
            resolve_fixture_source("samples/ambiguity/src/overloads.cpp", source);

        let decision = resolve_fixture_decision(
            &raw_calls,
            &symbols,
            "Renderer::Blend",
            "Blend",
            Some("Math"),
        );
        assert_eq!(decision.status, ResolutionStatus::Ambiguous);
        assert!(decision.chosen.is_none());

        assert!(calls.is_empty());
    }

    #[test]
    fn override_candidates_use_inheritance_and_matching_arity_as_high_confidence() {
        let symbols = vec![
            Symbol {
                parameter_count: Some(1),
                signature: Some("virtual void Tick(float dt)".into()),
                ..make_sym("Game::Actor::Tick", "Tick", "method", Some("Game::Actor"))
            },
            Symbol {
                parameter_count: Some(1),
                signature: Some("void Tick(float dt)".into()),
                ..make_sym("Game::Player::Tick", "Tick", "method", Some("Game::Player"))
            },
            make_sym("Game::Actor::Update", "Update", "method", Some("Game::Actor")),
            make_sym("Game::Player::Jump", "Jump", "method", Some("Game::Player")),
        ];
        let inheritance_edges = vec![InheritanceEdge {
            derived_symbol_id: "Game::Player".into(),
            base_symbol_id: "Game::Actor".into(),
            file_path: "player.h".into(),
            line: 7,
            confidence: RawExtractionConfidence::Partial,
        }];

        let candidates = find_override_candidates(&symbols, &inheritance_edges);

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].derived_method_id, "Game::Player::Tick");
        assert_eq!(candidates[0].base_method_id, "Game::Actor::Tick");
        assert_eq!(candidates[0].confidence, RawExtractionConfidence::High);
        assert!(candidates[0]
            .reasons
            .contains(&OverrideMatchReason::InheritanceEdge));
        assert!(candidates[0]
            .reasons
            .contains(&OverrideMatchReason::MatchingMethodName));
        assert!(candidates[0]
            .reasons
            .contains(&OverrideMatchReason::ParameterCountMatch));
    }

    #[test]
    fn override_candidates_remain_partial_when_only_name_and_hierarchy_match() {
        let symbols = vec![
            Symbol {
                signature: Some("virtual void Tick()".into()),
                ..make_sym("Game::System::Tick", "Tick", "method", Some("Game::System"))
            },
            Symbol {
                signature: None,
                ..make_sym("Game::DerivedSystem::Tick", "Tick", "method", Some("Game::DerivedSystem"))
            },
        ];
        let inheritance_edges = vec![InheritanceEdge {
            derived_symbol_id: "Game::DerivedSystem".into(),
            base_symbol_id: "Game::System".into(),
            file_path: "system.h".into(),
            line: 11,
            confidence: RawExtractionConfidence::Partial,
        }];

        let candidates = find_override_candidates(&symbols, &inheritance_edges);

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].confidence, RawExtractionConfidence::Partial);
        assert!(candidates[0]
            .reasons
            .contains(&OverrideMatchReason::InheritanceEdge));
        assert!(candidates[0]
            .reasons
            .contains(&OverrideMatchReason::MatchingMethodName));
        assert!(!candidates[0]
            .reasons
            .contains(&OverrideMatchReason::ParameterCountMatch));
    }
}
