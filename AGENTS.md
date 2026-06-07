# CodeAtlas Agent Instructions

## Project Structure

- **`indexer/`** — Rust binary (Tree-sitter-based code indexer). Outputs SQLite DB under `<workspace>/.codeatlas/index-*.db`, active pointer at `current-db.json`.
- **`server/`** — Node.js/TypeScript MCP + HTTP server. Reads the indexed DB via `Store` abstraction (`SqliteStore` / `JsonStore`).
- **`setup-wizard/`** — Electron GUI installer (not needed for CLI/MCP usage).
- **`dev_docs/`** — Milestone docs, API contract, evaluation reports. See `AGENT_WORKFLOW.md` for authoritative MCP tool guidance.

## Build & Run Commands

### Indexer (Rust)

```bash
cd indexer && cargo build --release          # release binary at target/release/codeatlas-indexer
# Full rebuild: ./target/release/codeatlas-indexer <workspace-root> --full --workspace-name <name>
# Incremental (default): ./target/release/codeatlas-indexer <workspace-root>
# Watch mode:      ./target/release/codeatlas-indexer watch <workspace-root> --workspace-name <name>
```

Optional flags: `--extensions cpp,h,hpp`, `--parse-timeout-ms 60000`, `--json`, `--verbose`.

### Server (Node.js/TypeScript)

```bash
cd server && npm install && npm run build   # tsc → dist/
npm test                                    # jest, matches **/__tests__/**/*.test.ts
# Start MCP:  npx ts-node src/mcp.ts <workspace-root>/.codeatlas
# Start HTTP + dashboard: CODEATLAS_PORT=8090 npx ts-node src/index.ts <workspace-root>/.codeatlas
```

### Windows one-command setup

```powershell
.\scripts\setup-all.ps1              # prereqs + indexer + server build (checks MSVC toolchain)
.\scripts\setup-prereqs.ps1          # Node.js, npm, Rust only — does NOT install C/C++ tools
```

## Key Files

| Component | Key Files |
|-----------|-----------|
| Indexer entry | `indexer/src/main.rs` (CLI parsing, full/incremental/watch pipeline) |
| Full rebuild logic | `indexer/src/indexing.rs` — batched parse → merge symbols → resolve calls → boundary propagation |
| Symbol resolver | `indexer/src/resolver.rs` — merge + call resolution + cross-boundary flow tags |
| C++ parser / Tree-sitter rules | `indexer/src/parser.rs`, `indexer/graph/*.tsg` (tree-sitter-graph DSL for call relations per language) |
| DB schema & persistence | `indexer/src/storage.rs` — SQLite tables, versioned publish flow, metadata tracking |
| Incremental planning | `indexer/src/incremental.rs` — file diff → plan → escalation to full rebuild if needed |
| Watch mode | `indexer/src/watcher.rs` (notify crate + auto-restart with backoff) |
| Server entry / HTTP routes | `server/src/app.ts` — Express routes, query builders (`buildExactSymbolPayload`, `buildImpactAnalysis`, etc.) |
| MCP tool definitions | `server/src/mcp-runtime.ts` — Zod schemas for all tools (lookup_symbol, lookup_function, search_symbols, callers/callees, propagation, investigation workflow) |
| Store abstraction | `server/src/storage/store.ts` (interface), `sqlite-store.ts` / `json-store.ts` (implementations). Server auto-selects SQLite if DB exists, else JSON fallback. |

## Environment Variables

| Variable | Default | Purpose |
|----------|---------|---------|
| `CODEATLAS_PORT` | `8090` | HTTP/dashboard port |
| `CODEATLAS_WATCHER` | `true` | Auto-start watcher from MCP server (live index refresh) |
| `CODEATLAS_INDEXER_PATH` | `codeatlas-indexer` | Indexer binary path for MCP launcher |
| `CODEATLAS_DATA` | `.codeatlas` | Data directory name |
| `CODEATLAS_REFERENCE_QUERY_CAP` | `2000` | Max rows per reference query (safety cap) — also in `server/src/constants.ts:11` via env override |
| `CODEATLAS_CPP_PARSE_TIMEOUT_MICROS` | `60_000_000` | Per-file C++ parse timeout |
| `CODEATLAS_INDEX_EXTENSIONS` | built-in | Extension allowlist (also settable via CLI `--extensions`) |

