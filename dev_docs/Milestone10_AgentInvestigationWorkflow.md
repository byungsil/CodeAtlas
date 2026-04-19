# Milestone 10. Agent Investigation Workflow

Status:

- Completed

## 1. Objective

Turn CodeAtlas from a strong lookup tool into a practical investigation workflow tool for AI agents working on large C++ repositories.

This milestone focuses on:

- investigation path queries and workflow summaries
- field- and member-centric propagation strengthening
- context-aware symbol disambiguation and ranking
- hybrid semantic plus structural evidence expansion
- zero-result and coverage diagnostics

Success outcome:

- an agent can usually start a real investigation with one bounded workflow answer, one or two grounded follow-up symbols, and an honest signal when the index is weak

Positioning note:

- Milestone 5 added call-path tracing and metadata-aware reasoning
- Milestone 6 added bounded propagation vocabulary and extraction
- Milestone 9 improved representative symbol quality
- MS10 is about stitching those capabilities into a workflow that is actually usable during coding

Operational note:

- MCP is the main runtime target for this milestone
- response latency and token cost matter as much as answer richness
- this milestone should avoid growing into a large explanation system

---

## 2. Recommended Order

1. M10-E1. Investigation path queries and workflow summaries
2. M10-E2. Field- and member-centric propagation strengthening
3. M10-E3. Context-aware symbol disambiguation and ranking
4. M10-E4. Hybrid semantic plus structural evidence expansion
5. M10-E5. Zero-result and coverage diagnostics

Recommended execution rule:

- work Epic by Epic as much as possible
- allow small cross-Epic support only when an earlier Epic cannot be validated without it
- do not keep polishing a later Epic while an earlier Epic still lacks its exit criteria

Completion note:

- MS10 is now considered complete
- the remaining small refinements, if any, should move to later milestone work instead of extending MS10 further

---

## 3. Epics

### M10-E1. Investigation Path Queries and Workflow Summaries

Goal:

- let agents ask source-to-sink or field-centric investigation questions and receive one compact workflow-shaped answer instead of manually composing many smaller queries

Status:

- Completed

Implementation tasks:

- keep `investigate_workflow` as the main stitched investigation entry point
- support bounded workflow summaries with:
  - `entry`
  - `mainPath`
  - `handoffPoints`
  - `sink`
  - `uncertainSegments`
- keep the stitched answer compact for:
  - direct source-to-sink flows
  - field-centric investigation starts
  - partial-confidence workflows
- avoid adding new response sections unless a core investigation question cannot be answered without them

Validation checklist:

- an agent can ask:
  - "how do we get from A to B?"
  - "how does input reach launch?"
  - "where is this field written and later consumed?"
- the answer is bounded and summary-first
- partial paths stay useful instead of collapsing into raw graph noise

Exit criteria:

- common investigation questions can start from one bounded workflow answer without requiring manual composition of call-path and propagation tools in the common case

Completion summary:

- `investigate_workflow` now exists as a bounded investigation entry point in both HTTP and MCP
- workflow summaries cover direct source-to-sink, field-centric, partial, relay, and constructor-seeded cases
- common investigation starts no longer require manual composition of call-path and propagation tools in the common case

---

### M10-E2. Field- and Member-Centric Propagation Strengthening

Goal:

- make field and member propagation reliable enough for gameplay-style state tracing through small carriers, helper-produced objects, constructor seeds, and relay steps

Status:

- Completed

Implementation tasks:

- strengthen supported propagation patterns for:
  - constructor initialization from arguments or helper-returned state
  - member writes and later member reads
  - helper-owned temporary carrier objects
  - nested carrier objects
  - staged helper and relay handoffs
- preserve honest partial confidence for:
  - pointer-heavy shapes
  - alias-heavy shapes
  - receiver-ambiguous cases
- prefer adding substrate coverage over adding response-level polish

Validation checklist:

- fixtures cover:
  - constructor-seeded field state
  - nested helper-return handoff
  - relay helper chains
  - one-more-boundary forwarding patterns
- field-centric investigations do not lose the flow immediately after the first object handoff
- weak cases remain explicitly partial

Exit criteria:

- common gameplay-style state movement survives more than one object or helper boundary often enough that investigation workflows no longer fail too early

Completion summary:

- propagation coverage now includes constructor seeds, carriers, nested helper returns, relay helpers, forwarding helpers, and field-centric relay owners
- common gameplay-style state movement survives multiple realistic object and helper boundaries often enough for workflow stitching to stay useful
- pointer-heavy and receiver-ambiguous cases remain explicitly partial instead of overstating certainty

---

### M10-E3. Context-Aware Symbol Disambiguation and Ranking

Goal:

- reduce investigation startup friction by ranking ambiguous symbols using recent and local context instead of name match alone

