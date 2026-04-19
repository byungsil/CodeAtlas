# Real Project Evaluation: LLVM 18.1.8

## Overview

This note captures a large real-project validation run of CodeAtlas against the `LLVM` monorepo workspace at `E:\Dev\llvm-project-llvmorg-18.1.8`.

Goal:

- verify that full indexing still completes on a substantially larger repository than OpenCV
- record real scale for symbols, calls, and propagation edges
- sample representative query surfaces against the generated LLVM database
- note any quality or operational issues exposed by the run

## Workspace

- Target: `E:\Dev\llvm-project-llvmorg-18.1.8`
- Output database: `E:\Dev\llvm-project-llvmorg-18.1.8\.codeatlas\index.db`
- Indexer: local debug build of `codeatlas-indexer`
- Query profiler: `server/src/query-profiler.ts`

## Full Indexing Result

Latest validation was run after Milestone 8 mixed-workspace support and after adding a larger internal indexing worker-thread stack.

Full indexing completed successfully under the default CodeAtlas runtime, without requiring manual `RUST_MIN_STACK` tuning.

- files: `56214`
- symbols: `526387`
- call edges: `849451`
- propagation edges: `2005690`
- elapsed time: `462006ms (462.01s)`

Stage timings:

- discovery: `6542ms (6.54s)`
- parse: `77623ms (77.62s)`
- resolve: `222180ms (222.18s)`
- persist: `129513ms (129.51s)`
- checkpoint: `1234ms (1.23s)`

Detailed parser and resolver breakdown emitted by the indexer:

- parse breakdown:
  - tree-sitter: emitted in runtime logs for this run
  - syntax-walk: emitted in runtime logs for this run
  - local-propagation: emitted in runtime logs for this run
  - graph-relations: emitted in runtime logs for this run
  - graph-compile: emitted in runtime logs for this run
  - graph-execute: emitted in runtime logs for this run
- resolve breakdown:
  - merge-symbols: emitted in runtime logs for this run
  - resolve-calls: emitted in runtime logs for this run
  - boundary-propagation: emitted in runtime logs for this run
  - propagation-merge: emitted in runtime logs for this run

This is a materially larger stress target than the current OpenCV validation dataset and confirms that the indexer completes on a monorepo-scale C++ codebase.

## Stack-Safety Note

LLVM-scale validation first exposed a real operational limit for very large repositories, and is now also the validation target for the runtime fix.

Historical progression:

- initial default run:
  - failed with stack overflow
- retry with `RUST_MIN_STACK=16777216`:
  - still failed with stack overflow
- retry with `RUST_MIN_STACK=67108864`:
  - completed successfully
- current runtime after adding a larger internal indexing worker-thread stack:
  - completed successfully without manual stack environment variables

Interpretation:

- the indexer is now stack-safe enough to complete LLVM-scale indexing under its default runtime configuration
- the previous `RUST_MIN_STACK` workaround is no longer required for this benchmark
- Unreal-engine-class game repositories are still expected to be a harder target than LLVM, so this fix should be treated as a major improvement rather than the final end state

Short-term operating guidance:

- the built-in larger worker-thread stack should now be enough for LLVM-scale indexing
- if a still larger repository needs more headroom, CodeAtlas can be overridden with:
  - `CODEATLAS_INDEXER_STACK_BYTES=<bytes>`
- `RUST_MIN_STACK` remains honored when present, but is no longer the preferred primary workaround

Required follow-up:

- validate the new runtime against an Unreal-engine-class game repository
- continue auditing parser and propagation traversal paths for remaining recursion depth hazards
- keep the dedicated larger worker-thread stack as a controlled runtime safety layer, not an excuse to ignore deeper structural stack growth

## Build-Metadata Note

Build metadata was partially disabled during this run.

- observed message:
  - `Failed to parse E:\Dev\llvm-project-llvmorg-18.1.8\clang\test\Index\compile_commands.json: trailing characters at line 18 column 1`

Interpretation:

- the indexer continued normally
- metadata ingestion degraded gracefully instead of stopping the run
- malformed or non-canonical compile DB files inside large workspaces remain an expected real-world edge case

