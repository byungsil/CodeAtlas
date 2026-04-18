# CodeAtlas API Contract

Base URL: `http://localhost:3000`

---

## Data Models

### Symbol

| Field         | Type     | Description                          |
|---------------|----------|--------------------------------------|
| id            | string   | Canonical exact symbol identifier    |
| name          | string   | Short name                           |
| qualifiedName | string   | Canonical exact qualified name       |
| type          | string   | One of: function, method, class, struct, enum, namespace, variable, typedef |
| filePath      | string   | Relative path from workspace root    |
| line          | number   | Start line (1-based)                 |
| endLine       | number   | End line (1-based)                   |
| signature     | string?  | Full signature (optional)            |
| parentId      | string?  | Parent symbol ID (optional)          |

**Exact identity rules:**

- `id` is the canonical exact identity for a symbol.
- `qualifiedName` is the canonical exact human-readable name.
- In the current contract, `id` and `qualifiedName` are expected to be identical for exact-match purposes.
- `name` is not an exact identifier and must be treated as exploratory or heuristic only.
- Declaration and definition pairs must resolve to one logical symbol identity, with file/line metadata attached to that logical symbol.
- Declaration-only and definition-only symbols are both valid exact lookup targets as long as they have a canonical `id`.
- When declaration and definition coexist, the API contract is one logical symbol, not two separate exact symbols.
- Inline/header-only callable implementations are represented as one logical symbol with inline-definition lifecycle semantics.

### Call

| Field    | Type   | Description                |
|----------|--------|----------------------------|
| callerId | string | Caller symbol ID           |
| calleeId | string | Callee symbol ID           |
| filePath | string | File where the call occurs |
| line     | number | Line of the call site      |

### FileRecord

| Field        | Type   | Description                    |
|--------------|--------|--------------------------------|
| path         | string | Relative file path             |
| contentHash  | string | SHA-256 of file content        |
| lastIndexed  | string | ISO 8601 timestamp             |
| symbolCount  | number | Number of symbols in this file |

### Internal Normalized Relation Event

This is an internal indexer contract for Milestone 2. It is not yet part of the public HTTP or MCP response surface.

| Field      | Type   | Description |
|------------|--------|-------------|
| relationKind | string | One of: `call`, `typeUsage`, `inheritance` |
| source     | string | Extraction origin: `legacyAst` or `treeSitterGraph` |
| confidence | string | Extraction confidence: `high` or `partial` |
| callerId   | string? | Source symbol ID for relation events that originate from a symbol body |
| targetName | string? | Unresolved target token for call-like events |
| callKind   | string? | One of: `unqualified`, `memberAccess`, `pointerMemberAccess`, `thisPointerAccess`, `qualified` |
| argumentCount | number? | Argument count hint for call-like events |
| receiver   | string? | Receiver token or normalized receiver text |
| receiverKind | string? | Receiver classification hint |
| qualifier  | string? | Namespace or type qualifier text |
| qualifierKind | string? | Qualifier classification hint |
| filePath   | string | Relative file path |
| line       | number | 1-based source line |

Milestone 2 intent:

- `RawRelationEvent` is the normalized extraction contract between parser-like extraction and later resolver/storage stages.
- During the transition, only `relationKind = call` is projected into the existing `RawCallSite` resolver input.
- `source` and `confidence` are tracked so graph-derived extraction can be compared against legacy AST extraction without changing public API behavior yet.

---

## Endpoints

Note:

- The legacy HTTP API remains name-oriented for `/function/:name` and `/class/:name`.
- The canonical exact lookup path is `GET /symbol`.
- `search` is the exploratory discovery path when exact identity is not yet known.

---

### GET /symbol

Retrieve one symbol by canonical exact identity.

**Parameters:**

- `id` (query, optional) — Canonical exact symbol identity.
- `qualifiedName` (query, optional) — Canonical exact human-readable symbol identity.

**Parameter rules:**

- At least one of `id` or `qualifiedName` is required.
- In the current contract, `id` and `qualifiedName` identify the same logical symbol.
- If both are provided and do not identify the same logical symbol, the request is invalid.
- `GET /symbol` must not fall back to short-name matching.

**Response 200:**

```json
{
  "lookupMode": "exact",
  "symbol": { "id": "Game::GameObject::Update", "qualifiedName": "Game::GameObject::Update" }
}
```

Depending on symbol kind, the payload may also include:

- `callers`
- `callees`
- `members`

**Response 400:**

```json
{ "error": "Invalid exact lookup request", "code": "BAD_REQUEST" }
```

**Response 404:**

```json
{ "error": "Symbol not found", "code": "NOT_FOUND" }
```

### GET /function/:name

Retrieve a function or method by name.

