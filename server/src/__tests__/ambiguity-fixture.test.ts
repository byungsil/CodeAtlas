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
import { PropagationEventRecord, ReferenceRecord } from "../models/responses";
import { deriveLanguageFromPath } from "../language";

function withLanguage<T extends { filePath: string }>(record: T): T & { language: ReturnType<typeof deriveLanguageFromPath> } {
  return {
    ...record,
    language: deriveLanguageFromPath(record.filePath),
  };
}

function withFileLanguage<T extends { path: string }>(record: T): T & { language: ReturnType<typeof deriveLanguageFromPath> } {
  return {
    ...record,
    language: deriveLanguageFromPath(record.path),
  };
}

function fixtureSymbols(): Symbol[] {
  return [
    withLanguage({
      id: "Gameplay::Update",
      name: "Update",
      qualifiedName: "Gameplay::Update",
      type: "function",
      filePath: "samples/ambiguity/src/namespace_dupes.h",
      line: 4,
      endLine: 4,
      scopeQualifiedName: "Gameplay",
      scopeKind: "namespace",
      module: "gameplay",
      subsystem: "runtime",
      projectArea: "gameplay",
      artifactKind: "runtime",
    }),
    withLanguage({
      id: "UI::Update",
      name: "Update",
      qualifiedName: "UI::Update",
      type: "function",
      filePath: "samples/ambiguity/src/namespace_dupes.h",
      line: 8,
      endLine: 8,
      scopeQualifiedName: "UI",
      scopeKind: "namespace",
      module: "ui",
      subsystem: "runtime",
      projectArea: "ui",
      artifactKind: "runtime",
    }),
    withLanguage({
      id: "Gameplay",
      name: "Gameplay",
      qualifiedName: "Gameplay",
      type: "namespace",
      filePath: "samples/ambiguity/src/namespace_dupes.h",
      line: 1,
      endLine: 5,
      module: "gameplay",
      subsystem: "runtime",
      projectArea: "gameplay",
      artifactKind: "runtime",
    }),
    withLanguage({
      id: "UI",
      name: "UI",
      qualifiedName: "UI",
      type: "namespace",
      filePath: "samples/ambiguity/src/namespace_dupes.h",
      line: 6,
      endLine: 9,
      module: "ui",
      subsystem: "runtime",
      projectArea: "ui",
      artifactKind: "runtime",
    }),
    withLanguage({
      id: "AI::Controller",
      name: "Controller",
      qualifiedName: "AI::Controller",
      type: "class",
      filePath: "samples/ambiguity/src/namespace_dupes.h",
      line: 12,
      endLine: 15,
      module: "ai",
      subsystem: "runtime",
      projectArea: "ai",
      artifactKind: "runtime",
    }),
    withLanguage({
      id: "AI::Controller::Update",
      name: "Update",
      qualifiedName: "AI::Controller::Update",
      type: "method",
      filePath: "samples/ambiguity/src/namespace_dupes.cpp",
      line: 11,
      endLine: 14,
      parentId: "AI::Controller",
      module: "ai",
      subsystem: "runtime",
      projectArea: "ai",
      artifactKind: "runtime",
    }),
    withLanguage({
      id: "Game::Worker",
      name: "Worker",
      qualifiedName: "Game::Worker",
      type: "class",
      filePath: "samples/ambiguity/src/split_update.h",
      line: 5,
      endLine: 9,
      module: "gameplay",
      subsystem: "runtime",
      projectArea: "gameplay",
      artifactKind: "runtime",
    }),
    withLanguage({
      id: "Game::Worker::Update",
      name: "Update",
      qualifiedName: "Game::Worker::Update",
      type: "method",
      filePath: "samples/ambiguity/src/split_update.cpp",
      line: 5,
      endLine: 5,
      parentId: "Game::Worker",
      module: "gameplay",
      subsystem: "runtime",
      projectArea: "gameplay",
      artifactKind: "runtime",
    }),
    withLanguage({
      id: "Game::Worker::Tick",
      name: "Tick",
      qualifiedName: "Game::Worker::Tick",
      type: "method",
      filePath: "samples/ambiguity/src/split_update.cpp",
      line: 7,
      endLine: 10,
      parentId: "Game::Worker",
      module: "gameplay",
      subsystem: "runtime",
      projectArea: "gameplay",
      artifactKind: "runtime",
    }),
    withLanguage({
      id: "Game::Actor",
      name: "Actor",
      qualifiedName: "Game::Actor",
      type: "class",
      filePath: "samples/ambiguity/src/hierarchy.h",
      line: 1,
      endLine: 6,
      module: "gameplay",
      subsystem: "runtime",
      projectArea: "gameplay",
      artifactKind: "runtime",
    }),
    withLanguage({
      id: "Game::Player",
      name: "Player",
      qualifiedName: "Game::Player",
      type: "class",
      filePath: "samples/ambiguity/src/hierarchy.h",
      line: 8,
      endLine: 12,
      module: "gameplay",
      subsystem: "runtime",
      projectArea: "gameplay",
      artifactKind: "runtime",
    }),
    withLanguage({
      id: "Game::Enemy",
      name: "Enemy",
      qualifiedName: "Game::Enemy",
      type: "class",
      filePath: "samples/ambiguity/src/hierarchy.h",
      line: 14,
      endLine: 18,
      module: "gameplay",
      subsystem: "runtime",
      projectArea: "gameplay",
      artifactKind: "runtime",
    }),
    withLanguage({
      id: "Game::Actor::Tick",
      name: "Tick",
      qualifiedName: "Game::Actor::Tick",
      type: "method",
      filePath: "samples/ambiguity/src/hierarchy.h",
      line: 4,
      endLine: 4,
      parentId: "Game::Actor",
      signature: "virtual void Tick(float dt)",
      parameterCount: 1,
      module: "gameplay",
      subsystem: "runtime",
      projectArea: "gameplay",
      artifactKind: "runtime",
    }),
    withLanguage({
      id: "Game::Player::Tick",
      name: "Tick",
      qualifiedName: "Game::Player::Tick",
      type: "method",
      filePath: "samples/ambiguity/src/hierarchy.h",
      line: 10,
      endLine: 10,
      parentId: "Game::Player",
      signature: "void Tick(float dt)",
      parameterCount: 1,
      module: "gameplay",
      subsystem: "runtime",
      projectArea: "gameplay",
      artifactKind: "runtime",
    }),
    withLanguage({
      id: "Game::Enemy::Tick",
      name: "Tick",
      qualifiedName: "Game::Enemy::Tick",
      type: "method",
      filePath: "samples/ambiguity/src/hierarchy.h",
      line: 16,
      endLine: 16,
      parentId: "Game::Enemy",
      signature: "void Tick(float dt)",
      parameterCount: 1,
      module: "gameplay",
      subsystem: "runtime",
      projectArea: "gameplay",
      artifactKind: "runtime",
    }),
    withLanguage({
      id: "Game::Bootstrap",
      name: "Bootstrap",
      qualifiedName: "Game::Bootstrap",
      type: "function",
      filePath: "samples/ambiguity/src/path_trace.cpp",
      line: 1,
      endLine: 2,
      module: "gameplay",
      subsystem: "runtime",
      projectArea: "gameplay",
      artifactKind: "runtime",
    }),
    withLanguage({
      id: "Game::Dispatch",
      name: "Dispatch",
      qualifiedName: "Game::Dispatch",
      type: "function",
      filePath: "samples/ambiguity/src/path_trace.cpp",
      line: 4,
      endLine: 5,
      module: "gameplay",
      subsystem: "runtime",
      projectArea: "gameplay",
      artifactKind: "runtime",
    }),
    withLanguage({
      id: "Game::ApplyDamage",
      name: "ApplyDamage",
      qualifiedName: "Game::ApplyDamage",
      type: "function",
      filePath: "samples/ambiguity/src/path_trace.cpp",
      line: 7,
      endLine: 8,
      module: "gameplay",
      subsystem: "runtime",
      projectArea: "gameplay",
      artifactKind: "runtime",
    }),
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
    {
      callerId: "Game::Bootstrap",
      calleeId: "Game::Dispatch",
      filePath: "samples/ambiguity/src/path_trace.cpp",
      line: 2,
    },
    {
      callerId: "Game::Dispatch",
      calleeId: "Game::ApplyDamage",
      filePath: "samples/ambiguity/src/path_trace.cpp",
      line: 5,
    },
  ];
}

