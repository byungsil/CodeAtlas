# CodeAtlas Benchmark Plan

## Goal

Define a stable benchmark contract for CodeAtlas so performance changes can be measured and compared across milestones.

## Dataset Tiers

### Tier 1. Small deterministic fixture

Purpose:

- fast correctness-adjacent performance smoke test
- stable output shape for local iteration

Recommended inputs:

- `samples/ambiguity`
- `samples/incremental`
- `samples/propagation`

Measurements:

- full index time
- no-change incremental time
- representative query latency on fixture-backed SQLite

### Tier 2. Medium curated sample

Purpose:

- catch scaling shifts that do not appear in tiny fixtures

Recommended inputs:

- `E:\Dev\nlohmann`
- `E:\Dev\benchmark`

Measurements:

- full index time
- incremental edit and delete scenarios
- DB size
- representative query latency

### Tier 3. Large real-project benchmark

Purpose:

- validate that CodeAtlas remains useful on a large production C++ repository

Recommended inputs:

- `E:\Dev\opencv`
- `E:\Dev\llvm-project-llvmorg-18.1.8`

Measurements:

- full index time
- no-change incremental rerun
- representative incremental `.cpp` edit
- representative header edit
- branch-like mass change response
- DB size
- representative query latency

Additional role:

- `E:\Dev\opencv` remains the primary large-project benchmark target for repeated optimization work
- `E:\Dev\llvm-project-llvmorg-18.1.8` is a larger monorepo-scale stress target for periodic validation

## LLVM Monorepo Stress Validation

Latest LLVM 18.1.8 full-rebuild validation:

- workspace: `E:\Dev\llvm-project-llvmorg-18.1.8`
- files: `56214`
- symbols: `526387`
- calls: `849451`
- propagation edges: `2005690`
- total time: `462006ms (462.01s)`

Stage timings:

- discovery: `6542ms (6.54s)`
- parse: `77623ms (77.62s)`
- resolve: `222180ms (222.18s)`
- persist: `129513ms (129.51s)`
- checkpoint: `1234ms (1.23s)`

Observed interpretation:

- LLVM-scale indexing completes successfully on a much larger dataset than OpenCV
- the dominant wall-clock costs are resolve and persistence at this scale
- build metadata ingestion degraded gracefully when it encountered a malformed subtree-local `compile_commands.json`
- the repository is genuinely mixed-language at scale, not only large-C++
- the current default CodeAtlas runtime is now sufficient for this run after adding a larger internal indexing worker-thread stack

Representative LLVM query profile:

- result file: `dev_docs/benchmark_results/llvm-query-profile.json`
- exact lookup `llvm::StringRef`: avg `0.117ms`
- search `StringRef`: avg `3.226ms`
- callers `llvm::outs`: avg `0.346ms`
- references `llvm::StringRef`: avg `1.295ms`
- impact `llvm::StringRef`: avg `0.813ms`

Quality note:

- `llvm::StringRef` resolved quickly, but its representative anchor landed on a test-heavy location rather than an intuitive canonical runtime definition
- this reinforces that canonical representative selection still needs follow-up on giant mixed-purpose repositories

Mixed-workspace summary from the same run:

- `cpp`: `53811` files, `507904` symbols
- `lua`: `6` files, `43` symbols
- `python`: `2378` files, `18383` symbols
- `rust`: `4` files, `19` symbols
- `typescript`: `15` files, `38` symbols
- total: `56214` files, `526387` symbols

Operational note:

- historical default run overflowed the stack
- `RUST_MIN_STACK=16777216` still overflowed
- `RUST_MIN_STACK=67108864` completed successfully
- current default runtime with internal worker-stack control also completes successfully

Interpretation:

- LLVM is now both a scale benchmark and a stack-safety benchmark
- CodeAtlas no longer depends on manual `RUST_MIN_STACK` tuning for this LLVM benchmark
- any repository in the Unreal-engine class should still be assumed to need further validation beyond LLVM

## Required Measurements

Every benchmark record should capture:

- workspace path or dataset identifier
- git commit or version under test
- build profile
- machine and OS notes
- full index time
- incremental index time where applicable
- watcher catch-up time where applicable
- query latency
- SQLite DB size
- symbol, call, reference, and propagation row counts where available

## Measurement Discipline

Performance work should leave a running measurement log in this document.

For every meaningful experiment, record:

- what changed
- whether the change was kept or reverted
- the dataset used
- the resulting full and incremental timings when available
- any row-count drift or correctness concern discovered during measurement

