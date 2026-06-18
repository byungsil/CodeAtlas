use std::time::Instant;
use std::sync::OnceLock;
use crate::models::{
    ParseResult, ParseMetrics, Symbol, RawCallSite, RawCallKind, FileRiskSignals,
    ParseFragility, MacroSensitivity, IncludeHeaviness,
};
use crate::parser::{
    extract_include_dependencies, extract_macro_info, compute_dependency_metrics,
    extract_conditional_symbols,
};
use clang::{Clang, Entity, EntityKind, EntityVisitResult, Index, Unsaved};

// ─── Global Clang instance ────────────────────────────────────────────────────
// We keep exactly ONE `Clang` guard alive for the process lifetime, initialised
// on first use via `OnceLock`.  After `Clang::new()` completes, the guard is
// never mutated, so sharing `&'static Clang` across threads is safe.
//
// Each parse call creates its own `Index` (= `CXIndex` in libclang).
// `clang_createIndex` and `clang_parseTranslationUnit` are thread-safe when
// called on distinct TUs, so multiple rayon workers can parse in parallel
// without any additional synchronisation.
//
// Safety: `Clang` wraps an `AtomicBool` that is set once on construction and
// never touched again.  `Index::new(&Clang)` calls `clang_createIndex()` which
// is documented as thread-safe in libclang.  No mutable aliasing occurs.
struct GlobalClang(Clang);
// SAFETY: see comment above.
unsafe impl Sync for GlobalClang {}
unsafe impl Send for GlobalClang {}

static GLOBAL_CLANG: OnceLock<GlobalClang> = OnceLock::new();

fn get_clang() -> &'static Clang {
    &GLOBAL_CLANG
        .get_or_init(|| GlobalClang(Clang::new().expect("Failed to initialise libclang")))
        .0
}

// ─── System-path filter ───────────────────────────────────────────────────────

/// Returns true when `path` belongs to a system or toolchain directory that
/// should never be indexed.
fn is_system_path(path: &str) -> bool {
    let p = path.replace('\\', "/").to_lowercase();
    p.contains("/program files")
        || p.contains("/program files (x86)")
        || p.contains("/windows kits")
        || p.contains("/microsoft visual studio")
        || p.contains("/vc/tools/msvc")
        || p.contains("/lib/clang")
        || p.contains("/usr/include")
        || p.contains("/usr/lib")
}

// ─── Scope-stack helpers ──────────────────────────────────────────────────────

/// One entry per enclosing namespace / class / function on the scope stack.
#[derive(Clone)]
struct ScopeFrame {
    /// USR of this scope entity (used as `parent_id` for direct children).
    usr: String,
    /// Simple (unqualified) name of this scope entity.
    name: String,
    /// "namespace" | "class" | "function"
    kind: &'static str,
}

/// Build a `qualified_name` for `entity_name` given the current scope stack.
fn build_qualified_name(entity_name: &str, scope_stack: &[ScopeFrame]) -> String {
    if scope_stack.is_empty() {
        entity_name.to_string()
    } else {
        let prefix = scope_stack
            .iter()
            .map(|f| f.name.as_str())
            .collect::<Vec<_>>()
            .join("::");
        format!("{}::{}", prefix, entity_name)
    }
}

/// Build `scope_qualified_name` from the scope stack (excludes the entity itself).
fn build_scope_qualified_name(scope_stack: &[ScopeFrame]) -> Option<String> {
    if scope_stack.is_empty() {
        None
    } else {
        Some(
            scope_stack
                .iter()
                .map(|f| f.name.as_str())
                .collect::<Vec<_>>()
                .join("::"),
        )
    }
}

// ─── Function-signature extraction ───────────────────────────────────────────

