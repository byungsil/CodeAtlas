use std::time::Instant;
use std::sync::OnceLock;
use crate::models::{
    ParseResult, ParseMetrics, Symbol, RawCallSite, RawCallKind, RawQualifierKind,
    RawRelationEvent, RawRelationKind, RawEventSource, RawExtractionConfidence,
    FileRiskSignals, ParseFragility, MacroSensitivity, IncludeHeaviness,
};
use crate::parser::{
    extract_include_dependencies, extract_macro_info, compute_dependency_metrics,
    extract_conditional_symbols,
};
use clang::{Clang, Entity, EntityKind, EntityVisitResult, Index, Unsaved};

/// Identifies the shape of the `ParseResult` this extractor produces.
///
/// Bump this whenever the libclang extraction output changes in any way that
/// affects the serialized `ParseResult` (new fields, changed semantics, USR
/// scheme tweaks, etc.). The parse cache (MS22) folds this tag into its
/// content-addressable key so a version bump transparently invalidates every
/// previously cached entry — old-format keys simply never match again.
pub const PARSER_VERSION_TAG: &str = "cpp-clang-v1";

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
    /// Inheritance and other non-call relation events.
    relation_events: Vec<RawRelationEvent>,
    /// Stack of enclosing scopes (namespace / class / function).
    scope_stack: Vec<ScopeFrame>,
    /// USR of the innermost enclosing function/method (for caller tracking).
    current_function_usr: Option<String>,
    /// Phase D: lambda body ranges pre-scanned from the TU.
    /// (start_line, end_line, lambda_id) — used to correctly attribute
    /// CallExpr nodes that libclang exposes outside the LambdaExpr cursor.
    lambda_ranges: Vec<(u32, u32, String)>,
    /// Phase D: deduplicate call sites that libclang visits more than once
    /// (same CallExpr exposed both as sibling of LambdaExpr and as its child).
    seen_call_sites: std::collections::HashSet<(u32, u32)>,  // (line, col)
}

