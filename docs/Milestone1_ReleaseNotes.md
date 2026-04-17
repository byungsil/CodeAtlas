# Milestone 1 Release Notes

Milestone 1 "Trustworthy Lookup" is complete.

CodeAtlas now supports deterministic exact symbol lookup alongside safer heuristic lookup for large C++ codebases, with ambiguity and confidence surfaced explicitly instead of being hidden behind silent first-match behavior.

## What Changed

- Added canonical exact lookup paths:
  - MCP: `lookup_symbol`
  - HTTP: `GET /symbol`
- Standardized exact identity around `id` / `qualifiedName`
- Extended response payloads with:
  - `lookupMode`
  - `confidence`
  - `matchReasons`
  - optional `ambiguity`
- Improved resolver quality with:
  - same-parent and same-namespace ranking
  - receiver-aware ranking
  - parameter-count / arity hints
  - explicit `Resolved / Ambiguous / Unresolved` handling
- Unified header/source lifecycle handling:
  - declaration and definition collapse into one logical symbol
  - representative selection prefers definitions
  - declaration/definition locations are both preserved
- Added realistic ambiguity fixtures covering:
  - duplicate short names across namespaces
  - overloads
  - sibling class methods
  - split declaration/definition
  - `this->` / pointer-member calls

## Behavior Changes

- Name-based lookup remains available, but is now explicitly documented and surfaced as heuristic.
- Duplicate short-name lookups no longer pretend to be exact:
  - ambiguous results surface `confidence = "ambiguous"`
  - responses include `matchReasons = ["ambiguous_top_score"]`
  - responses include `ambiguity.candidateCount`
- Exact lookup never falls back to short-name matching.

## Validation

Acceptance checklist status: `PASS`

Validated by:

- Rust parser / resolver / merge tests
- HTTP contract tests
- MCP contract tests
- fixture-backed storage and API tests
- TypeScript compile checks

## Known Minor Follow-up

- One non-blocking Rust warning remains:
  - unused `read_all_raw_symbols` in [storage.rs](E:\Dev\CodeAtlas\indexer\src\storage.rs)

## Recommended Usage

1. Use `search_symbols` / `GET /search` for discovery.
2. Use `lookup_function` / `lookup_class` only as heuristic convenience paths.
3. Use `lookup_symbol` / `GET /symbol` when deterministic exact targeting matters.
