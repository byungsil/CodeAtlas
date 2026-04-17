# CodeAtlas Next Tasks

## 1. Goal

This document defines the next development plan for CodeAtlas as a specialized MCP and indexing system for very large C++ projects.

The target is not to become a general-purpose semantic IDE toolkit like Serena.

The target is:

- faster structure lookup on large C++ codebases
- better call and reference understanding for AI agents
- lower token usage through structured queries
- stable incremental operation on active game-development repositories

This plan covers the priorities previously identified as:

- Top Priority
- Second Priority
- Third Priority

Only these priorities are included here.

---

## 2. Product Direction

CodeAtlas should evolve into:

- a high-speed C++ code intelligence engine
- optimized for large monolithic or multi-module game codebases
- focused on structure, relationships, impact analysis, and change awareness
- explicit about uncertainty instead of pretending to have full compiler-grade truth

CodeAtlas should avoid:

- broad multi-language ambitions
- generic text-editing tool sprawl
- large feature branches that do not improve C++ scale, accuracy, or workflow utility

The guiding question for every feature should be:

> Does this make an AI agent more accurate, faster, or more token-efficient on a very large C++ codebase?

If not, it is probably out of scope.

---

## 3. Execution Order

Recommended implementation order:

1. Accuracy foundation
2. Reverse-reference and impact queries
3. Incremental indexing reliability
4. Query-layer expansion for real agent workflows
5. Large-project metadata and subsystem intelligence
6. Benchmarking and performance proof
7. Selective semantic enrichment from build metadata

Reasoning:

- Steps 1 to 3 improve trust.
- Steps 4 to 5 improve practical usefulness.
- Step 6 proves the project's strategic advantage.
- Step 7 raises the semantic ceiling without changing the product identity.

---

## 4. Phase A. Accuracy Foundation

### A1. Qualified Symbol Lookup

Goal:

- Stop relying on short-name-first lookup as the main resolution path.
- Make agents able to request exact symbols deterministically.

Problems today:

- `lookup_function` and `lookup_class` currently begin from name-based matches.
- The API contract already acknowledges first-match behavior for duplicates.
- Large C++ projects frequently contain duplicate short names across namespaces, classes, modules, test code, and generated code.

Tasks:

- Define a canonical exact-lookup path using `id` or `qualifiedName`.
- Add new store methods:
  - `getSymbolByQualifiedName`
  - `getSymbolsByQualifiedPrefix` if useful for namespace/class browsing
- Decide whether `id` and `qualifiedName` are guaranteed identical long-term.
- Add MCP tools or tool parameters for exact lookup:
  - either `lookup_function({ qualifiedName })`
  - or a new explicit tool such as `lookup_symbol`
- Add exact lookup support in HTTP API.
- Preserve backward compatibility for existing name-based endpoints.
- Update docs so short-name lookup is clearly described as heuristic, while qualified lookup is exact.

Implementation notes:

- Prefer exact lookup when `qualifiedName` is present.
- Continue supporting short-name lookup for exploratory search.
- Avoid ambiguous auto-selection when multiple exact-scope candidates exist.

Deliverables:

- new exact lookup query path in SQLite store
- MCP support for qualified lookup
- API documentation update
- tests for duplicate short names across namespaces/classes

Validation:

- Fixture with at least:
  - `Gameplay::Update`
  - `UI::Update`
  - `AI::Controller::Update`
- Confirm short-name lookup exposes ambiguity or heuristic behavior explicitly.
- Confirm exact lookup always returns the intended symbol.

Done when:

- Agents can target exact symbols without relying on first-match behavior.

---

### A2. Overload and Scope Disambiguation

Goal:

- Improve correctness when multiple callable symbols share the same short name.

Problems today:

- Resolver logic falls back to sibling-parent matching and then first candidate.
- This is too weak for overloads, free-function duplicates, static helpers, or utility namespaces.

Tasks:

- Extend symbol model with disambiguation metadata where cheaply available:
  - parameter count
  - signature text
  - containing namespace
  - containing class
  - declaration vs definition role
- Improve parser output to retain more normalized signature fragments.
- Introduce candidate ranking rules for call resolution:
  - exact parent/class match
  - namespace proximity
  - file-local proximity
  - declaration/definition preference
  - parameter-count hint if callable syntax exposes argument count
- Record ambiguity when multiple candidates remain plausible.
- Add confidence and match-reason fields internally.
- Decide whether low-confidence calls should be:
  - omitted
  - stored as uncertain
  - returned only in debug mode

Deliverables:

- enhanced resolver ranking logic
- richer symbol metadata in index
- ambiguity policy
- regression fixtures for overloaded functions/methods

Validation:

- Cases to cover:
  - overloaded free functions in same namespace
  - same method name in sibling classes
  - global helper and member method with same short name
  - header declaration plus source definition
- Verify resolved call edges are more precise than current first-candidate behavior.
- Verify uncertain cases are marked, not silently misrepresented.

Done when:

- Call resolution uses ranked structural evidence instead of mostly first-candidate fallback.

---

### A3. Receiver-Aware Method Resolution

Goal:

- Use call-site shape to better resolve method calls on objects.

Problems today:

- `RawCallSite.receiver` exists but is not currently used.
- In C++, method meaning often depends on receiver type or naming context.

Tasks:

