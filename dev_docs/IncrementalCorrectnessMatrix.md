# Incremental Correctness Matrix

## 1. Purpose

This document defines the correctness contract for incremental indexing in CodeAtlas.

It is the acceptance reference for Milestone 4 before deeper planner, watcher, and recovery changes are implemented.

The goal is not to describe only current behavior. The goal is to define the minimum behavior that later Milestone 4 work must preserve or achieve.

---

## 2. Current Incremental Model

Current implementation behavior is based on:

- file path identity in the `files` table
- content-hash comparison for tracked files on disk
- file-level delete and rewrite for changed paths
- post-refresh cleanup of dangling calls and references
- targeted re-resolution for files whose call/reference edges became dangling

Current implementation does not yet provide first-class handling for:

- rename and move continuity
- explicit header fanout reasoning
- branch-switch detection
- watcher burst classification beyond debounce
- planner diagnostics that explain why a broader refresh was chosen

Those areas are covered by later Milestone 4 epics. This matrix defines the expected final behavior they must converge toward.

---

## 3. Global Correctness Invariants

Every incremental run should preserve the following invariants:

1. No symbol, call, or reference record may remain for a file that no longer exists in the workspace.
2. No call or reference may point to a symbol ID that no longer exists after the run completes.
3. Files not affected by the change must keep their symbol, call, and reference state unless they were intentionally re-resolved for correctness.
4. If a change cannot be handled safely with narrow incremental work, CodeAtlas must prefer a broader refresh or rebuild recommendation over silently stale state.
5. Planner decisions must be explainable in logs and test expectations.
6. Failed incremental work must not leave the published index in a partially corrupted state.

---

## 4. Scenario Matrix

| Scenario | Planner expectation | Required refresh scope | Expected DB outcome | Notes |
| --- | --- | --- | --- | --- |
| Edit inside existing `.cpp` without symbol signature change | classify file as `to_index` | changed file, plus any files whose dangling relations require re-resolution | raw symbols, merged symbols, calls, references, and file record for the edited file are replaced; unaffected files remain untouched | baseline edit path |
| Edit inside existing `.cpp` with symbol rename or signature change | classify file as `to_index` | changed file, plus affected callers/references discovered after symbol refresh | stale calls/references to removed symbol IDs are deleted; impacted files are re-resolved or swept until dangling edges are cleared | key acceptance case for resolver safety |
| Add new implementation file | classify file as `to_index` | new file only, plus any impacted relation resolution | new file record, raw symbols, merged symbols, calls, and references appear; existing records remain valid | should not trigger broad sweep by default |
| Delete implementation file | classify file as `to_delete` | deleted file, plus files affected by dangling call/reference cleanup | deleted file record and all records owned by the file are removed; impacted callers/references are re-resolved or removed | no stale rows for deleted path |
| Rename or move implementation file with unchanged content | treat as delete + add at minimum; later planner may collapse into content-assisted move handling | old path and new path, plus affected relations | old path records disappear, new path records appear, final symbol/call/reference graph matches a clean rebuild | continuity optimization is optional; correctness is mandatory |
| Header-only edit that changes declarations used elsewhere | classify header as `to_index`; trigger conservative fanout policy when dependent state may be stale | header file plus dependent sources or a broader consistency sweep | merged symbols and relation edges reflect the new declaration set; no stale call/reference edges remain due to declaration drift | exact fanout strategy is defined in `M4-E4` |
| Header-only edit with no semantic effect | classify header as `to_index` | header file only, unless planner cannot prove safety | index remains correct; planner may still choose conservative broader refresh | correctness-first beats minimal work |
| Add new header file that is not yet included anywhere | classify file as `to_index` | new header only | new declarations become searchable; unrelated files remain untouched | no false fanout required |
| Delete header file | classify file as `to_delete`; trigger conservative dependent refresh or rebuild recommendation | deleted header and dependent sources, or a broader safe fallback | header declarations disappear; dependent stale symbols and relations are cleared or rebuilt safely | high-risk scenario |
| Split declaration/definition update across header and source | both files become `to_index` | both changed files plus affected dependents if symbol IDs/relations shift | declaration/definition pairing remains consistent in merged symbols and references | must match clean rebuild semantics |
| Parser failure on one changed file during incremental run | changed file is attempted, then run fails safely | no published partial state | published index remains readable and equivalent to the last successful state; failure is logged clearly | later enforced in `M4-E6` |
| Temporary unreadable file during editor save/replace | watcher should debounce and retry instead of publishing broken state | narrow retry path first | final state reflects the completed save, not the transient temp-file state | later enforced in `M4-E5` |
| Branch switch or mass repository churn | planner or watcher detects abnormal change burst | either consistency sweep or full rebuild recommendation/trigger | final state must not silently drift stale; heavy path choice is logged | later enforced in `M4-E7` |
| Generated-file churn that touches many tracked files | planner may continue incrementally up to threshold, then escalate | bounded incremental work or broader safe fallback | final index remains correct; logs explain escalation | threshold defined later |

---

## 5. Expected Outcomes by Record Type

### Raw symbols

- Replaced for every `to_index` file.
- Removed for every `to_delete` file.
- Never retained for paths that no longer exist.

### Merged symbols

- Refreshed for all symbol IDs that were removed or recreated by changed paths.
- Must not retain merged representatives that only depended on removed raw symbols.
- Declaration/definition pairing must remain internally consistent after header/source edits.

### Calls

- Rewritten for changed files.
- Deleted for removed files.
- Any dangling call edges created by symbol removal must be either deleted or re-resolved before the run completes.

### References

- Rewritten for changed files.
- Deleted for removed files.
- Any dangling reference edges created by symbol removal must be either deleted or re-resolved before the run completes.

### File records

- Added for newly indexed files.
- Updated for changed files with the latest content hash and symbol count.
- Deleted for removed files.
- Used as planner state, not as a source of truth that can outlive the workspace.

---

## 6. Escalation Rules

When correctness cannot be guaranteed with narrow incremental work, CodeAtlas should escalate in this order:

1. re-resolve directly affected files
2. run a broader consistency sweep for dependent or suspicious files
3. recommend or trigger a full rebuild

Escalation is required when any of the following is true:

- declaration-heavy header changes make dependent state ambiguous
- rename or move churn prevents confident file identity reasoning
- branch-like repository transitions exceed safe incremental assumptions
- parser or filesystem failures prevent a trustworthy narrow update

---

## 7. How Later Milestone 4 Epics Use This Matrix

- `M4-E2` must encode these scenarios as deterministic regression fixtures.
- `M4-E3` must make planner output directly assertable against these expectations.
- `M4-E4` must formalize header fanout behavior for the header scenarios above.
- `M4-E5` must make watcher behavior converge toward the temporary-file and burst-save expectations.
- `M4-E6` must guarantee the parser-failure and DB-safety expectations.
- `M4-E7` must define thresholds and logging for branch-switch and mass-change escalation.

---

## 8. Exit Condition for M4-E1

`M4-E1` is complete when this matrix is accepted as the reference contract for incremental behavior and later Milestone 4 work is evaluated against it.
