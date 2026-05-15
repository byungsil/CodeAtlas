# CodeAtlas

[![GitHub](https://img.shields.io/badge/GitHub-byungsil%2FCodeAtlas-blue)](https://github.com/byungsil/CodeAtlas)

CodeAtlas is a local code intelligence system for AI-assisted development. It indexes a workspace into SQLite and serves structured queries through MCP and HTTP so agents can reason about code without scanning raw files.

## What You Get

- Rust indexer for full, incremental, and watch-mode indexing
- SQLite generations at `<workspace-root>/.codeatlas/index-<timestamp>.db`
- Active DB pointer at `<workspace-root>/.codeatlas/current-db.json`
- Node.js server for MCP tools, HTTP API, and dashboard
- Support for C/C++, Lua, Python, TypeScript, and Rust source files

## Requirements

- Rust 1.75+
- Node.js 18+
- npm
- C/C++ build toolchain (required by native dependencies)
  - Windows: Visual Studio Build Tools (MSVC + Windows SDK)
  - Linux: GCC/G++ (for example `build-essential`)
  - macOS: Xcode Command Line Tools (`clang`)

## Quick Start

### 1. Build

```bash
git clone https://github.com/byungsil/CodeAtlas.git
cd CodeAtlas

# one-touch Windows setup (prereqs + toolchain check + indexer/server build)
powershell -ExecutionPolicy Bypass -File .\scripts\setup-all.ps1

# optional Windows bootstrap for Node.js, npm, Rust, and server npm deps
powershell -ExecutionPolicy Bypass -File .\scripts\setup-prereqs.ps1
# note: C/C++ build tools are not installed by this script

cd indexer
cargo build --release

cd ../server
npm install
```

Windows note:
- `setup-all.ps1` checks MSVC toolchain availability.
- If Build Tools are installed but `cl.exe` is not visible in the current shell, the script warns and continues.
- If indexer build fails in that case, rerun from `Developer PowerShell for VS`.

### 2. Index a Workspace

```bash
# full rebuild
./indexer/target/release/codeatlas-indexer <workspace-root> --full --workspace-name <display-name>

# incremental update
./indexer/target/release/codeatlas-indexer <workspace-root>

# watch mode
./indexer/target/release/codeatlas-indexer watch <workspace-root> --workspace-name <display-name>
```

Common optional flags:

```bash
./indexer/target/release/codeatlas-indexer <workspace-root> --workspace-name opencv
./indexer/target/release/codeatlas-indexer <workspace-root> --extensions cpp,h,hpp,py
./indexer/target/release/codeatlas-indexer <workspace-root> --parse-timeout-ms 60000
./indexer/target/release/codeatlas-indexer <workspace-root> --verbose
```

### 3. Start the Server

```bash
cd server
CODEATLAS_PORT=3000 npx ts-node src/index.ts <workspace-root>/.codeatlas
```

Dashboard:

```text
http://localhost:3000/dashboard/
```

Run one dashboard instance per workspace/data dir. The dashboard shows the stored workspace name from DB metadata and resolves the active SQLite generation through `current-db.json`. Legacy `index.db` remains as a compatibility fallback for older workspaces.

## MCP Setup

Example MCP registration:

```json
{
  "mcpServers": {
    "codeatlas": {
      "command": "npx",
      "args": ["ts-node", "<path-to>/server/src/mcp.ts", "<workspace-root>/.codeatlas"],
      "env": {
        "CODEATLAS_WORKSPACE": "<workspace-root>",
        "CODEATLAS_PORT": "3000",
        "CODEATLAS_WATCHER": "true",
        "CODEATLAS_INDEXER_PATH": "<path-to>/indexer/target/release/codeatlas-indexer"
      }
    }
  }
}
```

If `CODEATLAS_WATCHER=true`, the MCP server launches the Rust watcher automatically.
`CODEATLAS_WATCHER` defaults to `true`, so live index refresh is on unless you explicitly disable it.

## Common Configuration

| Variable | Default | Purpose |
|---|---|---|
| `CODEATLAS_PORT` | `3000` | HTTP server and dashboard port |
| `CODEATLAS_WORKSPACE` | inferred | Explicit workspace root |
| `CODEATLAS_WATCHER` | `true` | Auto-start watcher from MCP server |
| `CODEATLAS_INDEXER_PATH` | `codeatlas-indexer` | Indexer binary path |
| `CODEATLAS_INDEX_EXTENSIONS` | built-in defaults | Default extension allowlist |
| `CODEATLAS_CPP_PARSE_TIMEOUT_MICROS` | `60000000` | Default C/C++ parse timeout |
| `CODEATLAS_DATA` | `.codeatlas` | Data directory name |
| `CODEATLAS_REFERENCE_QUERY_CAP` | `2000` | Max rows per reference query (safety cap) |

## Supported Indexer Commands

```text
codeatlas-indexer <workspace-root>
codeatlas-indexer <workspace-root> --full --workspace-name <display-name>
codeatlas-indexer <workspace-root> --json
codeatlas-indexer <workspace-root> --verbose
codeatlas-indexer <workspace-root> --workspace-name <display-name>
codeatlas-indexer <workspace-root> --extensions cpp,h,hpp
codeatlas-indexer <workspace-root> --parse-timeout-ms 60000
codeatlas-indexer watch <workspace-root> --workspace-name <display-name>
```

Supported extensions by default:

```text
.c, .cpp, .h, .hpp, .cc, .cxx, .inl, .inc, .lua, .py, .ts, .tsx, .rs
```

## Workspace Hygiene

- Create a `.codeatlasignore` file in the workspace root to exclude tests, docs, generated code, build output, or vendored trees.
- For best Windows stability, exclude `<workspace-root>/.codeatlas/` from aggressive background indexing or antivirus scanning if possible.
- If an existing DB was built for a different workspace, extension set, or DB format, CodeAtlas will force a full rebuild automatically.

Example `.codeatlasignore`:

```text
^tests/
^docs/
^dev_docs/
^generated/
^build/
^out/
```

## Main Query Surfaces

CodeAtlas exposes these major query paths through MCP and HTTP:

- exact symbol lookup
- symbol search
- callers and callees
- references
- impact analysis
- propagation tracing
- workspace summary