- Improve call-site extraction to distinguish:
  - direct unqualified calls
  - `obj.method()`
  - `ptr->method()`
  - `this->method()`
  - namespace-qualified calls
- Preserve the receiver token or receiver expression kind in the raw call model.
- Add heuristics for:
  - `this->foo()` preferring methods on caller parent class
  - `ClassName::foo()` preferring class-qualified or namespace-qualified matches
  - member access expressions preferring methods belonging to likely receiver class
- Track unresolved receiver cases explicitly.

Deliverables:

- richer `RawCallSite`
- parser extraction improvements
- receiver-aware resolver branch

Validation:

- Add fixtures with:
  - `this->UpdateState()`
  - `component->Update()`
  - `Namespace::Utility::Update()`
  - `obj.Process()` where sibling/global names also exist

Done when:

- Receiver context materially improves method resolution accuracy on class-heavy code.

---

### A4. Header/Source Pair Unification

Goal:

- Represent the same logical symbol consistently across header and source files.

Problems today:

- Merge logic already prefers `.cpp` over `.h` in some cases, but the policy is still narrow.
- Large C++ codebases rely heavily on declarations in headers and definitions in sources or inlined headers.

Tasks:

- Define explicit symbol lifecycle rules:
  - declaration-only
  - definition-only
  - both declaration and definition present
  - inline/header-only implementation
- Extend symbol schema if needed:
  - `decl_file_path`
  - `decl_line`
  - `def_file_path`
  - `def_line`
  - representative file/line for UI display
- Update merge logic to preserve both declaration and definition metadata.
- Ensure references and call edges map to the logical symbol ID, not separate pseudo-duplicates.

Deliverables:

- improved merged symbol model
- schema migration if necessary
- updated API response shape if exposing declaration/definition details

Validation:

- Add fixtures for:
  - declaration in `.h`, definition in `.cpp`
  - inline method in `.h`
  - template method defined in header

Done when:

- Agents see one logical symbol with consistent declaration/definition context.

---

### A5. Uncertainty and Confidence Surfacing

Goal:

- Make wrong answers visible as uncertain instead of silently authoritative.

Tasks:

- Add internal confidence grading for:
  - exact
  - high-confidence heuristic
  - ambiguous
  - unresolved
- Decide where confidence appears:
  - stored in DB
  - returned in query responses
  - debug-only endpoint/tool
- Add match-reason metadata such as:
  - `exact_qualified_match`
  - `same_parent_match`
  - `namespace_proximity_match`
  - `fallback_first_candidate`
- Add docs explaining that CodeAtlas is structural, not full compiler semantics.

Deliverables:

- confidence taxonomy
- response model extension
- docs update

Validation:

- Tests must assert that uncertain cases are labeled consistently.

Done when:

- CodeAtlas can communicate "likely" vs "exact" vs "ambiguous" clearly.

---

## 5. Phase B. Reverse References and Impact Queries

### B1. Caller-First Query Surface

Goal:

- Support the most common engineering question in large codebases:
  - "Who calls this?"

Tasks:

- Add explicit MCP tool:
  - `find_callers`
- Optionally add a unified relation tool:
  - `find_related_symbols`
- Add HTTP endpoint for caller queries if the HTTP layer remains part of the workflow.
- Ensure results support pagination or truncation markers for hot utility functions.
- Rank callers in a useful order:
  - direct callers first
  - group by subsystem/file/class if useful

Deliverables:

- caller query implementation
- MCP contract
- response schema with truncation support

Validation:

- Fixture with high fan-in symbol.
- Verify repeated callers are deduplicated.
- Verify missing symbols produce clear error shape.

Done when:

- Agents can retrieve inbound call relationships directly without reconstructing them manually.

---

### B2. General Symbol Reference Search

Goal:

- Go beyond call edges and find wider symbol usage where feasible.

Scope:

- This is not full compiler-grade reference finding.
- Start with structurally reliable references that are still useful for AI workflows.

Tasks:

- Define reference categories:
  - function call
  - method call
  - class instantiation
  - member access mention
  - type usage
  - inheritance mention
- Decide which categories are in scope for first release.
- Add a `references` storage table if needed.
- Extend parser/indexer to collect additional reference events.
- Add query tool:
  - `find_references(symbol)`
- Mark category and confidence per result.

Deliverables:

- reference model
- extraction path
- DB schema and query path
- MCP tool

Validation:

- Fixtures must include:
  - constructor usage
  - variable typed as class
  - inheritance
  - function pointer or callback-like syntax if supported
- Unsupported categories must fail clearly or be omitted intentionally.

Done when:

- CodeAtlas can answer more than just direct call questions for major symbol kinds.

---

### B3. Impact Analysis Query

Goal:

- Provide an agent-ready answer for change-risk questions.

Tasks:

- Design a high-level query:
  - `impact_analysis`
- Initial inputs:
  - symbol or qualified name
  - depth
  - relation kinds
  - optional subsystem/file filters
- Output should summarize:
  - direct callers
  - transitive callers or callees
  - likely affected classes/files/subsystems
  - uncertainty markers
  - truncation markers
- Avoid dumping raw full graphs when a summary is enough.
- Include counts and "next best drill-downs."

Deliverables:

- impact-analysis response model
- graph traversal logic for inbound and outbound relations
- summary builder

