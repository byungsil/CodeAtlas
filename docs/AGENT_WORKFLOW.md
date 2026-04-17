# CodeAtlas Agent Workflow Guide

## Overview

CodeAtlas provides structured code intelligence through MCP tools. AI agents query CodeAtlas instead of scanning raw source files.

Flow: **Agent -> MCP tool call -> structured JSON -> reasoning**

## Setup

### 1. Index the workspace

```bash
codeatlas-indexer <workspace-root>         # incremental (default)
codeatlas-indexer <workspace-root> --full  # full rebuild
codeatlas-indexer watch <workspace-root>   # auto-reindex on file changes
```

### 2. Start the MCP server

Add to your Claude Code MCP config (`.claude/settings.json`):

```json
{
  "mcpServers": {
    "codeatlas": {
      "command": "npx",
      "args": ["ts-node", "server/src/mcp.ts", "<workspace-root>/.codeatlas"],
      "env": {
        "CODEATLAS_WORKSPACE": "<workspace-root>",
        "CODEATLAS_PORT": "3000",
        "CODEATLAS_DASHBOARD_AUTOOPEN": "false",
        "CODEATLAS_WATCHER": "true",
        "CODEATLAS_INDEXER_PATH": "<path-to>/indexer/target/release/codeatlas-indexer"
      }
    }
  }
}
```

## Available Tools

### `lookup_symbol`

Look up a symbol by canonical exact identity.

**Input:** `{ "qualifiedName": "Game::GameObject::Update" }`

Alternative input:

```json
{ "id": "Game::GameObject::Update" }
```

Combined exact input:

```json
{ "id": "Game::GameObject::Update", "qualifiedName": "Game::GameObject::Update" }
```

**Behavior:**

- `lookup_symbol` is the canonical exact MCP lookup tool.
- At least one of `id` or `qualifiedName` is required.
- In the current contract, `id` and `qualifiedName` refer to the same logical symbol identity.
- If both are supplied and they do not identify the same logical symbol, the tool returns `BAD_REQUEST`.
- `lookup_symbol` never falls back to heuristic short-name matching.

**Output:**

- always returns `symbol`
- returns `callers` and `callees` for function or method symbols
- returns `members` for class or struct symbols
- returns `lookupMode: "exact"`
- returns `confidence: "exact"`
- returns `matchReasons`

**Use when:** You already know the exact symbol identity and need deterministic lookup.

### `lookup_function`

Look up a function or method by name.

**Input:** `{ "name": "UpdateAI" }`

**Output:** Symbol definition + callers + callees.

**Use when:** You need to understand what a function does, who calls it, and what it calls.

**Important:** `name`-based lookup is heuristic when duplicate short names exist. Prefer `lookup_symbol` for deterministic exact lookup. Heuristic responses include `lookupMode`, `confidence`, `matchReasons`, and optional `ambiguity`.

### `lookup_class`

Look up a class or struct by name.

**Input:** `{ "name": "GameObject" }`

**Output:** Class definition + member list.

**Use when:** You need to understand a class's interface and its members.

**Important:** `name`-based lookup is heuristic when duplicate short names exist. Prefer `lookup_symbol` for deterministic exact lookup. Heuristic responses include `lookupMode`, `confidence`, `matchReasons`, and optional `ambiguity`.

### `search_symbols`

Search symbols by name substring.

Minimum query length is 3 characters.

**Input:** `{ "query": "Update", "type": "method", "limit": 20 }`

**Output:** Matching symbols with `truncated` indicator.

**Use when:** You're looking for symbols but don't know the exact name.

### `get_callgraph`

Get the call graph rooted at a function.

**Input:** `{ "name": "UpdateAI", "depth": 3 }`

**Output:** Call graph tree with callees.

**Use when:** You need to trace execution flow or analyze impact.

## Example Workflows

### 1. Impact Analysis

> "What would break if I change `GameObject::Update`?"

```text
1. search_symbols({ query: "Update", type: "method" })
   -> Find the exact symbol: Game::GameObject::Update

2. Call lookup_symbol({ qualifiedName: "Game::GameObject::Update" })
   -> Deterministic exact lookup for the intended symbol

3. Inspect callers and callees from the exact response
   -> Use the returned qualified identities to continue the impact trace

4. If you started with a name-only lookup, verify the returned `qualifiedName`
   -> Treat it as heuristic unless the ambiguity metadata says otherwise
```

