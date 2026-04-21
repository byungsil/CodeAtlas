import { Symbol } from "./models/symbol";
import {
  CallerQueryResponse,
  CallGraphEdge,
  CallGraphResponse,
  CallReference,
  CompactCallerQueryResponse,
  CompactCallGraphNode,
  CompactCallGraphResponse,
  CompactClassMembersResponse,
  CompactFileGroupedReferenceResponse,
  CompactFileSymbol,
  CompactImpactAnalysisResponse,
  CompactNamespaceSymbolsResponse,
  CompactReferenceQueryResponse,
  CompactReferenceRecord,
  CompactSearchResponse,
  ClassMembersOverviewResponse,
  FileGroup,
  FileGroupedRef,
  FileSymbolsResponse,
  ImpactAnalysisResponse,
  NamespaceSymbolsResponse,
  ReferenceQueryResponse,
  ResolvedReference,
  SearchResponse,
} from "./models/responses";

export function toCompactFileSymbol(symbol: Symbol): CompactFileSymbol {
  return {
    id: symbol.id,
    name: symbol.name,
    qualifiedName: symbol.qualifiedName,
    type: symbol.type,
    line: symbol.line,
    endLine: symbol.endLine,
  };
}

export function toCompactFileSymbolsResponse(response: FileSymbolsResponse) {
  return {
    responseMode: "compact" as const,
    filePath: response.filePath,
    summary: response.summary,
    window: response.window,
    symbols: response.symbols.map(toCompactFileSymbol),
  };
}

export function toCompactCallGraphNode(root: CallGraphResponse["root"]): CompactCallGraphNode {
  return {
    symbol: {
      id: root.symbol.id,
      name: root.symbol.name,
      qualifiedName: root.symbol.qualifiedName,
      filePath: root.symbol.filePath,
      line: root.symbol.line,
    },
    callees: root.callees.map(cloneCallGraphEdge),
    ...(root.callers ? { callers: root.callers.map(cloneCallGraphEdge) } : {}),
  };
}

export function toCompactCallGraphResponse(response: CallGraphResponse): CompactCallGraphResponse {
  return {
    responseMode: "compact",
    reliability: response.reliability,
    ...(response.indexCoverage ? { indexCoverage: response.indexCoverage } : {}),
    ...(response.coverageWarning ? { coverageWarning: response.coverageWarning } : {}),
    root: toCompactCallGraphNode(response.root),
    direction: response.direction,
    depth: response.depth,
    maxDepth: response.maxDepth,
    nodeCount: response.nodeCount,
    nodeCap: response.nodeCap,
    truncated: response.truncated,
  };
}

export function toCompactReferenceRecord(
  reference: ResolvedReference,
  targetQualifiedName: string,
): CompactReferenceRecord {
  return {
    sourceSymbolId: reference.sourceSymbolId,
    sourceQualifiedName: reference.sourceQualifiedName,
    targetSymbolId: reference.targetSymbolId,
    targetQualifiedName,
    category: reference.category,
    filePath: reference.filePath,
    line: reference.line,
  };
}

export function toCompactReferenceQueryResponse(
  response: ReferenceQueryResponse,
  references: CompactReferenceRecord[],
): CompactReferenceQueryResponse {
  return {
    responseMode: "compact",
    reliability: response.reliability,
    ...(response.indexCoverage ? { indexCoverage: response.indexCoverage } : {}),
    ...(response.coverageWarning ? { coverageWarning: response.coverageWarning } : {}),
    lookupMode: response.lookupMode,
    symbol: response.symbol,
    confidence: response.confidence,
    matchReasons: response.matchReasons,
    ...(response.ambiguity ? { ambiguity: response.ambiguity } : {}),
    window: response.window,
    references,
    totalCount: response.totalCount,
    truncated: response.truncated,
    ...(response.category ? { category: response.category } : {}),
    ...(response.filePath ? { filePath: response.filePath } : {}),
    ...(response.subsystem ? { subsystem: response.subsystem } : {}),
    ...(response.module ? { module: response.module } : {}),
    ...(response.projectArea ? { projectArea: response.projectArea } : {}),
    ...(response.artifactKind ? { artifactKind: response.artifactKind } : {}),
    ...(response.groupedBySubsystem ? { groupedBySubsystem: response.groupedBySubsystem } : {}),
    ...(response.groupedByModule ? { groupedByModule: response.groupedByModule } : {}),
    ...(response.groupedByLanguage ? { groupedByLanguage: response.groupedByLanguage } : {}),
  };
}

function cloneCallGraphEdge(edge: CallGraphEdge): CallGraphEdge {
  return {
    targetId: edge.targetId,
    targetName: edge.targetName,
    targetQualifiedName: edge.targetQualifiedName,
    filePath: edge.filePath,
    line: edge.line,
    ...(edge.resolutionKind ? { resolutionKind: edge.resolutionKind } : {}),
    ...(edge.provenanceKind ? { provenanceKind: edge.provenanceKind } : {}),
    ...(edge.children ? { children: edge.children.map(cloneCallGraphEdge) } : {}),
  };
}

function groupByFile(
  items: Array<{ file: string; symbol: string; line: number }>,
): FileGroup[] {
  const groups = new Map<string, FileGroupedRef[]>();
  for (const item of items) {
    const existing = groups.get(item.file) ?? [];
    existing.push({ symbol: item.symbol, line: item.line });
    groups.set(item.file, existing);
  }
  return Array.from(groups.entries())
    .map(([file, refs]) => ({ file, refs: refs.sort((a, b) => a.line - b.line) }))
    .sort((a, b) => a.file.localeCompare(b.file));
}

