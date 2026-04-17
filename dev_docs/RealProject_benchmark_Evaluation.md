# Real Project Evaluation: google/benchmark

## Overview

This note captures a real-project validation run of CodeAtlas against the `google/benchmark` workspace at `E:\Dev\benchmark`.

Goal:

- verify that exact lookup remains reliable on a production C++ library
- inspect heuristic lookup behavior in a repository with real structural ambiguity
- compare this workspace against the noisier `nlohmann/json` evaluation

## Workspace

- Target: `E:\Dev\benchmark`
- Output database: `E:\Dev\benchmark\.codeatlas`
- Indexer: local debug build of `codeatlas-indexer`
- Query surface used for validation:
  - MCP `lookup_symbol`
  - MCP `lookup_function`

## Baseline Indexing Result

Initial indexing completed successfully.

- files: `116`
- symbols: `419`
- call edges: `386`

Top-level symbol distribution:

- `src`: `249`
- `test`: `113`
- `include`: `56`
- `cmake`: `1`

This repository still contains a meaningful test footprint, but it is much less dominated by non-production trees than the `nlohmann/json` workspace.

## Baseline Query Findings

### Exact Lookup

Exact lookup behaved well on the real project.

Example:

- query: `lookup_symbol({ qualifiedName: "benchmark::State::PauseTiming" })`
- result:
  - `lookupMode = "exact"`
  - `confidence = "exact"`
  - `matchReasons = ["exact_qualified_name_match"]`

Returned relationships were structurally sensible:

- caller: `benchmark::ScopedPauseTiming::ScopedPauseTiming`
- callee: `benchmark::internal::ThreadTimer::StopTimer`

### Heuristic Lookup

Heuristic lookup surfaced ambiguity correctly.

Example 1:

- query: `lookup_function({ name: "Run" })`
- result:
  - `confidence = "ambiguous"`
  - `ambiguity.candidateCount = 4`
- selected symbol:
  - `benchmark::Fixture::Run`

Example 2:

- query: `lookup_function({ name: "ReportRuns" })`
- result:
  - `confidence = "ambiguous"`
  - `ambiguity.candidateCount = 4`
- selected symbol:
  - `benchmark::CSVReporter::ReportRuns`

Unlike the `nlohmann/json` evaluation, this ambiguity was not primarily caused by noisy repository composition. A large part of it comes from legitimate structural duplication inside the codebase itself, such as multiple reporter implementations and multiple `Run` methods across benchmark-related types.

## Product Conclusions

### What Worked

- exact lookup is stable on this real project
- ambiguity surfacing is honest and useful
- caller/callee traversal remains informative on production code
- heuristic lookup is still usable even without ignore filtering

### What This Exposed

- not all ambiguity is repository noise
- some large C++ projects contain genuine architectural duplication that should remain visible
- `.codeatlasignore` is critical for noisy repositories, but less important for cleaner library layouts

### Comparison With `nlohmann/json`

- `nlohmann/json` exposed the importance of index scope control
- `google/benchmark` exposed the importance of correctly surfacing real ambiguity

Together, the two evaluations are complementary:

- `nlohmann/json` shows why repository curation matters
- `google/benchmark` shows why ambiguity should not be hidden even after curation

## Practical Guidance

For repositories like `google/benchmark`, the recommended workflow is:

1. Index the repository as-is.
2. Validate a few exact lookups for critical symbols.
3. Test a few common short names to understand real ambiguity patterns.
4. Add `.codeatlasignore` only if non-production trees begin to dominate lookup results.

## Bottom Line

CodeAtlas passed this real-project check.

The strongest result here is consistency: exact lookup remains dependable, and heuristic lookup exposes ambiguity instead of fabricating certainty. This is the right behavior for a tool that aims to help AI agents reason safely about large C++ codebases.