Validation:

- Prepare realistic scenarios:
  - changing a base gameplay method
  - changing a utility class used in many modules
  - changing a low-fanout leaf helper
- Verify result summaries are short and useful for AI agents.

Done when:

- An agent can ask "what breaks if I change this?" and get a compact structured answer.

---

## 6. Phase C. Incremental Indexing Reliability

### C1. Correctness Audit for Incremental Updates

Goal:

- Treat incremental correctness as a product feature, not a performance optimization.

Tasks:

- Map all change scenarios:
  - edit existing file
  - create new file
  - delete file
  - rename/move file
  - branch switch
  - mass generated-file churn
  - header-only change affecting many dependents
- Document current behavior for each case.
- Build a correctness matrix listing:
  - expected updated symbols
  - expected removed symbols
  - expected relation refresh scope
  - expected unchanged records
- Add regression tests for every scenario.

Deliverables:

- incremental correctness matrix
- expanded automated tests

Done when:

- There is an explicit tested contract for incremental behavior.

---

### C2. Robust File and Dependency Change Planning

Goal:

- Ensure changed files trigger the right amount of reprocessing.

Tasks:

- Audit current hashing and file-record logic.
- Detect rename/move cases more explicitly if possible.
- Decide whether file identity is path-based only or can be content-assisted.
- Add re-index planning rules for declaration-heavy files:
  - if a header changes, which downstream records must be revisited?
- Start with conservative correctness before aggressive minimization.
- Consider a dependency hint table for includes or symbol ownership if needed later.

Deliverables:

- improved incremental planner
- policy for header-change fanout

Validation:

- Header change should not leave stale call or symbol relationships.
- No-change rerun should remain near-zero work.

Done when:

- Incremental planning is predictably correct across normal game-dev edit patterns.

---

### C3. Watcher Reliability and Recovery

Goal:

- Make watch mode trustworthy for daily use.

Tasks:

- Audit event normalization for Windows-heavy developer workflows.
- Handle noisy save patterns:
  - temp-file swap
  - multiple rapid write events
  - editor-specific file replacement behavior
- Ensure queueing avoids DB corruption and duplicate work.
- Add recovery behavior after parser failure or partial indexing failure.
- Add startup validation for existing DB consistency.
- Add safe shutdown handling for MCP-launched watcher child process.

Deliverables:

- watcher event policy
- retry/recovery behavior
- failure-mode tests

Validation:

- Simulate event bursts and partial failures.
- Verify DB remains readable after interruption.

Done when:

- Watch mode can be used continuously without obvious drift or corruption.

---

### C4. Branch Switch and Large Change Resilience

Goal:

- Survive real repository usage instead of idealized single-file edits only.

Tasks:

- Add heuristics to detect branch-switch-like events:
  - many file updates in a short window
  - many deletes + creates
- Decide when to:
  - continue incrementally
  - trigger a fast consistency sweep
  - recommend or force a rebuild
- Add diagnostics for why a rebuild was suggested.

Deliverables:

- branch-switch handling policy
- rebuild recommendation logic

Validation:

- Simulate a branch switch in fixtures or a controlled repo sample.

Done when:

- CodeAtlas fails safe instead of silently serving stale structure after large repo transitions.

---

## 7. Phase D. Query-Layer Expansion for Real Agent Workflows

### D1. Symbol Overview Queries

Goal:

- Let agents inspect files, namespaces, and classes without opening raw code.

Tasks:

- Add:
  - `list_file_symbols(path)`
  - `list_namespace_symbols(qualifiedName)`
  - `list_class_members(qualifiedName)`
- Define stable ordering:
  - declaration order
  - line number
- Return compact summaries first.

Deliverables:

- overview endpoints/tools
- summary response models

Done when:

- Agents can browse structure progressively instead of jumping straight to raw files.

---

### D2. Override and Inheritance Queries

Goal:

- Support object-oriented gameplay code navigation.

Tasks:

- Extend parser/reference model to detect:
  - inheritance edges
  - virtual overrides where structurally inferable
- Add queries:
  - `find_base_methods`
  - `find_overrides`
  - `get_type_hierarchy`
- If exact override detection is impossible in some cases, return likely candidates with confidence markers.

Deliverables:

- inheritance relation model
- hierarchy query tools

Validation:

- Fixtures with:
  - base interface
  - derived gameplay component
  - multiple overrides

Done when:

- Agents can navigate polymorphic behavior without broad text search.

---

### D3. Trace Path Queries

Goal:

- Help agents connect two symbols through call relationships.

Tasks:

- Add query:
  - `trace_call_path(source, target, maxDepth)`
- Decide path search strategy and guardrails.
- Return:
  - shortest path found
  - multiple candidates if cheap enough
  - truncation or search-limit metadata

Deliverables:

- path-tracing query logic
- response model

Validation:

- Fixtures with multiple possible paths.
- Ensure expensive searches are bounded.

Done when:

- Agents can answer "how do we get from A to B?" without manual graph reconstruction.

---

## 8. Phase E. Large-Project Metadata and Subsystem Intelligence

### E1. Project Metadata Model

Goal:

- Enrich structure with repository-specific context that matters in large games.

Tasks:

- Define tags or derived metadata for:
  - subsystem
  - module
  - runtime/editor/tool/test/generated
  - public/private/internal header role
  - engine/gameplay/UI/networking/AI or project-specific domain buckets
