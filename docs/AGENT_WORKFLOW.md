# CodeAtlas Agent Workflow Guide

## Overview

CodeAtlas provides structured code intelligence through MCP tools. AI agents query CodeAtlas instead of scanning raw source files.

Flow: **Agent -> MCP tool call -> structured JSON -> reasoning**

## Setup

### 1. Index the workspace

```bash
codeatlas-indexer <workspace-root>        # incremental (default)
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

### `lookup_function`

Look up a function or method by name.

**Input:** `{ "name": "UpdateAI" }`

**Output:** Symbol definition + callers + callees.

**Use when:** You need to understand what a function does, who calls it, and what it calls.

### `lookup_class`

Look up a class or struct by name.

**Input:** `{ "name": "GameObject" }`

**Output:** Class definition + member list.

**Use when:** You need to understand a class's interface and its members.

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

```
1. search_symbols({ query: "Update", type: "method" })
   -> Find the exact symbol: Game::GameObject::Update

2. lookup_function({ name: "Update" })
   -> See callers: GameWorld::UpdateAll, AIComponent::ProcessIdle, ProcessPatrol, ProcessChase, ProcessAttack
   -> See callees: (none in this case)

3. For each caller, lookup_function({ name: "<caller>" })
   -> Trace the impact chain upward
```

### 2. Symbol Lookup

> "What is AnimationDatabase and what can it do?"

```
1. lookup_class({ name: "AnimationDatabase" })
   -> Class definition + all public methods

2. For interesting methods, lookup_function({ name: "<method>" })
   -> See what each method calls and who calls it
```

### 3. Call Graph Inspection

> "What happens when UpdateAI runs?"

```
1. get_callgraph({ name: "UpdateAI", depth: 3 })
   -> UpdateAI calls ProcessIdle, ProcessPatrol, ProcessChase, ProcessAttack
   -> Each Process* method calls GameObject::Update or SetPosition

2. Follow specific branches with additional lookup_function calls
```

### 4. Finding Related Code

> "Show me everything related to animation"

```
1. search_symbols({ query: "Animation", limit: 50 })
   -> Classes, functions, methods containing "Animation"

2. Pick relevant results and drill down with lookup_class or lookup_function
```

## Error Handling

| Error | Meaning | Agent Action |
|-------|---------|-------------|
| `NOT_FOUND` | Symbol doesn't exist in the index | Try search_symbols to find the correct name |
| `truncated: true` | More results exist than returned | Narrow the query or increase limit |
| Empty results | No matches | Broaden the search query |
| Stale data | Index hasn't been refreshed | Re-run the indexer or start watch mode |
