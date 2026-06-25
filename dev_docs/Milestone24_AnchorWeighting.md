# Milestone 24. Anchor Weighting Refinement

Status:

- Completed (2026-06-25)

## Completion Evidence

Two-sided change: indexer adds a same-USR-cluster penalty for inline-impl
headers; server adds a deterministic cross-USR `ORDER BY` so
`lookup_function(qname)` no longer returns an arbitrary row when a qualified
name resolves to multiple USRs.

Code:

- `indexer/src/models.rs` — extends `RepresentativeSelectionReason` with
  `InlineHeaderPenalty`.
- `indexer/src/resolver.rs` — emits the new reason for `.inl.hpp` /
  `.inl.h` / `.inl.hxx` paths (`looks_like_inline_header_path`),
  scores it −30 in `reason_score`. Sized so a same-USR-cluster inline
  definition in an `.inl.hpp` (280−30 = 250) still beats a
  declaration-only fallback (160), preserving the MS9-E2 tier ordering.
  3 new unit tests in the `resolver::tests` module.
- `server/src/storage/sqlite-store.ts` — `getSymbolByQualifiedName` now
  applies `REPRESENTATIVE_LOOKUP_SQL`, a 7-key `ORDER BY` that picks the
  public-header declaration first, then a definition, then public-header
  membership, then demotes inline-impl / test / sample / generated paths,
  and finally tie-breaks on file path then line.
- `server/src/storage/json-store.ts` — mirrors the same priority via
  `representativeSortKey` for the JSON fallback store.
- `eval/eval_fixtures.py` — adds `selected_path_pattern` check (mirrors
  the server's SQL so the harness reports the same anchor a real
  `lookup_function` call would) and `xfail_pending` field (failure
  becomes `XFAIL`, doesn't count; pass becomes `XPASS`, a signal that
  the referenced follow-up has landed).
- `eval/fixtures/opencv.toml` — `cv::imread` gains a strict
  `selected_path_pattern = "modules/imgcodecs/include/opencv2/imgcodecs\\.hpp"`;
  `cv::Mat::Mat` and `cv::Mat` are tagged
  `xfail_pending = "ms25-extraction-audit"`.

Tests:

- `cargo test resolver` — 39 passed, 0 failed (3 new MS24 tests +
  36 pre-existing). 0 regressions.
- `cd server && npx jest` — 173 passed, 19 suites (incl. 9 new MS24
  anchor tests across `sqlite-store-anchor.test.ts` and
  `json-store-anchor.test.ts`). 0 regressions.

End-to-end on OpenCV (post-reindex, F:/dev/opencv):

- `eval/fixtures/samples.toml` — 5/5 pass (unchanged).
- `eval/fixtures/opencv.toml` — 4 OK, 2 XFAIL, 0 FAIL. `cv::imread`
  passes its `selected_path_pattern` check; cv::Mat/cv::Mat::Mat XFAIL
  preserved.
- Manual:
  ```
  sqlite3 index-20260625T140353333Z.db "SELECT file_path, line, symbol_role, header_role
    FROM symbols WHERE qualified_name='cv::imread'
    ORDER BY <REPRESENTATIVE_LOOKUP_SQL> LIMIT 1"
  → modules/imgcodecs/include/opencv2/imgcodecs.hpp | 384 | declaration | public
  ```

## E1 — invariants preserved

Cache OFF (pre-MS24) and MS24 build produce structurally-identical core
counts on opencv (4888 files, full rebuild):

| metric              | pre-MS24    | MS24        | delta |
|---|---|---|---|
| symbols             | 74,562      | 74,562      | 0     |
| calls               | 183,326     | 183,326     | 0     |
| compiler_confirmed  | 11,043 (6.0%) | 11,043 (6.0%) | 0pt |
| heuristic           | 172,283 (94.0%) | 172,283 (94.0%) | 0pt |

`InlineHeaderPenalty` only adjusts a same-USR cluster's chosen
representative file path, not which rows exist. MS24 does not touch
call resolution, so the tier split is byte-stable.

## E2 — cross-USR determinism

Before MS24, `getSymbolByQualifiedName("cv::imread")` returned whichever
of three rows sqlite first matched. After MS24, the public-header
declaration (`imgcodecs.hpp:384`) wins deterministically over the two
out-of-line `.cpp` definitions, verified via 10 repeated calls in
`sqlite-store-anchor.test.ts` and manual sqlite3 spot-check.

## Constraints honored

- MS9-E2 tier ordering (out-of-line definition > inline > declaration)
  preserved within a same-USR cluster: the `−30` penalty is below the
  120-point tier gap.
- No DB columns added; no schema migration. Selection happens by query.
- No hardcoded OpenCV paths; only generic `.inl.h?p?p?x?` extensions and
  `/test|tests|sample|samples|generated/` substrings. The existing
  `.codeatlasrepresentative.json` repository-rule layer (M9-E6) is
  untouched and still composes on top of the new reason.
- The cv::Mat / cv::Mat::Mat anchor on opencv stays broken — that is a
  separate extraction issue (`xfail_pending = "ms25-extraction-audit"`).
  MS24 deliberately does not paper over it.

## Out of Scope (Deferred)

- The cv::Mat missing-class and the missing non-CUDA cv::Mat::Mat
  overloads. Their fixture entries XFAIL today and will XPASS when the
  follow-up extraction audit (MS25 candidate) lands. The XPASS will be
  the signal to retire the `xfail_pending` tag.
- Overload-resolution scoring on call edges. The compiler_confirmed
  share remains 6.0% on opencv — Tier-1 #1 work, separate milestone.
- Refreshing the stale `BASELINE` in `eval_opencv.py`. Out of MS24
  scope; the band-failure stays as a visible drift signal until a
  deliberate baseline bump commit.

## Rollback

Single revert on `feat/ms24-anchor-weighting-refinement`. No schema
change, no migration. The `selected_path_pattern` and `xfail_pending`
fixture fields are additive — old harnesses ignore them.