- Determine metadata sources:
  - path conventions
  - config file
  - explicit rules
  - generated heuristics
- Keep the first version simple and deterministic.

Deliverables:

- metadata schema
- tagging pipeline
- config format if needed

Validation:

- Use a realistic directory fixture to verify stable tagging.

Done when:

- Symbols and files can be grouped by meaningful project boundaries.

---

### E2. Metadata-Aware Query Results

Goal:

- Make results more useful by grouping and filtering on project structure.

Tasks:

- Add optional filters to search and impact queries:
  - subsystem
  - module
  - generated/non-generated
  - tests included/excluded
- Add grouped summaries:
  - callers by subsystem
  - references by module
  - impact counts by area

Deliverables:

- metadata-aware filters
- grouped summary output

Validation:

- Ensure grouped results remain token-efficient and not bloated.

Done when:

- Agents can reason at subsystem level, not just symbol level.

---

## 9. Phase F. Benchmarking and Performance Proof

### F1. Benchmark Harness

Goal:

- Turn performance claims into repeatable measurements.

Tasks:

- Define benchmark datasets:
  - small deterministic fixture
  - medium synthetic or curated sample
  - large real-project benchmark if available
- Capture:
  - full index time
  - incremental index time
  - watcher catch-up time
  - query latency by query type
  - DB size
  - memory usage
- Add a benchmark runner script or command set.
- Document machine/environment assumptions.

Deliverables:

- benchmark harness
- benchmark docs
- stored baseline results

Done when:

- Performance is measurable and regressions are trackable.

---

### F2. Query Hot-Path Optimization

Goal:

- Reduce latency on the queries agents will use most.

Tasks:

- Profile current hot queries:
  - search
  - exact symbol lookup
  - callers
  - references
  - call path
  - impact analysis
- Add or refine DB indexes.
- Review whether some precomputed aggregates are worth storing.
- Avoid premature denormalization that complicates incremental correctness.

Deliverables:

- profiled hot-path report
- targeted DB/index changes

Validation:

- Compare p50 and p95 latency before/after.

Done when:

- Common structural queries stay interactive on large datasets.

---

### F3. Incremental and Watcher Scale Benchmarks

Goal:

- Prove the real differentiator of CodeAtlas.

Tasks:

- Benchmark:
  - no-change rerun
  - single `.cpp` edit
  - header edit
  - mass file update burst
  - branch-switch-like event batch
- Record:
  - time
  - affected rows
  - memory
  - rebuild recommendation behavior

Deliverables:

- incremental-scale report
- regression thresholds

Done when:

- CodeAtlas can demonstrate operational superiority for large active C++ workspaces.

---

## 10. Phase G. Selective Semantic Enrichment

### G1. Build Metadata Integration

Goal:

- Raise resolution quality without turning CodeAtlas into a general LSP platform.

Tasks:

- Support optional ingestion of:
  - `compile_commands.json`
  - include directories
  - macro definitions when cheaply available
- Define how this metadata influences parsing or resolution:
  - file normalization
  - include ownership hints
  - namespace/class ambiguity reduction
- Keep this integration optional, not mandatory for basic operation.

Deliverables:

- build-metadata ingestion layer
- config and docs

Validation:

- Verify better resolution on tricky real-world fixtures.
- Verify basic mode still works without compile DB.

Done when:

- CodeAtlas can use available build data to improve accuracy, but does not depend on it for core identity.

---

### G2. Include and Macro Context Hints

Goal:

- Improve analysis around typical C++ complexity sources.

Tasks:

- Collect lightweight include graph information.
- Track macro-heavy or parse-fragile files explicitly.
- Add metadata on symbols or files:
  - parse fragility
  - macro-sensitive
  - include-heavy
- Use these signals in confidence scoring and agent guidance.

Deliverables:

- include graph or include hint model
- parse-fragility metadata

Done when:

- Hard C++ corners are surfaced as risk signals instead of silent inaccuracies.

---

## 11. Cross-Cutting Requirements

These rules apply across all phases:

- Optimize for correctness before optimization tricks.
- Prefer explicit uncertainty over silent false precision.
- Preserve workspace-relative paths everywhere.
- Keep MCP payloads compact and agent-friendly.
- Bound expensive queries and mark truncation explicitly.
- Keep full raw-source reading out of the agent query loop.
- Prefer repository-scale resilience over demo-scale polish.
- Avoid adding broad editing/refactoring scope unless it directly serves the C++ large-project mission.

---

## 12. Suggested Milestones

### Milestone 1. Trustworthy Lookup

Detailed plan:

- [Milestone1_TrustworthyLookup.md](Milestone1_TrustworthyLookup.md)

Includes:

- A1
- A2
- A3
- A4
- A5

Success outcome:

- Exact symbol lookup works.
- Call resolution is materially more reliable.
- Ambiguity is surfaced clearly.

### Milestone 2. Real Impact Navigation

Detailed plan:

- [Milestone2_RealImpactNavigation.md](Milestone2_RealImpactNavigation.md)

Includes:

- B1
- B2
- B3
- D1

Success outcome:

- Agents can answer caller, reference, and impact questions without raw file reading.

### Milestone 3. Production-Grade Incremental Operation

