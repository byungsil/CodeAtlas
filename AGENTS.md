# CodeAtlas Agent Instructions
You are Qwen, created by Alibaba Cloud. You are a helpful assistant.

## Project Structure

- **`indexer/`** — Rust binary (Tree-sitter-based code indexer)
- **`server/`** — Node.js/TypeScript MCP + HTTP server
- **`setup-wizard/`** — Electron GUI installer
- **`dev_docs/`** — Milestone docs, API contract, evaluation reports

## Build & Run Commands

### Indexer (Rust)
```bash
cd indexer
cargo build --release
# Full rebuild
./target/release/codeatlas-indexer <workspace-root> --full --workspace-name <name>
# Incremental (default)
./target/release/codeatlas-indexer <workspace-root>
# Watch mode
./target/release/codeatlas-indexer watch <workspace-root> --workspace-name <name>
```

### Server (Node.js/TypeScript)
```bash
cd server
npm install
npm run build        # tsc
npm test             # jest
npm run dev          # ts-node src/index.ts
# Start with data dir
CODEATLAS_PORT=8090 npx ts-node src/index.ts <workspace-root>/.codeatlas
```

### One-command setup (Windows)
```powershell
.\scripts\setup-all.ps1              # prereqs + indexer + server build
.\scripts\setup-prereqs.ps1          # Node.js, npm, Rust only (no C/C++ tools)
.\setup-gui.ps1                      # interactive GUI wizard
```

## Key Files

| Component | Key Files |
|-----------|-----------|
| Indexer entry | `indexer/src/main.rs` |
| C++ parser | `indexer/src/parser.rs` (tree-sitter + graph DSL) |
| Symbol resolver | `indexer/src/resolver.rs` (merge + call resolution + propagation) |
| DB schema | `indexer/src/storage.rs` |
| Graph DSL rules | `indexer/graph/*.tsg` |
| Server entry | `server/src/index.ts` |
| MCP tools | `server/src/mcp.ts` |
| API routes | `server/src/app.ts` |
| API contract | `dev_docs/API_CONTRACT.md` |
| Agent workflow | `dev_docs/AGENT_WORKFLOW.md` |

## Environment Variables

| Variable | Default | Purpose |
|----------|---------|---------|
| `CODEATLAS_PORT` | `8090` | HTTP/dashboard port |
| `CODEATLAS_WORKSPACE` | inferred | Workspace root override |
| `CODEATLAS_WATCHER` | `true` | Auto-start watcher from MCP |
| `CODEATLAS_INDEXER_PATH` | `codeatlas-indexer` | Indexer binary path |
| `CODEATLAS_DATA` | `.codeatlas` | Data directory name |
| `CODEATLAS_REFERENCE_QUERY_CAP` | `2000` | Max rows per reference query |
| `CODEATLAS_CPP_PARSE_TIMEOUT_MICROS` | `60000000` | Per-file C++ parse timeout (microseconds) |
| `CODEATLAS_INDEX_EXTENSIONS` | built-in | Extension allowlist |

## Indexing Gotchas

- `.codeatlasignore` in workspace root excludes files (supports regex with `^`)
- Binary-like / oversized C++ files auto-skipped (default 2MB, override: `CODEATLAS_SKIP_CPP_LARGER_THAN_BYTES`)
- DB format mismatch or extension change triggers full rebuild
- Versioned DBs at `<workspace>/.codeatlas/index-<timestamp>.db`, active pointer at `<workspace>/.codeatlas/current-db.json`
- One dashboard per workspace/data dir

## Query Patterns

1. **Discovery**: `search_symbols({ query: "..." })` — min 3 chars, case-insensitive substring
2. **Heuristic lookup**: `lookup_function({ name: "..." })` or `lookup_class({ name: "..." })` — check `confidence` and `matchReasons`
3. **Exact lookup**: `lookup_symbol({ qualifiedName: "..." })` — deterministic, never falls back

`confidence = ambiguous` → do not assume one winner.
`confidence = unresolved` → index has no viable answer.

## Language Support

C/C++, Lua, Python, TypeScript/TSX, Rust. C++ is the deepest (propagation, macro sensitivity, include deps). Others support exact lookup, search, callers/callees, references, impact analysis.

## Testing

```bash
cd server && npm test   # jest, matches **/__tests__/**/*.test.ts
```

No indexer tests beyond dev validation against real projects (OpenCV, LLVM, nlohmann/json).

## Behavior Rules

- Follow the query patterns above in order: discovery → heuristic → exact. Never skip to exact with a guessed name.
- Treat `ambiguous` confidence as "not enough info", not "pick the first one".
- Do not invent answers when index returns `unresolved`. Say so plainly.
- Prefer exact identity (`qualifiedName`) over short names whenever possible.
- When C++ analysis is shallow or uncertain, surface that. Do not overstate certainty.
- Keep responses short. One table or code block per answer. No summaries after the fact.
