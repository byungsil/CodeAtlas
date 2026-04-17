import * as path from "path";
import express from "express";
import { Store } from "./storage/store";
import {
  SEARCH_DEFAULT_LIMIT, SEARCH_MAX_LIMIT,
  CALLGRAPH_DEFAULT_DEPTH, CALLGRAPH_MAX_DEPTH,
} from "./constants";
import {
  FunctionResponse,
  ClassResponse,
  SearchResponse,
  CallGraphResponse,
  CallGraphEdge,
  CallReference,
  ErrorResponse,
  SymbolLookupResponse,
} from "./models/responses";
import {
  buildClassResponse,
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
    const symbols = store.getSymbolsByName(name);
    const sym = symbols.find((s) => s.type === "function" || s.type === "method");

    if (!sym) {
      return notFound(res);
    }

    const callers = buildCallRefs(store.getCallers(sym.id), "callerId");
    const callees = buildCallRefs(store.getCallees(sym.id), "calleeId");

    const response: FunctionResponse = buildFunctionResponse({
      symbol: sym,
      candidateCount: symbols.filter((s) => s.type === "function" || s.type === "method").length,
      callers,
      callees,
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
