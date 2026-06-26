# Milestone 26. Call Edge Overload Resolution Improvement

Status: In progress (2026-06-26).

## Goal

Reduce heuristic call edge mismatch rate by fixing three structural gaps
in `clang_parser.rs` that prevent the resolver's existing scoring signals
from firing on C++ call sites.

The resolver already has the scoring logic; the problem is that the parser
never populates the data those signals depend on.

### Success criteria

- Fixture suite: new overload-resolution fixtures pass.
- `samples` 5/5 unchanged.
- `cargo test` 0 failures.
- On the 19,379-file workspace: heuristic call edge count stable or
  reduced; cc_confirmed count stable or increased.

---

## Background

After MS25 the workspace call table looks like this:

| resolution_tier   | count     | share  |
|---|---|---|
| compiler_confirmed | 594,104  | 76.3 % |
| heuristic          | 184,266  | 23.7 % |

`compiler_confirmed` edges are resolved by libclang's USR — they are
correct by construction. The 184,266 heuristic edges go through
`resolve_calls` in `resolver.rs`, which scores candidates using 17+
signals and picks the highest-scoring one.

The scoring infrastructure works. The problem is upstream: three fields
that feed the highest-value scoring signals are never populated for C++
call sites.

---

## The Three Gaps

### Gap 1 — `argument_texts` always empty for C++ (highest impact)

`clang_parser.rs:772`:
```rust
argument_texts: Vec::new(),  // hardcoded
```

Every other parser (Lua, Python, Rust, TypeScript) calls
`split_arguments(args)` and fills this field. C++ does not.

`push_argument_signature_reasons` in `resolver.rs` awards +90 points
when an argument's leaf identifier matches the parameter type name
(e.g. `shotFlags` → `ShotFlags`), and +70 points for a bool literal
matching a bool parameter. With an empty `argument_texts` this function
returns immediately and contributes zero score to every C++ call site.

This is the most impactful gap because the score boost (+70 or +90)
exceeds the arity-match signal (+60) that is the current primary
overload discriminator.

**Fix**: extract argument text from each `Entity` in
`entity.get_arguments()` at the CallExpr site.

libclang's `entity.get_arguments()` on a `CallExpr` returns the
*actual* argument expressions, not the formal parameters. Each
argument entity can be probed:

1. If it is a `DeclRefExpr` or `MemberRefExpr` → `entity.get_name()`.
2. Walk its subtree for the first `DeclRefExpr` or `MemberRefExpr` leaf.
3. Fallback: `entity.get_display_name()` (may include type decoration;
   acceptable because `argument_leaf_identifier` in resolver.rs already
   strips to the last identifier token).

The result is a `Vec<String>` that goes into `argument_texts`, exactly
as Lua/Python/Rust/TypeScript produce it.

### Gap 2 — `call_kind` ignores the `qualifier` already extracted (medium impact)

`clang_parser.rs:744–749`:
```rust
let call_kind = match callee.get_kind() {
    EntityKind::Method | EntityKind::Constructor | EntityKind::Destructor
        => RawCallKind::MemberAccess,
    _   => RawCallKind::Unqualified,
};
// qualifier and qualifier_kind are extracted right below
let (qualifier, qualifier_kind) = callee.get_semantic_parent()...;
```

When a call is written as `Ns::func()` or `Type::method()`, libclang
provides the qualifier via `get_semantic_parent()`. The qualifier is
extracted, but `call_kind` is already set to `MemberAccess` or
`Unqualified` — never `Qualified`.

In `push_receiver_aware_reasons` the `Qualified` branch awards
`qualified_type_match` (+90) or `qualified_namespace_match` (+70),
which are the strongest overload discriminators when the call site
spells out the scope. `MemberAccess` only gets
`member_call_prefers_method` (+30).

**Fix**: compute `qualifier` and `qualifier_kind` first, then derive
`call_kind`:
```rust
let call_kind = if qualifier.is_some() {
    RawCallKind::Qualified
} else {
    match callee.get_kind() {
        EntityKind::Method | ... => RawCallKind::MemberAccess,
        _ => RawCallKind::Unqualified,
    }
};
```

