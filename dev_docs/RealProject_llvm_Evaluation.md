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

Full indexing completed successfully.

- files: `42978`
- symbols: `399546`
- call edges: `755292`
- propagation edges: `1851298`
- elapsed time: `388300ms (388.30s)`

Stage timings:

- discovery: `6013ms (6.01s)`
- parse: `66758ms (66.76s)`
- resolve: `186772ms (186.77s)`
- persist: `109650ms (109.65s)`
- checkpoint: `1139ms (1.14s)`

Detailed parser and resolver breakdown emitted by the indexer:

- parse breakdown:
  - tree-sitter: `171360ms (171.36s)` cumulative
  - syntax-walk: `108535ms (108.53s)` cumulative
  - local-propagation: `54072ms (54.07s)` cumulative
  - graph-relations: `269120ms (269.12s)` cumulative
  - graph-compile: `7199ms (7.20s)` cumulative
  - graph-execute: `238629ms (238.63s)` cumulative
- resolve breakdown:
  - merge-symbols: `1182ms (1.18s)`
  - resolve-calls: `163181ms (163.18s)`
  - boundary-propagation: `12205ms (12.21s)`
  - propagation-merge: `10202ms (10.20s)`

This is a materially larger stress target than the current OpenCV validation dataset and confirms that the indexer completes on a monorepo-scale C++ codebase.

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
- bounded trace query: `llvm::outs -> llvm::raw_ostream::flush`
- impact query: `llvm::outs`

Measured query latencies:

- exact lookup `llvm::StringRef`
  - average: `0.117ms`
- search `StringRef`
  - average: `3.226ms`
- direct callers `llvm::outs`
  - average: `0.346ms`
- references `llvm::StringRef`
  - average: `1.295ms`
  - returned `152` references in the final sampled result
- bounded trace `llvm::outs -> llvm::raw_ostream::flush`
  - average: `0.255ms`
  - `pathFound = false`
- impact `llvm::outs`
  - average: `0.813ms`
  - `totalAffectedSymbols = 31`
  - `totalAffectedFiles = 24`

Representative caller samples for `llvm::outs`:

- `llvm::riscvExtensionsHelp`
- `llvm::writeToOutput`
- `llvm::dlltoolDriverMain`
- `llvm::runPassPipeline`

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

## What This Validates

- full indexing completes on a very large monorepo-scale C++ codebase
- symbol, call, and propagation persistence scales beyond the OpenCV validation target
- query profiling remains operational on the generated LLVM database
- exact/search/caller/reference/impact surfaces remain fast on the sampled real-project queries
- build-metadata ingestion failure is non-fatal when encountering malformed compile DB inputs

## What This Exposed

- representative-symbol selection still needs improvement for duplicated or test-shadowed symbols such as `llvm::StringRef`
- query-path correctness is good enough for navigation, but canonical anchor quality can still be misleading on very large mixed-purpose repositories
- malformed `compile_commands.json` files inside subtrees should be tolerated more explicitly or filtered more selectively

## Bottom Line

CodeAtlas completed a full indexing pass on `llvm-project-llvmorg-18.1.8`, produced a large but queryable database, and handled sampled exact/search/caller/reference/impact queries without instability.

In practical terms:

- LLVM-scale indexing completes successfully
- sampled structural queries remain responsive
- metadata ingestion degrades gracefully on malformed compile DB inputs
- symbol representative quality remains a meaningful next improvement area for giant mixed-source repositories
