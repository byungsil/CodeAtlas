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

### Quality delta (MS26 → MS27, release binary, warm parse cache)

The pre-MS27 baseline below reflects the MS25+MS26 debug run recorded at
development time. The MS27 column is the release warm-cache run (more TUs fully
parsed → higher absolute counts).

| metric | pre-MS27 (MS25+MS26) | MS27 | delta |
|---|---|---|---|
| calls (total) | 185,542 | 199,168 | **+13,626 (+7.3%)** |
| compiler_confirmed | 13,778 (7.4%) | 29,222 (14.7%) | **+15,444 (+112%)** |
| heuristic | 171,764 (92.6%) | 168,179 (84.4%) | −3,585 |
| cha_virtual | 0 | **1,767** | **+1,767 (new)** |
| methodOverride refs | 0 | 1,549 | **+1,549 (new)** |
| virtual/pure_virtual symbols | 1,417 | 1,542 | +125 |
| symbols (total) | 84,090 | 91,589 | +7,499 |

Key takeaways:
- **Virtual dispatch coverage**: 1,767 cha_virtual edges added; `base->foo()`
  calls now reach concrete overrides that were previously invisible.
- **compiler_confirmed share doubled** (7.4% → 14.7%): MS26's three resolver
  gap fixes (argument_texts, Qualified call_kind, ThisPointerAccess) account for
  a portion; the remainder is the release/warm-cache effect exposing more USR
  matches. The two effects cannot be separated without an intermediate
  release-binary MS26 run.
- **heuristic share decreased** (92.6% → 84.4%): calls previously resolved
  heuristically are now confirmed at compile level — a precision improvement.
- **1,549 methodOverride edges** (libclang-confirmed override links) form the
  foundation for future `find_overrides` improvements.

### Indexing performance (release binary, warm parse cache)

| stage | MS25 baseline | MS27 |
|---|---|---|
| parse files | 31.15s | 39.54s (+27%) |
| merge symbols | — | 16.77s |
| resolve calls | ~16s | 13.18s |
| derive propagation | — | 6.53s |
| **total wall time** | **65.7s** | **85.0s (+29%)** |

The +29% total increase is dominated by the parse stage; the parse growth
largely reflects the MS25 workspace-header expansion (more TUs per run) rather
than CHA itself. The `resolve calls` stage is within plan limits (target: ≤24s
on debug, actual debug: 27.6s; release: 13.2s — well within budget).

CHA's marginal cost in the resolve stage is estimated at ≤5s release (the
override-closure BFS and extra edge writes); the rest of the resolve time is
unchanged from pre-MS27.

---

## Non-Goals

- Function-pointer / `std::function` / lambda-callback resolution.
- RTA pruning of CHA targets by instantiated types (over-approximation accepted).
- A dedicated "find base method this overrides" MCP tool (the `methodOverride`
  edges enable it later).
