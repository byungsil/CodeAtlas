import * as fs from "fs";
import * as path from "path";
import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import { z } from "zod";
import {
  SEARCH_DEFAULT_LIMIT, SEARCH_MAX_LIMIT, SEARCH_MIN_QUERY_LENGTH,
  CALLGRAPH_DEFAULT_DEPTH, CALLGRAPH_MAX_DEPTH,
  DATA_DIR_NAME, DB_FILENAME,
} from "./constants";
import { Store } from "./storage/store";
import { SqliteStore } from "./storage/sqlite-store";
import { JsonStore } from "./storage/json-store";
import {
  CallReference,
  CallGraphEdge,
} from "./models/responses";

const dataDir = process.argv[2] || process.env.CODEATLAS_DATA || DATA_DIR_NAME;

function openStore(dataDir: string): Store {
  const dbPath = path.join(dataDir, DB_FILENAME);
  if (fs.existsSync(dbPath)) {
    return new SqliteStore(dbPath);
  }
  return new JsonStore(dataDir);
}

const store = openStore(dataDir);

const server = new McpServer({
  name: "codeatlas",
  version: "0.1.0",
});

server.tool(
  "lookup_function",
  "Look up a function or method by name. Returns the symbol definition, its callers, and its callees.",
  {
    name: z.string().describe("Function or method name to look up"),
  },
  async ({ name }) => {
    const symbols = store.getSymbolsByName(name);
    const sym = symbols.find((s) => s.type === "function" || s.type === "method");

    if (!sym) {
      return {
        content: [{ type: "text", text: JSON.stringify({ error: "Symbol not found", code: "NOT_FOUND" }) }],
        isError: true,
      };
    }

    const callerCalls = store.getCallers(sym.id);
    const calleeCalls = store.getCallees(sym.id);

    const callers: CallReference[] = callerCalls
      .map((c) => {
        const s = store.getSymbolById(c.callerId);
        if (!s) return null;
        return { symbolId: s.id, symbolName: s.name, qualifiedName: s.qualifiedName, filePath: c.filePath, line: c.line };
      })
      .filter((r): r is CallReference => r !== null);

    const callees: CallReference[] = calleeCalls
      .map((c) => {
        const s = store.getSymbolById(c.calleeId);
        if (!s) return null;
        return { symbolId: s.id, symbolName: s.name, qualifiedName: s.qualifiedName, filePath: c.filePath, line: c.line };
      })
      .filter((r): r is CallReference => r !== null);

    return {
      content: [{ type: "text", text: JSON.stringify({ symbol: sym, callers, callees }, null, 2) }],
    };
  },
);

server.tool(
  "lookup_class",
  "Look up a class or struct by name. Returns the class definition and its members.",
  {
    name: z.string().describe("Class or struct name to look up"),
  },
  async ({ name }) => {
    const symbols = store.getSymbolsByName(name);
    const sym = symbols.find((s) => s.type === "class" || s.type === "struct");

    if (!sym) {
      return {
        content: [{ type: "text", text: JSON.stringify({ error: "Symbol not found", code: "NOT_FOUND" }) }],
        isError: true,
      };
    }

    const members = store.getMembers(sym.id);
    return {
      content: [{ type: "text", text: JSON.stringify({ symbol: sym, members }, null, 2) }],
    };
  },
);

server.tool(
  "search_symbols",
  "Search symbols by name substring. Returns matching symbols with truncation indicator. Minimum query length is 3 characters.",
  {
    query: z.string().describe(`Search query (minimum ${SEARCH_MIN_QUERY_LENGTH} characters; shorter queries return an empty result set)`),
    type: z.enum(["function", "method", "class", "struct", "enum", "namespace", "variable", "typedef"]).optional().describe("Filter by symbol type"),
    limit: z.number().int().min(1).max(SEARCH_MAX_LIMIT).default(SEARCH_DEFAULT_LIMIT).describe("Maximum results to return"),
  },
  async ({ query, type, limit }) => {
    const { results, totalCount } = store.searchSymbols(query, type, limit);
    const truncated = totalCount > limit;

    return {
      content: [{ type: "text", text: JSON.stringify({ query, results, totalCount, truncated }, null, 2) }],
    };
  },
);

function expandCallees(symbolId: string, currentDepth: number, maxDepth: number, visited: Set<string>): { edges: CallGraphEdge[]; truncated: boolean } {
  if (currentDepth >= maxDepth || visited.has(symbolId)) {
    const calls = store.getCallees(symbolId);
    return { edges: [], truncated: calls.length > 0 };
  }
  visited.add(symbolId);
  const calls = store.getCallees(symbolId);
  let anyTruncated = false;
  const edges: CallGraphEdge[] = calls
    .map((c) => {
      const target = store.getSymbolById(c.calleeId);
      if (!target) return null;
      const { edges: children, truncated } = expandCallees(target.id, currentDepth + 1, maxDepth, visited);
      if (truncated) anyTruncated = true;
      return {
        targetId: target.id,
        targetName: target.name,
        targetQualifiedName: target.qualifiedName,
        filePath: c.filePath,
        line: c.line,
        ...(children.length > 0 ? { children } : {}),
      };
    })
    .filter((e): e is CallGraphEdge => e !== null);
  return { edges, truncated: anyTruncated };
}

