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
  zeroResultLabel?: string;
}

export function buildResponseReliability(params: ReliabilityParams): ReliabilityMetadata {
  const { symbol, relatedResultCount, zeroResultLabel } = params;
  const factors = collectReliabilityFactors(symbol);
  const indexCoverage = deriveIndexCoverage(factors, relatedResultCount);
  const reliability = deriveReliabilitySummary(factors, indexCoverage);

  return {
    reliability,
    ...(indexCoverage ? { indexCoverage } : {}),
    ...(indexCoverage === "low" && zeroResultLabel
      ? {
        coverageWarning: `This symbol has elevated structural fragility. Zero ${zeroResultLabel} may reflect an index gap rather than a true absence.`,
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
): IndexCoverageLevel | undefined {
  if (relatedResultCount === undefined) {
    return undefined;
  }
  if (factors.length === 0) {
    return "full";
  }
  if (relatedResultCount === 0) {
    return "low";
  }
  return "partial";
}

function deriveReliabilitySummary(
  factors: ReliabilityFactor[],
  indexCoverage?: IndexCoverageLevel,
): ReliabilitySummary {
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
