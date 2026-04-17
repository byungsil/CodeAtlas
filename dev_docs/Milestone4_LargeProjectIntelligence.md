# Milestone 4. Large-Project Intelligence

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

1. M4-E1. Inheritance relation model
2. M4-E2. Override candidate logic
3. M4-E3. Type hierarchy queries
4. M4-E4. Call-path tracing
5. M4-E5. Project metadata model
6. M4-E6. Metadata-aware filtering and grouping

---

## 3. Epics

### M4-E1. Inheritance Relation Model

Goal:

- model base and derived type relationships explicitly

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

---

### M4-E2. Override Candidate Logic

Goal:

- detect likely method override relationships using structural evidence

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

---

### M4-E3. Type Hierarchy Queries

Goal:

- expose hierarchy information directly to agents

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

---

### M4-E4. Call-Path Tracing

Goal:

- help agents answer "how do we get from A to B?"

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

---

### M4-E5. Project Metadata Model

Goal:

- model repository-specific organization that matters in large game projects

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

---

### M4-E6. Metadata-Aware Filtering and Grouping

Goal:

- improve query usefulness by grouping and filtering on project structure

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

Exit criteria:

- agents can reason at subsystem and module level, not only symbol level

---

## 4. Final Exit Criteria

- inheritance and override relationships are queryable
- call-path tracing is available and bounded
- repository metadata can be derived and queried
- higher-level grouping improves impact and navigation workflows
