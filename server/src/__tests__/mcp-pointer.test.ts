import * as fs from "fs";
import * as os from "os";
import * as path from "path";
import { mcpCall } from "./mcp-test-helpers";

const SAMPLE_DB_PATH = path.resolve(__dirname, "../../../samples/.codeatlas/index.db");
const INIT = { jsonrpc: "2.0", id: 1, method: "initialize", params: { protocolVersion: "2024-11-05", capabilities: {}, clientInfo: { name: "test", version: "1.0" } } };
const INITIALIZED = { jsonrpc: "2.0", method: "notifications/initialized" };

describe("MCP pointer-backed SQLite startup", () => {
  it("resolves workspace_summary through current-db.json without legacy index.db", async () => {
    const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), "codeatlas-mcp-pointer-"));
    const dataDir = path.join(tempRoot, ".codeatlas");
    fs.mkdirSync(dataDir, { recursive: true });

    const generationName = "index-20260420T120000000Z.db";
    fs.copyFileSync(SAMPLE_DB_PATH, path.join(dataDir, generationName));
    fs.writeFileSync(
      path.join(dataDir, "current-db.json"),
      JSON.stringify({
        active_db_filename: generationName,
        published_at: "2026-04-20T12:00:00Z",
        format_version: 1,
      }, null, 2),
    );

    const responses = await mcpCall([
      INIT,
      INITIALIZED,
      { jsonrpc: "2.0", id: 2, method: "tools/call", params: { name: "workspace_summary", arguments: {} } },
    ], dataDir);

    const response = responses.find((entry) => entry.id === 2);
    expect(response).toBeDefined();
    const payload = JSON.parse(response.result.content[0].text);
    expect(payload.totalFiles).toBeGreaterThan(0);
    expect(payload.totalSymbols).toBeGreaterThan(0);

    fs.rmSync(tempRoot, { recursive: true, force: true });
  });
});
