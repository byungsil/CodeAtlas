# Milestone 27. Class Hierarchy Analysis for Virtual Call Resolution

Status: Completed (2026-06-26).

## Goal

Close the virtual-dispatch coverage gap in the call graph. When code calls
`base->foo()` where `foo` is virtual, libclang's pre-resolved callee USR points
only at the *static* type's method (`Base::foo`). Every concrete override that
could run at that site (`Derived::foo`, `GrandChild::foo`, …) was missing.

MS27 adds **Class Hierarchy Analysis (CHA)**: each resolved virtual call also
emits an edge to every transitive override in the static type's subtree — a
sound over-approximation (never misses a possible runtime target, may include
some that can't occur).

### Success criteria

- opencv full reindex: symbols/files unchanged; `calls` increases by CHA edges;
  `compiler_confirmed` and `heuristic` counts unchanged (CHA is additive).
- Fixtures: samples 5/5, opencv 5 OK + 2 XFAIL — no regressions.
- `cargo test` and server `jest` green.

---

## Design

Three layers, all reusing existing plumbing:

1. **Parse (`clang_parser.rs`)** — for each method/destructor,
   `Entity::get_overridden_methods()` (clang crate 2.0, wraps
   `clang_getOverriddenCursors`) yields the directly-overridden base methods.
   Each becomes a `RawRelationEvent { MethodOverride, caller_id = derived USR,
   target_name = base USR }`.

2. **Normalize (`parser.rs`) + Store (`storage.rs`)** — `MethodOverride` maps to
   a new `ReferenceCategory::MethodOverride`, persisted as a `symbol_references`
   row (`category = 'methodOverride'`, source = derived id, target = base id).
   Targets are USRs, so normalization resolves them by id directly rather than
   by name. New reads: `find_direct_overriders` (base → deriving) and
   `find_virtual_method_ids`.

3. **Resolve (`resolver.rs`)** — before the parallel pass, build the set of
   virtual callee ids and their transitive override closure (BFS over
   `find_direct_overriders`, depth-capped at `CHA_MAX_CLOSURE_DEPTH = 16`). For
   each resolved call whose callee is virtual, emit the primary edge unchanged
   plus one `ResolutionTier::ChaVirtual` edge per transitive override.

A new tier `cha_virtual` (`ResolutionTier::ChaVirtual`, stored as `cha_virtual`)
keeps the over-approximated edges distinct from `compiler_confirmed` /
`heuristic`. The `calls.resolution_tier` column is free-form TEXT — no DB
migration. `confirmedOnly` queries naturally exclude CHA edges.

Server: `ResolutionTier` union gains `"chaVirtual"`; `toCall` maps the stored
string. No exhaustive switch rejected unknown tiers, so this is purely additive.

### Cache invalidation

`PARSER_VERSION_TAG` bumped `cpp-clang-v3` → `cpp-clang-v4`. The `ParseResult`
now carries override relation events, so stale parse-cache entries must miss.
Without this bump a warm cache replays pre-MS27 blobs and emits zero override
edges (observed during development).

---

## Measured results (opencv, 4,888 files)

| metric | pre-MS27 | MS27 |
|---|---|---|
| symbols | 84,090 | 84,090 |
| files | 4,888 | 4,888 |
| calls (total) | 185,542 | 185,718 |
| compiler_confirmed | 13,778 | 13,778 |
| heuristic | 171,764 | 171,764 |
| **cha_virtual** | — | **176** |
| methodOverride refs | 0 | 1,272 |
| virtual/pure_virtual symbols | 789 | 789 |

CHA edges spot-checked correct: e.g. `Imf::compressTile` → virtual
`Imf::Compressor::compress` expands to the concrete override `Imf::compress`.

Fixtures: samples 5/5 OK; opencv 5 OK + 2 XFAIL (cv::Mat cases unchanged).
Tests: indexer 326 pass (319 + 7 new); server 173 pass.

Incremental mode inherits CHA automatically (same `resolve_calls_with_db`);
verified by touching a file with overrides and confirming edges persist.

---

## Non-Goals

- Function-pointer / `std::function` / lambda-callback resolution.
- RTA pruning of CHA targets by instantiated types (over-approximation accepted).
- A dedicated "find base method this overrides" MCP tool (the `methodOverride`
  edges enable it later).