Detailed plan:

- [Milestone3_ProductionGradeIncrementalOperation.md](Milestone3_ProductionGradeIncrementalOperation.md)

Includes:

- C1
- C2
- C3
- C4

Success outcome:

- CodeAtlas stays correct and current during real development workflows.

### Milestone 4. Large-Project Intelligence

Detailed plan:

- [Milestone4_LargeProjectIntelligence.md](Milestone4_LargeProjectIntelligence.md)

Includes:

- D2
- D3
- E1
- E2

Success outcome:

- Agents can reason in terms of hierarchy, path flow, and subsystem-level impact.

### Milestone 5. Performance Proof

Detailed plan:

- [Milestone5_PerformanceProof.md](Milestone5_PerformanceProof.md)

Includes:

- F1
- F2
- F3
- G1
- G2

Success outcome:

- CodeAtlas can prove its C++ large-repo advantage with data, not just design intent.

---

## 13. Milestone Breakdown for Implementation

Primary execution documents:

- [Milestone1_TrustworthyLookup.md](Milestone1_TrustworthyLookup.md)
- [Milestone2_RealImpactNavigation.md](Milestone2_RealImpactNavigation.md)
- [Milestone3_ProductionGradeIncrementalOperation.md](Milestone3_ProductionGradeIncrementalOperation.md)
- [Milestone4_LargeProjectIntelligence.md](Milestone4_LargeProjectIntelligence.md)
- [Milestone5_PerformanceProof.md](Milestone5_PerformanceProof.md)

Use the milestone files as the primary implementation plans. This section remains as an embedded reference copy.

This section breaks each milestone into implementation-ready work items.

Each milestone is organized as:

- Epic
- Implementation tasks
- Expected touch points
- Validation checklist
- Exit criteria

The goal is to make milestone planning directly usable for execution, issue creation, and branch planning.

---

### Milestone 1. Trustworthy Lookup

Objective:

- Make symbol lookup and relationship resolution trustworthy enough that agents can use CodeAtlas as a primary structural source.

Recommended internal order:

1. M1-E1. Exact lookup contract
2. M1-E2. Storage and query support
3. M1-E3. Parser metadata enrichment
4. M1-E4. Resolver ranking improvements
5. M1-E5. Header/source unification
6. M1-E6. Confidence and ambiguity surfacing
7. M1-E7. Tool and API updates
8. M1-E8. Fixture expansion and regression tests

#### M1-E1. Exact Lookup Contract

Tasks:

- Define the canonical exact identity for a symbol.
- Decide whether `id` remains the canonical storage key and whether `qualifiedName` is an alias or separately persisted field.
- Define exact lookup request/response shape for MCP.
- Define exact lookup request/response shape for HTTP.
- Define ambiguity behavior for legacy short-name lookups.
- Update docs to distinguish:
  - exact lookup
  - exploratory lookup
  - heuristic lookup

Expected touch points:

- `dev_docs/API_CONTRACT.md`
- `dev_docs/AGENT_WORKFLOW.md`
- `README.md`
- `server/src/models/*`

Validation checklist:

- Exact lookup contract is written before implementation starts.
- Legacy lookup behavior remains explicitly documented.

#### M1-E2. Storage and Query Support

Tasks:

- Add `getSymbolByQualifiedName` to the store interface.
- Add SQLite query path for qualified lookup.
- Add JSON store parity if JSON remains needed for tests or fallback.
- Add optional prefix or container listing support if useful for namespace/class browsing.
- Add DB indexes if qualified lookups are not already efficiently covered.

Expected touch points:

- `server/src/storage/store.ts`
- `server/src/storage/sqlite-store.ts`
- `server/src/storage/json-store.ts`

Validation checklist:

- Duplicate short-name fixtures return different results by qualified lookup.
- Exact lookup latency remains interactive.

#### M1-E3. Parser Metadata Enrichment

Tasks:

- Extend parser output with metadata useful for disambiguation:
  - parameter count
  - signature text
  - containing namespace/class
  - declaration/definition role
- Improve raw call-site extraction:
  - unqualified call
  - member access call
  - pointer member access call
  - class-qualified or namespace-qualified call
  - `this`-based call
- Decide whether additional metadata belongs in raw parse output only or in merged symbol rows too.

Expected touch points:

- `indexer/src/parser.rs`
- `indexer/src/models.rs`
- `server/src/parser/cpp-parser.ts` if kept aligned for fixtures or fallback

Validation checklist:

- Parser tests cover each new call-site kind.
- Metadata stays deterministic across repeated indexing.

#### M1-E4. Resolver Ranking Improvements

Tasks:

- Refactor resolver into explicit ranking stages instead of ad hoc fallback.
- Add candidate scoring inputs:
  - same parent match
  - same namespace match
  - receiver-aware class preference
  - parameter-count compatibility
  - declaration/definition preference
  - file-local proximity
- Define tie-breaking policy.
- Add unresolved and ambiguous result paths.
- Decide whether uncertain edges are stored, filtered, or both.

Expected touch points:

- `indexer/src/resolver.rs`
- `indexer/src/indexing.rs`
- `indexer/src/models.rs`

Validation checklist:

- Overload fixtures outperform current first-candidate behavior.
- Ambiguous cases are visible in tests.

#### M1-E5. Header/Source Unification

