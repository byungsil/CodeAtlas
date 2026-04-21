# Milestone 15. Macro-Sensitive Coverage and Disambiguation

Status:

- Completed

## 1. Objective

Close the most visible investigation gaps that remain in macro-heavy gameplay C++ code after MS12.

This milestone focuses on:

- recovering caller and callee evidence when structurally fragile macro-sensitive symbols lose resolved edges
- making zero-result caller answers less misleading for known fragile symbols such as `SetShotFlags`
- completing enum-member value usage coverage for flag-style patterns
- reducing repeated ambiguity-resolution queries for extremely common method names such as `Update`, `Init`, and `Execute`

Success outcome:

- macro-sensitive symbols no longer stop at "coverageWarning only" when usable call evidence already exists inside the local index
- representative zero-result cases such as `find_callers(SetShotFlags)` become explainable and, where possible, recoverable
- enum flags used through bitwise composition and argument passing are queryable often enough to matter in gameplay code
- ambiguous short-name lookup requires fewer follow-up queries because the first response carries better narrowing help

Positioning note:

- MS10 made investigation workflows practical
- MS12 added coverage signaling, enum-member queryability, and `topCandidates`
- the remaining gap is not basic usability anymore; it is recovery quality in exactly the macro-heavy, flag-heavy, overload-heavy codebases that matter most for EA gameplay investigation

Scope note:

- this document intentionally excludes external-tool fallback such as `ast-grep`
- this milestone should improve CodeAtlas using its own stored structural data, ranking context, and indexing pipeline

---

## 2. Applicability Review

The incoming requirements cluster into four product problems.

Included in MS15:

1. macro-sensitive caller/callee recovery from local indexed evidence
2. zero-result diagnosis and recovery for representative missing-call cases such as `SetShotFlags`
3. enum flag value indexing completion for direct flag-style usage
4. ambiguity narrowing improvements beyond today's `topCandidates`

Explicitly not in scope for MS15:

- external-tool corroboration such as `ast-grep`
- full compiler-grade preprocessing or configuration-matrix indexing
- broad architecture changes to the query model
- speculative text-only call extraction with no stored provenance

Why this scope is right:

- the user-visible pain is real, but the fastest trustworthy path is to reuse data CodeAtlas already persists or can extract deterministically
- external fallback tools would complicate trust and deployment before the core substrate is exhausted
- full preprocessor-faithful indexing is too large for one milestone and should not block practical recovery work

---

## 3. Recommended Order

1. M15-E1. Fragile Coverage Contract and Recovery Surfaces
2. M15-E2. Stored-Call Recovery for Macro-Sensitive Caller/Callee Gaps
3. M15-E3. Enum Flag Usage Completion
4. M15-E4. Ambiguity Narrowing and Follow-Up Reduction
5. M15-E5. Gameplay Validation and Release Readiness

Why this order:

- the response contract must be clear before new fallback evidence is surfaced
- stored-call recovery is the highest-value fix because it addresses the most painful zero-result failures directly
- enum flag completion touches index extraction and should land after the recovery contract is stable
- ambiguity improvements should be informed by the new evidence surfaces instead of designed in isolation
- validation belongs last so acceptance reflects the final integrated behavior

Execution rule:

- finish each Epic to the point of measurable acceptance before starting broad polish on the next one
- allow small support changes across Epics only when an earlier Epic cannot be validated without them

---

## 4. Epic Breakdown

### M15-E1. Fragile Coverage Contract and Recovery Surfaces

Status:

- Completed

Goal:

- upgrade fragile zero-result responses from passive warnings into actionable recovery-oriented answers

Problem being solved:

- today `coverageWarning` correctly admits that a macro-sensitive symbol may be under-indexed, but it still leaves the user with no next step and no recovered evidence

Implementation tasks:

- define response-level distinction between:
  - fully resolved call edges
  - recovered but lower-confidence call evidence sourced from locally persisted raw call data
  - known-fragile zero-result answers with no recoverable evidence
- add compact provenance metadata for recovered call evidence so the user can tell why an edge exists
- define when fallback evidence is eligible:
  - prioritize symbols with `parseFragility = elevated`
  - prioritize symbols with `macroSensitivity = high`
  - allow representative named regression cases such as `SetShotFlags` to be covered by tests
