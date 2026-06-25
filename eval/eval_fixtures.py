#!/usr/bin/env python3
"""Fixture-based precision/recall regression suite for CodeAtlas indexes.

Reads a TOML fixture file describing expected query results for a set of
known symbols, runs the queries against the latest `index-*.db` under the
fixture's workspace, and reports per-symbol pass/fail plus aggregate
precision/recall.

See dev_docs/GroundTruthRegressionSuite.md for the fixture schema.

Usage:
    py -3 eval/eval_fixtures.py eval/fixtures/samples.toml
"""
from __future__ import annotations

import glob
import json
import os
import re
import sqlite3
import sys
import tomllib
from dataclasses import dataclass, field
from typing import Any


# ───────────────────────────────────────────────────────────── helpers ──

def latest_db(workspace: str, db_glob: str) -> str:
    pattern = os.path.join(workspace, db_glob)
    matches = sorted(glob.glob(pattern))
    if not matches:
        sys.exit(f"ERROR: no DB matched {pattern}")
    return matches[-1]


def compile_patterns(patterns: list[str]) -> list[re.Pattern[str]]:
    return [re.compile(p, re.IGNORECASE) for p in patterns]


def any_match(patterns: list[re.Pattern[str]], text: str) -> bool:
    return any(p.search(text) for p in patterns)


# ─────────────────────────────────────────────────────────── DB queries ──

def candidates_for(db: sqlite3.Connection, qname: str) -> list[dict[str, Any]]:
    rows = db.execute(
        """
        SELECT id, qualified_name, type, file_path, line, end_line,
               symbol_role, parse_fragility, macro_sensitivity
        FROM symbols
        WHERE qualified_name = ?
        """,
        (qname,),
    ).fetchall()
    keys = [
        "id", "qualified_name", "type", "file_path", "line", "end_line",
        "symbol_role", "parse_fragility", "macro_sensitivity",
    ]
    return [dict(zip(keys, r)) for r in rows]


def callers_of(db: sqlite3.Connection, ids: list[str]) -> list[dict[str, Any]]:
    if not ids:
        return []
    placeholders = ",".join("?" for _ in ids)
    sql = f"""
        SELECT DISTINCT s.id, s.qualified_name, c.file_path, c.line,
                        c.resolution_tier
        FROM calls c
        JOIN symbols s ON s.id = c.caller_id
        WHERE c.callee_id IN ({placeholders})
    """
    rows = db.execute(sql, ids).fetchall()
    keys = ["id", "qualified_name", "file_path", "line", "resolution_tier"]
    return [dict(zip(keys, r)) for r in rows]


def callees_of(db: sqlite3.Connection, ids: list[str]) -> list[dict[str, Any]]:
    if not ids:
        return []
    placeholders = ",".join("?" for _ in ids)
    sql = f"""
        SELECT DISTINCT s.id, s.qualified_name, c.file_path, c.line,
                        c.resolution_tier
        FROM calls c
        JOIN symbols s ON s.id = c.callee_id
        WHERE c.caller_id IN ({placeholders})
    """
    rows = db.execute(sql, ids).fetchall()
    keys = ["id", "qualified_name", "file_path", "line", "resolution_tier"]
    return [dict(zip(keys, r)) for r in rows]


def reference_count(db: sqlite3.Connection, ids: list[str]) -> int:
    if not ids:
        return 0
    placeholders = ",".join("?" for _ in ids)
    sql = f"""
        SELECT COUNT(*) FROM symbol_references
        WHERE target_symbol_id IN ({placeholders})
    """
    return db.execute(sql, ids).fetchone()[0]


# ─────────────────────────────────────────────────────────── matching ──

def match_key(entry: dict[str, Any], match_mode: str) -> tuple[Any, ...]:
    if match_mode == "qualified_name":
        return (entry["qualified_name"],)
    if match_mode == "file_line":
        return (entry["file_path"], int(entry["line"]))
    raise ValueError(f"unknown match mode: {match_mode}")


def normalize_expected(expected: list[dict[str, Any]], match_mode: str
                       ) -> set[tuple[Any, ...]]:
    out: set[tuple[Any, ...]] = set()
    for e in expected:
        if match_mode == "qualified_name":
            out.add((e["qualified_name"],))
        elif match_mode == "file_line":
            out.add((e["file_path"], int(e["line"])))
    return out


