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
  it("tools/list returns 5 tools", () => {
    const responses = mcpCall([INIT, INITIALIZED, { jsonrpc: "2.0", id: 2, method: "tools/list" }]);
    const toolList = responses.find((r) => r.id === 2);
    expect(toolList).toBeDefined();
    expect(toolList.result.tools).toHaveLength(5);
    const names = toolList.result.tools.map((t: any) => t.name).sort();
    expect(names).toEqual(["get_callgraph", "lookup_class", "lookup_function", "lookup_symbol", "search_symbols"]);
  });

  it("lookup_symbol returns exact response by id", () => {
    const responses = mcpCall([
      INIT, INITIALIZED,
      {
        jsonrpc: "2.0",
        id: 3,
        method: "tools/call",
        params: { name: "lookup_symbol", arguments: { id: "Game::AIComponent::UpdateAI" } },
      },
    ]);
    const res = responses.find((r) => r.id === 3);
    expect(res).toBeDefined();
    expect(res.result.isError).toBeUndefined();
    const payload = JSON.parse(res.result.content[0].text);
    expect(payload.lookupMode).toBe("exact");
    expect(payload.confidence).toBe("exact");
    expect(payload.matchReasons).toEqual(["exact_id_match"]);
    expect(payload.symbol.qualifiedName).toBe("Game::AIComponent::UpdateAI");
    expect(payload.callers).toBeInstanceOf(Array);
    expect(payload.callees).toBeInstanceOf(Array);
  });

  it("lookup_symbol returns exact response by qualifiedName", () => {
    const responses = mcpCall([
      INIT, INITIALIZED,
      {
        jsonrpc: "2.0",
        id: 3,
        method: "tools/call",
        params: { name: "lookup_symbol", arguments: { qualifiedName: "Game::GameObject" } },
      },
    ]);
    const res = responses.find((r) => r.id === 3);
    const payload = JSON.parse(res.result.content[0].text);
    expect(payload.lookupMode).toBe("exact");
    expect(payload.confidence).toBe("exact");
    expect(payload.matchReasons).toEqual(["exact_qualified_name_match"]);
    expect(payload.symbol.qualifiedName).toBe("Game::GameObject");
    expect(payload.members).toBeInstanceOf(Array);
    expect(payload.members.length).toBeGreaterThan(0);
  });

  it("lookup_symbol rejects mismatched id and qualifiedName", () => {
    const responses = mcpCall([
      INIT, INITIALIZED,
      {
        jsonrpc: "2.0",
        id: 3,
        method: "tools/call",
        params: {
          name: "lookup_symbol",
          arguments: {
            id: "Game::AIComponent::UpdateAI",
            qualifiedName: "Game::GameObject",
          },
        },
      },
    ]);
    const res = responses.find((r) => r.id === 3);
    expect(res.result.isError).toBe(true);
    const payload = JSON.parse(res.result.content[0].text);
    expect(payload.code).toBe("BAD_REQUEST");
    expect(payload.error).toBe("Invalid exact lookup request");
  });

  it("lookup_symbol rejects missing exact lookup arguments", () => {
    const responses = mcpCall([
      INIT, INITIALIZED,
      {
        jsonrpc: "2.0",
        id: 3,
        method: "tools/call",
        params: { name: "lookup_symbol", arguments: {} },
      },
    ]);
    const res = responses.find((r) => r.id === 3);
    expect(res.result.isError).toBe(true);
    const payload = JSON.parse(res.result.content[0].text);
    expect(payload.code).toBe("BAD_REQUEST");
    expect(payload.error).toBe("Invalid exact lookup request");
  });

  it("lookup_symbol returns NOT_FOUND for unknown exact symbol", () => {
    const responses = mcpCall([
      INIT, INITIALIZED,
      {
        jsonrpc: "2.0",
        id: 3,
        method: "tools/call",
        params: { name: "lookup_symbol", arguments: { id: "Game::DoesNotExist" } },
      },
    ]);
    const res = responses.find((r) => r.id === 3);
    expect(res.result.isError).toBe(true);
    const payload = JSON.parse(res.result.content[0].text);
    expect(payload.code).toBe("NOT_FOUND");
    expect(payload.error).toBe("Symbol not found");
    expect(payload.symbol).toBeUndefined();
    expect(payload.confidence).toBeUndefined();
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
    expect(payload.lookupMode).toBe("heuristic");
    expect(payload.confidence).toBe("high_confidence_heuristic");
    expect(payload.matchReasons).toEqual([]);
    expect(payload.callers).toBeInstanceOf(Array);
    expect(payload.callees).toBeInstanceOf(Array);
    for (const ref of [...payload.callers, ...payload.callees]) {
      expect(ref.confidence).toBe("high_confidence_heuristic");
      expect(ref.matchReasons).toEqual([]);
    }
  });

  it("lookup_class returns correct shape", () => {
    const responses = mcpCall([
      INIT, INITIALIZED,
      { jsonrpc: "2.0", id: 3, method: "tools/call", params: { name: "lookup_class", arguments: { name: "GameObject" } } },
    ]);
    const res = responses.find((r) => r.id === 3);
    const payload = JSON.parse(res.result.content[0].text);
    expect(payload.symbol.name).toBe("GameObject");
    expect(payload.lookupMode).toBe("heuristic");
    expect(payload.confidence).toBe("high_confidence_heuristic");
    expect(payload.matchReasons).toEqual([]);
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
    expect(payload.error).toBe("Symbol not found");
    expect(payload.symbol).toBeUndefined();
    expect(payload.confidence).toBeUndefined();
  });
});
