# Ground-Truth Regression Suite

Status: Initial design — 2026-06-25.

## Purpose

`compare_dbs.py` measures *distributions* (resolution-tier %, ambiguity %, risk
signals). It cannot tell us whether the indexer returns the *correct* symbol,
caller, or reference for a known query.

This suite adds **fixture-based precision/recall** measurement on a fixed set
of hand-curated symbols across the deterministic `samples/` workspace and one
or more real workspaces (initially OpenCV).

Pass criteria become *recall against ground truth*, not just *no dangling
edges*. The output is comparable across runs and across indexer revisions,
which is the foundation every subsequent Tier 1 accuracy improvement
(overload resolution, field/method promotion, anchor weighting) will be
evaluated against.

## Scope

- Direct SQLite query path. The MCP server is NOT in the loop — that keeps
  the harness deterministic and avoids confusing "indexer accuracy" with
  "server response shaping".
- Read-only over an existing `index-*.db`. The suite never invokes the
  indexer itself; running the indexer remains the caller's responsibility.

## Fixture File Format (TOML)

One fixture file describes one workspace. The file lives next to its
workspace under `eval/fixtures/<workspace>.toml`.

```toml
# eval/fixtures/samples.toml
[fixture]
name        = "samples"
workspace   = "F:/dev/CodeAtlas"
db_glob     = ".codeatlas/index-*.db"   # latest match wins
description = "Deterministic in-repo C++ sample workspace."

# ─────────────────────────────────────────────────────────────────────
# Each [[symbol]] block names ONE query target and the expected results.
# Any of [representative], [callers], [callees], [references] can be
# omitted — only the supplied checks run.
# ─────────────────────────────────────────────────────────────────────

[[symbol]]
id             = "updateAI"
qualified_name = "Game::AIComponent::UpdateAI"

  [symbol.representative]
  # Representative anchor checks run against ALL candidates returned
  # by `SELECT ... FROM symbols WHERE qualified_name = ?`.
  #
  #   required_path_patterns:  at least one candidate's file_path must
  #                            match every pattern (regex, case-insensitive).
  #   forbidden_path_patterns: NO candidate's file_path may match any.
  #   prefer_role:             advisory — counts as a hint for a future
  #                            scoring metric; not yet enforced.
  required_path_patterns  = ["samples/src/ai_system\\.cpp"]
  forbidden_path_patterns = ["/test/", "\\.inl\\."]
  prefer_role             = "definition"

  [symbol.callers]
  # mode = "exact"      -> precision AND recall must both be 1.0
  # mode = "min_recall" -> only recall threshold enforced
  # mode = "superset"   -> every expected entry must appear, extras allowed
  mode  = "exact"
  match = "qualified_name"     # "qualified_name" | "file_line"
  expected = [
    { qualified_name = "::main" },
  ]

  [symbol.callees]
  mode  = "exact"
  match = "qualified_name"
  expected = [
    { qualified_name = "Game::AIComponent::ProcessIdle"   },
    { qualified_name = "Game::AIComponent::ProcessPatrol" },
    { qualified_name = "Game::AIComponent::ProcessChase"  },
    { qualified_name = "Game::AIComponent::ProcessAttack" },
  ]

  [symbol.references]
  # References use `symbol_references` table. Mode "count_band" tolerates
  # the noisy nature of reference counts on large workspaces.
  mode      = "count_band"
  min_count = 0
  max_count = 50
```

### Matching modes

| field             | mode          | meaning                                                  |
| ----------------- | ------------- | -------------------------------------------------------- |
| representative    | required      | ≥1 candidate matches every required regex                |
| representative    | forbidden     | 0 candidates match any forbidden regex                   |
| callers / callees | `exact`       | actual set == expected set (both precision and recall 1) |
| callers / callees | `superset`    | actual ⊇ expected (recall 1, precision unmeasured)       |
| callers / callees | `min_recall`  | recall ≥ `min_recall` threshold (default 1.0)            |
| references        | `count_band`  | `min_count ≤ count ≤ max_count`                          |

### Matching keys

For caller/callee lists, each expected entry is identified by:

- `qualified_name` (default) — robust to file moves and line shifts.
- `file_line` — `{ file_path = "...", line = N }`. Use only when the
  qualified name would be ambiguous (e.g. anonymous lambdas, macros).

`file_path` patterns in fixtures are POSIX-style, case-insensitive regexes
applied to the indexer's stored `file_path` column (which is already POSIX
forward-slash relative to the workspace).

## Output Schema

`eval_fixtures.py FIXTURE.toml` prints a human table and writes
`<fixture>.eval.json` next to it:

```json
{
  "fixture": "samples",
  "db": "index-20260624T084234973Z.db",
  "summary": {
    "symbols_total": 5,
    "symbols_pass": 4,
    "representative_pass": 5,
    "callers_recall_avg": 0.95,
    "callees_recall_avg": 1.0,
    "references_in_band": 5
  },
  "symbols": [
    {
      "id": "updateAI",
      "pass": true,
      "representative": { "candidate_count": 1, "required_matched": true, "forbidden_hit": [] },
      "callers":  { "precision": 1.0, "recall": 1.0, "missing": [], "unexpected": [] },
      "callees":  { "precision": 1.0, "recall": 1.0, "missing": [], "unexpected": [] }
    }
  ]
}
```

Exit code is non-zero iff any symbol's `pass` is `false`.

## How This Plugs In

- `eval_opencv.py` keeps its basic-stats role, but its final section calls
  `eval_fixtures.run("eval/fixtures/opencv.toml")` and adds the summary to
  its pass/fail roll-up.
- A future CI step runs `eval_fixtures` against `samples.toml` on every
  indexer change. Real-workspace fixtures (`opencv.toml`, eventually
  `llvm.toml`, `nlohmann.toml`) run on demand because they require a
  pre-indexed workspace.

## Out of Scope (for this milestone)

- Representative *scoring* simulation. The harness checks whether the right
  candidate exists in the result set; it does not yet reproduce the
  server-side single-anchor selection. Once Tier 1 #3 (anchor weighting)
  lands, a `prefer_role` / `prefer_path_pattern` check will be promoted from
  advisory to enforced.
- Trace-call-path and type-hierarchy checks. Add per-symbol once we have
  more than one stable fixture for those query kinds.
