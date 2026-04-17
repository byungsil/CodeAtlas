import * as path from "path";
import { SqliteStore } from "../storage/sqlite-store";

const DB_PATH = path.resolve(__dirname, "../../../samples/.codeatlas/index.db");

describe("SqliteStore exact lookup", () => {
  let store: SqliteStore;

  beforeAll(() => {
    store = new SqliteStore(DB_PATH);
  });

  afterAll(() => {
    store.close();
  });

  it("returns a symbol by canonical qualified name", () => {
    const symbol = store.getSymbolByQualifiedName("Game::GameObject::Update");
    expect(symbol).toBeDefined();
    expect(symbol?.id).toBe("Game::GameObject::Update");
    expect(symbol?.qualifiedName).toBe("Game::GameObject::Update");
    expect(symbol?.name).toBe("Update");
    expect(symbol?.type).toBe("method");
  });

  it("matches getSymbolById for the same exact identity", () => {
    const byId = store.getSymbolById("Game::GameObject::Update");
    const byQualifiedName = store.getSymbolByQualifiedName("Game::GameObject::Update");
    expect(byQualifiedName).toEqual(byId);
  });

  it("returns undefined for an unknown qualified name", () => {
    const symbol = store.getSymbolByQualifiedName("Game::DoesNotExist::Update");
    expect(symbol).toBeUndefined();
  });
});
