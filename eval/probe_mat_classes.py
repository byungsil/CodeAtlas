#!/usr/bin/env python3
"""MS25 probe: ask libclang directly what it reports for class declarations
in opencv's mat.hpp when it's pulled into matrix.cpp's translation unit.

We're trying to decide: when the indexer fails to extract `cv::Mat` (and
12 other non-template classes), is libclang already reporting them under
the wrong parent, or is libclang reporting them correctly and the bug is
on the indexer side (scope_stack maintenance)?

Output is a single table: line, kind, spelling, parent_kind, parent_spelling,
is_definition, usr. We grep for the mat.hpp ClassDecls of interest and
compare against what the indexer's SQLite DB shows.

Usage:
    py -3 eval/probe_mat_classes.py
"""
from __future__ import annotations

import json
import os
import shlex
import sys

LIBCLANG_DLL = r"C:\Program Files\LLVM\bin\libclang.dll"
OPENCV_ROOT = r"F:\dev\opencv"
COMPILE_COMMANDS = os.path.join(OPENCV_ROOT, ".codeatlas", "compile_commands.json")
TARGET_TU = r"F:\dev\opencv\modules\core\src\matrix.cpp"
MAT_HPP = r"F:\dev\opencv\modules\core\include\opencv2\core\mat.hpp"

# Class names from mat.hpp we care about. Source lines from grep on mat.hpp.
EXPECTED_CLASSES_IN_MAT_HPP = {
    72:   "_OutputArray (forward)",
    160:  "_InputArray",
    301:  "_OutputArray",
    396:  "_InputOutputArray",
    507:  "MatAllocator",
    839:  "Mat",
    2295: "Mat_  (template, survives in indexer)",
    2498: "UMat",
    2800: "SparseMat",
    3149: "MatConstIterator",
    3325: "SparseMatConstIterator",
    3369: "SparseMatIterator",
    3526: "NAryMatIterator",
    3564: "MatOp",
    3651: "MatExpr",
    3711: "MatCommaInitializer_  (template, survives in indexer)",
}


def import_clang():
    try:
        from clang import cindex  # type: ignore
    except ImportError:
        sys.exit("clang Python bindings missing. Install with: py -3 -m pip install clang")
    cindex.Config.set_library_file(LIBCLANG_DLL)
    return cindex


def load_args_for(tu_path: str) -> list[str]:
    """Translate clang-cl flags to clang driver flags libclang.cindex accepts.

    The clang Python bindings hand args straight to libclang's parser, which
    on Windows would normally also accept clang-cl flags via /flag form. But
    the safer path is to keep only the include/-D flags we actually need —
    libclang doesn't need /O2, /FS, /MD, etc. to walk the AST.
    """
    with open(COMPILE_COMMANDS, "r", encoding="utf-8") as f:
        entries = json.load(f)
    tu_norm = tu_path.replace("/", "\\").lower()
    entry = next(e for e in entries if e["file"].replace("/", "\\").lower() == tu_norm)
    raw = shlex.split(entry["command"], posix=False)

    keep: list[str] = []
    for tok in raw:
        # Strip the leading slash form (clang-cl) and dashes uniformly for the
        # tokens we care about.
        if tok.startswith("-I") or tok.startswith("/I"):
            inc = tok[2:].strip('"')
            keep.append("-I" + inc.replace("\\", "/"))
        elif tok.startswith("-D") or tok.startswith("/D"):
            d = tok[2:].strip('"')
            if d:
                keep.append("-D" + d)
            # /D <NAME> form — the actual define is the next token, but
            # shlex.split with posix=False already returned them as one token
            # in /D=NAME or -DNAME shape. Be tolerant.
        # Drop everything else; the include paths + defines are enough to
        # walk mat.hpp's cursor structure.
    # Force C++ mode (matrix.cpp).
    keep += ["-x", "c++", "-std=c++17", "-fparse-all-comments"]
    return keep


def main():
    cindex = import_clang()
    args = load_args_for(TARGET_TU)
    print(f"Probe TU: {TARGET_TU}")
    print(f"Flags ({len(args)}): {args[:4]}...{args[-4:]}")

    index = cindex.Index.create()
    print("Parsing...")
    tu = index.parse(
        TARGET_TU,
        args=args,
        options=(
            cindex.TranslationUnit.PARSE_DETAILED_PROCESSING_RECORD
            | cindex.TranslationUnit.PARSE_SKIP_FUNCTION_BODIES
        ),
    )
    print(f"Parsed. Diagnostics ({len(list(tu.diagnostics))}):")
    diag_errors = 0
    for d in tu.diagnostics:
        if d.severity >= cindex.Diagnostic.Error:
            diag_errors += 1
            if diag_errors <= 5:
                print(f"  [E] {d.spelling}")
    print(f"  total errors: {diag_errors}")

    # Walk every cursor; report class-shaped decls inside mat.hpp.
    mat_hpp_norm = MAT_HPP.replace("\\", "/").lower()

    findings = []
    for cur in tu.cursor.walk_preorder():
        loc = cur.location
        if loc.file is None:
            continue
        path = str(loc.file.name).replace("\\", "/").lower()
        if path != mat_hpp_norm:
            continue
        if cur.kind not in (
            cindex.CursorKind.CLASS_DECL,
            cindex.CursorKind.STRUCT_DECL,
            cindex.CursorKind.CLASS_TEMPLATE,
            cindex.CursorKind.CLASS_TEMPLATE_PARTIAL_SPECIALIZATION,
        ):
            continue
        parent = cur.semantic_parent
        findings.append(dict(
            line=loc.line,
            kind=cur.kind.name,
            spelling=cur.spelling,
            parent_kind=(parent.kind.name if parent else "<none>"),
            parent_spelling=(parent.spelling if parent else "<none>"),
            is_definition=cur.is_definition(),
            usr=cur.get_usr(),
        ))

    print()
    print(f"{'line':>5s} {'kind':<28s} {'name':<26s} {'parent':<30s} {'is_def':<6s} usr")
    print("-" * 130)
    findings.sort(key=lambda f: f["line"])
    for f in findings:
        parent = f"{f['parent_kind']} {f['parent_spelling']}"
        print(f"{f['line']:>5d} {f['kind']:<28s} {f['spelling']:<26s} {parent:<30s} {str(f['is_definition']):<6s} {f['usr']}")

    # Highlight the line ranges we care about.
    print()
    print("─" * 60)
    print("Cross-check vs expected:")
    seen_lines = {f["line"]: f for f in findings}
    for line, label in sorted(EXPECTED_CLASSES_IN_MAT_HPP.items()):
        if line in seen_lines:
            f = seen_lines[line]
            print(f"  L{line:<5d} '{label}': SEEN as {f['kind']} '{f['spelling']}' parent={f['parent_kind']} '{f['parent_spelling']}'")
        else:
            print(f"  L{line:<5d} '{label}': NOT SEEN by libclang at this exact line")


if __name__ == "__main__":
    main()
