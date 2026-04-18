import * as fs from "fs";
import * as path from "path";
import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import { z } from "zod";
import {
  SEARCH_DEFAULT_LIMIT, SEARCH_MAX_LIMIT, SEARCH_MIN_QUERY_LENGTH,
  CALLERS_DEFAULT_LIMIT, CALLERS_MAX_LIMIT,
  CALLGRAPH_DEFAULT_DEPTH, CALLGRAPH_MAX_DEPTH,
  DATA_DIR_NAME, DB_FILENAME,
} from "./constants";
import { Store } from "./storage/store";
import { SqliteStore } from "./storage/sqlite-store";
import { JsonStore } from "./storage/json-store";
import {
  ClassMembersOverviewResponse,
  CallReference,
  CallGraphEdge,
  CallerQueryResponse,
  FileSymbolsResponse,
  ImpactAnalysisResponse,
  ImpactedFileSummary,
  ImpactedSymbolSummary,
  NamespaceSymbolsResponse,
  ReferenceCategory,
  ReferenceQueryResponse,
  ResolvedReference,
  StructureOverviewSummary,
  SymbolLookupResponse,
} from "./models/responses";
import {
  buildClassResponse,
  buildCallerQueryResponse,
  buildResultWindow,
  buildExactLookupResponse,
  buildFunctionResponse,
  makeResolvedCallReference,
} from "./response-metadata";

export const DEFAULT_DATA_DIR = process.argv[2] || process.env.CODEATLAS_DATA || DATA_DIR_NAME;

export function openStore(dataDir: string): Store {
  const dbPath = path.join(dataDir, DB_FILENAME);
  if (fs.existsSync(dbPath)) {
    return new SqliteStore(dbPath);
  }
  return new JsonStore(dataDir);
}

