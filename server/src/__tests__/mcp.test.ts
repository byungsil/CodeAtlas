import { execFileSync } from "child_process";
import * as path from "path";

const MCP_SCRIPT = path.resolve(__dirname, "../mcp.ts");
const DATA_DIR = path.resolve(__dirname, "../../../samples/.codeatlas");

function mcpCall(messages: object[]): any[] {
  const input = messages.map((m) => JSON.stringify(m)).join("\n") + "\n";
  const output = execFileSync(process.execPath, ["-r", "ts-node/register", MCP_SCRIPT, DATA_DIR], {
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

const INIT = { jsonrpc: "2.0", id: 1, method: "initialize", params: { protocolVersion: "2024-11-05", capabilities: {}, clientInfo: { name: "test", version: "1.0" } } };
const INITIALIZED = { jsonrpc: "2.0", method: "notifications/initialized" };

describe("MCP payload contracts", () => {
  it("tools/list returns 4 tools", () => {
    const responses = mcpCall([INIT, INITIALIZED, { jsonrpc: "2.0", id: 2, method: "tools/list" }]);
    const toolList = responses.find((r) => r.id === 2);
    expect(toolList).toBeDefined();
    expect(toolList.result.tools).toHaveLength(4);
    const names = toolList.result.tools.map((t: any) => t.name).sort();
    expect(names).toEqual(["get_callgraph", "lookup_class", "lookup_function", "search_symbols"]);
  });

  it("lookup_function returns correct shape", () => {
    const responses = mcpCall([
      INIT, INITIALIZED,
      { jsonrpc: "2.0", id: 3, method: "tools/call", params: { name: "lookup_function", arguments: { name: "UpdateAI" } } },
    ]);
    const res = responses.find((r) => r.id === 3);
    expect(res).toBeDefined();
    expect(res.result.isError).toBeUndefined();
    const payload = JSON.parse(res.result.content[0].text);
    expect(payload.symbol).toBeDefined();
    expect(payload.symbol.name).toBe("UpdateAI");
    expect(payload.callers).toBeInstanceOf(Array);
    expect(payload.callees).toBeInstanceOf(Array);
  });

  it("lookup_class returns correct shape", () => {
    const responses = mcpCall([
      INIT, INITIALIZED,
      { jsonrpc: "2.0", id: 3, method: "tools/call", params: { name: "lookup_class", arguments: { name: "GameObject" } } },
    ]);
    const res = responses.find((r) => r.id === 3);
    const payload = JSON.parse(res.result.content[0].text);
    expect(payload.symbol.name).toBe("GameObject");
    expect(payload.members).toBeInstanceOf(Array);
    expect(payload.members.length).toBeGreaterThan(0);
  });

  it("search_symbols returns correct shape", () => {
    const responses = mcpCall([
      INIT, INITIALIZED,
      { jsonrpc: "2.0", id: 3, method: "tools/call", params: { name: "search_symbols", arguments: { query: "Update" } } },
    ]);
    const res = responses.find((r) => r.id === 3);
    const payload = JSON.parse(res.result.content[0].text);
    expect(payload.query).toBe("Update");
    expect(payload.results).toBeInstanceOf(Array);
    expect(payload).toHaveProperty("totalCount");
    expect(payload).toHaveProperty("truncated");
  });

  it("get_callgraph returns correct shape", () => {
    const responses = mcpCall([
      INIT, INITIALIZED,
      { jsonrpc: "2.0", id: 3, method: "tools/call", params: { name: "get_callgraph", arguments: { name: "UpdateAI" } } },
    ]);
    const res = responses.find((r) => r.id === 3);
    const payload = JSON.parse(res.result.content[0].text);
    expect(payload.root).toBeDefined();
    expect(payload.root.symbol.name).toBe("UpdateAI");
    expect(payload.root.callees).toBeInstanceOf(Array);
    expect(payload).toHaveProperty("depth");
    expect(payload).toHaveProperty("maxDepth");
    expect(payload).toHaveProperty("truncated");
  });

  it("NOT_FOUND returns isError", () => {
    const responses = mcpCall([
      INIT, INITIALIZED,
      { jsonrpc: "2.0", id: 3, method: "tools/call", params: { name: "lookup_function", arguments: { name: "DoesNotExist" } } },
    ]);
    const res = responses.find((r) => r.id === 3);
    expect(res.result.isError).toBe(true);
    const payload = JSON.parse(res.result.content[0].text);
    expect(payload.code).toBe("NOT_FOUND");
  });
});