function fixtureFiles(): FileRecord[] {
  return [
    withFileLanguage({
      path: "samples/ambiguity/src/namespace_dupes.h",
      contentHash: "fixture-namespace-dupes-h",
      lastIndexed: "2026-04-18T00:00:00Z",
      symbolCount: 4,
      module: "gameplay",
      subsystem: "runtime",
      projectArea: "gameplay",
      artifactKind: "runtime",
    }),
    withFileLanguage({
      path: "samples/ambiguity/src/namespace_dupes.cpp",
      contentHash: "fixture-namespace-dupes-cpp",
      lastIndexed: "2026-04-18T00:00:00Z",
      symbolCount: 3,
      module: "ai",
      subsystem: "runtime",
      projectArea: "ai",
      artifactKind: "runtime",
    }),
    withFileLanguage({
      path: "samples/ambiguity/src/split_update.h",
      contentHash: "fixture-split-update-h",
      lastIndexed: "2026-04-18T00:00:00Z",
      symbolCount: 3,
      module: "gameplay",
      subsystem: "runtime",
      projectArea: "gameplay",
      artifactKind: "runtime",
    }),
    withFileLanguage({
      path: "samples/ambiguity/src/split_update.cpp",
      contentHash: "fixture-split-update-cpp",
      lastIndexed: "2026-04-18T00:00:00Z",
      symbolCount: 2,
      module: "gameplay",
      subsystem: "runtime",
      projectArea: "gameplay",
      artifactKind: "runtime",
    }),
    withFileLanguage({
      path: "samples/ambiguity/src/path_trace.cpp",
      contentHash: "fixture-path-trace-cpp",
      lastIndexed: "2026-04-18T00:00:00Z",
      symbolCount: 3,
      module: "gameplay",
      subsystem: "runtime",
      projectArea: "gameplay",
      artifactKind: "runtime",
    }),
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
    {
      sourceSymbolId: "Game::Player",
      targetSymbolId: "Game::Actor",
      category: "inheritanceMention",
      filePath: "samples/ambiguity/src/hierarchy.h",
      line: 8,
      confidence: "partial",
    },
    {
      sourceSymbolId: "Game::Enemy",
      targetSymbolId: "Game::Actor",
      category: "inheritanceMention",
      filePath: "samples/ambiguity/src/hierarchy.h",
      line: 14,
      confidence: "partial",
    },
  ];
}

