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

# Restrict indexing to selected extensions and raise the C/C++ parse timeout to 60s
./indexer/target/release/codeatlas-indexer <workspace-root> --extensions cpp,h,hpp --parse-timeout-ms 60000

# Watch mode (auto re-index on file changes)
./indexer/target/release/codeatlas-indexer watch <workspace-root>

# Watch mode with a restricted extension set and 60s C/C++ parse timeout
./indexer/target/release/codeatlas-indexer watch <workspace-root> --extensions cpp,h,hpp --parse-timeout-ms 60000
```

The indexer creates a `.codeatlas/index.db` SQLite database inside the workspace root. The database uses a dual-table architecture (`symbols_raw` for per-file parsed symbols, `symbols` for merged representatives) to support correct incremental updates when header/source pairs change independently.

On Windows, large freshly written SQLite files can be touched briefly by file indexers or antivirus scanners. CodeAtlas now retries direct opens and falls back to a read-only snapshot when needed, but for the most stable operation you should exclude the workspace-local `.codeatlas/` directory from tools such as Everything, Windows Search, and Defender.

For large real-world C++ repositories, indexing scope matters a lot. If the workspace contains heavy `tests/`, `docs/`, `dev_docs/`, generated code, or vendored mirrors, add a `.codeatlasignore` before relying on heuristic lookup. This keeps the symbol set focused on the code agents actually need to reason about.

CodeAtlas also protects full and incremental indexing from pathological C/C++ files that are technically parseable text files but are not useful source inputs, such as embedded binary dumps or giant numeric lookup-table headers. By default, C/C++ files larger than `2 MB` are skipped when they do not show enough real source-structure markers, and smaller files can also be skipped when they look like binary-like or numeric-blob payloads instead of code. Override the size threshold with `CODEATLAS_SKIP_CPP_LARGER_THAN_BYTES=<bytes>`, or set it to `0` to disable the threshold entirely.

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

Common MCP client locations:

- Claude Code: `.claude/settings.json` or `.claude/settings.local.json`
- VS Code workspace: `.vscode/mcp.json`
- Visual Studio: `<solution>/.mcp.json`, `<solution>/.vs/mcp.json`, or `%USERPROFILE%/.mcp.json`
- Codex CLI / Codex IDE extension: `~/.codex/config.toml`
- Antigravity: `%APPDATA%/Antigravity/User/mcp.json`

For workspace-specific setup, prefer keeping the server pointed at `<workspace-root>/.codeatlas` from a repo-local config such as `.vscode/mcp.json` or `<solution>/.mcp.json` so different projects do not accidentally share the wrong index.

## Configuration

All configuration is through environment variables, set in the MCP client config or shell:

| Variable | Default | Description |
|----------|---------|-------------|
| `CODEATLAS_WORKSPACE` | *(inferred from data dir)* | Explicit workspace root path. Takes precedence over inference. |
| `CODEATLAS_PORT` | `3000` | HTTP server / dashboard port |
| `CODEATLAS_DASHBOARD_AUTOOPEN` | `false` | Auto-open dashboard in browser on MCP start |
| `CODEATLAS_WATCHER` | `false` | Auto-start watcher as child process on MCP start |
| `CODEATLAS_INDEXER_PATH` | `codeatlas-indexer` | Path to the Rust indexer binary |
| `CODEATLAS_INDEXER_STACK_BYTES` | `67108864` | Worker-thread stack size for the Rust indexer thread pool |
| `CODEATLAS_INDEX_EXTENSIONS` | `.c,.cpp,.h,.hpp,.cc,.cxx,.inl,.inc,.lua,.py,.ts,.tsx,.rs` | Default extension allowlist. CLI `--extensions` overrides it for the current run. |
| `CODEATLAS_CPP_PARSE_TIMEOUT_MICROS` | `60000000` | Per-file C/C++ Tree-sitter timeout in microseconds. CLI `--parse-timeout-ms` overrides it for the current run. |
| `CODEATLAS_SKIP_CPP_LARGER_THAN_BYTES` | `2097152` | Default oversized C/C++ skip threshold. Set to `0` to disable the threshold. |
| `CODEATLAS_DATA` | `.codeatlas` | Data directory (fallback if no CLI arg) |

## Indexer CLI

```
codeatlas-indexer --help                         # show usage
codeatlas-indexer <workspace-root>              # incremental index
codeatlas-indexer <workspace-root> --verbose     # incremental index with per-file logs
codeatlas-indexer <workspace-root> --full        # full rebuild
codeatlas-indexer <workspace-root> --full --verbose  # full rebuild with per-file logs
codeatlas-indexer <workspace-root> --full --json  # full rebuild + JSON output
codeatlas-indexer <workspace-root> --extensions cpp,h,hpp  # only index selected extensions
codeatlas-indexer <workspace-root> --parse-timeout-ms 60000  # 60s per-file C/C++ parse timeout
codeatlas-indexer <workspace-root> --extensions cpp,h,hpp --parse-timeout-ms 60000
codeatlas-indexer watch <workspace-root>          # watch mode
codeatlas-indexer watch <workspace-root> --extensions cpp,h,hpp --parse-timeout-ms 60000
```

Supported file extensions by default: `.c`, `.cpp`, `.h`, `.hpp`, `.cc`, `.cxx`, `.inl`, `.inc`, `.lua`, `.py`, `.ts`, `.tsx`, `.rs`

Large-repository stack safety:

- the Rust indexer now uses a larger internal worker-thread stack by default to survive LLVM-scale repositories without requiring manual `RUST_MIN_STACK` tuning
- override with `CODEATLAS_INDEXER_STACK_BYTES=<bytes>` if a still larger stack is needed for Unreal-engine-class projects
- if `CODEATLAS_INDEXER_STACK_BYTES` is not set, the indexer still honors `RUST_MIN_STACK` when present

Large-repository parse protection:

- full rebuilds now emit stage-level progress so long no-output windows are easier to distinguish from a true hang
- slow-file monitoring remains active in non-verbose runs and reports the currently active long-tail parse files
- the default per-file C/C++ parse timeout is now `60000 ms` (`60 s`)
- `--parse-timeout-ms <ms>` overrides that timeout for a single run; `CODEATLAS_CPP_PARSE_TIMEOUT_MICROS` remains available as an environment-level default
- oversized or blob-like C/C++ files are skipped before Tree-sitter parsing to prevent a small number of pathological headers from stalling the whole run
- the same skip protection applies consistently to full rebuilds, incremental runs, and watch mode

Indexer scope control:

- by default, the indexer scans the full built-in extension set listed above
- `--extensions cpp,h,hpp,py` narrows the run to a comma-separated allowlist
- `CODEATLAS_INDEX_EXTENSIONS` provides the same allowlist behavior through environment configuration when CLI flags are inconvenient
- watch mode uses the same extension allowlist and parse-timeout overrides as full and incremental runs

Mixed-workspace query behavior:

- shared query surfaces such as lookup, search, callers, references, impact, and overview now operate across mixed workspaces
- responses can expose `language` and grouped language summaries so agents can keep one structured workflow across C/C++, Lua, Python, TypeScript, and Rust
- MCP and HTTP also expose workspace-level language distribution through `workspace_summary` / `GET /workspace-summary`

Optional build metadata:

- if a workspace contains `compile_commands.json`, the indexer auto-detects it
- CodeAtlas uses workspace include directories, compile output paths, and cheap define hints to refine metadata such as `headerRole` and `artifactKind`
- the indexer still works normally without a compile database; build metadata is an optional enrichment layer, not a requirement

Risk signaling:

- CodeAtlas also emits lightweight fragility signals such as `parseFragility`, `macroSensitivity`, and `includeHeaviness`
- these signals are heuristic guidance for AI agents, not compiler-grade diagnostics
- they are meant to highlight areas where macro-heavy code, unstable parsing, or heavy include context may justify extra caution

## Benchmark Harness

Repeatable benchmark runs are available through:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\benchmark\Run-CodeAtlasBenchmark.ps1 `
  -WorkspaceRoot E:\Dev\CodeAtlas\samples\ambiguity `
  -Mode full `
  -OutputPath .\dev_docs\benchmark_results\ambiguity-full-debug-baseline.json
