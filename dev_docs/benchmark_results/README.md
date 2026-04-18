# Benchmark Results

This directory stores machine-readable benchmark outputs produced by:

- `scripts/benchmark/Run-CodeAtlasBenchmark.ps1`

Recommended usage:

- keep one or more checked-in baseline files for representative datasets
- write ad-hoc local comparison runs here while performance work is active
- summarize conclusions in `dev_docs/BenchmarkPlan.md`

Current intended baseline target:

- `E:\Dev\opencv` for large real-project indexing benchmarks

Current checked-in example baseline:

- `ambiguity-full-debug-baseline.json`
- `incremental-suite-samples.json`
- `opencv-query-profile.json`
