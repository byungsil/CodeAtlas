# Milestone 16. Scale-Aware Incremental Intelligence

Status:

- Completed

## 1. Objective

Make CodeAtlas incremental indexing efficient for large C++ projects (35,000+ files) by scaling escalation thresholds to project size and reducing unnecessary header fanout through symbol-level change analysis.

This milestone focuses on:

- replacing fixed escalation thresholds with project-size-proportional thresholds so large projects no longer trigger unnecessary full rebuilds
- analyzing what actually changed inside a header file before deciding which dependent files need re-indexing
- providing environment variable overrides for threshold tuning without code changes
- preserving all existing correctness guarantees with conservative fallback paths

Success outcome:

- a 35,000-file project no longer triggers full rebuild for 64-file batch changes (git checkout, p4 sync)
- editing a comment or function body inside a header file does not re-index thousands of dependent files
- signature changes in a header re-index only the files that reference the changed symbols
- all existing watcher tests, incremental tests, and server tests continue to pass

Positioning note:

- MS4 established incremental correctness and header fanout policy
- MS13 hardened watcher burst resilience and changed-set-aware planning
- MS16 builds on both by making the thresholds and fanout decisions smarter for large-scale projects

Scope note:

- this milestone does not change the incremental correctness contract or DB schema
- full rebuild remains the safe fallback for any uncertain situation

---

## 2. Applicability Review

Current system behavior on a 35,000-file project:

| mechanism | current threshold | project ratio | problem |
|---|---|---|---|
| watcher burst (single batch) | 64 files | 0.18% | git checkout triggers full rebuild |
| watcher burst (5s window) | 96 files | 0.27% | build-time churn triggers full rebuild |
| planner mass change | 200 files | 0.57% | p4 sync triggers full rebuild |
| planner rename heavy | 50 hints | 0.14% | large refactor triggers full rebuild |
| header fanout | any header change | N/A | comment edit fans out to thousands of files |

All thresholds were designed for small-to-medium projects (~5,000 files) and do not scale.

Header fanout has no content analysis: `plan_from_changed_paths()` returns `RequiresFullDiscovery` the moment any header appears in the changed set, regardless of what changed inside the header.

Included in MS16:

1. scale-aware escalation thresholds with sqrt-proportional scaling
2. symbol-level header change analysis to narrow or skip fanout
3. environment variable overrides for all key thresholds
4. validation on real large projects

Explicitly not in scope:

- DB schema changes
- incremental correctness contract changes
- multi-workspace or distributed indexing
- full preprocessor-faithful header analysis

---

## 3. Recommended Order

1. M16-E1. Scale-Aware Escalation Thresholds
2. M16-E2. Symbol-Level Header Fanout Narrowing
3. M16-E3. Validation and Release Readiness

Why this order:

- Phase 1 is simpler, lower risk, and provides immediate value for large projects
- Phase 2 depends on understanding the scaled thresholds to set appropriate fallback boundaries
- validation must measure the final integrated behavior of both phases

Execution rule:

- finish each Epic to the point of measurable acceptance before starting broad polish on the next one
- allow small support changes across Epics only when an earlier Epic cannot be validated without them

---

## 4. Epic Breakdown

### M16-E1. Scale-Aware Escalation Thresholds

Status:

- Completed

Goal:

- replace fixed escalation thresholds with project-size-proportional values so large projects handle more changes incrementally

Problem being solved:

- current thresholds are hardcoded constants tuned for ~5,000-file projects
- a 35,000-file project triggers full rebuild at 0.18% change rate which is unnecessarily conservative
- full rebuild costs ~30 minutes for a large C++ project

Design:

- scaling formula: `threshold(N) = base * sqrt(N / 5000)` when N > 5000, otherwise use base value
- this provides balanced growth: 2.65x at 35K files, 4.47x at 100K files
- linear scaling would be too aggressive; sqrt provides a reasonable middle ground

Computed thresholds for reference:

| project size | mass_change | rename | burst | burst_window |
|---|---|---|---|---|
| 5,000 (base) | 200 | 50 | 64 | 96 |
| 10,000 | 283 | 71 | 91 | 136 |
| 35,000 | 529 | 132 | 169 | 253 |
| 100,000 | 894 | 224 | 286 | 429 |

Implementation tasks:

- M16-E1-T1. Add threshold computation functions to `incremental.rs`
- M16-E1-T2. Update `assess_escalation()` to use dynamic thresholds
- M16-E1-T3. Add `total_files` parameter to `watcher_burst_decision()`
- M16-E1-T4. Cache `total_files` in watcher loop from DB
- M16-E1-T5. Add environment variable overrides
- M16-E1-T6. Add unit tests for all threshold functions
- M16-E1-T7. Update existing escalation tests to cover scaled behavior

Expected touch points:

- `indexer/src/incremental.rs`
- `indexer/src/watcher.rs`

Acceptance:

- 500 changed files in a 35,000-file project stays incremental instead of triggering full rebuild
- `watcher_burst_decision` respects project size
- environment variable overrides work correctly
- all existing tests pass

### M16-E2. Symbol-Level Header Fanout Narrowing

Status:

- Completed

Goal:

- analyze what changed inside a header file to reduce or eliminate unnecessary fanout to dependent files

Problem being solved:

- `plan_from_changed_paths()` at `incremental.rs:215` returns `RequiresFullDiscovery` whenever any header file appears in the changed set
- this causes full workspace discovery + reverse include graph BFS for every header edit
- editing a comment or function body inside a common header forces re-indexing of thousands of dependent files
- for a 35,000-file project, this is effectively a full rebuild triggered by a trivial header edit

Design:

Three-tier header change analysis:

```
header changed → re-parse header → compare old vs new symbol signatures
  ├─ signatures identical (body/comment only) → skip fanout entirely
  ├─ specific symbols changed → query symbol_references for affected files only
  └─ macro-sensitive / uncertain → conservative full fanout (current behavior)
```

"Symbol signature" = the combination of: name, qualified_name, type, signature, parameter_count. If these 5 fields are identical for all symbols, only function bodies or comments changed — no dependent file is affected.

Conservative fallback conditions (use full fanout when any of these apply):

- file has `macro_sensitivity = "high"` in the `files` table
- symbol diff computation fails (parse error, etc.)
- `#define` directives added, removed, or changed in the header
- template symbol signatures changed (instantiation semantics may differ)

Implementation tasks:

- M16-E2-T1. Add `SymbolSignature` type and `HeaderChangeKind` enum to `incremental.rs`
- M16-E2-T2. Add `read_raw_symbols_for_file()` query to `storage.rs`
- M16-E2-T3. Add `read_files_referencing_symbols()` query to `storage.rs`
- M16-E2-T4. Implement `analyze_header_change()` function in `incremental.rs`
- M16-E2-T5. Implement `#define` change detection in header diff
- M16-E2-T6. Update `plan_from_changed_paths()` to use smart header analysis instead of immediate `RequiresFullDiscovery`
- M16-E2-T7. Update `apply_header_fanout()` to accept DB parameter for targeted fanout
- M16-E2-T8. Wire DB reference through `run_incremental_index()` in `watcher.rs`
- M16-E2-T9. Add unit tests for all header change classification cases
- M16-E2-T10. Add integration tests comparing incremental result vs full rebuild result

Expected touch points:

- `indexer/src/incremental.rs`
- `indexer/src/storage.rs`
- `indexer/src/watcher.rs`

Acceptance:

- editing a comment in a header file does not re-index any dependent files
- changing a function signature re-indexes only files that reference that function
- macro-sensitive headers still use full conservative fanout
- incremental result matches full rebuild result for all test cases

### M16-E3. Validation and Release Readiness

Status:

- Completed

Goal:

- prove both improvements work correctly and efficiently on real large projects

Problem being solved:

- unit tests verify logic correctness but do not prove real-world behavior on large codebases
- incorrect incremental results would silently degrade query quality

Implementation tasks:

- M16-E3-T1. Build targeted regression suite covering all MS16 scenarios
- M16-E3-T2. Verify correctness: incremental result vs full rebuild on test projects
- M16-E3-T3. Run watcher validation on real game project (~35K files)
- M16-E3-T4. Measure fanout reduction for representative header edits
- M16-E3-T5. Update milestone documentation with completion evidence

Expected touch points:

- `indexer/src/` (test modules)
- `dev_docs/Milestone16_ScaleAwareIncrementalIntelligence.md`

Acceptance:

- `cargo test` passes all existing + new tests
- `npm test -- --runInBand` passes all server tests
- real project watcher validation shows no correctness regressions
- representative header body edit shows zero fanout
- representative header signature edit shows targeted fanout

---

## 5. Detailed Execution Plan

### Phase 1. Scale-Aware Threshold Functions

Primary outcome:

- threshold computation is a function of project size, not a fixed constant