### Exact vs Exploratory Lookup

Use this progression when the user asks about a symbol but exact identity is not yet known:

```text
1. search_symbols({ query: "Update", type: "method" })
   -> discovery step

2. lookup_function({ name: "Update" })
   -> heuristic convenience step
   -> inspect `lookupMode`, `confidence`, `matchReasons`, and optional `ambiguity`

3. lookup_symbol({ qualifiedName: "<chosen-qualified-name>" })
   -> exact targeting step
```

If the heuristic step returns `confidence = ambiguous`, do not treat the selected symbol as exact. Use the returned names to refine the query or switch to canonical identity lookup.

### 2. Symbol Lookup

> "What is AnimationDatabase and what can it do?"

```text
1. lookup_class({ name: "AnimationDatabase" })
   -> Class definition + members

2. If duplicate short names are possible, switch to lookup_symbol with the returned qualified identity

3. For interesting methods, use lookup_function or lookup_symbol
   -> See what each method calls and who calls it
```

### 3. Call Graph Inspection

> "What happens when UpdateAI runs?"

```text
1. get_callgraph({ name: "UpdateAI", depth: 3 })
   -> Traverses callees up to the requested depth

2. Follow specific branches with additional lookup_symbol or lookup_function calls
```

### 4. Finding Related Code

> "Show me everything related to animation"

```text
1. search_symbols({ query: "Animation", limit: 50 })
   -> Classes, functions, methods containing "Animation"

2. Pick relevant results and drill down with lookup_symbol, lookup_class, or lookup_function
```

## Error Handling

| Error | Meaning | Agent Action |
|-------|---------|-------------|
| `BAD_REQUEST` | Exact lookup request is invalid | Re-check `id` / `qualifiedName` inputs |
| `NOT_FOUND` | Symbol doesn't exist in the index | Try `search_symbols` to find the correct name |
| `truncated: true` | More results exist than returned | Narrow the query or increase limit |
| Empty results | No matches | Broaden the search query |
| Stale data | Index hasn't been refreshed | Re-run the indexer or start watch mode |

## Identity Guidance

- Treat `id` as the canonical exact symbol identity.
- Treat `qualifiedName` as the canonical exact human-readable name.
- In the current CodeAtlas contract, exact lookup should assume `id` and `qualifiedName` refer to the same logical symbol identity.
- Prefer `lookup_symbol` whenever exact identity is known.
- Treat short `name` lookups as exploratory only unless the result is otherwise disambiguated.

## Confidence Guidance

- Interpret `exact` as exact symbol targeting, not as proof of full compiler-grade semantic completeness.
- Interpret `high_confidence_heuristic` as a structurally well-supported best candidate chosen by ranking.
- Interpret `ambiguous` as "do not assume one winner"; refine the query or gather more context.
- Interpret `unresolved` as "no viable structural answer was found from the current index state".

Common Milestone 1 structural reasons:

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

Implementation note:

- In Milestone 1, these confidence labels and reasons should be understood as query-time interpretation metadata rather than permanently stored DB fields.

## Confidence Usage Rules

Use these rules when reasoning from CodeAtlas output:

- If `lookupMode = exact`, trust that the intended logical symbol was targeted, but do not over-interpret that as full compiler semantic certainty.
- If `confidence = high_confidence_heuristic`, treat the result as a strong structural lead that may still need confirmation in tricky C++ cases.
- If `confidence = ambiguous`, do not summarize the answer as if CodeAtlas found one definitive winner.
- If `confidence = unresolved`, say that CodeAtlas does not currently have a viable structural answer rather than inventing one.

Suggested agent behavior:

1. Prefer exact identity lookup whenever a canonical `qualifiedName` is already known.
2. Use heuristic lookup to narrow the search space when exact identity is not known yet.
3. Escalate to `search_symbols` or exact lookup when ambiguity remains visible.
4. Avoid presenting heuristic results as exact unless the confidence mode explicitly supports that claim.

Examples:

- Exact:
  - `lookup_symbol({ qualifiedName: "Game::GameObject::Update" })`
  - interpret as exact symbol targeting
- Ambiguous:
  - `lookup_function({ name: "Update" })`
  - if `confidence = ambiguous`, refine before concluding
- Unresolved:
  - exact lookup returns `NOT_FOUND`
  - report that CodeAtlas could not currently resolve the requested symbol