def precision_recall(actual: set[tuple[Any, ...]],
                     expected: set[tuple[Any, ...]]
                     ) -> tuple[float, float, set, set]:
    if not actual and not expected:
        return 1.0, 1.0, set(), set()
    tp = actual & expected
    missing = expected - actual
    unexpected = actual - expected
    precision = len(tp) / len(actual) if actual else 0.0
    recall = len(tp) / len(expected) if expected else 1.0
    return precision, recall, missing, unexpected


# ──────────────────────────────────────────────────────────── runner ──

@dataclass
class SymbolResult:
    id: str
    qualified_name: str
    candidates: int
    representative: dict[str, Any] | None = None
    callers: dict[str, Any] | None = None
    callees: dict[str, Any] | None = None
    references: dict[str, Any] | None = None
    failures: list[str] = field(default_factory=list)

    @property
    def passed(self) -> bool:
        return not self.failures

    def to_dict(self) -> dict[str, Any]:
        return {
            "id": self.id,
            "qualified_name": self.qualified_name,
            "pass": self.passed,
            "candidate_count": self.candidates,
            "representative": self.representative,
            "callers": self.callers,
            "callees": self.callees,
            "references": self.references,
            "failures": self.failures,
        }


def check_representative(sym: dict[str, Any], candidates: list[dict[str, Any]],
                         result: SymbolResult) -> None:
    cfg = sym.get("representative")
    if cfg is None:
        return
    required = compile_patterns(cfg.get("required_path_patterns", []))
    forbidden = compile_patterns(cfg.get("forbidden_path_patterns", []))

    paths = [c["file_path"] for c in candidates]
    required_matched = all(
        any(p.search(path) for path in paths) for p in required
    ) if required else True
    forbidden_hits = [
        c["file_path"] for c in candidates if any_match(forbidden, c["file_path"])
    ]

    result.representative = {
        "candidate_count": len(candidates),
        "required_patterns": cfg.get("required_path_patterns", []),
        "forbidden_patterns": cfg.get("forbidden_path_patterns", []),
        "required_matched": required_matched,
        "forbidden_hits": forbidden_hits,
        "paths": paths,
    }
    if not candidates:
        result.failures.append("representative: no candidate found for qualified_name")
        return
    if not required_matched:
        result.failures.append(
            "representative: no candidate matched required path pattern(s)"
        )
    if forbidden_hits:
        result.failures.append(
            f"representative: {len(forbidden_hits)} candidate(s) hit forbidden pattern"
        )


def check_call_edge(sym: dict[str, Any], kind: str, actual_rows: list[dict[str, Any]],
                    result: SymbolResult) -> None:
    cfg = sym.get(kind)
    if cfg is None:
        return
    mode = cfg.get("mode", "exact")
    match_mode = cfg.get("match", "qualified_name")
    expected_raw = cfg.get("expected", [])
    expected = normalize_expected(expected_raw, match_mode)
    actual = {match_key(r, match_mode) for r in actual_rows}

    precision, recall, missing, unexpected = precision_recall(actual, expected)

    out: dict[str, Any] = {
        "mode": mode,
        "match": match_mode,
        "expected_count": len(expected),
        "actual_count": len(actual),
        "precision": round(precision, 4),
        "recall": round(recall, 4),
        "missing": sorted(map(list, missing)),
        "unexpected_sample": sorted(map(list, unexpected))[:10],
        "unexpected_count": len(unexpected),
    }

    if mode == "exact":
        if missing:
            result.failures.append(f"{kind}: missing {len(missing)} expected entries")
        if unexpected:
            result.failures.append(
                f"{kind}: {len(unexpected)} unexpected entries (precision drop)"
            )
    elif mode == "superset":
        if missing:
            result.failures.append(f"{kind}: missing {len(missing)} expected entries")
    elif mode == "min_recall":
        threshold = float(cfg.get("min_recall", 1.0))
        out["threshold"] = threshold
        if recall < threshold:
            result.failures.append(
                f"{kind}: recall {recall:.2f} below threshold {threshold:.2f}"
            )
    else:
        result.failures.append(f"{kind}: unknown mode '{mode}'")

    result.__dict__[kind] = out


