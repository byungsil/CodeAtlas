# Milestone 2. Tree-Sitter Graph Integration

Status:

- Completed

## 1. Objective

Introduce `tree-sitter-graph` into the Rust indexing pipeline so CodeAtlas can extract structural relationship events in a more explicit, extensible, and testable way.

This milestone focuses on:

- integrating `tree-sitter-graph` into the existing Rust indexer
- defining graph-derived relationship event contracts
- preserving current call-resolution behavior while changing the extraction layer
- preparing the pipeline for richer relations beyond direct calls
- adding fixtures and regression coverage for graph-based extraction

Success outcome:

- CodeAtlas can derive stable relationship events from `tree-sitter-graph` without regressing current call-resolution quality

---

## 2. Recommended Order

1. M2-E1. Graph integration design
2. M2-E2. Graph event model
3. M2-E3. Initial call-edge graph rules
4. M2-E4. Parser pipeline integration
5. M2-E5. Resolver compatibility and parity validation
6. M2-E6. Additional structural relation rules
7. M2-E7. Fixture and regression hardening

---

## 3. Epics

### M2-E1. Graph Integration Design

Goal:

- decide how `tree-sitter-graph` fits into the current parser, indexing, and resolver boundaries

Design decisions:

- `tree-sitter` remains the source parser for all C++ files.
- handwritten Rust traversal remains responsible for symbol extraction during the first graph-integration pass.
- `tree-sitter-graph` is introduced first as a structural relation extractor, not as a replacement for the full parser pipeline.
- graph-derived relation events must be normalized into a resolver-compatible intermediate shape before scoring and disambiguation.
- graph extraction must be allowed to fail partially without aborting whole-file indexing.

Implementation tasks:

- define the target extraction pipeline:
  - `tree-sitter` parse
  - `tree-sitter-graph` rule execution
  - normalized relation events
  - resolver scoring and disambiguation
- decide which responsibilities stay in handwritten Rust traversal versus graph rules
- decide where graph rule files live and how they are versioned
- document failure handling and fallback behavior when graph extraction is incomplete

Boundary contract:

- `parser.rs`
  - owns tree creation, symbol extraction, graph execution, and normalization into raw relation events
- graph rule files under `indexer/`
  - own structural matching for supported relation shapes
- `resolver.rs`
  - continues to own candidate scoring, ambiguity handling, and final call-edge resolution
- `indexing.rs`
  - remains unaware of graph-rule details and only consumes normalized parser output

First-pass scope:

- in scope for graph extraction:
  - unqualified calls
  - namespace-qualified calls
  - `obj.method()`
  - `ptr->method()`
  - `this->method()`
- out of scope until later epics:
  - overload semantics beyond current resolver behavior
  - template-specific semantic recovery
  - macro-sensitive semantic interpretation
  - type usage and inheritance storage

Rule-file location and versioning:

- graph rule files should live under `indexer/graph/`
- the first milestone should use repository-tracked rule files rather than generated assets
- rule changes should be tested through parser fixtures in the same way Rust parser changes are tested

Failure and fallback policy:

- if graph extraction succeeds, normalized graph events become the preferred raw relation input for supported shapes
- if graph extraction is unavailable, incomplete, or fails for a file, call extraction falls back to the current handwritten AST traversal
- symbol extraction never depends on graph success
- unsupported shapes should be omitted deliberately rather than guessed silently by partially matched graph output

Expected touch points:

- `indexer/Cargo.toml`
- `indexer/src/parser.rs`
- `indexer/src/indexing.rs`
- milestone planning documents under `dev_docs/`

Validation checklist:

- the parser/resolver boundary is documented before implementation expands

Exit criteria:

- there is a clear architecture for introducing `tree-sitter-graph` without destabilizing existing indexing

---

### M2-E2. Graph Event Model

Goal:

- define the normalized event shape that graph rules should emit

Model decisions:

- graph extraction emits normalized `RawRelationEvent` records before resolver-specific call resolution runs
- `RawRelationEvent` becomes the extraction contract for both legacy AST-based extraction and future `tree-sitter-graph` extraction
- `RawCallSite` remains as the resolver-facing compatibility model for call resolution during the transition period
- only `RawRelationKind::Call` events are mapped into `RawCallSite` in the first pass
- future relation kinds can reuse the same event contract without forcing immediate storage changes

