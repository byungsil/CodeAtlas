import { buildResultWindow, rankHeuristicCandidates } from "./response-metadata";
import { performance } from "perf_hooks";
import { Symbol as CodeSymbol } from "./models/symbol";
import {
  InvestigationAnchorSummary,
  InvestigationEvidence,
  InvestigationSuggestedLookupCandidate,
  InvestigationWorkflowStep,
  InvestigateWorkflowResponse,
  PropagationEventRecord,
} from "./models/responses";
import { Store } from "./storage/store";

type WorkflowSymbol = NonNullable<ReturnType<Store["getSymbolById"]>>;
type PropagationDirection = "incoming" | "outgoing";

export interface WorkflowQueryContext {
  getSymbolById(id: string): ReturnType<Store["getSymbolById"]>;
  getSymbolsByName(name: string): ReturnType<Store["getSymbolsByName"]>;
  getCallees(symbolId: string): ReturnType<Store["getCallees"]>;
  getPropagation(symbolId: string, direction: PropagationDirection): PropagationEventRecord[];
}

export interface WorkflowProfiler {
  measure<T>(phase: string, action: () => T): T;
  flush(extra?: Record<string, unknown>): void;
}

export function createWorkflowQueryContext(store: Store): WorkflowQueryContext {
  const symbolCache = new Map<string, ReturnType<Store["getSymbolById"]>>();
  const symbolsByNameCache = new Map<string, ReturnType<Store["getSymbolsByName"]>>();
  const calleesCache = new Map<string, ReturnType<Store["getCallees"]>>();
  const propagationCache = new Map<string, PropagationEventRecord[]>();

  return {
    getSymbolById(id: string) {
      if (!symbolCache.has(id)) {
        symbolCache.set(id, store.getSymbolById(id));
      }
      return symbolCache.get(id);
    },
    getSymbolsByName(name: string) {
      if (!symbolsByNameCache.has(name)) {
        symbolsByNameCache.set(name, store.getSymbolsByName(name));
      }
      return symbolsByNameCache.get(name) ?? [];
    },
    getCallees(symbolId: string) {
      if (!calleesCache.has(symbolId)) {
        calleesCache.set(symbolId, store.getCallees(symbolId));
      }
      return calleesCache.get(symbolId) ?? [];
    },
    getPropagation(symbolId: string, direction: PropagationDirection) {
      const key = `${direction}:${symbolId}`;
      if (!propagationCache.has(key)) {
        propagationCache.set(
          key,
          direction === "incoming" ? store.getIncomingPropagation(symbolId) : store.getOutgoingPropagation(symbolId),
        );
      }
      return propagationCache.get(key) ?? [];
    },
  };
}

export function createWorkflowProfiler(params: {
  channel: "http" | "mcp";
  source: WorkflowSymbol;
  target?: WorkflowSymbol;
  maxDepth: number;
  maxEdges: number;
}): WorkflowProfiler {
  const enabled = matchesTruthyEnv(process.env.CODEATLAS_PROFILE_WORKFLOW);
  const phaseTimings = new Map<string, number>();
  const startedAt = performance.now();

  return {
    measure<T>(phase: string, action: () => T): T {
      if (!enabled) {
        return action();
      }
      const phaseStart = performance.now();
      try {
        return action();
      } finally {
        phaseTimings.set(phase, Number(((phaseTimings.get(phase) ?? 0) + (performance.now() - phaseStart)).toFixed(3)));
      }
    },
    flush(extra?: Record<string, unknown>) {
      if (!enabled) {
        return;
      }
      const totalMs = Number((performance.now() - startedAt).toFixed(3));
      console.error(JSON.stringify({
        event: "investigate_workflow_profile",
        channel: params.channel,
        sourceQualifiedName: params.source.qualifiedName,
        targetQualifiedName: params.target?.qualifiedName,
        maxDepth: params.maxDepth,
        maxEdges: params.maxEdges,
        totalMs,
        phaseTimingsMs: Object.fromEntries(Array.from(phaseTimings.entries())),
        ...extra,
      }));
    },
  };
}

export function toInvestigationAnchorSummary(params: {
  symbol?: CodeSymbol;
  anchor?: PropagationEventRecord["sourceAnchor"];
  fallbackFilePath?: string;
  fallbackLine?: number;
}): InvestigationAnchorSummary {
  const { symbol, anchor, fallbackFilePath, fallbackLine } = params;
  return {
    ...(symbol ? {
      symbolId: symbol.id,
      symbolName: symbol.name,
      qualifiedName: symbol.qualifiedName,
      filePath: symbol.filePath,
      line: symbol.line,
    } : {}),
    ...(anchor?.anchorKind ? { anchorKind: anchor.anchorKind } : {}),
    ...(anchor?.expressionText ? { expressionText: anchor.expressionText } : {}),
    ...(!symbol && fallbackFilePath ? { filePath: fallbackFilePath } : {}),
    ...(!symbol && fallbackLine ? { line: fallbackLine } : {}),
  };
}