## Indexing Gotchas

- `.codeatlasignore` in workspace root excludes files — supports regex with `^` anchor.
- Binary-like / oversized C++ files auto-skipped (default 2 MB, override: `CODEATLAS_SKIP_CPP_LARGER_THAN_BYTES`).
- DB format mismatch, extension change, or missing active DB triggers **full rebuild** automatically (`determine_effective_full_mode` in main.rs).
- Versioned DBs at `<workspace>/.codeatlas/index-<timestamp>.db`, active pointer at `<workspace>/.codeatlas/current-db.json`. One dashboard per workspace.
- Staging happens in `/tmp/codeatlas-{tag}-{key}-index.db`; published atomically via copy + pointer update. Cleanup retains 1 inactive generation max (`cleanup_inactive_generations`).
- On Windows, `mark_codeatlas_artifacts()` runs `attrib +H +I` on `.codeatlas/` and the DB file to hide from Explorer / mark as indexable.
- Large repos: worker threads use a larger default stack (override via `CODEATLAS_INDEXER_STACK_BYTES`; falls back to `RUST_MIN_STACK`).

## MCP Query Patterns & Confidence Rules

Always follow this order — **never skip directly to exact lookup with a guessed name**:

1. **Discovery**: `search_symbols({ query: "..." })` — min 3 chars, case-insensitive substring
2. **Heuristic lookup**: `lookup_function({ name: "..." })` or `lookup_class({ name: "..." })` — check `confidence` and `matchReasons` fields in response
3. **Exact lookup**: `lookup_symbol({ qualifiedName: "..." })` — deterministic, never falls back

**Confidence semantics (critical):**
- `exact` → canonical identity targeted; trust the symbol but do not over-interpret as compiler-grade semantic certainty.
- `high_confidence_heuristic` → structurally well-supported best candidate from ranking; may need confirmation in tricky C++ cases.
- `ambiguous` → **do NOT assume one winner**. Use returned names to refine query or switch to canonical identity lookup.
- `unresolved` → index has no viable answer. Say so plainly — never invent results.

**features:** The server now supports propagation tracing (`explain_symbol_propagation`, `trace_variable_flow`) and investigation workflows (`investigate_workflow`). These use flow tags, cross-boundary hops, type inferences, and analysis rules stored by the indexer. Responses include `riskMarkers` (e.g., `pointerHeavyFlow`, `receiverAmbiguity`, `unresolvedOverload`) and follow-up query suggestions — honor them for deeper traces.

## Server Architecture Notes

- **Store abstraction** (`server/src/storage/store.ts:65`): Both HTTP routes (`app.ts`) and MCP tools (`mcp-runtime.ts`) share the same Store interface. Query-building functions (e.g., `buildExactSymbolPayload`, `rankHeuristicCandidatesDetailed`, `buildImpactAnalysis`) are duplicated across both files — changes to one must be mirrored in the other.
- **MCP tool registration**: All tools use Zod schemas (`server/src/mcp-runtime.ts`). The `tool` wrapper records runtime stats (latency, errors) via `recordMcpToolCall`.
- **Compact responses** (`AUTO_COMPACT_THRESHOLD = 20`): When result counts exceed ~20 items the server auto-compacts caller/callee/reference payloads to save tokens. See `server/src/compact-responses.ts`.

## Testing

```bash
cd server && npm test   # jest — matches **/__tests__/**/*.test.ts
```

- Server has Jest tests (HTTP API, query logic). Indexer has no formal unit tests beyond dev validation against real projects (OpenCV, LLVM, nlohmann/json in `samples/` and `dev_docs/`).
- Before committing server changes: run `npm test`. If modifying shared query-building functions (`app.ts` ↔ `mcp-runtime.ts`), verify both MCP and HTTP paths work.
