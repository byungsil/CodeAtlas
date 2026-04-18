# Milestone 5. Large-Project Intelligence

Status:

- Completed

## 1. Objective

Add higher-level structure so agents can reason in subsystem, hierarchy, and path terms instead of only symbol terms.

This milestone focuses on:

- inheritance and override understanding
- call-path tracing
- project metadata modeling
- metadata-aware grouping and filtering

Success outcome:

- agents can reason in terms of hierarchy, path flow, and subsystem-level impact

---

## 2. Recommended Order

1. M5-E1. Inheritance relation model
2. M5-E2. Override candidate logic
3. M5-E3. Type hierarchy queries
4. M5-E4. Call-path tracing
5. M5-E5. Project metadata model
6. M5-E6. Metadata-aware filtering and grouping

---

## 3. Epics

### M5-E1. Inheritance Relation Model

Goal:

- model base and derived type relationships explicitly

Status:

- Completed

Implementation tasks:

- parse inheritance edges from class declarations
- store base/derived relations
- expose internal query methods for hierarchy traversal

Expected touch points:

- `indexer/src/parser.rs`
- `indexer/src/models.rs`
- `indexer/src/storage.rs`

Validation checklist:

- fixtures cover interfaces, abstract bases, and multiple derived classes

Exit criteria:

- type inheritance can be queried structurally

Completion summary:

- existing graph-derived `inheritance` events are now promoted as first-class stored hierarchy edges through `inheritanceMention` references
- `indexer` storage exposes internal direct-base and direct-derived query methods for hierarchy traversal
- regression coverage now includes interface, abstract-base, and multi-derived inheritance shapes

---

### M5-E2. Override Candidate Logic

Goal:

- detect likely method override relationships using structural evidence

Status:

- Completed

Implementation tasks:

- build override heuristics using:
  - matching method names
  - base/derived class relationship
  - compatible signatures where available
- attach confidence markers when exact proof is not available
- avoid overstating uncertain override relationships

Expected touch points:

- `indexer/src/resolver.rs`
- `indexer/src/models.rs`
- `server/src/models/responses.ts`

Validation checklist:

- fixtures cover base virtual method plus multiple derived overrides

Exit criteria:

- likely overrides can be surfaced with honest confidence levels

Completion summary:

- structural override candidate logic now combines base/derived hierarchy edges with matching method names
- confidence remains `high` only when arity evidence also matches, otherwise candidates stay `partial`
- response contract now has explicit override match reasons so later hierarchy APIs can expose why a candidate was suggested

---

### M5-E3. Type Hierarchy Queries

Goal:

- expose hierarchy information directly to agents

Status:

- Completed

Implementation tasks:

- add:
  - `get_type_hierarchy`
  - `find_base_methods`
  - `find_overrides`
- define compact and bounded response shapes
- decide how uncertainty is represented in hierarchy responses

Expected touch points:

- `server/src/mcp.ts`
- `server/src/storage/sqlite-store.ts`
- `server/src/models/responses.ts`

Validation checklist:

- agents can navigate a class family using only structured responses

Exit criteria:

- hierarchy and override queries are available and practical

Completion summary:

- added exact hierarchy query surfaces for direct base and direct derived types
- added exact method-level queries for likely base methods and likely overrides
- responses stay bounded and summary-first so agents can inspect class families without falling back to raw source scans

---

### M5-E4. Call-Path Tracing

Goal:

- help agents answer "how do we get from A to B?"

Status:

- Completed

Implementation tasks:

- implement bounded call-path search
- define path search limits and truncation semantics
- return compact path summaries rather than oversized graph payloads

Expected touch points:

- `server/src/mcp.ts`
- `server/src/storage/sqlite-store.ts`

Validation checklist:

- fixtures with multiple possible paths return deterministic bounded results

Exit criteria:

- call-path tracing exists and remains operationally bounded

Completion summary:

- added exact source-to-target call-path tracing over the resolved call graph
- search uses bounded breadth-first traversal so it returns a deterministic shortest path when one is found
- responses stay compact with explicit `pathFound`, `steps`, `maxDepth`, and `truncated` markers instead of dumping oversized graph payloads

---

### M5-E5. Project Metadata Model

Goal:

- model repository-specific organization that matters in large game projects

Status:

- Completed

Implementation tasks:

- define tags or derived metadata for:
  - subsystem
  - module
  - runtime/editor/tool/test/generated
  - public/private/internal header role
- decide whether metadata comes from path conventions, config, or explicit rules
- keep first release simple and deterministic

Expected touch points:

- `indexer/src/models.rs`
- `indexer/src/storage.rs`
- config and docs files

Validation checklist:

- realistic directory fixtures produce stable metadata assignment

Exit criteria:

- symbols and files can be grouped by meaningful project boundaries

Completion summary:

- introduced a deterministic path-derived metadata model with `subsystem`, `module`, `projectArea`, `artifactKind`, and `headerRole`
- metadata is attached during indexing and persisted on both symbol and file records
- first release uses only repository path conventions, keeping the model simple, explainable, and stable before config-driven overrides

---

### M5-E6. Metadata-Aware Filtering and Grouping

Goal:

- improve query usefulness by grouping and filtering on project structure

Status:

- Completed

Implementation tasks:

- add optional filters for search, callers, references, and impact queries
- add grouped summaries such as:
  - callers by subsystem
  - references by module
  - impact counts by area
- ensure grouped responses remain compact enough for AI use

Expected touch points:

- `server/src/mcp.ts`
- `server/src/models/responses.ts`
- `server/src/storage/sqlite-store.ts`

Validation checklist:

- grouped results improve readability without bloating responses
- metadata-filtered search, caller, reference, and impact queries remain deterministic
- MCP and HTTP responses echo active metadata filters and compact grouped summaries

Exit criteria:

- agents can reason at subsystem and module level, not only symbol level

Completion summary:

- added optional `subsystem`, `module`, `projectArea`, and `artifactKind` filters to `search`, `find_callers`, `find_references`, and `impact_analysis`
- added grouped metadata summaries for callers, references, and impact payloads so agents can pivot by subsystem and module without extra raw-source reads
- kept SQLite and JSON stores backward compatible by treating metadata filters as optional and returning empty filtered results when old SQLite snapshots do not expose metadata columns

---

## 4. Final Exit Criteria

- inheritance and override relationships are queryable
- call-path tracing is available and bounded
- repository metadata can be derived and queried
- higher-level grouping improves impact and navigation workflows

Completion summary:

- all six planned epics are complete, including hierarchy queries, bounded call-path tracing, path-derived project metadata, and metadata-aware filtering/grouping
- full automated validation passed on the current workspace:
  - `indexer`: `116 passed, 0 failed`
  - `server`: `99 passed, 0 failed`
- real-workspace validation against `E:\Dev\opencv` confirmed:
  - direct type hierarchy and override queries on `calib::FrameProcessor`
  - bounded call-path tracing on OpenCV functions
  - metadata-filtered search and impact responses on module-scoped symbols such as `cv::imread` and `cv::makeAgastOffsets`

Exit-criteria assessment:

- satisfied: inheritance and override relationships are queryable
- satisfied: call-path tracing is available and bounded
- satisfied: repository metadata can be derived and queried
- satisfied: higher-level grouping improves impact and navigation workflows
