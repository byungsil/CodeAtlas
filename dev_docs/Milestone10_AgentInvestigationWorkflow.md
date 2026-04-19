# Milestone 10. Agent Investigation Workflow

Status:

- Proposed

## 1. Objective

Improve CodeAtlas from a strong graph-backed lookup system into a more complete investigation workflow system for AI agents working on very large C++ repositories.

This milestone focuses on the highest-priority gaps still visible after Milestone 5, Milestone 6, and Milestone 9:

- source-to-sink investigation path queries
- field- and member-centric propagation quality
- context-aware symbol disambiguation
- hybrid semantic plus structural evidence gathering
- honest diagnostics for weak or incomplete answers

Success outcome:

- when an agent asks questions such as:
  - "how do we get from user input to shot launch?"
  - "where does this flag get set and where is it consumed?"
  - "why did lookup return no useful path or no references?"
- CodeAtlas can answer in a compact, investigation-oriented form without forcing the agent to manually stitch together many separate queries or fall back too early to raw file scanning

Positioning note:

- Milestone 5 added hierarchy, call-path tracing, and metadata-aware filtering
- Milestone 6 added bounded propagation vocabulary and extraction
- Milestone 9 improved representative anchor quality
- this milestone is about combining those capabilities into a practical agent workflow for real-world code archaeology
- the core problem is no longer basic graph availability
- the core problem is investigation friction:
  - too many separate queries
  - too much manual stitching
  - too much ambiguity in short-name lookups
  - too little clarity when the index is weak or incomplete in a region

Why this matters now:

- agents are increasingly using CodeAtlas to answer process and flow questions, not only symbol lookup questions
- real investigation tasks often start with an imprecise user question and need:
  - a likely path
  - a small number of strong anchors
  - structural evidence around those anchors
  - clear warning when the returned answer is incomplete or heuristic
- giant game repositories amplify the cost of missing any of those pieces

---

## 2. Recommended Order

1. M10-E1. Investigation path queries and workflow summaries
2. M10-E2. Field- and member-centric propagation strengthening
3. M10-E3. Context-aware symbol disambiguation and ranking
4. M10-E4. Hybrid semantic plus structural evidence expansion
5. M10-E5. Zero-result and coverage diagnostics

---

## 3. Priority Rationale

### 3.1 Why M10-E1 is first

- the highest-value missing capability is answering source-to-sink investigation questions directly
- current call-path and lookup features are individually useful but still require manual composition
- agents most often ask workflow questions, not isolated relation questions

### 3.2 Why M10-E2 is second

- method-level propagation is already useful, but field and member state still break many real-world investigations
- game codebases often move meaning through members, event payloads, hint objects, and param structs
- weak field propagation causes major blind spots in practical data-flow work

### 3.3 Why M10-E3 is third

- short-name ambiguity wastes investigation time before analysis even begins
- better disambiguation becomes more important as CodeAtlas gains more workflow features
- path quality, subsystem context, and recent investigation anchors should shape ranking more strongly

### 3.4 Why M10-E4 is fourth

- purely semantic queries still miss important nearby structural evidence
- hybrid expansion should build on stronger path and propagation foundations rather than precede them

### 3.5 Why M10-E5 is fifth

- diagnostics are essential for trust, but they are most useful after the main answer surfaces are improved
- the system should first answer more questions directly, then explain remaining gaps more precisely

---

## 4. Epics

### M10-E1. Investigation Path Queries and Workflow Summaries

Goal:

- let agents ask practical investigation questions in source-to-sink form and receive a compact, stitched summary instead of many disconnected query fragments

Priority:

- Highest

Implementation tasks:

- add investigation-oriented path queries that can start from:
  - a symbol
  - a field
  - a file anchor
  - a short workflow description with bounded symbol expansion
- support queries such as:
  - source symbol to sink symbol
  - source symbol to sink category
  - start symbol to likely launch or output action
- return bounded path summaries that combine:
  - representative anchors
  - key intermediate symbols
  - handoff kinds such as call, assignment, argument-to-parameter, field write, field read
- add workflow summary output shaped for agents:
  - "entry"
  - "main path"
  - "handoff points"
  - "launch or sink point"
  - "uncertain segments"

Expected touch points:

- `dev_docs/API_CONTRACT.md`
- `server/src/mcp.ts`
- `server/src/storage/sqlite-store.ts`
- `server/src/models/responses.ts`

Validation checklist:

- large-project fixtures can answer questions of the form:
  - "how do we get from A to B?"
  - "how does user input reach launch code?"
- the returned path is bounded, deterministic, and does not dump raw graph noise
- workflow summaries remain useful even when part of the path is only partial-confidence

Exit criteria:

- CodeAtlas can answer end-to-end investigation questions directly enough that agents no longer need to manually compose caller, callee, and propagation queries for the common case

---

### M10-E2. Field- and Member-Centric Propagation Strengthening

Goal:

- make field and member propagation reliable enough for real gameplay-style data tracing where values move through param structs, hint objects, event payloads, and cached state

Priority:

- High

Implementation tasks:

- strengthen supported field propagation patterns for:
  - `param.field = member`
  - `member = param.field`
  - constructor-style field initialization from arguments
  - event or hint object creation carrying field values
  - chained handoffs through small state carrier structs
- add better anchor continuity for field-copy chains across:
  - local variable
  - parameter
  - member
  - temporary object
- expose field-centric query modes such as:
  - "where is this member written?"
  - "where is this member copied next?"
  - "what are the likely downstream readers of this field?"
