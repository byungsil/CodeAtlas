# Milestone 23. Ground-Truth Regression Suite

Status:

- In progress — initial harness and seed fixtures landed 2026-06-25.

## Motivation

`compare_dbs.py` measures *distributions* (resolution-tier %, ambiguity %,
risk-signal counts). It cannot answer the question every subsequent
indexer-accuracy milestone needs to answer: **does the indexer return the
correct symbol, caller, or reference for a known query?**

Without that, every Tier 1 accuracy proposal in
`dev_docs/Improve_Indexer.md` (overload-resolution scoring, field-symbol
promotion, anchor-weighting) is a guess. MS23 builds the measurement
foundation before those changes start.

## Goals

1. Ground-truth fixtures, version-controlled with the indexer, that pin
   expected results for `lookup_function` / `find_callers` /
   `find_references` on a curated symbol set.
2. A harness that reads fixtures + queries the SQLite index directly
   (no MCP server in the loop — keeps the measurement of *indexer
   accuracy* separate from response-shaping at the server).
3. Per-symbol precision/recall + aggregate roll-up, comparable across
   indexer revisions.
4. Deliberately-failing fixtures for known weak cases (the
   `RepresentativeRegressionList.md` entries), so that future accuracy
   work has a concrete pass/fail signal.

## Non-Goals

- Reproducing the server's single-anchor representative *selection*. The
  harness verifies the right candidate is in the set; promoting that to
  an enforced "this exact anchor wins" check waits for the anchor
  weighting work.
- New indexer behavior. MS23 ships measurement only; it must not change
  any DB column or query path.

## Deliverables

Code:

- `eval/eval_fixtures.py` — fixture runner. Loads TOML, queries SQLite
  read-only, computes precision/recall per symbol, prints a table and
  writes a JSON sibling for diffing.
- `eval/fixtures/samples.toml` — 5 cases over the deterministic
  in-repo `samples/` workspace. All must pass on every change.
- `eval/fixtures/opencv.toml` — 6 cases over the OpenCV workspace.
  4 baseline + 2 deliberately failing (tracks `cv::Mat::Mat` weak anchor
  and missing `cv::Mat` class symbol).
- `eval_opencv.py` — restored. Basic counts ±10% band, tier
  distribution, risk-signal breakdown, plus fixture roll-up.

Docs:

- `dev_docs/GroundTruthRegressionSuite.md` — fixture schema, matching
  modes, output format, plug-in points.

## Fixture Format

TOML. One file per workspace under `eval/fixtures/<workspace>.toml`.
Per-symbol checks: `representative` (path-pattern require/forbid),
`callers` / `callees` (exact / superset / min_recall + qualified_name
or file_line matching), `references` (count band).

Full schema and rationale: `dev_docs/GroundTruthRegressionSuite.md`.

## Acceptance

A run is **green** when:

- `py -3 eval/eval_fixtures.py eval/fixtures/samples.toml` — 5/5 pass.
- `py -3 eval_opencv.py` — basic counts inside their tolerance band,
  fixture roll-up matches the expected pass/fail count (currently 4 OK
  / 2 known-FAIL; the failing two flip to OK when their referenced
  accuracy work lands and never silently regress in the other direction).

## Known-Failing Fixtures (Indexer Targets, Not Test Bugs)

These two fixtures fail today on purpose. They name the indexer fix, not
the test fix.

- `cv::Mat::Mat` — only candidate is `core/cuda.inl.hpp:752`. The right
  anchor lives under `modules/core/include/opencv2/core/mat.hpp`.
  Owner: Tier 1 #3 (anchor weighting — penalize `*.inl.hpp` / CUDA
  specializations vs. primary headers).
- `cv::Mat` — the class symbol itself is absent from the index even
  though its constructor (`cv::Mat::Mat`) is present. Reference count
  is 0 against an expected band ≥50. Owner: a separate symbol-extraction
  audit. May surface a fragility / parse-failure path on the primary
  `mat.hpp` translation unit.

If either fixture starts passing, the corresponding regression-list
entry in `RepresentativeRegressionList.md` should be flipped from
`weak` to `canonical` in the same change.

## Out-of-Band Findings (Captured for Follow-Up)

These were observed while seeding fixtures and are NOT part of MS23
scope — they are leads for future milestones.

- `compiler_confirmed` tier is only **6.0%** of edges on the OpenCV
  index (11,043 of 183,326). Heuristic-tier dominance at this level
  means most call-graph queries on the workspace are downstream of the
  17-signal scorer. Suggests Tier 1 #1 (overload resolution scoring
  upgrade) is the highest-leverage accuracy work after MS23.
- `parse_fragility = elevated` covers **64%** of symbols (47,545 of
  74,562). High enough that any subsequent accuracy work should report
  separate metrics for fragile vs. clean partitions.
- Recorded `BASELINE` in the previous `eval_opencv.py` (`symbols=65813`,
  `calls=111189`) is from a prior indexer revision. Current run shows
  `symbols=74562`, `calls=183326`. Baseline values need a deliberate
  refresh; MS23 left the legacy band in place rather than mask the
  drift behind a silent bump.

## Plug-In Points for Later Milestones

When a Tier 1 accuracy proposal lands:

1. Run `eval_opencv.py` before the change → record the baseline JSON
   sibling files (`opencv.eval.json`, `samples.eval.json`).
2. Apply the indexer change.
3. Re-run. Diff the JSON. Failures that flip to OK are the win;
   passes that flip to FAIL are the regression to investigate.
4. If new ground truth becomes available (e.g. a hand-verified caller
   set for a specific OpenCV function), add it to the fixture in the
   *same* PR as the indexer change so the new expectation is anchored
   to the implementation that satisfies it.