- keep trust explicit:
  - do not present recovered evidence as fully resolved semantic certainty
  - do not hide the original fragility signal

Expected touch points:

- `server/src/models/responses.ts`
- `server/src/response-metadata.ts`
- `server/src/reliability.ts`
- `dev_docs/API_CONTRACT.md`

Acceptance:

- zero-result caller/callee answers can distinguish "no evidence found" from "resolved graph empty but fallback evidence exists"
- recovered edges include bounded provenance instead of pretending to be ordinary resolved edges
- reliability wording remains concise and agent-friendly

Exit criteria:

- fragile coverage responses become actionable rather than merely cautionary

Completion notes:

- caller and callgraph responses can now distinguish resolved edges from recovered raw-call evidence
- reliability metadata now supports `recoveredResultCount` and explicit fallback-oriented coverage messaging
- API contract and response models document the trust distinction instead of treating recovery as silent normal output

### M15-E2. Stored-Call Recovery for Macro-Sensitive Caller/Callee Gaps

Status:

- Completed

Goal:

- recover useful caller and callee answers from locally stored unresolved call-site evidence when resolved edges are missing

Problem being solved:

- representative gameplay symbols in macro-heavy files often lose semantic call edges even though raw call-site evidence was captured during indexing
- this is the most likely root cause behind cases like `find_callers(SetShotFlags) = 0` despite a real source call

Implementation tasks:

- audit the persisted `raw_calls` model as the primary recovery substrate
- add server-store accessors for bounded recovery queries against `raw_calls`
- implement name-to-symbol recovery heuristics that remain local and explicit:
  - short-name match to the requested callable
  - receiver or qualifier alignment when available
  - file/module/subsystem context preference when available
  - ambiguity-safe refusal when multiple candidates remain equally plausible
- add fallback behavior to:
  - `find_callers`
  - `get_callgraph(direction = callers | both)` where practical
  - selected workflow summaries when a primary step is missing only because resolved edges vanished
- keep fallback bounded:
  - cap recovered edge count
  - surface only the strongest candidates
  - mark truncated recovery explicitly if needed

Recommended implementation sequence:

1. expose read access to stored raw-call rows in the server store layer
2. define a compact recovered-call response shape
3. wire fallback into `find_callers`
4. extend bounded caller traversal only after direct caller recovery is stable
5. add regression fixtures for `SetShotFlags`-style misses

Expected touch points:

- `server/src/storage/store.ts`
- `server/src/storage/sqlite-store.ts`
- `server/src/storage/json-store.ts`
- `server/src/app.ts`
- `server/src/mcp-runtime.ts`
- `server/src/investigation-workflow.ts`

Acceptance:

- representative fragile symbols can return recovered callers when local raw-call evidence exists
- `find_callers(SetShotFlags)`-style fixtures no longer silently collapse to zero when the missing evidence is recoverable from stored data
- ambiguous fallback matches are withheld rather than overstated

Exit criteria:

- macro-sensitive call recovery works often enough to materially reduce false zero-result caller answers

Completion notes:

- the server store can read bounded `raw_calls` candidates from SQLite
- `find_callers` falls back to recovered caller evidence when resolved callers are empty for fragile symbols
- caller-direction callgraph traversal can now surface recovered upstream edges with provenance metadata
- `SetShotFlags`-style recovery is covered by regression tests

### M15-E3. Enum Flag Usage Completion

Status:

- Completed

Goal:

- make enum-member value usage reliable for flag-heavy gameplay code, especially direct bitwise composition and argument-passing patterns

Problem being solved:

- current enum-member indexing remains incomplete for patterns such as:
  - `SHOTFLAG_CHIP | SHOTFLAG_FINESSE`
  - direct flag constants passed as function arguments
  - mixed assignment and composition flows common in gameplay option masks

Implementation tasks:

- expand enum-member usage extraction coverage for:
  - bitwise OR / AND / XOR expressions
  - nested binary expressions
  - direct argument passing
  - return expressions that forward combined flags
  - initializer and assignment forms already used in gameplay code
- improve enum-member target resolution when multiple same-name values exist:
  - prefer local enum scope
  - use nearby type clues where available
  - use parameter and variable type context when available
  - refuse ambiguous ties rather than emitting misleading references
- verify that enum members remain first-class query targets in existing reference APIs
- ensure flag-style extraction does not duplicate the same usage row excessively for nested expressions

