import * as fs from "fs";
import * as path from "path";
import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import { z } from "zod";
import {
  SEARCH_DEFAULT_LIMIT, SEARCH_MAX_LIMIT, SEARCH_MIN_QUERY_LENGTH,
  CALLERS_DEFAULT_LIMIT, CALLERS_MAX_LIMIT,
  CALLGRAPH_DEFAULT_DEPTH, CALLGRAPH_MAX_DEPTH,
  CALLGRAPH_DEFAULT_NODE_CAP, CALLGRAPH_MAX_NODE_CAP,
  DATA_DIR_NAME,
} from "./constants";
import { Store } from "./storage/store";
import { MetadataFilters } from "./storage/store";
import { SourceLanguage } from "./models/symbol";
import { resolveActiveDatabasePath, SqliteStore } from "./storage/sqlite-store";
import { JsonStore } from "./storage/json-store";
import {
  BaseMethodsResponse,
  ClassMembersOverviewResponse,
  CallReference,
  CallGraphDirection,
  CallGraphEdge,
  CallerQueryResponse,
  CompactCallGraphResponse,
  CompactFileSymbolsResponse,
  CompactReferenceQueryResponse,
  CompactReferenceRecord,
  ExplainSymbolPropagationResponse,
  FileSymbolsResponse,
  ImpactAnalysisResponse,
  InvestigationWorkflowStep,
  InvestigateWorkflowResponse,
  ImpactedFileSummary,
  ImpactedSymbolSummary,
  MetadataGroupSummary,
  NamespaceSymbolsResponse,
  OverrideQueryResponse,
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
  WorkspaceSummaryResponse,
} from "./models/responses";
import {
  toCompactCallGraphResponse,
  toCompactFileSymbolsResponse,
  toCompactReferenceQueryResponse,
} from "./compact-responses";
import {
  buildClassResponse,
  buildCallerQueryResponse,
  buildOverloadQueryResponse,
  deriveRepresentativeMetadata,
  buildResultWindow,
  buildExactLookupResponse,
  buildFunctionResponse,
  HeuristicLookupContext,
  makeResolvedCallReference,
  rankHeuristicCandidatesDetailed,
} from "./response-metadata";
import { buildResponseReliability } from "./reliability";
import { initRuntimeStats, prepareRuntimeStatsPath, recordMcpToolCall } from "./runtime-stats";
import {
  buildInvestigateWorkflowResponse,
  createWorkflowProfiler,
  createWorkflowQueryContext,
  buildPropagationWorkflowSteps as sharedBuildPropagationWorkflowSteps,
  toInvestigationAnchorSummary as sharedToInvestigationAnchorSummary,
} from "./investigation-workflow";

export const DEFAULT_DATA_DIR = process.argv[2] || process.env.CODEATLAS_DATA || DATA_DIR_NAME;

export function openStore(dataDir: string): Store {
  const dbPath = resolveActiveDatabasePath(dataDir);
  if (dbPath && fs.existsSync(dbPath)) {
    return new SqliteStore(dbPath);
  }
  return new JsonStore(dataDir);
}