export function toFileGroupedReferences(references: ResolvedReference[]): FileGroup[] {
  return groupByFile(references.map((r) => ({ file: r.filePath, symbol: r.sourceQualifiedName, line: r.line })));
}

export function toFileGroupedReferenceQueryResponse(
  response: ReferenceQueryResponse,
  references: ResolvedReference[],
): CompactFileGroupedReferenceResponse {
  return {
    responseMode: "compact" as const,
    reliability: response.reliability,
    ...(response.indexCoverage ? { indexCoverage: response.indexCoverage } : {}),
    ...(response.coverageWarning ? { coverageWarning: response.coverageWarning } : {}),
    lookupMode: response.lookupMode,
    symbol: response.symbol,
    confidence: response.confidence,
    matchReasons: response.matchReasons,
    ...(response.ambiguity ? { ambiguity: response.ambiguity } : {}),
    window: response.window,
    fileGroups: toFileGroupedReferences(references),
    totalCount: response.totalCount,
    truncated: response.truncated,
    ...(response.category ? { category: response.category } : {}),
    ...(response.groupedBySubsystem ? { groupedBySubsystem: response.groupedBySubsystem } : {}),
    ...(response.groupedByModule ? { groupedByModule: response.groupedByModule } : {}),
    ...(response.groupedByLanguage ? { groupedByLanguage: response.groupedByLanguage } : {}),
  };
}

export function toCompactCallerQueryResponse(response: CallerQueryResponse): CompactCallerQueryResponse {
  const fileGroups = groupByFile(
    response.callers.map((c: CallReference) => ({ file: c.filePath, symbol: c.qualifiedName, line: c.line })),
  );
  return {
    responseMode: "compact" as const,
    reliability: response.reliability,
    ...(response.indexCoverage ? { indexCoverage: response.indexCoverage } : {}),
    ...(response.coverageWarning ? { coverageWarning: response.coverageWarning } : {}),
    ...(response.selectedReason ? { selectedReason: response.selectedReason } : {}),
    ...(response.bestNextDiscriminator ? { bestNextDiscriminator: response.bestNextDiscriminator } : {}),
    ...(response.suggestedExactQueries ? { suggestedExactQueries: response.suggestedExactQueries } : {}),
    ...(response.topCandidates ? { topCandidates: response.topCandidates } : {}),
    lookupMode: response.lookupMode,
    symbol: response.symbol,
    confidence: response.confidence,
    matchReasons: response.matchReasons,
    ...(response.ambiguity ? { ambiguity: response.ambiguity } : {}),
    window: response.window,
    fileGroups,
    totalCount: response.totalCount,
    truncated: response.truncated,
    ...(response.groupedBySubsystem ? { groupedBySubsystem: response.groupedBySubsystem } : {}),
    ...(response.groupedByModule ? { groupedByModule: response.groupedByModule } : {}),
    ...(response.groupedByLanguage ? { groupedByLanguage: response.groupedByLanguage } : {}),
  };
}

export function toCompactSearchResponse(response: SearchResponse): CompactSearchResponse {
  return {
    responseMode: "compact" as const,
    query: response.query,
    window: response.window,
    results: response.results.map(toCompactFileSymbol),
    totalCount: response.totalCount,
    truncated: response.truncated,
    ...(response.groupedByLanguage ? { groupedByLanguage: response.groupedByLanguage } : {}),
  };
}

export function toCompactImpactAnalysisResponse(response: ImpactAnalysisResponse): CompactImpactAnalysisResponse {
  return {
    responseMode: "compact" as const,
    lookupMode: response.lookupMode,
    symbol: response.symbol,
    confidence: response.confidence,
    matchReasons: response.matchReasons,
    ...(response.ambiguity ? { ambiguity: response.ambiguity } : {}),
    maxDepth: response.maxDepth,
    callerFileGroups: groupByFile(
      response.directCallers.map((c: CallReference) => ({ file: c.filePath, symbol: c.qualifiedName, line: c.line })),
    ),
    calleeFileGroups: groupByFile(
      response.directCallees.map((c: CallReference) => ({ file: c.filePath, symbol: c.qualifiedName, line: c.line })),
    ),
    referenceFileGroups: toFileGroupedReferences(response.directReferences),
    totalAffectedSymbols: response.totalAffectedSymbols,
    totalAffectedFiles: response.totalAffectedFiles,
    topAffectedFiles: response.topAffectedFiles,
    suggestedFollowUpQueries: response.suggestedFollowUpQueries,
    truncated: response.truncated,
    ...(response.affectedSubsystems ? { affectedSubsystems: response.affectedSubsystems } : {}),
    ...(response.affectedModules ? { affectedModules: response.affectedModules } : {}),
    ...(response.affectedLanguages ? { affectedLanguages: response.affectedLanguages } : {}),
  };
}

export function toCompactClassMembersResponse(response: ClassMembersOverviewResponse): CompactClassMembersResponse {
  return {
    responseMode: "compact" as const,
    lookupMode: response.lookupMode,
    symbol: response.symbol,
    confidence: response.confidence,
    matchReasons: response.matchReasons,
    ...(response.ambiguity ? { ambiguity: response.ambiguity } : {}),
    summary: response.summary,
    window: response.window,
    members: response.members.map(toCompactFileSymbol),
  };
}

export function toCompactNamespaceSymbolsResponse(response: NamespaceSymbolsResponse): CompactNamespaceSymbolsResponse {
  return {
    responseMode: "compact" as const,
    lookupMode: response.lookupMode,
    symbol: response.symbol,
    confidence: response.confidence,
    matchReasons: response.matchReasons,
    ...(response.ambiguity ? { ambiguity: response.ambiguity } : {}),
    summary: response.summary,
    window: response.window,
    symbols: response.symbols.map(toCompactFileSymbol),
  };
}
