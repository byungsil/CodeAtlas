//! Cross-Boundary Flow Tracking Module.
//! Detects source/sink patterns and computes argument→parameter flow hops across call boundaries.

use std::collections::{HashMap as StdHashMap, HashSet};
use crate::models::{FlowKind, SymbolFlowTag, CrossBoundaryFlowHop, TransferKind};

// ── Source Detection Rules ────────────────────────────────────────────────

/// Check if an expression or identifier is likely a user input source.
pub fn detect_user_input_source(expression: &str) -> Option<SymbolFlowTag> {
    let expr_lower = expression.to_lowercase();

    // stdin / cin patterns (C++)
    if expr_lower.contains("cin") || expr_lower.contains("stdin") {
        return Some(SymbolFlowTag {
            symbol_id: String::new(),
            tag_kind: FlowKind::UserInput,
            label: Some("user_input".to_string()),
            confidence: "high".to_string(),
        });
    }

    // file.read() patterns (C++)
    if expr_lower.contains(".read(") || expr_lower.contains("_getline") {
        return Some(SymbolFlowTag {
            symbol_id: String::new(),
            tag_kind: FlowKind::UserInput,
            label: Some("file_input".to_string()),
            confidence: "high".to_string(),
        });
    }

    // Python/JS user input patterns  
    if expr_lower.contains("input()") || expr_lower.contains("prompt") {
        return Some(SymbolFlowTag {
            symbol_id: String::new(),
            tag_kind: FlowKind::UserInput,
            label: Some("user_input".to_string()),
            confidence: "partial".to_string(),
        });
    }

    // Query parameter patterns (TypeScript/Node.js)  
    if expr_lower.contains("req.query") || expr_lower.contains("request.params") {
        return Some(SymbolFlowTag {
            symbol_id: String::new(),
            tag_kind: FlowKind::UserInput,
            label: Some("query_param".to_string()),
            confidence: "partial".to_string(),
        });
    }

    None
}

/// Check if an expression is likely a configuration value source.
pub fn detect_config_value_source(expression: &str) -> Option<SymbolFlowTag> {
    let expr_lower = expression.to_lowercase();

    // Environment variable access patterns (C++)  
    if expr_lower.contains("getenv") || expr_lower.contains("_environ") {
        return Some(SymbolFlowTag {
            symbol_id: String::new(),
            tag_kind: FlowKind::ConfigValue,
            label: Some("config_value".to_string()),
            confidence: "high".to_string(),
        });
    }

    // Python config patterns  
    if expr_lower.contains(".env") || expr_lower.contains("environ") {
        return Some(SymbolFlowTag {
            symbol_id: String::new(),
            tag_kind: FlowKind::ConfigValue,
            label: Some("config_value".to_string()),
            confidence: "partial".to_string(),
        });
    }

    // JSON config file reads (TypeScript/Node.js)  
    if expr_lower.contains(".json") && expr_lower.contains("read") {
        return Some(SymbolFlowTag {
            symbol_id: String::new(),
            tag_kind: FlowKind::ConfigValue,
            label: Some("config_file".to_string()),
            confidence: "partial".to_string(),
        });
    }

    None
}

/// Check if an expression is a constant literal (safe to exclude from flow).  
pub fn detect_constant_literal(expression: &str) -> Option<SymbolFlowTag> {
    let trimmed = expression.trim();
    
    if trimmed.parse::<f64>().is_ok() || 
       trimmed.starts_with("'") || trimmed.starts_with("\"") || 
       trimmed == "null" || trimmed == "None" {
        return Some(SymbolFlowTag {
            symbol_id: String::new(),
            tag_kind: FlowKind::ConstantLiteral,
            label: Some("constant_literal".to_string()),
            confidence: "high".to_string(),
        });
    }

    None  
}

/// Detect sink patterns (execution targets where tainted data flows into).  
pub fn detect_sink(expression: &str, callee_name: Option<&String>) -> Vec<SymbolFlowTag> {
    let mut sinks = Vec::new();
    
    // SQL execution sinks
    if expression.contains("execute") || expression.contains("exec_") {
        sinks.push(SymbolFlowTag {
            symbol_id: String::new(),
            tag_kind: FlowKind::Sink,
            label: Some("sql_execution".to_string()),
            confidence: "partial".to_string(),
        });
    }

    // Shell/command execution sinks (C++)  
    if let Some(name) = callee_name {
        if name.contains("system") || name.contains("popen") || 
           (name.contains("exec") && !name.contains(".execute")) {  // exclude SQL execute from above
            sinks.push(SymbolFlowTag {
                symbol_id: String::new(),  
                tag_kind: FlowKind::Sink,
                label: Some("command_execution".to_string()),
                confidence: "partial".to_string(),
            });
        }
    }

    // Python shell execution
    if expression.contains("__import__") || expression.contains("eval(") {
        sinks.push(SymbolFlowTag {  
            symbol_id: String::new(),
            tag_kind: FlowKind::Sink,
            label: Some("dynamic_execution".to_string()),  
            confidence: "high".to_string(),
        });
    }

    // Template injection (TypeScript/Node.js)
    if expression.contains("${") && expression.contains("<") {
        sinks.push(SymbolFlowTag {
            symbol_id: String::new(),
            tag_kind: FlowKind::Sink,  
            label: Some("template_injection".to_string()),
            confidence: "partial".to_string(),
        });
    }

    sinks
}

