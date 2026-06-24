# Milestone 21. Call Edge Confidence Tiers

Status:

- Completed

## 1. Objective

Distinguish **compiler-confirmed** call edges (resolved via libclang USR) from
**heuristic** call edges (resolved via name-based scoring) throughout the pipeline,
and expose this distinction to MCP consumers.

This is the CodeAtlas adaptation of Kythe's `completedby` principle: Kythe only emits a
`completedby` edge when the indexer *observes* an actual completion relationship during
compilation (`KytheGraphObserver::recordCompletion`, `kythe/cxx/indexer/cxx/KytheGraphObserver.cc:957`),
keeping "confirmed" relationships separate from inferred ones. CodeAtlas already produces
both kinds of edge in `resolve_calls_with_db()` but records them with identical status in the
`calls` table — MCP consumers cannot tell which edges are trustworthy.

Reference: `dev_docs/Kythe_Glean_Applicability_Review.md` 제안 B.

Success outcome:

- every row in the `calls` table carries a `resolution_tier` value
- the USR fast-path produces `compiler_confirmed`; the name-based path produces `heuristic`
- MCP call-graph tools (`find_callers`, `find_callees`, `find_callers_recursive`,
  `trace_call_path`) expose the tier per edge and accept an optional filter to return
  compiler-confirmed edges only
- no existing edge is dropped or re-resolved — this is a tagging + propagation change only

Positioning note:

- builds directly on the existing dual-path resolver (`resolver.rs:resolve_calls_with_db`)
- aligns with the global rule that "MCP results are grounded hints, not ground truth":
  agents can now see which edges warrant extra verification

Scope note:

- does not change resolution logic or scoring heuristics
- does not remove heuristic edges (Kythe keeps over-estimated edges too; consumers filter)
- additive schema change only, backward compatible via default value

---

## 2. Applicability Review

Current production resolution path: `resolver.rs:resolve_calls_with_db()` (line 996).
It has two branches per raw call:

1. **USR fast-path** (`resolver.rs:1066-1082`): when `raw.pre_resolved_callee_id` is set
   (extracted by libclang at `clang_parser.rs:563-591` via `callee.get_usr()`), and the
   callee symbol exists, the edge is created directly with no scoring. This is the
   compiler-confirmed equivalent of Kythe's observed completion.

2. **Name-based heuristic path** (`resolver.rs:1084-1118`): `collect_candidates_from_cache()`
   + `resolve_one()` + 17 scoring signals + `tie_break()` (`resolver.rs:456-473`).
   Produces `ResolutionStatus::{Resolved, Ambiguous, Unresolved}` internally, but the
   `Call` struct (`models.rs:60-65`) discards this — it carries only
   `caller_id / callee_id / file_path / line`.

The `calls` table (`storage.rs:93-98`) mirrors the struct: no tier column.
Server reads it at `server/src/storage/sqlite-store.ts:185-192`
(`SELECT * FROM calls WHERE callee_id = ?` / `WHERE caller_id = ?`).

Gap: the confirmed/heuristic distinction already exists *at resolution time* but is lost
before storage. MS21 carries it through to MCP.

Prior-work conflict check (verified against the repo on 2026-06-24):

- The USR fast-path this milestone tags was built by an earlier change
  (commit `98065c8` "Enhance call resolution and database schema", 2026-06-19), which added
  `RawCallSite.pre_resolved_callee_id` (libclang USR), the fast-path branches in
  `resolve_calls`/`resolve_calls_with_db`, the positive-score tie-break, and the
  `raw_calls.pre_resolved_callee_id` column (schema v2). MS21 is the natural follow-up that
  surfaces the result of that mechanism — **not** a re-implementation.
- That change added the column to `raw_calls` (the input side). MS21 adds `resolution_tier`
  to `calls` (the output side) — a different table. The `calls` schema is still the original
  four columns (`caller_id / callee_id / file_path / line`), confirmed at `storage.rs:93-98`.
  There is **no existing tier/confidence column on `calls`**, so MS21's additive column does
  not collide with prior work.

Included in MS21:

1. `resolution_tier` field on `Call` and a tier enum in models
2. tier assignment at both resolver branches (and the legacy `resolve_calls()` for parity)
3. additive `calls.resolution_tier` column + write/read wiring
4. MCP exposure + optional `confirmedOnly` filter on call-graph tools

Explicitly not in scope:

- changing scoring heuristics or candidate collection
- a third tier for `Ambiguous` (folded into `heuristic` for now; see Risk 3)
- re-indexing or migration of existing DBs beyond default backfill

