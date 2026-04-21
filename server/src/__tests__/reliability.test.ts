import { buildResponseReliability } from "../reliability";
import { Symbol } from "../models/symbol";

function makeFragileSymbol(overrides: Partial<Symbol> = {}): Symbol {
  return {
    id: "Gameplay::Player::SetShotFlags",
    name: "SetShotFlags",
    qualifiedName: "Gameplay::Player::SetShotFlags",
    language: "cpp",
    type: "method",
    filePath: "gameplay/shotnormal.cpp",
    line: 1634,
    endLine: 1640,
    parseFragility: "elevated",
    macroSensitivity: "high",
    ...overrides,
  };
}

describe("buildResponseReliability", () => {
  it("flags fragile zero-result responses as low coverage", () => {
    const reliability = buildResponseReliability({
      symbol: makeFragileSymbol(),
      relatedResultCount: 0,
      zeroResultLabel: "callers",
    });

    expect(reliability.reliability.level).toBe("low");
    expect(reliability.indexCoverage).toBe("low");
    expect(reliability.coverageWarning).toContain("Zero callers");
    expect(reliability.recoveredResultCount).toBeUndefined();
  });

  it("marks recovered fragile results as partial coverage with explicit fallback warning", () => {
    const reliability = buildResponseReliability({
      symbol: makeFragileSymbol(),
      relatedResultCount: 0,
      recoveredResultCount: 2,
      zeroResultLabel: "callers",
    });

    expect(reliability.reliability.level).toBe("partial");
    expect(reliability.indexCoverage).toBe("partial");
    expect(reliability.recoveredResultCount).toBe(2);
    expect(reliability.coverageWarning).toContain("stored raw-call evidence");
    expect(reliability.reliability.suggestion).toContain("Recovered results include lower-confidence fallback evidence");
  });
});