export function buildPropagationWorkflowSteps(
  queryContext: WorkflowQueryContext,
  events: PropagationEventRecord[],
  startHop: number,
): InvestigationWorkflowStep[] {
  return events.map((event, index) => {
    const ownerSymbol = event.ownerSymbolId ? queryContext.getSymbolById(event.ownerSymbolId) : undefined;
    return {
      hop: startHop + index,
      handoffKind: event.propagationKind,
      ...(ownerSymbol ? {
        ownerSymbolId: ownerSymbol.id,
        ownerQualifiedName: ownerSymbol.qualifiedName,
      } : {}),
      from: toInvestigationAnchorSummary({
        symbol: event.sourceAnchor.symbolId ? queryContext.getSymbolById(event.sourceAnchor.symbolId) : undefined,
        anchor: event.sourceAnchor,
        fallbackFilePath: event.filePath,
        fallbackLine: event.line,
      }),
      to: toInvestigationAnchorSummary({
        symbol: event.targetAnchor.symbolId ? queryContext.getSymbolById(event.targetAnchor.symbolId) : undefined,
        anchor: event.targetAnchor,
        fallbackFilePath: event.filePath,
        fallbackLine: event.line,
      }),
      filePath: event.filePath,
      line: event.line,
      confidence: event.confidence,
      risks: event.risks,
    };
  });
}

export function buildWorkflowHandoffPoints(
  queryContext: WorkflowQueryContext,
  source: WorkflowSymbol,
  target: WorkflowSymbol | undefined,
  mainPath: InvestigationWorkflowStep[],
  maxEdges: number,
): InvestigationWorkflowStep[] {
  const candidateSymbolIds = new Set<string>([
    source.id,
    ...(target ? [target.id] : []),
    ...mainPath.flatMap((step) => [step.from.symbolId, step.to.symbolId].filter((value): value is string => Boolean(value))),
    ...queryContext.getCallees(source.id).map((call) => call.calleeId),
  ]);

  const propagationCandidates = Array.from(candidateSymbolIds)
    .flatMap((symbolId) => [
      ...queryContext.getPropagation(symbolId, "incoming"),
      ...queryContext.getPropagation(symbolId, "outgoing"),
    ])
    .filter((event) =>
      event.propagationKind === "fieldWrite"
      || event.propagationKind === "fieldRead"
      || event.propagationKind === "argumentToParameter"
      || event.propagationKind === "returnValue")
    .sort((a, b) =>
      a.filePath.localeCompare(b.filePath)
      || a.line - b.line
      || a.propagationKind.localeCompare(b.propagationKind));

  const seen = new Set<string>();
  const extraSteps = buildPropagationWorkflowSteps(queryContext, propagationCandidates, mainPath.length + 1)
    .filter((step) => {
      const key = `${step.handoffKind}:${step.filePath}:${step.line}:${step.from.symbolId ?? step.from.expressionText ?? ""}:${step.to.symbolId ?? step.to.expressionText ?? ""}`;
      if (seen.has(key)) {
        return false;
      }
      seen.add(key);
      return true;
    })
    .slice(0, Math.max(0, maxEdges - mainPath.length));

  return mainPath.concat(extraSteps).map((step, index) => ({ ...step, hop: index + 1 }));
}

