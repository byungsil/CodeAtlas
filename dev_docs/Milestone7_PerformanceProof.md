# Milestone 7. Performance Proof

## 1. Objective

Prove that CodeAtlas deserves to exist as a specialized large-C++ tool.

This milestone focuses on:

- benchmark design and automation
- query hot-path profiling and optimization
- incremental and watcher scale measurements
- optional build metadata ingestion
- include and macro risk signaling

Success outcome:

- CodeAtlas can prove its large-C++ value proposition with benchmark evidence, not just design intent

Current status:

- `M7-E1` is complete.
- `M7-E2` is complete.
- `M7-E3` is complete.
- `M7-E4` is complete.
- `M7-E5` is complete.
- `M7-E6` is complete.

---

## 2. Recommended Order

1. M7-E1. Benchmark design
2. M7-E2. Benchmark harness implementation
3. M7-E3. Query profiling and hot-path optimization
4. M7-E4. Incremental and watcher scale benchmarks
5. M7-E5. Build metadata ingestion
6. M7-E6. Include and macro risk signals

---

## 3. Epics

### M7-E1. Benchmark Design

Goal:

- define what performance evidence CodeAtlas should collect and publish

Status:

- Completed

Implementation tasks:

- define benchmark dataset tiers:
  - small deterministic fixture
  - medium curated or synthetic sample
  - large real-project benchmark if available
- define required measurements:
  - full index time
  - incremental index time
  - watcher catch-up time
  - query latency
  - DB size
  - memory usage
- define how benchmark environment details are recorded

Expected touch points:

- `dev_docs/`
- benchmark planning notes

Validation checklist:

- benchmark plan is documented before automation work starts

Exit criteria:

- there is a clear benchmark contract for future measurements

Completion summary:

- added a benchmark contract covering small fixture, medium curated, and large real-project datasets
- defined required metrics for indexing, incremental operation, query latency, DB size, and row counts
- documented stage timing expectations for full indexing runs so regressions can be localized
- recorded an initial OpenCV baseline after Milestone 6, showing that the main regression is concentrated in parse and resolve rather than persistence

---

### M7-E2. Benchmark Harness Implementation

Goal:

- make performance measurement repeatable

Status:

- Completed

Implementation tasks:

- add scripts or commands to run benchmark suites
- capture metrics and output them in a stable format
- store baseline results in a documented location
- document how to run benchmarks locally

Expected touch points:

- new benchmark tooling
- `README.md`
- `dev_docs/`

Validation checklist:

- benchmark runs can be repeated and compared across changes

Exit criteria:

- CodeAtlas has an operational benchmark harness

Current progress note:

- added `scripts/benchmark/Run-CodeAtlasBenchmark.ps1` as a repeatable PowerShell benchmark runner
- benchmark output now lands in `dev_docs/benchmark_results/` as machine-readable JSON
- checked-in baseline examples now exist for at least one deterministic fixture workspace
- the harness also records retry usage and raw output so transient `.codeatlas` publish lock failures remain diagnosable on Windows

Completion summary:

- created a repeatable benchmark runner for `full` and `incremental` indexer runs
- captured commit, environment notes, counts, stage timings, parse breakdowns, and resolve breakdowns in JSON
- documented harness usage in `README.md`
- established `dev_docs/benchmark_results/` as the stable baseline location

---

### M7-E3. Query Profiling and Hot-Path Optimization

Goal:

- optimize the queries agents use most often

Status:

- Completed

Implementation tasks:

- profile:
  - exact lookup
  - search
  - callers
  - references
  - path tracing
  - impact analysis
- add or refine DB indexes
- consider lightweight precomputed aggregates only when they do not compromise incremental correctness
- avoid speculative denormalization

Expected touch points:

- `server/src/storage/sqlite-store.ts`
- `indexer/src/storage.rs`

Validation checklist:

- p50 and p95 latency improve on important query paths

Exit criteria:

- common structural queries remain interactive on larger datasets

Current progress note:

- added `server/src/query-profiler.ts` to produce repeatable query-latency JSON output
- added batch symbol lookup support to reduce repeated `getSymbolById` calls in caller, reference, metadata-grouping, and impact paths
- added query-oriented SQLite indexes for exact lookup and common structural relation access patterns
- captured an OpenCV query-profile snapshot covering exact lookup, search, callers, references, call-path tracing, and impact analysis

Completion summary:

- query latency now has a repeatable measurement path instead of ad-hoc observation
- the main agent-facing structural queries have evidence-backed timing data on a large real-project dataset
- first-pass storage and response-shaping optimizations were applied without weakening correctness