// ─── Phase D: Pre-scan — collect lambda ranges ──────────────────────────────
// libclang exposes lambda body nodes both as children of `LambdaExpr` AND as
// siblings of it in the parent cursor's child list.  The sibling visit happens
// BEFORE the `LambdaExpr` cursor itself, so the live scope_stack cannot be
// used to detect the lambda context at that point.  Solve by pre-scanning the
// TU once for all `LambdaExpr` cursors before the main walk.
fn pre_scan_lambdas<'tu>(entity: Entity<'tu>, ranges: &mut Vec<(u32, u32, String)>) {
    if entity.get_kind() == EntityKind::LambdaExpr && entity.is_in_main_file() {
        let loc = entity.get_location().map(|l| l.get_file_location());
        let line = loc.as_ref().map(|l| l.line).unwrap_or(0);
        let col  = loc.as_ref().map(|l| l.column).unwrap_or(0);
        let end  = entity
            .get_range()
            .map(|r| r.get_end().get_file_location().line)
            .unwrap_or(line);
        let id   = entity
            .get_usr()
            .map(|u| u.0)
            .unwrap_or_else(|| format!("__lambda_{}_{}", line, col));
        ranges.push((line, end, id));
    }
    entity.visit_children(|child, _| {
        pre_scan_lambdas(child, ranges);
        EntityVisitResult::Continue
    });
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

    // `is_in_main_file()` is the authoritative check: it returns true for any
    // entity whose source location is in the main TU file (not in included headers).
    // This is more reliable than path string comparison, which can break with
    // unsaved files (where `get_file_location().file` may return None on some
    // libclang versions/platforms).
    // Fall back to path comparison for entities where location is unavailable
    // (e.g. implicit/injected declarations that libclang places in no file).
    let is_main_file = entity
        .get_location()
        .map(|l| l.is_in_main_file())
        .unwrap_or(false);
    let in_indexed_file = is_main_file || entity_path.as_deref().map(|ep| {
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
            | EntityKind::UnionDecl
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
                | EntityKind::UnionDecl
                | EntityKind::FunctionDecl
                | EntityKind::Method
                | EntityKind::Constructor
                | EntityKind::Destructor
                | EntityKind::FunctionTemplate
                | EntityKind::EnumDecl
                | EntityKind::TypeAliasDecl
                | EntityKind::TypedefDecl
                | EntityKind::EnumConstantDecl
                | EntityKind::FieldDecl
        );

    // ── Emit symbol ──────────────────────────────────────────────────────────
    if is_emittable {
        // FieldDecl: only emit when directly inside a class/struct/union scope.
        // This avoids indexing local struct fields declared inside function bodies
        // as standalone symbols.
        let is_field = kind == EntityKind::FieldDecl;
        let in_class_scope = state.scope_stack.last().map(|f| f.kind == "class").unwrap_or(false);
        if is_field && !in_class_scope {
            // still recurse into field children (e.g. anonymous struct/union members)
        } else if let Some(name) = entity.get_name() {
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
                | EntityKind::ClassTemplate
                | EntityKind::UnionDecl => "class",
                EntityKind::Namespace => "namespace",
                EntityKind::EnumDecl => "enum",
                EntityKind::TypeAliasDecl | EntityKind::TypedefDecl => "type_alias",
                EntityKind::EnumConstantDecl => "enum_value",
                EntityKind::FieldDecl => "field",
                EntityKind::Method
                | EntityKind::Constructor
                | EntityKind::Destructor => "method",
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

            // ── Declaration / definition cross-linking ────────────────────
            // Fill `definition_*` and `declaration_*` fields so that navigation
            // (header ↔ implementation) works without a second-pass merge.
            //
            // If this entity IS the definition:
            //   • definition_file_path/line/end_line = current file + range
            //   • declaration_* = None (we don't know which header declared it)
            //
            // If this entity is a declaration (not the definition):
            //   • declaration_file_path/line/end_line = current file + range
            //   • Try get_definition() to cross-link; only use the result when
            //     the definition is in a non-system file.
            let is_def = entity.is_definition();
            let (decl_file, decl_line, decl_end_line, def_file, def_line, def_end_line) = if is_def {
                // This entity is the definition.
                (
                    None,
                    None,
                    None,
                    Some(indexed_path.to_string()),
                    Some(start_line as usize),
                    Some(end_line as usize),
                )
            } else {
                // This entity is a declaration. Attempt to find the definition.
                let maybe_def = entity.get_definition().and_then(|def_entity| {
                    let def_range = def_entity.get_range()?;
                    let def_file_path = def_entity
                        .get_location()
                        .and_then(|l| l.get_file_location().file)
                        .map(|f| f.get_path().to_string_lossy().replace('\\', "/"))?;
                    if is_system_path(&def_file_path) {
                        return None;
                    }
                    let def_start = def_range.get_start().get_file_location().line as usize;
                    let def_end   = def_range.get_end().get_file_location().line as usize;
                    Some((def_file_path, def_start, def_end))
                });
                let (df, dl, de) = match maybe_def {
                    Some((f, l, e)) => (Some(f), Some(l), Some(e)),
                    None            => (None, None, None),
                };
                (
                    Some(indexed_path.to_string()),
                    Some(start_line as usize),
                    Some(end_line as usize),
                    df, dl, de,
                )
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
                symbol_role: Some({
                    // Pure-virtual and virtual annotations only apply to methods.
                    // For all other kinds fall back to definition / declaration.
                    let is_method_kind = matches!(
                        kind,
                        EntityKind::Method | EntityKind::Destructor | EntityKind::ConversionFunction
                    );
                    if is_method_kind && entity.is_pure_virtual_method() {
                        "pure_virtual"
                    } else if is_method_kind && entity.is_virtual_method() {
                        "virtual"
                    } else if is_def {
                        "definition"
                    } else {
                        "declaration"
                    }
                    .to_string()
                }),
                declaration_file_path: decl_file,
                declaration_line: decl_line,
                declaration_end_line: decl_end_line,
                definition_file_path: def_file,
                definition_line: def_line,
                definition_end_line: def_end_line,
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

    // ── Phase D: Emit lambda as a synthetic function symbol ─────────────────────
    // Lambdas have no name so the `is_emittable` block (which gates on `get_name()`)
    // skips them.  Emit a lightweight symbol so that call edges attributed to the
    // lambda have a resolvable caller_id in the symbols table.
    if in_indexed_file && kind == EntityKind::LambdaExpr {
        let loc = entity.get_location().map(|l| l.get_file_location());
        let lambda_line = loc.as_ref().map(|l| l.line).unwrap_or(0);
        let lambda_col  = loc.as_ref().map(|l| l.column).unwrap_or(0);
        let lambda_end  = entity
            .get_range()
            .map(|r| r.get_end().get_file_location().line)
            .unwrap_or(lambda_line);
        let lambda_id = entity
            .get_usr()
            .map(|u| u.0)
            .unwrap_or_else(|| format!("__lambda_{}_{}", lambda_line, lambda_col));
        let lambda_name  = format!("<lambda>@{}:{}", lambda_line, lambda_col);
        let lambda_qname = build_qualified_name(&lambda_name, &state.scope_stack);
        let scope_qualified_name = build_scope_qualified_name(&state.scope_stack);
        let scope_kind = state.scope_stack.last().map(|f| f.kind.to_string());
        let parent_id  = state.scope_stack.last().map(|f| f.usr.clone());
        state.symbols.push(Symbol {
            id: lambda_id,
            name: lambda_name,
            qualified_name: lambda_qname,
            symbol_type: "function".to_string(),
            file_path: indexed_path.to_string(),
            line: lambda_line as usize,
            end_line: lambda_end as usize,
            signature: None,
            parameter_count: None,
            scope_qualified_name,
            scope_kind,
            symbol_role: Some("definition".to_string()),
            declaration_file_path: None,
            declaration_line: None,
            declaration_end_line: None,
            definition_file_path: Some(indexed_path.to_string()),
            definition_line: Some(lambda_line as usize),
            definition_end_line: Some(lambda_end as usize),
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

    // ── Emit inheritance relation (base specifier) ──────────────────────────
    // When Clang visits a BaseSpecifier child (e.g. `: public Bar` in `class Foo : public Bar`),
    // the parent class frame is already on scope_stack (pushed before visit_children recursion).
    // `entity.get_reference()` returns the base class entity → its name resolves via the
    // existing normalize_relation_events() pipeline.
    if in_indexed_file && kind == EntityKind::BaseSpecifier {
        if let Some(parent_frame) = state.scope_stack.last() {
            if parent_frame.kind == "class" {
                let base_name = entity
                    .get_reference()
                    .and_then(|r| r.get_name().or_else(|| r.get_display_name()))
                    .unwrap_or_default();
                if !base_name.is_empty() {
                    let line = entity
                        .get_location()
                        .map(|l| l.get_file_location().line)
                        .unwrap_or(0);
                    state.relation_events.push(RawRelationEvent {
                        relation_kind: RawRelationKind::Inheritance,
                        source: RawEventSource::LegacyAst,
                        // libclang resolves types — higher confidence than text-based parsing
                        confidence: RawExtractionConfidence::High,
                        caller_id: Some(parent_frame.usr.clone()),
                        target_name: Some(base_name),
                        call_kind: None,
                        argument_count: None,
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
        return; // BaseSpecifier has no children worth recursing into
    }

    // ── Emit call site ───────────────────────────────────────────────────────
    if in_indexed_file && kind == EntityKind::CallExpr {
        let fl = entity.get_location().map(|l| l.get_file_location());
        let call_line = fl.as_ref().map(|l| l.line).unwrap_or(0);
        let call_col  = fl.as_ref().map(|l| l.column).unwrap_or(0);

        // Phase D: lambdas in libclang expose body nodes both as children of
        // `LambdaExpr` AND as siblings of it, so the same `CallExpr` may be
        // visited twice.  Use pre-scanned lambda_ranges to determine the
        // correct (innermost) caller and skip the site on the second visit.
        let effective_caller_id: Option<String> = {
            let lambda_caller = state.lambda_ranges.iter()
                .filter(|(start, end, _)| call_line >= *start && call_line <= *end)
                .min_by_key(|(start, end, _)| end - start)
                .map(|(_, _, usr)| usr.clone());
            lambda_caller.or_else(|| state.current_function_usr.clone())
        };

        if let Some(caller_id) = effective_caller_id {
            // Deduplicate: same physical call site visited more than once
            // (sibling + child of LambdaExpr).
            if state.seen_call_sites.insert((call_line, call_col)) {
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
                        // Extract USR for direct callee resolution (avoids name-based ambiguity)
                        let pre_resolved_callee_id = callee.get_usr().map(|u| u.0);
                        // Extract qualifier from the callee's semantic parent
                        let (qualifier, qualifier_kind) = callee.get_semantic_parent()
                            .and_then(|parent| {
                                let parent_name = parent.get_display_name()
                                    .or_else(|| parent.get_name())?;
                                let qk = match parent.get_kind() {
                                    EntityKind::ClassDecl
                                    | EntityKind::StructDecl
                                    | EntityKind::ClassTemplate => Some(RawQualifierKind::Type),
                                    EntityKind::Namespace => Some(RawQualifierKind::Namespace),
                                    _ => None,
                                }?;
                                Some((Some(parent_name), Some(qk)))
                            })
                            .unwrap_or((None, None));
                        state.raw_calls.push(RawCallSite {
                            caller_id,
                            called_name,
                            call_kind,
                            argument_count: arg_count,
                            argument_texts: Vec::new(),
                            result_target: None,
                            receiver: None,
                            receiver_kind: None,
                            qualifier,
                            qualifier_kind,
                            pre_resolved_callee_id,
                            file_path: indexed_path.to_string(),
                            line: call_line as usize,
                        });
                    }
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

    // Phase D: pre-scan the TU for lambda line ranges before the main walk.
    // This ensures correct caller attribution for CallExpr nodes that libclang
    // exposes as siblings of their LambdaExpr (before we enter lambda scope).
    let mut lambda_ranges: Vec<(u32, u32, String)> = Vec::new();
    pre_scan_lambdas(tu.get_entity(), &mut lambda_ranges);

    let mut state = VisitorState {
        symbols: Vec::new(),
        raw_calls: Vec::new(),
        relation_events: Vec::new(),
        scope_stack: Vec::new(),
        current_function_usr: None,
        lambda_ranges,
        seen_call_sites: std::collections::HashSet::new(),
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
        relation_events: state.relation_events,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::RawRelationKind;

    fn parse(source: &str) -> ParseResult {
        // Use .cpp extension so libclang infers C++ language mode (not C).
        // workspace_root = None is fine since is_in_main_file() handles location
        // matching without path comparison.
        parse_cpp_file("test.cpp", source, &[], None).expect("parse failed")
    }

    #[test]
    fn clang_extracts_single_inheritance() {
        let result = parse("class Base {}; class Derived : public Base {};");
        let inh: Vec<_> = result
            .relation_events
            .iter()
            .filter(|e| e.relation_kind == RawRelationKind::Inheritance)
            .collect();
        assert!(!inh.is_empty(), "no inheritance events emitted");
        assert_eq!(
            inh[0].target_name.as_deref(),
            Some("Base"),
            "wrong base class name"
        );
    }

    #[test]
    fn clang_extracts_multiple_inheritance() {
        let result = parse("class A {}; class B {}; class C : public A, public B {};");
        let inh: Vec<_> = result
            .relation_events
            .iter()
            .filter(|e| e.relation_kind == RawRelationKind::Inheritance)
            .collect();
        assert_eq!(inh.len(), 2, "expected two inheritance events, got {}", inh.len());
        let bases: Vec<_> = inh.iter().filter_map(|e| e.target_name.as_deref()).collect();
        assert!(bases.contains(&"A"), "missing base A");
        assert!(bases.contains(&"B"), "missing base B");
    }

    #[test]
    fn clang_inheritance_caller_id_is_derived_class_usr() {
        let result = parse("class Base {}; class Derived : public Base {};");
        let derived_sym = result
            .symbols
            .iter()
            .find(|s| s.name == "Derived")
            .expect("Derived symbol not found");
        let inh = result
            .relation_events
            .iter()
            .find(|e| e.relation_kind == RawRelationKind::Inheritance)
            .expect("no inheritance event");
        assert_eq!(
            inh.caller_id.as_deref(),
            Some(derived_sym.id.as_str()),
            "caller_id should be Derived's USR"
        );
    }

    #[test]
    fn clang_no_inheritance_events_for_plain_class() {
        let result = parse("class Standalone { void foo(); };");
        let inh_count = result
            .relation_events
            .iter()
            .filter(|e| e.relation_kind == RawRelationKind::Inheritance)
            .count();
        assert_eq!(inh_count, 0);
    }

    // ── Phase B: missing symbol types ────────────────────────────────────────

    fn sym<'a>(result: &'a crate::models::ParseResult, name: &str) -> Option<&'a crate::models::Symbol> {
        result.symbols.iter().find(|s| s.name == name)
    }

    #[test]
    fn clang_indexes_union() {
        let result = parse("union Variant { int i; float f; };");
        let s = sym(&result, "Variant").expect("Variant symbol not found");
        assert_eq!(s.symbol_type, "class");
    }

    #[test]
    fn clang_indexes_type_alias() {
        let result = parse("using MyInt = int;");
        let s = sym(&result, "MyInt").expect("MyInt symbol not found");
        assert_eq!(s.symbol_type, "type_alias");
    }

    #[test]
    fn clang_indexes_typedef() {
        let result = parse("typedef unsigned long ulong;");
        let s = sym(&result, "ulong").expect("ulong symbol not found");
        assert_eq!(s.symbol_type, "type_alias");
    }

    #[test]
    fn clang_indexes_enum_constant() {
        let result = parse("enum Color { Red, Green, Blue };");
        let s = sym(&result, "Red").expect("Red symbol not found");
        assert_eq!(s.symbol_type, "enum_value");
    }

    #[test]
    fn clang_indexes_field_inside_class() {
        let result = parse("class Foo { int x; float y; };");
        let x = sym(&result, "x").expect("field x not found");
        assert_eq!(x.symbol_type, "field");
        assert_eq!(x.scope_qualified_name.as_deref(), Some("Foo"));
        let y = sym(&result, "y").expect("field y not found");
        assert_eq!(y.symbol_type, "field");
    }

    #[test]
    fn clang_does_not_index_local_variable_as_field() {
        // Local variables inside function bodies must NOT be emitted as fields.
        let result = parse("void foo() { int local = 0; (void)local; }");
        assert!(sym(&result, "local").is_none(), "local variable should not be indexed");
    }

    // ── Phase C: declaration / definition metadata ────────────────────────────

    #[test]
    fn clang_definition_fills_definition_file_path() {
        // A class definition (has body) must fill definition_file_path.
        let result = parse("class Foo { int x; };");
        let s = sym(&result, "Foo").expect("Foo not found");
        assert_eq!(s.symbol_role.as_deref(), Some("definition"));
        assert!(s.definition_file_path.is_some(), "definition_file_path should be set");
        assert!(s.declaration_file_path.is_none(), "declaration_file_path should be None for definitions");
        assert_eq!(s.definition_line, Some(1));
    }

    #[test]
    fn clang_declaration_fills_declaration_file_path() {
        // A forward declaration (no body) must fill declaration_file_path.
        let result = parse("class Bar;");
        let s = sym(&result, "Bar").expect("Bar not found");
        assert_eq!(s.symbol_role.as_deref(), Some("declaration"));
        assert!(s.declaration_file_path.is_some(), "declaration_file_path should be set");
        assert_eq!(s.declaration_line, Some(1));
    }

    #[test]
    fn clang_declaration_cross_links_definition_in_same_tu() {
        // When declaration and definition are both in the same TU, the declaration
        // entity's get_definition() should find the definition → definition_file_path set.
        let result = parse("class Baz; class Baz { int x; };");
        // The forward declaration entity should have definition_file_path filled.
        // (There may be two Baz symbols: one declaration, one definition.)
        let decl = result.symbols.iter().find(|s| {
            s.name == "Baz" && s.symbol_role.as_deref() == Some("declaration")
        });
        if let Some(d) = decl {
            // definition_file_path should be set via get_definition()
            assert!(
                d.definition_file_path.is_some(),
                "declaration should cross-link to definition in same TU"
            );
        }
        // At minimum the definition itself must exist.
        let def = result.symbols.iter().find(|s| {
            s.name == "Baz" && s.symbol_role.as_deref() == Some("definition")
        });
        assert!(def.is_some(), "Baz definition should be indexed");
    }

    // ── Phase E: virtual / override annotations ───────────────────────────────

    #[test]
    fn clang_pure_virtual_method_role() {
        let result = parse("class IBase { virtual void tick() = 0; };");
        let s = result.symbols.iter().find(|s| s.name == "tick")
            .expect("tick not found");
        assert_eq!(s.symbol_role.as_deref(), Some("pure_virtual"),
            "pure virtual method should have role 'pure_virtual'");
    }

    #[test]
    fn clang_virtual_method_role() {
        let result = parse("class Base { virtual void update(); }; void Base::update() {}");
        // The declaration inside class body is virtual (not pure).
        let decl = result.symbols.iter().find(|s| s.name == "update" && s.symbol_role.as_deref() == Some("virtual"));
        assert!(decl.is_some(), "virtual method declaration should have role 'virtual'");
    }

    #[test]
    fn clang_non_virtual_method_role_is_not_virtual() {
        let result = parse("class Foo { void run(); };");
        let s = result.symbols.iter().find(|s| s.name == "run")
            .expect("run not found");
        assert!(
            s.symbol_role.as_deref() != Some("virtual") &&
            s.symbol_role.as_deref() != Some("pure_virtual"),
            "non-virtual method should not be 'virtual' or 'pure_virtual', got {:?}", s.symbol_role
        );
    }

    // ── Phase D: Lambda scope attribution ────────────────────────────────────

    #[test]
    fn clang_lambda_emitted_as_symbol() {
        let result = parse("void outer() { auto lam = [](){}; }");
        let lambda = result.symbols.iter().find(|s| s.name.starts_with("<lambda>"));
        assert!(lambda.is_some(), "lambda should be emitted as a symbol");
        let l = lambda.unwrap();
        assert_eq!(l.symbol_type, "function", "lambda symbol_type should be 'function'");
    }

    #[test]
    fn clang_lambda_call_attributed_to_lambda_not_outer() {
        let result = parse(r#"
void helper();
void outer() {
    auto lam = [&](){ helper(); };
}
"#);

        let outer_sym = result.symbols.iter().find(|s| s.name == "outer")
            .expect("outer not found");
        let lambda_sym = result.symbols.iter().find(|s| s.name.starts_with("<lambda>"))
            .expect("lambda symbol not found");

        // helper() called inside the lambda — should be attributed to lambda, NOT outer
        let outer_calls_helper = result.raw_calls.iter()
            .any(|c| c.caller_id == outer_sym.id && c.called_name == "helper");
        let lambda_calls_helper = result.raw_calls.iter()
            .any(|c| c.caller_id == lambda_sym.id && c.called_name == "helper");

        assert!(!outer_calls_helper,
            "helper() inside lambda must NOT be attributed to outer()");
        assert!(lambda_calls_helper,
            "helper() inside lambda must be attributed to the lambda");
    }

    #[test]
    fn clang_lambda_parent_id_is_outer_function() {
        let result = parse("void outer() { auto lam = [](){}; }");
        let outer_sym = result.symbols.iter().find(|s| s.name == "outer")
            .expect("outer not found");
        let lambda_sym = result.symbols.iter().find(|s| s.name.starts_with("<lambda>"))
            .expect("lambda symbol not found");
        assert_eq!(
            lambda_sym.parent_id.as_deref(),
            Some(outer_sym.id.as_str()),
            "lambda's parent_id should be outer function's USR"
        );
    }
}
