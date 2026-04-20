# Benchmark Results

This directory stores machine-readable benchmark outputs produced by:

- `scripts/benchmark/Run-CodeAtlasBenchmark.ps1`

Recommended usage:

- keep one or more checked-in baseline files for representative datasets
- write ad-hoc local comparison runs here while performance work is active
- summarize conclusions in the relevant milestone or benchmark evaluation docs under `dev_docs/`

Current intended baseline target:

- `E:\Dev\opencv` for large real-project indexing benchmarks
- `E:\Dev\llvm-project-llvmorg-18.1.8` as an additional monorepo-scale stress target

Current checked-in example baseline:

- `ambiguity-full-debug-baseline.json`
- `incremental-suite-samples.json`
- `opencv-query-profile.json`
- `llvm-query-profile.json`
