import { Symbol } from "./models/symbol";
import {
  CallGraphEdge,
  CallGraphResponse,
  CompactCallGraphNode,
  CompactCallGraphResponse,
  CompactFileSymbol,
  CompactReferenceQueryResponse,
  CompactReferenceRecord,
  FileSymbolsResponse,
  ReferenceQueryResponse,
  ResolvedReference,
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
    ...(edge.children ? { children: edge.children.map(cloneCallGraphEdge) } : {}),
  };
}