This log is intended to make future optimization work cumulative instead of rediscovering the same hotspots repeatedly.

Machine-readable benchmark outputs should be written to:

- `dev_docs/benchmark_results/`

Primary harness entrypoint:

- `scripts/benchmark/Run-CodeAtlasBenchmark.ps1`

## Post-MS8 Immediate Follow-Up

The next non-optional runtime hardening target is stack-safe indexing for Unreal-engine-class repositories.

Why it matters:

- LLVM already required a larger thread stack before the new internal worker-stack fix
- the intended production target is even larger and structurally harsher than LLVM
- LLVM now succeeds without manual stack environment variables, but that should be treated as a milestone, not the final proof point

Required work items:

1. identify remaining deep recursion or recursion-like stack growth in parser, propagation, and resolution paths
2. keep the controlled runtime fallback for larger worker thread stacks and validate whether it remains sufficient at Unreal-engine scale
3. validate the final behavior against at least:
   - LLVM
   - an Unreal-engine-class game repository
4. preserve existing benchmark discipline so stack-safety changes are measured, not guessed

## Stage Timing Contract

For full indexing runs, CodeAtlas should print:

- discovery
- parse
- resolve
- persist
- JSON write time when enabled
- checkpoint
- total elapsed time

This stage breakdown exists to make milestone-to-milestone regressions diagnosable instead of only observable.

## Initial Real-Project Baseline

Initial OpenCV baseline after Milestone 6 propagation support:

- workspace: `E:\Dev\opencv`
- files: `3695`
- symbols: `50526`
- calls: `84298`
- propagation edges: `265137`
- total time: `210294ms (210.29s)`

Stage timings:

- discovery: `354ms`
- parse: `89264ms (89.26s)`
- resolve: `114458ms (114.46s)`
- persist: `5616ms (5.62s)`
- checkpoint: `62ms`

Initial interpretation:

- the main Milestone 6 regression was not dominated by SQLite write cost
- the heaviest growth was in parse and especially resolve work
- the next optimization pass should inspect propagation-aware resolution before focusing on persistence

Current incremental spot checks on the same OpenCV workspace:

- no-change incremental rerun:
  - total time: `1468ms (1.47s)`
  - plan: `0 to index, 3695 unchanged, 0 to delete`
- representative single `.cpp` edit:
  - file: `modules/imgcodecs/src/loadsave.cpp`
  - total time: `2922ms (2.92s)`
  - plan: `1 to index, 3694 unchanged, 0 to delete`
  - re-resolved files: `1`
- representative header edit:
  - file: `modules/imgproc/include/opencv2/imgproc.hpp`
  - total time: `3973ms (3.97s)`
  - plan: `2 to index, 3693 unchanged, 0 to delete`
  - re-resolved files: `2`

These spot checks suggest that the operational incremental path is still fast even after Milestone 6, while the full-rebuild regression is concentrated in propagation-aware parsing and resolution.

## Early Optimization Result

After adding finer-grained timing instrumentation and removing an O(n^2) raw-call lookup from boundary propagation derivation, the same OpenCV full rebuild improved substantially.

Updated OpenCV full-rebuild result:

- workspace: `E:\Dev\opencv`
- files: `3695`
- symbols: `50526`
- calls: `84298`
- propagation edges: `266005`
- total time: `98305ms (98.31s)`

Updated stage timings:

- discovery: `361ms`
- parse: `88449ms (88.45s)`
- resolve: `3312ms (3.31s)`
- persist: `5573ms (5.57s)`
- checkpoint: `67ms`

Resolve breakdown:

- merge symbols: `110ms`
- resolve calls: `1603ms (1.60s)`
- boundary propagation: `868ms`
- propagation merge: `729ms`

Current interpretation:

- the dominant remaining cost is now parse, not resolve
- the earlier full-rebuild slowdown was heavily amplified by an inefficient boundary-propagation lookup path
- the next performance work should shift from resolver hot-path cleanup toward parser cost, fixture-aware profiling, and benchmark automation

## Graph Rule Cache Result

After caching the parsed `tree-sitter-graph` rule file instead of reparsing it for every source file, the same OpenCV full rebuild improved again.

Updated OpenCV full-rebuild result after graph-rule caching:

- workspace: `E:\Dev\opencv`
- files: `3695`
- symbols: `50526`
- calls: `84298`
- propagation edges: `266005`
- total time: `19521ms (19.52s)`

Updated stage timings:

- discovery: `338ms`
- parse: `9416ms (9.42s)`
- resolve: `3431ms (3.43s)`
- persist: `5655ms (5.66s)`
- checkpoint: `67ms`