/// Analyze how an argument is transferred to a callee (direct pass-through vs transformed).  
pub fn analyze_argument_transfer(
    arg_text: &str, 
    receiver_or_qualifier: Option<&str>,
) -> (TransferKind, bool) {
    
    // Check for common transformation patterns
    let has_transform = arg_text.contains("to_string") ||
                        arg_text.contains(".trim()") ||  
                        arg_text.contains("parseInt") ||
                        arg_text.contains("parseFloat");

    if receiver_or_qualifier.is_some() && !has_transform {
        (TransferKind::FieldStorage, true) // value extracted from object field
    } else if has_transform {
        (TransferKind::Transformed, true)  // argument is transformed before passing  
    } else {
        (TransferKind::DirectPassThrough, false)  // direct pass-through
    }
}

// ── Flow Hop Computation from Raw Call Sites ───────────────────────

/// Compute cross-boundary flow hops from raw call sites only.
/// Used when resolver has not yet matched calls to symbol IDs.  
pub fn compute_flow_hops_from_calls(raw_calls: &[crate::models::RawCallSite]) -> Vec<CrossBoundaryFlowHop> {  
    let mut hops = Vec::new();

    for raw in raw_calls {
        // Analyze each argument for flow tagging
        for arg_text in &raw.argument_texts {
            if let Some(source_tag) = detect_user_input_source(arg_text) 
                .or_else(|| detect_config_value_source(arg_text)) {
                
                hops.push(CrossBoundaryFlowHop {
                    from_symbol: raw.caller_id.clone(),
                    to_symbol: String::new(), // will be filled by resolver with actual callee id  
                    transfer_kind: TransferKind::DirectPassThrough,
                    value_transformed: false,
                    source_label: Some(source_tag.label.unwrap_or_default()),
                });
            }

            let (transfer_kind, transformed) = analyze_argument_transfer(
                arg_text, 
                raw.receiver.as_deref().or(raw.qualifier.as_deref())  
            );

            hops.push(CrossBoundaryFlowHop {
                from_symbol: raw.caller_id.clone(),  
                to_symbol: String::new(),
                transfer_kind,
                value_transformed: transformed,
                source_label: None, // will be populated if source detected above
            });
        }

        // Check return value targets for flow propagation
        if let Some(target) = &raw.result_target {
            hops.push(CrossBoundaryFlowHop {  
                from_symbol: raw.caller_id.clone(),
                to_symbol: target.anchor_id.clone().unwrap_or_default(), 
                transfer_kind: TransferKind::DirectPassThrough,
                value_transformed: false,
                source_label: Some("return_value".to_string()),
            });
        }

        // Detect file-level sources (e.g., global config reads)  
        if let Some(tag) = detect_user_input_source(raw.file_path.as_str()) 
            .or_else(|| detect_config_value_source(raw.file_path.as_str())) {
            hops.push(CrossBoundaryFlowHop {
                from_symbol: raw.caller_id.clone(),
                to_symbol: String::new(), // unresolved callee  
                transfer_kind: TransferKind::DirectPassThrough,
                value_transformed: false,
                source_label: Some(tag.label.unwrap_or_default()),
            });
        }
    }

    hops
}

/// Collect flow tags from raw call sites for a list of files.  
pub fn collect_flow_tags_from_calls(raw_calls: &[crate::models::RawCallSite]) -> Vec<SymbolFlowTag> {  
    let mut tags = Vec::new();
    let mut seen_ids: StdHashMap<String, bool> = StdHashMap::default();

    for raw in raw_calls {
        if !seen_ids.contains_key(&raw.caller_id) || !seen_ids[&raw.caller_id] {
            // Detect file-level sources  
            if let Some(tag) = detect_user_input_source(raw.file_path.as_str()) 
                .or_else(|| detect_config_value_source(raw.file_path.as_str())) {
                tags.push(SymbolFlowTag {
                    symbol_id: raw.caller_id.clone(),
                    tag_kind: tag.tag_kind,
                    label: tag.label,  
                    confidence: "partial".to_string(), // file-level detection is heuristic
                });
            }
        }

        seen_ids.insert(raw.caller_id.clone(), true);
    }

    tags
}

