import {
  CallReference,
  ConfidenceLevel,
  CallerQueryResponse,
  FunctionResponse,
  MatchReason,
  ClassResponse,
  RepresentativeMetadata,
  RepresentativeSelectionReason,
  ResultWindow,
  SymbolLookupResponse,
} from "./models/responses";
import { Symbol } from "./models/symbol";

type LookupMetadata = Pick<FunctionResponse, "lookupMode" | "confidence" | "matchReasons" | "ambiguity">;

export interface HeuristicLookupContext {
  language?: Symbol["language"];
  subsystem?: string;
  module?: string;
  projectArea?: string;
  artifactKind?: Symbol["artifactKind"];
  filePath?: string;
  anchorQualifiedName?: string;
  anchorNeighborSymbolIds?: string[];
  anchorScopePrefixes?: string[];
}

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
  representativeMetadata?: RepresentativeMetadata;
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
    ...params.representativeMetadata,
  };
}

export function deriveRepresentativeMetadata(symbol: Symbol, candidates: Symbol[]): RepresentativeMetadata {
  const uniqueCandidates = dedupeRepresentativeCandidates(candidates);
  const ranked: Array<{
    candidate: Symbol;
    reasons: RepresentativeSelectionReason[];
    score: number;
  }> = uniqueCandidates
    .map((candidate): { candidate: Symbol; reasons: RepresentativeSelectionReason[]; score: number } => {
      const reasons = deriveRepresentativeSelectionReasons(candidate);
      return {
        candidate,
        reasons,
        score: reasons.reduce((sum: number, reason: RepresentativeSelectionReason) => sum + representativeReasonScore(reason), 0),
      };
    })
    .sort((left, right) =>
      right.score - left.score
      || compareRepresentativeTieBreak(left.candidate, right.candidate)
      || left.candidate.filePath.localeCompare(right.candidate.filePath)
      || left.candidate.line - right.candidate.line);

  const selected: { candidate: Symbol; reasons: RepresentativeSelectionReason[]; score: number } = ranked.find((entry) => entry.candidate.filePath === symbol.filePath && entry.candidate.line === symbol.line)
    ?? ranked[0]
    ?? {
      candidate: symbol,
      reasons: deriveRepresentativeSelectionReasons(symbol),
      score: 0,
    };

  const topScore = ranked.length > 0 ? ranked[0].score : selected.score;
  const alternateCanonicalCandidateCount = ranked.filter((entry) =>
    entry.score === topScore
    && !(entry.candidate.filePath === selected.candidate.filePath && entry.candidate.line === selected.candidate.line)).length;

  const selectedReasons: RepresentativeSelectionReason[] = [...selected.reasons];
  if (
    alternateCanonicalCandidateCount > 0
    && !selectedReasons.includes("duplicateClusterWeakCanonicality")
  ) {
    selectedReasons.push("duplicateClusterWeakCanonicality");
  }

  const representativeConfidence =
    alternateCanonicalCandidateCount > 0
      ? "weak"
      : selectedReasons.includes("outOfLineDefinitionPreferred")
        || selectedReasons.includes("runtimeArtifactPreferred")
          ? "canonical"
          : "acceptable";

  return {
    representativeConfidence,
    representativeSelectionReasons: selectedReasons,
    ...(alternateCanonicalCandidateCount > 0 ? { alternateCanonicalCandidateCount } : {}),
  };
}

function dedupeRepresentativeCandidates(candidates: Symbol[]): Symbol[] {
  const seen = new Set<string>();
  const deduped: Symbol[] = [];
  for (const candidate of candidates) {
    const key = `${candidate.id}@${candidate.filePath}:${candidate.line}:${candidate.symbolRole ?? ""}`;
    if (seen.has(key)) continue;
    seen.add(key);
    deduped.push(candidate);
  }
  return deduped;
}

function deriveRepresentativeSelectionReasons(symbol: Symbol): RepresentativeSelectionReason[] {
  const reasons: RepresentativeSelectionReason[] = [];

  if (symbol.symbolRole === "definition" && !looksLikeHeaderPath(symbol.filePath)) {
    reasons.push("outOfLineDefinitionPreferred");
  } else if (
    symbol.symbolRole === "definition"
    || symbol.symbolRole === "inline_definition"
    || (!!symbol.definitionFilePath && looksLikeHeaderPath(symbol.filePath))
  ) {
    reasons.push("inlineDefinitionFallback");
  } else {
    reasons.push("declarationOnlyFallback");
  }

  if (symbol.artifactKind === "runtime") {
    reasons.push("runtimeArtifactPreferred");
  }
  if (symbol.headerRole === "public") {
    reasons.push("publicHeaderPreferred");
  }
  if (!isTestLikePath(symbol.filePath, symbol.artifactKind)) {
    reasons.push("nonTestPathPreferred");
  }
  if (!isGeneratedLikePath(symbol.filePath, symbol.artifactKind)) {
    reasons.push("nonGeneratedPathPreferred");
  }

  return reasons;
}

function representativeReasonScore(reason: RepresentativeSelectionReason): number {
  switch (reason) {
    case "outOfLineDefinitionPreferred":
      return 400;
    case "inlineDefinitionFallback":
      return 280;
    case "declarationOnlyFallback":
      return 160;
    case "runtimeArtifactPreferred":
      return 25;
    case "publicHeaderPreferred":
      return 20;
    case "nonTestPathPreferred":
      return 10;
    case "nonGeneratedPathPreferred":
      return 10;
    case "scopeCanonicalityPreferred":
      return 10;
    case "duplicateClusterWeakCanonicality":
      return -20;
  }
}