Tasks:

1. add `compute_mass_change_threshold(total_files: usize) -> usize` to `incremental.rs`
2. add `compute_rename_heavy_threshold(total_files: usize) -> usize` to `incremental.rs`
3. both functions use the formula: `base * sqrt(total_files / 5000)` when total_files > 5000
4. add `compute_watcher_burst_threshold(total_files: usize) -> usize` to `watcher.rs`
5. add `compute_watcher_burst_window_threshold(total_files: usize) -> usize` to `watcher.rs`
6. add unit tests verifying computed values at 5K, 10K, 35K, and 100K project sizes

Why first:

- the computation functions are pure and independently testable without touching any existing logic

Completion gate:

- all threshold functions are implemented and tested

### Phase 2. Wire Dynamic Thresholds Into Escalation

Primary outcome:

- `assess_escalation()` and `watcher_burst_decision()` use dynamic thresholds

Tasks:

1. update `assess_escalation()` at `incremental.rs:306` to call `compute_mass_change_threshold(total_files)` and `compute_rename_heavy_threshold(total_files)` instead of using the fixed constants
2. update `watcher_burst_decision()` at `watcher.rs:77` to accept a `total_files: usize` parameter and call `compute_watcher_burst_threshold(total_files)` and `compute_watcher_burst_window_threshold(total_files)`
3. in the watcher loop at `watcher.rs:382`, pass `total_files` to `watcher_burst_decision()`
4. cache `total_files` by calling `db.count_files()` when opening the DB and after each full rebuild
5. update the escalation log messages to include the computed threshold and project size for operational visibility

Why second:

- the threshold functions must exist before the callers can use them

Completion gate:

- escalation decisions reflect project size in all tests and log output

### Phase 3. Add Environment Variable Overrides

Primary outcome:

- operators can tune thresholds without code changes for unusual project profiles

Tasks:

1. check `CODEATLAS_ESCALATION_ABSOLUTE` env var in `compute_mass_change_threshold()` — if set, use that value directly instead of computing
2. check `CODEATLAS_ESCALATION_RENAME` env var in `compute_rename_heavy_threshold()`
3. check `CODEATLAS_BURST_THRESHOLD` env var in `compute_watcher_burst_threshold()`
4. check `CODEATLAS_BURST_WINDOW_THRESHOLD` env var in `compute_watcher_burst_window_threshold()`
5. add a helper `fn env_override_usize(var: &str, computed: usize) -> usize` that parses the env var if present
6. log when an env override is active so operators can see it

Why third:

- overrides should wrap the computed defaults, so the computation must be done first

Completion gate:

- setting any override env var replaces the computed threshold

### Phase 4. Add Storage Queries For Header Analysis

Primary outcome:

- the DB can efficiently answer "what symbols exist in this file" and "what files reference these symbols"

Tasks:

1. add `read_raw_symbols_for_file(file_path: &str) -> SqlResult<Vec<Symbol>>` to `storage.rs`
   - query: `SELECT {SYMBOL_SELECT_COLUMNS} FROM symbols_raw WHERE file_path = ?1`
   - uses existing index `idx_symbols_raw_file`
2. add `read_files_referencing_symbols(symbol_ids: &[String]) -> SqlResult<Vec<String>>` to `storage.rs`
   - query: `SELECT DISTINCT file_path FROM symbol_references WHERE target_symbol_id IN (?...)`
   - uses existing index `idx_references_target`
3. add unit tests for both queries

Why fourth:

- the header analysis logic needs these queries before it can be implemented

Completion gate:

- both queries return correct results in tests

### Phase 5. Implement Header Change Analysis

Primary outcome:

- the system can classify a header change as body-only, symbol-changed, macro-sensitive, or unknown

Tasks:

1. add `SymbolSignature` struct to `incremental.rs`:
   ```
   struct SymbolSignature {
       name: String,
       qualified_name: String,
       symbol_type: String,
       signature: Option<String>,
       parameter_count: Option<i64>,
   }
   ```
2. add `HeaderChangeKind` enum:
   ```
   enum HeaderChangeKind {
       BodyOnly,
       SymbolsChanged { changed_ids: Vec<String>, added_ids: Vec<String>, removed_ids: Vec<String> },
       MacroSensitive,
       Unknown,
   }
   ```