function computeDepth(edges: CallGraphEdge[]): number {
  if (edges.length === 0) return 0;
  let max = 0;
  for (const e of edges) {
    const d = e.children ? computeDepth(e.children) : 0;
    if (d + 1 > max) max = d + 1;
  }
  return max;
}

server.tool(
  "get_callgraph",
  "Get the call graph rooted at a function or method. Expands callees recursively up to the requested depth.",
  {
    name: z.string().describe("Root function or method name"),
    depth: z.number().int().min(1).max(CALLGRAPH_MAX_DEPTH).default(CALLGRAPH_DEFAULT_DEPTH).describe("Maximum traversal depth"),
  },
  async ({ name, depth: maxDepth }) => {
    const symbols = store.getSymbolsByName(name);
    const sym = symbols.find((s) => s.type === "function" || s.type === "method");

    if (!sym) {
      return {
        content: [{ type: "text", text: JSON.stringify({ error: "Symbol not found", code: "NOT_FOUND" }) }],
        isError: true,
      };
    }

    const visited = new Set<string>();
    const { edges: callees, truncated } = expandCallees(sym.id, 0, maxDepth, visited);
    const actualDepth = computeDepth(callees);

    const response = {
      root: {
        symbol: { id: sym.id, name: sym.name, qualifiedName: sym.qualifiedName, type: sym.type, filePath: sym.filePath, line: sym.line },
        callees,
      },
      depth: actualDepth,
      maxDepth,
      truncated,
    };

    return {
      content: [{ type: "text", text: JSON.stringify(response, null, 2) }],
    };
  },
);

async function main() {
  const { loadConfig, resolveWorkspace } = await import("./config");
  const { createApp } = await import("./app");
  const childProcess = await import("child_process");
  const config = loadConfig();

  let watcherProcess: ReturnType<typeof childProcess.spawn> | null = null;

  if (config.watcher.enabled) {
    const workspaceRoot = resolveWorkspace(dataDir);
    const indexerPath = config.watcher.indexerPath;
    process.stderr.write(`Watcher: starting ${indexerPath} watch ${workspaceRoot}\n`);

    watcherProcess = childProcess.spawn(indexerPath, ["watch", workspaceRoot], {
      stdio: ["ignore", "pipe", "pipe"],
    });

    watcherProcess.stdout?.on("data", (data: Buffer) => {
      process.stderr.write(`[watcher] ${data.toString().trimEnd()}\n`);
    });
    watcherProcess.stderr?.on("data", (data: Buffer) => {
      process.stderr.write(`[watcher:err] ${data.toString().trimEnd()}\n`);
    });
    watcherProcess.on("error", (err) => {
      process.stderr.write(`Watcher failed to start: ${err.message}\n`);
      process.stderr.write(`Set CODEATLAS_INDEXER_PATH to the correct path.\n`);
    });
    watcherProcess.on("exit", (code) => {
      process.stderr.write(`Watcher exited with code ${code}\n`);
      watcherProcess = null;
    });
  }

  if (config.dashboard.autoOpen) {
    const httpApp = createApp(store);
    const port = config.dashboard.port;
    const httpServer = httpApp.listen(port, () => {
      const url = `http://localhost:${port}/dashboard/`;
      import("child_process").then(({ exec }) => {
        const cmd = process.platform === "win32" ? `start ${url}`
          : process.platform === "darwin" ? `open ${url}`
          : `xdg-open ${url}`;
        exec(cmd);
      });
    });
    httpServer.on("error", (err: NodeJS.ErrnoException) => {
      if (err.code === "EADDRINUSE") {
        process.stderr.write(`Dashboard: port ${port} already in use. Set CODEATLAS_PORT to change.\n`);
      }
    });
  }

  function cleanup() {
    if (watcherProcess && !watcherProcess.killed) {
      process.stderr.write("Stopping watcher...\n");
      watcherProcess.kill("SIGTERM");
    }
  }

  process.on("SIGINT", () => { cleanup(); process.exit(0); });
  process.on("SIGTERM", () => { cleanup(); process.exit(0); });
  process.on("exit", cleanup);

  const transport = new StdioServerTransport();
  await server.connect(transport);
}

main().catch((err) => {
  console.error("MCP server error:", err);
  process.exit(1);
});