---

### M7-E4. Incremental and Watcher Scale Benchmarks

Goal:

- measure the core differentiator of CodeAtlas under active development conditions

Status:

- Completed

Implementation tasks:

- benchmark:
  - no-change rerun
  - single `.cpp` edit
  - header edit
  - repeated file burst
  - branch-like mass change
- record:
  - time
  - affected rows or updated records
  - memory
  - rebuild recommendation behavior

Expected touch points:

- benchmark tooling
- `dev_docs/`

Validation checklist:

- incremental and watch metrics are captured and comparable across revisions

Exit criteria:

- CodeAtlas has evidence for its operational scale advantage

Current progress note:

- added `scripts/benchmark/Run-CodeAtlasIncrementalSuite.ps1` for repeatable fixture-based incremental benchmarking
- generated `dev_docs/benchmark_results/incremental-suite-samples.json`
- captured no-change rerun, single `.cpp` edit, header edit, repeated burst edits, fixture mass change, and synthetic branch-like churn
- synthetic branch-like churn now proves that rebuild-escalation behavior is exercised by the benchmark suite rather than only inferred from tests

Completion summary:

- incremental operating behavior is now benchmarked through a repeatable script instead of one-off manual notes
- burst-edit and branch-like churn behavior are both represented in machine-readable benchmark output
- the suite records plan shape, elapsed time, parse and resolve timing details, and escalation behavior per scenario

---

### M7-E5. Build Metadata Ingestion

Goal:

- improve resolution quality using available build metadata without becoming an LSP-dependent product

Status:

- Completed

Implementation tasks:

- add optional ingestion of:
  - `compile_commands.json`
  - include directories
  - macro definitions if cheaply accessible
- define exactly how build metadata affects indexing or resolution
- keep basic mode fully usable without compile DB presence

Expected touch points:

- config ingestion code
- `indexer/src/*`
- docs

Validation checklist:

- tricky fixtures improve when build metadata is available
- baseline operation still works without build metadata

Exit criteria:

- CodeAtlas can selectively use build metadata while staying true to its lightweight identity

Current progress note:

- added optional `compile_commands.json` auto-detection in the Rust indexer
- build metadata now ingests workspace include directories, compile output paths, and cheap define hints
- build metadata refines existing metadata classification instead of attempting compiler-grade semantics
- current first-release effects are:
  - promoting `headerRole` to `public` when headers live under compile-db-discovered workspace include directories
  - refining `artifactKind` when compile output paths or define hints indicate test/editor/tool/generated intent
- baseline operation remains unchanged when compile DB metadata is absent or unreadable

Completion summary:

- build metadata ingestion is now an optional overlay instead of a hard dependency
- the indexer stays fully usable without compile DB presence
- first-release metadata refinement is intentionally lightweight and avoids redefining CodeAtlas as an LSP-dependent product

---

### M7-E6. Include and Macro Risk Signals

Goal:

- surface C++ fragility zones honestly instead of hiding them

Status:

- Completed

Implementation tasks:

- collect lightweight include graph information where cheap
- mark macro-heavy or parse-fragile files
- add file or symbol metadata such as:
  - parse fragility
  - macro sensitivity
  - include heaviness
- feed these signals into confidence or agent guidance

Expected touch points:

- `indexer/src/parser.rs`
- `indexer/src/models.rs`
- `server/src/models/responses.ts`

Validation checklist:

- fragility signals appear consistently on targeted fixtures

Exit criteria:

- hard C++ corners are surfaced as risk signals instead of silent inaccuracies

Current progress note:

- added lightweight file-risk signal derivation in the parser
- current first-release signals are:
  - `parseFragility`
  - `macroSensitivity`
  - `includeHeaviness`
- these signals are persisted onto file records and copied onto symbols from the same file
- propagation-oriented guidance now includes file-risk notes when exact symbols live in parse-fragile, macro-sensitive, or include-heavy files

Completion summary:

- CodeAtlas now exposes structural fragility cues instead of silently presenting all indexed C++ as equally trustworthy
- the implementation stays intentionally lightweight and heuristic-driven
- the result improves agent guidance without turning CodeAtlas into a compiler or LSP-dependent product

---

## 4. Final Exit Criteria

- benchmark datasets and metrics are formally defined
- benchmark execution is automated and repeatable
- hot-path queries are profiled and improved with evidence
- incremental and watcher performance is measured under realistic scenarios
- optional build metadata improves accuracy without redefining the product

Current milestone reading:

- all milestone exit criteria are satisfied
- Milestone 7 is complete