**Parameters:**
- `name` (path) — Symbol name to look up. Matches against `name` field.

**Response 200:**

```json
{
  "lookupMode": "heuristic",
  "symbol": { Symbol },
  "confidence": "high_confidence_heuristic",
  "matchReasons": [],
  "callers": [ CallReference ],
  "callees": [ CallReference ]
}
```

**CallReference:**

| Field         | Type   | Description             |
|---------------|--------|-------------------------|
| symbolId      | string | Related symbol ID       |
| symbolName    | string | Short name              |
| qualifiedName | string | Fully qualified name    |
| filePath      | string | File path               |
| line          | number | Line number             |
| confidence    | string | Confidence level        |
| matchReasons  | array  | Structural match reasons |

**Response 404:**

```json
{ "error": "Symbol not found", "code": "NOT_FOUND" }
```

**Notes:**
- If multiple symbols match the name, returns the first match. Use qualified name for precision.
- Name-only lookup is heuristic. Exact lookup should use canonical symbol identity once exposed by the API surface.
- `GET /function/:name` is not the canonical exact lookup path.
- Heuristic lookup responses now expose:
  - `lookupMode`
  - `confidence`
  - `matchReasons`
  - optional `ambiguity` when multiple short-name candidates exist
- Current ambiguity behavior:
  - the endpoint remains backward compatible and still returns one selected symbol payload
  - when multiple short-name candidates exist, callers must treat the result as heuristic rather than exact
  - ambiguity is surfaced through `confidence = "ambiguous"`, `matchReasons = ["ambiguous_top_score"]`, and `ambiguity.candidateCount`

---

### GET /class/:name

Retrieve a class or struct and its members.

**Parameters:**
- `name` (path) — Class name to look up.

**Response 200:**

```json
{
  "lookupMode": "heuristic",
  "symbol": { Symbol },
  "confidence": "high_confidence_heuristic",
  "matchReasons": [],
  "members": [ Symbol ]
}
```

**Response 404:**

```json
{ "error": "Symbol not found", "code": "NOT_FOUND" }
```

**Notes:**

- `GET /class/:name` is name-based and heuristic when duplicate short names exist.
- `GET /class/:name` is not the canonical exact lookup path.
- Heuristic lookup responses may include `ambiguity` metadata when multiple short-name candidates exist.

**Lookup mode guidance:**

- Use `GET /symbol` when exact symbol identity is already known.
- Use `GET /search` when the exact identity is not yet known.
- Use `GET /function/:name` and `GET /class/:name` as convenience heuristics for exploratory workflows, then switch to exact identity once the intended `qualifiedName` is known.

---

### GET /search?q=

Search symbols by name substring.

**Parameters:**
- `q` (query) — Search query string. Minimum length: 3 characters.
- `type` (query, optional) — Filter by symbol type.
- `limit` (query, optional) — Max results. Default: 50. Max: 200.

**Response 200:**

```json
{
  "query": "Update",
  "results": [ Symbol ],
  "totalCount": 3,
  "truncated": false
}
```

**Notes:**
- `truncated: true` when `totalCount > limit`.
- Queries shorter than 3 characters return an empty result set.
- Case-insensitive substring match on `name` and `qualifiedName`.

---

### GET /callgraph/:name

Retrieve the call graph rooted at a symbol.

**Parameters:**
- `name` (path) — Root symbol name.
- `depth` (query, optional) — Max traversal depth. Default: 3. Max: 10.

**Response 200:**

```json
{
  "root": {
    "symbol": { id, name, qualifiedName, type, filePath, line },
    "callees": [
      {
        "targetId": "string",
        "targetName": "string",
        "targetQualifiedName": "string",
        "filePath": "string",
        "line": 0
      }
    ]
  },
  "depth": 1,
  "maxDepth": 3,
  "truncated": false
}
```

**Notes:**
- `truncated: true` when the graph was cut short at `maxDepth` and unexplored edges remain.
- Depth 1 returns only direct callees. Depth 2 includes callees-of-callees, etc.

**Response 404:**

```json
{ "error": "Symbol not found", "code": "NOT_FOUND" }
```

---

## Common Behavior

- All responses are `Content-Type: application/json`.
- All file paths are relative to the indexed workspace root.
- Line numbers are 1-based.
- Symbol IDs use a canonical exact qualified-path format such as `Namespace::Class::Method`.
- `qualifiedName` uses the same canonical exact format as `id` in the current contract.
- Partial results are always explicitly marked with `truncated: true`.

## Lookup Mode Guidance

Recommended progression:

1. Use `search_symbols` / `GET /search` to discover likely candidates.
2. Use `lookup_function` / `lookup_class` or their HTTP equivalents only as heuristic convenience lookups.
3. Once the intended symbol identity is known, switch to `lookup_symbol` / `GET /symbol`.

