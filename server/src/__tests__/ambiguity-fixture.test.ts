import * as fs from "fs";
import * as os from "os";
import * as path from "path";
import Database from "better-sqlite3";
import request from "supertest";
import { createApp } from "../app";
import { JsonStore } from "../storage/json-store";
import { SqliteStore } from "../storage/sqlite-store";
import { mcpCall } from "./mcp-test-helpers";
import { Symbol } from "../models/symbol";
import { Call } from "../models/call";
import { FileRecord } from "../models/file-record";
import { ReferenceRecord } from "../models/responses";

function fixtureSymbols(): Symbol[] {
  return [
    {
      id: "Gameplay::Update",
      name: "Update",
      qualifiedName: "Gameplay::Update",
      type: "function",
      filePath: "samples/ambiguity/src/namespace_dupes.h",
      line: 4,
      endLine: 4,
      scopeQualifiedName: "Gameplay",
      scopeKind: "namespace",
    },
    {
      id: "UI::Update",
      name: "Update",
      qualifiedName: "UI::Update",
      type: "function",
      filePath: "samples/ambiguity/src/namespace_dupes.h",
      line: 8,
      endLine: 8,
      scopeQualifiedName: "UI",
      scopeKind: "namespace",
    },
    {
      id: "Gameplay",
      name: "Gameplay",
      qualifiedName: "Gameplay",
      type: "namespace",
      filePath: "samples/ambiguity/src/namespace_dupes.h",
      line: 1,
      endLine: 5,
    },
    {
      id: "UI",
      name: "UI",
      qualifiedName: "UI",
      type: "namespace",
      filePath: "samples/ambiguity/src/namespace_dupes.h",
      line: 6,
      endLine: 9,
    },
    {
      id: "AI::Controller",
      name: "Controller",
      qualifiedName: "AI::Controller",
      type: "class",
      filePath: "samples/ambiguity/src/namespace_dupes.h",
      line: 12,
      endLine: 15,
    },
    {
      id: "AI::Controller::Update",
      name: "Update",
      qualifiedName: "AI::Controller::Update",
      type: "method",
      filePath: "samples/ambiguity/src/namespace_dupes.cpp",
      line: 11,
      endLine: 14,
      parentId: "AI::Controller",
    },
    {
      id: "Game::Worker",
      name: "Worker",
      qualifiedName: "Game::Worker",
      type: "class",
      filePath: "samples/ambiguity/src/split_update.h",
      line: 5,
      endLine: 9,
    },
    {
      id: "Game::Worker::Update",
      name: "Update",
      qualifiedName: "Game::Worker::Update",
      type: "method",
      filePath: "samples/ambiguity/src/split_update.cpp",
      line: 5,
      endLine: 5,
      parentId: "Game::Worker",
    },
    {
      id: "Game::Worker::Tick",
      name: "Tick",
      qualifiedName: "Game::Worker::Tick",
      type: "method",
      filePath: "samples/ambiguity/src/split_update.cpp",
      line: 7,
      endLine: 10,
      parentId: "Game::Worker",
    },
  ];
}

function fixtureCalls(): Call[] {
  return [
    {
      callerId: "AI::Controller::Update",
      calleeId: "Gameplay::Update",
      filePath: "samples/ambiguity/src/namespace_dupes.cpp",
      line: 12,
    },
    {
      callerId: "AI::Controller::Update",
      calleeId: "UI::Update",
      filePath: "samples/ambiguity/src/namespace_dupes.cpp",
      line: 13,
    },
    {
      callerId: "Game::Worker::Tick",
      calleeId: "Game::Worker::Update",
      filePath: "samples/ambiguity/src/split_update.cpp",
      line: 8,
    },
    {
      callerId: "Game::Worker::Tick",
      calleeId: "Game::Worker::Update",
      filePath: "samples/ambiguity/src/split_update.cpp",
      line: 9,
    },
  ];
}

function fixtureFiles(): FileRecord[] {
  return [
    {
      path: "samples/ambiguity/src/namespace_dupes.h",
      contentHash: "fixture-namespace-dupes-h",
      lastIndexed: "2026-04-18T00:00:00Z",
      symbolCount: 4,
    },
    {
      path: "samples/ambiguity/src/namespace_dupes.cpp",
      contentHash: "fixture-namespace-dupes-cpp",
      lastIndexed: "2026-04-18T00:00:00Z",
      symbolCount: 3,
    },
    {
      path: "samples/ambiguity/src/split_update.h",
      contentHash: "fixture-split-update-h",
      lastIndexed: "2026-04-18T00:00:00Z",
      symbolCount: 3,
    },
    {
      path: "samples/ambiguity/src/split_update.cpp",
      contentHash: "fixture-split-update-cpp",
      lastIndexed: "2026-04-18T00:00:00Z",
      symbolCount: 2,
    },
  ];
}

