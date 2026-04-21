import { Symbol } from "./models/symbol";
import {
  IndexCoverageLevel,
  ReliabilityFactor,
  ReliabilityMetadata,
  ReliabilitySummary,
} from "./models/responses";

interface ReliabilityParams {
  symbol: Symbol;
  relatedResultCount?: number;
  recoveredResultCount?: number;
  zeroResultLabel?: string;
}

export function buildResponseReliability(params: ReliabilityParams): ReliabilityMetadata {
  const { symbol, relatedResultCount, recoveredResultCount, zeroResultLabel } = params;
  const factors = collectReliabilityFactors(symbol);
  const indexCoverage = deriveIndexCoverage(factors, relatedResultCount, recoveredResultCount);
  const reliability = deriveReliabilitySummary(factors, indexCoverage, recoveredResultCount);

  return {
    reliability,
    ...(indexCoverage ? { indexCoverage } : {}),
    ...(recoveredResultCount && recoveredResultCount > 0 ? { recoveredResultCount } : {}),
    ...(buildCoverageWarning(indexCoverage, zeroResultLabel, recoveredResultCount)
      ? {
        coverageWarning: buildCoverageWarning(indexCoverage, zeroResultLabel, recoveredResultCount),
      }
      : {}),
  };
}

function collectReliabilityFactors(symbol: Symbol): ReliabilityFactor[] {
  const factors: ReliabilityFactor[] = [];
  if (symbol.parseFragility === "elevated") {
    factors.push("elevated_parse_fragility");
  }
  if (symbol.macroSensitivity === "high") {
    factors.push("macro_sensitive");
  }
  if (symbol.includeHeaviness === "heavy") {
    factors.push("include_heavy");
  }
  return factors;
}

function deriveIndexCoverage(
  factors: ReliabilityFactor[],
  relatedResultCount?: number,
  recoveredResultCount?: number,
): IndexCoverageLevel | undefined {
  if (relatedResultCount === undefined) {
    return undefined;
  }
  if (factors.length === 0) {
    return "full";
  }
  if (relatedResultCount === 0 && (recoveredResultCount ?? 0) > 0) {
    return "partial";
  }
  if (relatedResultCount === 0) {
    return "low";
  }
  return "partial";
}

function deriveReliabilitySummary(
  factors: ReliabilityFactor[],
  indexCoverage?: IndexCoverageLevel,
  recoveredResultCount?: number,
): ReliabilitySummary {
  if ((recoveredResultCount ?? 0) > 0) {
    return {
      level: "partial",
      factors,
      suggestion: "Recovered results include lower-confidence fallback evidence. Treat them as grounded hints rather than fully resolved graph certainty.",
    };
  }

  if (factors.length === 0) {
    return {
      level: "full",
      factors: [],
    };
  }

  if (indexCoverage === "low") {
    return {
      level: "low",
      factors,
      suggestion: "Cross-check this result with file-level inspection before treating the absence as definitive.",
    };
  }

  return {
    level: "partial",
    factors,
    suggestion: "Treat negative results near this symbol with caution because the surrounding structure is more fragile than usual.",
  };
}

function buildCoverageWarning(
  indexCoverage?: IndexCoverageLevel,
  zeroResultLabel?: string,
  recoveredResultCount?: number,
): string | undefined {
  if (!zeroResultLabel) {
    return undefined;
  }
  if ((recoveredResultCount ?? 0) > 0) {
    const recoveredLabel = recoveredResultCount === 1 ? "result" : "results";
    return `Resolved ${zeroResultLabel} were empty, but ${recoveredResultCount} recovered ${recoveredLabel} from stored raw-call evidence are included.`;
  }
  if (indexCoverage === "low") {
    return `This symbol has elevated structural fragility. Zero ${zeroResultLabel} may reflect an index gap rather than a true absence.`;
  }
  return undefined;
}
