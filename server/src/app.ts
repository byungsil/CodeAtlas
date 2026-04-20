import * as path from "path";
import express from "express";
import { SourceLanguage, Symbol as CodeSymbol } from "./models/symbol";
import { Store } from "./storage/store";
import { MetadataFilters } from "./storage/store";
import {
  SEARCH_DEFAULT_LIMIT, SEARCH_MAX_LIMIT,
  CALLERS_DEFAULT_LIMIT, CALLERS_MAX_LIMIT,
  CALLGRAPH_DEFAULT_DEPTH, CALLGRAPH_MAX_DEPTH,
  CALLGRAPH_DEFAULT_NODE_CAP, CALLGRAPH_MAX_NODE_CAP,
} from "./constants";
import {
  FunctionResponse,
  CallerQueryResponse,
  ClassResponse,
  SearchResponse,
  CallGraphDirection,
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
  CompactCallGraphResponse,
  CompactFileSymbolsResponse,
  CompactReferenceRecord,
  CompactReferenceQueryResponse,
  PropagationEventRecord,
  PropagationKind,
  PropagationPathStep,
  InvestigationWorkflowStep,
  InvestigateWorkflowResponse,
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
import { getMcpRuntimeStatsSnapshot, readPersistedMcpRuntimeStatsSnapshot } from "./runtime-stats";
import {
  buildInvestigateWorkflowResponse,
  createWorkflowProfiler,
  createWorkflowQueryContext,
  buildPropagationWorkflowSteps as sharedBuildPropagationWorkflowSteps,
  toInvestigationAnchorSummary as sharedToInvestigationAnchorSummary,
} from "./investigation-workflow";

interface DashboardWorkspaceSource {
  id: string;
  label: string;
  dataDir: string;
  statsPath?: string;
  store: Store;
  isPrimary?: boolean;
}

interface AppOptions {
  dashboardWorkspaces?: DashboardWorkspaceSource[];
}

export function createApp(store: Store, options?: AppOptions): express.Express {
  const app = express();
  const dashboardWorkspace = (options?.dashboardWorkspaces ?? [{
    id: "default",
    label: "default",
    dataDir: "",
    store,
    isPrimary: true,
  }]).find((workspace) => workspace.isPrimary)?.store ?? store;
  const dashboardStatsPath = (options?.dashboardWorkspaces ?? [])
    .find((workspace) => workspace.isPrimary)?.statsPath;

  app.use("/dashboard", express.static(path.join(__dirname, "../public"), { index: "index.html" }));
  app.get("/dashboard", (_req, res) => res.redirect("/dashboard/"));

  function notFound(res: express.Response) {
    return res.status(404).json({ error: "Symbol not found", code: "NOT_FOUND" } as ErrorResponse);
  }

  function badRequest(res: express.Response) {
    return res.status(400).json({ error: "Invalid exact lookup request", code: "BAD_REQUEST" } as ErrorResponse);
  }

  function buildSymbolMapForStore(activeStore: Store, ids: Iterable<string>): Map<string, CodeSymbol> {
    const uniqueIds = Array.from(new Set(Array.from(ids)));
    return new Map(activeStore.getSymbolsByIds(uniqueIds).map((symbol) => [symbol.id, symbol]));
  }

  function buildSymbolMap(ids: Iterable<string>): Map<string, CodeSymbol> {
    return buildSymbolMapForStore(store, ids);
  }

  function makeCallRefForStore(
    activeStore: Store,
    call: { callerId?: string; calleeId?: string; filePath: string; line: number },
    targetField: "callerId" | "calleeId",
    symbolMap?: Map<string, CodeSymbol>,
  ): CallReference | null {
    const targetId = call[targetField];
    if (!targetId) return null;
    const sym = symbolMap?.get(targetId) ?? activeStore.getSymbolById(targetId);
    if (!sym) return null;
    return makeResolvedCallReference({
      symbol: sym,
      filePath: call.filePath,
      line: call.line,
    });
  }

  function makeCallRef(
    call: { callerId?: string; calleeId?: string; filePath: string; line: number },
    targetField: "callerId" | "calleeId",
    symbolMap?: Map<string, CodeSymbol>,
  ): CallReference | null {
    return makeCallRefForStore(store, call, targetField, symbolMap);
  }

  function parseMetadataFilters(source: Record<string, unknown>): MetadataFilters | undefined {
    const language = typeof source.language === "string" ? source.language as SourceLanguage : undefined;
    const subsystem = typeof source.subsystem === "string" ? source.subsystem : undefined;
    const module = typeof source.module === "string" ? source.module : undefined;
    const projectArea = typeof source.projectArea === "string" ? source.projectArea : undefined;
    const artifactKind = typeof source.artifactKind === "string"
      ? source.artifactKind as MetadataFilters["artifactKind"]
      : undefined;

    if (!language && !subsystem && !module && !projectArea && !artifactKind) {
      return undefined;
    }

    return { language, subsystem, module, projectArea, artifactKind };
  }

  function parseCompactMode(value: unknown): boolean {
    return value === "1" || value === "true" || value === true;
  }

  function parseHeuristicLookupContext(source: Record<string, unknown>): HeuristicLookupContext | undefined {
    const filePath = typeof source.filePath === "string" ? source.filePath : undefined;
    const anchorQualifiedName = typeof source.anchorQualifiedName === "string" ? source.anchorQualifiedName : undefined;
    const recentQualifiedName = typeof source.recentQualifiedName === "string" ? source.recentQualifiedName : undefined;
    const metadata = parseMetadataFilters(source);
    const anchorSymbol = resolveHeuristicAnchorSymbol({ anchorQualifiedName, recentQualifiedName });
    if (!filePath && !metadata && !anchorSymbol) {
      return undefined;
    }
    return {
      language: metadata?.language ?? anchorSymbol?.language,
      subsystem: metadata?.subsystem ?? anchorSymbol?.subsystem,
      module: metadata?.module ?? anchorSymbol?.module,
      projectArea: metadata?.projectArea ?? anchorSymbol?.projectArea,
      artifactKind: metadata?.artifactKind ?? anchorSymbol?.artifactKind,
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

  function parseCallerLookupContext(source: Record<string, unknown>): HeuristicLookupContext | undefined {
    return parseHeuristicLookupContext({
      anchorQualifiedName: source.anchorQualifiedName,
      recentQualifiedName: source.recentQualifiedName,
    });
  }

  function resolveHeuristicAnchorSymbol(params: {
    anchorQualifiedName?: string;
    recentQualifiedName?: string;
  }): CodeSymbol | undefined {
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

  function metadataFilterEcho(filters?: MetadataFilters): Partial<MetadataFilters> {
    return filters ?? {};
  }

  function matchesMetadataFilters(symbol: CodeSymbol | undefined, filters?: MetadataFilters): boolean {
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

  function buildWorkspaceSummary(activeStore: Store = store): WorkspaceSummaryResponse {
    const languages = activeStore.getWorkspaceLanguageSummary();
    return {
      languages,
      totalFiles: languages.reduce((sum, entry) => sum + entry.fileCount, 0),
      totalSymbols: languages.reduce((sum, entry) => sum + entry.symbolCount, 0),
    };
  }

  function buildDashboardOverview() {
    const activeStore = dashboardWorkspace;
    const workspaceSummary = buildWorkspaceSummary(activeStore);
    const indexDetails = typeof activeStore.getIndexDetails === "function"
      ? activeStore.getIndexDetails()
      : {
        backend: "json" as const,
        dataPath: "",
        counts: {
          symbols: workspaceSummary.totalSymbols,
          calls: 0,
          references: 0,
          propagation: 0,
          files: workspaceSummary.totalFiles,
        },
        fileRiskCounts: {
          elevatedParseFragility: 0,
          macroSensitive: 0,
          includeHeavy: 0,
        },
      };
    return {
      generatedAt: new Date().toISOString(),
      workspace: workspaceSummary,
      index: indexDetails,
      mcp: dashboardStatsPath
        ? readPersistedMcpRuntimeStatsSnapshot(dashboardStatsPath)
        : getMcpRuntimeStatsSnapshot(),
    };
  }

  function buildCallRefsForStore(
    activeStore: Store,
    calls: { callerId?: string; calleeId?: string; filePath: string; line: number }[],
    targetField: "callerId" | "calleeId",
  ): CallReference[] {
    const symbolMap = buildSymbolMapForStore(
      activeStore,
      calls.map((call) => call[targetField]).filter((symbolId): symbolId is string => Boolean(symbolId)),
    );
    return calls
      .map((call) => makeCallRefForStore(activeStore, call, targetField, symbolMap))
      .filter((call): call is CallReference => call !== null);
  }

  function buildDashboardFunctionPayload(activeStore: Store, name: string): FunctionResponse | null {
    const candidates = activeStore.getSymbolsByName(name).filter((symbol) => symbol.type === "function" || symbol.type === "method");
    const symbol = candidates[0];
    if (!symbol) {
      return null;
    }
    const payload = buildFunctionResponse({
      symbol,
      candidateCount: candidates.length,
      callers: buildCallRefsForStore(activeStore, activeStore.getCallers(symbol.id), "callerId"),
      callees: buildCallRefsForStore(activeStore, activeStore.getCallees(symbol.id), "calleeId"),
    });
    Object.assign(payload, buildResponseReliability({ symbol }));
    return payload;
  }

  function buildDashboardClassPayload(activeStore: Store, name: string): ClassResponse | null {
    const symbol = activeStore.getSymbolsByName(name).find((candidate) => candidate.type === "class" || candidate.type === "struct");
    if (!symbol) {
      return null;
    }
    return buildClassResponse({
      symbol,
      candidateCount: 1,
      members: activeStore.getMembers(symbol.id),
    });
  }

  function buildDashboardCallGraphPayload(activeStore: Store, name: string, maxDepth: number) {
    const symbol = activeStore.getSymbolsByName(name).find((candidate) => candidate.type === "function" || candidate.type === "method");
    if (!symbol) {
      return null;
    }

    function expand(symbolId: string, depth: number, visited: Set<string>): CallGraphEdge[] {
      if (depth >= maxDepth || visited.has(symbolId)) {
        return [];
      }
      visited.add(symbolId);
      return activeStore.getCallees(symbolId)
        .map((call) => {
          const target = activeStore.getSymbolById(call.calleeId);
          if (!target) return null;
          const children = expand(target.id, depth + 1, new Set(visited));
          return {
            targetId: target.id,
            targetName: target.name,
            targetQualifiedName: target.qualifiedName,
            filePath: call.filePath,
            line: call.line,
            ...(children.length > 0 ? { children } : {}),
          };
        })
        .filter((edge): edge is CallGraphEdge => edge !== null);
    }

    const root = {
      symbol: {
        id: symbol.id,
        name: symbol.name,
        qualifiedName: symbol.qualifiedName,
        type: symbol.type,
        filePath: symbol.filePath,
        line: symbol.line,
      },
      callees: expand(symbol.id, 0, new Set<string>()),
    };
    const edgeCount = countCallGraphEdges(root.callees);
    return {
      ...buildResponseReliability({ symbol, relatedResultCount: edgeCount, zeroResultLabel: "callee edges" }),
      root,
      direction: "callees" as const,
      depth: computeEdgeDepth(root.callees),
      maxDepth,
      nodeCount: edgeCount + 1,
      nodeCap: CALLGRAPH_DEFAULT_NODE_CAP,
      truncated: false,
    };
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
    symbol: CodeSymbol,
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

  function traceInvestigationWorkflow(
    source: CodeSymbol,
    target: CodeSymbol | undefined,
    maxDepth: number,
    maxEdges: number,
  ): InvestigateWorkflowResponse {
    const base = buildExactLookupResponse({ symbol: source, matchedBy: "qualifiedName" });
    const workflowQueryContext = createWorkflowQueryContext(store);
    const workflowProfiler = createWorkflowProfiler({
      channel: "http",
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
      const propagation = workflowProfiler.measure("trace_variable_flow", () => traceVariableFlow(source, "qualifiedName", maxDepth, maxEdges));
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
      affectedLanguages: buildMetadataGroupSummary(affectedSymbolIds, (affected) => affected.language),
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

  function buildExactSymbolResponse(params: { matchedBy: "id" | "qualifiedName" | "both"; symbol: ReturnType<Store["getSymbolById"]> }): SymbolLookupResponse | null {
    const { symbol, matchedBy } = params;
    if (!symbol) return null;

    const representativeMetadata = deriveRepresentativeMetadata(symbol, store.getRepresentativeCandidates(symbol.id));
    const base = buildExactLookupResponse({ symbol, matchedBy, representativeMetadata });

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

    if (symbol.type === "enum") {
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
    const context = parseHeuristicLookupContext(req.query as Record<string, unknown>);
    const { symbol: sym, candidateCount, rankedCandidates } = resolveFunctionSymbol(name, context);

    if (!sym) {
      return notFound(res);
    }

    const callers = buildCallRefs(store.getCallers(sym.id), "callerId");
    const callees = buildCallRefs(store.getCallees(sym.id), "calleeId");

    const response: FunctionResponse = buildFunctionResponse({
      symbol: sym,
      candidateCount,
      rankedCandidates,
      callers,
      callees,
    });
    Object.assign(response, buildResponseReliability({ symbol: sym }));
    return res.json(response);
  });

  app.get("/callers/:name", (req, res) => {
    const { name } = req.params;
    const limit = Math.min(parseInt((req.query.limit as string) || String(CALLERS_DEFAULT_LIMIT), 10), CALLERS_MAX_LIMIT);
    const metadataFilters = parseMetadataFilters(req.query as Record<string, unknown>);
    const context = parseCallerLookupContext(req.query as Record<string, unknown>);
    const { symbol: sym, candidateCount, rankedCandidates } = resolveFunctionSymbol(name, context);

    if (!sym) {
      return notFound(res);
    }

    const callers = buildUniqueCallRefs(store.getCallers(sym.id), "callerId", limit, metadataFilters);
    const response: CallerQueryResponse = buildCallerQueryResponse({
      symbol: sym,
      candidateCount,
      rankedCandidates,
      callers: callers.results,
      totalCount: callers.totalCount,
      truncated: callers.truncated,
      limitApplied: limit,
    });
    Object.assign(response, buildResponseReliability({
      symbol: sym,
      relatedResultCount: callers.totalCount,
      zeroResultLabel: "callers",
    }));
    const callerIds = callers.results.map((caller) => caller.symbolId);
    Object.assign(response, metadataFilterEcho(metadataFilters), {
      groupedBySubsystem: buildMetadataGroupSummary(callerIds, (caller) => caller.subsystem),
      groupedByModule: buildMetadataGroupSummary(callerIds, (caller) => caller.module),
      groupedByLanguage: buildMetadataGroupSummary(callerIds, (caller) => caller.language),
    });
    return res.json(response);
  });

  app.get("/class/:name", (req, res) => {
    const { name } = req.params;
    const context = parseHeuristicLookupContext(req.query as Record<string, unknown>);
    const rankedCandidates = rankHeuristicCandidatesDetailed(
      store.getSymbolsByName(name).filter((s) => s.type === "class" || s.type === "struct"),
      context,
    );
    const sym = rankedCandidates[0]?.symbol;

    if (!sym) {
      return notFound(res);
    }

    const members = store.getMembers(sym.id);
    const response: ClassResponse = buildClassResponse({
      symbol: sym,
      candidateCount: rankedCandidates.length,
      rankedCandidates,
      members,
    });
    return res.json(response);
  });

  app.get("/overloads/:name", (req, res) => {
    const { name } = req.params;
    const symbols = store.getSymbolsByName(name)
      .filter((symbol) => symbol.type === "function" || symbol.type === "method");
    return res.json(buildOverloadQueryResponse(name, symbols));
  });

  app.get("/references", (req, res) => {
    const id = typeof req.query.id === "string" ? req.query.id : undefined;
    const qualifiedName = typeof req.query.qualifiedName === "string" ? req.query.qualifiedName : undefined;
    const category = typeof req.query.category === "string" ? req.query.category as ReferenceCategory : undefined;
    const filePath = typeof req.query.filePath === "string" ? req.query.filePath : undefined;
    const includeEnumValueUsage =
      req.query.includeEnumValueUsage === "1"
      || req.query.includeEnumValueUsage === "true";
    const limit = Math.min(parseInt((req.query.limit as string) || String(SEARCH_DEFAULT_LIMIT), 10), SEARCH_MAX_LIMIT);
    const compact = parseCompactMode(req.query.compact);
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

    const references = buildResolvedReferences(
      buildReferenceTargetIds(symbol, category, includeEnumValueUsage),
      category,
      filePath,
      limit,
      metadataFilters,
    );
    const response: ReferenceQueryResponse = {
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
    if (compact) {
      const compactResponse: CompactReferenceQueryResponse = toCompactReferenceQueryResponse(
        response,
        buildCompactResolvedReferences(references.results),
      );
      return res.json(compactResponse);
    }
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

  app.get("/investigate-workflow", (req, res) => {
    const sourceQualifiedName = typeof req.query.sourceQualifiedName === "string" ? req.query.sourceQualifiedName : undefined;
    const targetQualifiedName = typeof req.query.targetQualifiedName === "string" ? req.query.targetQualifiedName : undefined;
    const maxDepth = Math.min(parseInt((req.query.maxDepth as string) || String(CALLGRAPH_DEFAULT_DEPTH), 10), CALLGRAPH_MAX_DEPTH);
    const maxEdges = Math.min(parseInt((req.query.maxEdges as string) || String(SEARCH_DEFAULT_LIMIT), 10), SEARCH_MAX_LIMIT);

    if (!sourceQualifiedName) {
      return badRequest(res);
    }

    const source = store.getSymbolByQualifiedName(sourceQualifiedName);
    const target = targetQualifiedName ? store.getSymbolByQualifiedName(targetQualifiedName) : undefined;
    if (!source || (targetQualifiedName && !target)) {
      return notFound(res);
    }

    return res.json(traceInvestigationWorkflow(source, target, maxDepth, maxEdges));
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

  app.get("/workspace-summary", (_req, res) => {
    return res.json(buildWorkspaceSummary());
  });

  app.get("/dashboard/api/overview", (req, res) => {
    return res.json(buildDashboardOverview());
  });

  app.get("/dashboard/api/search", (req, res) => {
    const q = (req.query.q as string) || "";
    const limit = Math.min(parseInt((req.query.limit as string) || String(SEARCH_DEFAULT_LIMIT), 10), SEARCH_MAX_LIMIT);
    const activeStore = dashboardWorkspace;
    if (!q) {
      return res.status(400).json({ error: "Missing query parameter 'q'", code: "BAD_REQUEST" } as ErrorResponse);
    }
    const { results, totalCount } = activeStore.searchSymbols(q, undefined, limit);
    return res.json({
      query: q,
      window: buildResultWindow(results.length, totalCount, totalCount > limit, limit),
      results,
      totalCount,
      truncated: totalCount > limit,
    });
  });

  app.get("/dashboard/api/function/:name", (req, res) => {
    const payload = buildDashboardFunctionPayload(dashboardWorkspace, req.params.name);
    if (!payload) {
      return notFound(res);
    }
    return res.json(payload);
  });

  app.get("/dashboard/api/class/:name", (req, res) => {
    const payload = buildDashboardClassPayload(dashboardWorkspace, req.params.name);
    if (!payload) {
      return notFound(res);
    }
    return res.json(payload);
  });

  app.get("/dashboard/api/callgraph/:name", (req, res) => {
    const maxDepth = Math.min(parseInt((req.query.depth as string) || String(CALLGRAPH_DEFAULT_DEPTH), 10), CALLGRAPH_MAX_DEPTH);
    const payload = buildDashboardCallGraphPayload(dashboardWorkspace, req.params.name, maxDepth);
    if (!payload) {
      return notFound(res);
    }
    return res.json(payload);
  });

  app.get("/file-symbols", (req, res) => {
    const filePath = typeof req.query.filePath === "string" ? req.query.filePath : undefined;
    const limit = Math.min(parseInt((req.query.limit as string) || String(SEARCH_DEFAULT_LIMIT), 10), SEARCH_MAX_LIMIT);
    const compact = parseCompactMode(req.query.compact);
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
    if (compact) {
      const compactResponse: CompactFileSymbolsResponse = toCompactFileSymbolsResponse(response);
      return res.json(compactResponse);
    }
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
      language: metadataFilters?.language,
      ...metadataFilterEcho(metadataFilters),
      groupedByLanguage: buildMetadataGroupSummary(results.map((result) => result.id), (symbol) => symbol.language),
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

  function buildCallGraphResponse(
    symbol: CodeSymbol,
    maxDepth: number,
    direction: CallGraphDirection,
    nodeCap: number,
  ): CallGraphResponse {
    const state = { remainingNodeBudget: Math.max(0, nodeCap - 1) };
    const root: CallGraphResponse["root"] = {
      symbol: {
        id: symbol.id,
        name: symbol.name,
        qualifiedName: symbol.qualifiedName,
        type: symbol.type,
        filePath: symbol.filePath,
        line: symbol.line,
      },
      callees: [],
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

    const edgeCount = countCallGraphEdges(root.callees) + countCallGraphEdges(root.callers ?? []);

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

  app.get("/callgraph/:name", (req, res) => {
    const { name } = req.params;
    const maxDepth = Math.min(parseInt((req.query.depth as string) || String(CALLGRAPH_DEFAULT_DEPTH), 10), CALLGRAPH_MAX_DEPTH);
    const direction = parseCallGraphDirection(req.query.direction);
    const nodeCap = Math.min(parseInt((req.query.nodeCap as string) || String(CALLGRAPH_DEFAULT_NODE_CAP), 10), CALLGRAPH_MAX_NODE_CAP);
    const compact = parseCompactMode(req.query.compact);

    const symbols = store.getSymbolsByName(name);
    const sym = symbols.find((s) => s.type === "function" || s.type === "method");

    if (!sym) {
      return notFound(res);
    }

    const response = buildCallGraphResponse(sym, maxDepth, direction, nodeCap);
    if (compact) {
      const compactResponse: CompactCallGraphResponse = toCompactCallGraphResponse(response);
      return res.json(compactResponse);
    }
    return res.json(response);
  });

  app.get("/callers-recursive/:name", (req, res) => {
    const { name } = req.params;
    const maxDepth = Math.min(parseInt((req.query.depth as string) || String(CALLGRAPH_DEFAULT_DEPTH), 10), CALLGRAPH_MAX_DEPTH);
    const context = parseCallerLookupContext(req.query as Record<string, unknown>);
    const nodeCap = Math.min(parseInt((req.query.nodeCap as string) || String(CALLGRAPH_DEFAULT_NODE_CAP), 10), CALLGRAPH_MAX_NODE_CAP);
    const { symbol: sym } = resolveFunctionSymbol(name, context);

    if (!sym) {
      return notFound(res);
    }

    return res.json(buildCallGraphResponse(sym, maxDepth, "callers", nodeCap));
  });

  function computeDepth(root: Pick<CallGraphResponse["root"], "callers" | "callees">): number {
    return Math.max(computeEdgeDepth(root.callees), computeEdgeDepth(root.callers ?? []));
  }

  function computeEdgeDepth(edges: CallGraphEdge[]): number {
    if (edges.length === 0) return 0;
    let max = 0;
    for (const e of edges) {
      const childDepth = e.children ? computeEdgeDepth(e.children) : 0;
      if (childDepth + 1 > max) max = childDepth + 1;
    }
    return max;
  }

  function countCallGraphEdges(edges: CallGraphEdge[]): number {
    let count = 0;
    for (const edge of edges) {
      count += 1;
      if (edge.children) {
        count += countCallGraphEdges(edge.children);
      }
    }
    return count;
  }

  function parseCallGraphDirection(value: unknown): CallGraphDirection {
    return value === "callers" || value === "both" ? value : "callees";
  }

  return app;
}