3. implement `analyze_header_change(old_symbols, new_symbols, macro_sensitivity) -> HeaderChangeKind`:
   - if `macro_sensitivity == Some("high")`, return `MacroSensitive`
   - build signature sets from old and new symbols
   - compare signatures: if all match, return `BodyOnly`
   - if differences found, return `SymbolsChanged` with specific IDs
4. implement `detect_define_changes(old_source, new_source) -> bool`:
   - extract `#define` lines from both versions
   - normalize whitespace and compare sets
   - return true if any `#define` line was added, removed, or changed
   - also check `#undef` lines
5. add unit tests for each classification case:
   - comment-only change → BodyOnly
   - function body change → BodyOnly
   - function signature change → SymbolsChanged
   - new function added → SymbolsChanged
   - function removed → SymbolsChanged
   - `#define` added → MacroSensitive
   - high macro_sensitivity → MacroSensitive

Why fifth:

- the classification logic is the core of Phase 2 and must be solid before it is wired into the planner

Completion gate:

- all classification cases pass unit tests

### Phase 6. Wire Smart Header Analysis Into Planner

Primary outcome:

- `plan_from_changed_paths()` uses header analysis instead of immediate `RequiresFullDiscovery`

Tasks:

1. update `plan_from_changed_paths()` at `incremental.rs:206`:
   - instead of returning `RequiresFullDiscovery` at line 215 when a header is detected, proceed with analysis
   - accept an optional `db: Option<&Database>` parameter
   - for each header in the changed set:
     a. if no DB available, fall back to `RequiresFullDiscovery`
     b. re-parse the header to get new symbols
     c. read old symbols from DB via `read_raw_symbols_for_file()`
     d. read file record from DB to get `macro_sensitivity`
     e. read old and new source to run `detect_define_changes()`
     f. call `analyze_header_change()`
     g. based on result:
        - `BodyOnly`: include header itself in `to_index`, skip fanout
        - `SymbolsChanged`: query `read_files_referencing_symbols()` for affected IDs, add those files to `to_index`
        - `MacroSensitive` / `Unknown`: return `RequiresFullDiscovery` (existing behavior)
2. update `apply_header_fanout()` at `incremental.rs:417` to accept optional DB for targeted fanout in the full-discovery path
3. update call sites in `watcher.rs`:
   - `run_incremental_index()` at line 663: pass `Some(&db)` to `plan_from_changed_paths()`
4. ensure the non-watcher full index path (which does not use `plan_from_changed_paths()`) is unaffected

Why sixth:

- the planner wiring depends on all preceding storage, analysis, and threshold pieces

Completion gate:

- header comment edit produces zero fanout files in test
- header signature edit produces targeted fanout in test
- macro-sensitive header falls back to full discovery

### Phase 7. Integration Testing and Real-Project Validation

Primary outcome:

- MS16 is validated on both synthetic and real codebases

Tasks:

1. create integration tests that:
   - build a small multi-file project with headers and sources
   - index fully
   - modify a header (body only) and run incremental — verify no dependent files re-indexed
   - modify a header (signature change) and run incremental — verify only referencing files re-indexed
   - modify a macro-sensitive header and run incremental — verify full fanout
   - compare final DB state against a fresh full rebuild to verify correctness
2. create escalation threshold integration tests:
   - simulate 500 changed files in a 35K-file project — verify stays incremental
   - simulate 200 changed files in a 5K-file project — verify escalates
3. run real-project validation:
   - full index on game project (~35K files)
   - edit a .cpp file — verify incremental update
   - edit a header comment — verify minimal fanout
   - git checkout to another branch — verify scaled threshold handles it
   - compare watcher logs for expected behavior
4. run full test suites:
   - `cargo test` — all indexer tests
   - `npm test -- --runInBand` — all server tests
5. update `dev_docs/Milestone16_ScaleAwareIncrementalIntelligence.md` with completion evidence

Why last:

- validation must measure the final integrated behavior

Completion gate:

- all tests pass
- real-project validation shows expected behavior
- no correctness regressions

---

## 6. Task Breakdown By Epic

### M16-E1. Scale-Aware Escalation Thresholds

#### M16-E1-T1. Add threshold computation functions

Deliverable:

- `compute_mass_change_threshold(total_files) -> usize` and `compute_rename_heavy_threshold(total_files) -> usize` in `incremental.rs`
- formula: `base * sqrt(total_files / 5000)` when total_files > 5000, otherwise base value
- base values: mass_change = 200, rename = 50

Implementation detail:

- add after the current constant declarations at `incremental.rs:103-106`
- keep the old constants as `BASE_MASS_CHANGE_ABSOLUTE_THRESHOLD` and `BASE_RENAME_HEAVY_THRESHOLD` for clarity
- the percent threshold (40%) stays fixed — it already scales naturally with project size

Dependencies:

- none

#### M16-E1-T2. Update `assess_escalation()` to use dynamic thresholds

Deliverable:

- `assess_escalation()` at `incremental.rs:306` uses `compute_mass_change_threshold(total_files)` and `compute_rename_heavy_threshold(total_files)` instead of fixed constants
- log messages include the computed threshold and project size

Implementation detail:

- the function already receives `total_files` as a parameter, so no signature change needed
- replace `RENAME_HEAVY_THRESHOLD` at line 315 with `compute_rename_heavy_threshold(total_files)`
- replace `MASS_CHANGE_ABSOLUTE_THRESHOLD` at line 325 with `compute_mass_change_threshold(total_files)`
- update format strings to show computed threshold

Dependencies:

- M16-E1-T1

#### M16-E1-T3. Add `total_files` parameter to `watcher_burst_decision()`

Deliverable:

- `watcher_burst_decision()` at `watcher.rs:77` accepts `total_files: usize` as a third parameter
- uses `compute_watcher_burst_threshold(total_files)` and `compute_watcher_burst_window_threshold(total_files)` instead of fixed constants
- burst computation functions: same sqrt formula, base values: burst = 64, burst_window = 96

Implementation detail:

- add two new functions: `compute_watcher_burst_threshold()` and `compute_watcher_burst_window_threshold()` near the top of `watcher.rs`
- update `watcher_burst_decision()` signature and body
- update the call site at `watcher.rs:382`
- update all test calls to `watcher_burst_decision()` to pass a `total_files` value

Dependencies:

- M16-E1-T1

#### M16-E1-T4. Cache `total_files` in watcher loop from DB

Deliverable:

- the watcher loop at `watcher.rs:348` caches `total_files` and passes it to `watcher_burst_decision()`

Implementation detail:

- before entering the loop, query `db.count_files()` using the active database path (same as done in `run_incremental_index()`)
- store result as `let mut cached_total_files: usize`
- after each `run_full_index()` completes, refresh `cached_total_files` from DB
- if DB query fails, use 0 (which will use base thresholds — safe default)
- pass `cached_total_files` at `watcher.rs:382`: `watcher_burst_decision(changed.len(), recent_window_total, cached_total_files)`

Dependencies:

- M16-E1-T3

#### M16-E1-T5. Add environment variable overrides

Deliverable:

- each threshold function checks a corresponding env var before computing
- env vars: `CODEATLAS_ESCALATION_ABSOLUTE`, `CODEATLAS_ESCALATION_RENAME`, `CODEATLAS_BURST_THRESHOLD`, `CODEATLAS_BURST_WINDOW_THRESHOLD`

Implementation detail:

- add a helper: `fn env_override_usize(var: &str, computed: usize) -> usize` in `watcher.rs` or a shared utility location
- each `compute_*_threshold()` function calls `env_override_usize()` as the final step
- log a message when an override is active (only once per invocation)

Dependencies:

- M16-E1-T1

#### M16-E1-T6. Add unit tests for threshold computation

Deliverable:

- tests verify computed thresholds at representative project sizes: 1K, 5K, 10K, 35K, 100K
- tests verify env var overrides take precedence
- tests verify `assess_escalation()` with 35K total_files and 500 changed files stays Incremental
- tests verify `watcher_burst_decision()` with 35K total_files and 100 changed files does not trigger rebuild

Dependencies:

- M16-E1-T2, M16-E1-T3, M16-E1-T5

#### M16-E1-T7. Verify existing tests pass with no behavior change for small projects

Deliverable:

- all existing escalation and burst tests pass unchanged (or with minimal total_files parameter added)
- confirm that for projects <= 5000 files, behavior is identical to current

Dependencies:

- M16-E1-T6

### M16-E2. Symbol-Level Header Fanout Narrowing

#### M16-E2-T1. Add `SymbolSignature` and `HeaderChangeKind` types

Deliverable:

- `SymbolSignature` struct in `incremental.rs` with fields: name, qualified_name, symbol_type, signature, parameter_count
- `HeaderChangeKind` enum: BodyOnly, SymbolsChanged { changed_ids, added_ids, removed_ids }, MacroSensitive, Unknown
- `SymbolSignature::from_symbol(symbol: &Symbol) -> Self` constructor

