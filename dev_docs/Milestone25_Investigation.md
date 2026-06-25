# Milestone 25 — Class Extraction Audit (Investigation)

Status:

- Investigation phase (2026-06-25). Plan + implementation deferred to a
  follow-up session.

## Origin

MS24 (`Milestone24_AnchorWeighting.md`) deliberately punted on two
OpenCV fixtures by tagging them
`xfail_pending = "ms25-extraction-audit"`:

- `cv::Mat` — class symbol is **entirely absent** from the index.
- `cv::Mat::Mat` — only one constructor row is extracted
  (`cuda.inl.hpp:752`); the 20+ overloads in `mat.hpp` / `mat.inl.hpp` /
  `matrix.cpp` are missing.

This document captures the on-disk evidence and the most likely root
cause class so the next session can write a focused MS25 plan rather
than re-discover the symptoms.

## Evidence (post-MS24 index, F:/dev/opencv/.codeatlas/index-20260625T140353333Z.db)

### Symptom 1 — `cv::Mat` class row missing everywhere

```
sqlite> SELECT COUNT(*) FROM symbols_raw WHERE qualified_name='cv::Mat';
0
sqlite> SELECT COUNT(*) FROM symbols     WHERE qualified_name='cv::Mat';
0
```

Zero rows in both raw and merged tables. This is not a merge-stage
attrition issue — the class never reaches `symbols_raw`.

### Symptom 2 — namespace prefix is lost for many mat.hpp symbols

Sample of rows whose `file_path` is `modules/core/include/opencv2/core/mat.hpp`:

| name                  | qualified_name                | type      |
|---|---|---|
| AccessFlag            | AccessFlag                    | enum      |
| ACCESS_READ           | AccessFlag::ACCESS_READ       | enumMember|
| UMatUsageFlags        | UMatUsageFlags                | enum      |
| MatCommaInitializer_  | MatCommaInitializer_          | class     |
| Mat_                  | Mat_                          | class     |
| Mat_                  | Mat_::Mat_                    | method    |

**No `cv::` prefix.** The same indexer DOES emit `cv::` prefix on
symbols extracted from .cpp files (`sqlite> SELECT COUNT(*) FROM symbols_raw WHERE qualified_name LIKE 'cv::%' → 30,098`)
and from some other headers (`cv::gapi::own::Mat` in `gapi/own/mat.hpp` —
correctly prefixed). The failure mode is **specific to mat.hpp's
own parse as part of including-TUs**: when a `.cpp` file `#include`s
mat.hpp, the cursor traversal of mat.hpp loses its `namespace cv {`
scope context.

### Symptom 3 — only templated classes survive

`mat.hpp` declares 14 `class CV_EXPORTS …` types. Database extraction
captures **2 of them**:

| in source (line)       | in DB |
|---|---|
| `_OutputArray` (72)    | no  |
| `_InputArray` (160)    | no  |
| `_OutputArray` (301)   | no  |
| `_InputOutputArray` (396) | no |
| `MatAllocator` (507)   | no  |
| **`Mat` (839)**        | **no** |
| `UMat` (2498)          | no  |
| `SparseMat` (2800)     | no  |
| `MatConstIterator` (3149) | no |
| `SparseMatConstIterator` (3325) | no |
| `SparseMatIterator` (3369) | no |
| `NAryMatIterator` (3526) | no |
| `MatOp` (3564)         | no  |
| `MatExpr` (3651)       | no  |
| `MatCommaInitializer_` (3711, template) | **yes** |
| `Mat_` (2295, template) | **yes** |

Both survivors are `template<typename _Tp> class …` declarations.
Every non-template class in mat.hpp is missing from extraction.

### Symptom 4 — confused symbol kinds in mat.inl.hpp

```
qualified_name   | type      | file_path                  | line
Mat::Mat         | method    | modules/.../mat.inl.hpp    | 496
Mat              | function  | modules/.../mat.inl.hpp    | 3098
```

`Mat::Mat` lacks the `cv::` prefix; line 3098 of mat.inl.hpp has a
member named `Mat` classified as a free function. This corroborates
that libclang's cursor stream into the `mat*.hpp` files is missing
the `namespace cv {` scope and the class context for some entities.

### Symptom 5 — compile_commands.json does not cover mat.hpp directly

```
$ grep -c '"file": "[^"]*mat\.hpp"' compile_commands.json
0
```

`mat.hpp` is never parsed as a standalone header-only TU. It only
reaches libclang as part of the >700 `.cpp` TUs that include it. Every
such TU evaluates mat.hpp under that TU's macro state.

## Hypothesis

The leading candidate root cause:

> When `mat.hpp` is parsed as part of an including `.cpp` TU, libclang's
> cursor stream reports the `namespace cv {` Entity at some line but the
> indexer's `scope_stack` (`clang_parser.rs`) does not retain that
> namespace context throughout the giant header. Around the
> macro-heavy `class CV_EXPORTS Mat` (and siblings), the cursor visit
> either (a) reports those ClassDecls under an outer scope without `cv`,
> (b) reports them as forward declarations that the
> `entity.is_definition()` branch routes into the declaration arm with
> no cross-link to a definition (which then collides with a USR-based
> dedup that prefers other variants), or (c) is silently dropped due to
> a non-emittable `EntityKind` (e.g. `ClassTemplateSpecialization` for
> things that should be plain `ClassDecl`).

Why templates `Mat_` and `MatCommaInitializer_` survive: they are
`EntityKind::ClassTemplate`, taken through a different cursor child
arrangement, and libclang reports them at a different traversal depth
that happens to retain the outer namespace correctly. The non-template
`Mat` does not.

Why one CUDA inline `cv::Mat::Mat` survives at `cuda.inl.hpp:752`:
that file is an `*.inl.hpp` that is referenced from a different code
path inside core's headers. Whatever cursor arrangement libclang uses
when entering cuda.inl.hpp DOES retain the `cv` scope. So one
constructor variant — anchored to the CUDA inline header — slips
through, while every constructor declared inside mat.hpp's own
non-template class body is silently dropped.

## What this is NOT

- It is **not** a `--full` vs `incremental` artifact. The post-MS24
  index was a clean `--full` rebuild.
- It is **not** a merge-stage filter. `symbols_raw` is also empty for
  `cv::Mat`; the class never reaches the raw table.
- It is **not** an OpenCV `compile_commands.json` problem in isolation.
  Other CV namespaced classes (`cv::imread`, `cv::AccessFlag`'s
  enclosing namespace work elsewhere) come through fine. The
  break is specific to non-template class bodies inside mat.hpp.
- It is **not** scoped to `cv::Mat`. `cv::UMat`, `cv::SparseMat`,
  `cv::MatAllocator`, `cv::MatExpr`, `cv::MatOp`, and the
  `_InputArray` family are also absent. Whatever the bug is, it costs
  this index **13 core classes** plus all their member constructors.

## Probes Needed Before Writing the MS25 Plan

1. **Add a temporary debug trace in `clang_parser.rs`** that logs every
   `ClassDecl` / `StructDecl` cursor entered, the current
   `state.scope_stack` (sizes + last entry's name), and whether
   `entity.is_definition()` returned true/false. Re-run on a single
   `.cpp` TU that includes `mat.hpp` (e.g. `modules/core/src/matrix.cpp`).
   Expected outcome: the trace shows either (a) the `cv::Mat` ClassDecl
   is visited but the scope_stack lacks `cv`, or (b) it is never
   visited at all.

2. **Compare cursor visitor depth around `mat.hpp:839`** vs
   `mat.hpp:2295` (Mat_ template, survives). The cursor child-list
   reaching the indexer for the two locations should reveal the
   structural difference.

3. **Run libclang directly** (Python `clang.cindex` or `c-index-test`
   from the LLVM install) against `matrix.cpp` and walk the TU. If
   libclang reports `cv::Mat` as a `ClassDecl` with parent
   `Namespace cv` correctly, the bug is **purely on the indexer side**
   (scope_stack maintenance / cursor handling). If libclang already
   reports it under the wrong parent, the bug is **upstream** and the
   fix is different (e.g. choose a different visit mode, or rebuild
   USRs from a stable source).

## Scope of MS25 (Tentative)

Once the probe identifies the indexer-side break point, MS25 implements
the fix in `clang_parser.rs` and verifies via fixture flips:

- `cv::Mat` fixture transitions FROM XFAIL → OK (anchor under
  `modules/core/include/opencv2/core/mat.hpp`).
- `cv::Mat::Mat` fixture transitions FROM XFAIL → OK (anchor candidate
  set includes mat.hpp/mat.inl.hpp/matrix.cpp variants; selected
  anchor under the same set per MS24 ORDER BY).
- Symbol count on opencv reindex should **increase** by roughly the
  recovered 13 classes + their members (estimate: +200–500 rows).
- Tier split should be unchanged (this work doesn't touch call
  resolution).

If the probe shows the bug is in libclang's cursor reporting (option
"upstream"), MS25's scope shifts to a different remedy:
post-traversal namespace inference or a second-pass USR-based name
repair. Decision point captured in the eventual plan file.

## Sources Consulted

- `indexer/src/clang_parser.rs:230-262` — emittable CursorKinds
- `indexer/src/clang_parser.rs:337-373` — declaration/definition split
- F:/dev/opencv/modules/core/include/opencv2/core/mat.hpp (3700+ lines)
- F:/dev/opencv/.codeatlas/index-20260625T140353333Z.db (post-MS24)
- F:/dev/opencv/.codeatlas/compile_commands.json (no mat.hpp TU)
