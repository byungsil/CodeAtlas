# CodeAtlas

[![GitHub](https://img.shields.io/badge/GitHub-byungsil%2FCodeAtlas-blue)](https://github.com/byungsil/CodeAtlas)

CodeAtlas is a local code intelligence system for AI-assisted development. It indexes a workspace into SQLite and serves structured queries through MCP and HTTP so agents can reason about code without scanning raw files.

## What You Get

- **Hybrid Rust indexer**: Tree-sitter (syntax parsing for all languages) + libclang (precise C++ analysis with compile flags) — automatically selects the best engine per file
- Full, incremental, and watch-mode indexing
- SQLite generations at `<workspace-root>/.codeatlas/index-<timestamp>.db`
- Active DB pointer at `<workspace-root>/.codeatlas/current-db.json`
- `generate-compile-commands` subcommand: generates `compile_commands.json` from a Visual Studio `.sln`/`.vcxproj` for large MSVC projects
- Watch mode with vcxproj-change detection: automatically patches `compile_commands.json` when project files change
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
- **LLVM** (optional, for libclang-powered C++ precision analysis)
  - Windows: [LLVM releases](https://github.com/llvm/llvm-project/releases) — install to `C:\Program Files\LLVM`
  - `libclang.dll` must be in `PATH` at indexing time
  - Without LLVM, CodeAtlas falls back to Tree-sitter for all C++ files

## GUI Setup (Recommended for First-Time Users)

The easiest way to get started is with the interactive setup wizard:

```powershell
# One-command GUI setup (installs prerequisites, builds indexer & server)
powershell -ExecutionPolicy Bypass -File .\setup-gui.ps1
```

The wizard guides you through:
1. **Environment check** — detects Node.js, npm, Rust, and LLVM; installs missing tools via winget; offers to add LLVM to the user PATH if installed but not on PATH
2. **Indexer build** — compiles the Tree-sitter + libclang hybrid engine with progress feedback
3. **Server setup** — installs npm dependencies and builds TypeScript
4. **Workspace configuration** — browse to your codebase; auto-detects `.sln` files; choose compile context method (`cpp_context` from `.vcxproj` scan, or `compile_commands` via MSBuild)
5. **Indexing configuration** — select languages and file extensions to index
6. **Complete** — apply MCP server config to your VS Code workspace (`.vscode/mcp.json`) with one click

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

### 2. (Optional) Compile Context for MSVC Projects

For C++ projects built with Visual Studio, providing compile context enables libclang-powered precision analysis. Two methods are available:

#### Method A — `cpp_context` (fast, no MSBuild required)

Scans `.vcxproj` files in the solution to collect compile flags and file lists. Completes in seconds.

```powershell
# Generates <workspace-root>/.codeatlas/cpp_context.json
.\indexer\target\release\codeatlas-indexer.exe generate-cpp-context <workspace-root> `
    --sln <path-to>.sln
```

- No MSBuild needed — reads `.vcxproj` XML directly
- Only files registered as `<ClCompile>` entries are included
- Respects `.codeatlasignore` when filtering translation units

#### Method B — `compile_commands` (precise, MSBuild-based)

Invokes MSBuild to capture the actual flags used for each translation unit. More accurate for Unity builds.

```powershell
# Generates <workspace-root>/.codeatlas/compile_commands.json
.\indexer\target\release\codeatlas-indexer.exe generate-compile-commands <workspace-root> `
    --sln <path-to>.sln `
    [--output <path>] `
    [--config Release] `
    [--platform x64]
```

- Requires MSBuild in `PATH` (or auto-detected via `vswhere`)
- Config/platform are auto-detected from the `.sln` if not specified
- Unity build (`.bbsource`) entries are automatically expanded to individual source files
- Respects `.codeatlasignore` when filtering translation units

Once either file is present, the indexer uses libclang for matching files and Tree-sitter for the rest.

### 3. Index a Workspace

```bash
# full rebuild
./indexer/target/release/codeatlas-indexer <workspace-root> --full --workspace-name <display-name>

# incremental update (re-indexes only changed files)
./indexer/target/release/codeatlas-indexer <workspace-root>

# watch mode (monitors for file changes and re-indexes automatically)
./indexer/target/release/codeatlas-indexer watch <workspace-root> --workspace-name <display-name>
```

Common optional flags:

```bash
./indexer/target/release/codeatlas-indexer <workspace-root> --workspace-name myproject
./indexer/target/release/codeatlas-indexer <workspace-root> --extensions cpp,h,hpp,py
./indexer/target/release/codeatlas-indexer <workspace-root> --parse-timeout-ms 60000
./indexer/target/release/codeatlas-indexer <workspace-root> --verbose
```

Watch mode notes:
- On startup, runs an incremental catch-up pass before entering watch mode
- Detects `.vcxproj` changes and automatically patches `compile_commands.json`
- File change → re-index is significantly faster than running standalone incremental

### 4. Start the Server

```bash
cd server
CODEATLAS_PORT=8090 npx ts-node src/index.ts <workspace-root>/.codeatlas
```

Dashboard:

```text
http://localhost:8090/dashboard/
```

Run one dashboard instance per workspace/data dir. The dashboard shows the stored workspace name from DB metadata and resolves the active SQLite generation through `current-db.json`. Legacy `index.db` remains as a compatibility fallback for older workspaces.

## MCP Setup

The setup wizard can write the MCP config automatically to `.vscode/mcp.json`. To configure manually:

**VS Code** (`.vscode/mcp.json`):

```json
{
  "servers": {
    "codeatlas": {
      "type": "stdio",
      "command": "node",
      "args": [
        "<path-to>/server/node_modules/ts-node/dist/bin.js",
        "<path-to>/server/src/mcp.ts",
        "<workspace-root>/.codeatlas"
      ],
      "env": {
        "CODEATLAS_WORKSPACE": "<workspace-root>",
        "CODEATLAS_PORT": "8090",
        "CODEATLAS_WATCHER": "true",
        "CODEATLAS_INDEXER_PATH": "<path-to>/indexer/target/release/codeatlas-indexer",
        "CODEATLAS_DASHBOARD_AUTOOPEN": "false"
      }
    }
  }
}
```

**Other MCP clients** (Claude, Cursor, etc.):

```json
{
  "mcpServers": {
    "codeatlas": {
      "command": "npx",
      "args": ["ts-node", "<path-to>/server/src/mcp.ts", "<workspace-root>/.codeatlas"],
      "env": {
        "CODEATLAS_WORKSPACE": "<workspace-root>",
        "CODEATLAS_PORT": "8090",
        "CODEATLAS_WATCHER": "true",
        "CODEATLAS_INDEXER_PATH": "<path-to>/indexer/target/release/codeatlas-indexer"
      }
    }
  }
}
```

If `CODEATLAS_WATCHER=true`, the MCP server launches the Rust watcher automatically.
`CODEATLAS_WATCHER` defaults to `true`, so live index refresh is on unless you explicitly disable it.

## Agent Custom Instructions

`instructions/codeatlas_instructions.md` contains a ready-made custom instruction file for GitHub Copilot (and compatible agents). It teaches the agent when and how to use CodeAtlas MCP tools — symbol lookup, call hierarchy, references, impact analysis, propagation tracing, and so on.

To activate it, copy or symlink the file to your agent's custom instructions folder:

**GitHub Copilot (VS Code)**

```text
# Repository-scoped (recommended — version-controlled with the project)
.github/copilot-instructions.md

# Or place it in VS Code's user prompts folder for user-scoped activation
# Windows: %APPDATA%\Code\User\prompts\
# macOS/Linux: ~/.config/Code/User/prompts/
```

**Claude (claude.ai / Claude Code)**

```text
# Place in the project root or your Claude custom instructions directory
CLAUDE.md
```

The file uses `applyTo: "**"` front matter so it is active for all files in the workspace once placed in the correct location.

## Common Configuration

| Variable | Default | Purpose |
|---|---|---|
| `CODEATLAS_PORT` | `8090` | HTTP server and dashboard port |
| `CODEATLAS_WORKSPACE` | inferred | Explicit workspace root |
| `CODEATLAS_WATCHER` | `true` | Auto-start watcher from MCP server |
| `CODEATLAS_INDEXER_PATH` | `codeatlas-indexer` | Indexer binary path |
| `CODEATLAS_INDEX_EXTENSIONS` | built-in defaults | Default extension allowlist |
| `CODEATLAS_CPP_PARSE_TIMEOUT_MICROS` | `60000000` | Default C/C++ parse timeout |
| `CODEATLAS_DATA` | `.codeatlas` | Data directory name |
| `CODEATLAS_REFERENCE_QUERY_CAP` | `2000` | Max rows per reference query (safety cap) |
| `CODEATLAS_DASHBOARD_AUTOOPEN` | `false` | Auto-open dashboard in browser when MCP server starts |
| `CODEATLAS_BACKGROUND_THREADS` | `(cpus / 2).clamp(4, 16)` | Worker thread count for indexing; `0` = use all logical CPUs |
| `CODEATLAS_INDEXER_STACK_BYTES` | large default | Per-worker stack size (override for very deep ASTs) |
| `CODEATLAS_SKIP_CPP_LARGER_THAN_BYTES` | `2097152` | Skip oversized C++ files; set to `0` to disable |

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
codeatlas-indexer generate-compile-commands <workspace-root> --sln <path>
```

Supported extensions by default:

```text
.c, .cpp, .h, .hpp, .cc, .cxx, .inl, .inc, .lua, .py, .ts, .tsx, .rs
```

## Parsing Engine: Tree-sitter vs libclang

| Condition | Engine used |
|-----------|-------------|
| File has entry in `compile_commands.json` + LLVM in PATH | libclang (precise, macro-aware) |
| No compile_commands entry, or LLVM not available | Tree-sitter (fast, always available) |
| All non-C++ languages | Tree-sitter |

libclang advantages over Tree-sitter for C++:
- Resolves macros with actual compile-time define flags
- Discovers ~17% more call edges on large MSVC projects (measured on a ~19k-file codebase)
- Correct include path resolution without guessing

## Workspace Hygiene

- Create a `.codeatlasignore` file in the workspace root to exclude tests, docs, generated code, build output, or vendored trees.
  - Patterns apply to **both** file scanning and build metadata (translation unit lists from `cpp_context.json` / `compile_commands.json`), so ignored paths are excluded end-to-end.
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