// ── Phase 2 Resolver Integration: Bridged Flow Hops & Path Computation ────  

/// Bridge resolved calls with raw call sites to produce flow hops with proper callee IDs.  
/// This is the key integration point between resolver.rs and cross_flow.rs — it takes
/// the output of resolve_calls() (Vec<Call>) and combines it with raw_call data to fill in
/// the to_symbol field that compute_flow_hops_from_calls leaves empty.
pub fn resolve_cross_boundary_flow_hops(
    raw_calls: &[crate::models::RawCallSite],  
    resolved_calls: &[(String, String)], // (caller_id, callee_id) pairs from resolver output
) -> Vec<CrossBoundaryFlowHop> {

    let mut hops = Vec::new();

    // Build a map of caller_id -> set of callee_ids for quick lookup.
    let call_map: StdHashMap<&str, HashSet<String>> = resolved_calls.iter()  
        .fold(StdHashMap::default(), |mut acc, (caller, callee)| {
            acc.entry(caller.as_str()).or_default().insert(callee.clone());
            acc  
        });

    for raw in raw_calls {
        // Find matching calls from resolved output to get actual callee IDs.  
        let callees = call_map.get(raw.caller_id.as_str())
            .map(|ids| ids.iter().cloned().collect::<Vec<_>>());

        if let Some(callee_ids) = &callees {
            // Resolved calls — compute hops with proper target symbols.
            for arg_text in raw.argument_texts.iter() {  
                if let Some(source_tag) = detect_user_input_source(arg_text)
                    .or_else(|| detect_config_value_source(arg_text)) 
                    .or_else(|| detect_constant_literal(arg_text)) {

                    // Clone label before using it (avoids moved-value error).
                    let tag_label = source_tag.label.clone().unwrap_or_default();  
                    
                    for callee_id in callee_ids.iter() {
                        hops.push(CrossBoundaryFlowHop {
                            from_symbol: raw.caller_id.clone(),  
                            to_symbol: callee_id.clone(), // filled from resolver output
                            transfer_kind: TransferKind::DirectPassThrough,
                            value_transformed: false,
                            source_label: Some(tag_label.clone()),
                        });
                    }
                }

                let tk = analyze_argument_transfer(
                    arg_text, 
                    raw.receiver.as_deref().or(raw.qualifier.as_deref())  
                );

                for callee_id in callee_ids.iter() {
                    hops.push(CrossBoundaryFlowHop {
                        from_symbol: raw.caller_id.clone(),
                        to_symbol: callee_id.clone(), // filled from resolver output
                        transfer_kind: tk.0.clone(),
                        value_transformed: tk.1,
                        source_label: None, // will be populated if source detected above
                    });
                }

            } // end for arg_text
        } else {
            // Unresolved calls — still compute hops but with empty callee IDs.
            let mut hop_list: Vec<CrossBoundaryFlowHop> = Vec::new();  

            for arg_text in raw.argument_texts.iter() {  
                if let Some(source_tag) = detect_user_input_source(arg_text)
                    .or_else(|| detect_config_value_source(arg_text)) 
                    .or_else(|| detect_constant_literal(arg_text)) {

                    hop_list.push(CrossBoundaryFlowHop {  
                        from_symbol: raw.caller_id.clone(),
                        to_symbol: String::new(), // unresolved callee
                        transfer_kind: TransferKind::DirectPassThrough,
                        value_transformed: false,
                        source_label: Some(source_tag.label.unwrap_or_default()),
                    });
                }

                let (transfer_kind, is_transformed) = analyze_argument_transfer(  
                    arg_text, 
                    raw.receiver.as_deref().or(raw.qualifier.as_deref())
                );

                hop_list.push(CrossBoundaryFlowHop {
                    from_symbol: raw.caller_id.clone(),
                    to_symbol: String::new(),  // unresolved callee
                    transfer_kind,
                    value_transformed: is_transformed,  
                    source_label: None, // will be populated if source detected above
                });
            }

            hops.extend(hop_list);
        }
    }

    hops
}


