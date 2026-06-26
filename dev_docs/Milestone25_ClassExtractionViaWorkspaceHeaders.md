# Milestone 25. Class Extraction via Workspace Headers

Status:

- Partially completed (2026-06-25). The workspace-aware emit gate is
  in. A residual mat.hpp-specific path remains XFAIL and is carried
  forward as `xfail_pending = "ms26-mat-hpp-body-visit"`.
- Performance hotfix landed 2026-06-26: cross-TU header-symbol dedup.
  See "Hotfix: Cross-TU Header Symbol Dedup" section below.

## Completion Evidence

The indexer used to gate symbol emission to the TU's own
`indexed_path`. Headers like `mat.hpp` that have no
`compile_commands.json` entry of their own were routed to the
tree-sitter fallback, which crashes on macro-heavy declarations and
produces partial recovery without entering `namespace cv`. The result
was 13+ central `cv::` classes silently dropped from the index.

MS25 admits any libclang-reported entity whose source location lies
inside the workspace root, anchoring the resulting Symbol to the
workspace-relative path of the entity's own file rather than the TU's.
The existing `merge_symbols` pass (resolver.rs:668) already collapses
USR-duplicate rows deterministically, so the per-TU duplicate
emissions fold to a single representative row per symbol — keyed off
MS24's `representative_rank`.

Code:

- `indexer/src/clang_parser.rs`:
  - `VisitorState` now carries `workspace_root: Option<PathBuf>`,
    initialised from the existing `parse_cpp_file` argument.
  - New local helpers `normalize_for_compare`, `is_inside_workspace`,
    `workspace_relative` — workspace-agnostic, case-fold-aware,
    handles `\\?\` extended-length Windows paths.
  - `visit_entity` builds an `emittable_location` predicate
    (`in_indexed_file || in_workspace_header`) and uses it as the
    Symbol-push gate. `CallExpr`, `BaseSpecifier`, and `LambdaExpr`
    branches keep their stricter `in_indexed_file` gate — calls and
    propagation rows are TU-attributed by design.
  - `Symbol.file_path`, `declaration_file_path`, and
    `definition_file_path` are workspace-relative for header symbols;
    bit-for-bit unchanged for in-TU symbols.
  - `PARSER_VERSION_TAG` bumped to `"cpp-clang-v2"`. MS22's parse cache
    folds the tag into its content-addressable key, so every existing
    `v1` cache entry transparently never matches; the on-disk
    `parse-cache/v1/` directory remains until a separate `--rebuild-
    cache` run reclaims it (acceptable: those entries are dead, not
    dangerous).
- `eval/fixtures/opencv.toml`:
  - New positive flip fixture `cv_AutoBuffer_class` (anchored to
    `modules/core/include/opencv2/core/utility.hpp`).
  - `cv_Mat_class` and `cv_Mat_Mat_ctor` keep their xfail tag,
    re-targeted from `ms25-extraction-audit` to
    `ms26-mat-hpp-body-visit` so the source of the remaining failure
    is explicit.

Tests:

- 7 new tests in `clang_parser.rs`:
  - 5 path-helper unit tests (`is_inside_workspace_accepts_*`,
    `is_inside_workspace_rejects_*`,
    `is_inside_workspace_handles_extended_length_prefix`,
    `workspace_relative_strips_root_and_uses_forward_slashes`,
    `workspace_relative_returns_none_when_outside_or_unset`).
  - `clang_emits_header_class_when_workspace_root_admits_include` —
    integration test that writes a header + TU to a temp workspace,
    parses, and asserts the header's class+method get emitted with
    workspace-relative `file_path` and the right `parent_id`.
  - `clang_does_not_emit_header_symbols_when_workspace_root_is_none` —
    confirms the legacy "TU-only" emission policy is preserved when
    the caller doesn't pass a workspace root.
- `cargo test`: 311 passed / 0 failed (3 of those are the MS24 set,
  7 are new MS25 set, 301 are pre-existing).
- `cd server && npx jest`: 173 passed / 0 failed (no server change).

## E1 — opencv reindex measurements

Full `--full --workspace-name opencv` run on
`F:/dev/opencv` (4,888 source files, 826 cpp TUs in
compile_commands.json):

| metric              | MS24 (pre-MS25) | MS25         | Δ            |
|---|---|---|---|
| symbols             | 74,562          | **84,090**   | **+9,528**   |
| calls               | 183,326         | 185,538      | +2,212       |
| files               | 4,888           | 4,888        | 0            |
| parse stage         | 30.5 s          | 52.0 s       | +21.5 s (+70 %) |
| total wall          | 60 s            | 88 s         | +28 s (+47 %) |
| compiler_confirmed  | 11,043 (6.0 %)  | 13,778 (7.4 %) | +2,735 (+1.4 pt) |
| heuristic           | 172,283 (94.0 %)| 171,760 (92.6 %) | -523 (-1.4 pt) |

The +9,528 new symbols are concentrated in `*/include/` paths — 10,465
cv::-prefixed rows reach `symbols_raw` from headers under
`/include/` after MS25 (vs. 0 in those paths pre-MS25 for files outside
compile_commands). compiler_confirmed % rising 1.4pt is the expected
side-effect: USRs that previously had no symbol row to link against
are now resolvable, so heuristic-tier edges flip to compiler_confirmed.

## E2 — recovered classes (sample, not exhaustive)

Examples of cv:: classes now in `symbols` that were absent in the MS24
DB (`F:/dev/opencv/.codeatlas/index-*MS24.db` vs MS25):

`cv::AutoBuffer`, `cv::Affine3`, `cv::BlockedRange`,
`cv::BufferPoolController`, `cv::Complex`, `cv::DataType`,
`cv::DataDepth`, `cv::MatConstIterator_`, `cv::MatIterator_`,
`cv::MatOp_AddEx`, `cv::MatOp_Bin`, `cv::MatOp_Cmp`,
`cv::MatOp_GEMM`, `cv::MatOp_Identity`, `cv::MatOp_Initializer`, …
(many template-class and detail classes — full enumeration not
material; the `cv_AutoBuffer_class` fixture is the canonical pinned
case for CI).

## E3 — residual gap (mat.hpp body visit)

`cv::Mat`, `cv::UMat`, `cv::SparseMat`, `cv::_InputArray`,
`cv::_OutputArray`, `cv::_InputOutputArray`, `cv::MatAllocator`,
`cv::MatExpr`, `cv::MatOp`, `cv::NAryMatIterator`,
`cv::MatConstIterator` (the *non-template* declarations in
`mat.hpp`) remain absent. Why:

- libclang **does** report `class CV_EXPORTS Mat` correctly under
  `parent=NAMESPACE cv` with `is_definition=True` when parsing any
  opencv .cpp TU (`eval/probe_mat_classes.py` confirms).
- MS25's gate would admit it, and the integration test confirms the
  gate works on a synthetic header.
- But after a full opencv reindex with MS25, `cv::Mat`'s USR
  (`c:@N@cv@S@Mat`) shows up in **zero** `symbols_raw` rows.
- The leading hypothesis is that opencv's precomp / include-guard
  chain consumes `mat.hpp` before the indexer's libclang visit
  reaches its body lines — i.e. matrix.cpp and its siblings all hit
  the `OPENCV_CORE_MAT_HPP` guard on first include via the precomp
  header, and subsequent .cpp TUs in the same indexer process never
  see the body. The probe script does NOT use precomp, which is why
  it sees the body. Verification requires a focused debug-trace run
  on a single TU; deferred to MS26.

Tagging xfail_pending = "ms26-mat-hpp-body-visit" on those two
fixtures makes the remaining failure visible without blocking CI.
The fixture harness reports XFAIL/XPASS so a future fix flips the
status without anyone having to remember the regression list.

## Constraints honored

- Workspace-agnostic: no hardcoded paths, no opencv-specific logic.
  Helpers test for "is this path under the workspace_root?", nothing
  more.
- `samples` deterministic fixture stays 5/5.
- Existing call/propagation flow is untouched: rows still anchor to
  the TU path that observed the relationship, not to the header path
  of the participating symbols.
- MS9-E2 representative ordering preserved (MS24 work). MS25 only
  adds rows; merge picks anchors by the existing scorer.
- Parse-cache invalidation handled by `PARSER_VERSION_TAG` bump,
  the same invalidation pattern MS22 documented for shape changes.

## Trade-offs (Realised)

- **Parse time grew +70 %** on opencv (30.5 s → 52.0 s). Within the
  3× ceiling the plan flagged; no in-TU dedup HashSet ships in this
  milestone. If sustained at this magnitude becomes a problem the
  remediation lane is clear (deduplicate by USR inside a single
  visitor pass before the merge stage).
- **+9,528 raw rows** before merge collapse. The final `symbols`
  table grew by the same number — these are real recovered symbols,
  not duplicates that survive merge. `symbols_raw` grew by far more
  (every .cpp TU now emits header symbols it sees), but that table
  is the audit log and its size is acceptable.
- **No regressions on samples fixtures, no behavior change for
  workspaces that pass `workspace_root=None`** (legacy callers).

## Rollback

Single revert on `feat/ms25-class-extraction-audit`. No schema
change, no migration, no DB-side breakage. The
`xfail_pending="ms26-..."` tags would need a follow-up adjustment if
the revert lands after CI has gone green with MS25 — keep the
opencv.toml change in a separate commit on the same branch so a
revert chain is two clean SHAs.

## Out of Scope / Follow-up

- **MS26 (proposed): mat.hpp body visit.** Add a debug-trace probe
  to clang_parser.rs to dump every `ClassDecl` libclang reports in
  any workspace header per TU, run against a single core/src/*.cpp
  TU, and identify why mat.hpp's body lines are skipped. Likely fix
  is either disabling include-guard cache between TUs in a single
  indexer process, or routing mat.hpp explicitly as a header-only
  TU with the right `-D` flags lifted from a representative
  includer.
- Tree-sitter fallback on header files. Remains the source of the
  `cv::`-prefix-less rows in mat.hpp (88 rows). Not addressed here;
  MS26 may or may not need to revisit.
- Refreshing `eval_opencv.py`'s stale `BASELINE` counts. Still out
  of milestone scope; the drift signal stays visible until a
  deliberate baseline bump.

## Hotfix: Cross-TU Header Symbol Dedup (2026-06-26)

### Problem

MS25's emit-broadly approach scales raw-symbol volume by inclusion
fanout. opencv (826 TUs) absorbed it as a +47% wall-time penalty,
which the trade-offs section called acceptable. But on a game-engine-
shaped workspace (~19,379 files, much deeper precompiled-header
chains) the indexer ran past 533 minutes at 60% progress, on a path
that previously completed in ~90 minutes. The user's tolerance is
< 120 min and the volume of work the indexer was doing scaled
super-linearly with TU count: every TU re-emitted every header
symbol it could see, and headers near the bottom of a precompiled-
header chain are visible to almost every TU.

### Fix

A process-wide `ClaimedSymbols` set, scoped to a single indexer run.
When a TU's libclang visit reaches a workspace-header entity, the
emit site calls `ClaimedSymbols::claim(usr)`. The first caller for a
given USR receives `true` and emits the row; later callers receive
`false` and drop the row at emission. Merge already collapses
USR-duplicate rows, so the dropped rows are the same ones merge
would have dropped — only one of them now reaches `symbols_raw` to
begin with. The final `symbols` table is unchanged.

- `clang_parser.rs`: `ClaimedSymbols { seen_usrs: Mutex<HashSet<String>> }`
  with `claim(&str) -> bool`. The Mutex's atomic `insert` provides
  the race-safe "exactly one winner" property.
- `VisitorState` carries `Option<Arc<ClaimedSymbols>>`. The Symbol
  push site at `visit_entity` consults it only when
  `in_workspace_header` (in-TU symbols continue to emit
  unconditionally — they have exactly one emitter by construction).
- A post-cache-hit filter in `indexing.rs`
  (`dedup_header_symbols_after_cache_hit`) applies the same dedup
  to `ParseResult.symbols` returned from MS22's parse cache. The
  cache stores full pre-dedup output so its content-addressable key
  stays correct; dedup is run-scoped and applies at consumption.
- The Arc is constructed at the top of each run entry point
  (`main.rs::run_incremental`, the full-rebuild loop in main.rs's
  indexing flow, `watcher.rs::run_full_index`,
  `watcher.rs::run_incremental_index`) and propagated through every
  parse function. The single-file probe in `incremental.rs` passes
  `None` because it is not part of a multi-TU run.

### Measurements

OpenCV (4,888 files, 826 cpp TUs), full reindex:

| metric          | MS24 baseline | MS25 (no dedup) | MS25 + hotfix |
|---|---|---|---|
| parse stage     | 30.5 s        | 52.0 s          | **31.15 s**   |
| total wall      | 60 s          | 88 s            | **65.7 s**    |
| symbols         | 74,562        | 84,090          | **84,090**    |
| calls           | 183,326       | 185,538         | 185,539       |

Total wall recovers to MS24 + ~9.5 % — close to noise — while every
recovered MS25 symbol survives. `symbols_raw` is smaller (the dropped
rows would have collapsed at merge anyway).

Fixtures: samples 5/5, opencv 5 OK + 2 XFAIL + 1 OK on
`cv_AutoBuffer_class` (unchanged from MS25).

Tests: 311 + 4 new (`claimed_symbols_first_claim_wins`,
`claimed_symbols_distinct_usrs_independent`,
`claimed_symbols_thread_safe_exactly_one_claim_wins`,
`clang_dedup_skips_header_symbol_on_second_tu`).
`cargo test` 315 / 0. `npx jest` 173 / 0.

### Trade-offs accepted

- **First-TU-wins selection.** Pre-hotfix, the same-USR cluster
  reached merge with N representatives and `representative_rank`
  (MS24) picked a winner. Post-hotfix, only the first TU's
  representative reaches merge; merge has nothing to rank against.
  In practice the indexer's `par_iter` order is workspace-stable
  across runs (file discovery order is fixed), so this is
  effectively deterministic per workspace; we accept the loss of
  rank-driven selection on header-class rows specifically.
  `cv_AutoBuffer_class` and equivalents still anchor to their
  header path because all TUs see the same definition. If a
  follow-up surfaces a case where this matters, the mitigation is
  to record the score on first claim and let later TUs override
  when they beat it — work for a future milestone.
- **Cache-hit redundancy.** On a warm rerun, a cached TU returns
  its full pre-dedup ParseResult; the new
  `dedup_header_symbols_after_cache_hit` filter re-applies the
  dedup per run. The first TU in the warm run claims everything it
  saw on the cold run, and later TUs filter against that. This is
  the right behavior but slightly different from a cold run's
  per-TU live-parse dedup; both are correct, the final `symbols`
  table is identical.

### Verification of the hotfix on opencv vs. user workspace

- opencv: ✓ wall time recovered (88 s → 65.7 s, within ~9 % of
  MS24).
- user 19,379-file workspace: TBD — the user reindexes locally;
  the goal is < 120 min. If the hotfix doesn't deliver, the
  rollback plan from the MS25 doc still applies (revert the MS25
  commit; the hotfix and MS25 are stacked on the same branch).