Tasks:

- Define symbol merge rules for declaration and definition pairs.
- Preserve declaration metadata and definition metadata separately.
- Decide representative symbol fields returned to agents.
- Add schema support if extra fields are needed.
- Ensure symbol IDs remain stable when only declaration/definition location changes.

Expected touch points:

- `indexer/src/resolver.rs`
- `indexer/src/storage.rs`
- `server/src/models/symbol.ts`
- `server/src/storage/sqlite-store.ts`

Validation checklist:

- Same logical method in `.h` and `.cpp` appears as one merged symbol.
- Agents can still inspect both declaration and definition context if exposed.

#### M1-E6. Confidence and Ambiguity Surfacing

Tasks:

- Define confidence levels and match reasons.
- Decide whether confidence is:
  - DB-persisted
  - query-time only
  - debug-only
- Extend response models for callers, callees, and symbol lookup.
- Add ambiguity markers to short-name lookup responses.
- Add docs explaining structural-confidence semantics.

Expected touch points:

- `server/src/models/responses.ts`
- `server/src/mcp.ts`
- `server/src/app.ts`
- `dev_docs/API_CONTRACT.md`

Validation checklist:

- Tests assert confidence and match-reason values for known ambiguous fixtures.

#### M1-E7. Tool and API Updates

Tasks:

- Add exact lookup support to MCP tools.
- Decide whether to:
  - extend `lookup_function` and `lookup_class`
  - or add a new generic `lookup_symbol`
- Add backward-compatible HTTP support.
- Add migration notes for users relying on name-only lookup.

Expected touch points:

- `server/src/mcp.ts`
- `server/src/app.ts`
- `server/src/__tests__/mcp.test.ts`
- `server/src/__tests__/endpoints.test.ts`

Validation checklist:

- Existing tests still pass.
- New exact lookup tests pass.

#### M1-E8. Fixture Expansion and Regression Tests

Tasks:

- Build a dedicated ambiguity fixture workspace containing:
  - duplicate short names across namespaces
  - overloaded free functions
  - same method names in sibling classes
  - declaration/definition split
  - `this->` and `ptr->` method calls
- Add regression tests at:
  - parser layer
  - resolver layer
  - storage layer
  - MCP/API contract layer

Expected touch points:

- `samples/`
- `indexer/src/*tests*`
- `server/src/__tests__/*`

Exit criteria:

- Exact lookup is stable.
- Resolver accuracy is improved on ambiguous fixtures.
- Agents can distinguish exact from heuristic answers.

---

### Milestone 2. Real Impact Navigation

Objective:

- Turn CodeAtlas from a lookup tool into a practical change-analysis tool.

Recommended internal order:

1. M2-E1. Direct caller queries
2. M2-E2. Reference model definition
3. M2-E3. Reference extraction
4. M2-E4. Reference storage and retrieval
5. M2-E5. Impact-analysis summarization
6. M2-E6. Symbol overview queries
7. M2-E7. Token-efficient response shaping

#### M2-E1. Direct Caller Queries

Tasks:

- Add `find_callers` MCP tool.
- Add corresponding HTTP endpoint if HTTP remains supported.
- Define pagination or truncation behavior for high fan-in symbols.
- Add ranking and grouping strategy.

Expected touch points:

- `server/src/mcp.ts`
- `server/src/app.ts`
- `server/src/storage/*`

Validation checklist:

- High fan-in fixtures return deterministic ordering and truncation markers.

#### M2-E2. Reference Model Definition

Tasks:

- Define the first-release reference categories.
- Decide which categories share storage with `calls` and which require a dedicated `references` table.
- Define normalized reference payload:
  - source symbol
  - target symbol
  - category
  - file path
  - line
  - confidence

Expected touch points:

- `dev_docs/API_CONTRACT.md`
- `indexer/src/models.rs`
- `server/src/models/*`

Validation checklist:

- Reference categories are documented and deliberately scoped.

#### M2-E3. Reference Extraction

Tasks:

- Extend parser/indexer to emit additional reference events.
- Start with the most reliable categories:
  - method/function call
  - class/type usage
  - inheritance
- Defer weaker categories until confidence handling is ready.

Expected touch points:

- `indexer/src/parser.rs`
- `indexer/src/indexing.rs`
- `indexer/src/models.rs`

Validation checklist:

- Each supported category has fixture coverage and explicit expectations.

#### M2-E4. Reference Storage and Retrieval

Tasks:

- Add schema migration for references if needed.
- Add query paths:
  - by target symbol
  - by reference category
  - optionally by file/subsystem
- Add MCP tool `find_references`.

Expected touch points:

- `indexer/src/storage.rs`
- `server/src/storage/sqlite-store.ts`
- `server/src/mcp.ts`

Validation checklist:

- `find_references` returns correct category labels and confidence fields.

#### M2-E5. Impact-Analysis Summarization

Tasks:

- Define `impact_analysis` request parameters.
- Implement graph traversal over callers, callees, and references.
- Add bounded search behavior.
- Build summary-first response:
  - top affected symbols
  - affected files
  - affected subsystems
  - confidence and truncation
  - recommended follow-up queries

Expected touch points:

- `server/src/mcp.ts`
- `server/src/models/responses.ts`
- `server/src/storage/sqlite-store.ts`

