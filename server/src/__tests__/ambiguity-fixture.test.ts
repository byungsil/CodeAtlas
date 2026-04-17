import * as fs from "fs";
import * as os from "os";
import * as path from "path";
import Database from "better-sqlite3";
import request from "supertest";
import { execFileSync } from "child_process";
import { createApp } from "../app";
import { JsonStore } from "../storage/json-store";
import { SqliteStore } from "../storage/sqlite-store";
import { Symbol } from "../models/symbol";
import { Call } from "../models/call";
import { FileRecord } from "../models/file-record";

const MCP_SCRIPT = path.resolve(__dirname, "../mcp.ts");

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
    },
    {
      id: "UI::Update",
      name: "Update",
      qualifiedName: "UI::Update",
      type: "function",
      filePath: "samples/ambiguity/src/namespace_dupes.h",
      line: 8,
      endLine: 8,
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

function writeFixtureJsonStore(): { dir: string; store: JsonStore } {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "codeatlas-ambiguity-json-"));
  const store = new JsonStore(dir);
  store.save({
    symbols: fixtureSymbols(),
    calls: fixtureCalls(),
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

  db.pragma("journal_mode = WAL");
  db.close();
  return dbPath;
}

function mcpCallWithDataDir(dataDir: string, messages: object[]): any[] {
  const input = messages.map((m) => JSON.stringify(m)).join("\n") + "\n";
  const output = execFileSync(process.execPath, ["-r", "ts-node/register", MCP_SCRIPT, dataDir], {
    input,
    timeout: 15000,
    cwd: path.resolve(__dirname, "../.."),
    encoding: "utf-8",
  });
  return output
    .trim()
    .split("\n")
    .map((line) => JSON.parse(line));
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
    } finally {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  });

  it("MCP exact lookup and heuristic ambiguity follow the fixture contract", () => {
    const { dir } = writeFixtureJsonStore();
    try {
      const exactResponses = mcpCallWithDataDir(dir, [
        INIT,
        INITIALIZED,
        {
          jsonrpc: "2.0",
          id: 2,
          method: "tools/call",
          params: { name: "lookup_symbol", arguments: { qualifiedName: "Gameplay::Update" } },
        },
      ]);
      const exact = JSON.parse(exactResponses.find((r) => r.id === 2).result.content[0].text);
      expect(exact.lookupMode).toBe("exact");
      expect(exact.confidence).toBe("exact");
      expect(exact.matchReasons).toEqual(["exact_qualified_name_match"]);
      expect(exact.symbol.qualifiedName).toBe("Gameplay::Update");

      const heuristicResponses = mcpCallWithDataDir(dir, [
        INIT,
        INITIALIZED,
        {
          jsonrpc: "2.0",
          id: 3,
          method: "tools/call",
          params: { name: "lookup_function", arguments: { name: "Update" } },
        },
      ]);
      const heuristic = JSON.parse(heuristicResponses.find((r) => r.id === 3).result.content[0].text);
      expect(heuristic.lookupMode).toBe("heuristic");
      expect(heuristic.confidence).toBe("ambiguous");
      expect(heuristic.matchReasons).toEqual(["ambiguous_top_score"]);
      expect(heuristic.ambiguity).toEqual({ candidateCount: 4 });
    } finally {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  });
});
