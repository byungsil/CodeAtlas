import {
  CallReference,
  ConfidenceLevel,
  CallerQueryResponse,
  FunctionResponse,
  MatchReason,
  ClassResponse,
  ResultWindow,
  SymbolLookupResponse,
} from "./models/responses";
import { Symbol } from "./models/symbol";

type LookupMetadata = Pick<FunctionResponse, "lookupMode" | "confidence" | "matchReasons" | "ambiguity">;

export function deriveLegacyLookupMetadata(candidateCount: number): LookupMetadata {
  if (candidateCount > 1) {
    return {
      lookupMode: "heuristic",
      confidence: "ambiguous",
      matchReasons: ["ambiguous_top_score"],
      ambiguity: { candidateCount },
    };
  }

  return {
    lookupMode: "heuristic",
    confidence: "high_confidence_heuristic",
    matchReasons: [],
  };
}

export function buildExactLookupResponse(params: {
  symbol: Symbol;
  matchedBy: "id" | "qualifiedName" | "both";
}): SymbolLookupResponse {
  const matchReasons: MatchReason[] = [];

  if (params.matchedBy === "id" || params.matchedBy === "both") {
    matchReasons.push("exact_id_match");
  }
  if (params.matchedBy === "qualifiedName" || params.matchedBy === "both") {
    matchReasons.push("exact_qualified_name_match");
  }

  return {
    lookupMode: "exact",
    symbol: params.symbol,
    confidence: "exact",
    matchReasons,
  };
}

export function makeResolvedCallReference(params: {
  symbol: Symbol;
  filePath: string;
  line: number;
  confidence?: ConfidenceLevel;
  matchReasons?: MatchReason[];
}): CallReference {
  return {
    symbolId: params.symbol.id,
    symbolName: params.symbol.name,
    qualifiedName: params.symbol.qualifiedName,
    filePath: params.filePath,
    line: params.line,
    confidence: params.confidence ?? "high_confidence_heuristic",
    matchReasons: params.matchReasons ?? [],
  };
}

export function buildFunctionResponse(params: {
  symbol: Symbol;
  candidateCount: number;
  callers: CallReference[];
  callees: CallReference[];
}): FunctionResponse {
  const metadata = deriveLegacyLookupMetadata(params.candidateCount);
  return {
    ...metadata,
    symbol: params.symbol,
    callers: params.callers,
    callees: params.callees,
  };
}

export function buildCallerQueryResponse(params: {
  symbol: Symbol;
  candidateCount: number;
  callers: CallReference[];
  totalCount: number;
  truncated: boolean;
  limitApplied?: number;
}): CallerQueryResponse {
  const metadata = deriveLegacyLookupMetadata(params.candidateCount);
  return {
    ...metadata,
    symbol: params.symbol,
    window: buildResultWindow(params.callers.length, params.totalCount, params.truncated, params.limitApplied),
    callers: params.callers,
    totalCount: params.totalCount,
    truncated: params.truncated,
  };
}

export function buildClassResponse(params: {
  symbol: Symbol;
  candidateCount: number;
  members: Symbol[];
}): ClassResponse {
  const metadata = deriveLegacyLookupMetadata(params.candidateCount);
  return {
    ...metadata,
    symbol: params.symbol,
    members: params.members,
  };
}

export function buildResultWindow(
  returnedCount: number,
  totalCount: number,
  truncated: boolean,
  limitApplied?: number,
): ResultWindow {
  return {
    returnedCount,
    totalCount,
    truncated,
    ...(limitApplied !== undefined ? { limitApplied } : {}),
  };
}
