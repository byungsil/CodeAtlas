import request from "supertest";
import { createApp } from "../app";
import { SqliteStore } from "../storage/sqlite-store";
import * as path from "path";
import { Store } from "../storage/store";
import { Symbol } from "../models/symbol";

const DB_PATH = path.resolve(__dirname, "../../../samples/.codeatlas/index.db");
let store: SqliteStore;
let app: ReturnType<typeof createApp>;

beforeAll(() => {
  store = new SqliteStore(DB_PATH);
  app = createApp(store);
});

afterAll(() => {
  store.close();
});

describe("GET /symbol", () => {
  it("returns exact callable lookup by id", async () => {
    const res = await request(app)
      .get("/symbol")
      .query({ id: "Game::AIComponent::UpdateAI" })
      .expect(200);

    expect(res.body.lookupMode).toBe("exact");
    expect(res.body.confidence).toBe("exact");
    expect(res.body.matchReasons).toEqual(["exact_id_match"]);
    expect(["canonical", "acceptable", "weak"]).toContain(res.body.representativeConfidence);
    expect(Array.isArray(res.body.representativeSelectionReasons)).toBe(true);
    expect(res.body.symbol.qualifiedName).toBe("Game::AIComponent::UpdateAI");
    expect(res.body.callers).toBeInstanceOf(Array);
    expect(res.body.callees).toBeInstanceOf(Array);
  });

  it("returns exact class lookup by qualifiedName", async () => {
    const res = await request(app)
      .get("/symbol")
      .query({ qualifiedName: "Game::GameObject" })
      .expect(200);

    expect(res.body.lookupMode).toBe("exact");
    expect(res.body.confidence).toBe("exact");
    expect(res.body.matchReasons).toEqual(["exact_qualified_name_match"]);
    expect(["canonical", "acceptable", "weak"]).toContain(res.body.representativeConfidence);
    expect(Array.isArray(res.body.representativeSelectionReasons)).toBe(true);
    expect(res.body.symbol.qualifiedName).toBe("Game::GameObject");
    expect(res.body.members).toBeInstanceOf(Array);
    expect(res.body.members.length).toBeGreaterThan(0);
  });

  it("returns both exact reasons when id and qualifiedName match", async () => {
    const res = await request(app)
      .get("/symbol")
      .query({ id: "Game::AIComponent::UpdateAI", qualifiedName: "Game::AIComponent::UpdateAI" })
      .expect(200);

    expect(res.body.lookupMode).toBe("exact");
    expect(res.body.confidence).toBe("exact");
    expect(res.body.matchReasons).toEqual(["exact_id_match", "exact_qualified_name_match"]);
  });

  it("returns 400 when id and qualifiedName target different symbols", async () => {
    const res = await request(app)
      .get("/symbol")
      .query({ id: "Game::AIComponent::UpdateAI", qualifiedName: "Game::GameObject" })
      .expect(400);

    expect(res.body.code).toBe("BAD_REQUEST");
    expect(res.body.error).toBe("Invalid exact lookup request");
    expect(res.body.symbol).toBeUndefined();
  });

  it("returns 400 when no exact lookup argument is supplied", async () => {
    const res = await request(app).get("/symbol").expect(400);
    expect(res.body.code).toBe("BAD_REQUEST");
    expect(res.body.error).toBe("Invalid exact lookup request");
  });

  it("returns 404 for unknown exact symbol", async () => {
    const res = await request(app)
      .get("/symbol")
      .query({ id: "Game::DoesNotExist" })
      .expect(404);

    expect(res.body.code).toBe("NOT_FOUND");
    expect(res.body.error).toBe("Symbol not found");
    expect(res.body.confidence).toBeUndefined();
  });
});

describe("GET /function/:name", () => {
  it("returns symbol with callers and callees", async () => {
    const res = await request(app).get("/function/UpdateAI").expect(200);
    expect(res.body.symbol).toBeDefined();
    expect(res.body.symbol.name).toBe("UpdateAI");
    expect(res.body.symbol.type).toBe("method");
    expect(res.body.lookupMode).toBe("heuristic");
    expect(res.body.confidence).toBe("high_confidence_heuristic");
    expect(res.body.matchReasons).toEqual([]);
    expect(res.body.callers).toBeInstanceOf(Array);
    expect(res.body.callees).toBeInstanceOf(Array);
    expect(res.body.callees.length).toBeGreaterThan(0);
    for (const ref of [...res.body.callers, ...res.body.callees]) {
      expect(ref.confidence).toBe("high_confidence_heuristic");
      expect(ref.matchReasons).toEqual([]);
    }
  });

  it("returns 404 for unknown symbol", async () => {
    const res = await request(app).get("/function/NonExistentXYZ").expect(404);
    expect(res.body.code).toBe("NOT_FOUND");
    expect(res.body.error).toBe("Symbol not found");
    expect(res.body.symbol).toBeUndefined();
    expect(res.body.confidence).toBeUndefined();
  });
});

