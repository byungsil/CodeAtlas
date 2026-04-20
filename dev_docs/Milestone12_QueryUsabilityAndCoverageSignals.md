# Milestone 12. Query Usability and Coverage Signals

Status:

- Completed

## 1. Objective

Reduce avoidable follow-up queries during real investigations by making ambiguity, upstream traversal, enum-value usage, and coverage confidence easier to consume from the first response.

This milestone focuses on:

- surfacing ranked alternatives on ambiguous heuristic lookups
- adding bounded upstream caller traversal
- making enum member values queryable as first-class usage targets
- shrinking bulky file and graph responses for agent-friendly delivery
- promoting existing fragility signals into response-level trust signals

Success outcome:

- an agent can disambiguate a common symbol name without immediately re-querying
- upstream entry-point tracing can be done in one bounded call instead of repeated manual caller hops
- enum flags and similar value-style members are directly queryable
- large structural responses are easier to consume without unnecessary verbosity
- weak or incomplete coverage is signaled explicitly instead of being buried in per-symbol metadata

Positioning note:

- Milestone 10 already improved context-aware ranking and investigation workflow stitching
- Milestone 11 hardened reference correctness and DB integrity
- MS12 should build on that stable base by improving user-facing query ergonomics rather than revisiting index correctness fundamentals

Scope note:

- this document captures the narrowed MS12 scope for query usability and coverage signaling improvements
- it intentionally excludes ideas that are too speculative, too transport-specific, or too invasive for the next milestone

---

## 2. Applicability Review

The original TODO list contains six themes. For the current project, these are the ones worth carrying forward now.

Included in MS12:

1. ambiguous lookup response improvements
2. upstream call-chain traversal
3. enum member value usage indexing
4. compact response modes for bulky structural tools
5. response-level reliability and coverage signals

Deferred from the original TODO:

- fallback text-call extraction and separate low-confidence fallback call tables
  - reason: high implementation cost, higher false-positive risk, and should follow stronger evidence that current graph extraction is insufficient after response-level coverage signaling lands
- `maxInlineBytes` transport behavior
  - reason: inline-vs-file behavior is often controlled by the MCP client/runtime rather than CodeAtlas itself
- broad investigate-workflow expansion beyond upstream traversal
  - reason: MS10 already established the workflow surface; MS12 should add focused substrate improvements rather than growing a second workflow milestone

---

## 3. Recommended Order

1. M12-E1. Ambiguous Lookup Response Quality
2. M12-E2. Upstream Callgraph Traversal
3. M12-E3. Enum Member Value Usage Indexing
4. M12-E4. Compact Structural Response Modes
5. M12-E5. Reliability and Coverage Signaling

Why this order:

- ambiguity improvements pay off immediately across many existing tools
- upstream traversal is a new capability with high investigation value and low conceptual overlap with enum work
- enum member value usage requires indexer and API changes and is best tackled after the navigation surfaces are clear
- compact-response work should happen after the new query shapes exist
- reliability and coverage signaling should land after the response shapes they annotate are settled

---

## 4. Epics

### M12-E1. Ambiguous Lookup Response Quality

Status:

- Completed

Goal:

- make ambiguous heuristic responses actionable on first return instead of requiring blind retry loops

Why this is needed now:

- MS10 already improved ranking quality, but the current response still largely exposes only the selected symbol plus `candidateCount`
- users and agents still need another query to inspect near-miss candidates when names like `Update`, `Init`, or `AddShotFlag` collide heavily

Implementation tasks:

- add `topCandidates` to ambiguous heuristic responses for:
  - `lookup_function`
  - `lookup_class`
  - `find_callers` when resolved through heuristic short-name lookup
- cap `topCandidates` at 5 entries and keep each entry compact:
  - `id`
  - `qualifiedName`
  - `filePath`
  - `line`
  - `signature`
  - `rankScore`
- add `selectedReason` to heuristic responses so the chosen candidate is explainable
- add a `find_all_overloads` MCP/HTTP surface for exact grouped inspection of all short-name matches without ranking collapse
- strengthen tests so `filePath`, `anchorQualifiedName`, and `recentQualifiedName` context remain visibly reflected in the chosen result and the candidate list

Expected touch points:

- `server/src/app.ts`
- `server/src/mcp-runtime.ts`
- `server/src/response-metadata.ts`
- `server/src/models/responses.ts`
- server ambiguity and investigation tests

Validation checklist:

- ambiguous responses include `topCandidates`
- heuristic responses include `selectedReason`
- `find_all_overloads` returns grouped exact candidates without hiding duplicates
- ranking context still affects the first result and is now inspectable in the alternatives list

Exit criteria:

- ambiguous symbol selection no longer feels like a blind one-shot guess

Completion note:

- `lookup_function`, `lookup_class`, and heuristic `find_callers` now surface `selectedReason`
- ambiguous heuristic responses now include capped `topCandidates`
- `find_all_overloads` is available in HTTP and MCP for grouped exact callable-name inspection
- server ambiguity, MCP, and investigation tests were updated and pass

---

### M12-E2. Upstream Callgraph Traversal

Status:

- Completed

Goal:

- allow bounded caller-chain expansion in one query instead of repeated `find_callers` chaining

Why this is needed now:

- current `get_callgraph` expands only callees
- investigation workflows frequently need to answer "who eventually calls this?" or "where does this become reachable from?"

Implementation tasks:

- extend `get_callgraph` with a `direction` parameter:
  - `callees`
  - `callers`
  - `both`
- add a `find_callers_recursive` convenience surface that wraps caller-direction traversal
- keep traversal cycle-safe and bounded:
  - maximum depth
  - node cap
  - explicit `truncated` signaling
- update `investigate_workflow` follow-up generation to prefer upstream traversal when the task is entry-point seeking

Expected touch points:

- `server/src/app.ts`
- `server/src/mcp-runtime.ts`
- callgraph response models and tests
- `server/src/investigation-workflow.ts`

Validation checklist:

- `get_callgraph(direction="callers")` returns a bounded upstream tree
- cycles terminate cleanly
- `find_callers_recursive` is available in MCP and HTTP
- investigation workflow suggestions can reuse upstream traversal where appropriate

Exit criteria:

- multi-hop caller tracing can start from one bounded query

Completion note:

- `get_callgraph` now supports `direction = callees | callers | both`
- traversal is bounded by both `depth` and `nodeCap`, and returns explicit `truncated` signaling
- `find_callers_recursive` is available in MCP and HTTP as a caller-direction convenience surface
- `investigate_workflow` follow-up suggestions now include recursive caller tracing for helper callables

---

### M12-E3. Enum Member Value Usage Indexing

Status:

- Completed

Goal:

- make enum member values queryable as first-class symbols and usage targets

Why this is needed now:

- current generalized references are strong for types and callables, but value-style enum flags remain a frequent blind spot in game and systems code
- this is a concrete gap for flag investigations, state masks, and option propagation

Implementation tasks:

- index enum members as individual symbols with a dedicated symbol type such as `enumMember`
- add normalized reference extraction for enum-member value usages:
  - direct assignment
  - comparison
  - bitwise composition
  - function-argument passing
- add a dedicated reference category such as `enumValueUsage`
- allow `find_references` to query enum members directly
- add an opt-in expansion on enum type queries:
  - `includeEnumValueUsage`
  - when enabled, aggregate value-usage references for the members of that enum
- add regression fixtures for:
  - plain enum constant assignment
  - namespaced enum-member usage
  - bitmask-style `|` and `|=` cases
  - function-argument use

Expected touch points:

- `indexer/src/parser.rs`
- language-specific parsers as needed
- `indexer/src/models.rs`
- `indexer/src/storage.rs`
- `server/src/storage/sqlite-store.ts`
- `server/src/app.ts`
- `server/src/mcp-runtime.ts`
- `dev_docs/API_CONTRACT.md`

Validation checklist:

- enum members appear as exact symbols
- `find_references` works on enum member qualified names
- `includeEnumValueUsage=true` expands enum-type reference inspection meaningfully
- indexing remains bounded on large enum-heavy headers

Exit criteria:

- flag-style investigations no longer require falling back to raw text search in the common case

Completion note:

- C++ enum declarations now emit `enumMember` symbols as exact lookup targets
- enum-member value uses are persisted under `enumValueUsage`
- `find_references` can query enum members directly and supports `includeEnumValueUsage` for enum-type aggregation
- server and indexer tests cover enum member symbol emission, enum-value reference persistence, and enum-type reference aggregation

---

### M12-E4. Compact Structural Response Modes

Status:

- Completed

Goal:

- make large structural responses easier for agents to consume without removing the richer default forms

Why this is needed now:

- `list_file_symbols` and larger graph-style results can become bulky for agent workflows
- CodeAtlas cannot reliably control whether a client writes large payloads out-of-band, but it can provide smaller response shapes directly

Implementation tasks:

- add `compact` mode to:
  - `list_file_symbols`
  - `get_callgraph`
  - `find_references`
- define compact payloads to keep only the fields needed for navigation:
  - file symbol lists:
    - `id`
    - `name`
    - `qualifiedName`
    - `type`
    - `line`
    - `endLine`
  - callgraph nodes:
    - `id`
    - `name`
    - `qualifiedName`
    - `filePath`
    - `line`
  - references:
    - source and target IDs
    - qualified names
    - category
    - file path
    - line
- keep default responses backward compatible
- update tool descriptions so agent runtimes know when compact mode is recommended

Expected touch points:

- `server/src/app.ts`
- `server/src/mcp-runtime.ts`
- response model types
- MCP and HTTP tests

Validation checklist:

- compact mode materially reduces response size
- default behavior remains unchanged when `compact` is omitted
- structural navigation still remains useful in compact form

Exit criteria:

- bulky structural responses can be requested in an agent-friendly compact shape

Completion note:

- `list_file_symbols`, `get_callgraph`, and `find_references` now accept `compact`
- default responses remain backward compatible when `compact` is omitted
- compact mode keeps the existing top-level wrapper but reduces bulky per-item payloads to navigation-oriented fields
- HTTP and MCP tests cover compact file-symbol, reference, and callgraph responses

---

### M12-E5. Reliability and Coverage Signaling

Status:

- Completed

Goal:

- turn existing fragility metadata into explicit response-level trust guidance

Why this is needed now:

- `parseFragility`, `macroSensitivity`, and similar fields already exist but are easy to miss
- a weak zero-result is much more dangerous than an honestly signaled partial result
- this is the most realistic next step from the original coverage-gap TODO without immediately introducing speculative fallback indexing passes

Implementation tasks:

- add top-level `reliability` to:
  - `lookup_function`
  - `find_callers`
  - `find_references`
  - `get_callgraph`
- define:
  - `level`: `full` | `partial` | `low`
  - `factors`: compact array such as `elevated_parse_fragility`, `macro_sensitive`
  - optional `suggestion`
- add `indexCoverage` and `coverageWarning` where appropriate:
  - especially for zero-result caller/reference responses on fragile symbols
- keep the first implementation heuristic and bounded:
  - derive from existing persisted symbol/file metadata
  - do not add fallback call extraction in this milestone
- update tests so fragile fixture symbols produce partial coverage signaling while stable symbols remain `full`

Expected touch points:

- `server/src/app.ts`
- `server/src/mcp-runtime.ts`
- `server/src/models/responses.ts`
- response-building helpers
- MCP and HTTP contract tests

Validation checklist:

- reliability is visible at the top level of the main navigation responses
- fragile zero-result responses carry an explicit warning instead of silent emptiness
- low-risk symbols still produce clean full-confidence responses without noisy warnings

Exit criteria:

- response consumers can tell when the index may be incomplete without digging through raw symbol metadata

Completion note:

- `lookup_function`, `find_callers`, `find_references`, and `get_callgraph` now surface top-level `reliability`
- `indexCoverage` and `coverageWarning` are emitted for bounded zero-result navigation responses when the root symbol sits in a fragile region
- the first implementation stays heuristic and metadata-driven; it does not introduce fallback extraction passes
- HTTP and MCP tests cover both ordinary responses and fragile zero-result warning behavior

Validation note:

- indexer test suite: `160/160` passed
- server test suite: `137/137` passed
- real-workspace full rebuild validation passed on:
  - `E:\Dev\opencv`
  - `E:\Dev\llvm-project-llvmorg-18.1.8`
- real-workspace DB checks:
  - `PRAGMA integrity_check = ok`
  - `dangling_calls = 0`
  - `dangling_references = 0`
  - `dangling_propagation = 0`
- real-workspace MCP smoke passed on both workspaces
- observed large-workspace memory stayed bounded well below the previous multi-gigabyte regression range:
  - OpenCV peak private bytes: about `304 MiB`
  - LLVM peak private bytes: about `1.34 GiB`

---

## 5. Acceptance Criteria Summary

| Epic | Exit Gate |
|---|---|
| M12-E1 | ambiguous heuristic responses expose `topCandidates` and `selectedReason` |
| M12-E2 | upstream traversal works through `get_callgraph(direction="callers")` and `find_callers_recursive` |
| M12-E3 | enum members are exact symbols and enum-value usages are queryable |
| M12-E4 | compact structural modes exist without breaking current default responses |
| M12-E5 | reliability and coverage signals are visible on major navigation responses |

---

## 6. Non-Goals

- full macro expansion or compiler-grade semantic recovery
- automatic transport-level control over whether an MCP client writes large results to disk
- broad redesign of `investigate_workflow`
- ranking-system overhaul beyond surfacing and explaining already-computed ambiguity results
- speculative low-confidence call-edge generation tables in this milestone

---

## 7. First Slice Recommendation

Start with M12-E1 first.

Reason:

- it is high user value
- it builds directly on existing MS10 ranking work
- it stays mostly in the server/query layer
- it should produce visible user-facing improvement without reopening indexer-scale risk immediately

Suggested implementation order for the first slice:

1. add `topCandidates` and `selectedReason` to heuristic response models
2. wire the fields through `lookup_function`, `lookup_class`, and `find_callers`
3. add ambiguity fixture tests
4. then decide whether `find_all_overloads` should land in the same slice or the next one
