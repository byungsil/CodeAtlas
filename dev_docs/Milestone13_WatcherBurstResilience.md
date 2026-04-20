# Milestone 13. Watcher Burst Resilience

Status:

- Completed

## 1. Objective

Harden CodeAtlas watcher behavior for real MCP operation when files change repeatedly in short bursts.

This milestone focuses on:

- keeping watcher-driven database updates correct under high edit frequency
- reducing redundant incremental work during bursty save patterns
- preventing watcher event backlog from growing without bound while indexing is already in progress
- improving reader/writer coexistence during active MCP usage
- proving the new behavior with burst-oriented validation instead of relying on reasoning alone

Success outcome:

- rapid repeated edits do not corrupt the index
- watcher does not repeatedly perform obviously redundant full planning work for the same burst
- MCP query behavior remains stable while watcher updates are happening
- large or branch-like churn still escalates safely when incremental handling is no longer efficient

Positioning note:

- Milestone 11 hardened persisted reference correctness
- Milestone 12 improved query usability and operational dashboard visibility
- MS13 should build on that by improving runtime freshness and watcher resilience during real editing sessions

---

## 2. Applicability Review

Current watcher behavior is already functionally safe in several ways:

- events are debounced
- changed paths are deduplicated in a `HashSet`
- incremental updates are transactional
- large bursts can escalate to full rebuild

However, current operation still has clear burst-mode risks:

- raw file-system events can continue to accumulate while indexing is already running
- incremental planning still rescans the whole workspace rather than starting from the changed-set
- repeated save bursts can trigger repeated expensive planner work even when a later run supersedes earlier events
- write-heavy update windows can increase read pressure for MCP consumers

These are meaningful operational issues for real MCP use and belong in the next milestone.

Included in MS13:

1. watcher burst coalescing and in-flight event compression
2. changed-set-aware incremental planning
3. burst-aware escalation rules
4. reader/writer coexistence hardening for live MCP usage
5. burst simulation and operational validation

Explicitly not in scope for MS13:

- generalized distributed indexing
- multi-workspace watcher orchestration
- language-semantic correctness improvements unrelated to watcher behavior
- replacing the current index format or storage backend

---

## 3. Recommended Order

1. M13-E1. Watcher Event Compression And In-Flight Coalescing
2. M13-E2. Changed-Set-Aware Incremental Planning
3. M13-E3. Burst-Aware Escalation Policy
4. M13-E4. Read/Write Coexistence Hardening
5. M13-E5. Burst Validation And Operational Proof

Why this order:

- event compression is the first runtime control point and reduces wasted work immediately
- planner narrowing should happen before escalation tuning so thresholds are based on a better substrate
- escalation rules should be tuned after the watcher/planner behavior is clearer
- read/write hardening should follow once write cadence is stabilized
- validation should be last so it measures the final behavior rather than intermediate drafts

---

## 4. Epic Breakdown

### M13-E1. Watcher Event Compression And In-Flight Coalescing

Goal:

- prevent raw watcher events from causing repeated redundant incremental runs during bursty editing

Problems to solve:

- the current watcher receives events continuously even while indexing is in progress
- reindex work is serialized, but incoming events can still pile up behind it
- save patterns like temp-file rename, repeated write bursts, or formatter rewrites can create redundant churn

Target behavior:

- maintain a stable "dirty paths" set across active indexing
- compress repeated events for the same normalized path
- preserve correctness without trying to replay every raw notify event one by one

Implementation areas:

- [indexer/src/watcher.rs](/E:/Dev/CodeAtlas/indexer/src/watcher.rs)

Expected changes:

- split watcher state into:
  - pending paths not yet scheduled
  - in-flight paths currently being processed
  - dirty-during-run paths observed while indexing is running
- after one indexing cycle finishes, immediately fold any dirty-during-run paths into the next cycle
- ensure logging reflects compressed batch behavior rather than noisy raw-event counts

Acceptance:

- repeated updates to the same file during one indexing cycle result in at most one extra follow-up cycle for that file
- no correctness regression for create/modify/remove/rename flows

### M13-E2. Changed-Set-Aware Incremental Planning

Goal:

- avoid rescanning the entire workspace for every small watcher burst when the changed-set is already known

Problems to solve:

- current `run_incremental_index(...)` rebuilds the file list and replans against the whole workspace every time
- this favors correctness, but becomes wasteful during active edit loops

Target behavior:

- incremental planning should start from the known changed paths and only widen scope where needed
- whole-workspace discovery should remain available as a fallback, not the default for every watcher cycle

Implementation areas:

- [indexer/src/watcher.rs](/E:/Dev/CodeAtlas/indexer/src/watcher.rs)
- [indexer/src/incremental.rs](/E:/Dev/CodeAtlas/indexer/src/incremental.rs)
- [indexer/src/discovery.rs](/E:/Dev/CodeAtlas/indexer/src/discovery.rs)

Expected changes:

- add a changed-set-driven incremental planning path
- reuse existing stored file records for most decisions
- widen scope for header fanout, rename detection, or missing-state recovery only when required
- keep full discovery as the safe fallback for ambiguous cases

Acceptance:

- a small local edit does not require full workspace discovery by default
- header-induced or rename-induced widening still behaves correctly

### M13-E3. Burst-Aware Escalation Policy

Goal:

- make escalation decisions reflect real burst pressure rather than only the current per-run changed count

Problems to solve:

- current escalation mainly looks at the immediate changed count and planner output
- high-frequency edits across a short window can still produce inefficient repeated incrementals before escalation triggers