export function buildWorkflowEvidence(
  queryContext: WorkflowQueryContext,
  source: WorkflowSymbol,
  handoffPoints: InvestigationWorkflowStep[],
  maxEdges: number,
): { evidence: InvestigationEvidence[]; totalCandidateCount: number } {
  const evidence: InvestigationEvidence[] = [];
  const includeOwnerContext = source.type === "variable";
  const mainPathTargets = new Set(handoffPoints.filter((step) => step.handoffKind === "call").map((step) => step.to.symbolId).filter((value): value is string => Boolean(value)));

  for (const call of queryContext.getCallees(source.id)) {
    const callee = queryContext.getSymbolById(call.calleeId);
    if (!callee || mainPathTargets.has(callee.id)) {
      continue;
    }
    evidence.push({
      kind: "adjacentCall",
      summary: `${source.qualifiedName} directly calls ${callee.qualifiedName}.`,
      filePath: call.filePath,
      line: call.line,
      relatedSymbolId: callee.id,
      relatedQualifiedName: callee.qualifiedName,
      confidence: "high",
      risks: [],
    });
  }

  for (const step of handoffPoints) {
    if (step.handoffKind === "fieldWrite") {
      evidence.push({
        kind: "fieldAssignment",
        summary: `${step.to.qualifiedName ?? step.to.expressionText ?? "field"} is assigned from nearby workflow state.`,
        filePath: step.filePath,
        line: step.line,
        relatedSymbolId: step.to.symbolId,
        relatedQualifiedName: step.to.qualifiedName,
        confidence: step.confidence,
        risks: step.risks,
      });
      if (includeOwnerContext && step.ownerQualifiedName) {
        evidence.push({
          kind: "ownerContext",
          summary: `${step.ownerQualifiedName} owns the nearby field assignment step.`,
          filePath: step.filePath,
          line: step.line,
          relatedSymbolId: step.ownerSymbolId,
          relatedQualifiedName: step.ownerQualifiedName,
          confidence: step.confidence,
          risks: step.risks,
        });
      }
    } else if (step.handoffKind === "fieldRead") {
      evidence.push({
        kind: "fieldRead",
        summary: `${step.from.qualifiedName ?? step.from.expressionText ?? "field"} is read into the nearby workflow.`,
        filePath: step.filePath,
        line: step.line,
        relatedSymbolId: step.from.symbolId,
        relatedQualifiedName: step.from.qualifiedName,
        confidence: step.confidence,
        risks: step.risks,
      });
      if (includeOwnerContext && step.ownerQualifiedName) {
        evidence.push({
          kind: "ownerContext",
          summary: `${step.ownerQualifiedName} owns the nearby field read step.`,
          filePath: step.filePath,
          line: step.line,
          relatedSymbolId: step.ownerSymbolId,
          relatedQualifiedName: step.ownerQualifiedName,
          confidence: step.confidence,
          risks: step.risks,
        });
      }
    } else if (step.handoffKind === "argumentToParameter") {
      evidence.push({
        kind: "boundaryArgument",
        summary: `${step.from.qualifiedName ?? step.from.expressionText ?? "value"} is passed into ${step.to.qualifiedName ?? step.to.expressionText ?? "a downstream parameter"}.`,
        filePath: step.filePath,
        line: step.line,
        relatedSymbolId: step.to.symbolId ?? step.from.symbolId,
        relatedQualifiedName: step.to.qualifiedName ?? step.from.qualifiedName,
        confidence: step.confidence,
        risks: step.risks,
      });
    } else if (step.handoffKind === "returnValue") {
      evidence.push({
        kind: "boundaryReturn",
        summary: `${step.from.qualifiedName ?? step.from.expressionText ?? "value"} returns into ${step.to.qualifiedName ?? step.to.expressionText ?? "the nearby workflow"}.`,
        filePath: step.filePath,
        line: step.line,
        relatedSymbolId: step.from.symbolId ?? step.to.symbolId,
        relatedQualifiedName: step.from.qualifiedName ?? step.to.qualifiedName,
        confidence: step.confidence,
        risks: step.risks,
      });
    }
  }

  const seen = new Set<string>();
  const boundedEvidence = evidence
    .sort((left, right) =>
      evidenceKindPriority(right.kind) - evidenceKindPriority(left.kind)
      || left.filePath.localeCompare(right.filePath)
      || left.line - right.line)
    .filter((item) => {
      const key = `${item.kind}:${item.filePath}:${item.line}:${item.relatedSymbolId ?? item.relatedQualifiedName ?? item.summary}`;
      if (seen.has(key)) {
        return false;
      }
      seen.add(key);
      return true;
    })
    .slice(0, Math.max(1, Math.min(6, maxEdges)));

  return {
    evidence: boundedEvidence,
    totalCandidateCount: evidence.length,
  };
}

function evidenceKindPriority(kind: InvestigationEvidence["kind"]): number {
  switch (kind) {
    case "fieldAssignment":
      return 5;
    case "fieldRead":
      return 4;
    case "ownerContext":
      return 3;
    case "boundaryReturn":
      return 2;
    case "boundaryArgument":
      return 1;
    case "adjacentCall":
      return 0;
  }
}

export function buildWorkflowCoverageConfidence(params: {
  source: WorkflowSymbol;
  target?: WorkflowSymbol;
  steps: InvestigationWorkflowStep[];
  pathFound: boolean;
  truncated: boolean;
}): "high" | "partial" | "weak" {
  const { source, target, steps, pathFound, truncated } = params;
  const symbols = [source, target].filter((symbol): symbol is WorkflowSymbol => Boolean(symbol));
  const hasElevatedFileRisk = symbols.some((symbol) =>
    symbol.parseFragility === "elevated"
    || symbol.macroSensitivity === "high"
    || symbol.includeHeaviness === "heavy");
  const hasPartialStep = steps.some((step) => step.confidence === "partial" || step.risks.length > 0);

  if (!pathFound && hasElevatedFileRisk) {
    return "weak";
  }
  if (truncated || hasPartialStep || hasElevatedFileRisk) {
    return "partial";
  }
  return "high";
}