/// Build semantic tag chains (flow paths) from raw call sites through resolved calls.
/// This traces how values propagate across function boundaries by following the call graph edges.  
pub fn build_flow_paths_from_calls(
    raw_calls: &[crate::models::RawCallSite], 
    resolved_calls: &[(String, String)], // (caller_id, callee_id)
) -> Vec<(String, String, Vec<CrossBoundaryFlowHop>, Vec<String>)> {

    let mut paths = Vec::new();

    if raw_calls.is_empty() || resolved_calls.is_empty() {  
        return paths;
    }

    // Build call graph adjacency: caller_id -> [callee_ids]
    let adj: StdHashMap<&str, HashSet<String>> = resolved_calls.iter()
        .fold(StdHashMap::default(), |mut acc, (caller, callee)| {
            acc.entry(caller.as_str()).or_default().insert(callee.clone());  
            acc
        });

    // Group raw calls by caller_id to collect all hops per source symbol.  
    let mut calls_by_caller: StdHashMap<&str, &crate::models::RawCallSite> = StdHashMap::new();
    for raw in raw_calls {
        if !calls_by_caller.contains_key(raw.caller_id.as_str()) {
            calls_by_caller.insert(&raw.caller_id, raw);  
        }
    }

    // For each caller with both raw calls AND resolved callees, build a flow path.  
    for (caller_id, raw) in &calls_by_caller {
        let callee_ids = adj.get(*caller_id).cloned();
        
        if let Some(callees) = callee_ids {
            if callees.is_empty() { continue; }

            // Collect hops and semantic tags from this caller's arguments.  
            let mut hop_list: Vec<CrossBoundaryFlowHop> = Vec::new();
            let mut semantic_tags: HashSet<String> = HashSet::new();  

            for arg_text in raw.argument_texts.iter() {
                if let Some(source_tag) = detect_user_input_source(arg_text)
                    .or_else(|| detect_config_value_source(arg_text)) 
                    .or_else(|| detect_constant_literal(arg_text)) {

                    // Clone label before using it to avoid moved-value error.  
                    semantic_tags.insert(source_tag.label.clone().unwrap_or_default());

                    let tl_for_loop = source_tag.label.clone().unwrap_or_default();  
                    
                    for callee_id in callees.iter() {
                        hop_list.push(CrossBoundaryFlowHop {
                            from_symbol: caller_id.to_string(),
                            to_symbol: callee_id.clone(), 
                            transfer_kind: TransferKind::DirectPassThrough,  
                            value_transformed: false,
                            source_label: Some(tl_for_loop.clone()),
                        });
                    }
                }

                // Also detect sink patterns (execution, SQL).
                let mut sinks = detect_sink(arg_text, Some(&raw.called_name));
                for tag in sinks.drain(..) {  
                    semantic_tags.insert(tag.label.unwrap_or_default());
                }
            }

            // Only record paths that have meaningful flow hops or tags.
            if !hop_list.is_empty() || !semantic_tags.is_empty() {
                let target_id = callees.iter().next().cloned()  
                    .unwrap_or_else(|| String::new());  

                paths.push((caller_id.to_string(), target_id, hop_list, semantic_tags.into_iter().collect()));
            }
        }
    }

    paths
}


/// Group CrossBoundaryFlowHops into flow path tuples for storage. 
pub fn group_hops_into_paths(hops: &[CrossBoundaryFlowHop]) -> Vec<(String, String, Vec<CrossBoundaryFlowHop>, Vec<String>)> {
    
    if hops.is_empty() { return vec![]; }

    // Group by from_symbol (source) and take first hop's to_symbol as target.  
    let mut groups: StdHashMap<&str, HashSet<String>> = StdHashMap::new();
    for hop in hops {
        groups.entry(&hop.from_symbol).or_default().insert(hop.to_symbol.clone());
    }

    // Build one path per source symbol with all its hops.  
    let mut paths = Vec::new();
    for (source_id, target_ids) in &groups {
        if !target_ids.is_empty() {
            let first_target = target_ids.iter().next().cloned().unwrap_or_default();

            // Collect semantic tags from source labels.  
            let mut tags: HashSet<String> = HashSet::new();
            for hop in hops {
                if &hop.from_symbol == *source_id && hop.source_label.as_deref() != Some("") {
                    if let Some(label) = &hop.source_label {
                        tags.insert(label.clone());
                    }  
                }
            }

            paths.push((source_id.to_string(), first_target, hops.iter().filter(|h| h.from_symbol == *source_id).cloned().collect(), tags.into_iter().collect()));
        }
    }

    paths  
}


/// Build flow path tuples directly from raw_calls + resolved calls (main integration entry point).
pub fn build_flow_paths_from_resolved(
    raw_calls: &[crate::models::RawCallSite], 
    resolved_call_pairs: &[(String, String)], // caller_id -> callee_id pairs
) -> Vec<(String, String, Vec<CrossBoundaryFlowHop>, Vec<String>)> {

    if raw_calls.is_empty() || resolved_call_pairs.is_empty() { return vec![]; }

    let hops = resolve_cross_boundary_flow_hops(raw_calls, resolved_call_pairs);  
    group_hops_into_paths(&hops)
}
