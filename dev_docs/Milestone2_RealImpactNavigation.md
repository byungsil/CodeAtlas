# Milestone 2. Real Impact Navigation

## 1. Objective

Turn CodeAtlas from a lookup tool into a practical impact-analysis tool for large C++ projects.

This milestone focuses on:

- direct caller queries
- generalized reference queries
- impact analysis summaries
- structure overview queries
- token-efficient response shaping

Success outcome:

- agents can answer caller, reference, and impact questions without raw file reading

---

## 2. Recommended Order

1. M2-E1. Direct caller queries
2. M2-E2. Reference model definition
3. M2-E3. Reference extraction
4. M2-E4. Reference storage and retrieval
5. M2-E5. Impact-analysis summarization
6. M2-E6. Symbol overview queries
7. M2-E7. Token-efficient response shaping

---

## 3. Epics

### M2-E1. Direct Caller Queries

Goal:

- answer the question "who calls this?" directly

Implementation tasks:

- add `find_callers` MCP tool
- add HTTP endpoint if HTTP remains part of the workflow
- define truncation and ordering behavior for high fan-in symbols
- consider grouping callers by file, class, or subsystem if inexpensive

Expected touch points:

- `server/src/mcp.ts`
- `server/src/app.ts`
- `server/src/storage/*`

Validation checklist:

- high fan-in fixtures return deterministic order
- duplicate callers are deduplicated

Exit criteria:

- inbound call relationships can be queried directly

---

### M2-E2. Reference Model Definition

Goal:

- define what counts as a reference in the first release

Implementation tasks:

- define supported reference categories:
  - function call
  - method call
  - class instantiation
  - type usage
  - inheritance mention
- define normalized reference payload:
  - source symbol
  - target symbol
  - category
  - file path
  - line
  - confidence
- decide whether references live in their own table or partially reuse call storage

Expected touch points:

- `dev_docs/API_CONTRACT.md`
- `indexer/src/models.rs`
- `server/src/models/*`

Validation checklist:

- reference categories are explicitly documented and intentionally limited

Exit criteria:

- there is a stable first-release reference model

---

### M2-E3. Reference Extraction

Goal:

- collect structurally useful references beyond direct call edges

Implementation tasks:

- extend parser/indexer to emit supported reference events
- start with the most reliable categories first
- mark unsupported or uncertain categories clearly

Expected touch points:

- `indexer/src/parser.rs`
- `indexer/src/indexing.rs`
- `indexer/src/models.rs`

Validation checklist:

- each supported reference category has fixture coverage

Exit criteria:

- indexer can emit reference events for the first supported categories

---

### M2-E4. Reference Storage and Retrieval

Goal:

- store and query references efficiently

Implementation tasks:

- add schema migration for references if needed
- add retrieval paths by:
  - target symbol
  - reference category
  - optional file or metadata filters
- add MCP tool `find_references`

Expected touch points:

- `indexer/src/storage.rs`
- `server/src/storage/sqlite-store.ts`
- `server/src/mcp.ts`

Validation checklist:

- `find_references` returns correct category labels and confidence fields

Exit criteria:

- references are queryable in the same way calls are queryable

---

### M2-E5. Impact-Analysis Summarization

Goal:

- answer "what breaks if I change this?" in a compact agent-friendly way

Implementation tasks:

- define `impact_analysis` request shape
- traverse callers, callees, and references with bounded search
- build summary-first output:
  - top affected symbols
  - top affected files
  - top affected subsystems if available
  - confidence
  - truncation
  - suggested follow-up queries
- avoid raw graph dumping by default

Expected touch points:

- `server/src/mcp.ts`
- `server/src/models/responses.ts`
- `server/src/storage/sqlite-store.ts`

Validation checklist:

- realistic scenarios produce concise and useful summaries

Exit criteria:

- agents can ask change-risk questions and receive structured summaries

---

### M2-E6. Symbol Overview Queries

Goal:

- allow progressive structure browsing without opening source files

Implementation tasks:

- add:
  - `list_file_symbols(path)`
  - `list_namespace_symbols(qualifiedName)`
  - `list_class_members(qualifiedName)`
- define stable ordering by line number or declaration order
- return compact summaries first

Expected touch points:

- `server/src/mcp.ts`
- `server/src/app.ts`
- `server/src/storage/sqlite-store.ts`

Validation checklist:

- agents can discover nearby structure using only CodeAtlas queries

Exit criteria:

- structure browsing is possible without raw file inspection

---

### M2-E7. Token-Efficient Response Shaping

Goal:

- keep new query responses useful without inflating token cost

Implementation tasks:

- add summary fields before detail arrays
- standardize truncation metadata
- add optional flags or limits if needed
- remove redundant fields from repeated result items where possible

Expected touch points:

- `server/src/models/responses.ts`
- `server/src/mcp.ts`
- `dev_docs/API_CONTRACT.md`

Validation checklist:

- responses remain compact on large fan-in or high-reference symbols

Exit criteria:

- milestone queries are practical for AI agent usage at scale

---

## 4. Final Exit Criteria

- caller queries exist and are stable
- references are modeled, stored, and queryable
- impact analysis produces useful summary-first responses
- symbol overview queries reduce the need for raw source access