### Gap 3 — `this->method()` never classified as `ThisPointerAccess` (lower impact)

`this->method()` calls are emitted as `MemberAccess` (callee is a
`Method`). `push_receiver_aware_reasons` has a `ThisPointerAccess`
branch that awards +80 points when the callee's parent matches the
caller's parent — a stronger boost than `MemberAccess`'s +30. But the
branch never fires because the call_kind never reaches it.

Detection: within a method body, if the current scope stack's innermost
`class` frame's USR matches the callee's semantic parent USR, the call
is effectively a `this->` dispatch.

```rust
let is_this_dispatch = matches!(callee.get_kind(), EntityKind::Method)
    && state.scope_stack.iter().rev()
        .find(|f| f.kind == "class")
        .and_then(|f| callee.get_semantic_parent())
        .map(|p| p.get_usr().map(|u| u.0).as_deref() == Some(frame.usr.as_str()))
        .unwrap_or(false);
```

**Fix**: set `call_kind = RawCallKind::ThisPointerAccess` when
`is_this_dispatch` is true, before the qualifier check.

Priority order: `ThisPointerAccess` > `Qualified` > `MemberAccess` /
`Unqualified`.

---

## Implementation Plan

All changes are in `indexer/src/clang_parser.rs` unless noted.

### Step 1 — Fixtures (measurement baseline)

Add fixtures to `eval/fixtures/` that cover the three gaps before any
code change. These start as XFAIL with `xfail_pending = "ms26-gaps"`;
they flip to OK when the corresponding fix lands.

Fixtures needed:

- `argument_text_overload` — two overloads differing only by parameter
  type name; argument variable name matches one.
- `qualified_call_overload` — `Ns::func(x)` where two namespaces have
  a `func`; qualifier resolves unambiguously.
- `this_dispatch_overload` — `this->update(dt)` in a class that
  inherits from a base also having `update(dt)`.

### Step 2 — Gap 1: C++ argument_texts

Add helper `extract_cpp_argument_text(arg: &Entity<'_>) -> String` in
`clang_parser.rs`:

```rust
fn extract_cpp_argument_text(arg: &Entity<'_>) -> String {
    // Direct name for simple variable/field references.
    let kind = arg.get_kind();
    if matches!(kind, EntityKind::DeclRefExpr | EntityKind::MemberRefExpr) {
        if let Some(name) = arg.get_name() {
            return name;
        }
    }
    // Walk subtree for first named reference leaf.
    let mut found: Option<String> = None;
    arg.visit_children(|child, _| {
        if found.is_some() { return EntityVisitResult::Break; }
        if matches!(child.get_kind(), EntityKind::DeclRefExpr | EntityKind::MemberRefExpr) {
            if let Some(name) = child.get_name() {
                found = Some(name);
                return EntityVisitResult::Break;
            }
        }
        EntityVisitResult::Recurse
    });
    if let Some(name) = found { return name; }
    // Fallback: display name (may carry type decoration, resolver handles it).
    arg.get_display_name().unwrap_or_default()
}
```

At the CallExpr site, replace:
```rust
argument_texts: Vec::new(),
```
with:
```rust
argument_texts: entity.get_arguments()
    .unwrap_or_default()
    .iter()
    .map(|arg| extract_cpp_argument_text(arg))
    .collect(),
```

`arg_count` is already computed from `entity.get_arguments()` — reuse
that iterator rather than calling it twice.

### Step 3 — Gap 2: call_kind ← qualifier

At the CallExpr site, reorder: compute `qualifier` and `qualifier_kind`
first, then derive `call_kind` from them:

```rust
let (qualifier, qualifier_kind) = callee.get_semantic_parent()
    .and_then(|parent| { ... }); // unchanged logic

let call_kind = if qualifier.is_some() {
    RawCallKind::Qualified
} else {
    match callee.get_kind() {
        EntityKind::Method | EntityKind::Constructor | EntityKind::Destructor
            => RawCallKind::MemberAccess,
        _ => RawCallKind::Unqualified,
    }
};
```

### Step 4 — Gap 3: ThisPointerAccess

Before the qualifier check, detect this-dispatch:

```rust
let callee_parent_usr: Option<String> = callee
    .get_semantic_parent()
    .and_then(|p| p.get_usr())
    .map(|u| u.0);

let enclosing_class_usr: Option<&str> = state.scope_stack.iter().rev()
    .find(|f| f.kind == "class")
    .map(|f| f.usr.as_str());

let is_this_dispatch = matches!(callee.get_kind(), EntityKind::Method)
    && callee_parent_usr.as_deref() == enclosing_class_usr;

let call_kind = if is_this_dispatch {
    RawCallKind::ThisPointerAccess
} else if qualifier.is_some() {
    RawCallKind::Qualified
} else {
    match callee.get_kind() {
        EntityKind::Method | EntityKind::Constructor | EntityKind::Destructor
            => RawCallKind::MemberAccess,
        _ => RawCallKind::Unqualified,
    }
};
```

### Step 5 — PARSER_VERSION_TAG bump

Bump `"cpp-clang-v2"` → `"cpp-clang-v3"` to invalidate cached
`RawCallSite` entries that have `argument_texts: []` and wrong
`call_kind`.

### Step 6 — Tests

Unit tests in `clang_parser.rs`:

- `clang_argument_texts_extracted_for_decl_ref` — simple variable
  argument → name captured.
- `clang_argument_texts_extracted_for_nested_ref` — argument wrapped in
  implicit cast → subtree walk finds name.
- `clang_call_kind_qualified_when_qualifier_present` — `Ns::func()`
  emits `Qualified` not `MemberAccess`.
- `clang_call_kind_this_dispatch_detected` — method call inside same
  class emits `ThisPointerAccess`.

---

## Files Changed

| file | change |
|---|---|
| `indexer/src/clang_parser.rs` | add `extract_cpp_argument_text`, fix argument_texts, call_kind, ThisPointerAccess, bump PARSER_VERSION_TAG |
| `eval/fixtures/samples.toml` (or new `overloads.toml`) | new overload fixtures |
| `dev_docs/Milestone26_CallEdgeOverloadResolution.md` | this document |

No changes to `resolver.rs`, `models.rs`, server, or DB schema.

---

## Build / Test Protocol

Use debug binary during development:

```sh
# build
C:/Users/byulee/.rustup/toolchains/stable-x86_64-pc-windows-msvc/bin/cargo.exe \
  build --manifest-path F:/dev/CodeAtlas/indexer/Cargo.toml

# test
C:/Users/byulee/.rustup/toolchains/stable-x86_64-pc-windows-msvc/bin/cargo.exe \
  test --manifest-path F:/dev/CodeAtlas/indexer/Cargo.toml

# debug binary
F:/dev/CodeAtlas/indexer/target/debug/codeatlas-indexer.exe
```

Release binary remains untouched until the milestone is ready to ship.

---

## Risks

- **argument_texts subtree walk cost**: `visit_children` on an argument
  entity is bounded by the argument expression depth, which is typically
  1–3 nodes. No performance concern at this scale.
- **Qualifier false-positive on template specialisation**: `callee.get_semantic_parent()`
  for a template method may return the class template itself rather than
  the specialisation. If `parent_name` contains `<...>`, the qualifier
  becomes a template-id string. The resolver's `qualified_type_match`
  check compares by string equality against `symbol.parent_id`, which is
  also a USR-derived string — mismatch is benign (zero score), not a
  regression.
- **ThisPointerAccess false-positive via inheritance**: if a derived
  class overrides a base method and we call `this->method()`, the callee
  entity's semantic parent is the class that *defines* the method, which
  may be the base class — `enclosing_class_usr` is the derived class.
  The two USRs differ → `is_this_dispatch = false` → falls through to
  `MemberAccess`. This is the safe fallback; no regression, slightly
  weaker signal for inherited-method calls.

---

## Out of Scope

- Receiver type inference (`obj.method()` where `obj`'s type is known):
  would require tracking variable-type bindings across the visitor, a
  substantially larger change. Future milestone.
- Improving the resolver's tie-break policy (score-ordered first-wins
  vs. rank-ordered selection). Orthogonal to this milestone.
