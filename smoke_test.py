#!/usr/bin/env python3
"""Smoke test for CodeAtlas Phase A-E libclang improvements.

Run AFTER indexing with absolute path:
  codeatlas-indexer.exe F:/dev/CodeAtlas/samples --full --workspace-name smoke_test
"""
import sqlite3
import sys
import os
import glob

DATA_DIR = r"F:\dev\CodeAtlas\samples\.codeatlas"
failures = []


def find_db(data_dir):
    dbs = sorted(glob.glob(os.path.join(data_dir, "*.db")))
    if not dbs:
        print(f"ERROR: No DB found in {data_dir}")
        sys.exit(1)
    return dbs[-1]


def section(title):
    print(f"\n{'='*60}")
    print(f"  {title}")
    print("="*60)


def check(label, condition, detail=""):
    status = "OK" if condition else "FAIL"
    suffix = f"  (got {detail})" if detail != "" else ""
    print(f"  [{status}] {label}{suffix}")
    if not condition:
        failures.append(label)
    return condition


def main():
    db_path = find_db(DATA_DIR)
    print(f"DB: {os.path.basename(db_path)}")
    db = sqlite3.connect(db_path)

    section("Basic: Symbol / file / call counts")
    total   = db.execute("SELECT COUNT(*) FROM symbols").fetchone()[0]
    files   = db.execute("SELECT COUNT(*) FROM files").fetchone()[0]
    raw_cnt = db.execute("SELECT COUNT(*) FROM raw_calls").fetchone()[0]
    check("Total symbols > 100", total > 100, total)
    check("Files indexed > 5",   files > 5,   files)
    check("raw_calls > 10",      raw_cnt > 10, raw_cnt)

    section("DB schema: PRAGMA user_version = 2")
    user_ver = db.execute("PRAGMA user_version").fetchone()[0]
    check("user_version = 2", user_ver == 2, user_ver)

    section("Phase B: Symbol types (class / method)")
    go_rows = db.execute(
        "SELECT name, type FROM symbols WHERE file_path LIKE '%game_object%' GROUP BY name, type"
    ).fetchall()
    type_map = {}
    for name, t in go_rows:
        if name not in type_map or t == "class":
            type_map[name] = t
    check("IUpdatable is class",  type_map.get("IUpdatable") == "class",  type_map.get("IUpdatable"))
    check("GameObject is class",  type_map.get("GameObject") == "class",  type_map.get("GameObject"))
    check("GameWorld is class",   type_map.get("GameWorld") == "class",   type_map.get("GameWorld"))
    method_cnt = db.execute(
        "SELECT COUNT(*) FROM symbols WHERE type = 'method' AND file_path LIKE '%game_object%'"
    ).fetchone()[0]
    check("Methods indexed as type=method", method_cnt > 0, method_cnt)
    print(f"  Types in game_object: {sorted(set(t for _, t in go_rows))}")

    section("Phase C: Declaration / Definition cross-links")
    h_rows = db.execute(
        "SELECT name, type, symbol_role, declaration_file_path, definition_file_path "
        "FROM symbols WHERE name = 'Update' AND file_path LIKE '%game_object.h%'"
    ).fetchall()
    cpp_rows = db.execute(
        "SELECT name, type, symbol_role, declaration_file_path, definition_file_path "
        "FROM symbols WHERE name = 'Update' AND file_path LIKE '%game_object.cpp%'"
    ).fetchall()
    print(f"  Update in game_object.h ({len(h_rows)}):")
    for r in h_rows:
        print(f"    {r[1]:8s} role={r[2]}  decl={os.path.basename(r[3]) if r[3] else None}  def={os.path.basename(r[4]) if r[4] else None}")
    print(f"  Update in game_object.cpp ({len(cpp_rows)}):")
    for r in cpp_rows:
        print(f"    {r[1]:8s} role={r[2]}  decl={os.path.basename(r[3]) if r[3] else None}  def={os.path.basename(r[4]) if r[4] else None}")
    check("Update declared in game_object.h",
          any(r[3] is not None for r in h_rows))
    check("Update defined in game_object.cpp",
          any(r[4] is not None for r in cpp_rows))

    section("Phase E: symbol_role = virtual / pure_virtual")
    virt_rows = db.execute(
        "SELECT name, type, symbol_role, file_path FROM symbols "
        "WHERE symbol_role IN ('virtual', 'pure_virtual') LIMIT 10"
    ).fetchall()
    print(f"  Virtual/pure-virtual symbols: {len(virt_rows)}")
    for r in virt_rows:
        print(f"    {r[0]:25s}  role={r[2]}  ({os.path.basename(r[3])})")
    check("At least 1 virtual or pure_virtual symbol", len(virt_rows) > 0, len(virt_rows))
    pure_names = [r[0] for r in virt_rows if r[2] == "pure_virtual"]
    check("Update is pure_virtual in IUpdatable", "Update" in pure_names, pure_names)

    section("Phase A: Inheritance (symbol_references category=inheritanceMention)")
    inherit_refs = db.execute(
        "SELECT source_symbol_id, target_symbol_id, file_path, line "
        "FROM symbol_references WHERE category = 'inheritanceMention' LIMIT 5"
    ).fetchall()
    print(f"  Inheritance references: {len(inherit_refs)}")
    for r in inherit_refs:
        print(f"    {r[0][:45]}  -> {r[1][:45]}")
    check("At least 1 inheritance reference", len(inherit_refs) > 0, len(inherit_refs))

    section("Phase D: Lambda symbols")
    lambdas = db.execute(
        "SELECT name, file_path, line FROM symbols WHERE name LIKE '<lambda>%' LIMIT 5"
    ).fetchall()
    print(f"  Lambda symbols found: {len(lambdas)}")
    for lam in lambdas:
        print(f"    {lam[0]}  in {os.path.basename(lam[1])}  line {lam[2]}")
    check("At least 1 lambda symbol indexed", len(lambdas) > 0, len(lambdas))

    section("Phase 3: USR fast-path (pre_resolved_callee_id)")
    pre = db.execute(
        "SELECT COUNT(*) total, COUNT(pre_resolved_callee_id) resolved FROM raw_calls "
        "WHERE file_path LIKE 'src/%'"
    ).fetchone()
    total_src, resolved_src = pre
    pct = round(resolved_src / total_src * 100, 1) if total_src else 0
    print(f"  src/ calls with pre_resolved: {resolved_src}/{total_src} ({pct}%)")
    check("100% of src/ calls have pre_resolved_callee_id",
          total_src > 0 and pct == 100.0, f"{pct}%")

    section("Call resolution quality")
    resolved  = db.execute("SELECT COUNT(*) FROM calls WHERE callee_id IS NOT NULL").fetchone()[0]
    total_res = db.execute("SELECT COUNT(*) FROM calls").fetchone()[0]
    pct_res = round(resolved / total_res * 100, 1) if total_res > 0 else 0
    check("Resolved calls >= 30%", pct_res >= 30, f"{pct_res}% ({resolved}/{total_res})")

    section("SUMMARY")
    if failures:
        print(f"  FAILED ({len(failures)}): {failures}")
        db.close()
        sys.exit(1)
    print(f"  All checks passed!")
    db.close()


if __name__ == "__main__":
    main()
