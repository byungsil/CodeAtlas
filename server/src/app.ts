import * as path from "path";
import express from "express";
import { Symbol as CodeSymbol } from "./models/symbol";
import { Store } from "./storage/store";
import {
  SEARCH_DEFAULT_LIMIT, SEARCH_MAX_LIMIT,
  CALLERS_DEFAULT_LIMIT, CALLERS_MAX_LIMIT,
  CALLGRAPH_DEFAULT_DEPTH, CALLGRAPH_MAX_DEPTH,
} from "./constants";
import {
  FunctionResponse,
  CallerQueryResponse,
  ClassResponse,
  SearchResponse,
  CallGraphResponse,
  CallGraphEdge,
  CallReference,
  ClassMembersOverviewResponse,
  ErrorResponse,
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

export function createApp(store: Store): express.Express {
  const app = express();

  app.use("/dashboard", express.static(path.join(__dirname, "../public"), { index: "index.html" }));
  app.get("/dashboard", (_req, res) => res.redirect("/dashboard/"));

  function notFound(res: express.Response) {
    return res.status(404).json({ error: "Symbol not found", code: "NOT_FOUND" } as ErrorResponse);
  }

  function badRequest(res: express.Response) {
    return res.status(400).json({ error: "Invalid exact lookup request", code: "BAD_REQUEST" } as ErrorResponse);
  }

  function makeCallRef(call: { callerId?: string; calleeId?: string; filePath: string; line: number }, targetField: "callerId" | "calleeId"): CallReference | null {
    const targetId = call[targetField];
    if (!targetId) return null;
    const sym = store.getSymbolById(targetId);
    if (!sym) return null;
    return makeResolvedCallReference({
      symbol: sym,
      filePath: call.filePath,
      line: call.line,
    });
  }

  function buildCallRefs(calls: { callerId?: string; calleeId?: string; filePath: string; line: number }[], targetField: "callerId" | "calleeId"): CallReference[] {
    return calls
      .map((c) => makeCallRef(c, targetField))
      .filter((r): r is CallReference => r !== null);
  }

  function buildUniqueCallRefs(
    calls: { callerId?: string; calleeId?: string; filePath: string; line: number }[],
    targetField: "callerId" | "calleeId",
    limit: number,
  ): { results: CallReference[]; totalCount: number; truncated: boolean } {
    const refs = buildCallRefs(calls, targetField)
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
    symbol: CodeSymbol,
    maxDepth: number,
    limit: number,
  ): ImpactAnalysisResponse {
    const directCallers = buildUniqueCallRefs(store.getCallers(symbol.id), "callerId", limit);
    const directCallees = buildUniqueCallRefs(store.getCallees(symbol.id), "calleeId", limit);
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

    const visitCallers = (queue: Array<{ symbolId: string; depth: number }>) => {
      while (queue.length > 0) {
        const current = queue.shift()!;
        if (current.depth > maxDepth || seenCallerSymbols.has(current.symbolId)) continue;
        seenCallerSymbols.add(current.symbolId);
        bumpSymbol(current.symbolId);
        if (current.depth === maxDepth) continue;
        const nextCallers = buildUniqueCallRefs(store.getCallers(current.symbolId), "callerId", limit).results;
        for (const next of nextCallers) {
          queue.push({ symbolId: next.symbolId, depth: current.depth + 1 });
        }
      }
    };

    const visitCallees = (queue: Array<{ symbolId: string; depth: number }>) => {
      while (queue.length > 0) {
        const current = queue.shift()!;
        if (current.depth > maxDepth || seenCalleeSymbols.has(current.symbolId)) continue;
        seenCalleeSymbols.add(current.symbolId);
        bumpSymbol(current.symbolId);
        if (current.depth === maxDepth) continue;
        const nextCallees = buildUniqueCallRefs(store.getCallees(current.symbolId), "calleeId", limit).results;
        for (const next of nextCallees) {
          queue.push({ symbolId: next.symbolId, depth: current.depth + 1 });
        }
      }
    };

    visitCallers(callerQueue);
    visitCallees(calleeQueue);

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

    const suggestedFollowUpQueries = [
      `find_callers qualifiedName=${symbol.qualifiedName}`,
      `get_callgraph name=${symbol.name} depth=${Math.min(maxDepth + 1, CALLGRAPH_MAX_DEPTH)}`,
      `find_references qualifiedName=${symbol.qualifiedName}`,
    ];

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
      suggestedFollowUpQueries,
      truncated:
        directCallers.truncated
        || directCallees.truncated
        || directReferences.truncated
        || impactedSymbolCounts.size > limit
        || impactedFileCounts.size > limit,
    };
  }

  function buildStructureOverviewSummary(symbols: CodeSymbol[]): StructureOverviewSummary {
    const typeCounts = symbols.reduce<Record<string, number>>((counts, symbol) => {
      counts[symbol.type] = (counts[symbol.type] ?? 0) + 1;
      return counts;
    }, {});

    return {
      totalCount: symbols.length,
      typeCounts,
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

  function buildExactSymbolResponse(params: { matchedBy: "id" | "qualifiedName" | "both"; symbol: ReturnType<Store["getSymbolById"]> }): SymbolLookupResponse | null {
    const { symbol, matchedBy } = params;
    if (!symbol) return null;

    const base = buildExactLookupResponse({ symbol, matchedBy });

    if (symbol.type === "function" || symbol.type === "method") {
      return {
        ...base,
        callers: buildCallRefs(store.getCallers(symbol.id), "callerId"),
        callees: buildCallRefs(store.getCallees(symbol.id), "calleeId"),
      };
    }

    if (symbol.type === "class" || symbol.type === "struct") {
      return {
        ...base,
        members: store.getMembers(symbol.id),
      };
    }

    return base;
  }

  app.get("/symbol", (req, res) => {
    const id = typeof req.query.id === "string" ? req.query.id : undefined;
    const qualifiedName = typeof req.query.qualifiedName === "string" ? req.query.qualifiedName : undefined;

    if (!id && !qualifiedName) {
      return badRequest(res);
    }

    const byId = id ? store.getSymbolById(id) : undefined;
    const byQualifiedName = qualifiedName ? store.getSymbolByQualifiedName(qualifiedName) : undefined;

    if (id && qualifiedName) {
      if (!byId || !byQualifiedName) {
        return notFound(res);
      }
      if (byId.id !== byQualifiedName.id) {
        return badRequest(res);
      }

      const response = buildExactSymbolResponse({ matchedBy: "both", symbol: byId });
      return response ? res.json(response) : notFound(res);
    }

    const symbol = byId ?? byQualifiedName;
    if (!symbol) {
      return notFound(res);
    }

    const response = buildExactSymbolResponse({
      matchedBy: id ? "id" : "qualifiedName",
      symbol,
    });
    return response ? res.json(response) : notFound(res);
  });

  app.get("/function/:name", (req, res) => {
    const { name } = req.params;
    const { symbol: sym, candidateCount } = resolveFunctionSymbol(name);

    if (!sym) {
      return notFound(res);
    }

    const callers = buildCallRefs(store.getCallers(sym.id), "callerId");
    const callees = buildCallRefs(store.getCallees(sym.id), "calleeId");

    const response: FunctionResponse = buildFunctionResponse({
      symbol: sym,
      candidateCount,
      callers,
      callees,
    });
    return res.json(response);
  });

  app.get("/callers/:name", (req, res) => {
    const { name } = req.params;
    const limit = Math.min(parseInt((req.query.limit as string) || String(CALLERS_DEFAULT_LIMIT), 10), CALLERS_MAX_LIMIT);
    const { symbol: sym, candidateCount } = resolveFunctionSymbol(name);

    if (!sym) {
      return notFound(res);
    }

    const callers = buildUniqueCallRefs(store.getCallers(sym.id), "callerId", limit);
    const response: CallerQueryResponse = buildCallerQueryResponse({
      symbol: sym,
      candidateCount,
      callers: callers.results,
      totalCount: callers.totalCount,
      truncated: callers.truncated,
      limitApplied: limit,
    });
    return res.json(response);
  });

  app.get("/class/:name", (req, res) => {
    const { name } = req.params;
    const symbols = store.getSymbolsByName(name);
    const sym = symbols.find((s) => s.type === "class" || s.type === "struct");

    if (!sym) {
      return notFound(res);
    }

    const members = store.getMembers(sym.id);
    const response: ClassResponse = buildClassResponse({
      symbol: sym,
      candidateCount: symbols.filter((s) => s.type === "class" || s.type === "struct").length,
      members,
    });
    return res.json(response);
  });

  app.get("/references", (req, res) => {
    const id = typeof req.query.id === "string" ? req.query.id : undefined;
    const qualifiedName = typeof req.query.qualifiedName === "string" ? req.query.qualifiedName : undefined;
    const category = typeof req.query.category === "string" ? req.query.category as ReferenceCategory : undefined;
    const filePath = typeof req.query.filePath === "string" ? req.query.filePath : undefined;
    const limit = Math.min(parseInt((req.query.limit as string) || String(SEARCH_DEFAULT_LIMIT), 10), SEARCH_MAX_LIMIT);

    if (!id && !qualifiedName) {
      return badRequest(res);
    }

    const byId = id ? store.getSymbolById(id) : undefined;
    const byQualifiedName = qualifiedName ? store.getSymbolByQualifiedName(qualifiedName) : undefined;
    if (id && qualifiedName) {
      if (!byId || !byQualifiedName) {
        return notFound(res);
      }
      if (byId.id !== byQualifiedName.id) {
        return badRequest(res);
      }
    }

    const symbol = byId ?? byQualifiedName;
    if (!symbol) {
      return notFound(res);
    }

    const references = buildResolvedReferences(symbol.id, category, filePath, limit);
    const response: ReferenceQueryResponse = {
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
    return res.json(response);
  });

  app.get("/impact", (req, res) => {
    const id = typeof req.query.id === "string" ? req.query.id : undefined;
    const qualifiedName = typeof req.query.qualifiedName === "string" ? req.query.qualifiedName : undefined;
    const maxDepth = Math.min(parseInt((req.query.depth as string) || "2", 10), CALLGRAPH_MAX_DEPTH);
    const limit = Math.min(parseInt((req.query.limit as string) || String(SEARCH_DEFAULT_LIMIT), 10), SEARCH_MAX_LIMIT);

    if (!id && !qualifiedName) {
      return badRequest(res);
    }

    const byId = id ? store.getSymbolById(id) : undefined;
    const byQualifiedName = qualifiedName ? store.getSymbolByQualifiedName(qualifiedName) : undefined;
    if (id && qualifiedName) {
      if (!byId || !byQualifiedName) {
        return notFound(res);
      }
      if (byId.id !== byQualifiedName.id) {
        return badRequest(res);
      }
    }

    const symbol = byId ?? byQualifiedName;
    if (!symbol) {
      return notFound(res);
    }

    return res.json(buildImpactAnalysis(symbol, maxDepth, limit));
  });

  app.get("/file-symbols", (req, res) => {
    const filePath = typeof req.query.filePath === "string" ? req.query.filePath : undefined;
    const limit = Math.min(parseInt((req.query.limit as string) || String(SEARCH_DEFAULT_LIMIT), 10), SEARCH_MAX_LIMIT);
    if (!filePath) {
      return res.status(400).json({ error: "Missing query parameter 'filePath'", code: "BAD_REQUEST" } as ErrorResponse);
    }

    const allSymbols = store.getFileSymbols(filePath);
    const symbols = applyLimit(allSymbols, limit);
    const response: FileSymbolsResponse = {
      filePath,
      summary: buildStructureOverviewSummary(allSymbols),
      window: buildResultWindow(symbols.results.length, symbols.totalCount, symbols.truncated, limit),
      symbols: symbols.results,
    };
    return res.json(response);
  });

  app.get("/namespace-symbols", (req, res) => {
    const qualifiedName = typeof req.query.qualifiedName === "string" ? req.query.qualifiedName : undefined;
    const limit = Math.min(parseInt((req.query.limit as string) || String(SEARCH_DEFAULT_LIMIT), 10), SEARCH_MAX_LIMIT);
    if (!qualifiedName) {
      return badRequest(res);
    }

    const symbol = store.getSymbolByQualifiedName(qualifiedName);
    if (!symbol || symbol.type !== "namespace") {
      return notFound(res);
    }

    const allSymbols = store.getNamespaceSymbols(symbol.qualifiedName);
    const symbols = applyLimit(allSymbols, limit);
    const response: NamespaceSymbolsResponse = {
      ...buildExactLookupResponse({ symbol, matchedBy: "qualifiedName" }),
      summary: buildStructureOverviewSummary(allSymbols),
      window: buildResultWindow(symbols.results.length, symbols.totalCount, symbols.truncated, limit),
      symbols: symbols.results,
    };
    return res.json(response);
  });

  app.get("/class-members", (req, res) => {
    const qualifiedName = typeof req.query.qualifiedName === "string" ? req.query.qualifiedName : undefined;
    const limit = Math.min(parseInt((req.query.limit as string) || String(SEARCH_DEFAULT_LIMIT), 10), SEARCH_MAX_LIMIT);
    if (!qualifiedName) {
      return badRequest(res);
    }

    const symbol = store.getSymbolByQualifiedName(qualifiedName);
    if (!symbol || (symbol.type !== "class" && symbol.type !== "struct")) {
      return notFound(res);
    }

    const allMembers = store.getMembers(symbol.id)
      .slice()
      .sort((a, b) => a.line - b.line || a.endLine - b.endLine || a.qualifiedName.localeCompare(b.qualifiedName));
    const members = applyLimit(allMembers, limit);
    const response: ClassMembersOverviewResponse = {
      ...buildExactLookupResponse({ symbol, matchedBy: "qualifiedName" }),
      summary: buildStructureOverviewSummary(allMembers),
      window: buildResultWindow(members.results.length, members.totalCount, members.truncated, limit),
      members: members.results,
    };
    return res.json(response);
  });

  app.get("/search", (req, res) => {
    const q = (req.query.q as string) || "";
    const type = req.query.type as string | undefined;
    const limit = Math.min(parseInt((req.query.limit as string) || String(SEARCH_DEFAULT_LIMIT), 10), SEARCH_MAX_LIMIT);

    if (!q) {
      return res.status(400).json({ error: "Missing query parameter 'q'", code: "BAD_REQUEST" } as ErrorResponse);
    }

    const { results, totalCount } = store.searchSymbols(q, type, limit);
    const response: SearchResponse = {
      query: q,
      window: buildResultWindow(results.length, totalCount, totalCount > limit, limit),
      results,
      totalCount,
      truncated: totalCount > limit,
    };
    return res.json(response);
  });

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

  app.get("/callgraph/:name", (req, res) => {
    const { name } = req.params;
    const maxDepth = Math.min(parseInt((req.query.depth as string) || String(CALLGRAPH_DEFAULT_DEPTH), 10), CALLGRAPH_MAX_DEPTH);

    const symbols = store.getSymbolsByName(name);
    const sym = symbols.find((s) => s.type === "function" || s.type === "method");

    if (!sym) {
      return notFound(res);
    }

    const visited = new Set<string>();
    const { edges: callees, truncated } = expandCallees(sym.id, 0, maxDepth, visited);
    const actualDepth = computeDepth(callees);

    const response: CallGraphResponse = {
      root: {
        symbol: {
          id: sym.id,
          name: sym.name,
          qualifiedName: sym.qualifiedName,
          type: sym.type,
          filePath: sym.filePath,
          line: sym.line,
        },
        callees,
      },
      depth: actualDepth,
      maxDepth,
      truncated,
    };
    return res.json(response);
  });

  function computeDepth(edges: CallGraphEdge[]): number {
    if (edges.length === 0) return 0;
    let max = 0;
    for (const e of edges) {
      const childDepth = e.children ? computeDepth(e.children) : 0;
      if (childDepth + 1 > max) max = childDepth + 1;
    }
    return max;
  }

  return app;
}