Parse breakdown:

- tree-sitter parse: `37059ms (37.06s)` cumulative
- syntax walk: `16318ms (16.32s)` cumulative
- local propagation: `11565ms (11.56s)` cumulative
- graph relations: `83562ms (83.56s)` cumulative
- graph compile: `13337ms (13.34s)` cumulative
- graph execute: `64709ms (64.71s)` cumulative
- reference normalization: `119ms` cumulative

Current interpretation:

- the graph-rule compile path was a major avoidable cost and caching it restored most of the lost full-index performance
- the dominant remaining parser-side cost is now graph execution itself, not graph rule compilation
- the next optimization pass should focus on reducing graph execution work or narrowing when graph extraction is invoked

## Type-Usage Extraction Split Result

After moving `typeUsage` extraction off `tree-sitter-graph` and into a lightweight AST-based extraction path, the OpenCV full rebuild improved slightly again.

Updated OpenCV full-rebuild result after removing graph-based type usage:

- workspace: `E:\Dev\opencv`
- files: `3695`
- symbols: `50526`
- calls: `84298`
- propagation edges: `266005`
- total time: `19458ms (19.46s)`

Updated stage timings:

- discovery: `364ms`
- parse: `9385ms (9.38s)`
- resolve: `3479ms (3.48s)`
- persist: `5577ms (5.58s)`
- checkpoint: `64ms`

Updated parse breakdown:

- tree-sitter parse: `37140ms (37.14s)` cumulative
- syntax walk: `15854ms (15.85s)` cumulative
- local propagation: `11216ms (11.22s)` cumulative
- graph relations: `61444ms (61.44s)` cumulative
- graph compile: `8313ms (8.31s)` cumulative
- graph execute: `49502ms (49.50s)` cumulative
- reference normalization: `106ms` cumulative

Current interpretation:

- removing graph-based `typeUsage` reduced graph execution cost further
- the wall-clock gain is modest, so the remaining performance ceiling is still mostly tied to graph-based call and inheritance execution
- the next likely win would come from narrowing graph execution for call extraction itself or reducing the amount of attribute work done per graph match

## Graph Attribute Slim-Down Result

After removing graph attributes that the Rust parser can safely reconstruct locally, the OpenCV full rebuild improved again.

Updated OpenCV full-rebuild result after graph attribute slimming:

- workspace: `E:\Dev\opencv`
- files: `3695`
- symbols: `50526`
- calls: `84298`
- propagation edges: `266005`
- total time: `18820ms (18.82s)`

Updated stage timings:

- discovery: `368ms`
- parse: `8818ms (8.82s)`
- resolve: `3385ms (3.38s)`
- persist: `5605ms (5.61s)`
- checkpoint: `63ms`

Updated parse breakdown:

- tree-sitter parse: `36639ms (36.64s)` cumulative
- syntax walk: `15913ms (15.91s)` cumulative
- local propagation: `11397ms (11.40s)` cumulative
- graph relations: `51090ms (51.09s)` cumulative
- graph compile: `8246ms (8.25s)` cumulative
- graph execute: `39542ms (39.54s)` cumulative
- reference normalization: `115ms` cumulative

Current interpretation:

- graph execution still dominates the parser-side cumulative cost, but slimming the emitted attribute set produced another meaningful reduction
- the remaining big lever is no longer rule compilation or auxiliary metadata, but how much graph matching we do for call and inheritance extraction

## Inheritance Extraction Split Experiment

Experiment:

- move `inheritance` extraction off `tree-sitter-graph` and onto a lightweight AST-based extraction path

Result on `E:\Dev\opencv`:

- workspace: `E:\Dev\opencv`
- files: `3695`
- symbols: `50526`
- calls: `84298`
- propagation edges: `266005`
- total time: `19375ms (19.38s)`

Stage timings:

- discovery: `366ms`
- parse: `9338ms (9.34s)`
- resolve: `3429ms (3.43s)`
- persist: `5621ms (5.62s)`
- checkpoint: `62ms`

Interpretation:

- graph cumulative cost dropped slightly
- wall-clock gain was negligible
- inheritance extraction volume was too small to justify treating this as a major performance win
- the change was kept because it simplified graph responsibilities without harming correctness

## Graph Calls Disabled Experiment

Experiment:

- run full indexing with `CODEATLAS_DISABLE_GRAPH_CALLS=1`
- purpose: test whether graph-based call extraction was still a major wall-clock bottleneck

