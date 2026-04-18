import * as path from "path";
import express from "express";
import { Symbol as CodeSymbol } from "./models/symbol";
import { Store } from "./storage/store";
import { MetadataFilters } from "./storage/store";
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
  BaseMethodsResponse,
  ErrorResponse,
  FileSymbolsResponse,
  ImpactAnalysisResponse,
  ImpactedFileSummary,
  ImpactedSymbolSummary,
  MetadataGroupSummary,
  NamespaceSymbolsResponse,
  OverrideQueryResponse,
  ExplainSymbolPropagationResponse,
  PropagationEventRecord,
  PropagationKind,
  PropagationPathStep,
  ReferenceCategory,
  ReferenceQueryResponse,
  ResolvedReference,
  StructureOverviewSummary,
  SymbolLookupResponse,
  TraceVariableFlowResponse,
  TraceCallPathResponse,
  TypeHierarchyResponse,
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

  function buildSymbolMap(ids: Iterable<string>): Map<string, CodeSymbol> {
    const uniqueIds = Array.from(new Set(Array.from(ids)));
    return new Map(store.getSymbolsByIds(uniqueIds).map((symbol) => [symbol.id, symbol]));
  }

  function makeCallRef(
    call: { callerId?: string; calleeId?: string; filePath: string; line: number },
    targetField: "callerId" | "calleeId",
    symbolMap?: Map<string, CodeSymbol>,
  ): CallReference | null {
    const targetId = call[targetField];
    if (!targetId) return null;
    const sym = symbolMap?.get(targetId) ?? store.getSymbolById(targetId);
    if (!sym) return null;
    return makeResolvedCallReference({
      symbol: sym,
      filePath: call.filePath,
      line: call.line,
    });
  }

  function parseMetadataFilters(source: Record<string, unknown>): MetadataFilters | undefined {
    const subsystem = typeof source.subsystem === "string" ? source.subsystem : undefined;
    const module = typeof source.module === "string" ? source.module : undefined;
    const projectArea = typeof source.projectArea === "string" ? source.projectArea : undefined;
    const artifactKind = typeof source.artifactKind === "string"
      ? source.artifactKind as MetadataFilters["artifactKind"]
      : undefined;

    if (!subsystem && !module && !projectArea && !artifactKind) {
      return undefined;
    }

    return { subsystem, module, projectArea, artifactKind };
  }

  function metadataFilterEcho(filters?: MetadataFilters): Partial<MetadataFilters> {
    return filters ?? {};
  }

  function matchesMetadataFilters(symbol: CodeSymbol | undefined, filters?: MetadataFilters): boolean {
    if (!filters) return true;
    if (!symbol) return false;
    if (filters.subsystem && symbol.subsystem !== filters.subsystem) return false;
    if (filters.module && symbol.module !== filters.module) return false;
    if (filters.projectArea && symbol.projectArea !== filters.projectArea) return false;
    if (filters.artifactKind && symbol.artifactKind !== filters.artifactKind) return false;
    return true;
  }

  function buildMetadataGroupSummary(
    symbolIds: Iterable<string>,
    keySelector: (symbol: CodeSymbol) => string | undefined,
  ): MetadataGroupSummary[] {
    const symbolMap = buildSymbolMap(symbolIds);
    const counts = new Map<string, number>();
    for (const symbol of symbolMap.values()) {
      const key = keySelector(symbol);
      if (!key) continue;
      counts.set(key, (counts.get(key) ?? 0) + 1);
    }
    return Array.from(counts.entries())
      .map(([key, count]) => ({ key, count }))
      .sort((a, b) => b.count - a.count || a.key.localeCompare(b.key));
  }

  function buildCallRefs(calls: { callerId?: string; calleeId?: string; filePath: string; line: number }[], targetField: "callerId" | "calleeId"): CallReference[] {
    const symbolMap = buildSymbolMap(
      calls
        .map((call) => call[targetField])
        .filter((symbolId): symbolId is string => Boolean(symbolId)),
    );
    return calls
      .map((c) => makeCallRef(c, targetField, symbolMap))
      .filter((r): r is CallReference => r !== null);
  }

  function buildUniqueCallRefs(
    calls: { callerId?: string; calleeId?: string; filePath: string; line: number }[],
    targetField: "callerId" | "calleeId",
    limit: number,
    metadataFilters?: MetadataFilters,
  ): { results: CallReference[]; totalCount: number; truncated: boolean } {
    const refs = buildCallRefs(calls, targetField);
    const symbolMap = buildSymbolMap(refs.map((ref) => ref.symbolId));
    const filteredRefs = refs
      .filter((ref) => matchesMetadataFilters(symbolMap.get(ref.symbolId), metadataFilters))
      .sort((a, b) => {
        if (a.qualifiedName !== b.qualifiedName) return a.qualifiedName.localeCompare(b.qualifiedName);
        if (a.filePath !== b.filePath) return a.filePath.localeCompare(b.filePath);
        if (a.line !== b.line) return a.line - b.line;
        return a.symbolId.localeCompare(b.symbolId);
      });

    const deduped: CallReference[] = [];
    const seen = new Set<string>();
    for (const ref of filteredRefs) {
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
    metadataFilters?: MetadataFilters,
  ): { results: ResolvedReference[]; totalCount: number; truncated: boolean } {
    const rawReferences = store.getReferences(targetSymbolId, category, filePath);
    const symbolMap = buildSymbolMap(rawReferences.map((reference) => reference.sourceSymbolId));
    const references = rawReferences
      .map((reference) => {
        const sourceSymbol = symbolMap.get(reference.sourceSymbolId);
        if (!sourceSymbol || !matchesMetadataFilters(sourceSymbol, metadataFilters)) return null;
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

  function parsePropagationKinds(source: Record<string, unknown>): PropagationKind[] | undefined {
    const raw = source.propagationKinds;
    if (typeof raw !== "string" || raw.trim().length === 0) {
      return undefined;
    }
    return raw
      .split(",")
      .map((value) => value.trim())
      .filter((value): value is PropagationKind => value.length > 0);
  }

  function anchorKey(anchor: PropagationEventRecord["sourceAnchor"]): string | null {
    if (anchor.anchorId) return `anchor:${anchor.anchorId}`;
    if (anchor.symbolId) return `symbol:${anchor.symbolId}`;
    if (anchor.expressionText) return `expr:${anchor.anchorKind}:${anchor.expressionText}`;
    return null;
  }

  function buildPropagationSummary(events: PropagationEventRecord[]): string[] {
    if (events.length === 0) {
      return ["No supported propagation events found for the exact symbol scope."];
    }

    const byKind = new Map<PropagationKind, number>();
    for (const event of events) {
      byKind.set(event.propagationKind, (byKind.get(event.propagationKind) ?? 0) + 1);
    }

    return Array.from(byKind.entries())
      .sort((a, b) => a[0].localeCompare(b[0]))
      .map(([kind, count]) => `${kind}: ${count} event(s)`);
  }

  function buildPropagationAssessment(
    events: PropagationEventRecord[],
    truncated: boolean,
  ): {
    propagationConfidence: "high" | "partial";
    riskMarkers: PropagationEventRecord["risks"];
    confidenceNotes: string[];
  } {
    const riskMarkers = Array.from(new Set(events.flatMap((event) => event.risks))).sort();
    const hasPartialEvent = events.some((event) => event.confidence === "partial");
    const propagationConfidence = truncated || hasPartialEvent || riskMarkers.length > 0 ? "partial" : "high";

    const confidenceNotes: string[] = [];
    if (events.length === 0) {
      confidenceNotes.push("No supported propagation events were available for the requested exact symbol scope.");
    }
    if (hasPartialEvent) {
      confidenceNotes.push("At least one propagation hop is structurally partial rather than high-confidence.");
    }
    if (truncated) {
      confidenceNotes.push("Traversal or response bounds truncated the propagation answer before all reachable hops were explored.");
    }
    if (riskMarkers.includes("pointerHeavyFlow")) {
      confidenceNotes.push("Pointer-heavy flow is present, so alias-sensitive propagation may be incomplete.");
    }
    if (riskMarkers.includes("receiverAmbiguity")) {
      confidenceNotes.push("Receiver identity is structurally weaker on at least one hop, so object-state ownership may be approximate.");
    }
    if (riskMarkers.includes("unresolvedOverload")) {
      confidenceNotes.push("At least one propagation hop depends on a weaker callable match and may need follow-up lookup.");
    }
    if (riskMarkers.includes("unsupportedFlowShape")) {
      confidenceNotes.push("Some nearby flow shapes are outside the first-release supported propagation model.");
    }
    if (riskMarkers.length === 0 && !truncated && !hasPartialEvent && events.length > 0) {
      confidenceNotes.push("All returned propagation hops come from supported structural patterns without additional risk markers.");
    }

    return { propagationConfidence, riskMarkers, confidenceNotes };
  }

  function buildFileRiskNotes(symbol: CodeSymbol): string[] {
    const notes: string[] = [];
    if (symbol.parseFragility === "elevated") {
      notes.push("This symbol lives in a parse-fragile file, so structurally exact results may still sit near unstable syntax.");
    }
    if (symbol.macroSensitivity === "high") {
      notes.push("This symbol lives in a macro-sensitive file, so macro-expanded meaning may be weaker than the structural index suggests.");
    }
    if (symbol.includeHeaviness === "heavy") {
      notes.push("This symbol lives in an include-heavy file, so build-context interactions may matter more than usual.");
    }
    return notes;
  }

  function buildPropagationFollowUpQueries(
    symbol: CodeSymbol,
    riskMarkers: PropagationEventRecord["risks"],
  ): string[] {
    const queries = [
      `find_references qualifiedName=${symbol.qualifiedName}`,
      `lookup_symbol qualifiedName=${symbol.qualifiedName}`,
    ];

    if (riskMarkers.includes("receiverAmbiguity") && symbol.parentId) {
      queries.unshift(`list_class_members qualifiedName=${symbol.parentId}`);
    }
    if (riskMarkers.includes("unresolvedOverload")) {
      queries.unshift(`find_overrides qualifiedName=${symbol.qualifiedName}`);
    }
    if (riskMarkers.includes("pointerHeavyFlow")) {
      queries.push(`trace_variable_flow qualifiedName=${symbol.qualifiedName} maxDepth=1`);
    }
    if (symbol.parseFragility === "elevated" || symbol.macroSensitivity === "high") {
      queries.push(`list_file_symbols filePath=${symbol.filePath}`);
    }

    return Array.from(new Set(queries));
  }

  function buildExplainPropagation(
    symbol: CodeSymbol,
    matchedBy: "id" | "qualifiedName" | "both",
    limit: number,
    propagationKinds?: PropagationKind[],
    filePath?: string,
  ): ExplainSymbolPropagationResponse {
    const incomingAll = store.getIncomingPropagation(symbol.id, propagationKinds, filePath);
    const outgoingAll = store.getOutgoingPropagation(symbol.id, propagationKinds, filePath);
    const incoming = applyLimit(incomingAll, limit);
    const outgoing = applyLimit(outgoingAll, limit);
    const returnedCount = incoming.results.length + outgoing.results.length;
    const totalCount = incoming.totalCount + outgoing.totalCount;
    const assessment = buildPropagationAssessment(
      [...incoming.results, ...outgoing.results],
      incoming.truncated || outgoing.truncated,
    );

    return {
      ...buildExactLookupResponse({ symbol, matchedBy }),
      window: buildResultWindow(returnedCount, totalCount, incoming.truncated || outgoing.truncated, limit),
      propagationConfidence: assessment.propagationConfidence,
      incoming: incoming.results,
      outgoing: outgoing.results,
      riskMarkers: assessment.riskMarkers,
      confidenceNotes: [...assessment.confidenceNotes, ...buildFileRiskNotes(symbol)],
      summary: [
        `incoming: ${incoming.totalCount} event(s)`,
        `outgoing: ${outgoing.totalCount} event(s)`,
        ...buildPropagationSummary([...incoming.results, ...outgoing.results]),
      ],
      suggestedFollowUpQueries: buildPropagationFollowUpQueries(symbol, assessment.riskMarkers),
    };
  }

  function traceVariableFlow(
    symbol: CodeSymbol,
    matchedBy: "id" | "qualifiedName" | "both",
    maxDepth: number,
    maxEdges: number,
    propagationKinds?: PropagationKind[],
    filePath?: string,
  ): TraceVariableFlowResponse {
    const outgoing = store.getOutgoingPropagation(symbol.id, propagationKinds, filePath);
    const adjacency = new Map<string, PropagationEventRecord[]>();
    for (const event of outgoing) {
      const key = anchorKey(event.sourceAnchor);
      if (!key) continue;
      const bucket = adjacency.get(key) ?? [];
      bucket.push(event);
      adjacency.set(key, bucket);
    }
    for (const bucket of adjacency.values()) {
      bucket.sort((a, b) =>
        a.filePath.localeCompare(b.filePath)
        || a.line - b.line
        || a.propagationKind.localeCompare(b.propagationKind),
      );
    }

    const targetedAnchorKeys = new Set(
      outgoing
        .map((event) => anchorKey(event.targetAnchor))
        .filter((key): key is string => key !== null),
    );
    const seeds = (outgoing.filter((event) => {
      const key = anchorKey(event.sourceAnchor);
      return key ? !targetedAnchorKeys.has(key) : true;
    }).length > 0
      ? outgoing.filter((event) => {
        const key = anchorKey(event.sourceAnchor);
        return key ? !targetedAnchorKeys.has(key) : true;
      })
      : outgoing
    ).slice().sort((a, b) =>
      a.filePath.localeCompare(b.filePath)
      || a.line - b.line
      || a.propagationKind.localeCompare(b.propagationKind),
    );
    const queue: Array<{ event: PropagationEventRecord; path: PropagationPathStep[]; depth: number }> = seeds.map((event) => ({
      event,
      path: [{ ...event, hop: 1 }],
      depth: 1,
    }));
    const visited = new Set<string>();
    let bestPath: PropagationPathStep[] = [];
    let truncated = false;
    let exploredEdges = 0;

    while (queue.length > 0 && exploredEdges < maxEdges) {
      const current = queue.shift()!;
      exploredEdges += 1;
      const eventKey = `${current.event.filePath}:${current.event.line}:${current.event.propagationKind}:${anchorKey(current.event.sourceAnchor)}:${anchorKey(current.event.targetAnchor)}`;
      if (visited.has(eventKey)) {
        continue;
      }
      visited.add(eventKey);

      if (
        current.path.length > bestPath.length
        || (current.path.length === bestPath.length
          && current.path[0]
          && bestPath[0]
          && current.path[0].line < bestPath[0].line)
      ) {
        bestPath = current.path;
      }

      if (current.depth >= maxDepth) {
        const nextKey = anchorKey(current.event.targetAnchor);
        if (nextKey && (adjacency.get(nextKey)?.length ?? 0) > 0) {
          truncated = true;
        }
        continue;
      }

      const nextKey = anchorKey(current.event.targetAnchor);
      if (!nextKey) {
        continue;
      }
      const nextEvents = adjacency.get(nextKey) ?? [];
      if (nextEvents.length > 0 && exploredEdges + nextEvents.length > maxEdges) {
        truncated = true;
      }
      for (const next of nextEvents) {
        queue.push({
          event: next,
          depth: current.depth + 1,
          path: current.path.concat({ ...next, hop: current.path.length + 1 }),
        });
      }
    }

    if (queue.length > 0) {
      truncated = true;
    }

    const assessment = buildPropagationAssessment(bestPath, truncated);

    return {
      ...buildExactLookupResponse({ symbol, matchedBy }),
      window: buildResultWindow(bestPath.length, bestPath.length, truncated, maxEdges),
      propagationConfidence: assessment.propagationConfidence,
      riskMarkers: assessment.riskMarkers,
      confidenceNotes: [...assessment.confidenceNotes, ...buildFileRiskNotes(symbol)],
      pathFound: bestPath.length > 0,
      truncated,
      maxDepth,
      maxEdges,
      ...(propagationKinds && propagationKinds.length > 0 ? { propagationKinds } : {}),
      steps: bestPath,
      suggestedFollowUpQueries: [
        `explain_symbol_propagation qualifiedName=${symbol.qualifiedName}`,
        ...buildPropagationFollowUpQueries(symbol, assessment.riskMarkers),
      ],
    };
  }

  function buildImpactAnalysis(
    symbol: CodeSymbol,
    maxDepth: number,
    limit: number,
    metadataFilters?: MetadataFilters,
  ): ImpactAnalysisResponse {
    const directCallers = buildUniqueCallRefs(store.getCallers(symbol.id), "callerId", limit, metadataFilters);
    const directCallees = buildUniqueCallRefs(store.getCallees(symbol.id), "calleeId", limit, metadataFilters);
    const directReferences = buildResolvedReferences(symbol.id, undefined, undefined, limit, metadataFilters);

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
      if (matchesMetadataFilters(affectedSymbol, metadataFilters) && affectedSymbol) {
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
        const nextCallers = buildUniqueCallRefs(store.getCallers(current.symbolId), "callerId", limit, metadataFilters).results;
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
        const nextCallees = buildUniqueCallRefs(store.getCallees(current.symbolId), "calleeId", limit, metadataFilters).results;
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

    const impactedSymbolEntries = Array.from(impactedSymbolCounts.entries())
      .map(([symbolId, count]) => ({ symbolId, count }))
      .sort((a, b) => b.count - a.count || a.symbolId.localeCompare(b.symbolId));
    const impactedSymbolMap = buildSymbolMap(impactedSymbolEntries.map((entry) => entry.symbolId));
    const resolvedTopAffectedSymbols: ImpactedSymbolSummary[] = impactedSymbolEntries
      .map(({ symbolId, count }) => {
        const impacted = impactedSymbolMap.get(symbolId);
        if (!impacted || !matchesMetadataFilters(impacted, metadataFilters)) return null;
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
    const affectedSymbolIds = Array.from(impactedSymbolCounts.keys());

    return {
      ...buildExactLookupResponse({ symbol, matchedBy: "qualifiedName" }),
      maxDepth,
      directCallers: directCallers.results,
      directCallees: directCallees.results,
      directReferences: directReferences.results,
      totalAffectedSymbols: impactedSymbolCounts.size,
      totalAffectedFiles: impactedFileCounts.size,
      topAffectedSymbols: resolvedTopAffectedSymbols,
      topAffectedFiles,
      suggestedFollowUpQueries,
      ...metadataFilterEcho(metadataFilters),
      affectedSubsystems: buildMetadataGroupSummary(affectedSymbolIds, (affected) => affected.subsystem),
      affectedModules: buildMetadataGroupSummary(affectedSymbolIds, (affected) => affected.module),
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

  function buildHierarchyResponse(
    symbol: CodeSymbol,
    limit: number,
  ): TypeHierarchyResponse {
    const directBases = store.getDirectBases(symbol.id)
      .map((candidate) => ({
        symbolId: candidate.id,
        qualifiedName: candidate.qualifiedName,
        type: candidate.type,
        filePath: candidate.filePath,
        line: candidate.line,
      }))
      .sort((a, b) => a.qualifiedName.localeCompare(b.qualifiedName));
    const directDerived = store.getDirectDerived(symbol.id)
      .map((candidate) => ({
        symbolId: candidate.id,
        qualifiedName: candidate.qualifiedName,
        type: candidate.type,
        filePath: candidate.filePath,
        line: candidate.line,
      }))
      .sort((a, b) => a.qualifiedName.localeCompare(b.qualifiedName));
    const totalCount = directBases.length + directDerived.length;

    return {
      ...buildExactLookupResponse({ symbol, matchedBy: "qualifiedName" }),
      directBases: directBases.slice(0, limit),
      directDerived: directDerived.slice(0, Math.max(0, limit - Math.min(limit, directBases.length))),
      window: buildResultWindow(
        Math.min(limit, totalCount),
        totalCount,
        totalCount > limit,
        limit,
      ),
    };
  }

  function traceShortestCallPath(
    source: CodeSymbol,
    target: CodeSymbol,
    maxDepth: number,
  ): TraceCallPathResponse {
    type QueueItem = { symbolId: string; depth: number; steps: TraceCallPathResponse["steps"] };
    const visited = new Set<string>([source.id]);
    const queue: QueueItem[] = [{ symbolId: source.id, depth: 0, steps: [] }];
    let truncated = false;

    while (queue.length > 0) {
      const current = queue.shift()!;
      if (current.depth >= maxDepth) {
        if (current.symbolId !== target.id && store.getCallees(current.symbolId).length > 0) {
          truncated = true;
        }
        continue;
      }

      const outgoing = store.getCallees(current.symbolId)
        .slice()
        .sort((a, b) => a.line - b.line || a.calleeId.localeCompare(b.calleeId));

      for (const call of outgoing) {
        const caller = store.getSymbolById(call.callerId);
        const callee = store.getSymbolById(call.calleeId);
        if (!caller || !callee) {
          continue;
        }

        const nextSteps = current.steps.concat({
          callerId: caller.id,
          callerQualifiedName: caller.qualifiedName,
          calleeId: callee.id,
          calleeQualifiedName: callee.qualifiedName,
          filePath: call.filePath,
          line: call.line,
        });

        if (callee.id === target.id) {
          return {
            source,
            target,
            maxDepth,
            pathFound: true,
            truncated,
            steps: nextSteps,
          };
        }

        if (visited.has(callee.id)) {
          continue;
        }

        visited.add(callee.id);
        queue.push({
          symbolId: callee.id,
          depth: current.depth + 1,
          steps: nextSteps,
        });
      }
    }

    return {
      source,
      target,
      maxDepth,
      pathFound: false,
      truncated,
      steps: [],
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
    const metadataFilters = parseMetadataFilters(req.query as Record<string, unknown>);
    const { symbol: sym, candidateCount } = resolveFunctionSymbol(name);

    if (!sym) {
      return notFound(res);
    }

    const callers = buildUniqueCallRefs(store.getCallers(sym.id), "callerId", limit, metadataFilters);
    const response: CallerQueryResponse = buildCallerQueryResponse({
      symbol: sym,
      candidateCount,
      callers: callers.results,
      totalCount: callers.totalCount,
      truncated: callers.truncated,
      limitApplied: limit,
    });
    const callerIds = callers.results.map((caller) => caller.symbolId);
    Object.assign(response, metadataFilterEcho(metadataFilters), {
      groupedBySubsystem: buildMetadataGroupSummary(callerIds, (caller) => caller.subsystem),
      groupedByModule: buildMetadataGroupSummary(callerIds, (caller) => caller.module),
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
    const metadataFilters = parseMetadataFilters(req.query as Record<string, unknown>);

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

    const references = buildResolvedReferences(symbol.id, category, filePath, limit, metadataFilters);
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
      ...metadataFilterEcho(metadataFilters),
      groupedBySubsystem: buildMetadataGroupSummary(references.results.map((reference) => reference.sourceSymbolId), (source) => source.subsystem),
      groupedByModule: buildMetadataGroupSummary(references.results.map((reference) => reference.sourceSymbolId), (source) => source.module),
    };
    return res.json(response);
  });

  app.get("/symbol-propagation", (req, res) => {
    const id = typeof req.query.id === "string" ? req.query.id : undefined;
    const qualifiedName = typeof req.query.qualifiedName === "string" ? req.query.qualifiedName : undefined;
    const filePath = typeof req.query.filePath === "string" ? req.query.filePath : undefined;
    const limit = Math.min(parseInt((req.query.limit as string) || String(SEARCH_DEFAULT_LIMIT), 10), SEARCH_MAX_LIMIT);
    const propagationKinds = parsePropagationKinds(req.query as Record<string, unknown>);

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

    return res.json(buildExplainPropagation(
      symbol,
      id && qualifiedName ? "both" : id ? "id" : "qualifiedName",
      limit,
      propagationKinds,
      filePath,
    ));
  });

  app.get("/trace-variable-flow", (req, res) => {
    const id = typeof req.query.id === "string" ? req.query.id : undefined;
    const qualifiedName = typeof req.query.qualifiedName === "string" ? req.query.qualifiedName : undefined;
    const filePath = typeof req.query.filePath === "string" ? req.query.filePath : undefined;
    const maxDepth = Math.min(parseInt((req.query.maxDepth as string) || "3", 10), CALLGRAPH_MAX_DEPTH);
    const maxEdges = Math.min(parseInt((req.query.maxEdges as string) || String(SEARCH_DEFAULT_LIMIT), 10), SEARCH_MAX_LIMIT);
    const propagationKinds = parsePropagationKinds(req.query as Record<string, unknown>);

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

    return res.json(traceVariableFlow(
      symbol,
      id && qualifiedName ? "both" : id ? "id" : "qualifiedName",
      maxDepth,
      maxEdges,
      propagationKinds,
      filePath,
    ));
  });

  app.get("/impact", (req, res) => {
    const id = typeof req.query.id === "string" ? req.query.id : undefined;
    const qualifiedName = typeof req.query.qualifiedName === "string" ? req.query.qualifiedName : undefined;
    const maxDepth = Math.min(parseInt((req.query.depth as string) || "2", 10), CALLGRAPH_MAX_DEPTH);
    const limit = Math.min(parseInt((req.query.limit as string) || String(SEARCH_DEFAULT_LIMIT), 10), SEARCH_MAX_LIMIT);
    const metadataFilters = parseMetadataFilters(req.query as Record<string, unknown>);

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

    return res.json(buildImpactAnalysis(symbol, maxDepth, limit, metadataFilters));
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

  app.get("/type-hierarchy", (req, res) => {
    const qualifiedName = typeof req.query.qualifiedName === "string" ? req.query.qualifiedName : undefined;
    const limit = Math.min(parseInt((req.query.limit as string) || String(SEARCH_DEFAULT_LIMIT), 10), SEARCH_MAX_LIMIT);
    if (!qualifiedName) {
      return badRequest(res);
    }

    const symbol = store.getSymbolByQualifiedName(qualifiedName);
    if (!symbol || (symbol.type !== "class" && symbol.type !== "struct")) {
      return notFound(res);
    }

    return res.json(buildHierarchyResponse(symbol, limit));
  });

  app.get("/base-methods", (req, res) => {
    const qualifiedName = typeof req.query.qualifiedName === "string" ? req.query.qualifiedName : undefined;
    const limit = Math.min(parseInt((req.query.limit as string) || String(SEARCH_DEFAULT_LIMIT), 10), SEARCH_MAX_LIMIT);
    if (!qualifiedName) {
      return badRequest(res);
    }

    const symbol = store.getSymbolByQualifiedName(qualifiedName);
    if (!symbol || symbol.type !== "method") {
      return notFound(res);
    }

    const baseMethods = store.getBaseMethods(symbol.id);
    const windowed = applyLimit(baseMethods, limit);
    const response: BaseMethodsResponse = {
      ...buildExactLookupResponse({ symbol, matchedBy: "qualifiedName" }),
      window: buildResultWindow(windowed.results.length, windowed.totalCount, windowed.truncated, limit),
      baseMethods: windowed.results,
    };
    return res.json(response);
  });

  app.get("/overrides", (req, res) => {
    const qualifiedName = typeof req.query.qualifiedName === "string" ? req.query.qualifiedName : undefined;
    const limit = Math.min(parseInt((req.query.limit as string) || String(SEARCH_DEFAULT_LIMIT), 10), SEARCH_MAX_LIMIT);
    if (!qualifiedName) {
      return badRequest(res);
    }

    const symbol = store.getSymbolByQualifiedName(qualifiedName);
    if (!symbol || symbol.type !== "method") {
      return notFound(res);
    }

    const overrides = store.getOverrides(symbol.id);
    const windowed = applyLimit(overrides, limit);
    const response: OverrideQueryResponse = {
      ...buildExactLookupResponse({ symbol, matchedBy: "qualifiedName" }),
      window: buildResultWindow(windowed.results.length, windowed.totalCount, windowed.truncated, limit),
      overrides: windowed.results,
    };
    return res.json(response);
  });

  app.get("/search", (req, res) => {
    const q = (req.query.q as string) || "";
    const type = req.query.type as string | undefined;
    const limit = Math.min(parseInt((req.query.limit as string) || String(SEARCH_DEFAULT_LIMIT), 10), SEARCH_MAX_LIMIT);
    const metadataFilters = parseMetadataFilters(req.query as Record<string, unknown>);

    if (!q) {
      return res.status(400).json({ error: "Missing query parameter 'q'", code: "BAD_REQUEST" } as ErrorResponse);
    }

    const { results, totalCount } = store.searchSymbols(q, type, limit, metadataFilters);
    const response: SearchResponse = {
      query: q,
      window: buildResultWindow(results.length, totalCount, totalCount > limit, limit),
      results,
      totalCount,
      truncated: totalCount > limit,
      ...metadataFilterEcho(metadataFilters),
    };
    return res.json(response);
  });

  app.get("/trace-call-path", (req, res) => {
    const sourceQualifiedName = typeof req.query.sourceQualifiedName === "string" ? req.query.sourceQualifiedName : undefined;
    const targetQualifiedName = typeof req.query.targetQualifiedName === "string" ? req.query.targetQualifiedName : undefined;
    const maxDepth = Math.min(parseInt((req.query.maxDepth as string) || String(CALLGRAPH_DEFAULT_DEPTH), 10), CALLGRAPH_MAX_DEPTH);

    if (!sourceQualifiedName || !targetQualifiedName) {
      return badRequest(res);
    }

    const source = store.getSymbolByQualifiedName(sourceQualifiedName);
    const target = store.getSymbolByQualifiedName(targetQualifiedName);
    if (!source || !target) {
      return notFound(res);
    }
    if ((source.type !== "function" && source.type !== "method") || (target.type !== "function" && target.type !== "method")) {
      return notFound(res);
    }

    return res.json(traceShortestCallPath(source, target, maxDepth));
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
