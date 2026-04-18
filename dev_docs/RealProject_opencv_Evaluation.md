# Real Project Evaluation: OpenCV

## Overview

This note captures a real-project validation run of CodeAtlas against the `OpenCV` workspace at `E:\Dev\opencv`.

Goal:

- verify that the Milestone 2 `tree-sitter-graph` integration survives a large real-world C++ codebase
- confirm that indexing still completes end to end on a large mixed repository
- sample a few real symbols and relationships after indexing
- record any storage or query-surface issues exposed by this run
- extend the validation to the Milestone 3 query surfaces after the newer indexer/schema changes

## Workspace

- Target: `E:\Dev\opencv`
- Output database: `E:\Dev\opencv\.codeatlas\index.db`
- Indexer: local debug build of `codeatlas-indexer`
- Validation helper: `cargo run --example query_db`

## Baseline Indexing Result

Full indexing completed successfully.

- files: `3695`
- symbols: `50526`
- call edges: `84298`
- elapsed time: about `95s` in non-verbose full-rebuild mode with the current debug build

This is materially larger and structurally noisier than the previous `benchmark` and `nlohmann/json` validation targets, so it is a useful scale check for the Milestone 2 extraction changes.

## Milestone 3 Query-Surface Validation

After rebuilding the indexer from the current workspace and regenerating `E:\Dev\opencv\.codeatlas`, the OpenCV database now includes the Milestone 3 `symbol_references` table as expected.

Validated live query surfaces:

- exact symbol lookup
  - `cv::imread` resolved successfully
- direct caller query
  - `/callers/imread` returned `2` direct callers with the shared `window` metadata
- impact analysis
  - `/impact?qualifiedName=cv::imread&depth=2&limit=5` returned summary-first output with affected symbols, affected files, and follow-up queries
- file overview query
  - `/file-symbols?filePath=modules/imgcodecs/include/opencv2/imgcodecs.hpp&limit=5`
  - returned `43` total symbols with summary-first payload and bounded `window`
- class member overview query
  - `/class-members?qualifiedName=cv::VideoCapture_DShow&limit=5`
  - returned `10` total members with deterministic ordering and bounded `window`
- generalized reference query
  - `/references?qualifiedName=cv::v_float32x4&limit=5`
  - returned `254` total references, truncated to `5`, with `typeUsage` records and shared `window` metadata

These checks show that the Milestone 3 caller, reference, impact, and overview surfaces are all live against the real OpenCV workspace, not only against fixture data.

## Sample Query Findings

Validation queries were executed against a copied database file because the original `index.db` could not yet be reopened reliably from a fresh process.

### Stable Samples

- exact: `cv::imread`
  - kind: `function`
  - location: `modules/imgcodecs/include/opencv2/imgcodecs.hpp:384`
  - callers: `2`
  - callees: `0`
- exact: `cv::VideoCapture::open`
  - kind: `method`
  - location: `modules/videoio/src/cap.cpp:112`
  - callers: `5`
  - callees: `48`
- exact: `cv::resize`
  - kind: `method`
  - location: `modules/imgproc/src/resize.cpp:4201`
  - callers: `161`
  - callees: `5`

These samples are enough to show that:

- the index contains meaningful symbol and call data on a large production C++ project
- graph-backed call extraction plus fallback still produces resolvable call edges at scale
- the queryable symbol set is useful enough for real navigation work

### Quality Signals To Watch

- `cv::Mat` did not resolve as an exact class symbol in this run
- `cv::Mat::Mat` resolved, but the returned representative location was `modules/core/include/opencv2/core/cuda.inl.hpp:752`, which is not an intuitive primary anchor for the main `cv::Mat` type
- short-name `Mat` search mixed unrelated entries such as a free `Mat` function and `cv::gapi::own::Mat`

This suggests that the Milestone 2 extraction pipeline is operational, but representative-symbol selection for heavily duplicated or declaration-rich OpenCV types still needs improvement.

## What This Validates

- Milestone 2 indexing still completes on a very large real project after `tree-sitter-graph` integration
- ambiguity and fallback behavior remain operational instead of crashing on large code volume
- sampled function and method lookups can still produce usable caller/callee data
- Milestone 3 query surfaces are operational on the real OpenCV workspace:
  - caller queries
  - reference queries
  - impact summaries
  - file and class overview queries

## What This Exposed

- the generated SQLite database can be consumed when copied, but the original `index.db` is not yet reliably reopenable from a fresh process
- this is now a storage durability / handoff issue, not a parser crash issue
- some high-value OpenCV symbols still need better representative selection and exact-lookup coverage

## Recommended Follow-up

1. Fix the database finalization / reopenability problem for the original `index.db`.
2. Add a real-project regression check that reopens a freshly generated database from a new process.
3. Improve representative selection for declaration-heavy symbols such as `cv::Mat`.

## Operational Guidance

The OpenCV run strongly suggests that external file indexers can interfere with the freshly generated `index.db` on Windows.

Recommended operating posture:

- exclude `<workspace-root>/.codeatlas/` from Everything indexing
- exclude `<workspace-root>/.codeatlas/` from Windows Search where practical
- exclude `<workspace-root>/.codeatlas/` from Defender scanning if local policy allows
- keep server-side retry and snapshot fallback enabled even after the indexing-path fixes

## Bottom Line

Milestone 2 passed the large-project extraction check, and the current Milestone 3 query surface is also usable on the same real workspace.

In practical terms:

- parsing and relationship extraction scale to OpenCV
- sampled symbol queries are already useful
- caller/reference/impact/overview queries all respond on real OpenCV data after rebuilding with the current schema
- the database finalization path still needs follow-up before this can be treated as production-ready end-to-end behavior