function fixtureReferences(): ReferenceRecord[] {
  return [
    {
      sourceSymbolId: "AI::Controller::Update",
      targetSymbolId: "Gameplay::Update",
      category: "functionCall",
      filePath: "samples/ambiguity/src/namespace_dupes.cpp",
      line: 12,
      confidence: "high",
    },
    {
      sourceSymbolId: "Game::Worker::Tick",
      targetSymbolId: "Game::Worker::Update",
      category: "methodCall",
      filePath: "samples/ambiguity/src/split_update.cpp",
      line: 8,
      confidence: "high",
    },
    {
      sourceSymbolId: "Game::Worker::Tick",
      targetSymbolId: "Game::Worker::Update",
      category: "methodCall",
      filePath: "samples/ambiguity/src/split_update.cpp",
      line: 9,
      confidence: "high",
    },
  ];
}

function writeFixtureJsonStore(): { dir: string; store: JsonStore } {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "codeatlas-ambiguity-json-"));
  const store = new JsonStore(dir);
  store.save({
    symbols: fixtureSymbols(),
    calls: fixtureCalls(),
    references: fixtureReferences(),
    files: fixtureFiles(),
  });
  return { dir, store };
}

function writeFixtureSqliteDb(): string {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "codeatlas-ambiguity-sqlite-"));
  const dbPath = path.join(dir, "index.db");
  const db = new Database(dbPath);

  db.exec(`
    CREATE TABLE symbols (
      id TEXT PRIMARY KEY,
      name TEXT NOT NULL,
      qualified_name TEXT NOT NULL,
      type TEXT NOT NULL,
      file_path TEXT NOT NULL,
      line INTEGER NOT NULL,
      end_line INTEGER NOT NULL,
      signature TEXT,
      parameter_count INTEGER,
      scope_qualified_name TEXT,
      scope_kind TEXT,
      symbol_role TEXT,
      declaration_file_path TEXT,
      declaration_line INTEGER,
      declaration_end_line INTEGER,
      definition_file_path TEXT,
      definition_line INTEGER,
      definition_end_line INTEGER,
      parent_id TEXT
    );
    CREATE TABLE calls (
      caller_id TEXT NOT NULL,
      callee_id TEXT NOT NULL,
      file_path TEXT NOT NULL,
      line INTEGER NOT NULL
    );
    CREATE TABLE symbol_references (
      source_symbol_id TEXT NOT NULL,
      target_symbol_id TEXT NOT NULL,
      category TEXT NOT NULL,
      file_path TEXT NOT NULL,
      line INTEGER NOT NULL,
      confidence TEXT NOT NULL
    );
  `);

  const insertSymbol = db.prepare(`
    INSERT INTO symbols (
      id, name, qualified_name, type, file_path, line, end_line, signature,
      parameter_count, scope_qualified_name, scope_kind, symbol_role,
      declaration_file_path, declaration_line, declaration_end_line,
      definition_file_path, definition_line, definition_end_line, parent_id
    ) VALUES (
      @id, @name, @qualified_name, @type, @file_path, @line, @end_line, @signature,
      @parameter_count, @scope_qualified_name, @scope_kind, @symbol_role,
      @declaration_file_path, @declaration_line, @declaration_end_line,
      @definition_file_path, @definition_line, @definition_end_line, @parent_id
    )
  `);
  const insertCall = db.prepare(`
    INSERT INTO calls (caller_id, callee_id, file_path, line)
    VALUES (@caller_id, @callee_id, @file_path, @line)
  `);
  const insertReference = db.prepare(`
    INSERT INTO symbol_references (source_symbol_id, target_symbol_id, category, file_path, line, confidence)
    VALUES (@source_symbol_id, @target_symbol_id, @category, @file_path, @line, @confidence)
  `);

  for (const symbol of fixtureSymbols()) {
    insertSymbol.run({
      id: symbol.id,
      name: symbol.name,
      qualified_name: symbol.qualifiedName,
      type: symbol.type,
      file_path: symbol.filePath,
      line: symbol.line,
      end_line: symbol.endLine,
      signature: symbol.signature ?? null,
      parameter_count: symbol.parameterCount ?? null,
      scope_qualified_name: symbol.scopeQualifiedName ?? null,
      scope_kind: symbol.scopeKind ?? null,
      symbol_role: symbol.symbolRole ?? null,
      declaration_file_path: symbol.declarationFilePath ?? null,
      declaration_line: symbol.declarationLine ?? null,
      declaration_end_line: symbol.declarationEndLine ?? null,
      definition_file_path: symbol.definitionFilePath ?? null,
      definition_line: symbol.definitionLine ?? null,
      definition_end_line: symbol.definitionEndLine ?? null,
      parent_id: symbol.parentId ?? null,
    });
  }

  for (const call of fixtureCalls()) {
    insertCall.run({
      caller_id: call.callerId,
      callee_id: call.calleeId,
      file_path: call.filePath,
      line: call.line,
    });
  }
  for (const reference of fixtureReferences()) {
    insertReference.run({
      source_symbol_id: reference.sourceSymbolId,
      target_symbol_id: reference.targetSymbolId,
      category: reference.category,
      file_path: reference.filePath,
      line: reference.line,
      confidence: reference.confidence,
    });
  }

  db.pragma("journal_mode = WAL");
  db.close();
  return dbPath;
}

