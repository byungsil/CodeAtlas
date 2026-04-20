# Milestone 11. Dangling Reference Quality

## 1. Objective

Eliminate misleading dangling `symbol_references` while keeping persisted references strictly in-workspace and exact-targeted.

This milestone focuses on:

- enforcing an internal-only persisted reference contract
- dropping unresolved out-of-workspace references instead of storing misleading dangling rows
- tightening inheritance-target normalization so type relationships point at type symbols
- adding repeatable validation for reference-quality regressions

Success outcome:

- `symbol_references` contains only valid in-workspace exact targets
- unresolved external names are dropped before persistence
- truly invalid targets do not survive indexing

Workspace scope definition:

- "in-workspace" means symbols discovered under the workspace root passed to the current indexing run
- references that do not resolve to symbols discovered within that indexing scope are treated as out-of-workspace and are not persisted

Positioning note:

- this milestone comes after the memory and staging work because the current blocker is no longer scale stability
- it also fits naturally after Milestone 6 and Milestone 7:
  - Milestone 6 introduced richer relation/propagation semantics
  - Milestone 7 established benchmarking and integrity instrumentation
- this milestone should improve data quality before we build more agent-facing workflows on top of generalized references

Current trigger:

- `E:\Dev\CodeAtlas\tmp\opencv_4k\.codeatlas\index.db` showed:
  - `references = 6208`
  - `dangling_references = 1055`
  - `moduleImport = 891`
  - `inheritanceMention = 164`

Interpretation:

- most dangling rows come from Python imports that do not resolve to in-workspace symbols
- the remainder are inheritance targets normalized to constructor-like callables instead of type symbols

---

## 2. Recommended Order

1. M11-E1. Reference semantics and schema contract
2. M11-E2. Internal-only module-import filtering
3. M11-E3. Inheritance target normalization hardening
4. M11-E4. SQLite persistence and cleanup hardening
5. M11-E5. Validation, real-DB checks, and regression harness

Why this order:

- contract and semantics must be fixed first so later work does not guess at transitional shapes
- parser and normalizer fixes should happen before broad validation, otherwise the validation would describe an intermediate state
- validation work should come last so the final checks reflect the intended contract rather than a temporary transition

---

## 3. Epics

### M11-E1. Reference Semantics and Schema Contract

Goal:

- define the authoritative model for valid internal references and invalid references

Status:

- Completed

Completion notes:

- `dev_docs/API_CONTRACT.md` now defines persisted references as internal-only within the workspace root of the current indexing run
- `indexer/src/models.rs` documents `NormalizedReference` as an internal-only persisted model
- E1 does not require server response-model expansion because `target_symbol_id` remains mandatory for persisted references

Implementation tasks:

- define the `symbol_references` target model:
  - valid persisted target:
    - `target_symbol_id` resolves to `symbols.id`
  - invalid or out-of-workspace target:
    - should not be persisted
- document that persisted references remain internal-only for this milestone
- explicitly constrain first-release behavior so unresolved targets are dropped rather than reclassified

Recommended contract for this milestone:

- `moduleImport` may be persisted only when it resolves to an in-workspace symbol
- `typeUsage`, `inheritanceMention`, `functionCall`, `methodCall`, and `classInstantiation` should resolve internally or be dropped as invalid

Expected touch points:

- `dev_docs/API_CONTRACT.md`
- `indexer/src/models.rs`

Validation checklist:

- there is one unambiguous reference-target contract
- the contract says persisted references always carry a valid `target_symbol_id`
- the contract says unresolved targets are dropped

Exit criteria:

- the milestone has a stable internal-only reference model

---

### M11-E2. Internal-Only Module-Import Filtering

Goal:

- keep module-import references only when they resolve to in-workspace symbols

Status:

- Completed

Progress notes:

- unresolved `moduleImport` targets are now filtered against the current workspace symbol set before persistence
- full rebuild, incremental, and watcher paths now apply the internal-only filter before finalized references remain in SQLite