export function buildWorkflowDiagnostics(params: {
  source: WorkflowSymbol;
  target?: WorkflowSymbol;
  steps: InvestigationWorkflowStep[];
  pathFound: boolean;
  truncated: boolean;
  coverageConfidence: "high" | "partial" | "weak";
  getFileRiskNotes: (symbol: WorkflowSymbol) => string[];
}): string[] {
  const { source, target, steps, pathFound, truncated, coverageConfidence, getFileRiskNotes } = params;
  const diagnostics: string[] = [];
  const riskMarkers = Array.from(new Set(steps.flatMap((step) => step.risks))).sort();

  if (!pathFound) {
    diagnostics.push(target
      ? "No bounded stitched workflow path was found between the requested exact source and target."
      : "No bounded workflow path was found from the requested exact source.");
  }
  if (truncated) {
    diagnostics.push("Traversal bounds truncated the workflow answer before every reachable segment could be explored.");
  }
  if (riskMarkers.includes("pointerHeavyFlow")) {
    diagnostics.push("Pointer-heavy flow appears in the stitched workflow, so alias-sensitive continuity may be incomplete.");
  }
  if (riskMarkers.includes("receiverAmbiguity")) {
    diagnostics.push("At least one object-state handoff is structurally weaker than a direct receiver-resolved path.");
  }
  if (steps.some((step) => step.handoffKind === "argumentToParameter")) {
    diagnostics.push("At least one workflow segment crosses a callable boundary through argument-to-parameter propagation.");
  }
  if (steps.some((step) => step.handoffKind === "returnValue")) {
    diagnostics.push("At least one workflow segment depends on a helper return value feeding the downstream workflow.");
  }
  if (coverageConfidence === "weak") {
    diagnostics.push("Nearby file risk signals suggest the absence of a stronger path may reflect weak coverage rather than true structural absence.");
  } else if (coverageConfidence === "partial") {
    diagnostics.push("The returned workflow should be treated as partial guidance rather than complete proof.");
  }
  diagnostics.push(...getFileRiskNotes(source));
  if (target && target.id !== source.id) {
    diagnostics.push(...getFileRiskNotes(target));
  }

  return Array.from(new Set(diagnostics));
}

export function buildWorkflowSuggestedQueries(
  queryContext: WorkflowQueryContext,
  source: WorkflowSymbol,
  target: WorkflowSymbol | undefined,
  steps: InvestigationWorkflowStep[],
): string[] {
  const queries = [
    `lookup_symbol qualifiedName=${source.qualifiedName}`,
    `find_references qualifiedName=${source.qualifiedName}`,
  ];
  const boundarySymbols = new Set<string>();
  const adjacentHelperSymbols = new Set<string>();

  if (target) {
    queries.push(`lookup_symbol qualifiedName=${target.qualifiedName}`);
    queries.push(`find_references qualifiedName=${target.qualifiedName}`);
    if ((source.type === "function" || source.type === "method") && (target.type === "function" || target.type === "method")) {
      queries.push(`trace_call_path sourceQualifiedName=${source.qualifiedName} targetQualifiedName=${target.qualifiedName}`);
    }
  }

  if (steps.some((step) => step.handoffKind !== "call")) {
    queries.push(`trace_variable_flow qualifiedName=${source.qualifiedName} maxDepth=3`);
  }
  if (steps.some((step) => step.handoffKind === "argumentToParameter" || step.handoffKind === "returnValue")) {
    queries.push(`trace_variable_flow qualifiedName=${source.qualifiedName} maxDepth=4 propagationKinds=argumentToParameter,returnValue,fieldWrite,fieldRead`);
  }
  for (const step of steps) {
    if (step.handoffKind === "argumentToParameter" && step.to.qualifiedName) {
      boundarySymbols.add(step.to.qualifiedName);
    }
    if (step.handoffKind === "returnValue" && step.from.qualifiedName) {
      boundarySymbols.add(step.from.qualifiedName);
    }
  }
  const mainPathTargets = new Set(
    steps
      .filter((step) => step.handoffKind === "call")
      .map((step) => step.to.symbolId)
      .filter((value): value is string => Boolean(value)),
  );
  for (const call of queryContext.getCallees(source.id)) {
    if (mainPathTargets.has(call.calleeId)) {
      continue;
    }
    const callee = queryContext.getSymbolById(call.calleeId);
    if (!callee || callee.qualifiedName === target?.qualifiedName) {
      continue;
    }
    if (callee.type === "function" || callee.type === "method") {
      adjacentHelperSymbols.add(callee.qualifiedName);
    }
  }
  for (const qualifiedName of Array.from(boundarySymbols).filter((value) => value !== source.qualifiedName && value !== target?.qualifiedName).slice(0, 2)) {
    queries.push(`lookup_symbol qualifiedName=${qualifiedName}`);
    queries.push(`find_references qualifiedName=${qualifiedName}`);
    const symbol = steps
      .flatMap((step) => [step.from.symbolId, step.to.symbolId])
      .filter((symbolId): symbolId is string => Boolean(symbolId))
      .map((symbolId) => queryContext.getSymbolById(symbolId))
      .find((candidate) => candidate?.qualifiedName === qualifiedName);
    if (symbol?.type === "function" || symbol?.type === "method") {
      queries.push(`lookup_function name=${symbol.name} recentQualifiedName=${source.qualifiedName}`);
    } else if (symbol?.type === "class" || symbol?.type === "struct") {
      queries.push(`lookup_class name=${symbol.name} recentQualifiedName=${source.qualifiedName}`);
    }
  }
  for (const qualifiedName of Array.from(adjacentHelperSymbols).filter((value) => value !== source.qualifiedName && value !== target?.qualifiedName).slice(0, 3)) {
    const symbol = queryContext.getSymbolById(steps
      .flatMap((step) => [step.from.symbolId, step.to.symbolId])
      .filter((symbolId): symbolId is string => Boolean(symbolId))
      .find((symbolId) => queryContext.getSymbolById(symbolId)?.qualifiedName === qualifiedName)
      ?? queryContext.getCallees(source.id).find((call) => queryContext.getSymbolById(call.calleeId)?.qualifiedName === qualifiedName)?.calleeId
      ?? "");
    if (!symbol || !(symbol.type === "function" || symbol.type === "method")) {
      continue;
    }
    queries.push(`lookup_function name=${symbol.name} recentQualifiedName=${source.qualifiedName}`);
    queries.push(`find_callers name=${symbol.name} recentQualifiedName=${source.qualifiedName}`);
  }
  if (source.parseFragility === "elevated" || source.macroSensitivity === "high") {
    queries.push(`list_file_symbols filePath=${source.filePath}`);
  }
  if (target && (target.parseFragility === "elevated" || target.macroSensitivity === "high")) {
    queries.push(`list_file_symbols filePath=${target.filePath}`);
  }

  return Array.from(new Set(queries));
}