## Query-Surface Validation

After the full rebuild completed, the query profiler was executed against the generated LLVM database and wrote:

- `dev_docs/benchmark_results/llvm-query-profile.json`

Profiler configuration:

- repeat count: `5`
- exact lookup: `llvm::StringRef`
- search query: `StringRef`
- direct callers query: `llvm::outs`
- generalized reference query: `llvm::StringRef`
- bounded impact query: `llvm::StringRef`

Measured query latencies:

- exact lookup `llvm::StringRef`
  - average: `0.117ms`
- search `StringRef`
  - average: `3.226ms`
- direct callers `llvm::outs`
  - average: `0.346ms`
- references `llvm::StringRef`
  - average: `1.295ms`
- impact `llvm::StringRef`
  - average: `0.813ms`

## Mixed-Workspace Validation

Milestone 8 behavior was also checked directly against the generated LLVM database through the shared server query surface.

Workspace summary:

- `cpp`: `53811` files, `507904` symbols
- `lua`: `6` files, `43` symbols
- `python`: `2378` files, `18383` symbols
- `rust`: `4` files, `19` symbols
- `typescript`: `15` files, `38` symbols
- total: `56214` files, `526387` symbols

Sample mixed-workspace checks:

- C++ search:
  - query: `StringRef`
  - filter: `language=cpp`
  - result grouping stayed entirely inside `cpp`
- Python search:
  - query: `lit`
  - filter: `language=python`
  - result grouping stayed entirely inside `python`
- C++ impact:
  - qualified symbol: `llvm::StringRef`
  - filter: `language=cpp`
  - `affectedLanguages` remained bounded to C++ for the sampled query

These checks show that the current exact/search/caller/reference/impact surfaces remain responsive on a very large LLVM-scale database.

## Quality Signals To Watch

The LLVM run also exposed an important representative-symbol quality issue.

- exact lookup for `llvm::StringRef` resolved successfully
- however the representative symbol anchored to:
  - file: `clang/test/Analysis/llvm-conventions.cpp`
  - line: `34`
- the same result carried declaration metadata pointing at:
  - `clang/include/clang/Basic/Cuda.h:13`

Interpretation:

- the symbol exists and is queryable
- but representative selection is still vulnerable to test-heavy or duplicate-rich workspaces
- large monorepos like LLVM make this issue more visible than smaller repositories

Representative regression target:

- see `dev_docs/RepresentativeRegressionList.md`
- tracked symbol:
  - `llvm::StringRef`
- current representative status:
  - `weak`

## What This Validates

- full indexing completes on a very large monorepo-scale mixed-language repository
- symbol, call, and propagation persistence scales well beyond the OpenCV validation target
- query profiling remains operational on the generated LLVM database
- exact/search/caller/reference/impact surfaces remain fast on the sampled real-project queries
- mixed-workspace summary and language-aware filtering still work at LLVM scale
- build-metadata ingestion failure is non-fatal when encountering malformed compile DB inputs

## What This Exposed

- representative-symbol selection still needs improvement for duplicated or test-shadowed symbols such as `llvm::StringRef`
- query-path correctness is good enough for navigation, but canonical anchor quality can still be misleading on very large mixed-purpose repositories
- malformed `compile_commands.json` files inside subtrees should be tolerated more explicitly or filtered more selectively
- LLVM stack safety is now materially improved under the default runtime
- Unreal-engine-class game repositories should still be treated as a stronger stack-safety target than LLVM, not merely another benchmark run

## Bottom Line

CodeAtlas completed a full indexing pass on `llvm-project-llvmorg-18.1.8`, produced a large but queryable mixed-language database, and handled sampled exact/search/caller/reference/impact queries without instability under the default current runtime.

In practical terms:

- LLVM-scale indexing completes successfully
- sampled structural queries remain responsive
- mixed-workspace query shaping still works at monorepo scale
- metadata ingestion degrades gracefully on malformed compile DB inputs
- symbol representative quality remains a meaningful next improvement area for giant mixed-source repositories
- LLVM-scale stack safety is now materially improved
- stack-safe indexing for Unreal-engine-class repositories remains a first-order follow-up requirement