/// Extract a human-readable signature and parameter count for a function/method.
fn extract_function_info(entity: &Entity<'_>) -> (Option<String>, Option<usize>) {
    let args = match entity.get_arguments() {
        Some(a) => a,
        None => return (None, None),
    };
    let param_count = args.len();
    let params: Vec<String> = args
        .iter()
        .map(|a| {
            let type_str = a
                .get_type()
                .map(|t| t.get_display_name())
                .unwrap_or_else(|| "_".to_string());
            let pname = a.get_name().unwrap_or_default();
            if pname.is_empty() {
                type_str
            } else {
                format!("{} {}", type_str, pname)
            }
        })
        .collect();
    let ret = entity
        .get_type()
        .and_then(|t| t.get_result_type())
        .map(|t| t.get_display_name())
        .unwrap_or_else(|| "auto".to_string());
    let fname = entity.get_name().unwrap_or_default();
    let sig = format!("{} {}({})", ret, fname, params.join(", "));
    (Some(sig), Some(param_count))
}

// ─── Mutable visitor state ────────────────────────────────────────────────────

struct VisitorState {
    symbols: Vec<Symbol>,
    raw_calls: Vec<RawCallSite>,
    /// Stack of enclosing scopes (namespace / class / function).
    scope_stack: Vec<ScopeFrame>,
    /// USR of the innermost enclosing function/method (for caller tracking).
    current_function_usr: Option<String>,
}

// ─── Recursive AST visitor ────────────────────────────────────────────────────

