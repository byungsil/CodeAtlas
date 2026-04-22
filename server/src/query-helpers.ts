import { Symbol } from "./models/symbol";
import { MetadataFilters } from "./storage/store";
import { MatchReason, MetadataGroupSummary } from "./models/responses";
import { AUTO_COMPACT_THRESHOLD } from "./constants";

export function shouldCompact(compact: boolean | undefined, resultCount: number): boolean {
  if (compact === true) return true;
  if (compact === false) return false;
  return resultCount >= AUTO_COMPACT_THRESHOLD;
}

export function metadataFilterEcho(filters?: MetadataFilters): Partial<MetadataFilters> {
  return filters ?? {};
}

export function collectAnchorScopePrefixes(qualifiedName: string): string[] {
  const parts = qualifiedName.split("::");
  const prefixes: string[] = [];
  for (let index = 1; index < parts.length; index += 1) {
    prefixes.push(parts.slice(0, index).join("::"));
  }
  return prefixes;
}

export function normalizeScopeValue(value?: string): string | undefined {
  return value?.trim().replace(/\s+/g, "");
}

export function qualifierMatchesScope(rawQualifier: string | undefined, scopeValue: string | undefined): boolean {
  const normalizedQualifier = normalizeScopeValue(rawQualifier);
  const normalizedScope = normalizeScopeValue(scopeValue);
  if (!normalizedQualifier || !normalizedScope) {
    return false;
  }
  return normalizedQualifier === normalizedScope
    || normalizedScope.endsWith(`::${normalizedQualifier}`)
    || normalizedQualifier.endsWith(`::${normalizedScope}`);
}

export function normalizeNameTokens(value?: string): string | undefined {
  if (!value) {
    return undefined;
  }
  return value
    .replace(/^[mp]_?/i, "")
    .replace(/[^a-zA-Z0-9]+/g, "")
    .toLowerCase();
}

export function receiverMatchesParentName(receiver: string | undefined, parentSymbol: Symbol | undefined): boolean {
  const normalizedReceiver = normalizeNameTokens(receiver);
  const parentName = parentSymbol?.name ?? parentSymbol?.qualifiedName.split("::").pop();
  const normalizedParent = normalizeNameTokens(parentName);
  if (!normalizedReceiver || !normalizedParent) {
    return false;
  }
  return normalizedReceiver === normalizedParent
    || normalizedReceiver.endsWith(normalizedParent)
    || normalizedParent.endsWith(normalizedReceiver);
}

export function callableNamespace(symbol: Symbol): string | undefined {
  if (symbol.scopeKind === "namespace" && symbol.scopeQualifiedName) {
    return symbol.scopeQualifiedName;
  }
  if (!symbol.qualifiedName.includes("::")) {
    return undefined;
  }
  return symbol.qualifiedName.split("::").slice(0, -1).join("::");
}

export function candidateLocationPaths(symbol: Symbol): string[] {
  return Array.from(new Set([
    symbol.filePath,
    symbol.definitionFilePath,
    symbol.declarationFilePath,
  ].filter((value): value is string => typeof value === "string" && value.length > 0)));
}

export function directoryPath(filePath: string): string | undefined {
  const normalized = filePath.replace(/\\/g, "/");
  const lastSlash = normalized.lastIndexOf("/");
  if (lastSlash <= 0) {
    return undefined;
  }
  return normalized.slice(0, lastSlash);
}

export function fileStem(filePath: string): string {
  const normalized = filePath.replace(/\\/g, "/");
  const leaf = normalized.split("/").pop() ?? normalized;
  return leaf.replace(/\.[^.]+$/, "");
}

export function fileAffinityScore(rawFilePath: string, candidate: Symbol): { score: number; matchReasons: MatchReason[] } {
  const candidatePaths = candidateLocationPaths(candidate);
  const matchReasons: MatchReason[] = [];
  let score = 0;

  if (candidatePaths.includes(rawFilePath)) {
    return {
      score: 240,
      matchReasons: ["same_file_match"],
    };
  }

  const rawDirectory = directoryPath(rawFilePath);
  if (rawDirectory && candidatePaths.some((candidatePath) => directoryPath(candidatePath) === rawDirectory)) {
    score += 90;
    matchReasons.push("same_directory_match");
  }

  const rawStem = fileStem(rawFilePath);
  if (candidatePaths.some((candidatePath) => fileStem(candidatePath) === rawStem)) {
    score += 45;
    matchReasons.push("same_file_stem_match");
  }

  return { score, matchReasons };
}

export function matchesMetadataFilters(symbol: Symbol | undefined, filters?: MetadataFilters): boolean {
  if (!filters) return true;
  if (!symbol) return false;
  if (filters.language && symbol.language !== filters.language) return false;
  if (filters.subsystem && symbol.subsystem !== filters.subsystem) return false;
  if (filters.module && symbol.module !== filters.module) return false;
  if (filters.projectArea && symbol.projectArea !== filters.projectArea) return false;
  if (filters.artifactKind && symbol.artifactKind !== filters.artifactKind) return false;
  return true;
}

export function isFragileCoverageSymbol(symbol: Symbol): boolean {
  return symbol.parseFragility === "elevated" || symbol.macroSensitivity === "high";
}

export function applyLimit<T>(items: T[], limit: number): { results: T[]; totalCount: number; truncated: boolean } {
  return {
    results: items.slice(0, limit),
    totalCount: items.length,
    truncated: items.length > limit,
  };
}
