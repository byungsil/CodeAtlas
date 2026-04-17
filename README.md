# CodeAtlas

[![GitHub](https://img.shields.io/badge/GitHub-byungsil%2FCodeAtlas-blue)](https://github.com/byungsil/CodeAtlas)

**Repository:** https://github.com/byungsil/CodeAtlas.git

CodeAtlas is a local AI-powered code intelligence system for large-scale C++ codebases. It indexes source code with Tree-sitter and serves structured queries through MCP, so AI agents can reason about code without scanning raw files.

## Architecture

```
C++ Source Code
      |
Rust Indexer (Tree-sitter, parallel)
      |
SQLite Index DB
      |
Node.js MCP Server
      |
AI Agents / Web Dashboard
```

## Prerequisites

- **Rust** (1.75+) - for the indexer
- **Node.js** (18+) - for the MCP server and dashboard
- **npm** - for package management

## Quick Start

### 0. Clone the repository

```bash
git clone https://github.com/byungsil/CodeAtlas.git
cd CodeAtlas
```

### 1. Build the indexer

```bash
cd indexer
cargo build --release
```

### 2. Install server dependencies

```bash
cd server
npm install
```

### 3. Index your workspace

```bash
# Full index
./indexer/target/release/codeatlas-indexer <workspace-root> --full

# Incremental (default - only re-indexes changed files)
./indexer/target/release/codeatlas-indexer <workspace-root>

# Watch mode (auto re-index on file changes)
./indexer/target/release/codeatlas-indexer watch <workspace-root>
```

The indexer creates a `.codeatlas/index.db` SQLite database inside the workspace root. The database uses a dual-table architecture (`symbols_raw` for per-file parsed symbols, `symbols` for merged representatives) to support correct incremental updates when header/source pairs change independently.

For large real-world C++ repositories, indexing scope matters a lot. If the workspace contains heavy `tests/`, `docs/`, generated code, or vendored mirrors, add a `.codeatlasignore` before relying on heuristic lookup. This keeps the symbol set focused on the code agents actually need to reason about.

### 4. Start the HTTP server (standalone)

```bash
cd server
CODEATLAS_PORT=3000 npx ts-node src/index.ts <workspace-root>/.codeatlas
```

Open `http://localhost:3000/dashboard/` for the web UI.

### 5. Register as an MCP server (for AI agents)

Add to `.claude/settings.json`:

```json
{
  "mcpServers": {
    "codeatlas": {
      "command": "npx",
      "args": ["ts-node", "<path-to>/server/src/mcp.ts", "<workspace-root>/.codeatlas"],
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

With `CODEATLAS_WATCHER=true`, the MCP server automatically launches the Rust watcher as a child process. File changes are re-indexed without a separate terminal session.

Set `CODEATLAS_DASHBOARD_AUTOOPEN` to `"true"` to auto-open the dashboard in a browser when the MCP server starts.

## Configuration

All configuration is through environment variables, set in the MCP client config or shell:

| Variable | Default | Description |
|----------|---------|-------------|
| `CODEATLAS_WORKSPACE` | *(inferred from data dir)* | Explicit workspace root path. Takes precedence over inference. |
| `CODEATLAS_PORT` | `3000` | HTTP server / dashboard port |
| `CODEATLAS_DASHBOARD_AUTOOPEN` | `false` | Auto-open dashboard in browser on MCP start |
| `CODEATLAS_WATCHER` | `false` | Auto-start watcher as child process on MCP start |
| `CODEATLAS_INDEXER_PATH` | `codeatlas-indexer` | Path to the Rust indexer binary |
| `CODEATLAS_DATA` | `.codeatlas` | Data directory (fallback if no CLI arg) |

## Indexer CLI

```
codeatlas-indexer <workspace-root>              # incremental index
codeatlas-indexer <workspace-root> --full        # full rebuild
codeatlas-indexer <workspace-root> --full --json  # full rebuild + JSON output
codeatlas-indexer watch <workspace-root>          # watch mode
```

Supported file extensions: `.cpp`, `.h`, `.hpp`, `.cc`, `.cxx`, `.inl`, `.inc`

## Ignore Rules

Create a `.codeatlasignore` file in the workspace root to exclude files/folders from indexing. Each line is a regex pattern matched against workspace-relative paths (forward slashes).

```
# Exclude test tooling and generated code
^tools/
^testbed/
_test\.cpp$
^generated/
```

- Blank lines and lines starting with `#` are ignored.
- Invalid regex patterns are silently skipped.
- Ignore rules apply consistently to full indexing, incremental indexing, and watcher events.

Recommended onboarding flow for large C++ repositories:

1. Run one full index.
2. Inspect which top-level directories dominate the symbol count.
3. Add `.codeatlasignore` entries for irrelevant trees such as tests, docs, generated code, vendored mirrors, and build output.
4. Reindex before evaluating heuristic lookup quality.

Practical starter preset:

```gitignore
# Keep the index focused on production code first
^tests/
^docs/
^generated/
^build/
^out/
```

This does not affect exact lookup semantics, but it can dramatically improve heuristic lookup quality by reducing noisy duplicate symbols.

## MCP Tools

When registered as an MCP server, CodeAtlas currently exposes five core tools:

- `lookup_symbol` for canonical exact identity lookup by `id` or `qualifiedName`
- `lookup_function` and `lookup_class` as backward-compatible name-based convenience tools
- `search_symbols`
- `get_callgraph`

`lookup_symbol` is the canonical exact MCP lookup path. `lookup_function` and `lookup_class` remain heuristic when duplicate short names exist.

Recommended usage flow:

1. Use `search_symbols` to discover candidates.
2. Use `lookup_function` / `lookup_class` only as heuristic convenience lookups.
3. Switch to `lookup_symbol` once the intended `qualifiedName` or `id` is known.

### `lookup_symbol`

Look up one symbol by canonical exact identity. Never falls back to short-name heuristics.

```json
{ "qualifiedName": "Game::GameObject::Update" }
```

Alternative exact input:

```json
{ "id": "Game::GameObject::Update" }
```

If both are supplied, they must identify the same logical symbol:

```json
{ "id": "Game::GameObject::Update", "qualifiedName": "Game::GameObject::Update" }
```

Successful responses include:

- `lookupMode: "exact"`
- `confidence: "exact"`
- `matchReasons`
- `symbol`
- `callers` / `callees` for callable symbols
- `members` for class or struct symbols

### `lookup_function`

Look up a function or method by name. Returns symbol definition, callers, and callees.

```json
{ "name": "UpdateAI" }
```

This path is heuristic. Responses include `lookupMode`, `confidence`, `matchReasons`, and optional `ambiguity` when duplicate short names exist.

If duplicate short names exist, the response stays backward compatible by returning one selected symbol, but callers should treat it as heuristic unless they switch to `lookup_symbol`.

### `lookup_class`

Look up a class or struct by name. Returns class definition and member list.

```json
{ "name": "GameObject" }
```

This path is heuristic. Responses include `lookupMode`, `confidence`, `matchReasons`, and optional `ambiguity` when duplicate short names exist.

### `search_symbols`

Search symbols by name substring (min 3 characters). Queries shorter than 3 characters return an empty result set.

```json
{ "query": "Update", "type": "method", "limit": 20 }
```

### `get_callgraph`

Get the call graph rooted at a function. Expands callees recursively up to the requested depth with cycle detection.

```json
{ "name": "UpdateAI", "depth": 3 }
```

## HTTP API

The same queries are available as REST endpoints:

| Endpoint | Description |
|----------|-------------|
| `GET /symbol?id=&qualifiedName=` | Canonical exact symbol lookup |
| `GET /function/:name` | Function/method lookup with callers and callees |
| `GET /class/:name` | Class/struct lookup with members |
| `GET /search?q=&type=&limit=` | Symbol search |
| `GET /callgraph/:name?depth=` | Call graph |
| `GET /dashboard/` | Web dashboard |

All responses are structured JSON with workspace-relative file paths.

`GET /symbol` is the canonical HTTP exact lookup path. `GET /function/:name` and `GET /class/:name` remain name-based convenience endpoints.

Exact lookup responses include:

- `lookupMode: "exact"`
- `confidence: "exact"`
- `matchReasons`

Heuristic lookup responses from `/function/:name` and `/class/:name` include:

- `lookupMode: "heuristic"`
- `confidence`
- `matchReasons`
- optional `ambiguity`

Recommended HTTP usage flow:

1. Use `GET /search` for discovery.
2. Use `/function/:name` and `/class/:name` as convenience heuristics.
3. Use `GET /symbol` for deterministic exact targeting.

## Web Dashboard

The dashboard provides a browser UI for symbol search, function/class detail views, and call graph visualization. It consumes the same HTTP API endpoints as the MCP tools.

Access at `http://localhost:<CODEATLAS_PORT>/dashboard/` when the HTTP server is running.

## Project Structure

```
indexer/           Rust indexer (Tree-sitter + rayon + SQLite)
  src/
    main.rs        CLI entry point
    parser.rs      Tree-sitter AST extraction
    discovery.rs   File discovery
    ignore.rs      .codeatlasignore regex-based filtering
    constants.rs   Shared constants
    resolver.rs    Cross-file symbol merge + call resolution
    incremental.rs Hash-based incremental planning
    indexing.rs    Shared file parsing utilities
    watcher.rs     Filesystem watcher (notify)
    storage.rs     SQLite read/write (symbols_raw + symbols dual-table)
    models.rs      Data models

server/            Node.js MCP server + HTTP API + Dashboard
  src/
    mcp.ts         MCP server (stdio transport)
    app.ts         Express HTTP app
    index.ts       Standalone HTTP server entry
    config.ts      Environment-based configuration
    parser/        Tree-sitter C++ parser (Node, used in Phase 1)
    storage/       SQLite + JSON store implementations
    models/        TypeScript data contracts
  public/
    index.html     Dashboard UI

docs/              Documentation
samples/           Sample C++ workspace for testing
```

## Testing

```bash
# Rust tests (39 tests)
cd indexer
cargo test

# Node tests (62 tests)
cd server
npx jest
```

## Performance

Benchmarked on a 25,962-file C++ codebase (gameplay):

| Metric | Value |
|--------|-------|
| Full index time | ~108s |
| Incremental (no changes) | 0ms (skipped) |
| SQLite DB size | 368 MB |
| Query latency | <50ms |
| Symbols extracted | 171,965 |
| Call relationships | 900,206 |