Expected touch points:

- `indexer/src/parser.rs`
- `indexer/src/models.rs`
- enum-member indexing tests
- server reference tests where response behavior depends on the new coverage

Acceptance:

- flag-style enum member usage appears in references for representative bitmask patterns
- direct argument usage of enum flags is queryable
- ambiguous same-name enum members still fail safely

Exit criteria:

- enum flag investigations no longer miss the most common value-style usage shapes

Completion notes:

- enum-member value resolution now uses contextual enum-type hints from qualified access, typed initializers, typed arguments, and typed returns
- flag-style bitwise composition such as `SHOTFLAG_CHIP | SHOTFLAG_FINESSE` is covered by parser regression tests
- same-name enum members still fail safely when context is insufficient

### M15-E4. Ambiguity Narrowing and Follow-Up Reduction

Status:

- Completed

Goal:

- reduce the number of extra queries needed to disambiguate extremely common method names in large EA-style codebases

Problem being solved:

- `topCandidates` helps, but names like `Update`, `Init`, and `Execute` still produce too many plausible options
- the user often needs one or more additional exact queries even when the first response already has enough local context to narrow further

Implementation tasks:

- enrich ambiguity responses with more decision-useful compact metadata, such as:
  - owning type
  - module or subsystem
  - artifact kind
  - nearby path context
  - caller/callee count hints when cheap and safe
- add first-response narrowing aids rather than only longer candidate lists:
  - stronger anchor-aware ranking explanations
  - compact "best next discriminator" hints
  - exact follow-up query suggestions for the top few candidates
- review whether `find_all_overloads` should expose additional grouping or filtering hints for class-heavy method names
- keep ambiguity bounded:
  - do not dump hundreds of overloads into first responses
  - do not trade clarity for exhaustiveness

Expected touch points:

- `server/src/response-metadata.ts`
- `server/src/models/responses.ts`
- `server/src/app.ts`
- `server/src/mcp-runtime.ts`
- ambiguity fixture tests

Acceptance:

- common ambiguous method-name lookups require fewer manual retries in representative investigation flows
- the first response better explains how to choose among the top candidates
- ambiguity responses remain compact enough for agent use

Exit criteria:

- ambiguity resolution feels guided rather than iterative guesswork

Completion notes:

- ambiguity responses now include `bestNextDiscriminator` and `suggestedExactQueries`
- `topCandidates` now expose compact narrowing metadata such as owner, artifact, module, subsystem, and an exact follow-up query
- HTTP and MCP ambiguity fixtures were updated so the new first-response narrowing hints stay covered

### M15-E5. Gameplay Validation and Release Readiness

Status:

- Completed

Goal:

- validate the milestone against the real class of code that motivated it: macro-heavy, flag-heavy gameplay C++

Problem being solved:

- MS15 is only successful if the fixes help on representative gameplay patterns rather than synthetic isolated fixtures alone

Implementation tasks:

- create a targeted regression set covering:
  - macro-sensitive caller misses
  - `SetShotFlags`-style recovered caller cases
  - bitwise enum-flag composition
  - direct enum-flag argument passing
  - ambiguity-heavy method-name lookup in class-dense gameplay code
- verify both HTTP and MCP surfaces where behavior changed
- update product-facing docs to explain:
  - when recovered call evidence appears
  - how it differs from fully resolved edges
  - what ambiguity narrowing metadata is intended to do
- record residual known limits that remain after MS15, especially around full preprocessor fidelity

Expected touch points:

- `server/src/__tests__/*`
- `indexer/src/*tests*`
- `dev_docs/API_CONTRACT.md`
- milestone completion notes

Acceptance:

- representative gameplay regressions are covered by repeatable tests
- new behavior is documented and does not weaken trust semantics
- remaining limitations are explicit rather than rediscovered ad hoc

Exit criteria:

- MS15 evidence is strong enough that the milestone can close without relying on anecdotal one-off checks

Completion notes:

- targeted regressions now cover fragile caller recovery, `SetShotFlags`-style misses, enum flag composition, enum flag argument passing, and ambiguity-heavy lookup flows
- API contract documentation now explains recovered-call semantics and ambiguity narrowing hints
- residual limits remain explicit: CodeAtlas still does not attempt full compiler-grade preprocessing fidelity
- real-project validation on `F:\dev\dev_future\client` confirms:
  - `/callers/SetShotFlags` returns `ShotNormal::CalcShotInformation` at `shotnormal.cpp:1634`
  - `Gameplay::ShotFlags::SHOTFLAG_CHIP` and `Gameplay::ShotFlags::SHOTFLAG_FINESSE` each expose `64` `enumValueUsage` references in the indexed project
  - ambiguity responses for `Update`, `Init`, and `Execute` keep returning narrowing hints and exact follow-up suggestions on live project data
- post-fix memory work stayed intentionally conservative:
  - `raw_calls`, non-call relation events, and callable summaries were preserved
  - only duplicate retention and oversized collection lifetime were reduced
  - diagnostic full rebuilds on `dev_future` showed private memory dropping from the prior `4.39-4.42 GiB` merge/resolve range to about `589 MiB` after merge symbols and `679 MiB` after resolve calls under the same diagnostic batch-size setup

Final validation summary:

- `indexer`
  - `cargo test` passed: `181` tests
  - `cargo build` passed
  - `cargo build --release` passed
- `server`
  - `npm test -- --runInBand` passed: `150` tests
  - `npm run build` passed
- `MCP`
  - HTTP and MCP-facing ambiguity and recovery fixtures remain covered in the server Jest suite
- real project smoke
  - `dev_future` full reindex completed successfully
  - recovered caller, enum flag references, and ambiguity narrowing all remained intact on live indexed data

---

## 5. Detailed Execution Plan

This section turns the milestone into an implementation sequence that can be executed without re-planning the work every time.

### Phase 1. Freeze The Response Contract

Primary outcome:

- the team agrees on how resolved edges, recovered edges, and unrecoverable fragile zero-result answers differ

Tasks:

1. define the recovered-call semantics in `dev_docs/API_CONTRACT.md`
2. decide the minimal response fields needed for recovered call evidence:
  - provenance kind
  - confidence tier
  - whether the result came from raw-call recovery rather than resolved graph edges
3. document when fallback is allowed and when it must refuse to guess
4. confirm current reliability wording still works once recovered evidence is introduced

Why first:

- server and test work will drift quickly if the contract is not pinned before implementation starts

Completion gate:

- one documented response contract exists for all later server and validation work

### Phase 2. Expose The Recovery Substrate

Primary outcome:

- the server can query persisted `raw_calls` deliberately instead of treating them as indexer-only implementation detail

Tasks:

1. audit `raw_calls` persistence fields and confirm which ones are strong enough for recovery ranking
2. add read APIs in the store layer for bounded raw-call lookup
3. keep store interfaces compact so fallback remains opt-in rather than infecting every query path
4. add storage-level tests for recovered raw-call reads where needed

Why second:

- fallback behavior cannot be implemented safely until the underlying evidence is available through a stable server abstraction

Completion gate:

- the server store can retrieve bounded candidate raw calls for one requested callable

### Phase 3. Land Direct Caller Recovery

Primary outcome:

- `find_callers` can recover useful caller evidence for fragile symbols when resolved edges are missing

Tasks:

1. add recovered-call response mapping and metadata formatting
2. wire fallback into `find_callers` only
3. rank raw-call candidates using local context:
  - name
  - qualifier or receiver
  - file/module/subsystem hints
4. refuse ambiguous ties instead of synthesizing low-trust edges
5. add the first regression fixture for `SetShotFlags`

Why third:

- this is the smallest end-to-end slice with direct user value

Completion gate:

- representative fragile caller misses can return bounded recovered callers through the normal caller surface

### Phase 4. Extend Recovery To Upstream Navigation

Primary outcome:

- caller recovery can be reused in bounded callgraph and workflow surfaces without creating inconsistent semantics

Tasks:

1. decide whether `get_callgraph(direction="callers")` should include recovered edges inline or in a separate section
2. reuse the caller fallback logic for bounded upstream traversal where confidence remains acceptable
3. update workflow summaries so a missing primary caller edge can still surface recovered evidence when appropriate
4. keep truncation and provenance explicit

Why fourth:

- direct caller recovery should prove itself before it is reused by more complex stitched responses

Completion gate:

- upstream navigation can incorporate recovered caller evidence without hiding uncertainty