Result on `E:\Dev\opencv`:

- workspace: `E:\Dev\opencv`
- files: `3695`
- symbols: `50526`
- calls: `84297`
- propagation edges: `265997`
- total time: `19482ms (19.48s)`

Stage timings:

- discovery: same class of run as baseline
- parse: `9189ms (9.19s)`
- resolve: `3478ms (3.48s)`
- persist: `5800ms (5.80s)`

Interpretation:

- disabling graph calls did not materially improve wall-clock time
- call and propagation counts changed slightly
- conclusion: graph call extraction was no longer the main bottleneck, and removing it would trade away behavior for almost no speed win
- the change was reverted; graph calls remain enabled by default

## Local Propagation Disabled Experiment

Experiment:

- run full indexing with `CODEATLAS_DISABLE_LOCAL_PROPAGATION=1`
- purpose: isolate the remaining cost contribution of local propagation

Result on `E:\Dev\opencv`:

- workspace: `E:\Dev\opencv`
- files: `3695`
- symbols: `50526`
- calls: `84298`
- propagation edges: `0`
- total time: `14683ms (14.68s)`

Stage timings:

- parse: `8730ms (8.73s)`
- resolve: `2230ms (2.23s)`
- persist: `3020ms (3.02s)`

Interpretation:

- local propagation was confirmed as a meaningful remaining cost center
- the experiment was diagnostic only
- the change was reverted immediately because it removed Milestone 6 functionality

## Local Propagation Discovery Breakdown

OpenCV measurement after splitting local propagation into discovery, owner lookup, seed, event-walk, and return collection:

- workspace: `E:\Dev\opencv`
- files: `3695`
- symbols: `50526`
- calls: `84298`
- propagation edges: `266005`
- total time: `19775ms (19.77s)`

Stage timings:

- discovery: `345ms`
- parse: `9407ms (9.41s)`
- resolve: `3489ms (3.49s)`
- persist: `5890ms (5.89s)`
- checkpoint: `78ms`

Detailed parse breakdown:

- local propagation: `11832ms (11.83s)` cumulative
- local function discovery: `2758ms (2.76s)` cumulative
- local owner lookup: `14ms` cumulative
- local seed: `91ms` cumulative
- local event walk: `1943ms (1.94s)` cumulative
- local return collection: `199ms` cumulative

Interpretation:

- owner lookup is effectively free
- repeated function discovery and general traversal overhead are the dominant parser-side costs within local propagation

## Function-Definition Reuse Experiments

Two separate attempts were made to reduce `local-function-discovery` by reusing function-definition information from syntax-walk.

### Attempt 1. Reuse function-definition nodes directly

Result on `E:\Dev\opencv`:

- propagation edges: `265508`
- total time: `19314ms (19.31s)`
- local function discovery: `0ms`

Interpretation:

- the optimization reduced local discovery cost
- propagation row count drifted from the safe baseline `266005`
- the change was reverted

### Attempt 2. Cache visited function-definition byte ranges and re-resolve nodes

Result on `E:\Dev\opencv`:

- propagation edges: `265508`
- total time: `19408ms (19.41s)`
- local function discovery: `223ms`

Interpretation:

- this also reduced discovery cost
- propagation row count still drifted
- the change was reverted

Takeaway:

- function-definition reuse looks promising for performance
- naive reuse changes effective coverage and is not acceptable yet
- future work in this area must prove row-count stability before being kept

## Immediate Follow-up

1. Add repeatable benchmark commands and result capture.
2. Record no-change incremental and representative edit scenarios on OpenCV.
3. Profile propagation-aware resolve work before optimizing storage writes.
Latest local propagation breakdown on `E:\Dev\opencv` full rebuild:
- total: `19775ms (19.77s)`
- parse: `9407ms (9.41s)`
- resolve: `3489ms (3.49s)`
- persist: `5890ms (5.89s)`
- parse breakdown:
  - `local-propagation`: `11832ms` cumulative
  - `local-function-discovery`: `2758ms` cumulative
  - `local-owner-lookup`: `14ms` cumulative
  - `local-seed`: `91ms` cumulative
  - `local-event-walk`: `1943ms` cumulative
  - `local-return-collection`: `199ms` cumulative

Interpretation:
- owner lookup is not a meaningful hotspot
- the remaining local-flow cost is dominated by repeated function discovery and general traversal overhead
- the next candidate optimization is reducing the second tree scan used only for local propagation discovery

## Local Event-Walk Breakdown

Detailed OpenCV measurement after splitting `local-event-walk` into subcategories:

- workspace: `E:\Dev\opencv`
- files: `3695`
- symbols: `50526`
- calls: `84298`
- propagation edges: `266005`
- total time: `18192ms (18.19s)`

Stage timings:

- discovery: `332ms`
- parse: `8118ms (8.12s)`
- resolve: `3379ms (3.38s)`
- persist: `5759ms (5.76s)`
- checkpoint: `64ms`

Detailed parse breakdown:

- tree-sitter parse: `36018ms (36.02s)` cumulative
- syntax walk: `15849ms (15.85s)` cumulative
- local propagation: `11207ms (11.21s)` cumulative
- local function discovery: `2495ms (2.50s)` cumulative
- local owner lookup: `50ms` cumulative
- local seed: `33ms` cumulative
- local event walk: `1816ms (1.82s)` cumulative
- local declaration: `182ms` cumulative
- local expression: `79ms` cumulative
- local return: `11ms` cumulative
- local nested block: `1167ms (1.17s)` cumulative
- local return collection: `173ms` cumulative
- graph relations: `48631ms (48.63s)` cumulative
- graph compile: `6380ms (6.38s)` cumulative
- graph execute: `38816ms (38.82s)` cumulative
- reference normalization: `147ms` cumulative

Interpretation:

- within local event-walk, nested block traversal is the dominant cost
- declaration, expression-statement, and return handling are comparatively small
- the safest next optimization target is traversal overhead, not local flow semantics

## Cursor Traversal Result

After replacing vector-building `named_children()` traversals in local-flow walking and return collection with cursor-based iteration, OpenCV remained semantically stable and local-flow cumulative costs improved.

Updated OpenCV result after cursor-based local traversal:

- workspace: `E:\Dev\opencv`
- files: `3695`
- symbols: `50526`
- calls: `84298`
- propagation edges: `266005`
- total time: `19328ms (19.33s)`

Updated stage timings:

- discovery: `344ms`
- parse: `9268ms (9.27s)`
- resolve: `3468ms (3.47s)`
- persist: `5612ms (5.61s)`
- checkpoint: `63ms`

Updated local-flow breakdown:

- local propagation: `10859ms (10.86s)` cumulative
- local function discovery: `2590ms (2.59s)` cumulative
- local owner lookup: `27ms` cumulative
- local seed: `55ms` cumulative
- local event walk: `1581ms (1.58s)` cumulative
- local declaration: `204ms` cumulative
- local expression: `14ms` cumulative
- local return: `8ms` cumulative
- local nested block: `888ms` cumulative
- local return collection: `122ms` cumulative

Interpretation:

- the optimization was kept
- propagation row count stayed stable at `266005`
- local event-walk and nested-block traversal both dropped noticeably
- wall-clock full-index time remained in the same overall range, so this is a modest but safe parser-side cleanup rather than a headline improvement

## Function-Discovery Cursor Result

After replacing vector-building child traversal in the local function-discovery scan with cursor-based iteration, OpenCV again remained semantically stable and the local discovery cost dropped further.

Updated OpenCV result after local function-discovery cursor traversal:

- workspace: `E:\Dev\opencv`
- files: `3695`
- symbols: `50526`
- calls: `84298`
- propagation edges: `266005`
- total time: `18324ms (18.32s)`

Updated stage timings:

- discovery: `367ms`
- parse: `8466ms (8.47s)`
- resolve: `3320ms (3.32s)`
- persist: `5571ms (5.57s)`
- checkpoint: `64ms`

Updated local-flow breakdown:

- local propagation: `10406ms (10.41s)` cumulative
- local function discovery: `2075ms (2.08s)` cumulative
- local owner lookup: `29ms` cumulative
- local seed: `57ms` cumulative
- local event walk: `1672ms (1.67s)` cumulative
- local declaration: `219ms` cumulative
- local expression: `72ms` cumulative
- local return: `3ms` cumulative
- local nested block: `964ms` cumulative
- local return collection: `161ms` cumulative

Interpretation:

- the optimization was kept
- propagation row count stayed stable at `266005`
- local function discovery dropped from the prior `2590ms` cumulative result to `2075ms`
- this is a safe and more meaningful parser-side improvement than the earlier drift-inducing function-definition reuse attempts

## Additional Local-Flow Cursor Cleanup

After extending cursor-based iteration to more local-flow helpers including declaration handling, parenthesized-expression handling, declarator inspection, and return-scope seeding, OpenCV remained semantically stable and local propagation costs dropped again.

Updated OpenCV result after broader local-flow cursor cleanup:

- workspace: `E:\Dev\opencv`
- files: `3695`
- symbols: `50526`
- calls: `84298`
- propagation edges: `266005`
- total time: `19087ms (19.09s)`

Updated stage timings:

- discovery: `369ms`
- parse: `9136ms (9.14s)`
- resolve: `3368ms (3.37s)`
- persist: `5612ms (5.61s)`
- checkpoint: `69ms`

Updated local-flow breakdown:

- local propagation: `9978ms (9.98s)` cumulative
- local function discovery: `2086ms (2.09s)` cumulative
- local owner lookup: `20ms` cumulative
- local seed: `18ms` cumulative
- local event walk: `1490ms (1.49s)` cumulative
- local declaration: `199ms` cumulative
- local expression: `44ms` cumulative
- local return: `6ms` cumulative
- local nested block: `793ms` cumulative
- local return collection: `143ms` cumulative

Interpretation:

- the optimization was kept
- propagation row count stayed stable at `266005`
- local propagation cumulative cost fell below `10s` cumulative on OpenCV
- local nested-block traversal remains the largest subcomponent inside local event-walk, but is now noticeably lower than the earlier `1167ms` cumulative measurement

## Broader Local-Flow Helper Cursor Cleanup

After extending cursor-based iteration into additional local-flow helpers such as parameter seeding, declaration inspection, parenthesized-expression handling, and declarator inspection, OpenCV stayed stable and parser-side local-flow costs dropped again.

Updated OpenCV result after broader helper cursor cleanup:

- workspace: `E:\Dev\opencv`
- files: `3695`
- symbols: `50526`
- calls: `84298`
- propagation edges: `266005`
- total time: `18780ms (18.78s)`

Updated stage timings:

- discovery: `351ms`
- parse: `8796ms (8.80s)`
- resolve: `3411ms (3.41s)`
- persist: `5581ms (5.58s)`
- checkpoint: `64ms`

Updated local-flow breakdown:

- local propagation: `9970ms (9.97s)` cumulative
- local function discovery: `2040ms (2.04s)` cumulative
- local owner lookup: `17ms` cumulative
- local seed: `80ms` cumulative
- local event walk: `1470ms (1.47s)` cumulative
- local declaration: `197ms` cumulative
- local expression: `38ms` cumulative
- local return: `0ms` cumulative
- local nested block: `873ms` cumulative
- local return collection: `141ms` cumulative

Interpretation:

- the optimization was kept
- propagation row count stayed stable at `266005`
- local propagation stayed below `10s` cumulative and improved slightly again
- local event-walk and expression handling dropped a little further, while overall full-index time moved back under `19s`

## Graph Skip on Non-Call Files

After skipping `tree-sitter-graph` call extraction for files where the legacy AST pass found zero raw call sites, OpenCV stayed semantically stable and graph execution cost dropped materially.

Updated OpenCV result after skipping graph extraction for non-call files:

- workspace: `E:\Dev\opencv`
- files: `3695`
- symbols: `50526`
- calls: `84298`
- propagation edges: `266005`
- total time: `18678ms (18.68s)`

Updated stage timings:

- discovery: `346ms`
- parse: `8824ms (8.82s)`
- resolve: `3321ms (3.32s)`
- persist: `5592ms (5.59s)`
- checkpoint: `61ms`

Updated parser breakdown:

- local propagation: `9966ms (9.97s)` cumulative
- graph relations: `39841ms (39.84s)` cumulative
- graph compile: `6354ms (6.35s)` cumulative
- graph execute: `30559ms (30.56s)` cumulative

Interpretation:

- the optimization was kept
- call and propagation row counts stayed stable
- graph execution dropped from the prior `38273ms` cumulative result to `30559ms`
- this is the first safe graph-side optimization in this phase that produces a clearly visible cumulative reduction without changing behavior

## Parameter and Argument Cursor Experiment

Experiment:

- extend cursor-based iteration into parameter seeding and call-argument text extraction

Observed result on `E:\Dev\opencv`:

- workspace: `E:\Dev\opencv`
- files: `3695`
- symbols: `50526`
- calls: `84298`
- propagation edges: `266005`
- total time: first rerun `20877ms (20.88s)`, second rerun `20763ms (20.76s)`

Interpretation:

- counts stayed stable, so this did not look like a correctness regression
- however wall-clock time was consistently worse than the surrounding baseline
- the change was reverted

## Current Safe Baseline After Revert

Post-revert OpenCV verification for the current safe state:

- workspace: `E:\Dev\opencv`
- files: `3695`
- symbols: `50526`
- calls: `84298`
- propagation edges: `266005`
- total time: `18198ms (18.20s)`

Stage timings:

- discovery: `350ms`
- parse: `8263ms (8.26s)`
- resolve: `3411ms (3.41s)`
- persist: `5536ms (5.54s)`
- checkpoint: `63ms`

Current interpretation:

- this is the preferred state to continue optimizing from
- the kept graph-side and local-flow cursor optimizations remain in place
- the parameter/argument cursor experiment should be considered a measured dead end unless a new reason emerges to revisit it

## Skip Local Propagation on Definition-Free Files

After skipping local propagation extraction for files that contain no function or method definitions, OpenCV remained semantically stable and local propagation cost dropped substantially.

Updated OpenCV result after definition-free local propagation skip:

- workspace: `E:\Dev\opencv`
- files: `3695`
- symbols: `50526`
- calls: `84298`
- propagation edges: `266005`
- total time: `18274ms (18.27s)`

Updated stage timings:

- discovery: `353ms`
- parse: `8336ms (8.34s)`
- resolve: `3378ms (3.38s)`
- persist: `5617ms (5.62s)`
- checkpoint: `70ms`

Updated local-flow breakdown:

- local propagation: `8745ms (8.74s)` cumulative
- local function discovery: `766ms` cumulative
- local owner lookup: `21ms` cumulative
- local seed: `29ms` cumulative
- local event walk: `1446ms (1.45s)` cumulative
- local declaration: `152ms` cumulative
- local expression: `38ms` cumulative
- local return: `3ms` cumulative
- local nested block: `791ms` cumulative
- local return collection: `162ms` cumulative

Interpretation:

- the optimization was kept
- propagation row count stayed stable at `266005`
- definition-free files were a significant source of wasted local propagation work
- this is one of the more meaningful safe parser-side wins in the current optimization phase

## Symbol-Free Type and Reference Skip Experiment

Tried skipping AST-side type usage extraction, inheritance extraction, and normalized reference materialization on files with no parsed symbols.

Measured OpenCV result before revert:

- workspace: `E:\Dev\opencv`
- files: `3695`
- symbols: `50526`
- calls: `84298`
- propagation edges: `266005`
- total time: `18447ms (18.45s)`

Stage timings:

- discovery: `337ms`
- parse: `8535ms (8.54s)`
- resolve: `3354ms (3.35s)`
- persist: `5612ms (5.61s)`
- checkpoint: `63ms`

Parse-side details:

- local propagation: `8981ms (8.98s)` cumulative
- local function discovery: `795ms` cumulative
- local owner lookup: `18ms` cumulative
- local seed: `85ms` cumulative
- local event walk: `1579ms (1.58s)` cumulative
- local declaration: `214ms` cumulative
- local expression: `54ms` cumulative
- local return: `13ms` cumulative
- local nested block: `985ms` cumulative
- local return collection: `166ms` cumulative
- graph relations: `39824ms (39.82s)` cumulative
- graph compile: `6223ms (6.22s)` cumulative
- graph execute: `30718ms (30.72s)` cumulative

Interpretation:

- counts stayed stable, so this did not indicate a correctness regression
- however wall-clock time was not better than the nearby safe baseline
- the change was reverted
- this path should not be revisited unless profiling later shows symbol-free files dominate a new workload

## Type-Usage and Inheritance Cursor Traversal Experiment

Tried replacing `named_children()` vector allocation with direct cursor iteration inside AST-side type-usage and inheritance extraction.

Observed OpenCV reruns before revert:

- first rerun total: `18263ms (18.26s)`
- second rerun total: `18333ms (18.33s)`
- symbols: `50526`
- calls: `84298`
- propagation edges: `266005`

Representative timing details from the reruns:

- parse: `8397ms (8.40s)` then `8344ms (8.34s)`
- resolve: `3276ms (3.28s)` then `3327ms (3.33s)`
- persist: `5604ms (5.60s)` then `5707ms (5.71s)`
- graph execute: `30718ms (30.72s)` then `31292ms (31.29s)` cumulative

Interpretation:

- counts stayed stable, so this was not a correctness regression
- however the wall-clock result was too close to the current safe baseline to justify keeping another code-path variation
- the change was reverted as inconclusive
- this confirms that type-usage and inheritance AST traversal are not currently dominant bottlenecks

## Normalized-Reference Early Exit Experiment

Tried returning early from normalized reference materialization when a file had no symbol records or no `TypeUsage` / `Inheritance` relation events with a caller.