function buildSuggestedLookupCandidates(
  queryContext: WorkflowQueryContext,
  source: WorkflowSymbol,
  target: WorkflowSymbol | undefined,
  steps: InvestigationWorkflowStep[],
  pathConfidence: InvestigateWorkflowResponse["pathConfidence"],
  coverageConfidence: InvestigateWorkflowResponse["coverageConfidence"],
  evidence: InvestigationEvidence[],
): InvestigationSuggestedLookupCandidate[] {
  const candidateSymbols = new Map<string, WorkflowSymbol>();
  const candidatePriority = new Map<string, number>();
  const mainPathTargets = new Set(
    steps
      .filter((step) => step.handoffKind === "call")
      .map((step) => step.to.symbolId)
      .filter((value): value is string => Boolean(value)),
  );

  for (const step of steps) {
    for (const symbolId of [step.from.symbolId, step.to.symbolId]) {
      if (!symbolId) continue;
      const symbol = queryContext.getSymbolById(symbolId);
      if (!symbol || symbol.id === source.id || symbol.id === target?.id) continue;
      if (symbol.type === "function" || symbol.type === "method" || symbol.type === "class" || symbol.type === "struct") {
        candidateSymbols.set(symbol.id, symbol);
        candidatePriority.set(symbol.id, Math.max(candidatePriority.get(symbol.id) ?? 0, suggestedCandidateStepPriority(step.handoffKind)));
      }
    }
    if (step.ownerSymbolId) {
      const ownerSymbol = queryContext.getSymbolById(step.ownerSymbolId);
      if (ownerSymbol && ownerSymbol.id !== source.id && ownerSymbol.id !== target?.id) {
        if (ownerSymbol.type === "function" || ownerSymbol.type === "method" || ownerSymbol.type === "class" || ownerSymbol.type === "struct") {
          candidateSymbols.set(ownerSymbol.id, ownerSymbol);
          candidatePriority.set(ownerSymbol.id, Math.max(candidatePriority.get(ownerSymbol.id) ?? 0, suggestedCandidateStepPriority(step.handoffKind) + 10));
        }
      }
    }
  }
  for (const call of queryContext.getCallees(source.id)) {
    if (mainPathTargets.has(call.calleeId)) continue;
    const callee = queryContext.getSymbolById(call.calleeId);
    if (!callee || callee.id === target?.id) continue;
    if (callee.type === "function" || callee.type === "method" || callee.type === "class" || callee.type === "struct") {
      candidateSymbols.set(callee.id, callee);
      candidatePriority.set(callee.id, Math.max(candidatePriority.get(callee.id) ?? 0, 30));
    }
  }

  const context = {
    language: source.language,
    subsystem: source.subsystem,
    module: source.module,
    projectArea: source.projectArea,
    artifactKind: source.artifactKind,
    filePath: source.filePath,
    anchorQualifiedName: source.qualifiedName,
    anchorNeighborSymbolIds: Array.from(new Set([
      ...queryContext.getCallees(source.id).map((call) => call.calleeId),
      ...steps.flatMap((step) => [step.from.symbolId, step.to.symbolId]).filter((value): value is string => Boolean(value)),
    ])),
    anchorScopePrefixes: collectScopePrefixes(source.qualifiedName),
  };

  return Array.from(candidateSymbols.values())
    .map((symbol) => {
      const sameName = queryContext.getSymbolsByName(symbol.name)
        .filter((candidate) => candidate.type === symbol.type || isClassLike(candidate.type) && isClassLike(symbol.type) || isCallable(candidate.type) && isCallable(symbol.type));
      const ranked = rankHeuristicCandidates(sameName, context);
      const selected = ranked[0] ?? symbol;
      const query = isClassLike(selected.type)
        ? `lookup_class name=${selected.name} recentQualifiedName=${source.qualifiedName}`
        : `lookup_function name=${selected.name} recentQualifiedName=${source.qualifiedName}`;
      const priority = candidatePriority.get(symbol.id) ?? 0;
      const supportingEvidence = buildSupportingEvidence(queryContext, source, target, selected, steps, evidence);
      return {
        shortName: selected.name,
        symbol: selected,
        query,
        confidence: ranked.length > 1 ? "ambiguous" : "high_confidence_heuristic",
        ...(buildSuggestedCandidateAdvisory(selected, pathConfidence, coverageConfidence)
          ? { advisory: buildSuggestedCandidateAdvisory(selected, pathConfidence, coverageConfidence) }
          : {}),
        contextSummary: {
          qualifiedName: source.qualifiedName,
          artifactKind: source.artifactKind,
          subsystem: source.subsystem,
          module: source.module,
          projectArea: source.projectArea,
          filePath: source.filePath,
        },
        ...(supportingEvidence.length > 0
          ? { supportingEvidence }
          : {}),
        sortPriority: priority,
        supportingEvidenceScore: supportingEvidenceScore(supportingEvidence),
        ...(ranked.length > 1 ? { ambiguity: { candidateCount: ranked.length } } : {}),
      } as InvestigationSuggestedLookupCandidate & { sortPriority: number; supportingEvidenceScore: number };
    })
    .sort((left, right) =>
      right.sortPriority - left.sortPriority
      || right.supportingEvidenceScore - left.supportingEvidenceScore
      || Number(Boolean(right.ambiguity)) - Number(Boolean(left.ambiguity))
      || left.shortName.localeCompare(right.shortName)
      || left.symbol.qualifiedName.localeCompare(right.symbol.qualifiedName))
    .filter((candidate, index, all) =>
      all.findIndex((other) => other.symbol.id === candidate.symbol.id) === index)
    .slice(0, 4)
    .map(({ sortPriority: _sortPriority, supportingEvidenceScore: _supportingEvidenceScore, ...candidate }) => candidate);
}

