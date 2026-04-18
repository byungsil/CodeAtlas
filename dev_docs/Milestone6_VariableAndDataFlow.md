# Milestone 6. Variable and Data-Flow Propagation

Status:

- Completed

## 1. Objective

Extend CodeAtlas from structural code intelligence into bounded value-propagation analysis that remains practical for AI agents on large C++ repositories.

This milestone focuses on:

- variable assignment flow
- argument and return propagation
- member and object-state propagation
- bounded interprocedural flow tracing
- honest confidence signaling for uncertain flows

Success outcome:

- agents can ask how a value, symbol, or field state likely propagates without reading large raw file regions by default

Positioning note:

- this milestone should start after Milestone 4 has made incremental state trustworthy
- it also benefits from the structural context added in Milestone 5, especially hierarchy and call-path reasoning
- it intentionally comes before Performance Proof because propagation meaningfully raises agent usefulness, while benchmarking should measure that richer capability rather than precede it

---

## 2. Recommended Order

1. M6-E1. Propagation model and scope definition
2. M6-E2. Intra-procedural local flow extraction
3. M6-E3. Function-boundary flow summaries
4. M6-E4. Member and state propagation
5. M6-E5. Bounded interprocedural propagation queries
6. M6-E6. Confidence and risk signaling for propagation answers

---

## 3. Epics

### M6-E1. Propagation Model and Scope Definition

Goal:

- define a first-release propagation model that is useful, bounded, and explicit about what it does not prove

Status:

- Completed

Implementation tasks:

- define supported first-release propagation kinds:
  - assignment
  - initializer binding
  - argument-to-parameter flow
  - return-value flow
  - field write
  - field read
- define normalized propagation payload:
  - source symbol or expression anchor
  - target symbol or expression anchor
  - propagation kind
  - file path
  - line
  - confidence
- define explicit non-goals for the first release:
  - full alias analysis
  - compiler-grade template instantiation semantics
  - exact macro-expanded data flow
  - whole-program pointer analysis

Expected touch points:

- `dev_docs/API_CONTRACT.md`
- `indexer/src/models.rs`
- `server/src/models/responses.ts`

Validation checklist:

- supported propagation kinds are documented and intentionally limited
- unsupported C++ cases are written down instead of being left implicit

Exit criteria:

- there is a stable first-release propagation contract

Completion summary:

- added a normalized propagation contract to the indexer model with explicit propagation kinds, anchor kinds, and risk markers
- added planned server response vocabulary for propagation events, bounded propagation paths, and symbol-propagation summaries
- documented first-release supported flow kinds, explicit non-goals, planned query directions, and propagation confidence semantics in `dev_docs/API_CONTRACT.md`
- fixed the first-release boundary so later epics can extend extraction and query behavior without renaming the contract

---

### M6-E2. Intra-Procedural Local Flow Extraction

Goal:

- extract the most reliable value movements within a single function or method

Status:

- Completed

Implementation tasks:

- extract local flows for patterns such as:
  - `a = b`
  - `T x = y`
  - `auto x = y`
  - chained assignments where structurally safe
- distinguish declaration, assignment, and later use sites where possible
- keep source anchors inspectable so agents can see why a local flow edge exists
- add fixtures for:
  - local variable propagation
  - shadowing
  - nested blocks
  - pointer/reference syntax that should remain unsupported or low-confidence

Expected touch points:

- `indexer/graph/`
- `indexer/src/parser.rs`
- `samples/`

Validation checklist:

- supported local flow patterns produce deterministic propagation events
- shadowed variables do not silently collapse into one flow chain

Exit criteria:

- CodeAtlas can answer bounded local-flow questions inside one callable body

Completion summary:

- added local propagation extraction for reliable intra-procedural patterns:
  - `a = b`
  - `T x = y`
  - `auto x = y`
  - chained assignments where the nested assignment can be structurally followed
- introduced scoped local and parameter anchor identities so later propagation traversal can distinguish shadowed locals
- pointer-heavy local flows now degrade to `partial` confidence with explicit risk markers instead of being presented as high-certainty propagation
- added dedicated propagation fixtures for local flow and shadowing behavior

---

### M6-E3. Function-Boundary Flow Summaries

Goal:

- connect local propagation with callable interfaces so values can be followed across calls in a bounded way

Status:

- Completed

Implementation tasks:

- model argument-to-parameter propagation
- model return-value propagation into caller-side assignments or initializers
- attach function-boundary summaries to callable symbols
- reuse existing call resolution confidence instead of pretending propagation is stronger than call identity
- keep unsupported call forms explicit:
  - unresolved overloads
  - function pointers
  - macro-generated call indirection unless clearly recoverable

Expected touch points:

- `indexer/src/models.rs`
- `indexer/src/resolver.rs`
- `indexer/src/storage.rs`

Validation checklist:

- fixtures cover simple caller-to-callee parameter flow
- return-value flow can be queried across at least one call boundary

Exit criteria:

- argument and return propagation can be traversed across supported call edges

Completion summary:

- added callable flow summaries that preserve ordered parameter anchors and return anchors for supported function and method definitions
- enriched raw call extraction with argument texts and caller-side result targets so resolved call edges can carry bounded propagation across one call boundary
- added boundary propagation derivation for:
  - argument-to-parameter flow
  - return-value flow into caller-side assignments or initializers
- unsupported call forms still stay out of the propagation graph unless the resolved call edge and required structural anchors are both present

---

### M6-E4. Member and State Propagation

Goal:

- capture common object-state transitions that agents frequently ask about in class-heavy C++ code

Status:

- Completed

Implementation tasks:

- extract common state propagation patterns:
  - parameter assigned into member
  - local assigned into member
  - member read into local
  - member read into return
  - simple setter/getter-like patterns where structurally obvious
- keep receiver form explicit:
  - `this->member`
  - `obj.member`
  - `ptr->member`
- separate object-local state flow from plain local variable flow
- mark cases requiring real alias reasoning as low-confidence or unsupported

Expected touch points:

- `indexer/src/parser.rs`
- `indexer/src/models.rs`
- `indexer/src/storage.rs`

Validation checklist:

- fixtures cover constructor state setup, setter/getter patterns, and simple object handoff cases
- unsupported pointer-heavy cases do not appear as false-certain flows

Exit criteria:

- common member-state propagation is represented for supported C++ shapes

Completion summary:

- added member/state propagation for common structural patterns:
  - parameter assigned into member
  - local assigned into member
  - member read into local
  - member read into return
- `this->member` is treated as the strongest supported member-state shape and produces stable field anchors
- `obj.member` and `ptr->member` are still emitted when structurally visible, but degrade to `partial` confidence with explicit receiver-ambiguity and pointer-heavy risk markers where appropriate
- dedicated member-state fixtures now lock in field-write and field-read behavior for constructor-like setup, getter/setter-like access, and weaker receiver forms

---

### M6-E5. Bounded Interprocedural Propagation Queries

Goal:

- expose propagation answers directly to agents in compact, bounded forms

Status:

- Completed

Implementation tasks:

- add agent-facing queries such as:
  - `trace_variable_flow`
  - `explain_symbol_propagation`
- support traversal bounds:
  - max depth
  - max edges
  - propagation kinds
  - optional file, class, or subsystem filters
- return compact path summaries:
  - likely path steps
  - propagation kind per hop
  - confidence per hop
  - truncation metadata
- avoid returning raw oversized propagation graphs by default

Expected touch points:

- `server/src/mcp.ts`
- `server/src/storage/sqlite-store.ts`
- `server/src/models/responses.ts`

Validation checklist:

- bounded propagation queries return deterministic compact summaries
- search limits and truncation are visible in the response

Exit criteria:

- agents can ask how a value likely moves across supported program boundaries

Completion summary:

- persisted propagation events in SQLite and JSON-backed storage so propagation survives indexing and can be queried by the server surface
- added exact propagation query surfaces:
  - `GET /symbol-propagation`
  - `GET /trace-variable-flow`
  - MCP `explain_symbol_propagation`
  - MCP `trace_variable_flow`
- bounded the query behavior with explicit `limit`, `maxDepth`, `maxEdges`, optional propagation-kind filters, and shared truncation metadata
- exposed compact scope summaries for incoming and outgoing propagation plus one deterministic bounded path for trace-oriented agent workflows

---

### M6-E6. Confidence and Risk Signaling for Propagation Answers

Goal:

- make propagation results trustworthy by exposing uncertainty honestly

Status:

- Completed

Implementation tasks:

- define propagation confidence levels
- add risk markers for:
  - alias-heavy code
  - unresolved overloads
  - pointer-heavy flows
  - macro-sensitive regions
  - receiver ambiguity
- guide agents toward follow-up queries when propagation confidence is limited
- document which propagation answers are structural approximations versus stronger evidence-backed paths

Expected touch points:

- `server/src/models/responses.ts`
- `dev_docs/API_CONTRACT.md`
- `README.md` or additional docs if operational guidance is needed

Validation checklist:

- low-confidence flows are labeled consistently
- ambiguous or unsupported cases degrade into honest risk signals, not false certainty

Exit criteria:

- propagation answers expose why they are exact, likely, or weak

Completion summary:

- added response-level propagation confidence so exact symbol targeting and propagation strength are no longer conflated in the same field
- propagation answers now aggregate hop-level risks into stable `riskMarkers` and `confidenceNotes`
- low-confidence situations such as truncation, partial hops, pointer-heavy flow, receiver ambiguity, and unsupported shapes now explain themselves explicitly
- added follow-up query guidance so agents can refine weak propagation answers instead of over-trusting them

---

## 4. Final Exit Criteria

- a first-release propagation model is documented and implemented
- supported local assignment and initializer flows are queryable
- supported argument and return propagation can be followed across bounded call edges
- common member-state propagation patterns are represented
- agent-facing propagation queries return compact structured answers with explicit confidence and truncation behavior
- unsupported or fragile C++ cases are surfaced as risks instead of being silently overstated

Completion validation:

- `indexer` full test suite passed:
  - `124 passed, 0 failed`
- `server` full test suite passed:
  - `99 passed, 0 failed`
- real-workspace validation on `E:\Dev\opencv` confirmed:
  - regenerated OpenCV index with propagation persistence enabled
  - live propagation queries returned bounded structured results from the real workspace
  - `GET /symbol-propagation` and `GET /trace-variable-flow` both responded successfully for a real OpenCV symbol

Milestone completion summary:

- first-release bounded propagation is now part of the shipped indexing and query surface
- local flow, function-boundary flow, and common member/state flow are all persisted and queryable
- agent-facing propagation answers now stay compact, bounded, and explicit about risk instead of implying compiler-grade certainty
- real-project validation confirms the propagation path works outside fixtures on a large C++ repository

## 5. Handoff to Milestone 7

- Milestone 7 benchmark scenarios should include propagation-specific query latency and index-size impact
- Milestone 7 build-metadata ingestion should be evaluated partly by how much it improves propagation confidence on difficult C++ fixtures
- Milestone 7 macro and include risk signals should feed directly into propagation risk markers rather than being isolated performance-only work
