import request from "supertest";
import { createApp } from "../app";
import { SqliteStore } from "../storage/sqlite-store";
import * as path from "path";
import { recordMcpToolCall, resetMcpRuntimeStatsForTests } from "../runtime-stats";

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

beforeEach(() => {
  resetMcpRuntimeStatsForTests();
});

describe("Dashboard static serving", () => {
  it("serves index.html at /dashboard/", async () => {
    const res = await request(app).get("/dashboard/").expect(200);
    expect(res.text).toContain("CodeAtlas");
    expect(res.text).toContain("<!DOCTYPE html>");
  });

  it("serves index.html at /dashboard/index.html", async () => {
    const res = await request(app).get("/dashboard/index.html").expect(200);
    expect(res.headers["content-type"]).toContain("text/html");
  });
});

describe("Dashboard consumes same API contracts as agents", () => {
  it("dashboard overview exposes index details and MCP runtime stats", async () => {
    recordMcpToolCall({ toolName: "lookup_symbol", elapsedMs: 12, ok: true });
    recordMcpToolCall({ toolName: "find_references", elapsedMs: 28, ok: false, errorMessage: "boom" });

    const res = await request(app).get("/dashboard/api/overview").expect(200);
    expect(res.body).toHaveProperty("generatedAt");
    expect(res.body).toHaveProperty("workspace");
    expect(res.body).toHaveProperty("index");
    expect(res.body).toHaveProperty("mcp");
    expect(res.body.index).toHaveProperty("counts");
    expect(res.body.index).toHaveProperty("fileRiskCounts");
    expect(res.body.index).toHaveProperty("backend");
    expect(res.body.mcp.totalToolCalls).toBe(2);
    expect(res.body.mcp.totalErrors).toBe(1);
    expect(Array.isArray(res.body.mcp.tools)).toBe(true);
    expect(Array.isArray(res.body.mcp.recentCalls)).toBe(true);
  });

  it("search endpoint returns same shape for dashboard and MCP", async () => {
    const res = await request(app).get("/search?q=Update").expect(200);
    expect(res.body).toHaveProperty("query");
    expect(res.body).toHaveProperty("window");
    expect(res.body).toHaveProperty("results");
    expect(res.body).toHaveProperty("totalCount");
    expect(res.body).toHaveProperty("truncated");
  });

  it("function endpoint returns same shape", async () => {
    const res = await request(app).get("/function/UpdateAI").expect(200);
    expect(res.body).toHaveProperty("symbol");
    expect(res.body).toHaveProperty("callers");
    expect(res.body).toHaveProperty("callees");
  });

  it("class endpoint returns same shape", async () => {
    const res = await request(app).get("/class/GameObject").expect(200);
    expect(res.body).toHaveProperty("symbol");
    expect(res.body).toHaveProperty("members");
  });

  it("callgraph endpoint returns same shape", async () => {
    const res = await request(app).get("/callgraph/UpdateAI").expect(200);
    expect(res.body).toHaveProperty("root");
    expect(res.body).toHaveProperty("depth");
    expect(res.body).toHaveProperty("truncated");
  });

  it("overview endpoints expose compact summary-first payloads", async () => {
    const fileSymbols = await request(app)
      .get("/file-symbols")
      .query({ filePath: "src/game_object.h", limit: 3 })
      .expect(200);
    expect(fileSymbols.body).toHaveProperty("summary");
    expect(fileSymbols.body).toHaveProperty("window");
    expect(fileSymbols.body.window.limitApplied).toBe(3);

    const classMembers = await request(app)
      .get("/class-members")
      .query({ qualifiedName: "Game::GameObject", limit: 3 })
      .expect(200);
    expect(classMembers.body).toHaveProperty("summary");
    expect(classMembers.body).toHaveProperty("window");
    expect(classMembers.body.window.limitApplied).toBe(3);
  });
});

describe("Dashboard handles edge cases", () => {
  it("handles missing symbol gracefully", async () => {
    const res = await request(app).get("/function/NonExistent").expect(404);
    expect(res.body.code).toBe("NOT_FOUND");
  });

  it("handles partial results", async () => {
    const res = await request(app).get("/search?q=Game&limit=1").expect(200);
    expect(res.body.truncated).toBe(true);
  });

  it("handles empty search", async () => {
    const res = await request(app).get("/search?q=a").expect(200);
    expect(res.body.results).toEqual([]);
  });
});
