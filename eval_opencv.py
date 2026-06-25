#!/usr/bin/env python3
"""OpenCV real-project functional evaluation.

Runs two layers of checks against the latest OpenCV index DB:

  1. Basic counts vs. recorded baseline (catches large drops/inflations).
  2. Fixture-based precision/recall via eval/eval_fixtures.py against
     eval/fixtures/opencv.toml — the part that actually measures whether
     the indexer returns the *right* answer for known queries.

Exit code is non-zero if any check fails.
"""
from __future__ import annotations

import glob
import os
import sqlite3
import sys

# Allow importing eval/eval_fixtures.py
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "eval"))
import eval_fixtures  # noqa: E402

DATA_DIR = r"F:\dev\opencv\.codeatlas"
FIXTURE = os.path.join(HERE, "eval", "fixtures", "opencv.toml")
BASELINE = {"symbols": 65813, "files": 4888, "calls": 111189}
# Per-metric tolerance band — a real change to the indexer that moves these
# numbers more than ±10% should be a deliberate decision, not a silent drift.
TOLERANCE = 0.10

failures: list[str] = []


def find_db(data_dir: str) -> str:
    dbs = sorted(glob.glob(os.path.join(data_dir, "index-*.db")))
    if not dbs:
        sys.exit(f"ERROR: No DB in {data_dir}")
    return dbs[-1]


def section(t: str) -> None:
    print(f"\n{'=' * 70}\n  {t}\n{'=' * 70}")


def check(label: str, cond: bool, detail: str = "") -> bool:
    st = "OK" if cond else "FAIL"
    suf = f"  ({detail})" if detail else ""
    print(f"  [{st}] {label}{suf}")
    if not cond:
        failures.append(label)
    return cond


def within_band(actual: int, expected: int, tol: float = TOLERANCE) -> bool:
    if expected == 0:
        return actual == 0
    return abs(actual - expected) / expected <= tol


def main() -> int:
    db_path = find_db(DATA_DIR)
    print(f"DB: {os.path.basename(db_path)}")
    db = sqlite3.connect(f"file:{db_path}?mode=ro", uri=True)

    # ── 1. Basic counts ──────────────────────────────────────────────────
    section("1. Basic counts vs baseline")
    sym_cnt  = db.execute("SELECT COUNT(*) FROM symbols").fetchone()[0]
    file_cnt = db.execute("SELECT COUNT(*) FROM files").fetchone()[0]
    call_cnt = db.execute("SELECT COUNT(*) FROM calls").fetchone()[0]
    print(f"  symbols={sym_cnt:,}  files={file_cnt:,}  calls={call_cnt:,}")
    print(f"  baseline: {BASELINE}  (±{TOLERANCE*100:.0f}%)")

    check("symbols within ±10% of baseline", within_band(sym_cnt, BASELINE["symbols"]),
          f"{sym_cnt} vs {BASELINE['symbols']}")
    check("files within ±10% of baseline",   within_band(file_cnt, BASELINE["files"]),
          f"{file_cnt} vs {BASELINE['files']}")
    check("calls within ±10% of baseline",   within_band(call_cnt, BASELINE["calls"]),
          f"{call_cnt} vs {BASELINE['calls']}")

    # ── 2. Call resolution tier health ───────────────────────────────────
    section("2. Call resolution tier distribution")
    tiers = dict(db.execute(
        "SELECT resolution_tier, COUNT(*) FROM calls GROUP BY resolution_tier"
    ).fetchall())
    for tier, cnt in tiers.items():
        pct = 100.0 * cnt / call_cnt if call_cnt else 0
        print(f"  {tier or 'NULL':<20s} {cnt:>10,}  ({pct:5.1f}%)")
    # Soft signal — a sudden drop to 0% compiler_confirmed is almost always a
    # compile_commands.json or libclang regression, but we don't fail the run
    # on absolute percentages because they legitimately vary by workspace.

    # ── 3. Risk-signal heatmap ───────────────────────────────────────────
    section("3. Risk signals on symbols")
    for col in ("parse_fragility", "macro_sensitivity", "include_heaviness"):
        rows = db.execute(
            f"SELECT {col}, COUNT(*) FROM symbols GROUP BY {col}"
        ).fetchall()
        print(f"  {col}:")
        for v, c in rows:
            print(f"    {v or 'NULL':<12s} {c:>10,}")

    # ── 4. Fixture-based precision/recall ────────────────────────────────
    section("4. Ground-truth fixture (eval/fixtures/opencv.toml)")
    if not os.path.exists(FIXTURE):
        print(f"  fixture not found: {FIXTURE}")
        failures.append("opencv fixture missing")
    else:
        out = eval_fixtures.run_fixture(FIXTURE)
        s = out["summary"]
        check(
            f"fixture: {s['symbols_pass']}/{s['symbols_total']} symbols pass",
            s["symbols_fail"] == 0,
            f"failing: {s['symbols_fail']}",
        )

    # ── 5. Roll-up ───────────────────────────────────────────────────────
    section("Result")
    if failures:
        print(f"  FAIL ({len(failures)})")
        for f in failures:
            print(f"    - {f}")
        return 1
    print("  OK")
    return 0


if __name__ == "__main__":
    sys.exit(main())
