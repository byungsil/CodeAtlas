# Milestone 5. Performance Proof

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

---

## 2. Recommended Order

1. M5-E1. Benchmark design
2. M5-E2. Benchmark harness implementation
3. M5-E3. Query profiling and hot-path optimization
4. M5-E4. Incremental and watcher scale benchmarks
5. M5-E5. Build metadata ingestion
6. M5-E6. Include and macro risk signals

---

## 3. Epics

### M5-E1. Benchmark Design

Goal:

- define what performance evidence CodeAtlas should collect and publish

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

---

### M5-E2. Benchmark Harness Implementation

Goal:

- make performance measurement repeatable

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

---

### M5-E3. Query Profiling and Hot-Path Optimization

Goal:

- optimize the queries agents use most often

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

---

### M5-E4. Incremental and Watcher Scale Benchmarks

Goal:

- measure the core differentiator of CodeAtlas under active development conditions

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

---

### M5-E5. Build Metadata Ingestion

Goal:

- improve resolution quality using available build metadata without becoming an LSP-dependent product

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

---

### M5-E6. Include and Macro Risk Signals

Goal:

- surface C++ fragility zones honestly instead of hiding them

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

---

## 4. Final Exit Criteria

- benchmark datasets and metrics are formally defined
- benchmark execution is automated and repeatable
- hot-path queries are profiled and improved with evidence
- incremental and watcher performance is measured under realistic scenarios
- optional build metadata improves accuracy without redefining the product