### Phase 5. Complete Enum Flag Usage Coverage

Primary outcome:

- enum-member references cover the common gameplay flag patterns that currently feel broken

Tasks:

1. expand extraction to bitwise expressions and nested compositions
2. add direct function-argument and forwarded-return coverage
3. improve enum-member disambiguation using local type clues
4. deduplicate repeated nested-expression hits
5. add focused parser tests for `SHOTFLAG_*`-style patterns

Why fifth:

- enum extraction is mostly indexer work and can proceed cleanly once the fallback-call story is already stable

Completion gate:

- representative bitmask and argument-passing enum usages resolve as queryable enum-member references

### Phase 6. Reduce Ambiguity Follow-Up Cost

Primary outcome:

- the first response becomes better at helping users choose the right symbol among many same-name methods

Tasks:

1. decide the smallest extra candidate metadata that meaningfully helps selection
2. add compact narrowing aids such as:
  - owning type
  - path neighborhood
  - module/subsystem
  - suggested exact follow-up query
3. confirm `find_all_overloads` should stay bounded and grouped rather than becoming a raw overload dump
4. add ambiguity fixtures for `Update`, `Init`, and `Execute`-style collisions

Why sixth:

- ambiguity work should be informed by the new recovery behavior and should stay compact

Completion gate:

- ambiguity-heavy short-name queries require fewer retries in representative fixtures

### Phase 7. Validate On Gameplay-Style Regressions

Primary outcome:

- MS15 closes on repeatable evidence rather than isolated spot checks

Tasks:

1. run the targeted regression suite covering:
  - fragile caller recovery
  - `SetShotFlags`
  - enum flag composition
  - enum flag argument passing
  - ambiguity-heavy lookups
2. verify both HTTP and MCP responses where semantics changed
3. update docs for recovered-call semantics and residual limitations
4. capture final milestone notes once all exit gates are met

Why last:

- the point of the milestone is practical investigation improvement in gameplay-style code, so final validation must reflect that target

Completion gate:

- the milestone can be closed from testable evidence, not anecdotal validation

---

## 6. Task Breakdown By Epic

This section is intentionally operational. Each task should be small enough to assign, implement, review, and validate independently.

### M15-E1. Fragile Coverage Contract and Recovery Surfaces

#### M15-E1-T1. Define recovered-call response vocabulary

Deliverable:

- agreed naming for resolved vs recovered vs unrecoverable fragile zero-result answers

Dependencies:

- none

#### M15-E1-T2. Update API contract for recovered call evidence

Deliverable:

- documented response shape and trust semantics in `dev_docs/API_CONTRACT.md`

Dependencies:

- M15-E1-T1

#### M15-E1-T3. Add response model fields for recovered call metadata

Deliverable:

- `server/src/models/responses.ts` can express recovered call evidence compactly

Dependencies:

- M15-E1-T2

#### M15-E1-T4. Update reliability wording and helper builders

Deliverable:

- reliability helpers can describe fragile zero-result vs recovered results consistently

Dependencies:

- M15-E1-T3

#### M15-E1-T5. Add contract-level response tests

Deliverable:

- tests lock the new semantics before fallback logic expands

Dependencies:

- M15-E1-T4

### M15-E2. Stored-Call Recovery for Macro-Sensitive Caller/Callee Gaps

#### M15-E2-T1. Audit `raw_calls` fields for recovery usefulness

Deliverable:

- short note or code comments identifying which persisted fields participate in fallback ranking

Dependencies:

- M15-E1-T2

#### M15-E2-T2. Add store-layer raw-call read interface

Deliverable:

- server store can query bounded raw-call candidates for a requested callable name

Dependencies:

- M15-E2-T1

#### M15-E2-T3. Implement SQLite raw-call candidate query

Deliverable:

- `sqlite-store.ts` returns ranked or rankable raw-call candidates with caller symbol context

Dependencies:

- M15-E2-T2

#### M15-E2-T4. Implement JSON-store parity or explicit no-op behavior

Deliverable:

- fallback behavior is either supported consistently or intentionally unavailable with documented semantics

Dependencies:

- M15-E2-T2

#### M15-E2-T5. Build recovered-call ranking helper

Deliverable:

- one reusable ranking helper evaluates raw-call candidates using name, qualifier, and local context

Dependencies:

- M15-E2-T3

#### M15-E2-T6. Wire fallback into `find_callers`

Deliverable:

- `find_callers` returns recovered callers when resolved callers are empty and fallback evidence qualifies

Dependencies:

- M15-E2-T5

#### M15-E2-T7. Add `SetShotFlags` regression fixture

Deliverable:

- a repeatable test reproduces the missing-caller symptom and proves recovery

Dependencies:

- M15-E2-T6

#### M15-E2-T8. Extend fallback to bounded caller traversal

Deliverable:

- upstream traversal can reuse recovered callers without losing provenance

Dependencies:

- M15-E2-T6

#### M15-E2-T9. Extend workflow summaries where justified

Deliverable:

- investigation workflow can surface recovered upstream evidence in bounded form

Dependencies:

- M15-E2-T8

### M15-E3. Enum Flag Usage Completion

#### M15-E3-T1. Expand enum usage eligibility for bitwise expressions

Deliverable:

- parser recognizes enum-member usage inside bitwise flag composition

Dependencies:

- none

#### M15-E3-T2. Add nested-expression traversal and deduplication

Deliverable:

- nested flag expressions do not lose usages or emit excessive duplicates

Dependencies:

- M15-E3-T1

#### M15-E3-T3. Add direct argument-passing coverage

Deliverable:

- enum-member references survive direct function-argument usage

Dependencies:

- M15-E3-T1

#### M15-E3-T4. Add forwarded-return coverage

Deliverable:

- returned combined flags still generate enum-member references where appropriate

Dependencies:

- M15-E3-T1

#### M15-E3-T5. Improve enum-member target disambiguation

Deliverable:

- same-name enum members use local type clues when available and fail safely otherwise

Dependencies:

- M15-E3-T1

#### M15-E3-T6. Add gameplay-style enum regression fixtures

Deliverable:

- `SHOTFLAG_CHIP` and `SHOTFLAG_FINESSE`-style patterns are represented in tests

Dependencies:

- M15-E3-T2
- M15-E3-T3
- M15-E3-T4
- M15-E3-T5

### M15-E4. Ambiguity Narrowing and Follow-Up Reduction

#### M15-E4-T1. Choose minimal extra discriminator fields

Deliverable:

- a bounded metadata set is selected for ambiguity narrowing

Dependencies:

- M15-E1-T2

#### M15-E4-T2. Add richer top-candidate metadata

Deliverable:

- ambiguity responses expose extra context without becoming verbose

Dependencies:

- M15-E4-T1

#### M15-E4-T3. Add exact follow-up query hints

Deliverable:

- top candidates can suggest the next exact query to run

Dependencies:

- M15-E4-T2

#### M15-E4-T4. Review overload-grouping behavior

Deliverable:

- `find_all_overloads` remains useful for class-dense code without returning an unbounded wall of candidates

Dependencies:

- M15-E4-T2

#### M15-E4-T5. Add ambiguity-heavy fixture coverage

Deliverable:

- repeated-name methods such as `Update`, `Init`, and `Execute` are covered by regression tests

Dependencies:

- M15-E4-T3
- M15-E4-T4

### M15-E5. Gameplay Validation and Release Readiness

#### M15-E5-T1. Build targeted regression matrix

Deliverable:

- one checklist or fixture inventory covers all MS15 representative scenarios

Dependencies:

- M15-E2-T7
- M15-E3-T6
- M15-E4-T5

#### M15-E5-T2. Verify HTTP response behavior

Deliverable:

- HTTP endpoints preserve documented semantics for recovered calls and ambiguity hints

Dependencies:

- M15-E5-T1

#### M15-E5-T3. Verify MCP response behavior

Deliverable:

- MCP tools preserve the same semantics as HTTP where applicable

Dependencies:

- M15-E5-T1

#### M15-E5-T4. Update documentation and residual limits

Deliverable:

- docs explain what MS15 fixed and what remains outside scope

Dependencies:

- M15-E5-T2
- M15-E5-T3

#### M15-E5-T5. Capture completion notes and close-out evidence

Deliverable:

- milestone completion can be reviewed from a single evidence set

Dependencies:

- M15-E5-T4

---

## 7. Task Breakdown By File

This section maps the milestone to likely edit locations so implementation can start without another discovery pass.

### `dev_docs/API_CONTRACT.md`

