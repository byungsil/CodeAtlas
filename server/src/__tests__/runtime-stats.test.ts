import * as fs from "fs";
import * as os from "os";
import * as path from "path";
import {
  getMcpRuntimeStatsSnapshot,
  initRuntimeStats,
  prepareRuntimeStatsPath,
  readPersistedMcpRuntimeStatsSnapshot,
  recordMcpToolCall,
  resetMcpRuntimeStatsForTests,
  resolveRuntimeStatsPath,
} from "../runtime-stats";

describe("runtime stats cache path", () => {
  const originalCacheDir = process.env.CODEATLAS_CACHE_DIR;
  let tempRoot: string;

  beforeEach(() => {
    tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), "codeatlas-runtime-stats-"));
    process.env.CODEATLAS_CACHE_DIR = tempRoot;
    resetMcpRuntimeStatsForTests();
  });

  afterEach(() => {
    resetMcpRuntimeStatsForTests();
    process.env.CODEATLAS_CACHE_DIR = originalCacheDir;
    fs.rmSync(tempRoot, { recursive: true, force: true });
  });

  it("stores runtime stats directly inside the workspace data directory", () => {
    const dataDir = path.join(tempRoot, "workspace-a", ".codeatlas");
    fs.mkdirSync(dataDir, { recursive: true });

    const statsPath = resolveRuntimeStatsPath(dataDir);

    expect(statsPath).toBe(path.join(dataDir, "mcp-runtime-stats.json"));
  });

 it("reads an existing workspace-local stats file in place", () => {
    const dataDir = path.join(tempRoot, "workspace-b", ".codeatlas");
    fs.mkdirSync(dataDir, { recursive: true });
    const statsPath = path.join(dataDir, "mcp-runtime-stats.json");
    fs.writeFileSync(statsPath, JSON.stringify({
      startedAt: "2026-04-20T00:00:00.000Z",
      uptimeSeconds: 120,
      totalToolCalls: 1,
      totalErrors: 0,
      avgLatencyMs: 12,
      tools: [{
        toolName: "lookup_symbol",
        count: 1,
        errorCount: 0,
        totalLatencyMs: 12,
        avgLatencyMs: 12,
        lastCalledAt: "2026-04-20T00:01:00.000Z",
      }],
      recentCalls: [{
        toolName: "lookup_symbol",
        elapsedMs: 12,
        ok: true,
        timestamp: "2026-04-20T00:01:00.000Z",
      }],
    }, null, 2));

    const resolved = prepareRuntimeStatsPath(dataDir);
    const persisted = readPersistedMcpRuntimeStatsSnapshot(resolved);

    expect(fs.existsSync(resolved)).toBe(true);
    expect(resolved).toBe(statsPath);
    expect(persisted.totalToolCalls).toBe(1);
    expect(persisted.tools[0].toolName).toBe("lookup_symbol");
  });

  it("persists fresh MCP runtime stats to the cache location", () => {
    const dataDir = path.join(tempRoot, "workspace-c", ".codeatlas");
    fs.mkdirSync(dataDir, { recursive: true });

    const statsPath = prepareRuntimeStatsPath(dataDir);
    initRuntimeStats(statsPath);
    recordMcpToolCall({ toolName: "find_references", elapsedMs: 34, ok: true });

    const persisted = readPersistedMcpRuntimeStatsSnapshot(statsPath);
    const live = getMcpRuntimeStatsSnapshot();

    expect(fs.existsSync(statsPath)).toBe(true);
    expect(persisted.totalToolCalls).toBe(1);
    expect(persisted.tools[0].toolName).toBe("find_references");
    expect(live.totalToolCalls).toBe(1);
  });
});