function suggestedCandidateStepPriority(handoffKind: InvestigationWorkflowStep["handoffKind"]): number {
  switch (handoffKind) {
    case "argumentToParameter":
      return 60;
    case "returnValue":
      return 55;
    case "fieldWrite":
      return 50;
    case "fieldRead":
      return 45;
    case "call":
      return 40;
  }
  return 0;
}

function buildSuggestedCandidateAdvisory(
  symbol: WorkflowSymbol,
  pathConfidence: InvestigateWorkflowResponse["pathConfidence"],
  coverageConfidence: InvestigateWorkflowResponse["coverageConfidence"],
): string | undefined {
  const isCallableCandidate = symbol.type === "function" || symbol.type === "method";
  if (coverageConfidence === "weak") {
    return isCallableCandidate
      ? "Suggested owning callable under weak workflow coverage; inspect the callable and nearby file context before treating it as definitive."
      : "Suggested under weak workflow coverage; verify with direct symbol or file-level inspection before treating it as definitive.";
  }
  if (pathConfidence === "partial" || coverageConfidence === "partial") {
    return isCallableCandidate
      ? "Suggested owning callable under partial workflow coverage; inspect the callable before treating it as a definitive continuation."
      : "Suggested under partial workflow coverage; verify before treating it as a definitive continuation.";
  }
  return undefined;
}