Validation checklist:

- Output is structured for agents, not raw graph dumping.
- Realistic fixtures produce concise summaries.

#### M2-E6. Symbol Overview Queries

Tasks:

- Add file-level symbol listing.
- Add class/member listing by qualified name.
- Add namespace/container listing.
- Ensure ordering is stable and compact.

Expected touch points:

- `server/src/mcp.ts`
- `server/src/app.ts`
- `server/src/storage/sqlite-store.ts`

Validation checklist:

- Agents can browse structure progressively using only structured queries.

#### M2-E7. Token-Efficient Response Shaping

Tasks:

- Add summary fields before large detail arrays.
- Add truncation metadata consistently.
- Add optional `maxResults`, `includeSummary`, or detail flags if needed.
- Remove redundant payload fields where possible.

Expected touch points:

- `server/src/models/responses.ts`
- `server/src/mcp.ts`
- `dev_docs/API_CONTRACT.md`

Exit criteria:

- Agents can answer "who calls this?", "where is this used?", and "what breaks if this changes?" without raw file reading.

---

### Milestone 3. Production-Grade Incremental Operation

Objective:

- Make CodeAtlas safe to trust during active repository development.

Recommended internal order:

1. M3-E1. Incremental correctness matrix
2. M3-E2. Regression fixture suite
3. M3-E3. File identity and planning upgrades
4. M3-E4. Header-change fanout policy
5. M3-E5. Watcher event hardening
6. M3-E6. Failure recovery and DB safety
7. M3-E7. Branch switch and mass-change handling

#### M3-E1. Incremental Correctness Matrix

Tasks:

- Enumerate all change scenarios and expected outcomes.
- Write the matrix into docs or a dedicated engineering note.
- Use the matrix as the acceptance reference for later code changes.

Expected touch points:

- `dev_docs/`
- `dev_docs/NestTask.md` or a dedicated incremental design doc

Validation checklist:

- Every change scenario has expected symbol, reference, and file-record outcomes.

#### M3-E2. Regression Fixture Suite

Tasks:

- Build scenario fixtures for:
  - edit
  - add
  - delete
  - rename
  - header change
  - branch-like bulk churn
- Add helpers to assert DB state before and after update.

Expected touch points:

- `samples/`
- `indexer/src/incremental.rs`
- `indexer/src/storage.rs`

Validation checklist:

- Regression tests reproduce each scenario deterministically.

#### M3-E3. File Identity and Planning Upgrades

Tasks:

- Audit content hash and file-record behavior.
- Improve rename/move handling if feasible.
- Decide whether planner needs path-only identity or content-assisted heuristics.
- Refactor planner to make re-index decisions inspectable in logs/tests.

Expected touch points:

- `indexer/src/incremental.rs`
- `indexer/src/models.rs`
- `indexer/src/storage.rs`

Validation checklist:

- Planner output is testable, not implicit.

#### M3-E4. Header-Change Fanout Policy

Tasks:

- Define what happens when a header changes:
  - only local symbol rewrite
  - dependent relation refresh
  - broader consistency sweep
- Add conservative fallback behavior.
- Record why a broader refresh was triggered.

Expected touch points:

- `indexer/src/incremental.rs`
- `indexer/src/indexing.rs`
- `indexer/src/storage.rs`

Validation checklist:

- Header edits never leave stale merged symbols or stale call/reference relationships.

#### M3-E5. Watcher Event Hardening

Tasks:

- Normalize editor-specific save patterns.
- Add or refine debounce behavior.
- Prevent duplicate queue entries for repeated burst events.
- Add event tracing for debugging.

Expected touch points:

- `indexer/src/watcher.rs`
- `indexer/src/incremental.rs`

Validation checklist:

- Save bursts do not trigger redundant large work.

#### M3-E6. Failure Recovery and DB Safety

Tasks:

- Ensure partial parse failures do not corrupt DB state.
- Define transactional boundaries for incremental writes.
- Add startup integrity check for existing DB.
- Add recovery logging and actionable error messages.

Expected touch points:

- `indexer/src/storage.rs`
- `indexer/src/indexing.rs`
- `indexer/src/watcher.rs`

Validation checklist:

- Interrupted or failed runs leave DB readable and recoverable.

#### M3-E7. Branch Switch and Mass-Change Handling

Tasks:

- Detect branch-like event bursts.
- Define thresholds for:
  - continue incrementally
  - consistency sweep
  - rebuild recommendation
- Surface diagnostics explaining why a heavier recovery path was chosen.

Expected touch points:

- `indexer/src/watcher.rs`
- `indexer/src/incremental.rs`
- `README.md` or docs

Exit criteria:

- CodeAtlas remains correct across normal edits, noisy saves, and large repository state transitions.

---

### Milestone 4. Large-Project Intelligence

Objective:

- Add higher-level structure so agents can reason in subsystem and hierarchy terms, not only symbol terms.

Recommended internal order:

1. M4-E1. Inheritance relation model
2. M4-E2. Override candidate logic
3. M4-E3. Type hierarchy queries
4. M4-E4. Call-path tracing
5. M4-E5. Project metadata model
6. M4-E6. Metadata-aware filtering and grouping

#### M4-E1. Inheritance Relation Model

Tasks:

- Parse inheritance edges.
- Add relation storage for base/derived links.
- Expose them through internal query methods.

