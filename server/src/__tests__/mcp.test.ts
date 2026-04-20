import * as path from "path";
import { mcpCall } from "./mcp-test-helpers";

const DATA_DIR = path.resolve(__dirname, "../../../samples/.codeatlas");

const INIT = { jsonrpc: "2.0", id: 1, method: "initialize", params: { protocolVersion: "2024-11-05", capabilities: {}, clientInfo: { name: "test", version: "1.0" } } };
const INITIALIZED = { jsonrpc: "2.0", method: "notifications/initialized" };

describe("MCP payload contracts", () => {
  it("tools/list returns 21 tools", async () => {
    const responses = await mcpCall([INIT, INITIALIZED, { jsonrpc: "2.0", id: 2, method: "tools/list" }], DATA_DIR);
    const toolList = responses.find((r) => r.id === 2);
    expect(toolList).toBeDefined();
    expect(toolList.result.tools).toHaveLength(21);
    const names = toolList.result.tools.map((t: any) => t.name).sort();
    expect(names).toEqual([
      "explain_symbol_propagation",
      "find_all_overloads",
      "find_base_methods",
      "find_callers",
      "find_callers_recursive",
      "find_overrides",
      "find_references",
      "get_callgraph",
      "get_type_hierarchy",
      "impact_analysis",
      "investigate_workflow",
      "list_class_members",
      "list_file_symbols",
      "list_namespace_symbols",
      "lookup_class",
      "lookup_function",
      "lookup_symbol",
      "search_symbols",
      "trace_call_path",
      "trace_variable_flow",
      "workspace_summary",
    ]);
  });

  it("lookup_symbol returns exact response by id", async () => {
    const responses = await mcpCall([
      INIT, INITIALIZED,
      {
        jsonrpc: "2.0",
        id: 3,
        method: "tools/call",
        params: { name: "lookup_symbol", arguments: { id: "Game::AIComponent::UpdateAI" } },
      },
    ], DATA_DIR);
    const res = responses.find((r) => r.id === 3);
    expect(res).toBeDefined();
    expect(res.result.isError).toBeUndefined();
    const payload = JSON.parse(res.result.content[0].text);
    expect(payload.lookupMode).toBe("exact");
    expect(payload.confidence).toBe("exact");
    expect(payload.matchReasons).toEqual(["exact_id_match"]);
    expect(["canonical", "acceptable", "weak"]).toContain(payload.representativeConfidence);
    expect(Array.isArray(payload.representativeSelectionReasons)).toBe(true);
    expect(payload.symbol.qualifiedName).toBe("Game::AIComponent::UpdateAI");
    expect(payload.callers).toBeInstanceOf(Array);
    expect(payload.callees).toBeInstanceOf(Array);
  });

  it("lookup_symbol returns exact response by qualifiedName", async () => {
    const responses = await mcpCall([
      INIT, INITIALIZED,
      {
        jsonrpc: "2.0",
        id: 3,
        method: "tools/call",
        params: { name: "lookup_symbol", arguments: { qualifiedName: "Game::GameObject" } },
      },
    ], DATA_DIR);
    const res = responses.find((r) => r.id === 3);
    const payload = JSON.parse(res.result.content[0].text);
    expect(payload.lookupMode).toBe("exact");
    expect(payload.confidence).toBe("exact");
    expect(payload.matchReasons).toEqual(["exact_qualified_name_match"]);
    expect(["canonical", "acceptable", "weak"]).toContain(payload.representativeConfidence);
    expect(Array.isArray(payload.representativeSelectionReasons)).toBe(true);
    expect(payload.symbol.qualifiedName).toBe("Game::GameObject");
    expect(payload.members).toBeInstanceOf(Array);
    expect(payload.members.length).toBeGreaterThan(0);
  });

  it("lookup_symbol rejects mismatched id and qualifiedName", async () => {
    const responses = await mcpCall([
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
    ], DATA_DIR);
    const res = responses.find((r) => r.id === 3);
    expect(res.result.isError).toBe(true);
    const payload = JSON.parse(res.result.content[0].text);
    expect(payload.code).toBe("BAD_REQUEST");
    expect(payload.error).toBe("Invalid exact lookup request");
  });

  it("lookup_symbol rejects missing exact lookup arguments", async () => {
    const responses = await mcpCall([
      INIT, INITIALIZED,
      {
        jsonrpc: "2.0",
        id: 3,
        method: "tools/call",
        params: { name: "lookup_symbol", arguments: {} },
      },
    ], DATA_DIR);
    const res = responses.find((r) => r.id === 3);
    expect(res.result.isError).toBe(true);
    const payload = JSON.parse(res.result.content[0].text);
    expect(payload.code).toBe("BAD_REQUEST");
    expect(payload.error).toBe("Invalid exact lookup request");
  });

  it("lookup_symbol returns NOT_FOUND for unknown exact symbol", async () => {
    const responses = await mcpCall([
      INIT, INITIALIZED,
      {
        jsonrpc: "2.0",
        id: 3,
        method: "tools/call",
        params: { name: "lookup_symbol", arguments: { id: "Game::DoesNotExist" } },
      },
    ], DATA_DIR);
    const res = responses.find((r) => r.id === 3);
    expect(res.result.isError).toBe(true);
    const payload = JSON.parse(res.result.content[0].text);
    expect(payload.code).toBe("NOT_FOUND");
    expect(payload.error).toBe("Symbol not found");
    expect(payload.symbol).toBeUndefined();
    expect(payload.confidence).toBeUndefined();
  });

  it("lookup_function returns correct shape", async () => {
    const responses = await mcpCall([
      INIT, INITIALIZED,
      { jsonrpc: "2.0", id: 3, method: "tools/call", params: { name: "lookup_function", arguments: { name: "UpdateAI" } } },
    ], DATA_DIR);
    const res = responses.find((r) => r.id === 3);
    expect(res).toBeDefined();
    expect(res.result.isError).toBeUndefined();
    const payload = JSON.parse(res.result.content[0].text);
    expect(payload.symbol).toBeDefined();
    expect(payload.symbol.name).toBe("UpdateAI");
    expect(payload.lookupMode).toBe("heuristic");
    expect(payload.confidence).toBe("high_confidence_heuristic");
    expect(payload.matchReasons).toEqual([]);
    expect(payload.reliability.level).toBeDefined();
    expect(Array.isArray(payload.reliability.factors)).toBe(true);
    expect(typeof payload.selectedReason).toBe("string");
    expect(payload.topCandidates).toBeUndefined();
    expect(payload.callers).toBeInstanceOf(Array);
    expect(payload.callees).toBeInstanceOf(Array);
    for (const ref of [...payload.callers, ...payload.callees]) {
      expect(ref.confidence).toBe("high_confidence_heuristic");
      expect(ref.matchReasons).toEqual([]);
    }
  });

  it("lookup_class returns correct shape", async () => {
    const responses = await mcpCall([
      INIT, INITIALIZED,
      { jsonrpc: "2.0", id: 3, method: "tools/call", params: { name: "lookup_class", arguments: { name: "GameObject" } } },
    ], DATA_DIR);
    const res = responses.find((r) => r.id === 3);
    const payload = JSON.parse(res.result.content[0].text);
    expect(payload.symbol.name).toBe("GameObject");
    expect(payload.lookupMode).toBe("heuristic");
    expect(payload.confidence).toBe("high_confidence_heuristic");
    expect(payload.matchReasons).toEqual([]);
    expect(typeof payload.selectedReason).toBe("string");
    expect(payload.topCandidates).toBeUndefined();
    expect(payload.members).toBeInstanceOf(Array);
    expect(payload.members.length).toBeGreaterThan(0);
  });

  it("find_all_overloads returns grouped exact same-name callable matches", async () => {
    const responses = await mcpCall([
      INIT, INITIALIZED,
      { jsonrpc: "2.0", id: 3, method: "tools/call", params: { name: "find_all_overloads", arguments: { name: "UpdateAI" } } },
    ], DATA_DIR);
    const res = responses.find((r) => r.id === 3);
    const payload = JSON.parse(res.result.content[0].text);
    expect(payload.query).toBe("UpdateAI");
    expect(typeof payload.totalCount).toBe("number");
    expect(typeof payload.groupCount).toBe("number");
    expect(Array.isArray(payload.groups)).toBe(true);
    expect(payload.groups.length).toBeGreaterThan(0);
    expect(payload.groups[0]).toHaveProperty("qualifiedName");
    expect(payload.groups[0]).toHaveProperty("candidates");
  });

  it("search_symbols returns correct shape", async () => {
    const responses = await mcpCall([
      INIT, INITIALIZED,
      { jsonrpc: "2.0", id: 3, method: "tools/call", params: { name: "search_symbols", arguments: { query: "Update" } } },
    ], DATA_DIR);
    const res = responses.find((r) => r.id === 3);
    const payload = JSON.parse(res.result.content[0].text);
    expect(payload.query).toBe("Update");
    expect(payload.window.totalCount).toBe(payload.totalCount);
    expect(payload.window.returnedCount).toBe(payload.results.length);
    expect(payload.results).toBeInstanceOf(Array);
    expect(payload).toHaveProperty("totalCount");
    expect(payload).toHaveProperty("truncated");
  });

  it("find_references supports compact mode", async () => {
    const responses = await mcpCall([
      INIT, INITIALIZED,
      { jsonrpc: "2.0", id: 3, method: "tools/call", params: { name: "find_references", arguments: { qualifiedName: "Game::AIComponent::UpdateAI", compact: true } } },
    ], DATA_DIR);
    const res = responses.find((r) => r.id === 3);
    const payload = JSON.parse(res.result.content[0].text);
    expect(payload.responseMode).toBe("compact");
    expect(payload.reliability.level).toBeDefined();
    expect(payload.references).toBeInstanceOf(Array);
    if (payload.references.length > 0) {
      expect(payload.references[0].sourceQualifiedName).toBeDefined();
      expect(payload.references[0].targetQualifiedName).toBeDefined();
      expect(payload.references[0].confidence).toBeUndefined();
    }
  });

  it("get_callgraph returns correct shape", async () => {
    const responses = await mcpCall([
      INIT, INITIALIZED,
      { jsonrpc: "2.0", id: 3, method: "tools/call", params: { name: "get_callgraph", arguments: { name: "UpdateAI" } } },
    ], DATA_DIR);
    const res = responses.find((r) => r.id === 3);
    const payload = JSON.parse(res.result.content[0].text);
    expect(payload.root).toBeDefined();
    expect(payload.root.symbol.name).toBe("UpdateAI");
    expect(payload.root.callees).toBeInstanceOf(Array);
    expect(payload.direction).toBe("callees");
    expect(payload.reliability.level).toBeDefined();
    expect(payload).toHaveProperty("depth");
    expect(payload).toHaveProperty("maxDepth");
    expect(payload).toHaveProperty("nodeCount");
    expect(payload).toHaveProperty("nodeCap");
    expect(payload).toHaveProperty("truncated");
  });

  it("get_callgraph supports compact mode", async () => {
    const responses = await mcpCall([
      INIT, INITIALIZED,
      { jsonrpc: "2.0", id: 3, method: "tools/call", params: { name: "get_callgraph", arguments: { name: "UpdateAI", compact: true } } },
    ], DATA_DIR);
    const res = responses.find((r) => r.id === 3);
    const payload = JSON.parse(res.result.content[0].text);
    expect(payload.responseMode).toBe("compact");
    expect(payload.root.symbol.qualifiedName).toBe("Game::AIComponent::UpdateAI");
    expect(payload.root.symbol.type).toBeUndefined();
  });

  it("get_callgraph supports caller-direction traversal", async () => {
    const responses = await mcpCall([
      INIT, INITIALIZED,
      { jsonrpc: "2.0", id: 3, method: "tools/call", params: { name: "get_callgraph", arguments: { name: "UpdateAI", direction: "callers", depth: 2 } } },
    ], DATA_DIR);
    const res = responses.find((r) => r.id === 3);
    const payload = JSON.parse(res.result.content[0].text);
    expect(payload.direction).toBe("callers");
    expect(payload.root.callers).toBeInstanceOf(Array);
    expect(payload.root.callers.length).toBeGreaterThan(0);
    expect(payload.root.callees).toEqual([]);
  });

  it("find_callers_recursive returns bounded recursive caller graph", async () => {
    const responses = await mcpCall([
      INIT, INITIALIZED,
      { jsonrpc: "2.0", id: 3, method: "tools/call", params: { name: "find_callers_recursive", arguments: { name: "UpdateAI", depth: 2 } } },
    ], DATA_DIR);
    const res = responses.find((r) => r.id === 3);
    const payload = JSON.parse(res.result.content[0].text);
    expect(payload.direction).toBe("callers");
    expect(payload.root.callers).toBeInstanceOf(Array);
    expect(payload.root.callers.length).toBeGreaterThan(0);
    expect(payload.nodeCount).toBeGreaterThanOrEqual(1);
  });

  it("find_callers returns deduplicated callers in deterministic order", async () => {
    const responses = await mcpCall([
      INIT, INITIALIZED,
      { jsonrpc: "2.0", id: 3, method: "tools/call", params: { name: "find_callers", arguments: { name: "Update", limit: 10 } } },
    ], DATA_DIR);
    const res = responses.find((r) => r.id === 3);
    const payload = JSON.parse(res.result.content[0].text);
    expect(payload.symbol.name).toBe("Update");
    expect(payload.reliability.level).toBeDefined();
    expect(payload.indexCoverage).toBeDefined();
    expect(payload.callers).toBeInstanceOf(Array);
    expect(payload.callers.length).toBeGreaterThan(0);
    expect(payload.totalCount).toBe(payload.callers.length);
    expect(payload.truncated).toBe(false);
    expect(payload.window.totalCount).toBe(payload.totalCount);
    expect(payload.window.returnedCount).toBe(payload.callers.length);

    const qualifiedNames = payload.callers.map((ref: any) => ref.qualifiedName);
    expect(qualifiedNames).toEqual([...qualifiedNames].sort((a, b) => a.localeCompare(b)));
    expect(new Set(payload.callers.map((ref: any) => ref.symbolId)).size).toBe(payload.callers.length);
  });

  it("list_file_symbols returns stable overview for one file", async () => {
    const responses = await mcpCall([
      INIT, INITIALIZED,
      { jsonrpc: "2.0", id: 3, method: "tools/call", params: { name: "list_file_symbols", arguments: { filePath: "src/game_object.h" } } },
    ], DATA_DIR);
    const res = responses.find((r) => r.id === 3);
    const payload = JSON.parse(res.result.content[0].text);
    expect(payload.filePath).toBe("src/game_object.h");
    expect(payload.summary.totalCount).toBe(payload.symbols.length);
    expect(payload.window.totalCount).toBe(payload.summary.totalCount);
    expect(payload.window.returnedCount).toBe(payload.symbols.length);
    expect(payload.symbols).toBeInstanceOf(Array);
    expect(payload.symbols.length).toBeGreaterThan(0);
  });

  it("list_file_symbols supports compact mode", async () => {
    const responses = await mcpCall([
      INIT, INITIALIZED,
      { jsonrpc: "2.0", id: 3, method: "tools/call", params: { name: "list_file_symbols", arguments: { filePath: "src/game_object.h", compact: true } } },
    ], DATA_DIR);
    const res = responses.find((r) => r.id === 3);
    const payload = JSON.parse(res.result.content[0].text);
    expect(payload.responseMode).toBe("compact");
    expect(payload.symbols[0].qualifiedName).toBeDefined();
    expect(payload.symbols[0].endLine).toEqual(expect.any(Number));
    expect(payload.symbols[0].filePath).toBeUndefined();
  });

  it("list_class_members returns exact member overview", async () => {
    const responses = await mcpCall([
      INIT, INITIALIZED,
      { jsonrpc: "2.0", id: 3, method: "tools/call", params: { name: "list_class_members", arguments: { qualifiedName: "Game::GameObject" } } },
    ], DATA_DIR);
    const res = responses.find((r) => r.id === 3);
    const payload = JSON.parse(res.result.content[0].text);
    expect(payload.lookupMode).toBe("exact");
    expect(payload.symbol.qualifiedName).toBe("Game::GameObject");
    expect(payload.summary.totalCount).toBe(payload.members.length);
    expect(payload.window.totalCount).toBe(payload.summary.totalCount);
    expect(payload.window.returnedCount).toBe(payload.members.length);
    expect(payload.members).toBeInstanceOf(Array);
    expect(payload.members.length).toBeGreaterThan(0);
  });

  it("NOT_FOUND returns isError", async () => {
    const responses = await mcpCall([
      INIT, INITIALIZED,
      { jsonrpc: "2.0", id: 3, method: "tools/call", params: { name: "lookup_function", arguments: { name: "DoesNotExist" } } },
    ], DATA_DIR);
    const res = responses.find((r) => r.id === 3);
    expect(res.result.isError).toBe(true);
    const payload = JSON.parse(res.result.content[0].text);
    expect(payload.code).toBe("NOT_FOUND");
    expect(payload.error).toBe("Symbol not found");
    expect(payload.symbol).toBeUndefined();
    expect(payload.confidence).toBeUndefined();
  });
});