function buildSupportingEvidence(
  queryContext: WorkflowQueryContext,
  source: WorkflowSymbol,
  target: WorkflowSymbol | undefined,
  symbol: WorkflowSymbol,
  steps: InvestigationWorkflowStep[],
  evidence: InvestigationEvidence[],
): Array<{
  kind: InvestigationEvidence["kind"];
  relatedQualifiedName?: string;
}> {
  const directEvidence = evidence
    .filter((item) =>
      item.relatedSymbolId === symbol.id
      || item.relatedQualifiedName === symbol.qualifiedName)
    .map((item) => ({
      kind: item.kind,
      ...(item.relatedQualifiedName ? { relatedQualifiedName: item.relatedQualifiedName } : {}),
    }));
  const inferredEvidence: Array<{
    kind: InvestigationEvidence["kind"];
    relatedQualifiedName?: string;
  }> = [];

  const isDirectSourceCallee = queryContext.getCallees(source.id).some((call) => call.calleeId === symbol.id);
  const appearsOnMainPath = steps.some((step) =>
    step.handoffKind === "call"
    && step.to.symbolId === symbol.id);
  if (isDirectSourceCallee || appearsOnMainPath) {
    inferredEvidence.push({
      kind: "adjacentCall",
      relatedQualifiedName: symbol.qualifiedName,
    });
  }
  if (target && queryContext.getCallees(symbol.id).some((call) => call.calleeId === target.id)) {
    inferredEvidence.push({
      kind: "adjacentCall",
      relatedQualifiedName: target.qualifiedName,
    });
  }
  const returnedInto = steps.find((step) =>
    step.handoffKind === "returnValue"
    && (step.from.symbolId === symbol.id || step.from.qualifiedName === symbol.qualifiedName));
  if (returnedInto?.to.expressionText) {
    const forwardedArgument = steps.find((step) =>
      step.handoffKind === "argumentToParameter"
      && step.from.expressionText === returnedInto.to.expressionText
      && step.to.symbolId
      && step.to.symbolId !== symbol.id);
    if (forwardedArgument?.to.qualifiedName) {
      inferredEvidence.push({
        kind: "adjacentCall",
        relatedQualifiedName: forwardedArgument.to.qualifiedName,
      });
      if (target) {
        const forwardedTarget = queryContext.getSymbolById(forwardedArgument.to.symbolId!);
        if (forwardedTarget && queryContext.getCallees(forwardedTarget.id).some((call) => call.calleeId === target.id)) {
          inferredEvidence.push({
            kind: "adjacentCall",
            relatedQualifiedName: target.qualifiedName,
          });
        }
      }
    }
  }

  return directEvidence
    .concat(inferredEvidence)
    .filter((item, index, all) =>
      all.findIndex((other) =>
        other.kind === item.kind
        && other.relatedQualifiedName === item.relatedQualifiedName) === index)
    .slice(0, 5);
}

function supportingEvidenceScore(
  items: Array<{
    kind: InvestigationEvidence["kind"];
    relatedQualifiedName?: string;
  }>,
): number {
  return items.reduce((score, item) => score + supportingEvidenceKindWeight(item.kind), 0);
}

function supportingEvidenceKindWeight(kind: InvestigationEvidence["kind"]): number {
  switch (kind) {
    case "ownerContext":
      return 4;
    case "fieldAssignment":
      return 3;
    case "fieldRead":
      return 3;
    case "boundaryReturn":
      return 2;
    case "boundaryArgument":
      return 2;
    case "adjacentCall":
      return 1;
  }
  return 0;
}

function filePathNeighborhoodScore(candidatePath: string, contextPath: string): number {
  const candidateParts = normalizePathParts(candidatePath);
  const contextParts = normalizePathParts(contextPath);
  let prefix = 0;
  const maxPrefix = Math.min(candidateParts.length, contextParts.length);
  while (prefix < maxPrefix && candidateParts[prefix] === contextParts[prefix]) {
    prefix += 1;
  }

  const candidateDirs = new Set(candidateParts.slice(0, -1));
  const overlap = contextParts.slice(0, -1).filter((part) => candidateDirs.has(part)).length;
  return prefix * 30 + overlap * 10;
}

function normalizePathParts(filePath: string): string[] {
  return filePath
    .replace(/\\/g, "/")
    .split("/")
    .map((part) => part.toLowerCase())
    .filter((part) => part.length > 0);
}

function isCallable(type: CodeSymbol["type"]): boolean {
  return type === "function" || type === "method";
}

function isClassLike(type: CodeSymbol["type"]): boolean {
  return type === "class" || type === "struct";
}

function collectScopePrefixes(qualifiedName: string): string[] {
  const parts = qualifiedName.split("::");
  const prefixes: string[] = [];
  for (let index = 1; index < parts.length; index += 1) {
    prefixes.push(parts.slice(0, index).join("::"));
  }
  return prefixes;
}

