use std::collections::HashSet;

use crate::models;
use crate::storage;

pub fn load_missing_callable_summaries(
    db: &storage::Database,
    calls: &[models::Call],
    in_memory: &[models::CallableFlowSummary],
) -> rusqlite::Result<Vec<models::CallableFlowSummary>> {
    let existing: HashSet<&str> = in_memory
        .iter()
        .map(|summary| summary.callable_symbol_id.as_str())
        .collect();
    let mut missing: Vec<String> = calls
        .iter()
        .map(|call| call.callee_id.clone())
        .filter(|callee_id| !existing.contains(callee_id.as_str()))
        .collect();
    missing.sort();
    missing.dedup();
    db.read_callable_flow_summaries_for_ids(&missing)
}

pub fn merge_callable_summaries(
    primary: &[models::CallableFlowSummary],
    secondary: &[models::CallableFlowSummary],
) -> Vec<models::CallableFlowSummary> {
    let mut merged = primary.to_vec();
    let mut seen: HashSet<String> = merged
        .iter()
        .map(|summary| summary.callable_symbol_id.clone())
        .collect();
    for summary in secondary {
        if seen.insert(summary.callable_symbol_id.clone()) {
            merged.push(summary.clone());
        }
    }
    merged
}
