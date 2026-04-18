# Milestone 6. Variable and Data-Flow Propagation

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

---

### M6-E2. Intra-Procedural Local Flow Extraction

Goal:

- extract the most reliable value movements within a single function or method

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

---

### M6-E3. Function-Boundary Flow Summaries

Goal:

- connect local propagation with callable interfaces so values can be followed across calls in a bounded way

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

---

### M6-E4. Member and State Propagation

Goal:

- capture common object-state transitions that agents frequently ask about in class-heavy C++ code

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

---

### M6-E5. Bounded Interprocedural Propagation Queries

Goal:

- expose propagation answers directly to agents in compact, bounded forms

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

---

### M6-E6. Confidence and Risk Signaling for Propagation Answers

Goal:

- make propagation results trustworthy by exposing uncertainty honestly

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

---

## 4. Final Exit Criteria

- a first-release propagation model is documented and implemented
- supported local assignment and initializer flows are queryable
- supported argument and return propagation can be followed across bounded call edges
- common member-state propagation patterns are represented
- agent-facing propagation queries return compact structured answers with explicit confidence and truncation behavior
- unsupported or fragile C++ cases are surfaced as risks instead of being silently overstated

## 5. Handoff to Milestone 7

- Milestone 7 benchmark scenarios should include propagation-specific query latency and index-size impact
- Milestone 7 build-metadata ingestion should be evaluated partly by how much it improves propagation confidence on difficult C++ fixtures
- Milestone 7 macro and include risk signals should feed directly into propagation risk markers rather than being isolated performance-only work