Expected touch points:

- `indexer/src/parser.rs`
- `indexer/src/models.rs`
- `indexer/src/storage.rs`

#### M4-E2. Override Candidate Logic

Tasks:

- Build structural override detection heuristics.
- Attach confidence markers for likely overrides.
- Avoid false certainty when type information is incomplete.

Expected touch points:

- `indexer/src/resolver.rs`
- `server/src/models/responses.ts`

#### M4-E3. Type Hierarchy Queries

Tasks:

- Add `get_type_hierarchy`.
- Add `find_base_methods`.
- Add `find_overrides`.

Expected touch points:

- `server/src/mcp.ts`
- `server/src/storage/sqlite-store.ts`

#### M4-E4. Call-Path Tracing

Tasks:

- Implement bounded path search.
- Define search limits and truncation semantics.
- Return compact path summaries.

Expected touch points:

- `server/src/mcp.ts`
- `server/src/storage/sqlite-store.ts`

#### M4-E5. Project Metadata Model

Tasks:

- Define subsystem/module/tag derivation rules.
- Add config or rule format if path heuristics alone are insufficient.
- Persist or derive metadata consistently.

Expected touch points:

- `indexer/src/models.rs`
- `indexer/src/storage.rs`
- config/docs files

#### M4-E6. Metadata-Aware Filtering and Grouping

Tasks:

- Add query filters by subsystem/module/tag.
- Add grouped summaries in caller/reference/impact outputs.

Expected touch points:

- `server/src/mcp.ts`
- `server/src/models/responses.ts`
- `server/src/storage/sqlite-store.ts`

Exit criteria:

- Agents can ask not only "what symbol?" but "what subsystem?", "what hierarchy?", and "what path?".

---

### Milestone 5. Performance Proof

Objective:

- Prove that CodeAtlas deserves to exist as a specialized large-C++ tool.

Recommended internal order:

1. M5-E1. Benchmark design
2. M5-E2. Benchmark harness implementation
3. M5-E3. Query profiling and hot-path optimization
4. M5-E4. Incremental and watcher scale benchmarks
5. M5-E5. Build metadata ingestion
6. M5-E6. Include and macro risk signals

#### M5-E1. Benchmark Design

Tasks:

- Define dataset tiers.
- Define required measurements and output format.
- Define benchmark environment recording rules.

Expected touch points:

- `dev_docs/`
- benchmark scripts folder if created

#### M5-E2. Benchmark Harness Implementation

Tasks:

- Add scripts or commands for repeatable benchmark runs.
- Capture timing, DB size, memory, and row counts.
- Store baseline results in a documented location.

Expected touch points:

- new benchmark tooling files
- `README.md`

#### M5-E3. Query Profiling and Hot-Path Optimization

Tasks:

- Profile the most important queries.
- Add targeted indexes or query rewrites.
- Avoid changes that weaken incremental correctness.

Expected touch points:

- `server/src/storage/sqlite-store.ts`
- `indexer/src/storage.rs`

#### M5-E4. Incremental and Watcher Scale Benchmarks

Tasks:

- Measure no-change rerun, single-file edits, header edits, burst edits, and branch-like churn.
- Add regression thresholds where practical.

Expected touch points:

- benchmark tooling
- `dev_docs/`

#### M5-E5. Build Metadata Ingestion

Tasks:

- Add optional `compile_commands.json` ingestion.
- Define metadata influence boundaries carefully.
- Ensure basic mode remains independent from compile DB availability.

Expected touch points:

- config ingestion code
- `indexer/src/*`
- docs

#### M5-E6. Include and Macro Risk Signals

Tasks:

- Add include graph hints where cheap.
- Mark parse-fragile or macro-heavy files.
- Feed these signals into confidence and agent guidance.

Expected touch points:

- `indexer/src/parser.rs`
- `indexer/src/models.rs`
- `server/src/models/responses.ts`

Exit criteria:

- CodeAtlas has benchmark evidence for its value proposition and a higher semantic ceiling without losing its identity.

---

## 14. Recommended Immediate Start Order

Start with these concrete tasks first:

1. A1. Qualified Symbol Lookup
2. A2. Overload and Scope Disambiguation
3. A4. Header/Source Pair Unification
4. A5. Uncertainty and Confidence Surfacing
5. B1. Caller-First Query Surface
6. B3. Impact Analysis Query
7. C1. Correctness Audit for Incremental Updates
8. C2. Robust File and Dependency Change Planning
9. D1. Symbol Overview Queries
10. F1. Benchmark Harness

Reason:

- These tasks create the fastest path to a meaningfully stronger product.
- They improve trust, usefulness, and strategic differentiation without needing a major architecture pivot.

---

## 15. Not In Scope For This Plan

The following are intentionally not primary goals for this phase:

- general multi-language support
- Serena-style broad symbolic editing suite
- generic IDE replacement ambitions
- rich human dashboard work beyond what supports validation
- broad refactoring automation beyond narrow C++ intelligence needs

---

## 16. Final Direction Check

If this plan is followed successfully, CodeAtlas should become:

- better than simple grep-based MCP tools
- narrower than Serena by design
- stronger than general semantic tools in the specific area of very large C++ project indexing, relationship analysis, and incremental operational performance

That is the intended competitive position.