In Milestone 1, name-based lookup remains intentionally backward compatible and may still return a selected symbol even when duplicate short names exist. That selected result must not be treated as exact unless it came from the exact lookup path.

## Confidence Taxonomy

- `exact`
  - canonical exact identity lookup succeeded
- `high_confidence_heuristic`
  - a unique top-ranked structural candidate was chosen
- `ambiguous`
  - multiple candidates share the top score
- `unresolved`
  - no viable candidate was found

Milestone 1 match-reason vocabulary:

- `exact_id_match`
- `exact_qualified_name_match`
- `same_parent_match`
- `same_namespace_match`
- `this_receiver_match`
- `member_call_prefers_method`
- `qualified_type_match`
- `qualified_namespace_match`
- `parameter_count_match`
- `signature_arity_hint`
- `ambiguous_top_score`
- `no_viable_candidate`

Persistence note:

- Milestone 1 treats confidence and match reasons as query-time derived metadata, not persisted source-of-truth fields in the database.
- Persisted storage remains canonical-symbol and canonical-edge oriented.
- API lookup payloads surface confidence at the top level for the selected symbol lookup result.
- `CallReference` payloads surface confidence for persisted resolved relations; in Milestone 1 these are emitted as `high_confidence_heuristic` unless a future confidence-aware query path can reattach richer reasons.

## Structural Confidence Semantics

What confidence means:

- CodeAtlas confidence describes how strongly the current structural evidence supports a returned result.
- Confidence is about lookup and relation-selection semantics inside CodeAtlas, not about full C++ semantic completeness.
- `exact` means canonical symbol targeting succeeded through exact identity input.
- `high_confidence_heuristic` means CodeAtlas selected one structurally strongest candidate from heuristic lookup or persisted resolved relations.
- `ambiguous` means multiple candidates remain plausible at the current query surface.
- `unresolved` means CodeAtlas does not currently have a viable structural answer for that query path.

What confidence does not mean:

- `exact` does not mean compiler-grade truth about templates, macros, overload sets, or build-configuration-specific semantics.
- `high_confidence_heuristic` does not mean the answer is guaranteed correct in every C++ edge case.
- `ambiguous` does not mean the index is broken; it means the current structural evidence does not justify pretending there is one exact winner.
- `unresolved` does not necessarily mean the symbol is absent from the codebase; it may mean the current index or query path cannot safely resolve it.

Interpretation guidance:

- prefer exact identity lookup whenever a canonical `qualifiedName` or `id` is known
- treat heuristic lookup as a navigation aid, not as canonical truth
- when `confidence = ambiguous`, refine the query or switch to exact identity lookup
- when `confidence = unresolved`, search for nearby candidates or refresh the index before drawing conclusions

Examples:

- exact result:
  - `GET /symbol?qualifiedName=Game::GameObject::Update`
  - `lookupMode = exact`
  - `confidence = exact`
- heuristic ambiguous result:
  - `GET /function/Update`
  - `lookupMode = heuristic`
  - `confidence = ambiguous`
  - `matchReasons = ["ambiguous_top_score"]`
- heuristic resolved relation:
  - persisted caller/callee reference
  - `confidence = high_confidence_heuristic`

---

## MCP Exact Lookup Contract

### Tool: `lookup_symbol`

Retrieve one symbol by canonical exact identity.

**Arguments:**

- `id` (optional) — canonical exact symbol identity
- `qualifiedName` (optional) — canonical exact human-readable symbol identity

**Argument rules:**

- At least one of `id` or `qualifiedName` is required.
- In the current contract, `id` and `qualifiedName` identify the same logical symbol.
- If both are provided and do not identify the same logical symbol, the request is invalid.
- `lookup_symbol` must not fall back to short-name matching.

**Success payload:**

```json
{
  "lookupMode": "exact",
  "symbol": {
    "id": "Game::GameObject::Update",
    "qualifiedName": "Game::GameObject::Update"
  }
}
```

Depending on symbol kind, the payload may also include:

- `callers`
- `callees`
- `members`

**Error payloads:**

```json
{ "error": "Symbol not found", "code": "NOT_FOUND" }
```

```json
{ "error": "Invalid exact lookup request", "code": "BAD_REQUEST" }
```

**Legacy MCP tools:**

- `lookup_function` and `lookup_class` remain available
- they are name-based and heuristic when duplicate short names exist
- they are not the canonical exact lookup path

## Error Codes

| Code           | HTTP Status | Description               |
|----------------|-------------|---------------------------|
| NOT_FOUND      | 404         | Symbol not found          |
| BAD_REQUEST    | 400         | Invalid query parameters  |
| INTERNAL_ERROR | 500         | Server-side failure       |