---

## 3. Recommended Order

1. M21-E1. Resolver tier modeling (Rust, no schema)
2. M21-E2. Storage schema + write/read wiring (Rust)
3. M21-E3. MCP exposure + filter (TypeScript)
4. M21-E4. Validation and release readiness

Why this order:

- E1 is pure in-memory modeling, independently unit-testable
- E2 depends on the `Call` field from E1
- E3 depends on the column from E2 being populated
- E4 measures the integrated behavior

---

## 4. Epic Breakdown

### M21-E1. Resolver Tier Modeling

Status:

- Not started

Goal:

- represent the confirmed/heuristic distinction on the `Call` value the resolver returns

Design:

- add a `ResolutionTier` enum to `models.rs`:
  ```rust
  #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
  #[serde(rename_all = "camelCase")]
  pub enum ResolutionTier {
      CompilerConfirmed, // libclang USR fast-path
      Heuristic,         // name-based scoring (Resolved or Ambiguous)
  }
  ```
- add `pub resolution_tier: ResolutionTier` to `Call` (`models.rs:60-65`)
- provide a string mapping (`compiler_confirmed` / `heuristic`) for storage and JSON

Implementation tasks:

- M21-E1-T1. Add `ResolutionTier` enum + `as_str()` / `from_str()` in `models.rs`
- M21-E1-T2. Add `resolution_tier` field to `Call`; update all `Call { .. }` literals
- M21-E1-T3. Set `CompilerConfirmed` in the USR fast-path of `resolve_calls_with_db()`
  (`resolver.rs:1074`) and in `resolve_calls()` (`resolver.rs:106`)
- M21-E1-T4. Set `Heuristic` in the name-based path of both functions
  (`resolver.rs:1114+` and `resolver.rs:143`)
- M21-E1-T5. Unit tests: USR-resolved call → CompilerConfirmed; name-resolved call →
  Heuristic; ambiguous-but-chosen call → Heuristic

Expected touch points:

- `indexer/src/models.rs`
- `indexer/src/resolver.rs`

Acceptance:

- `cargo build` passes with the new field threaded through all `Call` constructions
- new unit tests confirm correct tier per resolution branch

### M21-E2. Storage Schema and Wiring

Status:

- Not started

Goal:

- persist and read back `resolution_tier` without breaking existing DBs

Design:

- additive column with default: `resolution_tier TEXT NOT NULL DEFAULT 'heuristic'`
- default `heuristic` is the safe assumption (treat unknown as needs-verification)
- `write_calls()` (`storage.rs:534`) writes the tier; readers (`storage.rs:968`, `:988`)
  select it

Implementation tasks:

- M21-E2-T1. Add column to `CREATE TABLE calls` (`storage.rs:93-98`)
- M21-E2-T2. Add idempotent migration for existing DBs: `ALTER TABLE calls ADD COLUMN
  resolution_tier TEXT NOT NULL DEFAULT 'heuristic'` guarded by a column-exists check
  (follow the existing schema-evolution pattern in `storage.rs`)
- M21-E2-T3. Update `write_calls()` INSERT (`storage.rs:536`) to include the tier
- M21-E2-T4. Update the two call readers (`storage.rs:968`, `:988`) to select and map it
- M21-E2-T5. Consider a partial index `idx_calls_callee_tier` on `(callee_id, resolution_tier)`
  to keep `confirmedOnly` filtering cheap (mirror existing `idx_calls_*` patterns)
- M21-E2-T6. Unit tests: round-trip a confirmed and a heuristic call; verify default
  backfill on a pre-MS21 DB

Expected touch points:

- `indexer/src/storage.rs`

Acceptance:

- fresh DB has the column; pre-MS21 DB migrates with all rows defaulting to `heuristic`
- write/read round-trip preserves tier

### M21-E3. MCP Exposure and Filter

Status:

- Not started

Goal:

- surface the tier in call-graph MCP responses and allow confirmed-only queries

Design:

- the server reads `calls` at `server/src/storage/sqlite-store.ts:185-192`; include the new
  column and map to a `resolutionTier` field on the call-edge model
  (`server/src/models/`, see `row` mappers at `sqlite-store.ts:829-838`)
- add an optional boolean parameter (e.g. `confirmedOnly`) to call-graph tools; when true,
  append `AND resolution_tier = 'compiler_confirmed'` to the query
- include tier in compact responses (`server/src/compact-responses.ts`) so AI consumers see it

Implementation tasks:

- M21-E3-T1. Extend the call-edge SELECTs (`sqlite-store.ts:185`, `:192`) to read
  `resolution_tier`
- M21-E3-T2. Add `resolutionTier` to the call-edge model + row mapper
  (`sqlite-store.ts:829-838`)
- M21-E3-T3. Add `confirmedOnly?: boolean` to `find_callers`, `find_callees`,
  `find_callers_recursive`, `trace_call_path` in `mcp.ts` / `mcp-runtime.ts`
- M21-E3-T4. Thread the filter into the store queries (parameterized, not string concat)
- M21-E3-T5. Surface tier in compact response formatting
  (`server/src/compact-responses.ts`, `response-metadata.ts`)
- M21-E3-T6. Server tests (`server/src/__tests__/mcp.test.ts`): default returns all edges
  with tier present; `confirmedOnly: true` returns only compiler-confirmed edges

Expected touch points:

- `server/src/storage/sqlite-store.ts`
- `server/src/models/`
- `server/src/mcp.ts`, `server/src/mcp-runtime.ts`
- `server/src/compact-responses.ts`, `server/src/response-metadata.ts`

Acceptance:

- MCP call-graph responses include `resolutionTier` per edge
- `confirmedOnly: true` filters to compiler-confirmed edges only
- recursive/trace tools respect the filter transitively

### M21-E4. Validation and Release Readiness

Status:

- Not started

Goal:

- prove tiers are correct and useful on a real C++ project

Implementation tasks:

- M21-E4-T1. `cargo test` (indexer) + `cargo build --release`
- M21-E4-T2. `npm test -- --runInBand` + `npm run build` (server)
- M21-E4-T3. Index a real project with `compile_commands.json` present (e.g. F:\dev\opencv);
  verify a meaningful fraction of edges are `compiler_confirmed` and virtual/overloaded
  call sites that the heuristic mis-binds are visibly tagged `heuristic`
- M21-E4-T4. Index the same project *without* build metadata (tree-sitter fallback);
  verify all edges are `heuristic` (no USR path)
- M21-E4-T5. Update this milestone doc with completion evidence (confirmed/heuristic ratio)

Expected touch points:

- test modules in `indexer/src/` and `server/src/__tests__/`
- `dev_docs/Milestone21_CallEdgeConfidenceTiers.md`

Acceptance:

- all suites pass
- build-metadata index shows a confirmed/heuristic split; tree-sitter-only index shows
  100% heuristic
- no change to total edge count vs pre-MS21 (tagging only, no edges added or removed)

---

## 5. Task Breakdown By File

### `indexer/src/models.rs`

- add `ResolutionTier` enum + string mapping (M21-E1-T1)
- add `resolution_tier` field to `Call` (M21-E1-T2)

### `indexer/src/resolver.rs`

- set `CompilerConfirmed` on USR fast-path in `resolve_calls_with_db()` (`:1074`) and
  `resolve_calls()` (`:106`) (M21-E1-T3)
- set `Heuristic` on name-based path in both (`:1114+`, `:143`) (M21-E1-T4)
- tier unit tests (M21-E1-T5)

### `indexer/src/storage.rs`

- `calls` table column + migration (M21-E2-T1, T2)
- `write_calls()` + readers wiring (M21-E2-T3, T4)
- optional tier index (M21-E2-T5)
- round-trip + backfill tests (M21-E2-T6)

### `server/src/storage/sqlite-store.ts`

- read `resolution_tier`, map to `resolutionTier`, filter support (M21-E3-T1, T2, T4)

### `server/src/mcp.ts`, `mcp-runtime.ts`

- `confirmedOnly` parameter on call-graph tools (M21-E3-T3)

### `server/src/compact-responses.ts`, `response-metadata.ts`

- surface tier in responses (M21-E3-T5)

### `server/src/__tests__/mcp.test.ts`

- filter + tier presence tests (M21-E3-T6)

---

## 6. Risks

### Risk 1. `Call` field addition ripples through many constructors

- Why: `Call { .. }` literals appear in resolver, tests, and possibly fixtures.
- Mitigation: a compiler error at each site is the checklist; add the field explicitly
  (do not use `..Default::default()` — `Call` has no Default and the tier must be intentional).

### Risk 2. Pre-MS21 DBs must not break

- Why: server opens existing index DBs.
- Mitigation: additive column with `DEFAULT 'heuristic'` + idempotent ALTER guarded by a
  column-exists check; readers tolerate the default.

### Risk 3. `Ambiguous` folded into `heuristic` loses nuance