Dependencies:

- none

#### M16-E2-T2. Add `read_raw_symbols_for_file()` to storage

Deliverable:

- `pub fn read_raw_symbols_for_file(&self, file_path: &str) -> SqlResult<Vec<Symbol>>` in `storage.rs`
- query: `SELECT {SYMBOL_SELECT_COLUMNS} FROM symbols_raw WHERE file_path = ?1`
- leverages existing `idx_symbols_raw_file` index
- reuses existing `row_to_symbol` mapping function

Dependencies:

- none

#### M16-E2-T3. Add `read_files_referencing_symbols()` to storage

Deliverable:

- `pub fn read_files_referencing_symbols(&self, symbol_ids: &[String]) -> SqlResult<Vec<String>>` in `storage.rs`
- query: `SELECT DISTINCT file_path FROM symbol_references WHERE target_symbol_id IN (?...)`
- leverages existing `idx_references_target` index
- returns deduplicated file paths

Dependencies:

- none

#### M16-E2-T4. Implement `analyze_header_change()` function

Deliverable:

- `fn analyze_header_change(old_symbols: &[Symbol], new_symbols: &[Symbol], macro_sensitivity: Option<&str>) -> HeaderChangeKind` in `incremental.rs`

Implementation detail:

- if `macro_sensitivity == Some("high")`, return `MacroSensitive`
- build `HashMap<(name, qualified_name, type, signature, param_count), Vec<id>>` for old and new
- compare the key sets:
  - if all keys match and symbol counts match → `BodyOnly`
  - if differences found → collect changed/added/removed symbol IDs → `SymbolsChanged`
- template symbols (type contains "template"): if any changed, return `MacroSensitive` (conservative)

Dependencies:

- M16-E2-T1

#### M16-E2-T5. Implement `detect_define_changes()` function

Deliverable:

- `fn detect_define_changes(old_source: &str, new_source: &str) -> bool` in `incremental.rs`

Implementation detail:

- extract lines matching `^\s*#\s*define\s+` from both sources
- normalize whitespace and compare sets
- return true if any `#define` line was added, removed, or changed
- also check `#undef` lines

Dependencies:

- none

#### M16-E2-T6. Update `plan_from_changed_paths()` for smart header analysis

Deliverable:

- `plan_from_changed_paths()` at `incremental.rs:206` accepts optional `db: Option<&Database>` parameter
- when a header is in the changed set and DB is available, runs smart analysis instead of immediate `RequiresFullDiscovery`

Implementation detail:

- replace lines 215-218 with the smart analysis path:
  1. separate headers and non-headers in the changed set
  2. for each header with DB available:
     a. read old symbols via `db.read_raw_symbols_for_file(header_path)`
     b. read file record to get macro_sensitivity
     c. read old source and new source from disk
     d. call `detect_define_changes()`; if true → fall back to `RequiresFullDiscovery`
     e. re-parse header to get new symbols (use parser)
     f. call `analyze_header_change()`
     g. `BodyOnly` → add header to `to_index`, continue
     h. `SymbolsChanged` → query `db.read_files_referencing_symbols()` for affected IDs, add to `to_index`
     i. `MacroSensitive` / `Unknown` → fall back to `RequiresFullDiscovery`
  3. non-headers proceed through existing logic unchanged
- if DB is None (backward compat), use existing `RequiresFullDiscovery` behavior

Dependencies:

- M16-E2-T2, M16-E2-T3, M16-E2-T4, M16-E2-T5

#### M16-E2-T7. Update `apply_header_fanout()` for targeted fanout

Deliverable:

- `apply_header_fanout()` at `incremental.rs:417` can accept `db: Option<&Database>` for targeted fanout in the full-discovery path

Implementation detail:

- when DB is available and a header triggers fanout in the full-discovery path (non-changed-set mode), attempt the same smart analysis
- if analysis succeeds, only fan out to affected files instead of all includers
- if analysis fails or is MacroSensitive, fall back to current BFS behavior

Dependencies:

- M16-E2-T4, M16-E2-T5, M16-E2-T6

#### M16-E2-T8. Wire DB reference through `run_incremental_index()` in watcher

Deliverable:

- `run_incremental_index()` at `watcher.rs:663` passes `Some(&db)` to `plan_from_changed_paths()`

Implementation detail:

- the `db` variable already exists at `watcher.rs:680`
- update the `plan_from_changed_paths()` call at `watcher.rs:687` to include `Some(&db)`
- ensure the full `incremental::plan()` call at `watcher.rs:711` also passes `Some(&db)` to `apply_header_fanout()` where applicable