Completion notes:

- rebuilt `tmp/opencv_4k` DB now shows `moduleImport = 0` and `dangling_references = 0`
- unresolved module imports no longer survive as persisted references

Implementation tasks:

- audit how Python imports become `moduleImport` references today
- find the point where unresolved imported module names are normalized into `target_symbol_id`
- if no workspace symbol exists for an import target:
  - drop the normalized reference
  - do not invent fake internal IDs
- ensure internal module imports still resolve to real `target_symbol_id` values when possible
- keep behavior bounded:
  - do not attempt Python environment/package discovery
  - do not attempt third-party package introspection

Recommended implementation sequence:

1. inspect parser-side relation emission for Python imports
2. inspect reference normalization rules in `parser.rs`
3. add explicit unresolved-import drop behavior
4. confirm only internal module imports survive persistence

Expected touch points:

- `indexer/src/python_parser.rs`
- `indexer/src/parser.rs`

Validation checklist:

- out-of-workspace Python imports are no longer persisted as dangling references
- internal Python modules inside the workspace still resolve normally

Exit criteria:

- `moduleImport` rows are persisted only for valid internal targets

---

### M11-E3. Inheritance Target Normalization Hardening

Goal:

- make inheritance references point only at type-like symbols

Status:

- Completed

Progress notes:

- inheritance normalization now rejects non-type targets for `inheritanceMention`
- a focused parser-level regression test now asserts that constructor-like targets are not persisted for inheritance references

Completion notes:

- rebuilt `tmp/opencv_4k` DB shows `inheritance_non_type_targets = 0`
- constructor-like inheritance targets no longer survive in persisted reference data

Implementation tasks:

- inspect how inheritance relation events are emitted from parsing
- inspect how those events are normalized to `NormalizedReference`
- identify why constructor-like targets such as `ThreadPoolProvider::ThreadPoolProvider` are currently chosen
- enforce category-aware normalization for `inheritanceMention`:
  - prefer `class` and `struct` symbols
  - reject `method`, constructor, and free-function targets
- where no valid type target exists:
  - drop the reference
  - or keep the raw relation event only, without persisting an invalid normalized reference

Recommended implementation sequence:

1. reproduce one failing OpenCV inheritance sample in a focused test
2. patch target ranking and selection for `inheritanceMention`
3. add tests for:
  - simple base class
  - names colliding between type and constructor
  - multiple inheritance
  - header and source split declarations

Expected touch points:

- `indexer/src/parser.rs`
- `indexer/src/resolver.rs` if ranking logic is shared there
- `samples/` or new focused fixtures

Validation checklist:

- `inheritanceMention` never persists constructor-like targets
- exact hierarchy queries still behave correctly
- no new false negatives appear in existing inheritance tests

Exit criteria:

- type hierarchy references are normalized to type symbols only

---

### M11-E4. SQLite Persistence and Cleanup Hardening

Goal:

- enforce the internal-only reference contract safely in persistence and cleanup

Status:

- Completed

Progress notes:

- storage now has explicit helpers for filtering persistable `moduleImport` references against the current workspace symbol set
- full rebuild can now rewrite staged references after merged symbols are known

Completion notes:

- persistence now drops unresolved references before finalization rather than relying on post-hoc dangling cleanup
- rebuilt `tmp/opencv_4k` DB shows `dangling_references = 0`, `dangling_calls = 0`, and `dangling_propagation = 0`

Implementation tasks:

- keep `symbol_references` storage internal-only
- update read and write logic in `storage.rs` if invalid rows can still leak through
- update migration helpers only if schema or cleanup semantics require it
- update cleanup logic so any reference without valid source and target symbols is removed

Expected touch points:

- `indexer/src/storage.rs`
- `indexer/src/main.rs`
- DB validation helper logic

Validation checklist:

- old DBs can open and migrate if storage changes are needed
- new DBs persist only valid internal references
- reference cleanup removes unresolved rows consistently

Exit criteria:

- the database layer fully enforces the internal-only reference contract

---

### M11-E5. Validation, Real-DB Checks, and Regression Harness

Goal:

- prove that the new contract removes misleading dangling counts without hiding true problems

Status:

- Completed

Completion notes:

- rebuilt `tmp/opencv_4k` DB now shows `dangling_references = 0`, `dangling_calls = 0`, and `dangling_propagation = 0`
- MCP regression tests stayed green
- live MCP smoke checks against `tmp/opencv_4k/.codeatlas` returned stable `workspace_summary`, `find_references`, and `impact_analysis` responses

Implementation tasks:

- keep the dangling-reference metric strict so persisted references must resolve internally
- keep existing integrity checks for:
  - dangling calls
  - dangling propagation
  - duplicate calls
  - duplicate propagation
- rebuild the `tmp/opencv_4k` sample DB
- rerun MCP smoke checks against the rebuilt DB
- add at least one focused real-DB inspection command or example for future debugging

Recommended validation order:

1. run targeted unit tests
2. rebuild `tmp/opencv_4k`
3. inspect counts and sample rows
4. run MCP tests
5. run live MCP smoke checks on the rebuilt DB

Expected touch points:

- `indexer/src/main.rs`
- `indexer/src/storage.rs`
- `server/src/__tests__/`
- optional helper under `indexer/examples/` if still useful

Validation checklist:

- `dangling_references = 0` on the OpenCV 4k sample
- unresolved external imports are no longer persisted
- inheritance samples no longer point at constructor-like targets
- MCP tests and smoke checks stay green

Exit criteria:

- the milestone has evidence-backed reference-integrity validation

---

## 4. Detailed Execution Plan

This section is the concrete implementation order for actual development work.

### Phase 1. Lock The Contract Before Editing Code

Tasks:

1. Update `dev_docs/API_CONTRACT.md` with the internal-only reference distinction.
2. State clearly that unresolved targets are dropped rather than persisted.
3. Confirm server response shapes do not need external-target fields for this milestone.

Why first:

- this prevents parser, storage, and validation changes from drifting into incompatible transitional shapes

Completion gate:

- one written contract exists for all later code changes

### Phase 2. Fix The Storage Model

Tasks:

1. confirm or tighten `NormalizedReference` and related Rust models around internal-only persistence
2. update SQLite cleanup logic if needed
3. update write and read paths in `storage.rs` if invalid rows can still leak through

Why second:

- parser-side filtering changes need the persistence layer to enforce the same contract

Completion gate:

- new rows persist only valid internal targets

### Phase 3. Fix The Indexer Normalization Logic

Tasks:

1. drop unresolved `moduleImport` targets instead of persisting them
2. harden `inheritanceMention` normalization to prefer type symbols only
3. add focused parser and normalizer tests for both cases

Why third:

- once the contract and persistence behavior are settled, parser-side fixes can be implemented without temporary hacks

Completion gate:

- rebuilt references are semantically correct at the source

### Phase 4. Verify Query Surfaces

Tasks:

1. verify `sqlite-store.ts` still matches the internal-only contract
2. verify `find_references` and `impact_analysis` behavior remains correct
3. add MCP fixture coverage only for the new internal-only behavior

Why fourth:

- query surfaces should be validated against the stabilized stored data even if no response-shape changes are needed

Completion gate:

- client-visible semantics still match the persisted data model

### Phase 5. Run Real-Project Validation

Tasks:

1. rebuild `tmp/opencv_4k`
2. verify:
  - `dangling_references = 0`
  - unresolved external imports are no longer persisted
  - inheritance rows target types only
3. rerun MCP smoke checks
4. capture final notes in the relevant real-project evaluation doc if the result materially changes prior conclusions

Why last:

- this is the proof stage after the contract and implementation stabilize