// `indexed_path` = relative path used for symbol storage.
// `clang_path`    = the path Clang used for the TU (may be absolute); used
//                   only for matching entity locations so we correctly
//                   filter entities that belong to this file.
fn visit_entity<'tu>(entity: Entity<'tu>, state: &mut VisitorState, indexed_path: &str, clang_path: &str) {
    // Determine which file this node belongs to.
    let entity_path = entity
        .get_location()
        .and_then(|loc| loc.get_file_location().file)
        .map(|f| f.get_path().to_string_lossy().replace('\\', "/"));

    // Prune system/toolchain directories entirely — skip recursion too.
    if let Some(ref p) = entity_path {
        if is_system_path(p) {
            return;
        }
    }

    // Match by the exact path Clang used (which may be absolute) OR by the
    // relative path as a suffix fallback (for cases where clang_path == indexed_path).
    let in_indexed_file = entity_path.as_deref().map(|ep| {
        ep == clang_path
            || ep == indexed_path
            || ep.ends_with(&format!("/{}", clang_path))
            || ep.ends_with(&format!("/{}", indexed_path))
    }).unwrap_or(false);

    let kind = entity.get_kind();

    // Kinds that open a new scope frame (affect scope_stack / current_function_usr).
    let is_scope_kind = matches!(
        kind,
        EntityKind::Namespace
            | EntityKind::ClassDecl
            | EntityKind::StructDecl
            | EntityKind::ClassTemplate
            | EntityKind::FunctionDecl
            | EntityKind::Method
            | EntityKind::Constructor
            | EntityKind::Destructor
            | EntityKind::FunctionTemplate
    );

    // Kinds that produce a Symbol entry.
    let is_emittable = in_indexed_file
        && matches!(
            kind,
            EntityKind::Namespace
                | EntityKind::ClassDecl
                | EntityKind::StructDecl
                | EntityKind::ClassTemplate
                | EntityKind::FunctionDecl
                | EntityKind::Method
                | EntityKind::Constructor
                | EntityKind::Destructor
                | EntityKind::FunctionTemplate
                | EntityKind::EnumDecl
        );

    // ── Emit symbol ──────────────────────────────────────────────────────────
    if is_emittable {
        if let Some(name) = entity.get_name() {
            let range = entity.get_range();
            let start_line = range
                .as_ref()
                .map(|r| r.get_start().get_file_location().line)
                .unwrap_or(0);
            let end_line = range
                .as_ref()
                .map(|r| r.get_end().get_file_location().line)
                .unwrap_or(0);

            let id = entity
                .get_usr()
                .map(|u| u.0)
                .unwrap_or_else(|| name.clone());

            let qualified_name = build_qualified_name(&name, &state.scope_stack);
            let scope_qualified_name = build_scope_qualified_name(&state.scope_stack);
            let scope_kind = state.scope_stack.last().map(|f| f.kind.to_string());
            let parent_id = state.scope_stack.last().map(|f| f.usr.clone());

            let symbol_type = match kind {
                EntityKind::ClassDecl
                | EntityKind::StructDecl
                | EntityKind::ClassTemplate => "class",
                EntityKind::Namespace => "namespace",
                EntityKind::EnumDecl => "enum",
                _ => "function",
            }
            .to_string();

            let is_fn = matches!(
                kind,
                EntityKind::FunctionDecl
                    | EntityKind::Method
                    | EntityKind::Constructor
                    | EntityKind::Destructor
                    | EntityKind::FunctionTemplate
            );
            let (signature, parameter_count) = if is_fn {
                extract_function_info(&entity)
            } else {
                (None, None)
            };

            state.symbols.push(Symbol {
                id,
                name,
                qualified_name,
                symbol_type,
                file_path: indexed_path.to_string(),
                line: start_line as usize,
                end_line: end_line as usize,
                signature,
                parameter_count,
                scope_qualified_name,
                scope_kind,
                symbol_role: Some(
                    if entity.is_definition() {
                        "definition"
                    } else {
                        "declaration"
                    }
                    .to_string(),
                ),
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
        }
    }

    // ── Emit call site ───────────────────────────────────────────────────────
    if in_indexed_file && kind == EntityKind::CallExpr {
        if let Some(caller_id) = state.current_function_usr.clone() {
            let line = entity
                .get_location()
                .map(|l| l.get_file_location().line)
                .unwrap_or(0);

            // `get_reference()` on CallExpr may return None in some Clang versions.
            // Fall back to scanning the entire subtree (DeclRefExpr may be nested
            // inside ImplicitCastExpr or similar intermediate nodes).
            let callee_ref = entity.get_reference().or_else(|| {
                let mut found = None;
                entity.visit_children(|child, _| {
                    if found.is_some() {
                        return EntityVisitResult::Break;
                    }
                    let ck = child.get_kind();
                    if matches!(ck, EntityKind::DeclRefExpr | EntityKind::MemberRefExpr | EntityKind::OverloadedDeclRef) {
                        if let Some(r) = child.get_reference() {
                            found = Some(r);
                            return EntityVisitResult::Break;
                        }
                    }
                    EntityVisitResult::Recurse
                });
                found
            });

            if let Some(callee) = callee_ref {
                let called_name = callee
                    .get_name()
                    .or_else(|| callee.get_display_name())
                    .unwrap_or_default();

                if !called_name.is_empty() {
                    let arg_count = entity.get_arguments().map(|a| a.len());
                    let call_kind = match callee.get_kind() {
                        EntityKind::Method
                        | EntityKind::Constructor
                        | EntityKind::Destructor => RawCallKind::MemberAccess,
                        _ => RawCallKind::Unqualified,
                    };
                    state.raw_calls.push(RawCallSite {
                        caller_id,
                        called_name,
                        call_kind,
                        argument_count: arg_count,
                        argument_texts: Vec::new(),
                        result_target: None,
                        receiver: None,
                        receiver_kind: None,
                        qualifier: None,
                        qualifier_kind: None,
                        file_path: indexed_path.to_string(),
                        line: line as usize,
                    });
                }
            }
        }
    }

    // ── Push scope frame before recursing ────────────────────────────────────
    let entity_name_for_scope = entity.get_name().unwrap_or_default();
    let should_push = is_scope_kind && !entity_name_for_scope.is_empty();
    let prev_function_usr = state.current_function_usr.clone();

    if should_push {
        let usr = entity
            .get_usr()
            .map(|u| u.0)
            .unwrap_or_else(|| entity_name_for_scope.clone());

        let frame_kind = match kind {
            EntityKind::ClassDecl | EntityKind::StructDecl | EntityKind::ClassTemplate => "class",
            EntityKind::Namespace => "namespace",
            _ => "function",
        };

        // Track the innermost function so CallExpr can record its caller.
        let is_fn_scope = matches!(
            kind,
            EntityKind::FunctionDecl
                | EntityKind::Method
                | EntityKind::Constructor
                | EntityKind::Destructor
                | EntityKind::FunctionTemplate
        );
        if is_fn_scope {
            state.current_function_usr = Some(usr.clone());
        }

        state.scope_stack.push(ScopeFrame {
            usr,
            name: entity_name_for_scope,
            kind: frame_kind,
        });
    }

    // ── Recurse into children ─────────────────────────────────────────────────
    entity.visit_children(|child, _| {
        visit_entity(child, state, indexed_path, clang_path);
        EntityVisitResult::Continue
    });

    // ── Pop scope frame ───────────────────────────────────────────────────────
    if should_push {
        state.scope_stack.pop();
        state.current_function_usr = prev_function_usr;
    }
}

// ─── Public entry point ───────────────────────────────────────────────────────

pub fn parse_cpp_file(
    file_path: &str,
    source: &str,
    compile_args: &[String],
    workspace_root: Option<&std::path::Path>,
) -> Result<ParseResult, String> {
    let start = Instant::now();

    // Each call borrows the global Clang guard and creates its own Index
    // (CXIndex).  Multiple threads can do this concurrently — libclang is
    // thread-safe at the CXIndex / TU level.
    let index = Index::new(get_clang(), false, false);

    // When workspace_root is provided, use the absolute path for the Clang TU so
    // that `#include "..."` directives resolve relative to the file's real directory
    // on disk instead of the process CWD.
    let abs_file_path_storage;
    let clang_path = if let Some(root) = workspace_root {
        let rel = file_path.replace('/', std::path::MAIN_SEPARATOR_STR);
        abs_file_path_storage = root
            .join(&rel)
            .to_string_lossy()
            .replace('\\', "/");
        abs_file_path_storage.as_str()
    } else {
        file_path
    };

    let unsaved = Unsaved::new(clang_path, source);
    let mut parser = index.parser(clang_path);
    parser.arguments(compile_args);
    parser.unsaved(&[unsaved]);
    parser.detailed_preprocessing_record(true);

    let tu = parser.parse().map_err(|e| format!("Clang parse error: {}", e))?;
    let tree_sitter_parse_ms = start.elapsed().as_millis();

    let syntax_walk_start = Instant::now();

    // `indexed_path` is the RELATIVE path used for symbol storage, while
    // `clang_path` (absolute) is used for Clang entity-location matching.
    let normalized_path = file_path.replace('\\', "/");

    let mut state = VisitorState {
        symbols: Vec::new(),
        raw_calls: Vec::new(),
        scope_stack: Vec::new(),
        current_function_usr: None,
    };

    let root = tu.get_entity();
    root.visit_children(|child, _| {
        visit_entity(child, &mut state, &normalized_path, clang_path);
        EntityVisitResult::Continue
    });

    let syntax_walk_ms = syntax_walk_start.elapsed().as_millis();

    // Text-based extraction for includes and macros (reuse existing parser.rs logic).
    let include_deps = extract_include_dependencies(source, file_path);
    let (macro_defs, cond_blocks) = extract_macro_info(source, file_path);
    let cond_symbols = extract_conditional_symbols(source, file_path, &cond_blocks);

    let global_graph = std::collections::HashMap::new();
    let dependency_metrics = compute_dependency_metrics(file_path, &include_deps, &global_graph);

    let file_risk_signals = FileRiskSignals {
        parse_fragility: ParseFragility::Low,
        macro_sensitivity: MacroSensitivity::Low,
        include_heaviness: IncludeHeaviness::Light,
    };

    Ok(ParseResult {
        symbols: state.symbols,
        file_risk_signals,
        relation_events: Vec::new(),
        normalized_references: Vec::new(),
        propagation_events: Vec::new(),
        callable_flow_summaries: Vec::new(),
        raw_calls: state.raw_calls,
        metrics: ParseMetrics {
            tree_sitter_parse_ms,
            syntax_walk_ms,
            ..Default::default()
        },
        include_dependencies: include_deps,
        macro_definitions: macro_defs,
        conditional_blocks: cond_blocks,
        dependency_metrics,
        conditional_symbols: cond_symbols,
    })
}