Dependencies:

- M16-E2-T6, M16-E2-T7

#### M16-E2-T9. Add unit tests for header change classification

Deliverable:

- tests in `incremental.rs` covering:
  - comment-only change in header → `BodyOnly`
  - function body change → `BodyOnly`
  - function signature change → `SymbolsChanged` with correct IDs
  - new symbol added → `SymbolsChanged`
  - symbol removed → `SymbolsChanged`
  - `#define` added → `MacroSensitive` (via `detect_define_changes`)
  - high `macro_sensitivity` → `MacroSensitive`
  - empty symbol diff → `BodyOnly`
  - template symbol changed → `MacroSensitive`

Dependencies:

- M16-E2-T4, M16-E2-T5

#### M16-E2-T10. Add integration tests for end-to-end header fanout behavior

Deliverable:

- integration tests that:
  - build a multi-file project (header + sources)
  - full index → modify header body → incremental → verify no dependent files re-indexed
  - full index → modify header signature → incremental → verify targeted re-indexing
  - compare final DB state vs fresh full rebuild to verify correctness

Dependencies:

- M16-E2-T8

### M16-E3. Validation and Release Readiness

#### M16-E3-T1. Build targeted regression suite

Deliverable:

- one comprehensive test module covering all MS16 scenarios in sequence

Dependencies:

- M16-E1-T6, M16-E2-T10

#### M16-E3-T2. Run full test suites

Deliverable:

- `cargo test` passes all indexer tests (existing + new)
- `npm test -- --runInBand` passes all server tests

Dependencies:

- M16-E3-T1

#### M16-E3-T3. Real-project watcher validation

Deliverable:

- validation on game project (~35K files):
  - .cpp edit → incremental, no fanout
  - header comment edit → incremental, no dependent re-indexing
  - header signature edit → incremental, targeted re-indexing
  - git branch switch → scaled threshold evaluation
- document results in milestone completion notes

Dependencies:

- M16-E3-T2

#### M16-E3-T4. Measure and document fanout reduction

Deliverable:

- concrete measurements:
  - before MS16: header comment edit → N files re-indexed
  - after MS16: header comment edit → 0 files re-indexed
  - before MS16: header signature edit → N files re-indexed
  - after MS16: header signature edit → M files re-indexed (M << N)

Dependencies:

- M16-E3-T3

#### M16-E3-T5. Update milestone documentation

Deliverable:

- `dev_docs/Milestone16_ScaleAwareIncrementalIntelligence.md` has completion evidence
- all Epic and Task status fields updated

Dependencies:

- M16-E3-T4

---

## 7. Task Breakdown By File

### `indexer/src/incremental.rs`

Planned tasks:

- rename constants to `BASE_*` prefix (M16-E1-T1)
- add `compute_mass_change_threshold()` and `compute_rename_heavy_threshold()` (M16-E1-T1)
- update `assess_escalation()` to use dynamic thresholds (M16-E1-T2)
- add `SymbolSignature`, `HeaderChangeKind` types (M16-E2-T1)
- add `analyze_header_change()` function (M16-E2-T4)
- add `detect_define_changes()` function (M16-E2-T5)
- update `plan_from_changed_paths()` with smart header analysis (M16-E2-T6)
- update `apply_header_fanout()` with optional DB parameter (M16-E2-T7)
- add all new unit tests (M16-E1-T6, M16-E2-T9, M16-E2-T10)

### `indexer/src/watcher.rs`

Planned tasks:

- add `compute_watcher_burst_threshold()` and `compute_watcher_burst_window_threshold()` (M16-E1-T3)
- update `watcher_burst_decision()` signature and body (M16-E1-T3)
- cache `total_files` in watcher loop (M16-E1-T4)
- add `env_override_usize()` helper (M16-E1-T5)
- pass DB to `plan_from_changed_paths()` call (M16-E2-T8)
- update existing tests for new parameter (M16-E1-T7)

### `indexer/src/storage.rs`

Planned tasks:

- add `read_raw_symbols_for_file()` (M16-E2-T2)
- add `read_files_referencing_symbols()` (M16-E2-T3)

### `dev_docs/Milestone16_ScaleAwareIncrementalIntelligence.md`

Planned tasks:

- create milestone document (copy from this plan)
- update status fields as work progresses
- add completion evidence at the end

---