First-release event contract:

- `relation_kind`
  - `call`
  - reserved now for later expansion:
    - `type_usage`
    - `inheritance`
- `source`
  - `legacy_ast`
  - `tree_sitter_graph`
- `confidence`
  - `high`
  - `partial`
- optional structural hints for call events:
  - `caller_id`
  - `target_name`
  - `call_kind`
  - `argument_count`
  - `receiver`
  - `receiver_kind`
  - `qualifier`
  - `qualifier_kind`
- always-required location fields:
  - `file_path`
  - `line`

Mapping rules:

- `RawRelationEvent` with `relation_kind = call` maps into `RawCallSite`
- `target_name` maps to `called_name`
- `source` and `confidence` stay on the normalized event layer and do not change resolver scoring yet
- unsupported future relation kinds must not be forced through `RawCallSite`

Implementation tasks:

- define first-release graph event kinds:
  - direct call
  - qualified call
  - member access call
  - pointer member access call
  - optional type usage
  - optional inheritance mention
- decide how graph events map to current `RawCallSite` fields and future generalized relation models
- add enough metadata to preserve:
  - caller identity
  - candidate callee name
  - qualifier or receiver hints
  - file path
  - line
  - extraction confidence or source

Expected touch points:

- `indexer/src/models.rs`
- `dev_docs/API_CONTRACT.md`

Validation checklist:

- graph-derived event fields are explicit and deterministic

Exit criteria:

- there is a stable event contract between graph extraction and resolver logic

---

### M2-E3. Initial Call-Edge Graph Rules

Goal:

- replace the most repetitive call-site extraction logic with graph rules first

Implementation tasks:

- add `tree-sitter-graph` rules for:
  - unqualified calls
  - namespace-qualified calls
  - `obj.method()`
  - `ptr->method()`
  - `this->method()`
- keep unsupported patterns clearly out of scope until validated
- record graph output in a form that can be inspected during fixture tests

Expected touch points:

- graph rule files under `indexer/`
- `indexer/src/parser.rs`

Validation checklist:

- graph rules emit the expected raw events on representative fixtures

Exit criteria:

- common C++ call-site shapes are emitted through graph rules instead of only manual AST traversal

---

### M2-E4. Parser Pipeline Integration

Goal:

- run graph extraction as part of normal parsing without breaking current indexing flow

Implementation tasks:

- add the `tree-sitter-graph` dependency and loading path
- integrate graph execution into `parse_cpp_file`
- normalize graph outputs into the parser result structure
- keep parsing tolerant of incomplete graph coverage so one missing rule does not abort the whole file

Expected touch points:

- `indexer/Cargo.toml`
- `indexer/src/parser.rs`
- `indexer/src/indexing.rs`

Validation checklist:

- normal indexing still completes when graph coverage is partial

Exit criteria:

- graph extraction runs in the standard indexing pipeline

---

### M2-E5. Resolver Compatibility and Parity Validation

Goal:

- ensure the resolver keeps or improves current behavior on top of graph-derived inputs

Current parity policy:

- graph-derived call events are compared against legacy AST-derived call events before they are allowed to replace resolver input for a file
- the first acceptance bar is structural parity on existing ambiguity fixtures, not broader semantic improvement claims
- when graph extraction diverges from the legacy shape set, parser output falls back to legacy raw calls for that file
- parity must be evaluated on representative fixtures for:
  - namespace-qualified calls
  - sibling methods
  - overload-oriented ambiguity
  - `this` and pointer receivers

Implementation tasks:

- adapt resolver inputs if needed so graph events can feed existing ranking logic
- compare legacy parser-produced call events with graph-produced call events on fixtures
- define acceptable parity gaps versus intentional improvements
- keep ambiguity and unresolved behavior explicit

Expected touch points:

- `indexer/src/resolver.rs`
- `indexer/src/parser.rs`
- `samples/ambiguity`

Validation checklist:

- graph-backed call resolution matches or improves current fixture behavior

Exit criteria:

- resolver quality is preserved across the extraction-layer transition

---

### M2-E6. Additional Structural Relation Rules

Goal:

- expand beyond direct calls once the basic integration is stable

Current scope decision:

- Milestone 2 extends `relation_events` with additional graph-derived relation kinds before changing persisted storage
- `type_usage` and `inheritance` are valid normalized extraction outputs in this milestone
- the `calls` table remains call-only for now
- promotion of non-call relations into first-class persisted storage is intentionally deferred to the next reference-focused milestone
- the acceptance bar for this epic is stable extraction and fixture coverage, not full query-surface exposure yet

Implementation tasks:

- evaluate graph rules for:
  - type usage
  - class instantiation
  - inheritance
  - member-access mentions where useful
- decide which of these become first-class stored relations now versus later milestones
- avoid inflating the stored model with weak or noisy relations

Expected touch points:

- graph rule files under `indexer/`
- `indexer/src/models.rs`
- `indexer/src/storage.rs`

Validation checklist:

- each new relation kind is backed by a deliberate fixture and storage decision

Exit criteria:

- CodeAtlas has a clear path from call-only extraction to richer structural relations

---

### M2-E7. Fixture and Regression Hardening

Goal:

- make graph extraction safe to evolve over time

Current hardening notes:

- ambiguity fixtures are the primary parity baseline for graph-backed call extraction
- regression coverage must include both:
  - supported shapes that should prefer graph-backed relation events
  - unsupported shapes that must fall back safely to legacy AST-derived call extraction
- parser tolerance remains a hard requirement for template-heavy and macro-bearing files even when graph extraction is partial

Known unsupported or intentionally deferred constructs in Milestone 2:

- member calls whose receiver is a complex expression such as `MakeWorker().Update()`
- semantic overload resolution beyond the existing resolver heuristics
- template-dependent or macro-sensitive semantic interpretation beyond structural extraction
- persistence and query-surface exposure for non-call relations such as `type_usage` and `inheritance`

Handoff to Milestone 3:

- remaining graph call-coverage expansion now continues in `Milestone 3`, starting with `M3-E2`
- persistence and query-surface exposure for `type_usage` and `inheritance` now continue in `Milestone 3` through:
  - `M3-E3. Reference model definition`
  - `M3-E4. Reference extraction`
  - `M3-E5. Reference storage and retrieval`

Implementation tasks:

- add fixture coverage for:
  - namespaces
  - sibling methods
  - overloaded names
  - pointer and `this` receivers
  - declaration/definition splits
  - templates and macro-bearing files that should remain parser-tolerant
- add regression tests that compare graph-derived extraction against expected normalized events
- document known unsupported constructs explicitly

Expected touch points:

- `samples/`
- `indexer/src/parser.rs`
- `indexer/src/resolver.rs`

Validation checklist:

- graph extraction behavior is locked by focused regression fixtures

Exit criteria:

- the new extraction layer is testable enough to support follow-on milestones safely

---

## 4. Final Exit Criteria

- `tree-sitter-graph` is integrated into the Rust indexer
- graph-derived relationship events feed the existing resolver pipeline
- call-site extraction parity is maintained or improved on current fixtures
- the project is ready to build caller, reference, and impact features on top of the new relation layer

## 5. Real-Project Validation Status

OpenCV validation confirmed that the Milestone 2 extraction layer scales to a large real-world C++ repository:

- full rebuild completed on `E:\Dev\opencv`
- output size reached `50526` symbols and `84298` call edges across `3695` files
- sampled queries such as `cv::imread`, `cv::VideoCapture::open`, and `cv::resize` returned usable symbol and call data

However, this validation also exposed an unresolved storage handoff issue:

- the generated SQLite database can be queried successfully after copying the file
- the original `index.db` is not yet reliably reopenable from a fresh process

This means Milestone 2 is complete as an extraction milestone, but not yet perfect as an end-to-end durability milestone. The reopenability issue should now be treated as follow-on work in the next milestone alongside the remaining graph-coverage and reference-surface tasks.

Completion summary:

- `tree-sitter-graph` is integrated into the Rust indexer
- graph-derived relation events are normalized and tested
- call extraction parity and fallback behavior are established
- additional non-call structural relations are extracted into the intermediate event layer
- full `indexer` and `server` test suites pass
- large-project validation was completed on `OpenCV`
- operational hardening now includes retry/backoff, snapshot fallback, and Windows-specific operating guidance
