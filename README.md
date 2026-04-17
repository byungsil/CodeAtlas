# CodeAtlas

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

## MCP Tools

When registered as an MCP server, CodeAtlas exposes four tools:

### `lookup_function`

Look up a function or method by name. Returns symbol definition, callers, and callees.

```json
{ "name": "UpdateAI" }
```

### `lookup_class`

Look up a class or struct by name. Returns class definition and member list.

```json
{ "name": "GameObject" }
```

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
| `GET /function/:name` | Function/method lookup with callers and callees |
| `GET /class/:name` | Class/struct lookup with members |
| `GET /search?q=&type=&limit=` | Symbol search |
| `GET /callgraph/:name?depth=` | Call graph |
| `GET /dashboard/` | Web dashboard |

All responses are structured JSON with workspace-relative file paths.

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