- keep pointer-heavy and alias-heavy paths explicitly partial instead of overstating certainty

Expected touch points:

- `dev_docs/API_CONTRACT.md`
- `indexer/src/parser.rs`
- `indexer/src/models.rs`
- `indexer/src/storage.rs`
- `server/src/models/responses.ts`

Validation checklist:

- fixtures cover:
  - param-to-member copies
  - member-to-param copies
  - event-style object construction
  - hint-style object construction
  - multi-step field propagation through small carrier structs
- weak pointer-heavy cases remain explicitly partial

Exit criteria:

- field-state investigations no longer collapse after the first member write or first object handoff in common C++ patterns

---

### M10-E3. Context-Aware Symbol Disambiguation and Ranking

Goal:

- reduce investigation startup friction by ranking ambiguous symbol candidates using active context instead of name similarity alone

Priority:

- High

Implementation tasks:

- improve heuristic lookup ranking using:
  - current workspace path context
  - current or recent file anchors
  - subsystem and module metadata
  - artifact kind
  - recently visited representative anchors
- add bounded ambiguity presentation that shows the top plausible candidates with reasons such as:
  - same subsystem
  - same path neighborhood
  - runtime artifact preferred
  - recent investigation anchor proximity
- let MCP responses distinguish:
  - a likely intended candidate
  - alternate plausible candidates
  - cases where the system should ask the agent to choose

Expected touch points:

- `dev_docs/API_CONTRACT.md`
- `server/src/models/responses.ts`
- `server/src/mcp-runtime.ts`
- `server/src/storage/sqlite-store.ts`

Validation checklist:

- short-name lookups such as `UpdateShot`, `HintToAction`, or `SetShotFlags` rank the most contextually relevant production symbol first in real repositories
- ambiguity results remain compact and explainable

Exit criteria:

- agents can start real investigations from short or partial symbol names with substantially less manual disambiguation

---

### M10-E4. Hybrid Semantic Plus Structural Evidence Expansion

Goal:

- augment semantic answers with a small amount of nearby structural evidence so agents do not need to leave CodeAtlas immediately for separate AST or grep tools

Priority:

- Medium

Implementation tasks:

- add bounded structural expansion around semantic anchors for patterns such as:
  - constructor or hint creation
  - request object creation
  - field assignment
  - bit-flag checks
  - enum comparisons
- define a compact evidence payload that can attach to a semantic answer without overwhelming it
- support investigation-oriented expansion modes such as:
  - "show nearby assignment evidence"
  - "show nearby request construction"
  - "show nearby flag checks"
- keep expansion bounded by line window, symbol body, or match count

Expected touch points:

- `dev_docs/API_CONTRACT.md`
- `server/src/mcp.ts`
- `server/src/models/responses.ts`
- indexer or server-side lightweight pattern extraction surface

Validation checklist:

- semantic answers can include nearby structural evidence without requiring raw-file fallback in common investigation tasks
- responses stay bounded and summary-first

Exit criteria:

- CodeAtlas can provide enough adjacent evidence to validate a semantic hypothesis without immediately requiring a second tool for simple structural confirmation

---

### M10-E5. Zero-Result and Coverage Diagnostics

Goal:

- make it clear whether an empty or weak answer means "no relation exists" or "the index is currently weak here"

Priority:

- Medium

Implementation tasks:

- add explicit response diagnostics for:
  - zero references with weak coverage suspicion
  - parse-fragile regions
  - macro-heavy regions
  - field-propagation incompleteness
  - ambiguous representative clusters
- separate query-result confidence from coverage confidence more clearly
- add bounded diagnostic hints such as:
  - likely unsupported shape
  - likely parser fragility
  - likely macro sensitivity
  - likely name-resolution ambiguity
- expose diagnostic summaries without turning every response into a debugging payload

Expected touch points:

- `dev_docs/API_CONTRACT.md`
- `server/src/models/responses.ts`
- `server/src/mcp-runtime.ts`
- indexer metadata or resolver confidence surfaces

Validation checklist:

- an empty result can distinguish:
  - structurally absent
  - unresolved
  - partial coverage
  - ambiguous target selection
- agents can decide more intelligently when to trust the empty answer and when to escalate to raw inspection

Exit criteria:

- weak answers and zero results are more trustworthy because the system explains their likely failure mode compactly and honestly

---

## 5. Recommended Delivery Notes

- M10-E1 and M10-E2 should be designed together even if implemented sequentially
- M10-E1 depends on stronger path composition across call and propagation edges
- M10-E2 should preserve Milestone 6 honesty principles and avoid pretending to solve alias analysis
- M10-E3 should reuse Milestone 9 representative metadata rather than introducing a second, unrelated ranking vocabulary
- M10-E4 should stay bounded and evidence-oriented, not become a raw-source dumping feature
- M10-E5 should improve trust without making normal responses noisy

---

## 6. Non-Goals

- compiler-complete program slicing
- full alias or pointer analysis
- unrestricted natural-language planning over arbitrary repository text
- replacing dedicated structural search tools for large-scale pattern sweeps
- replacing raw file reading in all cases

---

## 7. Expected Outcome

If this milestone is completed in the recommended order, CodeAtlas should become substantially better at the exact class of questions AI agents ask most often in large C++ repositories:

- process and workflow reconstruction
- flag and state propagation tracing
- entry-point selection under ambiguity
- investigation under incomplete or noisy indexing conditions

In practical terms, this milestone should reduce the need for agents to leave CodeAtlas for supplementary tools during the first phase of a real investigation, while still preserving honest confidence and bounded responses.