Completion gate:

- milestone acceptance criteria are met on a real sample DB

---

## 5. Task Breakdown By File

This section is intentionally operational, so implementation can proceed without re-planning.

### `indexer/src/models.rs`

Planned tasks:

- keep normalized reference persistence internal-only and explicit

### `indexer/src/parser.rs`

Planned tasks:

- drop unresolved module-import targets
- tighten inheritance target selection
- add or update focused fixtures and tests

### `indexer/src/storage.rs`

Planned tasks:

- tighten cleanup semantics for internal-only persisted references
- update read and write mapping only if invalid rows can still be constructed

### `indexer/src/main.rs`

Planned tasks:

- keep integrity and inspection queries strict for persisted references
- ensure cleanup and summary output reflects the internal-only contract

### `server/src/storage/sqlite-store.ts`

Planned tasks:

- verify internal-only read assumptions still hold

### `server/src/models/responses.ts`

Planned tasks:

- no response-shape expansion is expected for this milestone

### `server/src/mcp-runtime.ts`

Planned tasks:

- verify `find_references` remains correct with internal-only persisted references
- audit `impact_analysis`

### `server/src/__tests__/`

Planned tasks:

- add MCP fixture coverage for:
  - dropped unresolved module imports
  - inheritance target normalization
  - impact-analysis stability with only internal refs persisted

---

## 6. Acceptance Criteria

MS11 is complete when all of the following are true:

1. `symbol_references` contains only valid internal exact targets.
2. unresolved module imports are no longer persisted as dangling references.
3. `inheritanceMention` rows no longer point at constructor-like or callable targets.
4. real dangling references, if any, still surface as failures.
5. OpenCV 4k rebuild shows:
  - `dangling_calls = 0`
  - `dangling_propagation = 0`
  - `dangling_references = 0`
6. MCP tests still pass.
7. live MCP smoke checks on the rebuilt DB still return useful reference results.

---

## 7. Risks and Guardrails

### Risk 1. Hiding Real Reference Bugs By Dropping Too Broadly

Guardrail:

- unresolved-drop behavior should be limited to cases that are genuinely out of workspace scope
- other categories should remain strict and internally resolvable for this milestone

### Risk 2. Regressing Existing Query Consumers While Tightening Persistence

Guardrail:

- keep the `target_symbol_id` exact-match contract unchanged
- add contract tests before broad real-project validation

### Risk 3. Regressing Type Hierarchy Queries

Guardrail:

- add focused inheritance fixtures before changing normalization behavior
- rerun hierarchy-related tests after every normalization change

### Risk 4. Letting Cleanup Logic Mask Parser Bugs

Guardrail:

- cleanup should be paired with focused parser tests so invalid rows are fixed at source, not only erased after the fact

---

## 8. Non-Goals

This milestone does not include:

- third-party package resolution beyond workspace scope
- Python environment discovery
- general external symbol graph construction
- redesign of all reference categories
- large new agent workflows unrelated to reference integrity

---

## 9. Definition of Done Checklist

Before marking MS11 complete, verify all items below:

- API contract updated
- Rust reference model updated
- SQLite cleanup and persistence behavior updated as needed
- parser and normalizer tests added
- unresolved external module imports dropped
- inheritance targets normalized to type symbols
- MCP tests updated and passing
- OpenCV 4k rebuilt and inspected
- milestone document updated with completion notes

---

## 10. First Implementation Slice Recommendation

The smallest safe first slice is:

1. lock the internal-only reference contract in docs
2. drop unresolved `moduleImport` targets before persistence
3. keep dangling-reference validation strict for persisted rows

Why this slice first:

- it removes the majority of false dangling counts immediately
- it avoids unnecessary schema and server churn
- it gives us a stable base before changing type-normalization behavior

After that, the second slice should be:

1. inheritance normalization hardening
2. MCP regression coverage
3. final real-DB validation