def check_references(sym: dict[str, Any], ref_count: int,
                     result: SymbolResult) -> None:
    cfg = sym.get("references")
    if cfg is None:
        return
    mode = cfg.get("mode", "count_band")
    out: dict[str, Any] = {"mode": mode, "count": ref_count}
    if mode == "count_band":
        lo = int(cfg.get("min_count", 0))
        hi = int(cfg.get("max_count", 1 << 30))
        out["min_count"] = lo
        out["max_count"] = hi
        if not (lo <= ref_count <= hi):
            result.failures.append(
                f"references: count {ref_count} outside [{lo}, {hi}]"
            )
    else:
        result.failures.append(f"references: unknown mode '{mode}'")
    result.references = out


def run_fixture(path: str) -> dict[str, Any]:
    with open(path, "rb") as fh:
        data = tomllib.load(fh)

    fixture_meta = data["fixture"]
    workspace = fixture_meta["workspace"]
    db_path = latest_db(workspace, fixture_meta["db_glob"])
    db = sqlite3.connect(f"file:{db_path}?mode=ro", uri=True)

    print(f"Fixture: {fixture_meta['name']}")
    print(f"  workspace: {workspace}")
    print(f"  db:        {os.path.basename(db_path)}")

    results: list[SymbolResult] = []
    for sym in data.get("symbol", []):
        qname = sym["qualified_name"]
        cands = candidates_for(db, qname)
        ids = [c["id"] for c in cands]

        res = SymbolResult(
            id=sym.get("id", qname),
            qualified_name=qname,
            candidates=len(cands),
        )

        check_representative(sym, cands, res)

        if sym.get("callers") is not None:
            check_call_edge(sym, "callers", callers_of(db, ids), res)
        if sym.get("callees") is not None:
            check_call_edge(sym, "callees", callees_of(db, ids), res)
        if sym.get("references") is not None:
            check_references(sym, reference_count(db, ids), res)

        results.append(res)

    summary = build_summary(results)
    print_table(results)
    print_summary(summary)

    out_path = os.path.splitext(path)[0] + ".eval.json"
    with open(out_path, "w", encoding="utf-8") as fh:
        json.dump({
            "fixture": fixture_meta["name"],
            "workspace": workspace,
            "db": os.path.basename(db_path),
            "summary": summary,
            "symbols": [r.to_dict() for r in results],
        }, fh, indent=2)
    print(f"\nWrote {out_path}")

    return {"results": results, "summary": summary}


def build_summary(results: list[SymbolResult]) -> dict[str, Any]:
    passes = sum(1 for r in results if r.passed)

    def avg(key: str) -> float | None:
        vals = [getattr(r, key)["recall"] for r in results if getattr(r, key)]
        return round(sum(vals) / len(vals), 4) if vals else None

    return {
        "symbols_total": len(results),
        "symbols_pass": passes,
        "symbols_fail": len(results) - passes,
        "representative_pass": sum(
            1 for r in results
            if r.representative
            and r.representative["required_matched"]
            and not r.representative["forbidden_hits"]
        ),
        "callers_recall_avg": avg("callers"),
        "callees_recall_avg": avg("callees"),
        "references_in_band": sum(
            1 for r in results
            if r.references and r.references["mode"] == "count_band"
            and r.references["min_count"] <= r.references["count"] <= r.references["max_count"]
        ),
    }


# ──────────────────────────────────────────────────────────── output ──

def print_table(results: list[SymbolResult]) -> None:
    print()
    print(f"  {'symbol':40s}  {'reps':>4s}  {'callers':>13s}  {'callees':>13s}  {'refs':>6s}  status")
    print(f"  {'-'*40}  {'-'*4}  {'-'*13}  {'-'*13}  {'-'*6}  ------")
    for r in results:
        reps = f"{r.candidates}"
        callers = f"{r.callers['recall']:.2f}/{r.callers['precision']:.2f}" if r.callers else "-"
        callees = f"{r.callees['recall']:.2f}/{r.callees['precision']:.2f}" if r.callees else "-"
        refs = str(r.references["count"]) if r.references else "-"
        status = "OK  " if r.passed else "FAIL"
        print(f"  {r.qualified_name[:40]:40s}  {reps:>4s}  {callers:>13s}  {callees:>13s}  {refs:>6s}  {status}")
        for f in r.failures:
            print(f"      - {f}")


def print_summary(summary: dict[str, Any]) -> None:
    print()
    print("Summary:")
    for k, v in summary.items():
        print(f"  {k:24s} {v}")


def main() -> int:
    if len(sys.argv) < 2:
        sys.exit("usage: eval_fixtures.py <fixture.toml>")
    fixture_path = sys.argv[1]
    out = run_fixture(fixture_path)
    failed = out["summary"]["symbols_fail"]
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