export function createMcpServer(dataDir: string = DEFAULT_DATA_DIR): {
  server: McpServer;
  store: Store;
  close: () => void;
} {
  const store = openStore(dataDir);
  const server = new McpServer({
    name: "codeatlas",
    version: "0.1.0",
  });

  function buildCallReferences(
    calls: { callerId?: string; calleeId?: string; filePath: string; line: number }[],
    targetField: "callerId" | "calleeId",
  ): CallReference[] {
    return calls
      .map((c) => {
        const targetId = c[targetField];
        if (!targetId) return null;
        const s = store.getSymbolById(targetId);
        if (!s) return null;
        return makeResolvedCallReference({
          symbol: s,
          filePath: c.filePath,
          line: c.line,
        });
      })
      .filter((r): r is CallReference => r !== null);
  }

  function buildUniqueCallReferences(
    calls: { callerId?: string; calleeId?: string; filePath: string; line: number }[],
    targetField: "callerId" | "calleeId",
    limit: number,
  ): { results: CallReference[]; totalCount: number; truncated: boolean } {
    const refs = buildCallReferences(calls, targetField)
      .sort((a, b) => {
        if (a.qualifiedName !== b.qualifiedName) return a.qualifiedName.localeCompare(b.qualifiedName);
        if (a.filePath !== b.filePath) return a.filePath.localeCompare(b.filePath);
        if (a.line !== b.line) return a.line - b.line;
        return a.symbolId.localeCompare(b.symbolId);
      });

    const deduped: CallReference[] = [];
    const seen = new Set<string>();
    for (const ref of refs) {
      if (seen.has(ref.symbolId)) continue;
      seen.add(ref.symbolId);
      deduped.push(ref);
    }

    return {
      results: deduped.slice(0, limit),
      totalCount: deduped.length,
      truncated: deduped.length > limit,
    };
  }

  function applyLimit<T>(items: T[], limit: number): { results: T[]; totalCount: number; truncated: boolean } {
    return {
      results: items.slice(0, limit),
      totalCount: items.length,
      truncated: items.length > limit,
    };
  }

  function resolveFunctionSymbol(name: string) {
    const symbols = store.getSymbolsByName(name);
    const functions = symbols.filter((s) => s.type === "function" || s.type === "method");
    const symbol = functions[0];
    return {
      symbol,
      candidateCount: functions.length,
    };
  }

  function buildExactSymbolPayload(params: {
    matchedBy: "id" | "qualifiedName" | "both";
    symbol: ReturnType<Store["getSymbolById"]>;
  }): SymbolLookupResponse | null {
    const { symbol, matchedBy } = params;
    if (!symbol) return null;

    const base = buildExactLookupResponse({ symbol, matchedBy });

    if (symbol.type === "function" || symbol.type === "method") {
      return {
        ...base,
        callers: buildCallReferences(store.getCallers(symbol.id), "callerId"),
        callees: buildCallReferences(store.getCallees(symbol.id), "calleeId"),
      } as SymbolLookupResponse;
    }

    if (symbol.type === "class" || symbol.type === "struct") {
      return {
        ...base,
        members: store.getMembers(symbol.id),
      } as SymbolLookupResponse;
    }

    return base;
  }

  function buildResolvedReferences(
    targetSymbolId: string,
    category?: ReferenceCategory,
    filePath?: string,
    limit = SEARCH_DEFAULT_LIMIT,
  ): { results: ResolvedReference[]; totalCount: number; truncated: boolean } {
    const references = store.getReferences(targetSymbolId, category, filePath)
      .map((reference) => {
        const sourceSymbol = store.getSymbolById(reference.sourceSymbolId);
        if (!sourceSymbol) return null;
        return {
          ...reference,
          sourceSymbolName: sourceSymbol.name,
          sourceQualifiedName: sourceSymbol.qualifiedName,
        };
      })
      .filter((reference): reference is ResolvedReference => reference !== null)
      .sort((a, b) =>
        a.category.localeCompare(b.category)
        || a.filePath.localeCompare(b.filePath)
        || a.line - b.line
        || a.sourceQualifiedName.localeCompare(b.sourceQualifiedName));

    return {
      results: references.slice(0, limit),
      totalCount: references.length,
      truncated: references.length > limit,
    };
  }

  function buildImpactAnalysis(
    symbol: NonNullable<ReturnType<Store["getSymbolById"]>>,
    maxDepth: number,
    limit: number,
  ): ImpactAnalysisResponse {
    const directCallers = buildUniqueCallReferences(store.getCallers(symbol.id), "callerId", limit);
    const directCallees = buildUniqueCallReferences(store.getCallees(symbol.id), "calleeId", limit);
    const directReferences = buildResolvedReferences(symbol.id, undefined, undefined, limit);

    const impactedSymbolCounts = new Map<string, number>();
    const impactedFileCounts = new Map<string, number>();
    const callerQueue: Array<{ symbolId: string; depth: number }> = directCallers.results.map((ref) => ({ symbolId: ref.symbolId, depth: 1 }));
    const calleeQueue: Array<{ symbolId: string; depth: number }> = directCallees.results.map((ref) => ({ symbolId: ref.symbolId, depth: 1 }));
    const seenCallerSymbols = new Set<string>();
    const seenCalleeSymbols = new Set<string>();

    const bumpSymbol = (symbolId: string) => {
      if (symbolId === symbol.id) return;
      impactedSymbolCounts.set(symbolId, (impactedSymbolCounts.get(symbolId) ?? 0) + 1);
      const affectedSymbol = store.getSymbolById(symbolId);
      if (affectedSymbol) {
        impactedFileCounts.set(affectedSymbol.filePath, (impactedFileCounts.get(affectedSymbol.filePath) ?? 0) + 1);
      }
    };

    while (callerQueue.length > 0) {
      const current = callerQueue.shift()!;
      if (current.depth > maxDepth || seenCallerSymbols.has(current.symbolId)) continue;
      seenCallerSymbols.add(current.symbolId);
      bumpSymbol(current.symbolId);
      if (current.depth === maxDepth) continue;
      const nextCallers = buildUniqueCallReferences(store.getCallers(current.symbolId), "callerId", limit).results;
      for (const next of nextCallers) {
        callerQueue.push({ symbolId: next.symbolId, depth: current.depth + 1 });
      }
    }

    while (calleeQueue.length > 0) {
      const current = calleeQueue.shift()!;
      if (current.depth > maxDepth || seenCalleeSymbols.has(current.symbolId)) continue;
      seenCalleeSymbols.add(current.symbolId);
      bumpSymbol(current.symbolId);
      if (current.depth === maxDepth) continue;
      const nextCallees = buildUniqueCallReferences(store.getCallees(current.symbolId), "calleeId", limit).results;
      for (const next of nextCallees) {
        calleeQueue.push({ symbolId: next.symbolId, depth: current.depth + 1 });
      }
    }

    for (const reference of directReferences.results) {
      bumpSymbol(reference.sourceSymbolId);
      impactedFileCounts.set(reference.filePath, (impactedFileCounts.get(reference.filePath) ?? 0) + 1);
    }

    const topAffectedSymbols: ImpactedSymbolSummary[] = Array.from(impactedSymbolCounts.entries())
      .map(([symbolId, count]) => {
        const impacted = store.getSymbolById(symbolId);
        if (!impacted) return null;
        return {
          symbolId,
          symbolName: impacted.name,
          qualifiedName: impacted.qualifiedName,
          type: impacted.type,
          filePath: impacted.filePath,
          count,
        };
      })
      .filter((item): item is ImpactedSymbolSummary => item !== null)
      .sort((a, b) => b.count - a.count || a.qualifiedName.localeCompare(b.qualifiedName))
      .slice(0, limit);

    const topAffectedFiles: ImpactedFileSummary[] = Array.from(impactedFileCounts.entries())
      .map(([filePath, count]) => ({ filePath, count }))
      .sort((a, b) => b.count - a.count || a.filePath.localeCompare(b.filePath))
      .slice(0, limit);

    return {
      ...buildExactLookupResponse({ symbol, matchedBy: "qualifiedName" }),
      maxDepth,
      directCallers: directCallers.results,
      directCallees: directCallees.results,
      directReferences: directReferences.results,
      totalAffectedSymbols: impactedSymbolCounts.size,
      totalAffectedFiles: impactedFileCounts.size,
      topAffectedSymbols,
      topAffectedFiles,
      suggestedFollowUpQueries: [
        `find_callers qualifiedName=${symbol.qualifiedName}`,
        `get_callgraph name=${symbol.name} depth=${Math.min(maxDepth + 1, CALLGRAPH_MAX_DEPTH)}`,
        `find_references qualifiedName=${symbol.qualifiedName}`,
      ],
      truncated:
        directCallers.truncated
        || directCallees.truncated
        || directReferences.truncated
        || impactedSymbolCounts.size > limit
        || impactedFileCounts.size > limit,
    };
  }

  function buildStructureOverviewSummary(symbols: NonNullable<ReturnType<Store["getSymbolById"]>>[]): StructureOverviewSummary {
    const typeCounts = symbols.reduce<Record<string, number>>((counts, symbol) => {
      counts[symbol.type] = (counts[symbol.type] ?? 0) + 1;
      return counts;
    }, {});

    return {
      totalCount: symbols.length,
      typeCounts,
    };
  }

  function badRequestPayload() {
    return {
      content: [{ type: "text" as const, text: JSON.stringify({ error: "Invalid exact lookup request", code: "BAD_REQUEST" }) }],
      isError: true,
    };
  }

  function notFoundPayload() {
    return {
      content: [{ type: "text" as const, text: JSON.stringify({ error: "Symbol not found", code: "NOT_FOUND" }) }],
      isError: true,
    };
  }

  server.tool(
    "lookup_symbol",
    "Look up one symbol by canonical exact identity. Accepts id and/or qualifiedName and never falls back to short-name heuristics.",
    {
      id: z.string().optional().describe("Canonical exact symbol identity"),
      qualifiedName: z.string().optional().describe("Canonical exact human-readable symbol identity"),
    },
    async ({ id, qualifiedName }) => {
      if (!id && !qualifiedName) {
        return badRequestPayload();
      }

      const byId = id ? store.getSymbolById(id) : undefined;
      const byQualifiedName = qualifiedName ? store.getSymbolByQualifiedName(qualifiedName) : undefined;

      if (id && qualifiedName) {
        if (!byId || !byQualifiedName) {
          return notFoundPayload();
        }
        if (byId.id !== byQualifiedName.id) {
          return badRequestPayload();
        }

        const payload = buildExactSymbolPayload({ matchedBy: "both", symbol: byId });
        if (!payload) {
          return notFoundPayload();
        }

        return {
          content: [{ type: "text", text: JSON.stringify(payload, null, 2) }],
        };
      }

      const symbol = byId ?? byQualifiedName;
      if (!symbol) {
        return notFoundPayload();
      }

      const payload = buildExactSymbolPayload({
        matchedBy: id ? "id" : "qualifiedName",
        symbol,
      });

      if (!payload) {
        return notFoundPayload();
      }

      return {
        content: [{ type: "text", text: JSON.stringify(payload, null, 2) }],
      };
    },
  );

  server.tool(
    "lookup_function",
    "Look up a function or method by name. Returns the symbol definition, its callers, and its callees.",
    {
      name: z.string().describe("Function or method name to look up"),
    },
    async ({ name }) => {
      const { symbol: sym, candidateCount } = resolveFunctionSymbol(name);

      if (!sym) {
        return notFoundPayload();
      }

      const callers = buildCallReferences(store.getCallers(sym.id), "callerId");
      const callees = buildCallReferences(store.getCallees(sym.id), "calleeId");

      return {
        content: [{
          type: "text",
          text: JSON.stringify(buildFunctionResponse({
            symbol: sym,
            candidateCount,
            callers,
            callees,
          }), null, 2),
        }],
      };
    },
  );

  server.tool(
    "find_callers",
    "Find direct inbound callers for a function or method. Results are deduplicated by caller symbol and returned in deterministic order.",
    {
      name: z.string().describe("Function or method name to inspect"),
      limit: z.number().int().min(1).max(CALLERS_MAX_LIMIT).default(CALLERS_DEFAULT_LIMIT).describe("Maximum deduplicated callers to return"),
    },
    async ({ name, limit }) => {
      const { symbol: sym, candidateCount } = resolveFunctionSymbol(name);

      if (!sym) {
        return notFoundPayload();
      }

      const callers = buildUniqueCallReferences(store.getCallers(sym.id), "callerId", limit);
      const payload: CallerQueryResponse = buildCallerQueryResponse({
        symbol: sym,
        candidateCount,
        callers: callers.results,
        totalCount: callers.totalCount,
        truncated: callers.truncated,
        limitApplied: limit,
      });

      return {
        content: [{ type: "text", text: JSON.stringify(payload, null, 2) }],
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
        return notFoundPayload();
      }

      const members = store.getMembers(sym.id);
      return {
        content: [{
          type: "text",
          text: JSON.stringify(buildClassResponse({
            symbol: sym,
            candidateCount: symbols.filter((s) => s.type === "class" || s.type === "struct").length,
            members,
          }), null, 2),
        }],
      };
    },
  );

  server.tool(
    "find_references",
    "Find generalized references for one exact target symbol. Accepts id and/or qualifiedName and supports optional category and filePath filters.",
    {
      id: z.string().optional().describe("Canonical exact target symbol identity"),
      qualifiedName: z.string().optional().describe("Canonical exact target symbol qualified name"),
      category: z.enum(["functionCall", "methodCall", "classInstantiation", "typeUsage", "inheritanceMention"]).optional().describe("Optional reference category filter"),
      filePath: z.string().optional().describe("Optional exact file path filter"),
      limit: z.number().int().min(1).max(SEARCH_MAX_LIMIT).default(SEARCH_DEFAULT_LIMIT).describe("Maximum references to return"),
    },
    async ({ id, qualifiedName, category, filePath, limit }) => {
      if (!id && !qualifiedName) {
        return badRequestPayload();
      }

      const byId = id ? store.getSymbolById(id) : undefined;
      const byQualifiedName = qualifiedName ? store.getSymbolByQualifiedName(qualifiedName) : undefined;

      if (id && qualifiedName) {
        if (!byId || !byQualifiedName) {
          return notFoundPayload();
        }
        if (byId.id !== byQualifiedName.id) {
          return badRequestPayload();
        }
      }

      const symbol = byId ?? byQualifiedName;
      if (!symbol) {
        return notFoundPayload();
      }

      const references = buildResolvedReferences(symbol.id, category, filePath, limit);
      const payload: ReferenceQueryResponse = {
        ...buildExactLookupResponse({
          symbol,
          matchedBy: id && qualifiedName ? "both" : id ? "id" : "qualifiedName",
        }),
        window: buildResultWindow(references.results.length, references.totalCount, references.truncated, limit),
        references: references.results,
        totalCount: references.totalCount,
        truncated: references.truncated,
        ...(category ? { category } : {}),
        ...(filePath ? { filePath } : {}),
      };

      return {
        content: [{ type: "text", text: JSON.stringify(payload, null, 2) }],
      };
    },
  );

  server.tool(
    "impact_analysis",
    "Summarize likely impact for changing one exact target symbol using callers, callees, and generalized references with bounded traversal.",
    {
      id: z.string().optional().describe("Canonical exact target symbol identity"),
      qualifiedName: z.string().optional().describe("Canonical exact target symbol qualified name"),
      depth: z.number().int().min(1).max(CALLGRAPH_MAX_DEPTH).default(2).describe("Maximum caller/callee traversal depth"),
      limit: z.number().int().min(1).max(SEARCH_MAX_LIMIT).default(SEARCH_DEFAULT_LIMIT).describe("Maximum result items per summarized section"),
    },
    async ({ id, qualifiedName, depth, limit }) => {
      if (!id && !qualifiedName) {
        return badRequestPayload();
      }

      const byId = id ? store.getSymbolById(id) : undefined;
      const byQualifiedName = qualifiedName ? store.getSymbolByQualifiedName(qualifiedName) : undefined;
      if (id && qualifiedName) {
        if (!byId || !byQualifiedName) {
          return notFoundPayload();
        }
        if (byId.id !== byQualifiedName.id) {
          return badRequestPayload();
        }
      }

      const symbol = byId ?? byQualifiedName;
      if (!symbol) {
        return notFoundPayload();
      }

      return {
        content: [{ type: "text", text: JSON.stringify(buildImpactAnalysis(symbol, depth, limit), null, 2) }],
      };
    },
  );

  server.tool(
    "list_file_symbols",
    "List symbols declared in one exact file path in stable line order with a compact type summary first.",
    {
      filePath: z.string().describe("Exact workspace-relative file path"),
      limit: z.number().int().min(1).max(SEARCH_MAX_LIMIT).default(SEARCH_DEFAULT_LIMIT).describe("Maximum symbols to return"),
    },
    async ({ filePath, limit }) => {
      const allSymbols = store.getFileSymbols(filePath);
      const symbols = applyLimit(allSymbols, limit);
      const payload: FileSymbolsResponse = {
        filePath,
        summary: buildStructureOverviewSummary(allSymbols),
        window: buildResultWindow(symbols.results.length, symbols.totalCount, symbols.truncated, limit),
        symbols: symbols.results,
      };

      return {
        content: [{ type: "text", text: JSON.stringify(payload, null, 2) }],
      };
    },
  );

  server.tool(
    "list_namespace_symbols",
    "List direct symbols whose enclosing namespace matches one exact namespace qualified name.",
    {
      qualifiedName: z.string().describe("Exact namespace qualified name"),
      limit: z.number().int().min(1).max(SEARCH_MAX_LIMIT).default(SEARCH_DEFAULT_LIMIT).describe("Maximum symbols to return"),
    },
    async ({ qualifiedName, limit }) => {
      const symbol = store.getSymbolByQualifiedName(qualifiedName);
      if (!symbol || symbol.type !== "namespace") {
        return notFoundPayload();
      }

      const allSymbols = store.getNamespaceSymbols(symbol.qualifiedName);
      const symbols = applyLimit(allSymbols, limit);
      const payload: NamespaceSymbolsResponse = {
        ...buildExactLookupResponse({ symbol, matchedBy: "qualifiedName" }),
        summary: buildStructureOverviewSummary(allSymbols),
        window: buildResultWindow(symbols.results.length, symbols.totalCount, symbols.truncated, limit),
        symbols: symbols.results,
      };

      return {
        content: [{ type: "text", text: JSON.stringify(payload, null, 2) }],
      };
    },
  );

  server.tool(
    "list_class_members",
    "List direct members for one exact class or struct qualified name in stable declaration order.",
    {
      qualifiedName: z.string().describe("Exact class or struct qualified name"),
      limit: z.number().int().min(1).max(SEARCH_MAX_LIMIT).default(SEARCH_DEFAULT_LIMIT).describe("Maximum members to return"),
    },
    async ({ qualifiedName, limit }) => {
      const symbol = store.getSymbolByQualifiedName(qualifiedName);
      if (!symbol || (symbol.type !== "class" && symbol.type !== "struct")) {
        return notFoundPayload();
      }

      const allMembers = store.getMembers(symbol.id)
        .slice()
        .sort((a, b) => a.line - b.line || a.endLine - b.endLine || a.qualifiedName.localeCompare(b.qualifiedName));
      const members = applyLimit(allMembers, limit);
      const payload: ClassMembersOverviewResponse = {
        ...buildExactLookupResponse({ symbol, matchedBy: "qualifiedName" }),
        summary: buildStructureOverviewSummary(allMembers),
        window: buildResultWindow(members.results.length, members.totalCount, members.truncated, limit),
        members: members.results,
      };

      return {
        content: [{ type: "text", text: JSON.stringify(payload, null, 2) }],
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
        content: [{
          type: "text",
          text: JSON.stringify({
            query,
            window: buildResultWindow(results.length, totalCount, truncated, limit),
            results,
            totalCount,
            truncated,
          }, null, 2),
        }],
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
        return notFoundPayload();
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

  const close = () => {
    const closable = store as Store & { close?: () => void };
    closable.close?.();
  };

  return { server, store, close };
}

export async function runMcpServer(dataDir: string = DEFAULT_DATA_DIR): Promise<void> {
  const { loadConfig, resolveWorkspace } = await import("./config");
  const { createApp } = await import("./app");
  const childProcess = await import("child_process");
  const config = loadConfig();
  const { server, store, close } = createMcpServer(dataDir);

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
    close();
  }

  process.on("SIGINT", () => { cleanup(); process.exit(0); });
  process.on("SIGTERM", () => { cleanup(); process.exit(0); });
  process.on("exit", cleanup);

  const transport = new StdioServerTransport();
  await server.connect(transport);
}