describe("GET /class/:name", () => {
  it("returns class with members", async () => {
    const res = await request(app).get("/class/GameObject").expect(200);
    expect(res.body.symbol).toBeDefined();
    expect(res.body.symbol.name).toBe("GameObject");
    expect(res.body.symbol.type).toBe("class");
    expect(res.body.lookupMode).toBe("heuristic");
    expect(res.body.confidence).toBe("high_confidence_heuristic");
    expect(res.body.matchReasons).toEqual([]);
    expect(res.body.members).toBeInstanceOf(Array);
    expect(res.body.members.length).toBeGreaterThan(0);
  });

  it("returns 404 for unknown class", async () => {
    const res = await request(app).get("/class/FakeClass").expect(404);
    expect(res.body.code).toBe("NOT_FOUND");
    expect(res.body.error).toBe("Symbol not found");
    expect(res.body.symbol).toBeUndefined();
    expect(res.body.confidence).toBeUndefined();
  });
});

describe("Heuristic ambiguity metadata", () => {
  it("marks duplicate legacy function lookup as ambiguous", async () => {
    const makeSymbol = (id: string, filePath: string): Symbol => ({
      id,
      name: "Tick",
      qualifiedName: id,
      language: "cpp",
      type: "method",
      filePath,
      line: 1,
      endLine: 3,
      parentId: filePath,
    });

    const symbols = [
      makeSymbol("Gameplay::Actor::Tick", "src/gameplay_actor.h"),
      makeSymbol("UI::Widget::Tick", "src/ui_widget.h"),
    ];

    const ambiguousStore: Store = {
      getSymbolsByName(name: string) {
        return name === "Tick" ? symbols : [];
      },
      getSymbolById(id: string) {
        return symbols.find((symbol) => symbol.id === id);
      },
      getSymbolsByIds(ids: string[]) {
        return symbols.filter((symbol) => ids.includes(symbol.id));
      },
      getRepresentativeCandidates(symbolId: string) {
        return symbols.filter((symbol) => symbol.id === symbolId);
      },
      getSymbolByQualifiedName(qualifiedName: string) {
        return symbols.find((symbol) => symbol.qualifiedName === qualifiedName);
      },
      searchSymbols() {
        return { results: [], totalCount: 0 };
      },
      getFileSymbols() {
        return [];
      },
      getNamespaceSymbols() {
        return [];
      },
      getCallers() {
        return [];
      },
      getCallees() {
        return [];
      },
      getReferences() {
        return [];
      },
      getMembers() {
        return [];
      },
      getDirectBases() {
        return [];
      },
      getDirectDerived() {
        return [];
      },
      getBaseMethods() {
        return [];
      },
      getOverrides() {
        return [];
      },
      getIncomingPropagation() {
        return [];
      },
      getOutgoingPropagation() {
        return [];
      },
      getWorkspaceLanguageSummary() {
        return [];
      },
    };

    const ambiguousApp = createApp(ambiguousStore);
    const res = await request(ambiguousApp).get("/function/Tick").expect(200);
    expect(res.body.symbol.qualifiedName).toBe("Gameplay::Actor::Tick");
    expect(res.body.lookupMode).toBe("heuristic");
    expect(res.body.confidence).toBe("ambiguous");
    expect(res.body.matchReasons).toEqual(["ambiguous_top_score"]);
    expect(res.body.ambiguity).toEqual({ candidateCount: 2 });
  });
});