export function buildInvestigateWorkflowResponse(params: {
  queryContext: WorkflowQueryContext;
  source: WorkflowSymbol;
  target?: WorkflowSymbol;
  mainPath: InvestigationWorkflowStep[];
  pathFound: boolean;
  truncated: boolean;
  maxEdges: number;
  lookupMode: InvestigateWorkflowResponse["lookupMode"];
  confidence: InvestigateWorkflowResponse["confidence"];
  matchReasons: InvestigateWorkflowResponse["matchReasons"];
  getFileRiskNotes: (symbol: WorkflowSymbol) => string[];
  profiler?: WorkflowProfiler;
}): InvestigateWorkflowResponse {
  const { queryContext, source, target, mainPath, pathFound, truncated, maxEdges, lookupMode, confidence, matchReasons, getFileRiskNotes, profiler } = params;
  const handoffPoints = profiler
    ? profiler.measure("build_handoff_points", () => buildWorkflowHandoffPoints(queryContext, source, target, mainPath, maxEdges))
    : buildWorkflowHandoffPoints(queryContext, source, target, mainPath, maxEdges);
  const evidenceBundle = profiler
    ? profiler.measure("build_evidence", () => buildWorkflowEvidence(queryContext, source, handoffPoints, maxEdges))
    : buildWorkflowEvidence(queryContext, source, handoffPoints, maxEdges);
  const evidence = evidenceBundle.evidence;
  const pathConfidence = !pathFound || handoffPoints.some((step) => step.confidence === "partial" || step.risks.length > 0) || truncated
    ? "partial"
    : "high";
  const coverageConfidence = buildWorkflowCoverageConfidence({ source, target, steps: handoffPoints, pathFound, truncated });
  const uncertainSegments = [
    ...(!pathFound
      ? [target
        ? "No continuous bounded chain could be stitched between the requested exact anchors."
        : "No bounded workflow continuation was found from the requested exact source."]
      : []),
    ...(handoffPoints.some((step) => step.confidence === "partial")
      ? ["At least one workflow segment is only structurally partial rather than high-confidence."]
      : []),
    ...(handoffPoints.some((step) => step.risks.includes("pointerHeavyFlow"))
      ? ["Pointer-heavy flow interrupts full-confidence continuity in at least one segment."]
      : []),
  ];

  const suggestedFollowUpQueries = profiler
    ? profiler.measure("build_followups", () => buildWorkflowSuggestedQueries(queryContext, source, target, handoffPoints))
    : buildWorkflowSuggestedQueries(queryContext, source, target, handoffPoints);
  const suggestedLookupCandidates = profiler
    ? profiler.measure("build_lookup_candidates", () => buildSuggestedLookupCandidates(queryContext, source, target, handoffPoints, pathConfidence, coverageConfidence, evidence))
    : buildSuggestedLookupCandidates(queryContext, source, target, handoffPoints, pathConfidence, coverageConfidence, evidence);
  const inferredTargetSupportOnly = Boolean(
    target
    && suggestedLookupCandidates.some((candidate) =>
      candidate.supportingEvidence?.some((item) =>
        item.kind === "adjacentCall" && item.relatedQualifiedName === target.qualifiedName))
    && !evidence.some((item) => item.relatedQualifiedName === target.qualifiedName),
  );
  const diagnostics = profiler
    ? profiler.measure("build_diagnostics", () => buildWorkflowDiagnostics({
      source,
      target,
      steps: handoffPoints,
      pathFound,
      truncated,
      coverageConfidence,
      getFileRiskNotes,
    }))
    : buildWorkflowDiagnostics({
      source,
      target,
      steps: handoffPoints,
      pathFound,
      truncated,
      coverageConfidence,
      getFileRiskNotes,
    });

  return {
    lookupMode,
    confidence,
    matchReasons,
    source,
    ...(target ? { target } : {}),
    window: buildResultWindow(mainPath.length, mainPath.length, truncated, maxEdges),
    targetConfidence: "exact",
    pathConfidence,
    coverageConfidence,
    pathFound,
    truncated,
    entry: toInvestigationAnchorSummary({ symbol: source }),
    mainPath,
    handoffPoints,
    evidence,
    ...(target ? { sink: toInvestigationAnchorSummary({ symbol: target }) } : {}),
    uncertainSegments: Array.from(new Set(uncertainSegments)),
    diagnostics,
    suggestedFollowUpQueries,
    ...(suggestedLookupCandidates.length > 0 ? { suggestedLookupCandidates } : {}),
  };
}

function matchesTruthyEnv(value: string | undefined): boolean {
  return value === "1" || value === "true" || value === "TRUE" || value === "yes" || value === "YES";
}