Observed OpenCV reruns before revert:

- first rerun total: `18305ms (18.30s)`
- second rerun total: `18121ms (18.12s)`
- third rerun total: `18896ms (18.90s)`
- symbols: `50526`
- calls: `84298`
- propagation edges: `266005`

Representative timing details:

- parse: `8352ms`, `8051ms`, `8899ms`
- resolve: `3376ms`, `3447ms`, `3395ms`
- persist: `5628ms`, `5646ms`, `5621ms`
- reference normalization cumulative: `124ms`, `125ms`, `136ms`

Interpretation:

- counts stayed stable, so this did not indicate a correctness regression
- however wall-clock behavior was too noisy to justify keeping a tiny early-exit that targets a relatively small cost center
- the change was reverted
- normalized reference materialization is not currently a primary bottleneck

## Query Profiling and Store Hot-Path Optimization

First query-profiling pass was added through:

- `server/src/query-profiler.ts`

Supporting optimizations:

- added batch symbol lookup support through `Store.getSymbolsByIds`
- reduced N+1 lookups in caller, reference, metadata-grouping, and impact summary paths
- added additional SQLite indexes for:
  - `qualified_name`
  - ordered file and scope symbol scans
  - caller/callee ordered call access
  - target/category/file reference access
  - propagation source/target anchor and kind/file access

OpenCV query profile snapshot:

- exact lookup `cv::imread`: avg `3.904ms`
- search `imread`: avg `1.961ms`
- callers `cv::imread`: avg `1.614ms`
- references `cv::v_float32x4`: avg `5.122ms`
- trace `cv::AGAST -> cv::makeAgastOffsets`: avg `7.912ms`
- impact `cv::imread`: avg `1.743ms`

Result file:

- `dev_docs/benchmark_results/opencv-query-profile.json`

Interpretation:

- the first profiling pass confirms that the main structural query paths remain interactive on a large OpenCV dataset
- batching symbol resolution removes repeated point lookups from high-level response shaping
- the next profiling pass can focus on p95 stability and on whether any specific path still needs dedicated query rewrites

## Incremental and Burst Benchmark Suite

Repeatable incremental-scale measurement is now available through:

- `scripts/benchmark/Run-CodeAtlasIncrementalSuite.ps1`

Current result file:

- `dev_docs/benchmark_results/incremental-suite-samples.json`

Covered scenarios:

- no-change rerun
- single `.cpp` edit
- header edit
- repeated file burst
- fixture-based mass change
- synthetic branch-like percentage churn

Current sample-suite highlights:

- no-change rerun: `0ms`, `0 to index`, `3 unchanged`, `0 to delete`
- single `.cpp` edit: `191ms`, `3 to index`, `0 unchanged`, `0 to delete`
- header edit: `187ms`, `3 to index`, `0 unchanged`, `0 to delete`
- repeated file burst: `17ms`, `17ms`, `17ms`, `21ms`, `24ms`, each with `1 to index`, `2 unchanged`, `0 to delete`
- fixture mass change: `177ms`, `5 to index`, `0 unchanged`, `2 to delete`
- synthetic branch-like churn: `23ms`, `12 to index`, `18 unchanged`, `0 to delete`, with escalation
  - `branch-like churn detected (12 of 30 files changed, 40%)`
  - `Mode override: full rebuild`

Interpretation:

- the benchmark suite now captures both ordinary incremental behavior and fail-safe escalation behavior in a repeatable way
- burst edits remain cheap on the small deterministic suite
- branch-like churn behavior is now backed by a concrete reproducible scenario instead of only unit tests

## Narrow Local-Flow Statement Recursion Experiment

Tried restricting generic local-flow recursion so it only descended into a whitelist of statement/container kinds.

Observed OpenCV result before revert:

- workspace: `E:\Dev\opencv`
- files: `3695`
- symbols: `50526`
- calls: `84298`
- propagation edges: `246813`
- total time: `18243ms (18.24s)`

Observed timing details:

- parse: `8508ms (8.51s)`
- resolve: `3303ms (3.30s)`
- persist: `5505ms (5.50s)`
- local propagation cumulative: `7496ms (7.50s)`
- local event walk cumulative: `683ms`
- local nested block cumulative: `280ms`

Interpretation:

- this significantly reduced propagation edge count (`266005 -> 246813`)
- the improvement therefore came from dropping valid flow collection, not from a safe optimization
- the change was reverted immediately
- this confirms that broad generic recursion inside local-flow extraction is still covering semantically important shapes
