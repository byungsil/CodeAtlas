# Milestone 25. Class Extraction via Workspace Headers

Status:

- Partially completed (2026-06-25). The workspace-aware emit gate is
  in. A residual mat.hpp-specific path remains XFAIL and is carried
  forward as `xfail_pending = "ms26-mat-hpp-body-visit"`.

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