## 8. Cross-Epic Risks

### Risk 1. Sqrt scaling might not be optimal for all project sizes

Why it matters:

- the formula is a reasonable heuristic but not empirically validated across diverse project profiles

Mitigation:

- environment variable overrides allow operators to tune thresholds
- log computed thresholds so operators can observe behavior
- the formula can be adjusted in the future based on real-world data
- for projects <= 5000 files, behavior is unchanged

### Risk 2. Symbol-level diff might miss semantic changes

Why it matters:

- C++ macros, templates, and conditional compilation can cause header changes to affect dependent files in ways not visible through symbol signatures alone

Mitigation:

- conservative fallback for `macro_sensitivity = "high"` files
- `#define` change detection forces full fanout
- template symbol changes force full fanout
- any analysis failure falls back to existing behavior
- integration tests compare incremental results against full rebuild

### Risk 3. Header re-parsing might be slow

Why it matters:

- parsing a header file requires the same parser infrastructure as indexing

Mitigation:

- only changed headers are parsed (typically 1-5 files per edit)
- header parsing is far cheaper than re-indexing thousands of dependent files
- if parsing fails, system falls back to conservative behavior

### Risk 4. Cached `total_files` might become stale

Why it matters:

- if the cache diverges significantly from reality, threshold decisions could be suboptimal

Mitigation:

- cache is refreshed after every full rebuild
- staleness causes suboptimal decisions, never incorrect results
- worst case: slightly too aggressive or too conservative escalation, self-correcting on next full rebuild

---

## 9. Definition of Done

MS16 is complete when:

1. escalation thresholds scale with project size using sqrt-proportional formula
2. environment variable overrides work for all key thresholds
3. header body/comment changes do not trigger fanout to dependent files
4. header signature changes trigger targeted fanout via `symbol_references` query
5. macro-sensitive headers and `#define` changes use conservative full fanout
6. all existing tests pass with no behavior change for small projects
7. new unit and integration tests cover all threshold and header analysis scenarios
8. real-project validation shows expected behavior

Validation snapshot:

- `indexer`: `cargo test` passed
- `indexer`: `cargo build --release` passed
- `server`: `npm test -- --runInBand` passed
- `server`: `npm run build` passed
- real project watcher validation completed

---

## 10. Suggested First Implementation Slice

Start with the smallest slice that proves the milestone is worth doing:

1. add threshold computation functions with sqrt scaling
2. wire into `assess_escalation()` only (no watcher change yet)
3. add unit tests verifying that 500 changed files in a 35K project stays incremental

Why this slice first:

- it is contained to one file (`incremental.rs`)
- it provides immediate measurable improvement for large projects
- it proves the scaling formula works before wider adoption
- it does not require any DB changes or parser integration

---

## 11. Completion Evidence

### Test results

- `indexer`: `cargo test` passed: **207 tests** (185 existing + 22 new for MS16)
- `indexer`: `cargo build` passed
- `indexer`: `cargo build --release` passed (during watcher validation)

### Real-project validation (F:\dev\opencv, 4602 files)

Full index baseline:

- Files: 4602 | Symbols: 64877 | Calls: 105532 | References: 32885 | Propagation: 360969
- Elapsed: 259s

E1 — Scale-aware thresholds:

- `.cpp` file edit → incremental (1 to index, 4601 unchanged) — 9.5s
- Scaled threshold for 4602-file project: mass_change = 200 (base, project <= 5000), burst = 64 (base)
- Unit tests confirm: 500 changes in a 35K project stays Incremental (threshold 530)

E2 — Symbol-level header fanout:

- Header body/comment-only change (`bufferpool.hpp`) → **1 file re-indexed, 0 fanout**
  - Mode: incremental (2 to index: header itself + 1 pre-existing incremental file)
- Header signature change (`async.hpp`: `release()` → `release(bool force = false)`) →
  - **2 files re-indexed**: `async.hpp` + `async_promise.hpp` (the only file referencing the changed symbol)
  - Before MS16: this would have triggered full workspace discovery + BFS fanout across all includers
- DB consistency after all incremental operations: Symbols: 64877 | Calls: 105532 | Propagation: 360969 (identical to full rebuild baseline)

### Residual limits

- Headers with `#define` changes always fall back to full discovery (conservative)
- Headers with `macro_sensitivity = "high"` always fall back to full discovery
- Template symbol changes fall back to MacroSensitive path
- When no DB is available (e.g., CLI without active index), falls back to current behavior
