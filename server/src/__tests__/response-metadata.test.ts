import {
  buildClassResponse,
  buildExactLookupResponse,
  buildFunctionResponse,
  deriveLegacyLookupMetadata,
  makeRecoveredCallReference,
  makeResolvedCallReference,
  rankHeuristicCandidatesDetailed,
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
    const ranked = rankHeuristicCandidatesDetailed([
      makeSymbol({ id: "Game::Runtime::Update", qualifiedName: "Game::Runtime::Update", filePath: "runtime/update.cpp", artifactKind: "runtime" }),
      makeSymbol({ id: "Game::Editor::Update", qualifiedName: "Game::Editor::Update", filePath: "editor/update.cpp", artifactKind: "editor" }),
      makeSymbol({ id: "Game::Tests::Update", qualifiedName: "Game::Tests::Update", filePath: "tests/update.cpp", artifactKind: "test" }),
    ]);

    expect(deriveLegacyLookupMetadata(3, ranked)).toEqual({
      lookupMode: "heuristic",
      confidence: "ambiguous",
      matchReasons: ["ambiguous_top_score"],
      ambiguity: { candidateCount: 3 },
      selectedReason: "Preferred runtime candidate by default heuristic ranking.",
      bestNextDiscriminator: "Use artifactKind filtering to narrow these candidates.",
      suggestedExactQueries: [
        "lookup_symbol qualifiedName=Game::Runtime::Update",
        "lookup_symbol qualifiedName=Game::Editor::Update",
        "lookup_symbol qualifiedName=Game::Tests::Update",
      ],
      topCandidates: [
        {
          id: "Game::Runtime::Update",
          qualifiedName: "Game::Runtime::Update",
          filePath: "runtime/update.cpp",
          line: 10,
          ownerQualifiedName: "Game::GameObject",
          artifactKind: "runtime",
          exactQuery: "lookup_symbol qualifiedName=Game::Runtime::Update",
          discriminator: "owner:Game::GameObject",
          rankScore: 42,
        },
        {
          id: "Game::Editor::Update",
          qualifiedName: "Game::Editor::Update",
          filePath: "editor/update.cpp",
          line: 10,
          ownerQualifiedName: "Game::GameObject",
          artifactKind: "editor",
          exactQuery: "lookup_symbol qualifiedName=Game::Editor::Update",
          discriminator: "owner:Game::GameObject",
          rankScore: 32,
        },
        {
          id: "Game::Tests::Update",
          qualifiedName: "Game::Tests::Update",
          filePath: "tests/update.cpp",
          line: 10,
          ownerQualifiedName: "Game::GameObject",
          artifactKind: "test",
          exactQuery: "lookup_symbol qualifiedName=Game::Tests::Update",
          discriminator: "owner:Game::GameObject",
          rankScore: 14,
        },
      ],
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
    expect(ref.resolutionKind).toBe("resolved");
    expect(ref.provenanceKind).toBe("resolved_call_edge");
  });

  it("builds recovered call references for fallback evidence", () => {
    const ref = makeRecoveredCallReference({
      symbol: makeSymbol({ id: "Game::AIComponent::UpdateAI", name: "UpdateAI", qualifiedName: "Game::AIComponent::UpdateAI" }),
      filePath: "src/game_loop.cpp",
      line: 42,
    });

    expect(ref.confidence).toBe("ambiguous");
    expect(ref.matchReasons).toEqual([]);
    expect(ref.resolutionKind).toBe("recovered");
    expect(ref.provenanceKind).toBe("raw_call");
  });

  it("propagates ambiguity metadata through function response", () => {
    const ranked = rankHeuristicCandidatesDetailed([
      makeSymbol({ id: "Game::Runtime::Update", qualifiedName: "Game::Runtime::Update", filePath: "runtime/update.cpp", artifactKind: "runtime" }),
      makeSymbol({ id: "Game::Editor::Update", qualifiedName: "Game::Editor::Update", filePath: "editor/update.cpp", artifactKind: "editor" }),
    ]);
    const response = buildFunctionResponse({
      symbol: ranked[0].symbol,
      candidateCount: 2,
      rankedCandidates: ranked,
      callers: [],
      callees: [],
    });

    expect(response.confidence).toBe("ambiguous");
    expect(response.matchReasons).toEqual(["ambiguous_top_score"]);
    expect(response.ambiguity).toEqual({ candidateCount: 2 });
    expect(response.selectedReason).toBe("Preferred runtime candidate by default heuristic ranking.");
    expect(response.bestNextDiscriminator).toBe("Use artifactKind filtering to narrow these candidates.");
    expect(response.suggestedExactQueries).toEqual([
      "lookup_symbol qualifiedName=Game::Runtime::Update",
      "lookup_symbol qualifiedName=Game::Editor::Update",
    ]);
    expect(response.topCandidates?.map((candidate) => candidate.qualifiedName)).toEqual([
      "Game::Runtime::Update",
      "Game::Editor::Update",
    ]);
    expect(response.topCandidates?.[0].exactQuery).toBe("lookup_symbol qualifiedName=Game::Runtime::Update");
  });

  it("propagates heuristic confidence through class response", () => {
    const ranked = rankHeuristicCandidatesDetailed([
      makeSymbol({
        id: "Game::Runtime::GameObject",
        name: "GameObject",
        qualifiedName: "Game::Runtime::GameObject",
        type: "class",
        filePath: "runtime/game_object.h",
        artifactKind: "runtime",
      }),
    ]);
    const response = buildClassResponse({
      symbol: ranked[0].symbol,
      candidateCount: 1,
      rankedCandidates: ranked,
      members: [],
    });

    expect(response.lookupMode).toBe("heuristic");
    expect(response.confidence).toBe("high_confidence_heuristic");
    expect(response.matchReasons).toEqual([]);
    expect(response.selectedReason).toBe("Preferred runtime candidate by default heuristic ranking.");
    expect(response.topCandidates).toBeUndefined();
  });
});
