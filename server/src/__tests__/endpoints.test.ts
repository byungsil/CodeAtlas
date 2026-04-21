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
    expect(res.body.reliability.level).toBeDefined();
    expect(Array.isArray(res.body.reliability.factors)).toBe(true);
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
    expect(typeof res.body.selectedReason).toBe("string");
    expect(typeof res.body.bestNextDiscriminator).toBe("string");
    expect(res.body.suggestedExactQueries).toContain("lookup_symbol qualifiedName=Gameplay::Actor::Tick");
    expect(res.body.topCandidates).toHaveLength(2);
    expect(res.body.topCandidates[0].qualifiedName).toBe("Gameplay::Actor::Tick");
    expect(typeof res.body.topCandidates[0].rankScore).toBe("number");
    expect(res.body.topCandidates[0].ownerQualifiedName).toBe("src/gameplay_actor.h");
    expect(res.body.topCandidates[0].exactQuery).toBe("lookup_symbol qualifiedName=Gameplay::Actor::Tick");
  });

  it("uses anchor-qualified context to steer ambiguous HTTP function lookup", async () => {
    const makeSymbol = (params: {
      id: string;
      name: string;
      type: Symbol["type"];
      filePath: string;
      artifactKind?: Symbol["artifactKind"];
      subsystem?: string;
      module?: string;
      projectArea?: string;
    }): Symbol => ({
      id: params.id,
      name: params.name,
      qualifiedName: params.id,
      language: "cpp",
      type: params.type,
      filePath: params.filePath,
      line: 1,
      endLine: 3,
      artifactKind: params.artifactKind,
      subsystem: params.subsystem,
      module: params.module,
      projectArea: params.projectArea,
    });

    const symbols = [
      makeSymbol({
        id: "Game::Runtime::UpdateShot",
        name: "UpdateShot",
        type: "function",
        filePath: "runtime/update_shot.cpp",
        artifactKind: "runtime",
        subsystem: "runtime",
        module: "gameplay",
        projectArea: "investigation",
      }),
      makeSymbol({
        id: "Game::Editor::UpdateShot",
        name: "UpdateShot",
        type: "function",
        filePath: "editor/update_shot.cpp",
        artifactKind: "editor",
        subsystem: "editor",
        module: "editor",
        projectArea: "investigation",
      }),
      makeSymbol({
        id: "Game::Investigation::RunHintWorkflow",
        name: "RunHintWorkflow",
        type: "function",
        filePath: "runtime/workflow.cpp",
        artifactKind: "runtime",
        subsystem: "runtime",
        module: "gameplay",
        projectArea: "investigation",
      }),
      makeSymbol({
        id: "Game::Editor::RefreshShotPreview",
        name: "RefreshShotPreview",
        type: "function",
        filePath: "editor/panel.cpp",
        artifactKind: "editor",
        subsystem: "editor",
        module: "editor",
        projectArea: "investigation",
      }),
    ];

    const anchorAwareStore: Store = {
      getSymbolsByName(name: string) {
        return symbols.filter((symbol) => symbol.name === name);
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

    const anchorAwareApp = createApp(anchorAwareStore);

    const runtimeRes = await request(anchorAwareApp)
      .get("/function/UpdateShot")
      .query({ anchorQualifiedName: "Game::Investigation::RunHintWorkflow" })
      .expect(200);
    expect(runtimeRes.body.lookupMode).toBe("heuristic");
    expect(runtimeRes.body.confidence).toBe("ambiguous");
    expect(runtimeRes.body.symbol.qualifiedName).toBe("Game::Runtime::UpdateShot");
    expect(runtimeRes.body.selectedReason).toBe("Matched artifact kind 'runtime'.");
    expect(runtimeRes.body.topCandidates[0].qualifiedName).toBe("Game::Runtime::UpdateShot");

    const editorRes = await request(anchorAwareApp)
      .get("/function/UpdateShot")
      .query({ anchorQualifiedName: "Game::Editor::RefreshShotPreview" })
      .expect(200);
    expect(editorRes.body.lookupMode).toBe("heuristic");
    expect(editorRes.body.confidence).toBe("ambiguous");
    expect(editorRes.body.symbol.qualifiedName).toBe("Game::Editor::UpdateShot");
    expect(editorRes.body.selectedReason).toBe("Matched artifact kind 'editor'.");

    const runtimeRecentRes = await request(anchorAwareApp)
      .get("/function/UpdateShot")
      .query({ recentQualifiedName: "Game::Investigation::RunHintWorkflow" })
      .expect(200);
    expect(runtimeRecentRes.body.lookupMode).toBe("heuristic");
    expect(runtimeRecentRes.body.confidence).toBe("ambiguous");
    expect(runtimeRecentRes.body.symbol.qualifiedName).toBe("Game::Runtime::UpdateShot");
    expect(runtimeRecentRes.body.selectedReason).toBe("Matched artifact kind 'runtime'.");

    const editorRecentRes = await request(anchorAwareApp)
      .get("/function/UpdateShot")
      .query({ recentQualifiedName: "Game::Editor::RefreshShotPreview" })
      .expect(200);
    expect(editorRecentRes.body.lookupMode).toBe("heuristic");
    expect(editorRecentRes.body.confidence).toBe("ambiguous");
    expect(editorRecentRes.body.symbol.qualifiedName).toBe("Game::Editor::UpdateShot");
    expect(editorRecentRes.body.selectedReason).toBe("Matched artifact kind 'editor'.");
  });

  it("uses recent exact symbol context to steer ambiguous HTTP caller lookup", async () => {
    const makeSymbol = (params: {
      id: string;
      name: string;
      type: Symbol["type"];
      filePath: string;
      artifactKind?: Symbol["artifactKind"];
      subsystem?: string;
      module?: string;
      projectArea?: string;
      parentId?: string;
    }): Symbol => ({
      id: params.id,
      name: params.name,
      qualifiedName: params.id,
      language: "cpp",
      type: params.type,
      filePath: params.filePath,
      line: 1,
      endLine: 3,
      artifactKind: params.artifactKind,
      subsystem: params.subsystem,
      module: params.module,
      projectArea: params.projectArea,
      ...(params.parentId ? { parentId: params.parentId } : {}),
    });

    const symbols = [
      makeSymbol({
        id: "Game::Runtime::UpdateShot",
        name: "UpdateShot",
        type: "function",
        filePath: "runtime/update_shot.cpp",
        artifactKind: "runtime",
        subsystem: "runtime",
        module: "gameplay",
        projectArea: "investigation",
      }),
      makeSymbol({
        id: "Game::Editor::UpdateShot",
        name: "UpdateShot",
        type: "function",
        filePath: "editor/update_shot.cpp",
        artifactKind: "editor",
        subsystem: "editor",
        module: "editor",
        projectArea: "investigation",
      }),
      makeSymbol({
        id: "Game::Runtime::TickRuntimeShot",
        name: "TickRuntimeShot",
        type: "function",
        filePath: "runtime/tick_runtime_shot.cpp",
        artifactKind: "runtime",
        subsystem: "runtime",
        module: "gameplay",
        projectArea: "investigation",
      }),
      makeSymbol({
        id: "Game::Editor::RefreshShotPreview",
        name: "RefreshShotPreview",
        type: "function",
        filePath: "editor/panel.cpp",
        artifactKind: "editor",
        subsystem: "editor",
        module: "editor",
        projectArea: "investigation",
      }),
      makeSymbol({
        id: "Game::Investigation::HintController",
        name: "HintController",
        type: "class",
        filePath: "runtime/workflow.h",
        artifactKind: "runtime",
        subsystem: "runtime",
        module: "gameplay",
        projectArea: "investigation",
      }),
      makeSymbol({
        id: "Game::Investigation::HintController::hintedPower",
        name: "hintedPower",
        type: "variable",
        filePath: "runtime/workflow.h",
        artifactKind: "runtime",
        subsystem: "runtime",
        module: "gameplay",
        projectArea: "investigation",
        parentId: "Game::Investigation::HintController",
      }),
    ];

    const calls = [
      { callerId: "Game::Runtime::TickRuntimeShot", calleeId: "Game::Runtime::UpdateShot", filePath: "runtime/tick_runtime_shot.cpp", line: 7 },
      { callerId: "Game::Editor::RefreshShotPreview", calleeId: "Game::Editor::UpdateShot", filePath: "editor/panel.cpp", line: 9 },
    ];

    const recentAwareStore: Store = {
      getSymbolsByName(name: string) {
        return symbols.filter((symbol) => symbol.name === name);
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
      getCallers(symbolId: string) {
        return calls.filter((call) => call.calleeId === symbolId);
      },
      getCallees(symbolId: string) {
        return calls.filter((call) => call.callerId === symbolId);
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

    const recentAwareApp = createApp(recentAwareStore);

    const runtimeRes = await request(recentAwareApp)
      .get("/callers/UpdateShot")
      .query({ recentQualifiedName: "Game::Investigation::HintController::hintedPower", limit: 10 })
      .expect(200);
    expect(runtimeRes.body.lookupMode).toBe("heuristic");
    expect(runtimeRes.body.confidence).toBe("ambiguous");
    expect(runtimeRes.body.symbol.qualifiedName).toBe("Game::Runtime::UpdateShot");
    expect(runtimeRes.body.selectedReason).toBe("Matched artifact kind 'runtime'.");
    expect(runtimeRes.body.callers).toHaveLength(1);
    expect(runtimeRes.body.callers[0].qualifiedName).toBe("Game::Runtime::TickRuntimeShot");

    const editorRes = await request(recentAwareApp)
      .get("/callers/UpdateShot")
      .query({ recentQualifiedName: "Game::Editor::RefreshShotPreview", limit: 10 })
      .expect(200);
    expect(editorRes.body.lookupMode).toBe("heuristic");
    expect(editorRes.body.confidence).toBe("ambiguous");
    expect(editorRes.body.symbol.qualifiedName).toBe("Game::Editor::UpdateShot");
    expect(editorRes.body.selectedReason).toBe("Matched a direct neighbor of the anchor symbol.");
    expect(editorRes.body.callers).toHaveLength(1);
    expect(editorRes.body.callers[0].qualifiedName).toBe("Game::Editor::RefreshShotPreview");
  });

  it("uses recent exact symbol context to steer ambiguous HTTP class lookup", async () => {
    const makeSymbol = (params: {
      id: string;
      name: string;
      type: Symbol["type"];
      filePath: string;
      artifactKind?: Symbol["artifactKind"];
      subsystem?: string;
      module?: string;
      projectArea?: string;
    }): Symbol => ({
      id: params.id,
      name: params.name,
      qualifiedName: params.id,
      language: "cpp",
      type: params.type,
      filePath: params.filePath,
      line: 1,
      endLine: 3,
      artifactKind: params.artifactKind,
      subsystem: params.subsystem,
      module: params.module,
      projectArea: params.projectArea,
    });

    const symbols = [
      makeSymbol({
        id: "Game::Runtime::ShotPanel",
        name: "ShotPanel",
        type: "class",
        filePath: "runtime/update_shot.cpp",
        artifactKind: "runtime",
        subsystem: "runtime",
        module: "gameplay",
        projectArea: "investigation",
      }),
      makeSymbol({
        id: "Game::Editor::ShotPanel",
        name: "ShotPanel",
        type: "class",
        filePath: "editor/update_shot.cpp",
        artifactKind: "editor",
        subsystem: "editor",
        module: "editor",
        projectArea: "investigation",
      }),
      makeSymbol({
        id: "Game::Investigation::RunHintWorkflow",
        name: "RunHintWorkflow",
        type: "function",
        filePath: "runtime/workflow.cpp",
        artifactKind: "runtime",
        subsystem: "runtime",
        module: "gameplay",
        projectArea: "investigation",
      }),
      makeSymbol({
        id: "Game::Editor::RefreshShotPreview",
        name: "RefreshShotPreview",
        type: "function",
        filePath: "editor/panel.cpp",
        artifactKind: "editor",
        subsystem: "editor",
        module: "editor",
        projectArea: "investigation",
      }),
    ];

    const recentAwareStore: Store = {
      getSymbolsByName(name: string) {
        return symbols.filter((symbol) => symbol.name === name);
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

    const recentAwareApp = createApp(recentAwareStore);

    const runtimeRes = await request(recentAwareApp)
      .get("/class/ShotPanel")
      .query({ recentQualifiedName: "Game::Investigation::RunHintWorkflow" })
      .expect(200);
    expect(runtimeRes.body.lookupMode).toBe("heuristic");
    expect(runtimeRes.body.confidence).toBe("ambiguous");
    expect(runtimeRes.body.symbol.qualifiedName).toBe("Game::Runtime::ShotPanel");
    expect(runtimeRes.body.selectedReason).toBe("Matched artifact kind 'runtime'.");

    const editorRes = await request(recentAwareApp)
      .get("/class/ShotPanel")
      .query({ recentQualifiedName: "Game::Editor::RefreshShotPreview" })
      .expect(200);
    expect(editorRes.body.lookupMode).toBe("heuristic");
    expect(editorRes.body.confidence).toBe("ambiguous");
    expect(editorRes.body.symbol.qualifiedName).toBe("Game::Editor::ShotPanel");
    expect(editorRes.body.selectedReason).toBe("Matched artifact kind 'editor'.");
  });

  it("prefers direct workflow neighbors when anchor metadata alone would still tie", async () => {
    const makeSymbol = (id: string, filePath: string): Symbol => ({
      id,
      name: id.endsWith("UpdateShot") ? "UpdateShot" : id.split("::").at(-1)!,
      qualifiedName: id,
      language: "cpp",
      type: "function",
      filePath,
      line: 1,
      endLine: 3,
      artifactKind: "runtime",
      subsystem: "runtime",
      module: "gameplay",
      projectArea: "investigation",
    });

    const symbols = [
      makeSymbol("Game::Runtime::UpdateShot", "runtime/update_shot.cpp"),
      makeSymbol("Game::Runtime::Alt::UpdateShot", "runtime/alt_update_shot.cpp"),
      makeSymbol("Game::Runtime::TickRuntimeShot", "runtime/tick_runtime_shot.cpp"),
    ];

    const calls = [
      { callerId: "Game::Runtime::TickRuntimeShot", calleeId: "Game::Runtime::UpdateShot", filePath: "runtime/tick_runtime_shot.cpp", line: 7 },
    ];

    const neighborAwareStore: Store = {
      getSymbolsByName(name: string) {
        return symbols.filter((symbol) => symbol.name === name);
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
      getCallers(symbolId: string) {
        return calls.filter((call) => call.calleeId === symbolId);
      },
      getCallees(symbolId: string) {
        return calls.filter((call) => call.callerId === symbolId);
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

    const neighborAwareApp = createApp(neighborAwareStore);
    const res = await request(neighborAwareApp)
      .get("/function/UpdateShot")
      .query({ anchorQualifiedName: "Game::Runtime::TickRuntimeShot" })
      .expect(200);
    expect(res.body.lookupMode).toBe("heuristic");
    expect(res.body.confidence).toBe("ambiguous");
    expect(res.body.symbol.qualifiedName).toBe("Game::Runtime::UpdateShot");
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
    expect(res.body.direction).toBe("callees");
    expect(res.body.reliability.level).toBeDefined();
    expect(res.body.depth).toBeGreaterThanOrEqual(1);
    expect(res.body.nodeCount).toBeGreaterThanOrEqual(1);
    expect(res.body.nodeCap).toBeGreaterThanOrEqual(res.body.nodeCount);
    expect(res.body.truncated).toBe(false);
  });

  it("returns call graph with callers when requested", async () => {
    const res = await request(app)
      .get("/callgraph/UpdateAI")
      .query({ direction: "callers", depth: 2 })
      .expect(200);
    expect(res.body.root).toBeDefined();
    expect(res.body.direction).toBe("callers");
    expect(res.body.root.callers).toBeInstanceOf(Array);
    expect(res.body.root.callers.length).toBeGreaterThan(0);
    expect(res.body.root.callees).toEqual([]);
  });

  it("returns bidirectional call graph when requested", async () => {
    const res = await request(app)
      .get("/callgraph/UpdateAI")
      .query({ direction: "both", depth: 2 })
      .expect(200);
    expect(res.body.direction).toBe("both");
    expect(res.body.root.callees).toBeInstanceOf(Array);
    expect(res.body.root.callers).toBeInstanceOf(Array);
  });

  it("returns compact call graph mode when requested", async () => {
    const res = await request(app)
      .get("/callgraph/UpdateAI")
      .query({ compact: "true" })
      .expect(200);
    expect(res.body.responseMode).toBe("compact");
    expect(res.body.root.symbol.qualifiedName).toBeDefined();
    expect(res.body.root.symbol.filePath).toBeDefined();
    expect(res.body.root.symbol.line).toEqual(expect.any(Number));
    expect(res.body.root.symbol.type).toBeUndefined();
  });

  it("marks call graph truncated when node cap is too small", async () => {
    const res = await request(app)
      .get("/callgraph/UpdateAI")
      .query({ direction: "callers", depth: 3, nodeCap: 1 })
      .expect(200);
    expect(res.body.direction).toBe("callers");
    expect(res.body.nodeCap).toBe(1);
    expect(res.body.truncated).toBe(true);
  });

  it("returns 404 for unknown symbol", async () => {
    const res = await request(app).get("/callgraph/FakeFunc").expect(404);
    expect(res.body.code).toBe("NOT_FOUND");
    expect(res.body.error).toBe("Symbol not found");
    expect(res.body.root).toBeUndefined();
  });
});

describe("GET /callers-recursive/:name", () => {
  it("returns recursive caller graph", async () => {
    const res = await request(app)
      .get("/callers-recursive/UpdateAI")
      .query({ depth: 2 })
      .expect(200);
    expect(res.body.direction).toBe("callers");
    expect(res.body.root.symbol.name).toBe("UpdateAI");
    expect(res.body.root.callers).toBeInstanceOf(Array);
    expect(res.body.root.callers.length).toBeGreaterThan(0);
  });

  it("returns 404 for unknown symbol", async () => {
    const res = await request(app).get("/callers-recursive/FakeFunc").expect(404);
    expect(res.body.code).toBe("NOT_FOUND");
    expect(res.body.error).toBe("Symbol not found");
  });
});

describe("Reliability signaling", () => {
  it("surfaces partial and low reliability for fragile zero-result navigation responses", async () => {
    const symbols: Symbol[] = [
      {
        id: "Game::FragileUpdate",
        name: "FragileUpdate",
        qualifiedName: "Game::FragileUpdate",
        language: "cpp",
        type: "method",
        filePath: "runtime/fragile.cpp",
        line: 10,
        endLine: 20,
        parseFragility: "elevated",
      },
    ];

    const fragileStore: Store = {
      getSymbolsByName(name: string) {
        return symbols.filter((symbol) => symbol.name === name);
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

    const fragileApp = createApp(fragileStore);

    const functionRes = await request(fragileApp).get("/function/FragileUpdate").expect(200);
    expect(functionRes.body.reliability.level).toBe("partial");
    expect(functionRes.body.reliability.factors).toContain("elevated_parse_fragility");
    expect(functionRes.body.indexCoverage).toBeUndefined();

    const callersRes = await request(fragileApp).get("/callers/FragileUpdate").expect(200);
    expect(callersRes.body.reliability.level).toBe("low");
    expect(callersRes.body.indexCoverage).toBe("low");
    expect(callersRes.body.coverageWarning).toContain("Zero callers");

    const refsRes = await request(fragileApp)
      .get("/references")
      .query({ qualifiedName: "Game::FragileUpdate" })
      .expect(200);
    expect(refsRes.body.reliability.level).toBe("low");
    expect(refsRes.body.indexCoverage).toBe("low");
    expect(refsRes.body.coverageWarning).toContain("Zero references");

    const graphRes = await request(fragileApp).get("/callgraph/FragileUpdate").expect(200);
    expect(graphRes.body.reliability.level).toBe("low");
    expect(graphRes.body.indexCoverage).toBe("low");
    expect(graphRes.body.coverageWarning).toContain("Zero callee edges");
  });

  it("recovers fragile callers from stored raw-call evidence when resolved callers are empty", async () => {
    const target: Symbol = {
      id: "Gameplay::ShotSystem::SetShotFlags",
      name: "SetShotFlags",
      qualifiedName: "Gameplay::ShotSystem::SetShotFlags",
      language: "cpp",
      type: "method",
      filePath: "gameplay/shotnormal.cpp",
      line: 1500,
      endLine: 1510,
      parentId: "Gameplay::ShotSystem",
      parseFragility: "elevated",
      macroSensitivity: "high",
      subsystem: "Gameplay",
      module: "Shot",
      artifactKind: "runtime",
    };
    const decoy: Symbol = {
      id: "Gameplay::ReplayShotSystem::SetShotFlags",
      name: "SetShotFlags",
      qualifiedName: "Gameplay::ReplayShotSystem::SetShotFlags",
      language: "cpp",
      type: "method",
      filePath: "gameplay/replay_shot.cpp",
      line: 80,
      endLine: 90,
      parentId: "Gameplay::ReplayShotSystem",
      subsystem: "Gameplay",
      module: "Replay",
      artifactKind: "editor",
    };
    const owner: Symbol = {
      id: "Gameplay::ShotSystem",
      name: "ShotSystem",
      qualifiedName: "Gameplay::ShotSystem",
      language: "cpp",
      type: "class",
      filePath: "gameplay/shotnormal.h",
      line: 10,
      endLine: 120,
    };
    const caller: Symbol = {
      id: "Gameplay::ShotSystem::ApplyShot",
      name: "ApplyShot",
      qualifiedName: "Gameplay::ShotSystem::ApplyShot",
      language: "cpp",
      type: "method",
      filePath: "gameplay/shotnormal.cpp",
      line: 1600,
      endLine: 1660,
      parentId: "Gameplay::ShotSystem",
      subsystem: "Gameplay",
      module: "Shot",
    };
    const symbols = [target, decoy, owner, caller];
    const recoveredStore: Store = {
      getSymbolsByName(name: string) {
        return name === "SetShotFlags" ? [target, decoy] : name === "ApplyShot" ? [caller] : [];
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
      getRawCallersByCalledName(calledName: string) {
        return calledName === "SetShotFlags"
          ? [{
            callerId: caller.id,
            calledName,
            callKind: "thisPointerAccess",
            filePath: "gameplay/shotnormal.cpp",
            line: 1634,
            receiver: "this",
          }]
          : [];
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

    const recoveredApp = createApp(recoveredStore);

    const callersRes = await request(recoveredApp).get("/callers/SetShotFlags").expect(200);
    expect(callersRes.body.callers).toHaveLength(1);
    expect(callersRes.body.callers[0].qualifiedName).toBe("Gameplay::ShotSystem::ApplyShot");
    expect(callersRes.body.callers[0].resolutionKind).toBe("recovered");
    expect(callersRes.body.callers[0].provenanceKind).toBe("raw_call");
    expect(callersRes.body.recoveredResultCount).toBe(1);
    expect(callersRes.body.indexCoverage).toBe("partial");
    expect(callersRes.body.coverageWarning).toContain("stored raw-call evidence");

    const graphRes = await request(recoveredApp)
      .get("/callgraph/SetShotFlags")
      .query({ direction: "callers" })
      .expect(200);
    expect(graphRes.body.root.callers).toHaveLength(1);
    expect(graphRes.body.root.callers[0].targetQualifiedName).toBe("Gameplay::ShotSystem::ApplyShot");
    expect(graphRes.body.root.callers[0].resolutionKind).toBe("recovered");
    expect(graphRes.body.root.callers[0].provenanceKind).toBe("raw_call");
    expect(graphRes.body.recoveredResultCount).toBe(1);
  });

  it("prefers direct base methods when recovering unqualified fragile callers", async () => {
    const target: Symbol = {
      id: "Gameplay::ShotSubSystem::SetShotFlags",
      name: "SetShotFlags",
      qualifiedName: "Gameplay::ShotSubSystem::SetShotFlags",
      language: "cpp",
      type: "method",
      filePath: "gameplay/shootingsubsys.cpp",
      line: 152,
      endLine: 160,
      parentId: "Gameplay::ShotSubSystem",
      parseFragility: "elevated",
      macroSensitivity: "high",
      subsystem: "Gameplay",
      module: "Shot",
      artifactKind: "runtime",
    };
    const decoy: Symbol = {
      id: "Gameplay::BallHandler::SetShotFlags",
      name: "SetShotFlags",
      qualifiedName: "Gameplay::BallHandler::SetShotFlags",
      language: "cpp",
      type: "method",
      filePath: "gameplay/ballhandler.cpp",
      line: 13085,
      endLine: 13120,
      parentId: "Gameplay::BallHandler",
      subsystem: "Gameplay",
      module: "User",
      artifactKind: "runtime",
    };
    const callerOwner: Symbol = {
      id: "Gameplay::ShotNormal",
      name: "ShotNormal",
      qualifiedName: "Gameplay::ShotNormal",
      language: "cpp",
      type: "class",
      filePath: "gameplay/shotnormal.h",
      line: 10,
      endLine: 120,
    };
    const baseOwner: Symbol = {
      id: "Gameplay::ShotSubSystem",
      name: "ShotSubSystem",
      qualifiedName: "Gameplay::ShotSubSystem",
      language: "cpp",
      type: "class",
      filePath: "gameplay/shootingsubsys.h",
      line: 10,
      endLine: 200,
    };
    const caller: Symbol = {
      id: "Gameplay::ShotNormal::CalcShotInformation",
      name: "CalcShotInformation",
      qualifiedName: "Gameplay::ShotNormal::CalcShotInformation",
      language: "cpp",
      type: "method",
      filePath: "gameplay/shotnormal.cpp",
      line: 1620,
      endLine: 1665,
      parentId: "Gameplay::ShotNormal",
      subsystem: "Gameplay",
      module: "Shot",
      artifactKind: "runtime",
    };
    const symbols = [target, decoy, callerOwner, baseOwner, caller];
    const recoveredStore: Store = {
      getSymbolsByName(name: string) {
        return name === "SetShotFlags" ? [target] : name === "CalcShotInformation" ? [caller] : [];
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
      getRawCallersByCalledName(calledName: string) {
        return calledName === "SetShotFlags"
          ? [{
            callerId: caller.id,
            calledName,
            callKind: "unqualified",
            filePath: "gameplay/shotnormal.cpp",
            line: 1634,
          }]
          : [];
      },
      getReferences() {
        return [];
      },
      getMembers() {
        return [];
      },
      getDirectBases(symbolId: string) {
        return symbolId === "Gameplay::ShotNormal" ? [baseOwner] : [];
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

    const recoveredApp = createApp(recoveredStore);
    const callersRes = await request(recoveredApp).get("/callers/SetShotFlags").expect(200);
    expect(callersRes.body.callers).toHaveLength(1);
    expect(callersRes.body.callers[0].qualifiedName).toBe("Gameplay::ShotNormal::CalcShotInformation");
    expect(callersRes.body.callers[0].matchReasons).toContain("base_parent_match");
  });

  it("uses callee file affinity to break recovered caller ties for macro-sensitive methods", async () => {
    const target: Symbol = {
      id: "Gameplay::ShotNormal::CalcShotInformation",
      name: "CalcShotInformation",
      qualifiedName: "Gameplay::ShotNormal::CalcShotInformation",
      language: "cpp",
      type: "method",
      filePath: "gameplay/shotnormal.cpp",
      definitionFilePath: "gameplay/shotnormal.cpp",
      declarationFilePath: "gameplay/shotnormal.h",
      line: 1620,
      endLine: 1665,
      parentId: "Gameplay::ShotNormal",
      parseFragility: "elevated",
      macroSensitivity: "high",
      subsystem: "Gameplay",
      module: "Shot",
      artifactKind: "runtime",
    };
    const decoy: Symbol = {
      id: "Gameplay::ReplayShotNormal::CalcShotInformation",
      name: "CalcShotInformation",
      qualifiedName: "Gameplay::ReplayShotNormal::CalcShotInformation",
      language: "cpp",
      type: "method",
      filePath: "gameplay/replay_shot.cpp",
      definitionFilePath: "gameplay/replay_shot.cpp",
      declarationFilePath: "gameplay/replay_shot.h",
      line: 400,
      endLine: 460,
      parentId: "Gameplay::ReplayShotNormal",
      parseFragility: "elevated",
      macroSensitivity: "high",
      subsystem: "Gameplay",
      module: "Replay",
      artifactKind: "editor",
    };
    const caller: Symbol = {
      id: "Gameplay::ShotController::UpdatePreview",
      name: "UpdatePreview",
      qualifiedName: "Gameplay::ShotController::UpdatePreview",
      language: "cpp",
      type: "method",
      filePath: "gameplay/shotcontroller.cpp",
      line: 200,
      endLine: 260,
      parentId: "Gameplay::ShotController",
      subsystem: "Gameplay",
      module: "Shot",
      artifactKind: "runtime",
    };
    const symbols = [target, decoy, caller];
    const recoveredStore: Store = {
      getSymbolsByName(name: string) {
        return name === "CalcShotInformation" ? [target, decoy] : name === "UpdatePreview" ? [caller] : [];
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
      getRawCallersByCalledName(calledName: string) {
        return calledName === "CalcShotInformation"
          ? [{
            callerId: caller.id,
            calledName,
            callKind: "memberAccess",
            filePath: "gameplay/shotnormal.cpp",
            line: 211,
            receiver: "m_shotNormal",
          }]
          : [];
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

    const recoveredApp = createApp(recoveredStore);
    const callersRes = await request(recoveredApp).get("/callers/CalcShotInformation").expect(200);
    expect(callersRes.body.callers).toHaveLength(1);
    expect(callersRes.body.symbol.qualifiedName).toBe("Gameplay::ShotNormal::CalcShotInformation");
    expect(callersRes.body.callers[0].qualifiedName).toBe("Gameplay::ShotController::UpdatePreview");
    expect(callersRes.body.callers[0].matchReasons).toContain("same_file_match");
  });

  it("recovers non-fragile callers when receiver names strongly match the owner type", async () => {
    const target: Symbol = {
      id: "Gameplay::ShootingSys::CalcShotInformation",
      name: "CalcShotInformation",
      qualifiedName: "Gameplay::ShootingSys::CalcShotInformation",
      language: "cpp",
      type: "method",
      filePath: "gameplay/shootingsys.cpp",
      line: 61,
      endLine: 90,
      parentId: "Gameplay::ShootingSys",
      parseFragility: "low",
      macroSensitivity: "low",
      subsystem: "Gameplay",
      module: "Shot",
      artifactKind: "runtime",
    };
    const decoyA: Symbol = {
      id: "Gameplay::ShotNormal::CalcShotInformation",
      name: "CalcShotInformation",
      qualifiedName: "Gameplay::ShotNormal::CalcShotInformation",
      language: "cpp",
      type: "method",
      filePath: "gameplay/shotnormal.cpp",
      line: 1600,
      endLine: 1665,
      parentId: "Gameplay::ShotNormal",
      parseFragility: "elevated",
      macroSensitivity: "high",
      subsystem: "Gameplay",
      module: "Shot",
      artifactKind: "runtime",
    };
    const decoyB: Symbol = {
      id: "Gameplay::ShotSubSystem::CalcShotInformation",
      name: "CalcShotInformation",
      qualifiedName: "Gameplay::ShotSubSystem::CalcShotInformation",
      language: "cpp",
      type: "method",
      filePath: "gameplay/shootingsubsys.h",
      line: 97,
      endLine: 104,
      parentId: "Gameplay::ShotSubSystem",
      parseFragility: "elevated",
      macroSensitivity: "high",
      subsystem: "Gameplay",
      module: "Shot",
      artifactKind: "runtime",
    };
    const caller: Symbol = {
      id: "Gameplay::BallHandler::UpdateShot",
      name: "UpdateShot",
      qualifiedName: "Gameplay::BallHandler::UpdateShot",
      language: "cpp",
      type: "method",
      filePath: "gameplay/ballhandler.cpp",
      line: 12600,
      endLine: 12680,
      parentId: "Gameplay::BallHandler",
      subsystem: "Gameplay",
      module: "Shot",
      artifactKind: "runtime",
    };
    const owner: Symbol = {
      id: "Gameplay::ShootingSys",
      name: "ShootingSys",
      qualifiedName: "Gameplay::ShootingSys",
      language: "cpp",
      type: "class",
      filePath: "gameplay/shootingsys.h",
      line: 1,
      endLine: 120,
    };
    const decoyOwnerA: Symbol = {
      id: "Gameplay::ShotNormal",
      name: "ShotNormal",
      qualifiedName: "Gameplay::ShotNormal",
      language: "cpp",
      type: "class",
      filePath: "gameplay/shotnormal.h",
      line: 1,
      endLine: 120,
    };
    const decoyOwnerB: Symbol = {
      id: "Gameplay::ShotSubSystem",
      name: "ShotSubSystem",
      qualifiedName: "Gameplay::ShotSubSystem",
      language: "cpp",
      type: "class",
      filePath: "gameplay/shootingsubsys.h",
      line: 1,
      endLine: 120,
    };
    const symbols = [target, decoyA, decoyB, caller, owner, decoyOwnerA, decoyOwnerB];
    const recoveredStore: Store = {
      getSymbolsByName(name: string) {
        return name === "CalcShotInformation" ? [target, decoyA, decoyB] : name === "UpdateShot" ? [caller] : [];
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
      getRawCallersByCalledName(calledName: string) {
        return calledName === "CalcShotInformation"
          ? [{
            callerId: caller.id,
            calledName,
            callKind: "pointerMemberAccess",
            filePath: "gameplay/ballhandler.cpp",
            line: 12627,
            receiver: "mShootingSys",
          }]
          : [];
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

    const recoveredApp = createApp(recoveredStore);
    const callersRes = await request(recoveredApp).get("/callers/CalcShotInformation").expect(200);
    expect(callersRes.body.symbol.qualifiedName).toBe("Gameplay::ShootingSys::CalcShotInformation");
    expect(callersRes.body.callers).toHaveLength(1);
    expect(callersRes.body.callers[0].qualifiedName).toBe("Gameplay::BallHandler::UpdateShot");
    expect(callersRes.body.callers[0].matchReasons).toContain("receiver_parent_name_match");
    expect(callersRes.body.reliability.level).toBe("partial");
    expect(callersRes.body.recoveredResultCount).toBe(1);
  });

  it("uses owner factory evidence to recover exact override-style callers", async () => {
    const target: Symbol = {
      id: "Gameplay::ShotNormal::CalcShotInformation",
      name: "CalcShotInformation",
      qualifiedName: "Gameplay::ShotNormal::CalcShotInformation",
      language: "cpp",
      type: "method",
      filePath: "gameplay/shotnormal.cpp",
      line: 1615,
      endLine: 1665,
      parentId: "Gameplay::ShotNormal",
      parseFragility: "elevated",
      macroSensitivity: "high",
      subsystem: "Gameplay",
      module: "Shot",
      artifactKind: "runtime",
    };
    const decoyA: Symbol = {
      id: "Gameplay::ShotSubSystem::CalcShotInformation",
      name: "CalcShotInformation",
      qualifiedName: "Gameplay::ShotSubSystem::CalcShotInformation",
      language: "cpp",
      type: "method",
      filePath: "gameplay/shootingsubsys.h",
      line: 97,
      endLine: 104,
      parentId: "Gameplay::ShotSubSystem",
      subsystem: "Gameplay",
      module: "Shot",
      artifactKind: "runtime",
    };
    const decoyB: Symbol = {
      id: "Gameplay::ShootingSys::CalcShotInformation",
      name: "CalcShotInformation",
      qualifiedName: "Gameplay::ShootingSys::CalcShotInformation",
      language: "cpp",
      type: "method",
      filePath: "gameplay/shootingsys.cpp",
      line: 61,
      endLine: 90,
      parentId: "Gameplay::ShootingSys",
      subsystem: "Gameplay",
      module: "Shot",
      artifactKind: "runtime",
    };
    const caller: Symbol = {
      id: "Gameplay::ShootingSys::CalcShotInformation",
      name: "CalcShotInformation",
      qualifiedName: "Gameplay::ShootingSys::CalcShotInformation",
      language: "cpp",
      type: "method",
      filePath: "gameplay/shootingsys.cpp",
      line: 61,
      endLine: 90,
      parentId: "Gameplay::ShootingSys",
      subsystem: "Gameplay",
      module: "Shot",
      artifactKind: "runtime",
    };
    const owner: Symbol = {
      id: "Gameplay::ShootingSys",
      name: "ShootingSys",
      qualifiedName: "Gameplay::ShootingSys",
      language: "cpp",
      type: "class",
      filePath: "gameplay/shootingsys.h",
      line: 1,
      endLine: 120,
    };
    const targetOwner: Symbol = {
      id: "Gameplay::ShotNormal",
      name: "ShotNormal",
      qualifiedName: "Gameplay::ShotNormal",
      language: "cpp",
      type: "class",
      filePath: "gameplay/shotnormal.h",
      line: 1,
      endLine: 120,
    };
    const baseOwner: Symbol = {
      id: "Gameplay::ShotSubSystem",
      name: "ShotSubSystem",
      qualifiedName: "Gameplay::ShotSubSystem",
      language: "cpp",
      type: "class",
      filePath: "gameplay/shootingsubsys.h",
      line: 1,
      endLine: 120,
    };
    const createMethod: Symbol = {
      id: "Gameplay::ShootingSys::CreateSubSystem",
      name: "CreateSubSystem",
      qualifiedName: "Gameplay::ShootingSys::CreateSubSystem",
      language: "cpp",
      type: "method",
      filePath: "gameplay/shootingsys.cpp",
      line: 40,
      endLine: 58,
      parentId: "Gameplay::ShootingSys",
    };
    const symbols = [target, decoyA, decoyB, caller, owner, targetOwner, baseOwner, createMethod];
    const recoveredStore: Store = {
      getSymbolsByName(name: string) {
        return name === "CalcShotInformation" ? [target, decoyA, decoyB] : [];
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
      getRawCallersByCalledName(calledName: string) {
        return calledName === "CalcShotInformation"
          ? [{
            callerId: caller.id,
            calledName,
            callKind: "pointerMemberAccess",
            filePath: "gameplay/shootingsys.cpp",
            line: 63,
            receiver: "mSubSystem",
          }]
          : [];
      },
      getRawCallsByCallerId(callerId: string) {
        return callerId === createMethod.id
          ? [{
            callerId,
            calledName: "Create",
            callKind: "qualified",
            filePath: "gameplay/shootingsys.cpp",
            line: 46,
            qualifier: "ShotNormal",
          }]
          : [];
      },
      getReferences() {
        return [];
      },
      getMembers(parentId: string) {
        return parentId === owner.id ? [caller, createMethod] : [];
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

    const recoveredApp = createApp(recoveredStore);
    const callersRes = await request(recoveredApp)
      .get("/symbol")
      .query({ qualifiedName: "Gameplay::ShotNormal::CalcShotInformation" })
      .expect(200);
    expect(callersRes.body.callers).toHaveLength(1);
    expect(callersRes.body.callers[0].qualifiedName).toBe("Gameplay::ShootingSys::CalcShotInformation");
    expect(callersRes.body.callers[0].matchReasons).toContain("owner_factory_type_match");
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

describe("Enum member references", () => {
  it("returns exact enum member references and can aggregate enum value usage from the enum type", async () => {
    const symbols: Symbol[] = [
      {
        id: "Game::AIState",
        name: "AIState",
        qualifiedName: "Game::AIState",
        language: "cpp",
        type: "enum",
        filePath: "runtime/ai_state.h",
        line: 1,
        endLine: 5,
      },
      {
        id: "Game::AIState::Idle",
        name: "Idle",
        qualifiedName: "Game::AIState::Idle",
        language: "cpp",
        type: "enumMember",
        filePath: "runtime/ai_state.h",
        line: 2,
        endLine: 2,
        parentId: "Game::AIState",
      },
      {
        id: "Game::AIState::Chase",
        name: "Chase",
        qualifiedName: "Game::AIState::Chase",
        language: "cpp",
        type: "enumMember",
        filePath: "runtime/ai_state.h",
        line: 3,
        endLine: 3,
        parentId: "Game::AIState",
      },
      {
        id: "Game::Controller::Update",
        name: "Update",
        qualifiedName: "Game::Controller::Update",
        language: "cpp",
        type: "method",
        filePath: "runtime/controller.cpp",
        line: 10,
        endLine: 20,
        parentId: "Game::Controller",
      },
    ];
    const references = [
      {
        sourceSymbolId: "Game::Controller::Update",
        targetSymbolId: "Game::AIState::Idle",
        category: "enumValueUsage" as const,
        filePath: "runtime/controller.cpp",
        line: 12,
        confidence: "partial" as const,
      },
      {
        sourceSymbolId: "Game::Controller::Update",
        targetSymbolId: "Game::AIState::Chase",
        category: "enumValueUsage" as const,
        filePath: "runtime/controller.cpp",
        line: 14,
        confidence: "partial" as const,
      },
    ];

    const enumStore: Store = {
      getSymbolsByName(name: string) {
        return symbols.filter((symbol) => symbol.name === name);
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
      getReferences(targetSymbolId: string, category?: any) {
        return references.filter((reference) =>
          reference.targetSymbolId === targetSymbolId
          && (!category || reference.category === category));
      },
      getMembers(parentId: string) {
        return symbols.filter((symbol) => symbol.parentId === parentId);
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

    const enumApp = createApp(enumStore);

    const memberRes = await request(enumApp)
      .get("/references")
      .query({ qualifiedName: "Game::AIState::Idle", category: "enumValueUsage" })
      .expect(200);
    expect(memberRes.body.references).toHaveLength(1);
    expect(memberRes.body.references[0].targetSymbolId).toBe("Game::AIState::Idle");

    const enumRes = await request(enumApp)
      .get("/references")
      .query({ qualifiedName: "Game::AIState", includeEnumValueUsage: "true" })
      .expect(200);
    expect(enumRes.body.references).toHaveLength(2);
    expect(enumRes.body.references.map((reference: any) => reference.targetSymbolId).sort()).toEqual([
      "Game::AIState::Chase",
      "Game::AIState::Idle",
    ]);

    const compactRes = await request(enumApp)
      .get("/references")
      .query({ qualifiedName: "Game::AIState", includeEnumValueUsage: "true", compact: "true" })
      .expect(200);
    expect(compactRes.body.responseMode).toBe("compact");
    expect(compactRes.body.references).toHaveLength(2);
    expect(compactRes.body.references[0].sourceQualifiedName).toBeDefined();
    expect(compactRes.body.references[0].targetQualifiedName).toBeDefined();
    expect(compactRes.body.references[0].confidence).toBeUndefined();
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

  it("returns compact file overview when requested", async () => {
    const res = await request(app)
      .get("/file-symbols")
      .query({ filePath: "src/game_object.h", compact: "true" })
      .expect(200);
    expect(res.body.responseMode).toBe("compact");
    expect(res.body.symbols.length).toBeGreaterThan(0);
    expect(res.body.symbols[0].qualifiedName).toBeDefined();
    expect(res.body.symbols[0].endLine).toEqual(expect.any(Number));
    expect(res.body.symbols[0].filePath).toBeUndefined();
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