Target behavior:

- escalation policy should consider compressed churn over a recent watcher window
- branch-like churn, rename-heavy churn, or sustained backlog should escalate earlier and more predictably

Implementation areas:

- [indexer/src/watcher.rs](/E:/Dev/CodeAtlas/indexer/src/watcher.rs)
- [indexer/src/incremental.rs](/E:/Dev/CodeAtlas/indexer/src/incremental.rs)

Expected changes:

- introduce short-window burst accounting
- distinguish:
  - small repeated save noise
  - sustained high-churn editing
  - branch-like or mass-change events
- improve watcher log messages so escalation reasons are more operationally meaningful

Acceptance:

- sustained high churn escalates to full rebuild earlier than repeated inefficient incrementals
- small save bursts still remain incremental

### M13-E4. Read/Write Coexistence Hardening

Goal:

- reduce live-query disruption while watcher writes are happening under MCP usage

Problems to solve:

- current writer side uses SQLite with `journal_mode=DELETE`
- server already has retry/snapshot fallback, but write bursts can still make read behavior less stable than necessary

Target behavior:

- MCP queries remain usable while watcher updates are active
- reader behavior under write contention is explicit and testable

Implementation areas:

- [indexer/src/storage.rs](/E:/Dev/CodeAtlas/indexer/src/storage.rs)
- [server/src/storage/sqlite-store.ts](/E:/Dev/CodeAtlas/server/src/storage/sqlite-store.ts)
- [server/src/mcp-runtime.ts](/E:/Dev/CodeAtlas/server/src/mcp-runtime.ts)

Expected changes:

- evaluate whether writer-side WAL is now safe and beneficial
- if DELETE mode remains necessary, tighten retry/snapshot behavior and operational logging
- surface watcher-write pressure in runtime stats or dashboard if useful

Acceptance:

- concurrent MCP reads remain successful under active watcher updates in burst tests
- read fallback behavior is measurable rather than implicit

### M13-E5. Burst Validation And Operational Proof

Goal:

- prove watcher stability with realistic burst scenarios instead of ad hoc manual confidence

Problems to solve:

- current tests cover correctness cases, but not enough burst-frequency behavior
- we need proof for both correctness and operational efficiency

Target behavior:

- deterministic tests and real-workspace validations cover rapid edit bursts

Implementation areas:

- [indexer/src/watcher.rs](/E:/Dev/CodeAtlas/indexer/src/watcher.rs)
- [server/src/__tests__](/E:/Dev/CodeAtlas/server/src/__tests__)
- `samples/`
- real workspaces such as `E:\Dev\opencv` and `E:\Dev\llvm-project-llvmorg-18.1.8`

Expected validation:

- synthetic burst tests for repeated writes to the same file
- synthetic mixed bursts for create/remove/rename/format-save patterns
- MCP read smoke while watcher is updating
- real workspace watcher validation on at least one medium or large project

Acceptance:

- no DB corruption or dangling core tables caused by burst operation
- watcher settles to the latest state after sustained bursts
- performance is materially better than repeated full-scan incremental churn

---

## 5. Recommended First Slice

Start with M13-E1 and keep the first change intentionally narrow.

Suggested first implementation slice:

1. introduce explicit watcher run-state and dirty-path buckets
2. compress repeated normalized paths while an indexing cycle is active
3. after a cycle completes, schedule exactly one immediate follow-up run if new dirty paths arrived during the previous run
4. add focused tests for:
   - repeated writes to one file
   - temp-file replacement followed by real-file rename
   - burst arrival during active indexing

Why this is the right first slice:

- it improves burst behavior immediately
- it does not require planner redesign up front
- it gives clearer runtime data before changing escalation or storage policy

---

## 6. Risks And Guardrails

Risks:

- over-aggressive compression could hide real delete/create ordering issues
- partial planner narrowing could miss legitimate wider fallout
- WAL changes could help reads but alter watcher write assumptions

Guardrails:

- correctness wins over minimizing work
- full rebuild remains the safe fallback
- all burst optimizations must preserve the invariant that the final DB reflects the latest workspace state
- validation must include real MCP read activity, not just standalone indexer runs

---

## 7. Completion Criteria

MS13 can be considered complete when all of the following are true:

- watcher burst handling is explicitly coalesced rather than implicitly serialized
- small rapid edits no longer force obviously redundant whole-workspace incremental planning
- sustained high churn escalates predictably
- MCP reads remain stable during watcher write activity
- automated and real-workspace burst validation both pass

## 8. Completion Note

Implemented outcomes:

- watcher now compresses paths observed during an active indexing cycle and schedules one immediate follow-up run instead of relying only on delayed raw-event replay
- watcher incremental mode now uses changed-set-aware planning for common non-header edits and falls back to wider discovery only when needed
- watcher escalation now considers sustained recent churn in addition to single-batch size
- SQLite writer/read-only coexistence was hardened with WAL mode, busy timeouts, and WAL-aware snapshot fallback helpers
- burst-oriented unit tests were added for watcher state, queued-event compression, changed-set planning, and WAL snapshot handling

Validation performed:

- repeated per-epic Rust unit and full test runs
- repeated per-epic server test runs
- final full project validation:
  - indexer: `168/168`
  - server: `142/142`
- real workspace rebuild validation:
  - `E:\Dev\opencv`
  - `E:\Dev\llvm-project-llvmorg-18.1.8`
- real workspace watcher smoke validation on both workspaces
- final SQLite integrity checks:
  - OpenCV: `ok`
  - LLVM: `ok`
