import * as path from "path";
import { SqliteStore } from "../storage/sqlite-store";
import { JsonStore } from "../storage/json-store";

const CODEATLAS_DIR = path.resolve(__dirname, "../../../samples/.codeatlas");
const DB_PATH = path.join(CODEATLAS_DIR, "index.db");

describe("SQLite-JSON compatibility", () => {
  let sqlite: SqliteStore;
  let json: JsonStore;

  beforeAll(() => {
    sqlite = new SqliteStore(DB_PATH);
    json = new JsonStore(CODEATLAS_DIR);
  });

  afterAll(() => {
    sqlite.close();
  });

  it("getSymbolsByName returns same symbols", () => {
    const sqlResult = sqlite.getSymbolsByName("UpdateAI");
    const jsonResult = json.getSymbolsByName("UpdateAI");
    expect(sqlResult.length).toBe(jsonResult.length);
    expect(sqlResult[0].id).toBe(jsonResult[0].id);
    expect(sqlResult[0].name).toBe(jsonResult[0].name);
    expect(sqlResult[0].type).toBe(jsonResult[0].type);
  });

  it("getSymbolById returns same symbol", () => {
    const sqlResult = sqlite.getSymbolById("Game::AIComponent::UpdateAI");
    const jsonResult = json.getSymbolById("Game::AIComponent::UpdateAI");
    expect(sqlResult).toBeDefined();
    expect(jsonResult).toBeDefined();
    expect(sqlResult!.id).toBe(jsonResult!.id);
    expect(sqlResult!.filePath).toBe(jsonResult!.filePath);
    expect(sqlResult!.line).toBe(jsonResult!.line);
  });

  it("searchSymbols returns same result count", () => {
    const sqlResult = sqlite.searchSymbols("Update", undefined, 50);
    const jsonResult = json.searchSymbols("Update", undefined, 50);
    expect(sqlResult.results.length).toBe(jsonResult.results.length);
  });

  it("getCallers returns same callers", () => {
    const sqlResult = sqlite.getCallers("Game::AIComponent::UpdateAI");
    const jsonResult = json.getCallers("Game::AIComponent::UpdateAI");
    expect(sqlResult.length).toBe(jsonResult.length);
  });

  it("getCallees returns same callees", () => {
    const sqlResult = sqlite.getCallees("Game::AIComponent::UpdateAI");
    const jsonResult = json.getCallees("Game::AIComponent::UpdateAI");
    expect(sqlResult.length).toBe(jsonResult.length);
    const sqlIds = sqlResult.map(c => c.calleeId).sort();
    const jsonIds = jsonResult.map(c => c.calleeId).sort();
    expect(sqlIds).toEqual(jsonIds);
  });

  it("getMembers returns same members", () => {
    const sqlResult = sqlite.getMembers("Game::GameObject");
    const jsonResult = json.getMembers("Game::GameObject");
    expect(sqlResult.length).toBe(jsonResult.length);
    const sqlNames = sqlResult.map(s => s.name).sort();
    const jsonNames = jsonResult.map(s => s.name).sort();
    expect(sqlNames).toEqual(jsonNames);
  });
});