function compareRepresentativeTieBreak(left: Symbol, right: Symbol): number {
  return compareNumbers(roleTieBreakPriority(right), roleTieBreakPriority(left))
    || compareNumbers(implementationPathPriority(right.filePath), implementationPathPriority(left.filePath))
    || compareNumbers(dualLocationPriority(right), dualLocationPriority(left))
    || compareNumbers(left.line, right.line);
}

function compareNumbers(left: number, right: number): number {
  return left - right;
}

function roleTieBreakPriority(symbol: Symbol): number {
  if (symbol.symbolRole === "definition" && !looksLikeHeaderPath(symbol.filePath)) return 4;
  if (symbol.symbolRole === "definition") return 3;
  if (symbol.symbolRole === "inline_definition") return 2;
  if (symbol.symbolRole === "declaration") return 1;
  return 0;
}

function implementationPathPriority(filePath: string): number {
  if (looksLikeImplementationPath(filePath)) return 2;
  if (looksLikeHeaderPath(filePath)) return 1;
  return 0;
}

function dualLocationPriority(symbol: Symbol): number {
  if (symbol.declarationFilePath && symbol.definitionFilePath) return 2;
  if (!symbol.declarationFilePath && symbol.definitionFilePath) return 1;
  return 0;
}

function looksLikeHeaderPath(filePath: string): boolean {
  const lower = filePath.toLowerCase();
  return lower.endsWith(".h")
    || lower.endsWith(".hh")
    || lower.endsWith(".hpp")
    || lower.endsWith(".hxx")
    || lower.endsWith(".inl")
    || lower.endsWith(".inc");
}

function looksLikeImplementationPath(filePath: string): boolean {
  const lower = filePath.toLowerCase();
  return lower.endsWith(".c")
    || lower.endsWith(".cc")
    || lower.endsWith(".cpp")
    || lower.endsWith(".cxx")
    || lower.endsWith(".m")
    || lower.endsWith(".mm");
}

function isTestLikePath(filePath: string, artifactKind?: Symbol["artifactKind"]): boolean {
  if (artifactKind === "test") return true;
  const lower = filePath.toLowerCase();
  return lower.includes("/test/")
    || lower.includes("/tests/")
    || lower.includes("/spec/")
    || lower.includes("/specs/")
    || lower.includes("/sample/")
    || lower.includes("/samples/")
    || lower.includes("/benchmark/")
    || lower.includes("/benchmarks/")
    || lower.includes("_test.")
    || lower.includes(".test.");
}

function isGeneratedLikePath(filePath: string, artifactKind?: Symbol["artifactKind"]): boolean {
  if (artifactKind === "generated") return true;
  const lower = filePath.toLowerCase();
  return lower.includes("/generated/")
    || lower.includes("/gen/")
    || lower.includes("/autogen/");
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

export function rankHeuristicCandidates(symbols: Symbol[], context?: HeuristicLookupContext): Symbol[] {
  return symbols.slice().sort((left, right) =>
    heuristicContextScore(right, context) - heuristicContextScore(left, context)
    || representativeTieBreakScore(right) - representativeTieBreakScore(left)
    || left.filePath.localeCompare(right.filePath)
    || left.line - right.line
    || left.qualifiedName.localeCompare(right.qualifiedName));
}

function heuristicContextScore(symbol: Symbol, context?: HeuristicLookupContext): number {
  if (!context) {
    return baseHeuristicScore(symbol);
  }

  let score = baseHeuristicScore(symbol);
  if (context.language && symbol.language === context.language) score += 80;
  if (context.subsystem && symbol.subsystem === context.subsystem) score += 120;
  if (context.module && symbol.module === context.module) score += 110;
  if (context.projectArea && symbol.projectArea === context.projectArea) score += 90;
  if (context.artifactKind && symbol.artifactKind === context.artifactKind) score += 140;
  if (context.filePath) score += filePathNeighborhoodScore(symbol.filePath, context.filePath);
  if (context.anchorNeighborSymbolIds?.includes(symbol.id)) score += 220;
  if (context.anchorScopePrefixes?.some((prefix) => symbol.qualifiedName === prefix || symbol.qualifiedName.startsWith(`${prefix}::`))) score += 70;

  return score;
}

function baseHeuristicScore(symbol: Symbol): number {
  let score = 0;
  if (symbol.artifactKind === "runtime") score += 20;
  if (symbol.artifactKind === "editor") score += 10;
  if (!isTestLikePath(symbol.filePath, symbol.artifactKind)) score += 8;
  if (!isGeneratedLikePath(symbol.filePath, symbol.artifactKind)) score += 8;
  if (symbol.type === "function") score += 8;
  if (looksLikeImplementationPath(symbol.filePath)) score += 4;
  score += scopeDepthPreference(symbol.qualifiedName);
  return score;
}

function scopeDepthPreference(qualifiedName: string): number {
  const scopeDepth = qualifiedName.split("::").length - 1;
  if (scopeDepth <= 1) return 6;
  if (scopeDepth === 2) return 2;
  return 0;
}

function representativeTieBreakScore(symbol: Symbol): number {
  return representativeReasonScore(deriveRepresentativeSelectionReasons(symbol)[0] ?? "declarationOnlyFallback");
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
