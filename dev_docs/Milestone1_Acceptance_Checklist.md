# Milestone 1 Acceptance Checklist

Use this checklist before closing Milestone 1 "Trustworthy Lookup".

## Exact Lookup

- `lookup_symbol` performs exact MCP lookup by `id` and `qualifiedName`
- `GET /symbol` performs exact HTTP lookup by `id` and `qualifiedName`
- exact lookup never falls back to short-name matching
- invalid exact lookup input returns `BAD_REQUEST`
- unknown exact lookup returns `NOT_FOUND`

## Ambiguity Handling

- legacy short-name lookup remains available for exploratory use
- duplicate short names are surfaced as heuristic ambiguity, not silent exactness
- heuristic ambiguity responses include `confidence = ambiguous`
- heuristic ambiguity responses include `matchReasons = ["ambiguous_top_score"]`
- heuristic ambiguity responses include `ambiguity.candidateCount`

## Confidence Surfacing

- exact lookup responses include `lookupMode = exact`
- exact lookup responses include `confidence = exact`
- exact lookup responses include exact match reasons
- heuristic lookup responses include `lookupMode`, `confidence`, and `matchReasons`
- persisted caller/callee references surface compact query-time confidence metadata

## Header/Source Unification

- declaration and definition pairs collapse into one logical symbol identity
- representative symbol selection prefers definition over declaration
- declaration and definition locations are both preserved
- representative fallback from source to header does not break canonical relation IDs

## Fixture Coverage

- parser fixture tests cover namespace-qualified calls, overload arity, sibling methods, and split declaration/definition
- resolver fixture tests cover ranked disambiguation and ambiguous outcomes on `samples/ambiguity`
- storage tests cover exact lookup with duplicate short names
- MCP and HTTP contract tests cover exact lookup and heuristic ambiguity on fixture-backed data
