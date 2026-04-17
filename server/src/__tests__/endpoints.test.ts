import request from "supertest";
import { createApp } from "../app";
import { SqliteStore } from "../storage/sqlite-store";
import * as path from "path";

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

describe("GET /function/:name", () => {
  it("returns symbol with callers and callees", async () => {
    const res = await request(app).get("/function/UpdateAI").expect(200);
    expect(res.body.symbol).toBeDefined();
    expect(res.body.symbol.name).toBe("UpdateAI");
    expect(res.body.symbol.type).toBe("method");
    expect(res.body.callers).toBeInstanceOf(Array);
    expect(res.body.callees).toBeInstanceOf(Array);
    expect(res.body.callees.length).toBeGreaterThan(0);
  });

  it("returns 404 for unknown symbol", async () => {
    const res = await request(app).get("/function/NonExistentXYZ").expect(404);
    expect(res.body.code).toBe("NOT_FOUND");
  });
});

describe("GET /class/:name", () => {
  it("returns class with members", async () => {
    const res = await request(app).get("/class/GameObject").expect(200);
    expect(res.body.symbol).toBeDefined();
    expect(res.body.symbol.name).toBe("GameObject");
    expect(res.body.symbol.type).toBe("class");
    expect(res.body.members).toBeInstanceOf(Array);
    expect(res.body.members.length).toBeGreaterThan(0);
  });

  it("returns 404 for unknown class", async () => {
    const res = await request(app).get("/class/FakeClass").expect(404);
    expect(res.body.code).toBe("NOT_FOUND");
  });
});

describe("GET /search", () => {
  it("returns matching symbols", async () => {
    const res = await request(app).get("/search?q=Update").expect(200);
    expect(res.body.query).toBe("Update");
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
  });

  it("returns empty for two-char query", async () => {
    const res = await request(app).get("/search?q=Ga").expect(200);
    expect(res.body.results).toEqual([]);
    expect(res.body.totalCount).toBe(0);
  });

  it("respects limit parameter", async () => {
    const res = await request(app).get("/search?q=Game&limit=2").expect(200);
    expect(res.body.results.length).toBeLessThanOrEqual(2);
  });

  it("marks truncated when more results exist", async () => {
    const res = await request(app).get("/search?q=Game&limit=1").expect(200);
    expect(res.body.truncated).toBe(true);
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
