# Milestone 9. Representative Symbol Quality

Status:

- Completed

## 1. Objective

Improve how CodeAtlas chooses the canonical or representative anchor for a logical symbol in very large monorepo-scale repositories.

This milestone focuses on:

- choosing more useful representative file and line anchors
- preferring production and canonical definitions over noisy duplicates
- reducing test-shadowed or fixture-shadowed representative selection
- making exact lookup results more agent-usable on giant repositories such as LLVM and Unreal-engine-class game projects
- surfacing representative-quality uncertainty honestly when one canonical anchor cannot be chosen with confidence

Success outcome:

- when an agent asks for a symbol, the first returned anchor is much more likely to be the file and location a human engineer would consider the canonical place to start

Positioning note:

- this milestone is about symbol-quality and agent usability, not parser correctness
- the core problem is not "can CodeAtlas find the symbol at all?"
- the problem is "which location should CodeAtlas present as the representative face of that symbol?"
- this becomes especially important in giant monorepos where:
  - declaration and definition are split across many files
  - test trees duplicate production APIs
  - inline, generated, fixture, and compatibility layers all coexist

Why this matters:

- agents usually continue reasoning from the first anchor returned
- poor representative selection wastes tokens and attention on:
  - tests
  - compatibility shims
  - secondary declarations
  - non-canonical duplicate locations
- LLVM validation already exposed this issue for symbols such as `llvm::StringRef`
- Unreal-engine-class repositories are likely to magnify the same problem

---

## 2. Recommended Order

1. M9-E1. Representative-selection model and ranking policy
2. M9-E2. Declaration and definition preference refinement
3. M9-E3. Path- and artifact-aware canonicality scoring
4. M9-E4. Duplicate-cluster and ambiguity surfacing
5. M9-E5. Real-project regression and evaluation suite
6. M9-E6. Optional repository-tunable representative rules

---

## 3. Epics

### M9-E1. Representative-Selection Model and Ranking Policy

Goal:

- define exactly how CodeAtlas decides which symbol location becomes the representative anchor

Status:

- Completed

Implementation tasks:

- document the representative-selection problem formally
- define ranking inputs such as:
  - definition vs declaration
  - path quality
  - artifact kind
  - header role
  - test/generated/editor/tool/runtime classification
  - symbol role and scope context
- define a first-release representative scoring taxonomy
- define how representative confidence should be surfaced

Expected touch points:

- `dev_docs/API_CONTRACT.md`
- `indexer/src/models.rs`
- `server/src/models/responses.ts`

Validation checklist:

- ranking policy is written down before heuristics expand
- the policy distinguishes canonical, acceptable, and weak representative anchors

Exit criteria:

- CodeAtlas has an explicit representative-selection contract instead of relying on incidental merge order

Completion summary:

- representative-selection policy is now documented in `dev_docs/API_CONTRACT.md`
- the contract now distinguishes:
  - lookup confidence
  - representative-anchor confidence
- first-release representative ranking inputs are now fixed in writing:
  - declaration vs definition
  - out-of-line vs inline definition
  - artifact kind
  - header role
  - path quality
  - scope quality
- shared vocabulary for representative confidence and representative selection reasons is now present in:
  - `indexer/src/models.rs`
  - `server/src/models/responses.ts`
- this creates a stable vocabulary for later implementation work in `M9-E2` and beyond without changing runtime behavior yet

---

### M9-E2. Declaration and Definition Preference Refinement

Goal:

- improve representative choice when the same logical symbol has both declarations and definitions

Status:

- Completed

Implementation tasks:

- refine symbol merge logic so representative choice evaluates:
  - out-of-line definition
  - inline definition
  - declaration-only anchor
  - header-only implementation
- prefer the most useful anchor for agent workflows, not merely the first merged record
- preserve declaration and definition metadata independently even when one becomes representative
- ensure representative selection remains stable under incremental reindexing

Expected touch points:

- `indexer/src/resolver.rs`
- `indexer/src/models.rs`
- `indexer/src/storage.rs`

Validation checklist:

- fixtures cover:
  - declaration in header plus definition in source
  - header-only inline implementation
  - duplicated declaration-rich symbols

Exit criteria:

- representative symbol selection respects declaration/definition structure instead of merge incidental order

Completion summary:

- representative merge logic now decides structural preference before dual-location metadata is folded into the surviving anchor
- first-release structural order is now implemented as:
  - out-of-line definition
  - inline or header-only definition
  - declaration-only fallback
- representative choice is now deterministic even when definition and declaration variants arrive in different raw-merge order
- declaration and definition metadata remain preserved independently on the merged logical symbol
- regression coverage now includes:
  - definition-before-declaration merge order
  - inline-definition vs declaration-only preference
  - header fallback when the source-side variant is removed

---

### M9-E3. Path- and Artifact-Aware Canonicality Scoring

Goal:

- teach representative selection to prefer the file path a human would most likely consider canonical

Status:

- Completed

Implementation tasks:

- introduce path-aware representative preferences using existing metadata:
  - `artifactKind`
  - `projectArea`
  - `module`
  - `subsystem`
  - `headerRole`
- prefer runtime or production anchors over:
  - test
  - sample
  - benchmark
  - generated
  - compatibility-only paths
- decide how to score:
  - `src/` vs `tests/`
  - public include vs private/internal helper
  - engine/runtime vs tooling/editor
- keep the first-release scoring deterministic and explainable

Expected touch points:

- `indexer/src/metadata.rs`
- `indexer/src/resolver.rs`
- `server/src/models/responses.ts`

Validation checklist:

- fixtures and real-project samples show production/runtime anchors outranking test-shadowed copies
- response payloads can explain why a representative was chosen when needed

Exit criteria:

- representative anchors align better with human expectations on large mixed-purpose repositories

Completion summary:

- representative selection now uses persisted path and metadata hints to improve tie-breaking between structurally similar anchors
- runtime anchors now outrank test-shadowed anchors when structural preference is otherwise comparable
- public headers now outrank private or internal declaration anchors when both remain declaration-shaped
- the policy remains deterministic and explainable because it only uses already persisted metadata:
  - `artifactKind`
  - `headerRole`
  - file path classification
- regression coverage now includes:
  - runtime vs test-shadowed representative choice
  - public-header vs private-header representative choice

---

### M9-E4. Duplicate-Cluster and Ambiguity Surfacing

Goal:

- avoid false certainty when one symbol has several plausible representative anchors

Status:

- Completed

Implementation tasks:

- define how duplicate clusters are represented internally
- surface when the representative anchor was selected from multiple plausible anchors
- add response fields or notes such as:
  - representative confidence
  - representative selection reason
  - alternate canonical candidates when useful
- avoid bloating normal responses while still letting agents detect weak canonicality

Expected touch points:

- `indexer/src/models.rs`
- `server/src/models/responses.ts`
- `server/src/app.ts`
- `server/src/mcp-runtime.ts`

Validation checklist:

- duplicate-heavy fixtures produce explicit, bounded ambiguity notes
- high-confidence single-anchor cases remain compact

Exit criteria:

- CodeAtlas can say "this symbol exists, but the representative anchor is weak or contested" instead of silently overclaiming certainty

Completion summary:

- exact symbol lookup now surfaces representative metadata without changing exact identity semantics
- exact responses can now include:
  - `representativeConfidence`
  - `representativeSelectionReasons`
  - `alternateCanonicalCandidateCount`
- representative confidence now degrades to `weak` when multiple top-ranked representative candidates remain plausible
- JSON-backed stores fall back safely to singleton representative metadata when raw candidate clusters are unavailable
- targeted HTTP and MCP contract tests now verify that representative metadata is present on exact lookup responses

---

### M9-E5. Real-Project Regression and Evaluation Suite

Goal:

- lock representative-quality behavior against the kinds of repositories that exposed the issue

Status:

- Completed

Implementation tasks:

- add evaluation cases from:
  - OpenCV
  - LLVM
  - at least one Unreal-engine-class or similarly structured game repository when available
- record representative-quality findings as part of real-project validation notes
- define a regression list of representative-sensitive symbols
- compare before/after representative anchors instead of only measuring query latency

Expected touch points:

- `dev_docs/RealProject_opencv_Evaluation.md`
- `dev_docs/RealProject_llvm_Evaluation.md`
- `dev_docs/benchmark_results/`

Validation checklist:

- representative-sensitive symbols are tracked explicitly
- improvements can be demonstrated with before/after anchor comparisons

Exit criteria:

- representative-quality changes are validated on real repositories, not only toy fixtures

Completion summary:

- a shared regression checklist now exists in `dev_docs/RepresentativeRegressionList.md`
- representative-sensitive symbols are now tracked explicitly across real repositories:
  - LLVM: `llvm::StringRef`
  - OpenCV: `cv::Mat::Mat`
  - nlohmann/json: `parse_error`
- real-project evaluation notes now point back to the shared representative regression list
- the project now has a concrete before/after target set for future representative-quality work instead of relying on anecdotal observations

---

### M9-E6. Optional Repository-Tunable Representative Rules

Goal:

- allow carefully bounded repository-specific biasing when generic heuristics are not enough

Status:

- Completed

Implementation tasks:

- evaluate whether CodeAtlas should support optional representative-preference rules
- possible inputs:
  - preferred path prefixes
  - demoted test/sample/generated patterns
  - favored engine/runtime roots
- keep this optional and bounded so the default product does not become config-fragile
- document how repository-specific rules interact with the generic representative scorer

Expected touch points:

- config/docs files
- `indexer/src/metadata.rs`
- `indexer/src/resolver.rs`

Validation checklist:

- default behavior remains useful without custom rules
- repository-specific rules improve hard cases without destabilizing the shared model

Exit criteria:

- CodeAtlas can optionally bias representative selection for unusually large or structurally eccentric monorepos without abandoning deterministic defaults

Completion summary:

- CodeAtlas now supports an optional workspace-root `.codeatlasrepresentative.json`
- first-release supported rule inputs are:
  - `preferredPathPrefixes`
  - `demotedPathPrefixes`
  - `favoredArtifactKinds`
  - `favoredHeaderRoles`
- repository-specific rules now apply only as a bounded score adjustment layered on top of the generic structural representative scorer
- default behavior remains unchanged when no representative-rules file is present
- representative-rules loading is now wired into both normal indexing and watch mode
- regression coverage now includes:
  - representative-rules file loading and normalization
  - preferred-prefix vs demoted-prefix scoring

---

## 4. Final Exit Criteria

- representative symbol selection has an explicit documented ranking model
- declaration/definition structure materially improves representative choice
- test/sample/generated anchors are less likely to shadow production/runtime anchors
- duplicate-heavy repositories surface representative uncertainty honestly
- representative-quality improvements are validated on large real repositories such as LLVM and, when available, Unreal-engine-class game repositories

Milestone completion target:

- CodeAtlas still finds the same logical symbols as before, but agents are guided to much more useful first anchors on giant monorepos
- exact lookup becomes not only correct, but also substantially more usable in real large-repository workflows