function fixturePropagationEvents(): PropagationEventRecord[] {
  return [
    {
      ownerSymbolId: "Game::Worker::Update",
      sourceAnchor: {
        anchorId: "Game::Worker::Update::param:value@5",
        anchorKind: "parameter",
      },
      targetAnchor: {
        anchorId: "Game::Worker::Update::field:stored",
        anchorKind: "field",
      },
      propagationKind: "fieldWrite",
      filePath: "samples/ambiguity/src/split_update.cpp",
      line: 5,
      confidence: "high",
      risks: [],
    },
    {
      ownerSymbolId: "Game::Worker::Update",
      sourceAnchor: {
        anchorId: "Game::Worker::Update::field:stored",
        anchorKind: "field",
      },
      targetAnchor: {
        anchorId: "Game::Worker::Update::return@6",
        anchorKind: "returnValue",
      },
      propagationKind: "fieldRead",
      filePath: "samples/ambiguity/src/split_update.cpp",
      line: 6,
      confidence: "high",
      risks: [],
    },
    {
      ownerSymbolId: "Game::Dispatch",
      sourceAnchor: {
        anchorId: "Game::Dispatch::param:value@4",
        anchorKind: "parameter",
      },
      targetAnchor: {
        anchorId: "Game::Dispatch::return@5",
        anchorKind: "returnValue",
      },
      propagationKind: "assignment",
      filePath: "samples/ambiguity/src/path_trace.cpp",
      line: 5,
      confidence: "high",
      risks: [],
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
    propagationEvents: fixturePropagationEvents(),
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
      parent_id TEXT,
      module TEXT,
      subsystem TEXT,
      project_area TEXT,
      artifact_kind TEXT,
      header_role TEXT
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
    CREATE TABLE propagation_events (
      owner_symbol_id TEXT,
      source_anchor_id TEXT,
      source_symbol_id TEXT,
      source_expression_text TEXT,
      source_anchor_kind TEXT NOT NULL,
      target_anchor_id TEXT,
      target_symbol_id TEXT,
      target_expression_text TEXT,
      target_anchor_kind TEXT NOT NULL,
      propagation_kind TEXT NOT NULL,
      file_path TEXT NOT NULL,
      line INTEGER NOT NULL,
      confidence TEXT NOT NULL,
      risks TEXT NOT NULL
    );
  `);

  const insertSymbol = db.prepare(`
    INSERT INTO symbols (
      id, name, qualified_name, type, file_path, line, end_line, signature,
      parameter_count, scope_qualified_name, scope_kind, symbol_role,
      declaration_file_path, declaration_line, declaration_end_line,
      definition_file_path, definition_line, definition_end_line, parent_id,
      module, subsystem, project_area, artifact_kind, header_role
    ) VALUES (
      @id, @name, @qualified_name, @type, @file_path, @line, @end_line, @signature,
      @parameter_count, @scope_qualified_name, @scope_kind, @symbol_role,
      @declaration_file_path, @declaration_line, @declaration_end_line,
      @definition_file_path, @definition_line, @definition_end_line, @parent_id,
      @module, @subsystem, @project_area, @artifact_kind, @header_role
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
  const insertPropagation = db.prepare(`
    INSERT INTO propagation_events (
      owner_symbol_id, source_anchor_id, source_symbol_id, source_expression_text, source_anchor_kind,
      target_anchor_id, target_symbol_id, target_expression_text, target_anchor_kind,
      propagation_kind, file_path, line, confidence, risks
    ) VALUES (
      @owner_symbol_id, @source_anchor_id, @source_symbol_id, @source_expression_text, @source_anchor_kind,
      @target_anchor_id, @target_symbol_id, @target_expression_text, @target_anchor_kind,
      @propagation_kind, @file_path, @line, @confidence, @risks
    )
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
      module: symbol.module ?? null,
      subsystem: symbol.subsystem ?? null,
      project_area: symbol.projectArea ?? null,
      artifact_kind: symbol.artifactKind ?? null,
      header_role: symbol.headerRole ?? null,
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
  for (const event of fixturePropagationEvents()) {
    insertPropagation.run({
      owner_symbol_id: event.ownerSymbolId ?? null,
      source_anchor_id: event.sourceAnchor.anchorId ?? null,
      source_symbol_id: event.sourceAnchor.symbolId ?? null,
      source_expression_text: event.sourceAnchor.expressionText ?? null,
      source_anchor_kind: event.sourceAnchor.anchorKind,
      target_anchor_id: event.targetAnchor.anchorId ?? null,
      target_symbol_id: event.targetAnchor.symbolId ?? null,
      target_expression_text: event.targetAnchor.expressionText ?? null,
      target_anchor_kind: event.targetAnchor.anchorKind,
      propagation_kind: event.propagationKind,
      file_path: event.filePath,
      line: event.line,
      confidence: event.confidence,
      risks: JSON.stringify(event.risks),
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
      expect(typeof heuristic.body.selectedReason).toBe("string");
      expect(typeof heuristic.body.bestNextDiscriminator).toBe("string");
      expect(heuristic.body.suggestedExactQueries).toHaveLength(3);
      expect(heuristic.body.topCandidates).toHaveLength(4);
      expect(heuristic.body.topCandidates[0].qualifiedName).toBe(heuristic.body.symbol.qualifiedName);
      expect(typeof heuristic.body.topCandidates[0].exactQuery).toBe("string");

      const callers = await request(app).get("/callers/Update").expect(200);
      expect(callers.body.lookupMode).toBe("heuristic");
      expect(callers.body.confidence).toBe("ambiguous");
      expect(callers.body.matchReasons).toEqual(["ambiguous_top_score"]);
      expect(callers.body.ambiguity).toEqual({ candidateCount: 4 });
      expect(typeof callers.body.selectedReason).toBe("string");
      expect(typeof callers.body.bestNextDiscriminator).toBe("string");
      expect(callers.body.suggestedExactQueries).toHaveLength(3);
      expect(callers.body.topCandidates).toHaveLength(4);
      expect(callers.body.totalCount).toBe(1);
      expect(callers.body.truncated).toBe(false);
      expect(callers.body.window.totalCount).toBe(1);
      expect(callers.body.window.returnedCount).toBe(1);
      expect(callers.body.callers).toHaveLength(1);
      expect(callers.body.callers[0].qualifiedName).toBe("AI::Controller::Update");
      expect(callers.body.groupedByModule).toEqual([{ key: "ai", count: 1 }]);

      const filteredCallers = await request(app)
        .get("/callers/Update")
        .query({ module: "ai", limit: 10 })
        .expect(200);
      expect(filteredCallers.body.module).toBe("ai");
      expect(filteredCallers.body.callers).toHaveLength(1);
      expect(filteredCallers.body.groupedByModule).toEqual([{ key: "ai", count: 1 }]);

      const overloads = await request(app).get("/overloads/Update").expect(200);
      expect(overloads.body.query).toBe("Update");
      expect(overloads.body.totalCount).toBe(4);
      expect(overloads.body.groupCount).toBe(4);
      expect(overloads.body.groups).toHaveLength(4);
      expect(overloads.body.groups[0]).toHaveProperty("qualifiedName");
      expect(overloads.body.groups[0]).toHaveProperty("candidates");

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
      expect(references.body.groupedByLanguage).toEqual([{ key: "cpp", count: 1 }]);

      const filteredReferences = await request(app)
        .get("/references")
        .query({ qualifiedName: "Gameplay::Update", category: "functionCall", module: "ai" })
        .expect(200);
      expect(filteredReferences.body.module).toBe("ai");
      expect(filteredReferences.body.references).toHaveLength(1);
      expect(filteredReferences.body.groupedByModule).toEqual([{ key: "ai", count: 1 }]);

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

      const filteredImpact = await request(app)
        .get("/impact")
        .query({ qualifiedName: "Gameplay::Update", depth: 2, limit: 10, module: "ai" })
        .expect(200);
      expect(filteredImpact.body.module).toBe("ai");
      expect(filteredImpact.body.directCallers).toHaveLength(1);
      expect(filteredImpact.body.affectedModules).toEqual([{ key: "ai", count: 1 }]);
      expect(filteredImpact.body.affectedLanguages).toEqual([{ key: "cpp", count: 1 }]);

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

      const hierarchy = await request(app)
        .get("/type-hierarchy")
        .query({ qualifiedName: "Game::Actor" })
        .expect(200);
      expect(hierarchy.body.lookupMode).toBe("exact");
      expect(hierarchy.body.directBases).toHaveLength(0);
      expect(hierarchy.body.directDerived.map((item: any) => item.qualifiedName)).toEqual([
        "Game::Enemy",
        "Game::Player",
      ]);

      const baseMethods = await request(app)
        .get("/base-methods")
        .query({ qualifiedName: "Game::Player::Tick" })
        .expect(200);
      expect(baseMethods.body.lookupMode).toBe("exact");
      expect(baseMethods.body.baseMethods).toHaveLength(1);
      expect(baseMethods.body.baseMethods[0].method.qualifiedName).toBe("Game::Actor::Tick");
      expect(baseMethods.body.baseMethods[0].confidence).toBe("high");

      const overrides = await request(app)
        .get("/overrides")
        .query({ qualifiedName: "Game::Actor::Tick" })
        .expect(200);
      expect(overrides.body.lookupMode).toBe("exact");
      expect(overrides.body.overrides).toHaveLength(2);
      expect(overrides.body.overrides.map((item: any) => item.method.qualifiedName)).toEqual([
        "Game::Enemy::Tick",
        "Game::Player::Tick",
      ]);

      const pathTrace = await request(app)
        .get("/trace-call-path")
        .query({
          sourceQualifiedName: "Game::Bootstrap",
          targetQualifiedName: "Game::ApplyDamage",
          maxDepth: 3,
        })
        .expect(200);
      expect(pathTrace.body.pathFound).toBe(true);
      expect(pathTrace.body.steps).toHaveLength(2);
      expect(pathTrace.body.steps[0].callerQualifiedName).toBe("Game::Bootstrap");
      expect(pathTrace.body.steps[1].calleeQualifiedName).toBe("Game::ApplyDamage");

      const propagation = await request(app)
        .get("/symbol-propagation")
        .query({ qualifiedName: "Game::Worker::Update", limit: 10 })
        .expect(200);
      expect(propagation.body.lookupMode).toBe("exact");
      expect(propagation.body.outgoing).toHaveLength(2);
      expect(propagation.body.incoming).toHaveLength(2);
      expect(propagation.body.window.totalCount).toBe(4);
      expect(propagation.body.propagationConfidence).toBe("high");
      expect(propagation.body.riskMarkers).toEqual([]);
      expect(propagation.body.confidenceNotes).toContain(
        "All returned propagation hops come from supported structural patterns without additional risk markers.",
      );
      expect(propagation.body.outgoing[0].propagationKind).toBe("fieldWrite");

      const tracedFlow = await request(app)
        .get("/trace-variable-flow")
        .query({ qualifiedName: "Game::Worker::Update", maxDepth: 3, maxEdges: 10 })
        .expect(200);
      expect(tracedFlow.body.lookupMode).toBe("exact");
      expect(tracedFlow.body.propagationConfidence).toBe("high");
      expect(tracedFlow.body.riskMarkers).toEqual([]);
      expect(tracedFlow.body.pathFound).toBe(true);
      expect(tracedFlow.body.steps).toHaveLength(2);
      expect(tracedFlow.body.steps[0].propagationKind).toBe("fieldWrite");
      expect(tracedFlow.body.steps[1].propagationKind).toBe("fieldRead");

      const metadataSearch = await request(app)
        .get("/search")
        .query({ q: "Update", module: "ui" })
        .expect(200);
      expect(metadataSearch.body.module).toBe("ui");
      expect(metadataSearch.body.results).toHaveLength(1);
      expect(metadataSearch.body.results[0].qualifiedName).toBe("UI::Update");
      expect(metadataSearch.body.groupedByLanguage).toEqual([{ key: "cpp", count: 1 }]);

      const workspaceSummary = await request(app)
        .get("/workspace-summary")
        .expect(200);
      expect(workspaceSummary.body.totalFiles).toBe(5);
      expect(workspaceSummary.body.totalSymbols).toBe(18);
      expect(workspaceSummary.body.languages).toEqual([{ language: "cpp", fileCount: 5, symbolCount: 18 }]);
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
      expect(typeof heuristic.selectedReason).toBe("string");
      expect(typeof heuristic.bestNextDiscriminator).toBe("string");
      expect(heuristic.suggestedExactQueries).toHaveLength(3);
      expect(heuristic.topCandidates).toHaveLength(4);
      expect(heuristic.topCandidates[0].qualifiedName).toBe(heuristic.symbol.qualifiedName);
      expect(typeof heuristic.topCandidates[0].exactQuery).toBe("string");

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
      expect(typeof callers.selectedReason).toBe("string");
      expect(typeof callers.bestNextDiscriminator).toBe("string");
      expect(callers.suggestedExactQueries).toHaveLength(3);
      expect(callers.topCandidates).toHaveLength(4);
      expect(callers.totalCount).toBe(1);
      expect(callers.truncated).toBe(false);
      expect(callers.window.totalCount).toBe(1);
      expect(callers.window.returnedCount).toBe(1);
      expect(callers.callers).toHaveLength(1);
      expect(callers.callers[0].qualifiedName).toBe("AI::Controller::Update");
      expect(callers.module).toBeUndefined();
      expect(callers.groupedByModule).toEqual([{ key: "ai", count: 1 }]);

      const filteredCallerResponses = await mcpCall([
        INIT,
        INITIALIZED,
        {
          jsonrpc: "2.0",
          id: 14,
          method: "tools/call",
          params: { name: "find_callers", arguments: { name: "Update", limit: 10, module: "ai" } },
        },
      ], dir);
      const filteredCallers = JSON.parse(filteredCallerResponses.find((r) => r.id === 14).result.content[0].text);
      expect(filteredCallers.module).toBe("ai");
      expect(filteredCallers.callers).toHaveLength(1);
      expect(filteredCallers.groupedByModule).toEqual([{ key: "ai", count: 1 }]);

      const overloadResponses = await mcpCall([
        INIT,
        INITIALIZED,
        {
          jsonrpc: "2.0",
          id: 15,
          method: "tools/call",
          params: { name: "find_all_overloads", arguments: { name: "Update" } },
        },
      ], dir);
      const overloads = JSON.parse(overloadResponses.find((r) => r.id === 15).result.content[0].text);
      expect(overloads.query).toBe("Update");
      expect(overloads.totalCount).toBe(4);
      expect(overloads.groupCount).toBe(4);
      expect(overloads.groups).toHaveLength(4);

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

      const filteredReferenceResponses = await mcpCall([
        INIT,
        INITIALIZED,
        {
          jsonrpc: "2.0",
          id: 15,
          method: "tools/call",
          params: {
            name: "find_references",
            arguments: { qualifiedName: "Gameplay::Update", category: "functionCall", module: "ai", limit: 10 },
          },
        },
      ], dir);
      const filteredReferences = JSON.parse(filteredReferenceResponses.find((r) => r.id === 15).result.content[0].text);
      expect(filteredReferences.module).toBe("ai");
      expect(filteredReferences.references).toHaveLength(1);
      expect(filteredReferences.groupedByModule).toEqual([{ key: "ai", count: 1 }]);

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

      const filteredImpactResponses = await mcpCall([
        INIT,
        INITIALIZED,
        {
          jsonrpc: "2.0",
          id: 16,
          method: "tools/call",
          params: {
            name: "impact_analysis",
            arguments: { qualifiedName: "Gameplay::Update", depth: 2, limit: 10, module: "ai" },
          },
        },
      ], dir);
      const filteredImpact = JSON.parse(filteredImpactResponses.find((r) => r.id === 16).result.content[0].text);
      expect(filteredImpact.module).toBe("ai");
      expect(filteredImpact.directCallers).toHaveLength(1);
      expect(filteredImpact.affectedModules).toEqual([{ key: "ai", count: 1 }]);
      expect(filteredImpact.affectedLanguages).toEqual([{ key: "cpp", count: 1 }]);

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

      const hierarchyResponses = await mcpCall([
        INIT,
        INITIALIZED,
        {
          jsonrpc: "2.0",
          id: 10,
          method: "tools/call",
          params: { name: "get_type_hierarchy", arguments: { qualifiedName: "Game::Actor" } },
        },
      ], dir);
      const hierarchy = JSON.parse(hierarchyResponses.find((r) => r.id === 10).result.content[0].text);
      expect(hierarchy.lookupMode).toBe("exact");
      expect(hierarchy.directDerived.map((item: any) => item.qualifiedName)).toEqual([
        "Game::Enemy",
        "Game::Player",
      ]);

      const baseMethodResponses = await mcpCall([
        INIT,
        INITIALIZED,
        {
          jsonrpc: "2.0",
          id: 11,
          method: "tools/call",
          params: { name: "find_base_methods", arguments: { qualifiedName: "Game::Player::Tick" } },
        },
      ], dir);
      const baseMethods = JSON.parse(baseMethodResponses.find((r) => r.id === 11).result.content[0].text);
      expect(baseMethods.baseMethods).toHaveLength(1);
      expect(baseMethods.baseMethods[0].method.qualifiedName).toBe("Game::Actor::Tick");
      expect(baseMethods.baseMethods[0].confidence).toBe("high");

      const overrideResponses = await mcpCall([
        INIT,
        INITIALIZED,
        {
          jsonrpc: "2.0",
          id: 12,
          method: "tools/call",
          params: { name: "find_overrides", arguments: { qualifiedName: "Game::Actor::Tick" } },
        },
      ], dir);
      const overrides = JSON.parse(overrideResponses.find((r) => r.id === 12).result.content[0].text);
      expect(overrides.overrides).toHaveLength(2);
      expect(overrides.overrides.map((item: any) => item.method.qualifiedName)).toEqual([
        "Game::Enemy::Tick",
        "Game::Player::Tick",
      ]);

      const pathResponses = await mcpCall([
        INIT,
        INITIALIZED,
        {
          jsonrpc: "2.0",
          id: 13,
          method: "tools/call",
          params: {
            name: "trace_call_path",
            arguments: {
              sourceQualifiedName: "Game::Bootstrap",
              targetQualifiedName: "Game::ApplyDamage",
              maxDepth: 3,
            },
          },
        },
      ], dir);
      const pathTrace = JSON.parse(pathResponses.find((r) => r.id === 13).result.content[0].text);
      expect(pathTrace.pathFound).toBe(true);
      expect(pathTrace.steps).toHaveLength(2);
      expect(pathTrace.steps[0].callerQualifiedName).toBe("Game::Bootstrap");
      expect(pathTrace.steps[1].calleeQualifiedName).toBe("Game::ApplyDamage");

      const propagationResponses = await mcpCall([
        INIT,
        INITIALIZED,
        {
          jsonrpc: "2.0",
          id: 18,
          method: "tools/call",
          params: { name: "explain_symbol_propagation", arguments: { qualifiedName: "Game::Worker::Update", limit: 10 } },
        },
      ], dir);
      const propagation = JSON.parse(propagationResponses.find((r) => r.id === 18).result.content[0].text);
      expect(propagation.lookupMode).toBe("exact");
      expect(propagation.outgoing).toHaveLength(2);
      expect(propagation.incoming).toHaveLength(2);
      expect(propagation.window.totalCount).toBe(4);
      expect(propagation.propagationConfidence).toBe("high");
      expect(propagation.riskMarkers).toEqual([]);

      const traceResponses = await mcpCall([
        INIT,
        INITIALIZED,
        {
          jsonrpc: "2.0",
          id: 19,
          method: "tools/call",
          params: {
            name: "trace_variable_flow",
            arguments: { qualifiedName: "Game::Worker::Update", maxDepth: 3, maxEdges: 10 },
          },
        },
      ], dir);
      const tracedFlow = JSON.parse(traceResponses.find((r) => r.id === 19).result.content[0].text);
      expect(tracedFlow.propagationConfidence).toBe("high");
      expect(tracedFlow.riskMarkers).toEqual([]);
      expect(tracedFlow.pathFound).toBe(true);
      expect(tracedFlow.steps).toHaveLength(2);
      expect(tracedFlow.steps[0].propagationKind).toBe("fieldWrite");
      expect(tracedFlow.steps[1].propagationKind).toBe("fieldRead");

      const searchResponses = await mcpCall([
        INIT,
        INITIALIZED,
        {
          jsonrpc: "2.0",
          id: 17,
          method: "tools/call",
          params: { name: "search_symbols", arguments: { query: "Update", module: "ui", limit: 10 } },
        },
      ], dir);
      const metadataSearch = JSON.parse(searchResponses.find((r) => r.id === 17).result.content[0].text);
      expect(metadataSearch.module).toBe("ui");
      expect(metadataSearch.results).toHaveLength(1);
      expect(metadataSearch.results[0].qualifiedName).toBe("UI::Update");
      expect(metadataSearch.groupedByLanguage).toEqual([{ key: "cpp", count: 1 }]);

      const workspaceSummaryResponses = await mcpCall([
        INIT,
        INITIALIZED,
        {
          jsonrpc: "2.0",
          id: 20,
          method: "tools/call",
          params: { name: "workspace_summary", arguments: {} },
        },
      ], dir);
      const workspaceSummary = JSON.parse(workspaceSummaryResponses.find((r) => r.id === 20).result.content[0].text);
      expect(workspaceSummary.totalFiles).toBe(5);
      expect(workspaceSummary.totalSymbols).toBe(18);
      expect(workspaceSummary.languages).toEqual([{ language: "cpp", fileCount: 5, symbolCount: 18 }]);
    } finally {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  });
});