```

Representative examples:

```powershell
# Small deterministic fixture
powershell -ExecutionPolicy Bypass -File .\scripts\benchmark\Run-CodeAtlasBenchmark.ps1 `
  -WorkspaceRoot E:\Dev\CodeAtlas\samples\ambiguity `
  -Mode full

# Medium real-project sample
powershell -ExecutionPolicy Bypass -File .\scripts\benchmark\Run-CodeAtlasBenchmark.ps1 `
  -WorkspaceRoot E:\Dev\benchmark `
  -Mode full

# Large OpenCV benchmark with retry for transient publish locks
powershell -ExecutionPolicy Bypass -File .\scripts\benchmark\Run-CodeAtlasBenchmark.ps1 `
  -WorkspaceRoot E:\Dev\opencv `
  -Mode full `
  -RetryCount 3 `
  -RetryDelayMs 1000
```

Harness output:

- writes machine-readable JSON to `dev_docs/benchmark_results/`
- captures commit, machine notes, counts, stage timings, parse/resolve breakdowns, and raw indexer output
- supports both `full` and `incremental` runs through `-Mode`

Operational note:

- on Windows, external indexing or antivirus tools can still transiently lock `<workspace>/.codeatlas/`
- if that happens, the harness preserves the run output and retry count in JSON
- excluding `.codeatlas/` from Everything, Windows Search, and Defender remains recommended for stable large-workspace benchmarking

Query profiling is available through:

```powershell
E:\Dev\CodeAtlas\.tools\node\node.exe E:\Dev\CodeAtlas\server\node_modules\ts-node\dist\bin.js `
  E:\Dev\CodeAtlas\server\src\query-profiler.ts `
  --data-dir E:\Dev\opencv\.codeatlas `
  --output E:\Dev\CodeAtlas\dev_docs\benchmark_results\opencv-query-profile.json `
  --repeat 5 `
  --exact-qualified cv::imread `
  --callers-qualified cv::imread `
  --impact-qualified cv::imread `
  --references-qualified cv::v_float32x4 `
  --trace-source-qualified cv::AGAST `
  --trace-target-qualified cv::makeAgastOffsets `
  --search-query imread
```