const INIT = { jsonrpc: "2.0", id: 1, method: "initialize", params: { protocolVersion: "2024-11-05", capabilities: {}, clientInfo: { name: "fixture-test", version: "1.0" } } };
const INITIALIZED = { jsonrpc: "2.0", method: "notifications/initialized" };

describe("ambiguity fixture storage and API contracts", () => {
  it("JsonStore preserves exact lookup across duplicate short names", () => {
    const { dir, store } = writeFixtureJsonStore();
    try {
      const duplicateUpdates = store.getSymbolsByName("Update");
      expect(duplicateUpdates).toHaveLength(4);

      const exact = store.getSymbolByQualifiedName("Gameplay::Update");
      expect(exact?.id).toBe("Gameplay::Update");
      expect(exact?.qualifiedName).toBe("Gameplay::Update");
    } finally {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  });

  it("SqliteStore preserves exact lookup across duplicate short names", () => {
    const dbPath = writeFixtureSqliteDb();
    const store = new SqliteStore(dbPath);
    try {
      const duplicateUpdates = store.getSymbolsByName("Update");
      expect(duplicateUpdates).toHaveLength(4);

      const exact = store.getSymbolByQualifiedName("Game::Worker::Update");
      expect(exact?.id).toBe("Game::Worker::Update");
      expect(exact?.qualifiedName).toBe("Game::Worker::Update");
    } finally {
      store.close();
      fs.rmSync(path.dirname(dbPath), { recursive: true, force: true });
    }
  });

  it("HTTP exact lookup and heuristic ambiguity follow the fixture contract", async () => {
    const { dir } = writeFixtureJsonStore();
    const store = new JsonStore(dir);
    const app = createApp(store);
    try {
      const exact = await request(app)
        .get("/symbol")
        .query({ qualifiedName: "Game::Worker::Update" })
        .expect(200);
      expect(exact.body.lookupMode).toBe("exact");
      expect(exact.body.confidence).toBe("exact");
      expect(exact.body.matchReasons).toEqual(["exact_qualified_name_match"]);
      expect(exact.body.symbol.qualifiedName).toBe("Game::Worker::Update");

      const heuristic = await request(app).get("/function/Update").expect(200);
      expect(heuristic.body.lookupMode).toBe("heuristic");
      expect(heuristic.body.confidence).toBe("ambiguous");
      expect(heuristic.body.matchReasons).toEqual(["ambiguous_top_score"]);
      expect(heuristic.body.ambiguity).toEqual({ candidateCount: 4 });

      const callers = await request(app).get("/callers/Update").expect(200);
      expect(callers.body.lookupMode).toBe("heuristic");
      expect(callers.body.confidence).toBe("ambiguous");
      expect(callers.body.matchReasons).toEqual(["ambiguous_top_score"]);
      expect(callers.body.ambiguity).toEqual({ candidateCount: 4 });
      expect(callers.body.totalCount).toBe(1);
      expect(callers.body.truncated).toBe(false);
      expect(callers.body.window.totalCount).toBe(1);
      expect(callers.body.window.returnedCount).toBe(1);
      expect(callers.body.callers).toHaveLength(1);
      expect(callers.body.callers[0].qualifiedName).toBe("AI::Controller::Update");

      const references = await request(app)
        .get("/references")
        .query({ qualifiedName: "Game::Worker::Update", category: "methodCall" })
        .expect(200);
      expect(references.body.lookupMode).toBe("exact");
      expect(references.body.references).toHaveLength(2);
      expect(references.body.totalCount).toBe(2);
      expect(references.body.window.totalCount).toBe(2);
      expect(references.body.window.returnedCount).toBe(2);
      expect(references.body.references[0].sourceQualifiedName).toBe("Game::Worker::Tick");
      expect(references.body.references[0].category).toBe("methodCall");

      const impact = await request(app)
        .get("/impact")
        .query({ qualifiedName: "Game::Worker::Update", depth: 2, limit: 10 })
        .expect(200);
      expect(impact.body.lookupMode).toBe("exact");
      expect(impact.body.directCallers).toHaveLength(1);
      expect(impact.body.directReferences).toHaveLength(2);
      expect(impact.body.totalAffectedSymbols).toBeGreaterThanOrEqual(1);
      expect(impact.body.topAffectedSymbols[0].qualifiedName).toBe("Game::Worker::Tick");
      expect(impact.body.suggestedFollowUpQueries).toContain("find_references qualifiedName=Game::Worker::Update");

      const fileSymbols = await request(app)
        .get("/file-symbols")
        .query({ filePath: "samples/ambiguity/src/namespace_dupes.h" })
        .expect(200);
      expect(fileSymbols.body.summary.totalCount).toBe(fileSymbols.body.symbols.length);
      expect(fileSymbols.body.window.totalCount).toBe(fileSymbols.body.summary.totalCount);
      expect(fileSymbols.body.symbols[0].qualifiedName).toBe("Gameplay");

      const namespaceSymbols = await request(app)
        .get("/namespace-symbols")
        .query({ qualifiedName: "Gameplay" })
        .expect(200);
      expect(namespaceSymbols.body.lookupMode).toBe("exact");
      expect(namespaceSymbols.body.summary.totalCount).toBe(1);
      expect(namespaceSymbols.body.window.totalCount).toBe(1);
      expect(namespaceSymbols.body.symbols[0].qualifiedName).toBe("Gameplay::Update");

      const classMembers = await request(app)
        .get("/class-members")
        .query({ qualifiedName: "Game::Worker" })
        .expect(200);
      expect(classMembers.body.lookupMode).toBe("exact");
      expect(classMembers.body.summary.totalCount).toBe(2);
      expect(classMembers.body.window.totalCount).toBe(2);
      expect(classMembers.body.members.map((member: any) => member.qualifiedName)).toEqual([
        "Game::Worker::Update",
        "Game::Worker::Tick",
      ]);
    } finally {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  });

  it("MCP exact lookup and heuristic ambiguity follow the fixture contract", async () => {
    const { dir } = writeFixtureJsonStore();
    try {
      const exactResponses = await mcpCall([
        INIT,
        INITIALIZED,
        {
          jsonrpc: "2.0",
          id: 2,
          method: "tools/call",
          params: { name: "lookup_symbol", arguments: { qualifiedName: "Gameplay::Update" } },
        },
      ], dir);
      const exact = JSON.parse(exactResponses.find((r) => r.id === 2).result.content[0].text);
      expect(exact.lookupMode).toBe("exact");
      expect(exact.confidence).toBe("exact");
      expect(exact.matchReasons).toEqual(["exact_qualified_name_match"]);
      expect(exact.symbol.qualifiedName).toBe("Gameplay::Update");

      const heuristicResponses = await mcpCall([
        INIT,
        INITIALIZED,
        {
          jsonrpc: "2.0",
          id: 3,
          method: "tools/call",
          params: { name: "lookup_function", arguments: { name: "Update" } },
        },
      ], dir);
      const heuristic = JSON.parse(heuristicResponses.find((r) => r.id === 3).result.content[0].text);
      expect(heuristic.lookupMode).toBe("heuristic");
      expect(heuristic.confidence).toBe("ambiguous");
      expect(heuristic.matchReasons).toEqual(["ambiguous_top_score"]);
      expect(heuristic.ambiguity).toEqual({ candidateCount: 4 });

      const callerResponses = await mcpCall([
        INIT,
        INITIALIZED,
        {
          jsonrpc: "2.0",
          id: 4,
          method: "tools/call",
          params: { name: "find_callers", arguments: { name: "Update", limit: 10 } },
        },
      ], dir);
      const callers = JSON.parse(callerResponses.find((r) => r.id === 4).result.content[0].text);
      expect(callers.lookupMode).toBe("heuristic");
      expect(callers.confidence).toBe("ambiguous");
      expect(callers.matchReasons).toEqual(["ambiguous_top_score"]);
      expect(callers.ambiguity).toEqual({ candidateCount: 4 });
      expect(callers.totalCount).toBe(1);
      expect(callers.truncated).toBe(false);
      expect(callers.window.totalCount).toBe(1);
      expect(callers.window.returnedCount).toBe(1);
      expect(callers.callers).toHaveLength(1);
      expect(callers.callers[0].qualifiedName).toBe("AI::Controller::Update");

      const referenceResponses = await mcpCall([
        INIT,
        INITIALIZED,
        {
          jsonrpc: "2.0",
          id: 5,
          method: "tools/call",
          params: {
            name: "find_references",
            arguments: { qualifiedName: "Gameplay::Update", category: "functionCall", limit: 10 },
          },
        },
      ], dir);
      const references = JSON.parse(referenceResponses.find((r) => r.id === 5).result.content[0].text);
      expect(references.lookupMode).toBe("exact");
      expect(references.references).toHaveLength(1);
      expect(references.totalCount).toBe(1);
      expect(references.window.totalCount).toBe(1);
      expect(references.window.returnedCount).toBe(1);
      expect(references.references[0].sourceQualifiedName).toBe("AI::Controller::Update");
      expect(references.references[0].category).toBe("functionCall");

      const impactResponses = await mcpCall([
        INIT,
        INITIALIZED,
        {
          jsonrpc: "2.0",
          id: 6,
          method: "tools/call",
          params: {
            name: "impact_analysis",
            arguments: { qualifiedName: "Gameplay::Update", depth: 2, limit: 10 },
          },
        },
      ], dir);
      const impact = JSON.parse(impactResponses.find((r) => r.id === 6).result.content[0].text);
      expect(impact.lookupMode).toBe("exact");
      expect(impact.directCallers).toHaveLength(1);
      expect(impact.directReferences).toHaveLength(1);
      expect(impact.totalAffectedSymbols).toBeGreaterThanOrEqual(1);
      expect(impact.topAffectedSymbols[0].qualifiedName).toBe("AI::Controller::Update");
      expect(impact.suggestedFollowUpQueries).toContain("find_callers qualifiedName=Gameplay::Update");

      const fileResponses = await mcpCall([
        INIT,
        INITIALIZED,
        {
          jsonrpc: "2.0",
          id: 7,
          method: "tools/call",
          params: { name: "list_file_symbols", arguments: { filePath: "samples/ambiguity/src/namespace_dupes.h" } },
        },
      ], dir);
      const fileSymbols = JSON.parse(fileResponses.find((r) => r.id === 7).result.content[0].text);
      expect(fileSymbols.summary.totalCount).toBe(fileSymbols.symbols.length);
      expect(fileSymbols.window.totalCount).toBe(fileSymbols.summary.totalCount);
      expect(fileSymbols.symbols[0].qualifiedName).toBe("Gameplay");

      const namespaceResponses = await mcpCall([
        INIT,
        INITIALIZED,
        {
          jsonrpc: "2.0",
          id: 8,
          method: "tools/call",
          params: { name: "list_namespace_symbols", arguments: { qualifiedName: "Gameplay" } },
        },
      ], dir);
      const namespaceSymbols = JSON.parse(namespaceResponses.find((r) => r.id === 8).result.content[0].text);
      expect(namespaceSymbols.lookupMode).toBe("exact");
      expect(namespaceSymbols.summary.totalCount).toBe(1);
      expect(namespaceSymbols.window.totalCount).toBe(1);
      expect(namespaceSymbols.symbols[0].qualifiedName).toBe("Gameplay::Update");

      const classResponses = await mcpCall([
        INIT,
        INITIALIZED,
        {
          jsonrpc: "2.0",
          id: 9,
          method: "tools/call",
          params: { name: "list_class_members", arguments: { qualifiedName: "Game::Worker" } },
        },
      ], dir);
      const classMembers = JSON.parse(classResponses.find((r) => r.id === 9).result.content[0].text);
      expect(classMembers.lookupMode).toBe("exact");
      expect(classMembers.summary.totalCount).toBe(2);
      expect(classMembers.window.totalCount).toBe(2);
      expect(classMembers.members.map((member: any) => member.qualifiedName)).toEqual([
        "Game::Worker::Update",
        "Game::Worker::Tick",
      ]);
    } finally {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  });
});