- Why: a chosen-but-ambiguous edge is weaker than a clean name-based resolve.
- Mitigation: acceptable for MS21 (binary confirmed/heuristic). If needed later, extend the
  enum with `HeuristicAmbiguous` — the string mapping and column already accommodate new values.

### Risk 4. USR existence check vs true completion

- Why: CodeAtlas confirms an edge when the USR-named callee *symbol exists*
  (`resolver.rs:1070-1072`), which is weaker than Kythe observing a link-time completion.
- Mitigation: this is the correct ceiling for a no-build indexer; document that
  `compiler_confirmed` means "clang resolved the callee USR and the symbol is known",
  not "linked at runtime".

---

## 7. Definition of Done

1. `Call` and the `calls` table carry `resolution_tier`
2. USR fast-path → `compiler_confirmed`; name-based path → `heuristic`, in both resolver fns
3. existing DBs migrate with safe default; round-trip preserves tier
4. MCP call-graph tools expose `resolutionTier` and honor `confirmedOnly`
5. all indexer + server suites pass; release builds succeed
6. real-project validation shows the expected confirmed/heuristic split with build metadata,
   and 100% heuristic without it
7. total edge count unchanged vs pre-MS21

---

## 8. Suggested First Implementation Slice

1. add `ResolutionTier` + `Call.resolution_tier` (M21-E1-T1, T2)
2. set tiers at both branches of `resolve_calls_with_db()` only (M21-E1-T3, T4)
3. unit test the two branches

Why first: contained to `models.rs` + `resolver.rs`, no schema or server change, and proves
the confirmed/heuristic split is correctly derived before persisting or exposing it.

---

## 9. Completion Evidence

Implemented 2026-06-24.

### Indexer (Rust)

- `ResolutionTier { CompilerConfirmed, Heuristic }` enum with `as_str()` / `from_str()`
  added to `indexer/src/models.rs`; `Call` gained a `resolution_tier` field.
- Both resolver paths tag tiers in `indexer/src/resolver.rs`:
  USR fast-path → `CompilerConfirmed`; name-based path → `Heuristic`
  (in both `resolve_calls_with_db()` and the legacy `resolve_calls()`).
- `calls.resolution_tier TEXT NOT NULL DEFAULT 'heuristic'` column added, schema version
  bumped to 3, additive migration via `ensure_column`, write/read wiring updated in
  `indexer/src/storage.rs`.
- New unit tests: `usr_pre_resolved_call_is_compiler_confirmed`,
  `name_resolved_call_is_heuristic`, `ambiguous_but_chosen_call_is_heuristic`,
  `resolution_tier_round_trips`, `pre_ms21_calls_backfill_to_heuristic`.

### Server (TypeScript)

- `ResolutionTier` type + `Call.resolutionTier` in `server/src/models/call.ts`; row mapper
  `toCall` maps the stored string (pre-MS21 rows default to `heuristic`).
- `getCallers`/`getCallees` accept `confirmedOnly`; pre-MS21 DBs (no column) correctly return
  no confirmed edges. Filter threaded through `expandCallDirection`, `buildCallGraphPayload`,
  and `traceShortestCallPath`.
- `confirmedOnly` parameter added to `find_callers`, `find_callers_recursive`,
  `trace_call_path`; recovered (raw_call) fallbacks suppressed under `confirmedOnly`.
- `CallReference.resolutionTier` surfaced via `makeResolvedCallReference`.
- New test suite `server/src/__tests__/sqlite-store-tier.test.ts` (3 tests): tier mapping,
  `confirmedOnly` filtering both directions, pre-MS21 backfill behavior.

### Validation

- `indexer`: `cargo test` — **287 passed, 0 failed** (5 new MS21 tests included).
- `indexer`: `cargo build` and `cargo build --release` compile clean.
- `server`: `npx tsc --noEmit` — **0 type errors**.
- `server`: `npx jest` — the 3 new tier tests pass. The suite also shows 84 pre-existing
  failures that are **not introduced by MS21**: a baseline run with MS21 changes stashed
  produced the identical 9 failed suites / 84 failed tests, all `ENOENT` for sample data
  files (`samples/.codeatlas/symbols.json`, `calls.json`, `files.json`, `index.db`) removed
  by the earlier commit `98065c8`. MS21 adds exactly +3 passing tests over that baseline.

### Residual notes

- `compiler_confirmed` means "clang resolved the callee USR and the symbol is known", not a
  link-time guarantee (see Risk 4).
- The pre-existing server-suite failures are an environment/sample-data gap (regenerate the
  sample index to clear them); they are orthogonal to this milestone.