describe("GET /search", () => {
  it("returns matching symbols", async () => {
    const res = await request(app).get("/search?q=Update").expect(200);
    expect(res.body.query).toBe("Update");
    expect(res.body.window.totalCount).toBe(res.body.totalCount);
    expect(res.body.window.returnedCount).toBe(res.body.results.length);
    expect(res.body.results).toBeInstanceOf(Array);
    expect(res.body.results.length).toBe(3);
    expect(res.body.totalCount).toBe(3);
    expect(res.body.truncated).toBe(false);
  });

  it("returns 400 when q is missing", async () => {
    const res = await request(app).get("/search").expect(400);
    expect(res.body.code).toBe("BAD_REQUEST");
  });

  it("returns empty for short query guardrail", async () => {
    const res = await request(app).get("/search?q=a").expect(200);
    expect(res.body.results).toEqual([]);
    expect(res.body.totalCount).toBe(0);
    expect(res.body.window.returnedCount).toBe(0);
  });

  it("returns empty for two-char query", async () => {
    const res = await request(app).get("/search?q=Ga").expect(200);
    expect(res.body.results).toEqual([]);
    expect(res.body.totalCount).toBe(0);
    expect(res.body.window.returnedCount).toBe(0);
  });

  it("respects limit parameter", async () => {
    const res = await request(app).get("/search?q=Game&limit=2").expect(200);
    expect(res.body.results.length).toBeLessThanOrEqual(2);
  });

  it("marks truncated when more results exist", async () => {
    const res = await request(app).get("/search?q=Game&limit=1").expect(200);
    expect(res.body.truncated).toBe(true);
    expect(res.body.window.truncated).toBe(true);
  });
});

describe("GET /callgraph/:name", () => {
  it("returns call graph with callees", async () => {
    const res = await request(app).get("/callgraph/UpdateAI").expect(200);
    expect(res.body.root).toBeDefined();
    expect(res.body.root.symbol.name).toBe("UpdateAI");
    expect(res.body.root.callees).toBeInstanceOf(Array);
    expect(res.body.root.callees.length).toBe(4);
    expect(res.body.depth).toBeGreaterThanOrEqual(1);
    expect(res.body.truncated).toBe(false);
  });

  it("returns 404 for unknown symbol", async () => {
    const res = await request(app).get("/callgraph/FakeFunc").expect(404);
    expect(res.body.code).toBe("NOT_FOUND");
    expect(res.body.error).toBe("Symbol not found");
    expect(res.body.root).toBeUndefined();
  });
});

describe("GET /callers/:name", () => {
  it("returns deduplicated direct callers", async () => {
    const res = await request(app).get("/callers/UpdateAI").expect(200);
    expect(res.body.symbol).toBeDefined();
    expect(res.body.symbol.name).toBe("UpdateAI");
    expect(res.body.callers).toBeInstanceOf(Array);
    expect(res.body.totalCount).toBe(res.body.callers.length);
    expect(res.body.truncated).toBe(false);
    expect(res.body.window.totalCount).toBe(res.body.totalCount);
    expect(res.body.window.returnedCount).toBe(res.body.callers.length);

    const qualifiedNames = res.body.callers.map((ref: any) => ref.qualifiedName);
    expect(qualifiedNames).toEqual([...qualifiedNames].sort((a, b) => a.localeCompare(b)));
    expect(new Set(res.body.callers.map((ref: any) => ref.symbolId)).size).toBe(res.body.callers.length);
  });

  it("returns 404 for unknown symbol", async () => {
    const res = await request(app).get("/callers/FakeFunc").expect(404);
    expect(res.body.code).toBe("NOT_FOUND");
    expect(res.body.error).toBe("Symbol not found");
    expect(res.body.symbol).toBeUndefined();
  });
});

describe("Overview queries", () => {
  it("returns file overview in stable order", async () => {
    const res = await request(app)
      .get("/file-symbols")
      .query({ filePath: "src/game_object.h" })
      .expect(200);
    expect(res.body.filePath).toBe("src/game_object.h");
    expect(res.body.summary.totalCount).toBe(res.body.symbols.length);
    expect(res.body.window.totalCount).toBe(res.body.summary.totalCount);
    expect(res.body.window.returnedCount).toBe(res.body.symbols.length);
    expect(res.body.symbols.length).toBeGreaterThan(0);
  });

  it("returns class member overview for exact class", async () => {
    const res = await request(app)
      .get("/class-members")
      .query({ qualifiedName: "Game::GameObject" })
      .expect(200);
    expect(res.body.lookupMode).toBe("exact");
    expect(res.body.symbol.qualifiedName).toBe("Game::GameObject");
    expect(res.body.summary.totalCount).toBe(res.body.members.length);
    expect(res.body.window.totalCount).toBe(res.body.summary.totalCount);
    expect(res.body.window.returnedCount).toBe(res.body.members.length);
    expect(res.body.members.length).toBeGreaterThan(0);
  });
});

describe("Response structure consistency", () => {
  it("all symbol filePaths are workspace-relative", async () => {
    const res = await request(app).get("/search?q=Game").expect(200);
    for (const sym of res.body.results) {
      expect(sym.filePath).not.toMatch(/^[A-Z]:/i);
      expect(sym.filePath).not.toMatch(/^\//);
    }
  });
});
