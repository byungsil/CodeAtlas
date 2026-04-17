use std::collections::{HashMap, HashSet};
use crate::models::{Call, RawCallSite, Symbol};
use crate::storage::Database;

pub fn resolve_calls(raw_calls: &[RawCallSite], symbols: &[Symbol]) -> Vec<Call> {
    let by_name: HashMap<&str, Vec<&Symbol>> = {
        let mut map: HashMap<&str, Vec<&Symbol>> = HashMap::new();
        for sym in symbols {
            if sym.symbol_type == "function" || sym.symbol_type == "method" {
                map.entry(sym.name.as_str()).or_default().push(sym);
            }
        }
        map
    };

    let parent_of: HashMap<&str, &str> = symbols
        .iter()
        .filter_map(|s| s.parent_id.as_deref().map(|p| (s.id.as_str(), p)))
        .collect();

    let mut calls = Vec::new();
    let mut seen = HashSet::new();

    for raw in raw_calls {
        let candidates = match by_name.get(raw.called_name.as_str()) {
            Some(c) => c,
            None => continue,
        };

        let callee = resolve_one(raw, candidates, &parent_of);

        if let Some(callee) = callee {
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

fn resolve_one<'a>(
    raw: &RawCallSite,
    candidates: &[&'a Symbol],
    parent_of: &HashMap<&str, &str>,
) -> Option<&'a Symbol> {
    if candidates.len() == 1 {
        return Some(candidates[0]);
    }

    let caller_parent = parent_of.get(raw.caller_id.as_str()).copied();

    if let Some(parent) = caller_parent {
        let sibling = candidates.iter().find(|s| s.parent_id.as_deref() == Some(parent));
        if sibling.is_some() {
            return sibling.copied();
        }
    }

    candidates.first().copied()
}

pub fn merge_symbols(all_symbols: &[Symbol]) -> Vec<Symbol> {
    let mut by_id: HashMap<String, Symbol> = HashMap::new();

    for sym in all_symbols {
        let entry = by_id.entry(sym.id.clone());
        entry
            .and_modify(|existing| {
                if sym.file_path.ends_with(".cpp") && existing.file_path.ends_with(".h") {
                    existing.file_path = sym.file_path.clone();
                    existing.line = sym.line;
                    existing.end_line = sym.end_line;
                    if sym.signature.is_some() {
                        existing.signature = sym.signature.clone();
                    }
                }
            })
            .or_insert_with(|| sym.clone());
    }

    by_id.into_values().collect()
}

pub fn resolve_calls_with_db(raw_calls: &[RawCallSite], new_symbols: &[Symbol], db: &Database) -> Vec<Call> {
    let new_by_name: HashMap<&str, Vec<&Symbol>> = {
        let mut map: HashMap<&str, Vec<&Symbol>> = HashMap::new();
        for sym in new_symbols {
            if sym.symbol_type == "function" || sym.symbol_type == "method" {
                map.entry(sym.name.as_str()).or_default().push(sym);
            }
        }
        map
    };

    let new_parent_of: HashMap<&str, &str> = new_symbols
        .iter()
        .filter_map(|s| s.parent_id.as_deref().map(|p| (s.id.as_str(), p)))
        .collect();

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

        let mut candidates: Vec<Symbol> = Vec::new();
        if let Some(local) = new_by_name.get(raw.called_name.as_str()) {
            candidates.extend(local.iter().map(|s| (*s).clone()));
        }
        let db_candidates = db.find_symbols_by_name(&raw.called_name).unwrap_or_default();
        for db_sym in db_candidates {
            if !candidates.iter().any(|c| c.id == db_sym.id) {
                candidates.push(db_sym);
            }
        }

        if candidates.is_empty() {
            continue;
        }

        let callee = if candidates.len() == 1 {
            &candidates[0]
        } else if let Some(parent) = caller_parent {
            candidates.iter().find(|s| s.parent_id.as_deref() == Some(parent))
                .unwrap_or(&candidates[0])
        } else {
            &candidates[0]
        };

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

    calls
}

#[cfg(test)]
mod tests {
    use super::*;
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
            parent_id: parent.map(|p| p.to_string()),
        }
    }

    #[test]
    fn resolves_simple_call() {
        let symbols = vec![
            make_sym("main", "main", "function", None),
            make_sym("foo", "foo", "function", None),
        ];
        let raw = vec![RawCallSite {
            caller_id: "main".to_string(),
            called_name: "foo".to_string(),
            receiver: None,
            file_path: "test.cpp".to_string(),
            line: 5,
        }];
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
        let raw = vec![
            RawCallSite { caller_id: "main".into(), called_name: "foo".into(), receiver: None, file_path: "t.cpp".into(), line: 5 },
            RawCallSite { caller_id: "main".into(), called_name: "foo".into(), receiver: None, file_path: "t.cpp".into(), line: 5 },
        ];
        let calls = resolve_calls(&raw, &symbols);
        assert_eq!(calls.len(), 1);
    }

    #[test]
    fn skips_self_calls() {
        let symbols = vec![make_sym("foo", "foo", "function", None)];
        let raw = vec![RawCallSite {
            caller_id: "foo".into(),
            called_name: "foo".into(),
            receiver: None,
            file_path: "t.cpp".into(),
            line: 2,
        }];
        let calls = resolve_calls(&raw, &symbols);
        assert_eq!(calls.len(), 0);
    }

    #[test]
    fn merge_prefers_cpp_over_h() {
        let syms = vec![
            Symbol {
                id: "Foo::Bar".into(), name: "Bar".into(), qualified_name: "Foo::Bar".into(),
                symbol_type: "method".into(), file_path: "foo.h".into(),
                line: 5, end_line: 5, signature: Some("void Bar()".into()), parent_id: Some("Foo".into()),
            },
            Symbol {
                id: "Foo::Bar".into(), name: "Bar".into(), qualified_name: "Foo::Bar".into(),
                symbol_type: "method".into(), file_path: "foo.cpp".into(),
                line: 10, end_line: 15, signature: Some("void Foo::Bar()".into()), parent_id: Some("Foo".into()),
            },
        ];
        let merged = merge_symbols(&syms);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].file_path, "foo.cpp");
        assert_eq!(merged[0].line, 10);
    }

    #[test]
    fn resolve_calls_with_db_uses_db_caller_parent_for_unchanged_callers() {
        let db = Database::open(Path::new(":memory:")).unwrap();
        db.write_symbols(&[
            make_sym("Alpha::Caller", "Caller", "method", Some("Alpha")),
            make_sym("Alpha::Target", "Target", "method", Some("Alpha")),
        ]).unwrap();

        let new_symbols = vec![
            make_sym("Beta::Target", "Target", "method", Some("Beta")),
        ];

        let raw = vec![RawCallSite {
            caller_id: "Alpha::Caller".into(),
            called_name: "Target".into(),
            receiver: None,
            file_path: "alpha.cpp".into(),
            line: 7,
        }];

        let calls = resolve_calls_with_db(&raw, &new_symbols, &db);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].callee_id, "Alpha::Target");
    }
}