Planned tasks:

- document recovered-call semantics
- document trust distinctions for resolved vs recovered edges
- document any new ambiguity narrowing metadata

### `dev_docs/Milestone15_MacroSensitiveCoverageAndDisambiguation.md`

Planned tasks:

- keep execution progress, task status, and completion notes aligned with actual implementation

### `server/src/models/responses.ts`

Planned tasks:

- add recovered-call metadata fields
- extend ambiguity response metadata only as far as needed for narrowing

### `server/src/response-metadata.ts`

Planned tasks:

- format recovered-call provenance compactly
- add ambiguity discriminators and exact follow-up hints
- preserve bounded output shape

### `server/src/reliability.ts`

Planned tasks:

- distinguish fragile zero-result responses from recovered-result responses

### `server/src/storage/store.ts`

Planned tasks:

- add bounded raw-call lookup interface for recovery

### `server/src/storage/sqlite-store.ts`

Planned tasks:

- implement raw-call lookup query
- join caller symbol context as needed for ranking
- keep fallback query bounded and testable

### `server/src/storage/json-store.ts`

Planned tasks:

- implement compatible fallback behavior or explicit unsupported behavior for raw-call recovery

### `server/src/app.ts`

Planned tasks:

- wire recovered-call fallback into HTTP caller surfaces
- expose any new ambiguity narrowing fields in endpoint responses

### `server/src/mcp-runtime.ts`

Planned tasks:

- wire recovered-call fallback into MCP caller surfaces
- keep MCP semantics aligned with HTTP

### `server/src/investigation-workflow.ts`

Planned tasks:

- incorporate recovered caller evidence only where it improves bounded workflow answers

### `server/src/__tests__/`

Planned tasks:

- add regression coverage for:
  - fragile recovered callers
  - `SetShotFlags`
  - ambiguity-heavy method names
  - HTTP/MCP parity

### `indexer/src/parser.rs`

Planned tasks:

- expand enum-member usage extraction for bitwise and argument patterns
- improve enum-member target disambiguation
- deduplicate nested-expression hits
- add focused parser fixtures

### `indexer/src/models.rs`

Planned tasks:

- update models only if enum-member usage metadata or recovery-adjacent structures require it

### `indexer/src/storage.rs`

Planned tasks:

- verify persisted `raw_calls` fields are sufficient and stable for server-side recovery use

---

## 8. Cross-Epic Risks

### Risk 1. False confidence from fallback call recovery

Why it matters:

- recovered raw-call evidence is useful only if users can still distinguish it from fully resolved call edges

Mitigation:

- keep provenance explicit
- preserve fragility signals
- prefer omission over ambiguous overclaim

### Risk 2. Enum flag extraction over-generates noisy references

Why it matters:

- flag expressions are dense and nested; naive extraction can duplicate or misattribute usages quickly

Mitigation:

- deduplicate nested expression hits
- keep ambiguity-safe resolution rules
- validate on representative bitmask fixtures instead of only toy enums

### Risk 3. Ambiguity responses become larger without becoming more useful

Why it matters:

- adding more candidate metadata can increase token cost while still not helping selection

Mitigation:

- prefer discriminators and exact next-step hints over larger candidate dumps
- keep response additions compact and tested against real ambiguous names

---

## 9. Definition of Done

MS15 is complete when:

1. fragile macro-sensitive symbols can recover caller or callee evidence from local stored data when that evidence exists
2. representative missing-call cases such as `SetShotFlags` are covered by repeatable regression tests
3. enum-member value usage covers common flag-style patterns used in gameplay code
4. ambiguous lookup responses reduce follow-up query count in representative method-name collisions
5. API and reliability semantics remain honest about what is resolved, recovered, and still unknown

Validation snapshot:

- `server`: `npm test -- --runInBand` passed
- `server`: `npm run build` passed
- `indexer`: `cargo test` passed

---

## 10. Suggested First Implementation Slice

Start with the smallest slice that proves the milestone is worth doing:

1. add recovered-call response semantics for fragile symbols
2. wire `find_callers` fallback from stored `raw_calls`
3. land a regression fixture for `SetShotFlags`

Why this slice first:

- it attacks the most visible user pain immediately
- it validates the recovery model before broader enum and ambiguity work
- it gives the milestone a concrete proof point early