This profiler records repeated timings for:

- exact lookup
- search
- callers
- references
- call-path tracing
- impact analysis

Incremental scale benchmarking is available through:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\benchmark\Run-CodeAtlasIncrementalSuite.ps1 `
  -OutputPath .\dev_docs\benchmark_results\incremental-suite-samples.json
```

This suite currently records:

- no-change rerun
- representative single `.cpp` edit
- representative header edit
- repeated single-file burst updates
- fixture-driven mass change
- synthetic branch-like churn that verifies rebuild-escalation behavior

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

Optional representative tuning:

- For unusually large or structurally eccentric monorepos, you can place a workspace-root `.codeatlasrepresentative.json` file to apply a small bounded bias to representative symbol selection.
- This does not replace the default structural scorer. It only nudges tie-breaking.
- Supported first-release fields:
  - `preferredPathPrefixes`
  - `demotedPathPrefixes`
  - `favoredArtifactKinds`
  - `favoredHeaderRoles`

Example:

```json
{
  "preferredPathPrefixes": ["Engine/Source/Runtime", "Game/Source"],
  "demotedPathPrefixes": ["Engine/Source/Editor", "Tests", "Generated"],
  "favoredArtifactKinds": ["runtime"],
  "favoredHeaderRoles": ["public"]
}
```
4. Reindex before evaluating heuristic lookup quality.

Windows stability guidance for `.codeatlas/`:

1. Exclude `<workspace-root>/.codeatlas/` from Everything indexing.
2. Exclude `<workspace-root>/.codeatlas/` from Windows Search indexing when possible.
3. Exclude `<workspace-root>/.codeatlas/` from Defender real-time scanning if policy allows.
4. Keep `.codeatlas/` as generated runtime data only; do not edit or browse it aggressively during active indexing.

Practical starter preset:

```gitignore
# Keep the index focused on production code first
^tests/
^docs/
^dev_docs/
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

dev_docs/          Development documentation
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