Status:

- Completed

Implementation tasks:

- keep context-aware ranking focused on:
  - anchor context
  - recent symbol context
  - module and subsystem context
  - path neighborhood
  - workflow-local neighbors
- keep ambiguity visible and compact
- avoid turning ranking into a large heuristic system unless it clearly improves investigation starts

Validation checklist:

- short-name lookups such as duplicated runtime/editor symbols usually start on the intended candidate when recent or local context exists
- ambiguity responses remain compact and explainable
- ranking improvements affect actual start-of-investigation decisions, not only explanation quality

Exit criteria:

- ambiguous lookup startup is materially less annoying in real investigation flows, while alternate plausible candidates remain visible

Completion summary:

- recent and local context now steer ambiguous symbol lookup in practical investigation flows
- ambiguity remains visible and compact
- the startup friction reduction is sufficient for MS10 and does not need further ranking expansion inside this milestone

---

### M10-E4. Hybrid Semantic Plus Structural Evidence Expansion

Goal:

- attach a small amount of structural evidence to semantic workflow answers so the next investigation step can often be chosen without leaving CodeAtlas immediately

Status:

- Completed

Implementation tasks:

- keep evidence bounded and decision-oriented
- ground likely next candidates with compact support such as:
  - field handoff context
  - owner callable context
  - adjacent call support
  - relay or forwarded-step support
- improve grounding only when it helps choose the next lookup
- avoid turning responses into mini callgraph dumps or AST summaries

Validation checklist:

- suggested follow-up candidates feel grounded enough to choose a next lookup
- evidence remains bounded by count and stays summary-first
- mixed helper and relay workflows still return compact evidence

Exit criteria:

- the response contains enough bounded support that an agent can often choose the next symbol to inspect without raw file fallback

Completion summary:

- bounded evidence is now sufficient to support the next lookup decision in common investigation flows
- owner-aware and relay-aware grounding exists without turning responses into oversized structural dumps
- recent cleanup reduced explanation-heavy payload growth while keeping practical grounding intact

---

### M10-E5. Zero-Result and Coverage Diagnostics

Goal:

- make weak or empty investigation answers honest enough that agents can tell whether to trust them, retry with context, or escalate to raw inspection

Status:

- Completed

Implementation tasks:

- distinguish compact failure modes such as:
  - weak coverage
  - parse-fragile region
  - unsupported field shape
  - ambiguity-driven weak start
  - weak local evidence with no stitched continuation
- keep diagnostics short and actionable
- avoid narrating internal heuristics unless that changes the next user action

Validation checklist:

- weak answers do not look like confident negatives
- zero-result cases distinguish:
  - likely absent
  - likely weakly covered
  - likely ambiguous
  - likely unsupported
- successful answers are not drowned in diagnostics

Exit criteria:

- agents can usually decide whether to trust the result, retry with context, or inspect raw code next

Completion summary:

- weak, partial, and zero-result answers now carry enough compact diagnostics to guide trust, retry, or raw inspection decisions
- normal successful responses remain summary-first
- implementation-detail diagnostics that added noise without changing next actions were intentionally trimmed back

---

## 4. Delivery Rules

- finish earlier Epic exit criteria before spending much time on later-Epic polish
- prefer E2 substrate work over E3 and E4 polish when choosing between them
- do not add new response fields unless a current Epic is blocked without them
- keep MCP responses bounded:
  - bounded path length
  - bounded handoff count
  - bounded evidence count
  - bounded follow-up count
- add fixtures only when they prove a new capability family or close a real validation gap

---

## 5. Non-Goals

- full alias analysis
- compiler-complete slicing
- unrestricted natural-language planning
- replacing raw file inspection in all cases
- replacing large-scale structural search tools
- expanding MS10 into a general-purpose explanation engine

---

## 6. Practical Definition of Done

MS10 is done when:

- `M10-E1` gives a usable bounded investigation starting point
- `M10-E2` keeps common field/member flows alive through a few realistic boundaries
- `M10-E3` makes ambiguous startup less painful in real usage
- `M10-E4` gives enough bounded support for the next lookup choice
- `M10-E5` makes weak or empty answers honest and actionable

Anything beyond that should move to a later milestone instead of being polished further inside MS10.

Completion assessment:

- this definition is now satisfied
- further work in this area should be framed as follow-on refinement, not as unfinished MS10 scope

Operational validation:

- full local validation passed:
  - `indexer` Rust tests
  - `server` Jest tests
- operational full indexing completed on:
  - `E:\Dev\opencv`
  - `E:\Dev\llvm-project-llvmorg-18.1.8`
- representative MCP smoke checks succeeded against both real project databases:
  - `lookup_function`
  - `find_callers`
  - `trace_call_path`
  - `investigate_workflow`
  - `search_symbols`
