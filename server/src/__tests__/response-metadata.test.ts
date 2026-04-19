import {
  buildClassResponse,
  buildExactLookupResponse,
  buildFunctionResponse,
  deriveLegacyLookupMetadata,
  makeResolvedCallReference,
} from "../response-metadata";
import { Symbol } from "../models/symbol";

function makeSymbol(overrides: Partial<Symbol> = {}): Symbol {
  return {
    id: "Game::GameObject::Update",
    name: "Update",
    qualifiedName: "Game::GameObject::Update",
    language: "cpp",
    type: "method",
    filePath: "src/game_object.h",
    line: 10,
    endLine: 20,
    parentId: "Game::GameObject",
    ...overrides,
  };
}

describe("response confidence metadata", () => {
  it("derives heuristic confidence for unique legacy lookup", () => {
    expect(deriveLegacyLookupMetadata(1)).toEqual({
      lookupMode: "heuristic",
      confidence: "high_confidence_heuristic",
      matchReasons: [],
    });
  });

  it("derives ambiguity metadata for duplicate legacy lookup", () => {
    expect(deriveLegacyLookupMetadata(3)).toEqual({
      lookupMode: "heuristic",
      confidence: "ambiguous",
      matchReasons: ["ambiguous_top_score"],
      ambiguity: { candidateCount: 3 },
    });
  });

  it("builds exact lookup response with exact confidence for id match", () => {
    const response = buildExactLookupResponse({
      symbol: makeSymbol(),
      matchedBy: "id",
    });

    expect(response.lookupMode).toBe("exact");
    expect(response.confidence).toBe("exact");
    expect(response.matchReasons).toEqual(["exact_id_match"]);
  });

  it("builds exact lookup response with both exact reasons when both aliases are supplied", () => {
    const response = buildExactLookupResponse({
      symbol: makeSymbol(),
      matchedBy: "both",
    });

    expect(response.lookupMode).toBe("exact");
    expect(response.confidence).toBe("exact");
    expect(response.matchReasons).toEqual(["exact_id_match", "exact_qualified_name_match"]);
  });

  it("defaults persisted call references to high-confidence heuristic", () => {
    const ref = makeResolvedCallReference({
      symbol: makeSymbol({ id: "Game::AIComponent::UpdateAI", name: "UpdateAI", qualifiedName: "Game::AIComponent::UpdateAI" }),
      filePath: "src/game_loop.cpp",
      line: 42,
    });

    expect(ref.confidence).toBe("high_confidence_heuristic");
    expect(ref.matchReasons).toEqual([]);
    expect(ref.ambiguity).toBeUndefined();
  });

  it("propagates ambiguity metadata through function response", () => {
    const response = buildFunctionResponse({
      symbol: makeSymbol(),
      candidateCount: 2,
      callers: [],
      callees: [],
    });

    expect(response.confidence).toBe("ambiguous");
    expect(response.matchReasons).toEqual(["ambiguous_top_score"]);
    expect(response.ambiguity).toEqual({ candidateCount: 2 });
  });

  it("propagates heuristic confidence through class response", () => {
    const response = buildClassResponse({
      symbol: makeSymbol({ id: "Game::GameObject", name: "GameObject", qualifiedName: "Game::GameObject", type: "class" }),
      candidateCount: 1,
      members: [],
    });

    expect(response.lookupMode).toBe("heuristic");
    expect(response.confidence).toBe("high_confidence_heuristic");
    expect(response.matchReasons).toEqual([]);
  });
});