export function createMcpServer(dataDir: string = DEFAULT_DATA_DIR): {
  server: McpServer;
  store: Store;
  close: () => void;
} {
  initRuntimeStats(prepareRuntimeStatsPath(dataDir));
  const store = openStore(dataDir);
  const server = new McpServer({
    name: "codeatlas",
    version: "0.1.0",
  });
  const originalTool = server.tool.bind(server);
  (server as unknown as { tool: typeof server.tool }).tool = ((name: string, description: string, schema: any, handler: any) =>
    originalTool(name, description, schema, async (args: any) => {
      const startedAt = Date.now();
      try {
        const result = await handler(args);
        recordMcpToolCall({
          toolName: name,
          elapsedMs: Date.now() - startedAt,
          ok: true,
        });
        return result;
      } catch (error) {
        recordMcpToolCall({
          toolName: name,
          elapsedMs: Date.now() - startedAt,
          ok: false,
          errorMessage: error instanceof Error ? error.message : String(error),
        });
        throw error;
      }
    })) as typeof server.tool;

  function buildSymbolMap(ids: Iterable<string>) {
    const uniqueIds = Array.from(new Set(Array.from(ids)));
    return new Map(store.getSymbolsByIds(uniqueIds).map((symbol) => [symbol.id, symbol]));
  }

  function buildCallReferences(
    calls: { callerId?: string; calleeId?: string; filePath: string; line: number }[],
    targetField: "callerId" | "calleeId",
  ): CallReference[] {
    const symbolMap = buildSymbolMap(
      calls
        .map((call) => call[targetField])
        .filter((symbolId): symbolId is string => Boolean(symbolId)),
    );
    return calls
      .map((c) => {
        const targetId = c[targetField];
        if (!targetId) return null;
        const s = symbolMap.get(targetId) ?? store.getSymbolById(targetId);
        if (!s) return null;
        return makeResolvedCallReference({
          symbol: s,
          filePath: c.filePath,
          line: c.line,
        });
      })
      .filter((r): r is CallReference => r !== null);
  }

  function metadataFilterEcho(filters?: MetadataFilters): Partial<MetadataFilters> {
    return filters ?? {};
  }

  function buildHeuristicLookupContext(params: {
    language?: SourceLanguage;
    subsystem?: string;
    module?: string;
    projectArea?: string;
    artifactKind?: "runtime" | "editor" | "tool" | "test" | "generated";
    filePath?: string;
    anchorQualifiedName?: string;
    recentQualifiedName?: string;
  }): HeuristicLookupContext | undefined {
    const { language, subsystem, module, projectArea, artifactKind, filePath, anchorQualifiedName, recentQualifiedName } = params;
    const anchorSymbol = resolveHeuristicAnchorSymbol({ anchorQualifiedName, recentQualifiedName });
    if (!language && !subsystem && !module && !projectArea && !artifactKind && !filePath && !anchorSymbol) {
      return undefined;
    }
    return {
      language: language ?? anchorSymbol?.language,
      subsystem: subsystem ?? anchorSymbol?.subsystem,
      module: module ?? anchorSymbol?.module,
      projectArea: projectArea ?? anchorSymbol?.projectArea,
      artifactKind: artifactKind ?? anchorSymbol?.artifactKind,
      filePath: filePath ?? anchorSymbol?.filePath,
      anchorQualifiedName: anchorSymbol?.qualifiedName,
      anchorNeighborSymbolIds: anchorSymbol
        ? Array.from(new Set([
          ...store.getCallers(anchorSymbol.id).map((call) => call.callerId),
          ...store.getCallees(anchorSymbol.id).map((call) => call.calleeId),
        ]))
        : undefined,
      anchorScopePrefixes: anchorSymbol ? collectAnchorScopePrefixes(anchorSymbol.qualifiedName) : undefined,
    };
  }

  function buildCallerLookupContext(params: {
    anchorQualifiedName?: string;
    recentQualifiedName?: string;
  }): HeuristicLookupContext | undefined {
    return buildHeuristicLookupContext({
      anchorQualifiedName: params.anchorQualifiedName,
      recentQualifiedName: params.recentQualifiedName,
    });
  }

  function resolveHeuristicAnchorSymbol(params: {
    anchorQualifiedName?: string;
    recentQualifiedName?: string;
  }) {
    const explicitAnchor = params.anchorQualifiedName
      ? store.getSymbolByQualifiedName(params.anchorQualifiedName)
      : undefined;
    if (explicitAnchor) {
      return explicitAnchor;
    }

    const recentSymbol = params.recentQualifiedName
      ? store.getSymbolByQualifiedName(params.recentQualifiedName)
      : undefined;
    if (!recentSymbol) {
      return undefined;
    }
    if (recentSymbol.type === "function"
      || recentSymbol.type === "method"
      || recentSymbol.type === "class"
      || recentSymbol.type === "struct"
      || recentSymbol.type === "namespace") {
      return recentSymbol;
    }
    if (recentSymbol.parentId) {
      const parentSymbol = store.getSymbolById(recentSymbol.parentId);
      if (parentSymbol) {
        return parentSymbol;
      }
    }
    if (recentSymbol.scopeQualifiedName) {
      const scopeSymbol = store.getSymbolByQualifiedName(recentSymbol.scopeQualifiedName);
      if (scopeSymbol) {
        return scopeSymbol;
      }
    }
    return recentSymbol;
  }

  function collectAnchorScopePrefixes(qualifiedName: string): string[] {
    const parts = qualifiedName.split("::");
    const prefixes: string[] = [];
    for (let index = 1; index < parts.length; index += 1) {
      prefixes.push(parts.slice(0, index).join("::"));
    }
    return prefixes;
  }

  function matchesMetadataFilters(symbol: ReturnType<Store["getSymbolById"]>, filters?: MetadataFilters): boolean {
    if (!filters) return true;
    if (!symbol) return false;
    if (filters.language && symbol.language !== filters.language) return false;
    if (filters.subsystem && symbol.subsystem !== filters.subsystem) return false;
    if (filters.module && symbol.module !== filters.module) return false;
    if (filters.projectArea && symbol.projectArea !== filters.projectArea) return false;
    if (filters.artifactKind && symbol.artifactKind !== filters.artifactKind) return false;
    return true;
  }

  function buildMetadataGroupSummary(
    symbolIds: Iterable<string>,
    keySelector: (symbol: NonNullable<ReturnType<Store["getSymbolById"]>>) => string | undefined,
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

  function buildWorkspaceSummary(): WorkspaceSummaryResponse {
    const languages = store.getWorkspaceLanguageSummary();
    return {
      languages,
      totalFiles: languages.reduce((sum, entry) => sum + entry.fileCount, 0),
      totalSymbols: languages.reduce((sum, entry) => sum + entry.symbolCount, 0),
    };
  }

  function buildUniqueCallReferences(
    calls: { callerId?: string; calleeId?: string; filePath: string; line: number }[],
    targetField: "callerId" | "calleeId",
    limit: number,
    metadataFilters?: MetadataFilters,
  ): { results: CallReference[]; totalCount: number; truncated: boolean } {
    const refs = buildCallReferences(calls, targetField);
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

  function resolveFunctionSymbol(name: string, context?: HeuristicLookupContext) {
    const symbols = store.getSymbolsByName(name);
    const rankedCandidates = rankHeuristicCandidatesDetailed(
      symbols.filter((s) => s.type === "function" || s.type === "method"),
      context,
    );
    const symbol = rankedCandidates[0]?.symbol;
    return {
      symbol,
      candidateCount: rankedCandidates.length,
      rankedCandidates,
    };
  }

  function buildExactSymbolPayload(params: {
    matchedBy: "id" | "qualifiedName" | "both";
    symbol: ReturnType<Store["getSymbolById"]>;
  }): SymbolLookupResponse | null {
    const { symbol, matchedBy } = params;
    if (!symbol) return null;

    const representativeMetadata = deriveRepresentativeMetadata(symbol, store.getRepresentativeCandidates(symbol.id));
    const base = buildExactLookupResponse({ symbol, matchedBy, representativeMetadata });

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

    if (symbol.type === "enum") {
      return {
        ...base,
        members: store.getMembers(symbol.id),
      } as SymbolLookupResponse;
    }

    return base;
  }

  function buildResolvedReferences(
    targetSymbolIds: string[],
    category?: ReferenceCategory,
    filePath?: string,
    limit = SEARCH_DEFAULT_LIMIT,
    metadataFilters?: MetadataFilters,
  ): { results: ResolvedReference[]; totalCount: number; truncated: boolean } {
    const rawReferences = targetSymbolIds.flatMap((targetSymbolId) => store.getReferences(targetSymbolId, category, filePath));
    const symbolMap = buildSymbolMap(
      rawReferences.flatMap((reference) => [reference.sourceSymbolId, reference.targetSymbolId]),
    );
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
      .filter((reference, index, all) =>
        all.findIndex((candidate) =>
          candidate.sourceSymbolId === reference.sourceSymbolId
          && candidate.targetSymbolId === reference.targetSymbolId
          && candidate.category === reference.category
          && candidate.filePath === reference.filePath
          && candidate.line === reference.line) === index)
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

  function buildCompactResolvedReferences(references: ResolvedReference[]): CompactReferenceRecord[] {
    const targetMap = buildSymbolMap(references.map((reference) => reference.targetSymbolId));
    return references
      .map((reference) => {
        const targetSymbol = targetMap.get(reference.targetSymbolId);
        if (!targetSymbol) return null;
        return {
          sourceSymbolId: reference.sourceSymbolId,
          sourceQualifiedName: reference.sourceQualifiedName,
          targetSymbolId: reference.targetSymbolId,
          targetQualifiedName: targetSymbol.qualifiedName,
          category: reference.category,
          filePath: reference.filePath,
          line: reference.line,
        };
      })
      .filter((reference): reference is CompactReferenceRecord => reference !== null);
  }

  function buildReferenceTargetIds(
    symbol: NonNullable<ReturnType<Store["getSymbolById"]>>,
    category?: ReferenceCategory,
    includeEnumValueUsage = false,
  ): string[] {
    if (!(includeEnumValueUsage && symbol.type === "enum")) {
      return [symbol.id];
    }

    const enumMemberIds = store.getMembers(symbol.id)
      .filter((member) => member.type === "enumMember")
      .map((member) => member.id);
    if (category === "enumValueUsage") {
      return enumMemberIds;
    }
    return [symbol.id, ...enumMemberIds];
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

    const counts = new Map<PropagationKind, number>();
    for (const event of events) {
      counts.set(event.propagationKind, (counts.get(event.propagationKind) ?? 0) + 1);
    }

    return Array.from(counts.entries())
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

  function buildFileRiskNotes(symbol: NonNullable<ReturnType<Store["getSymbolById"]>>): string[] {
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
    symbol: NonNullable<ReturnType<Store["getSymbolById"]>>,
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

  function buildInvestigateWorkflowPayload(
    source: NonNullable<ReturnType<Store["getSymbolById"]>>,
    target: NonNullable<ReturnType<Store["getSymbolById"]>> | undefined,
    maxDepth: number,
    maxEdges: number,
  ): InvestigateWorkflowResponse {
    const base = buildExactLookupResponse({ symbol: source, matchedBy: "qualifiedName" });
    const workflowQueryContext = createWorkflowQueryContext(store);
    const workflowProfiler = createWorkflowProfiler({
      channel: "mcp",
      source,
      target,
      maxDepth,
      maxEdges,
    });
    let steps: InvestigationWorkflowStep[] = [];
    let truncated = false;
    let pathFound = false;

    if (
      target
      && (source.type === "function" || source.type === "method")
      && (target.type === "function" || target.type === "method")
    ) {
      const callPath = workflowProfiler.measure("trace_call_path", () => traceShortestCallPath(source, target, maxDepth));
      pathFound = callPath.pathFound;
      truncated = callPath.truncated;
      steps = callPath.steps.map((step, index) => {
        const fromSymbol = store.getSymbolById(step.callerId);
        const toSymbol = store.getSymbolById(step.calleeId);
        return {
          hop: index + 1,
          handoffKind: "call",
          from: sharedToInvestigationAnchorSummary({ symbol: fromSymbol, fallbackFilePath: step.filePath, fallbackLine: step.line }),
          to: sharedToInvestigationAnchorSummary({ symbol: toSymbol, fallbackFilePath: step.filePath, fallbackLine: step.line }),
          filePath: step.filePath,
          line: step.line,
          confidence: "high",
          risks: [],
        };
      });
    } else {
      const propagation = workflowProfiler.measure("trace_variable_flow", () => buildTraceVariableFlowPayload(source, "qualifiedName", maxDepth, maxEdges));
      pathFound = propagation.pathFound;
      truncated = propagation.truncated;
      steps = workflowProfiler.measure("map_main_path", () => sharedBuildPropagationWorkflowSteps(workflowQueryContext, propagation.steps, 1));
    }

    const response = buildInvestigateWorkflowResponse({
      queryContext: workflowQueryContext,
      source,
      target,
      mainPath: steps,
      pathFound,
      truncated,
      maxEdges,
      lookupMode: base.lookupMode,
      confidence: base.confidence,
      matchReasons: base.matchReasons,
      getFileRiskNotes: buildFileRiskNotes,
      profiler: workflowProfiler,
    });
    workflowProfiler.flush({
      pathFound,
      mainPathLength: steps.length,
      handoffPointCount: response.handoffPoints.length,
      evidenceCount: response.evidence.length,
      pathConfidence: response.pathConfidence,
      coverageConfidence: response.coverageConfidence,
    });
    return response;
  }

  function buildExplainPropagationPayload(
    symbol: NonNullable<ReturnType<Store["getSymbolById"]>>,
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
    const riskMarkers = Array.from(new Set(
      [...incoming.results, ...outgoing.results].flatMap((event) => event.risks),
    )).sort();

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

  function buildTraceVariableFlowPayload(
    symbol: NonNullable<ReturnType<Store["getSymbolById"]>>,
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
    symbol: NonNullable<ReturnType<Store["getSymbolById"]>>,
    maxDepth: number,
    limit: number,
    metadataFilters?: MetadataFilters,
  ): ImpactAnalysisResponse {
    const directCallers = buildUniqueCallReferences(store.getCallers(symbol.id), "callerId", limit, metadataFilters);
    const directCallees = buildUniqueCallReferences(store.getCallees(symbol.id), "calleeId", limit, metadataFilters);
    const directReferences = buildResolvedReferences([symbol.id], undefined, undefined, limit, metadataFilters);

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

    while (callerQueue.length > 0) {
      const current = callerQueue.shift()!;
      if (current.depth > maxDepth || seenCallerSymbols.has(current.symbolId)) continue;
      seenCallerSymbols.add(current.symbolId);
      bumpSymbol(current.symbolId);
      if (current.depth === maxDepth) continue;
      const nextCallers = buildUniqueCallReferences(store.getCallers(current.symbolId), "callerId", limit, metadataFilters).results;
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
      const nextCallees = buildUniqueCallReferences(store.getCallees(current.symbolId), "calleeId", limit, metadataFilters).results;
      for (const next of nextCallees) {
        calleeQueue.push({ symbolId: next.symbolId, depth: current.depth + 1 });
      }
    }

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
      ...metadataFilterEcho(metadataFilters),
      affectedSubsystems: buildMetadataGroupSummary(affectedSymbolIds, (affected) => affected.subsystem),
      affectedModules: buildMetadataGroupSummary(affectedSymbolIds, (affected) => affected.module),
      affectedLanguages: buildMetadataGroupSummary(affectedSymbolIds, (affected) => affected.language),
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

  function buildHierarchyPayload(
    symbol: NonNullable<ReturnType<Store["getSymbolById"]>>,
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
    source: NonNullable<ReturnType<Store["getSymbolById"]>>,
    target: NonNullable<ReturnType<Store["getSymbolById"]>>,
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
      language: z.enum(["cpp", "lua", "python", "typescript", "rust"]).optional().describe("Optional language hint for ranking ambiguous candidates"),
      subsystem: z.string().optional().describe("Optional subsystem hint for ranking ambiguous candidates"),
      module: z.string().optional().describe("Optional module hint for ranking ambiguous candidates"),
      projectArea: z.string().optional().describe("Optional project-area hint for ranking ambiguous candidates"),
      artifactKind: z.enum(["runtime", "editor", "tool", "test", "generated"]).optional().describe("Optional artifact-kind hint for ranking ambiguous candidates"),
      filePath: z.string().optional().describe("Optional workspace-relative path hint for ranking ambiguous candidates"),
      anchorQualifiedName: z.string().optional().describe("Optional exact anchor symbol whose metadata should seed ranking context"),
      recentQualifiedName: z.string().optional().describe("Optional recently inspected symbol used to derive anchor context when no exact anchor is provided"),
    },
    async ({ name, language, subsystem, module, projectArea, artifactKind, filePath, anchorQualifiedName, recentQualifiedName }) => {
      const context = buildHeuristicLookupContext({ language, subsystem, module, projectArea, artifactKind, filePath, anchorQualifiedName, recentQualifiedName });
      const { symbol: sym, candidateCount, rankedCandidates } = resolveFunctionSymbol(name, context);

      if (!sym) {
        return notFoundPayload();
      }

      const callers = buildCallReferences(store.getCallers(sym.id), "callerId");
      const callees = buildCallReferences(store.getCallees(sym.id), "calleeId");
      const payload = buildFunctionResponse({
        symbol: sym,
        candidateCount,
        rankedCandidates,
        callers,
        callees,
      });
      Object.assign(payload, buildResponseReliability({ symbol: sym }));

      return {
        content: [{
          type: "text",
          text: JSON.stringify(payload, null, 2),
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
      language: z.enum(["cpp", "lua", "python", "typescript", "rust"]).optional().describe("Optional language filter applied to caller symbols"),
      subsystem: z.string().optional().describe("Optional subsystem filter applied to caller symbols"),
      module: z.string().optional().describe("Optional module filter applied to caller symbols"),
      projectArea: z.string().optional().describe("Optional project-area filter applied to caller symbols"),
      artifactKind: z.enum(["runtime", "editor", "tool", "test", "generated"]).optional().describe("Optional artifact-kind filter applied to caller symbols"),
      anchorQualifiedName: z.string().optional().describe("Optional exact anchor symbol whose metadata should seed ranking context"),
      recentQualifiedName: z.string().optional().describe("Optional recently inspected symbol used to derive anchor context when no exact anchor is provided"),
    },
    async ({ name, limit, language, subsystem, module, projectArea, artifactKind, anchorQualifiedName, recentQualifiedName }) => {
      const metadataFilters: MetadataFilters | undefined = language || subsystem || module || projectArea || artifactKind
        ? { language, subsystem, module, projectArea, artifactKind }
        : undefined;
      const context = buildCallerLookupContext({ anchorQualifiedName, recentQualifiedName });
      const { symbol: sym, candidateCount, rankedCandidates } = resolveFunctionSymbol(name, context);

      if (!sym) {
        return notFoundPayload();
      }

      const callers = buildUniqueCallReferences(store.getCallers(sym.id), "callerId", limit, metadataFilters);
      const payload: CallerQueryResponse = buildCallerQueryResponse({
        symbol: sym,
        candidateCount,
        rankedCandidates,
        callers: callers.results,
        totalCount: callers.totalCount,
        truncated: callers.truncated,
        limitApplied: limit,
      });
      Object.assign(payload, buildResponseReliability({
        symbol: sym,
        relatedResultCount: callers.totalCount,
        zeroResultLabel: "callers",
      }));
      Object.assign(payload, metadataFilterEcho(metadataFilters), {
        groupedBySubsystem: buildMetadataGroupSummary(callers.results.map((caller) => caller.symbolId), (caller) => caller.subsystem),
        groupedByModule: buildMetadataGroupSummary(callers.results.map((caller) => caller.symbolId), (caller) => caller.module),
        groupedByLanguage: buildMetadataGroupSummary(callers.results.map((caller) => caller.symbolId), (caller) => caller.language),
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
      language: z.enum(["cpp", "lua", "python", "typescript", "rust"]).optional().describe("Optional language hint for ranking ambiguous candidates"),
      subsystem: z.string().optional().describe("Optional subsystem hint for ranking ambiguous candidates"),
      module: z.string().optional().describe("Optional module hint for ranking ambiguous candidates"),
      projectArea: z.string().optional().describe("Optional project-area hint for ranking ambiguous candidates"),
      artifactKind: z.enum(["runtime", "editor", "tool", "test", "generated"]).optional().describe("Optional artifact-kind hint for ranking ambiguous candidates"),
      filePath: z.string().optional().describe("Optional workspace-relative path hint for ranking ambiguous candidates"),
      anchorQualifiedName: z.string().optional().describe("Optional exact anchor symbol whose metadata should seed ranking context"),
      recentQualifiedName: z.string().optional().describe("Optional recently inspected symbol used to derive anchor context when no exact anchor is provided"),
    },
    async ({ name, language, subsystem, module, projectArea, artifactKind, filePath, anchorQualifiedName, recentQualifiedName }) => {
      const context = buildHeuristicLookupContext({ language, subsystem, module, projectArea, artifactKind, filePath, anchorQualifiedName, recentQualifiedName });
      const rankedCandidates = rankHeuristicCandidatesDetailed(
        store.getSymbolsByName(name).filter((s) => s.type === "class" || s.type === "struct"),
        context,
      );
      const sym = rankedCandidates[0]?.symbol;

      if (!sym) {
        return notFoundPayload();
      }

      const members = store.getMembers(sym.id);
      return {
        content: [{
          type: "text",
          text: JSON.stringify(buildClassResponse({
            symbol: sym,
            candidateCount: rankedCandidates.length,
            rankedCandidates,
            members,
          }), null, 2),
        }],
      };
    },
  );

  server.tool(
    "find_all_overloads",
    "Return all exact same-name function and method matches grouped by qualified name, without heuristic ranking collapse.",
    {
      name: z.string().describe("Function or method short name to inspect exactly"),
    },
    async ({ name }) => ({
      content: [{
        type: "text",
        text: JSON.stringify(
          buildOverloadQueryResponse(
            name,
            store.getSymbolsByName(name).filter((symbol) => symbol.type === "function" || symbol.type === "method"),
          ),
          null,
          2,
        ),
      }],
    }),
  );

  server.tool(
    "find_references",
    "Find generalized references for one exact target symbol. Accepts id and/or qualifiedName and supports optional category and filePath filters. Use compact=true for lighter navigation payloads.",
    {
      id: z.string().optional().describe("Canonical exact target symbol identity"),
      qualifiedName: z.string().optional().describe("Canonical exact target symbol qualified name"),
      category: z.enum(["functionCall", "methodCall", "classInstantiation", "moduleImport", "typeUsage", "inheritanceMention", "enumValueUsage"]).optional().describe("Optional reference category filter"),
      filePath: z.string().optional().describe("Optional exact file path filter"),
      includeEnumValueUsage: z.boolean().optional().describe("When the exact target is an enum, also include aggregated enum-member value usage references"),
      compact: z.boolean().optional().describe("When true, return only source/target identity fields needed for navigation"),
      limit: z.number().int().min(1).max(SEARCH_MAX_LIMIT).default(SEARCH_DEFAULT_LIMIT).describe("Maximum references to return"),
      language: z.enum(["cpp", "lua", "python", "typescript", "rust"]).optional().describe("Optional language filter applied to source symbols"),
      subsystem: z.string().optional().describe("Optional subsystem filter applied to source symbols"),
      module: z.string().optional().describe("Optional module filter applied to source symbols"),
      projectArea: z.string().optional().describe("Optional project-area filter applied to source symbols"),
      artifactKind: z.enum(["runtime", "editor", "tool", "test", "generated"]).optional().describe("Optional artifact-kind filter applied to source symbols"),
    },
    async ({ id, qualifiedName, category, filePath, includeEnumValueUsage, compact, limit, language, subsystem, module, projectArea, artifactKind }) => {
      const metadataFilters: MetadataFilters | undefined = language || subsystem || module || projectArea || artifactKind
        ? { language, subsystem, module, projectArea, artifactKind }
        : undefined;
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

      const references = buildResolvedReferences(
        buildReferenceTargetIds(symbol, category, includeEnumValueUsage),
        category,
        filePath,
        limit,
        metadataFilters,
      );
      const payload: ReferenceQueryResponse = {
        ...buildExactLookupResponse({
          symbol,
          matchedBy: id && qualifiedName ? "both" : id ? "id" : "qualifiedName",
        }),
        ...buildResponseReliability({
          symbol,
          relatedResultCount: references.totalCount,
          zeroResultLabel: "references",
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
        groupedByLanguage: buildMetadataGroupSummary(references.results.map((reference) => reference.sourceSymbolId), (source) => source.language),
      };
      const response: ReferenceQueryResponse | CompactReferenceQueryResponse = compact
        ? toCompactReferenceQueryResponse(payload, buildCompactResolvedReferences(references.results))
        : payload;

      return {
        content: [{ type: "text", text: JSON.stringify(response, null, 2) }],
      };
    },
  );

  server.tool(
    "explain_symbol_propagation",
    "Summarize incoming and outgoing bounded propagation events for one exact symbol identity.",
    {
      id: z.string().optional().describe("Canonical exact target symbol identity"),
      qualifiedName: z.string().optional().describe("Canonical exact target symbol qualified name"),
      filePath: z.string().optional().describe("Optional exact file path filter"),
      limit: z.number().int().min(1).max(SEARCH_MAX_LIMIT).default(SEARCH_DEFAULT_LIMIT).describe("Maximum propagation events to return per section"),
      propagationKinds: z.array(z.enum(["assignment", "initializerBinding", "argumentToParameter", "returnValue", "fieldWrite", "fieldRead"])).optional().describe("Optional propagation-kind filters"),
    },
    async ({ id, qualifiedName, filePath, limit, propagationKinds }) => {
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
        content: [{
          type: "text",
          text: JSON.stringify(
            buildExplainPropagationPayload(
              symbol,
              id && qualifiedName ? "both" : id ? "id" : "qualifiedName",
              limit,
              propagationKinds,
              filePath,
            ),
            null,
            2,
          ),
        }],
      };
    },
  );

  server.tool(
    "trace_variable_flow",
    "Trace one bounded propagation path for an exact symbol identity across supported local, field, and function-boundary events.",
    {
      id: z.string().optional().describe("Canonical exact target symbol identity"),
      qualifiedName: z.string().optional().describe("Canonical exact target symbol qualified name"),
      filePath: z.string().optional().describe("Optional exact file path filter"),
      maxDepth: z.number().int().min(1).max(CALLGRAPH_MAX_DEPTH).default(3).describe("Maximum propagation hops to follow"),
      maxEdges: z.number().int().min(1).max(SEARCH_MAX_LIMIT).default(SEARCH_DEFAULT_LIMIT).describe("Maximum propagation edges to explore"),
      propagationKinds: z.array(z.enum(["assignment", "initializerBinding", "argumentToParameter", "returnValue", "fieldWrite", "fieldRead"])).optional().describe("Optional propagation-kind filters"),
    },
    async ({ id, qualifiedName, filePath, maxDepth, maxEdges, propagationKinds }) => {
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
        content: [{
          type: "text",
          text: JSON.stringify(
            buildTraceVariableFlowPayload(
              symbol,
              id && qualifiedName ? "both" : id ? "id" : "qualifiedName",
              maxDepth,
              maxEdges,
              propagationKinds,
              filePath,
            ),
            null,
            2,
          ),
        }],
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
      language: z.enum(["cpp", "lua", "python", "typescript", "rust"]).optional().describe("Optional language filter applied to impacted symbols"),
      subsystem: z.string().optional().describe("Optional subsystem filter applied to impacted symbols"),
      module: z.string().optional().describe("Optional module filter applied to impacted symbols"),
      projectArea: z.string().optional().describe("Optional project-area filter applied to impacted symbols"),
      artifactKind: z.enum(["runtime", "editor", "tool", "test", "generated"]).optional().describe("Optional artifact-kind filter applied to impacted symbols"),
    },
    async ({ id, qualifiedName, depth, limit, language, subsystem, module, projectArea, artifactKind }) => {
      const metadataFilters: MetadataFilters | undefined = language || subsystem || module || projectArea || artifactKind
        ? { language, subsystem, module, projectArea, artifactKind }
        : undefined;
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
        content: [{ type: "text", text: JSON.stringify(buildImpactAnalysis(symbol, depth, limit, metadataFilters), null, 2) }],
      };
    },
  );

  server.tool(
    "list_file_symbols",
    "List symbols declared in one exact file path in stable line order. Use compact=true when you only need symbol identity and line ranges for navigation.",
    {
      filePath: z.string().describe("Exact workspace-relative file path"),
      limit: z.number().int().min(1).max(SEARCH_MAX_LIMIT).default(SEARCH_DEFAULT_LIMIT).describe("Maximum symbols to return"),
      compact: z.boolean().optional().describe("When true, return only compact symbol navigation fields"),
    },
    async ({ filePath, limit, compact }) => {
      const allSymbols = store.getFileSymbols(filePath);
      const symbols = applyLimit(allSymbols, limit);
      const payload: FileSymbolsResponse = {
        filePath,
        summary: buildStructureOverviewSummary(allSymbols),
        window: buildResultWindow(symbols.results.length, symbols.totalCount, symbols.truncated, limit),
        symbols: symbols.results,
      };
      const response: FileSymbolsResponse | CompactFileSymbolsResponse = compact
        ? toCompactFileSymbolsResponse(payload)
        : payload;

      return {
        content: [{ type: "text", text: JSON.stringify(response, null, 2) }],
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
    "get_type_hierarchy",
    "Get direct base and direct derived type relationships for one exact class or struct qualified name.",
    {
      qualifiedName: z.string().describe("Exact class or struct qualified name"),
      limit: z.number().int().min(1).max(SEARCH_MAX_LIMIT).default(SEARCH_DEFAULT_LIMIT).describe("Maximum hierarchy nodes to return"),
    },
    async ({ qualifiedName, limit }) => {
      const symbol = store.getSymbolByQualifiedName(qualifiedName);
      if (!symbol || (symbol.type !== "class" && symbol.type !== "struct")) {
        return notFoundPayload();
      }

      return {
        content: [{ type: "text", text: JSON.stringify(buildHierarchyPayload(symbol, limit), null, 2) }],
      };
    },
  );

  server.tool(
    "find_base_methods",
    "Find likely base methods for one exact method qualified name using hierarchy and structural override evidence.",
    {
      qualifiedName: z.string().describe("Exact method qualified name"),
      limit: z.number().int().min(1).max(SEARCH_MAX_LIMIT).default(SEARCH_DEFAULT_LIMIT).describe("Maximum base method candidates to return"),
    },
    async ({ qualifiedName, limit }) => {
      const symbol = store.getSymbolByQualifiedName(qualifiedName);
      if (!symbol || symbol.type !== "method") {
        return notFoundPayload();
      }

      const baseMethods = applyLimit(store.getBaseMethods(symbol.id), limit);
      const payload: BaseMethodsResponse = {
        ...buildExactLookupResponse({ symbol, matchedBy: "qualifiedName" }),
        window: buildResultWindow(baseMethods.results.length, baseMethods.totalCount, baseMethods.truncated, limit),
        baseMethods: baseMethods.results,
      };

      return {
        content: [{ type: "text", text: JSON.stringify(payload, null, 2) }],
      };
    },
  );

  server.tool(
    "find_overrides",
    "Find likely overriding methods for one exact base method qualified name using hierarchy and structural override evidence.",
    {
      qualifiedName: z.string().describe("Exact base method qualified name"),
      limit: z.number().int().min(1).max(SEARCH_MAX_LIMIT).default(SEARCH_DEFAULT_LIMIT).describe("Maximum override candidates to return"),
    },
    async ({ qualifiedName, limit }) => {
      const symbol = store.getSymbolByQualifiedName(qualifiedName);
      if (!symbol || symbol.type !== "method") {
        return notFoundPayload();
      }

      const overrides = applyLimit(store.getOverrides(symbol.id), limit);
      const payload: OverrideQueryResponse = {
        ...buildExactLookupResponse({ symbol, matchedBy: "qualifiedName" }),
        window: buildResultWindow(overrides.results.length, overrides.totalCount, overrides.truncated, limit),
        overrides: overrides.results,
      };

      return {
        content: [{ type: "text", text: JSON.stringify(payload, null, 2) }],
      };
    },
  );

  server.tool(
    "trace_call_path",
    "Find one bounded shortest call path between two exact callable symbols.",
    {
      sourceQualifiedName: z.string().describe("Exact source function or method qualified name"),
      targetQualifiedName: z.string().describe("Exact target function or method qualified name"),
      maxDepth: z.number().int().min(1).max(CALLGRAPH_MAX_DEPTH).default(CALLGRAPH_DEFAULT_DEPTH).describe("Maximum call-path depth to explore"),
    },
    async ({ sourceQualifiedName, targetQualifiedName, maxDepth }) => {
      const source = store.getSymbolByQualifiedName(sourceQualifiedName);
      const target = store.getSymbolByQualifiedName(targetQualifiedName);
      if (!source || !target) {
        return notFoundPayload();
      }
      if ((source.type !== "function" && source.type !== "method") || (target.type !== "function" && target.type !== "method")) {
        return notFoundPayload();
      }

      return {
        content: [{ type: "text", text: JSON.stringify(traceShortestCallPath(source, target, maxDepth), null, 2) }],
      };
    },
  );

  server.tool(
    "investigate_workflow",
    "Compose a bounded investigation-oriented workflow answer from exact source and optional target symbols.",
    {
      sourceQualifiedName: z.string().describe("Exact source function, method, or field qualified name"),
      targetQualifiedName: z.string().optional().describe("Optional exact target function, method, or field qualified name"),
      maxDepth: z.number().int().min(1).max(CALLGRAPH_MAX_DEPTH).default(CALLGRAPH_DEFAULT_DEPTH).describe("Maximum workflow traversal depth"),
      maxEdges: z.number().int().min(1).max(SEARCH_MAX_LIMIT).default(SEARCH_DEFAULT_LIMIT).describe("Maximum workflow edges to explore"),
    },
    async ({ sourceQualifiedName, targetQualifiedName, maxDepth, maxEdges }) => {
      const source = store.getSymbolByQualifiedName(sourceQualifiedName);
      const target = targetQualifiedName ? store.getSymbolByQualifiedName(targetQualifiedName) : undefined;
      if (!source || (targetQualifiedName && !target)) {
        return notFoundPayload();
      }

      return {
        content: [{
          type: "text",
          text: JSON.stringify(buildInvestigateWorkflowPayload(source, target, maxDepth, maxEdges), null, 2),
        }],
      };
    },
  );

  server.tool(
    "search_symbols",
    "Search symbols by name substring. Returns matching symbols with truncation indicator. Minimum query length is 3 characters.",
    {
      query: z.string().describe(`Search query (minimum ${SEARCH_MIN_QUERY_LENGTH} characters; shorter queries return an empty result set)`),
      type: z.enum(["function", "method", "class", "struct", "enum", "enumMember", "namespace", "variable", "typedef"]).optional().describe("Filter by symbol type"),
      limit: z.number().int().min(1).max(SEARCH_MAX_LIMIT).default(SEARCH_DEFAULT_LIMIT).describe("Maximum results to return"),
      language: z.enum(["cpp", "lua", "python", "typescript", "rust"]).optional().describe("Optional language filter applied to result symbols"),
      subsystem: z.string().optional().describe("Optional subsystem filter applied to result symbols"),
      module: z.string().optional().describe("Optional module filter applied to result symbols"),
      projectArea: z.string().optional().describe("Optional project-area filter applied to result symbols"),
      artifactKind: z.enum(["runtime", "editor", "tool", "test", "generated"]).optional().describe("Optional artifact-kind filter applied to result symbols"),
    },
    async ({ query, type, limit, language, subsystem, module, projectArea, artifactKind }) => {
      const metadataFilters: MetadataFilters | undefined = language || subsystem || module || projectArea || artifactKind
        ? { language, subsystem, module, projectArea, artifactKind }
        : undefined;
      const { results, totalCount } = store.searchSymbols(query, type, limit, metadataFilters);
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
            language: metadataFilters?.language,
            ...metadataFilterEcho(metadataFilters),
            groupedByLanguage: buildMetadataGroupSummary(results.map((result) => result.id), (symbol) => symbol.language),
          }, null, 2),
        }],
      };
    },
  );

  server.tool(
    "workspace_summary",
    "Summarize the mixed-language workspace with per-language file and symbol counts.",
    {},
    async () => ({
      content: [{ type: "text", text: JSON.stringify(buildWorkspaceSummary(), null, 2) }],
    }),
  );

  function expandCallDirection(
    symbolId: string,
    currentDepth: number,
    maxDepth: number,
    visited: Set<string>,
    state: { remainingNodeBudget: number },
    direction: "callers" | "callees",
  ): { edges: CallGraphEdge[]; truncated: boolean } {
    const calls = direction === "callers" ? store.getCallers(symbolId) : store.getCallees(symbolId);
    if (currentDepth >= maxDepth || visited.has(symbolId)) {
      return { edges: [], truncated: calls.length > 0 };
    }
    visited.add(symbolId);

    let anyTruncated = false;
    const edges: CallGraphEdge[] = [];
    for (const call of calls) {
      const targetId = direction === "callers" ? call.callerId : call.calleeId;
      const target = targetId ? store.getSymbolById(targetId) : undefined;
      if (!target) {
        continue;
      }
      if (visited.has(target.id)) {
        anyTruncated = true;
        continue;
      }
      if (state.remainingNodeBudget <= 0) {
        anyTruncated = true;
        break;
      }

      state.remainingNodeBudget -= 1;
      const { edges: children, truncated } = expandCallDirection(
        target.id,
        currentDepth + 1,
        maxDepth,
        new Set(visited),
        state,
        direction,
      );
      if (truncated) anyTruncated = true;
      edges.push({
        targetId: target.id,
        targetName: target.name,
        targetQualifiedName: target.qualifiedName,
        filePath: call.filePath,
        line: call.line,
        ...(children.length > 0 ? { children } : {}),
      });
    }

    return { edges, truncated: anyTruncated };
  }

  function buildCallGraphPayload(
    symbol: NonNullable<ReturnType<Store["getSymbolById"]>>,
    maxDepth: number,
    direction: CallGraphDirection,
    nodeCap: number,
  ) {
    const state = { remainingNodeBudget: Math.max(0, nodeCap - 1) };
    const root = {
      symbol: { id: symbol.id, name: symbol.name, qualifiedName: symbol.qualifiedName, type: symbol.type, filePath: symbol.filePath, line: symbol.line },
      callees: [] as CallGraphEdge[],
      callers: undefined as CallGraphEdge[] | undefined,
    };
    let truncated = false;

    if (direction === "callees" || direction === "both") {
      const result = expandCallDirection(symbol.id, 0, maxDepth, new Set<string>(), state, "callees");
      root.callees = result.edges;
      truncated = truncated || result.truncated;
    }
    if (direction === "callers" || direction === "both") {
      const result = expandCallDirection(symbol.id, 0, maxDepth, new Set<string>(), state, "callers");
      root.callers = result.edges;
      truncated = truncated || result.truncated;
    }

    const edgeCount = countGraphEdges(root.callees) + countGraphEdges(root.callers ?? []);

    return {
      ...buildResponseReliability({
        symbol,
        relatedResultCount: edgeCount,
        zeroResultLabel: direction === "callers"
          ? "caller edges"
          : direction === "both"
            ? "callgraph edges"
            : "callee edges",
      }),
      root,
      direction,
      depth: computeDepth(root),
      maxDepth,
      nodeCount: nodeCap - state.remainingNodeBudget,
      nodeCap,
      truncated,
    };
  }

  function computeDepth(root: { callees: CallGraphEdge[]; callers?: CallGraphEdge[] }): number {
    return Math.max(computeEdgeDepth(root.callees), computeEdgeDepth(root.callers ?? []));
  }

  function computeEdgeDepth(edges: CallGraphEdge[]): number {
    if (edges.length === 0) return 0;
    let max = 0;
    for (const e of edges) {
      const d = e.children ? computeEdgeDepth(e.children) : 0;
      if (d + 1 > max) max = d + 1;
    }
    return max;
  }

  function countGraphEdges(edges: CallGraphEdge[]): number {
    let count = 0;
    for (const edge of edges) {
      count += 1;
      if (edge.children) {
        count += countGraphEdges(edge.children);
      }
    }
    return count;
  }

  server.tool(
    "get_callgraph",
    "Get the call graph rooted at a function or method. Supports bounded callee, caller, or bidirectional expansion. Use compact=true for lighter navigation payloads.",
    {
      name: z.string().describe("Root function or method name"),
      depth: z.number().int().min(1).max(CALLGRAPH_MAX_DEPTH).default(CALLGRAPH_DEFAULT_DEPTH).describe("Maximum traversal depth"),
      direction: z.enum(["callees", "callers", "both"]).default("callees").describe("Traversal direction"),
      nodeCap: z.number().int().min(1).max(CALLGRAPH_MAX_NODE_CAP).default(CALLGRAPH_DEFAULT_NODE_CAP).describe("Maximum total nodes to expand before truncation"),
      compact: z.boolean().optional().describe("When true, reduce root node metadata to compact navigation fields"),
    },
    async ({ name, depth: maxDepth, direction, nodeCap, compact }) => {
      const symbols = store.getSymbolsByName(name);
      const sym = symbols.find((s) => s.type === "function" || s.type === "method");

      if (!sym) {
        return notFoundPayload();
      }

      const payload = buildCallGraphPayload(sym, maxDepth, direction, nodeCap);
      const response: ReturnType<typeof buildCallGraphPayload> | CompactCallGraphResponse = compact
        ? toCompactCallGraphResponse(payload)
        : payload;

      return {
        content: [{ type: "text", text: JSON.stringify(response, null, 2) }],
      };
    },
  );

  server.tool(
    "find_callers_recursive",
    "Expand callers recursively for a function or method using the same bounded traversal as caller-direction callgraph queries.",
    {
      name: z.string().describe("Root function or method name"),
      depth: z.number().int().min(1).max(CALLGRAPH_MAX_DEPTH).default(CALLGRAPH_DEFAULT_DEPTH).describe("Maximum traversal depth"),
      nodeCap: z.number().int().min(1).max(CALLGRAPH_MAX_NODE_CAP).default(CALLGRAPH_DEFAULT_NODE_CAP).describe("Maximum total nodes to expand before truncation"),
      anchorQualifiedName: z.string().optional().describe("Optional exact anchor symbol whose metadata should seed ranking context"),
      recentQualifiedName: z.string().optional().describe("Optional recently inspected symbol used to derive anchor context when no exact anchor is provided"),
    },
    async ({ name, depth: maxDepth, nodeCap, anchorQualifiedName, recentQualifiedName }) => {
      const context = buildCallerLookupContext({ anchorQualifiedName, recentQualifiedName });
      const { symbol: sym } = resolveFunctionSymbol(name, context);

      if (!sym) {
        return notFoundPayload();
      }

      return {
        content: [{ type: "text", text: JSON.stringify(buildCallGraphPayload(sym, maxDepth, "callers", nodeCap), null, 2) }],
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
