import * as fs from "fs";
import * as os from "os";
import * as path from "path";

export interface ToolCallEvent {
  toolName: string;
  elapsedMs: number;
  ok: boolean;
  timestamp: string;
  errorMessage?: string;
}

export interface ToolUsageSummary {
  toolName: string;
  count: number;
  errorCount: number;
  totalLatencyMs: number;
  avgLatencyMs: number;
  lastCalledAt?: string;
  lastErrorAt?: string;
}

export interface McpRuntimeStatsSnapshot {
  startedAt: string;
  uptimeSeconds: number;
  totalToolCalls: number;
  totalErrors: number;
  avgLatencyMs: number;
  tools: ToolUsageSummary[];
  recentCalls: ToolCallEvent[];
}

interface ToolAccumulator {
  count: number;
  errorCount: number;
  totalLatencyMs: number;
  lastCalledAt?: string;
  lastErrorAt?: string;
}

interface RuntimeStatsState {
  startedAtMs: number;
  statsFilePath?: string;
  tools: Map<string, ToolAccumulator>;
  recentCalls: ToolCallEvent[];
}

const MAX_RECENT_CALLS = 20;
const STATS_FILENAME = "mcp-runtime-stats.json";
let state = createEmptyState();

export function initRuntimeStats(statsFilePath?: string): void {
  state = createEmptyState();
  state.statsFilePath = statsFilePath;
  if (statsFilePath) {
    const persisted = readStatsFile(statsFilePath);
    if (persisted) {
      state.startedAtMs = Date.parse(persisted.startedAt) || Date.now();
      for (const tool of persisted.tools) {
        state.tools.set(tool.toolName, {
          count: tool.count,
          errorCount: tool.errorCount,
          totalLatencyMs: tool.totalLatencyMs,
          lastCalledAt: tool.lastCalledAt,
          lastErrorAt: tool.lastErrorAt,
        });
      }
      state.recentCalls = persisted.recentCalls.slice(0, MAX_RECENT_CALLS);
    }
  }
}

export function recordMcpToolCall(params: {
  toolName: string;
  elapsedMs: number;
  ok: boolean;
  errorMessage?: string;
}): void {
  const now = new Date().toISOString();
  const current = state.tools.get(params.toolName) ?? {
    count: 0,
    errorCount: 0,
    totalLatencyMs: 0,
  };
  current.count += 1;
  current.totalLatencyMs += params.elapsedMs;
  current.lastCalledAt = now;
  if (!params.ok) {
    current.errorCount += 1;
    current.lastErrorAt = now;
  }
  state.tools.set(params.toolName, current);

  state.recentCalls.unshift({
    toolName: params.toolName,
    elapsedMs: params.elapsedMs,
    ok: params.ok,
    timestamp: now,
    ...(params.errorMessage ? { errorMessage: params.errorMessage } : {}),
  });
  if (state.recentCalls.length > MAX_RECENT_CALLS) {
    state.recentCalls.length = MAX_RECENT_CALLS;
  }

  persistStatsIfConfigured();
}

export function getMcpRuntimeStatsSnapshot(): McpRuntimeStatsSnapshot {
  return snapshotFromState(state);
}

const MERGE_CACHE_TTL_MS = 3000;
let mergeCache: { dir: string; snapshot: McpRuntimeStatsSnapshot; expireAt: number } | null = null;

export function readPersistedMcpRuntimeStatsSnapshot(statsFilePath?: string): McpRuntimeStatsSnapshot {
  if (!statsFilePath) {
    return getMcpRuntimeStatsSnapshot();
  }
  const statsDir = path.dirname(statsFilePath);
  const now = Date.now();
  if (mergeCache && mergeCache.dir === statsDir && now < mergeCache.expireAt) {
    return mergeCache.snapshot;
  }
  const snapshot = mergeAllStatsInDirectory(statsDir);
  mergeCache = { dir: statsDir, snapshot, expireAt: now + MERGE_CACHE_TTL_MS };
  return snapshot;
}

export function resetMcpRuntimeStatsForTests(): void {
  state = createEmptyState();
}

export function prepareRuntimeStatsPath(dataDir: string): string {
  const statsFilePath = resolveRuntimeStatsPath(dataDir);
  migrateLegacyStatsFile(path.resolve(dataDir), statsFilePath);
  return statsFilePath;
}

export function resolveRuntimeStatsPath(dataDir: string): string {
  const resolvedDataDir = path.resolve(dataDir);
  const workspaceRoot = path.resolve(resolvedDataDir, "..");
  const slug = sanitizeSegment(path.basename(workspaceRoot) || path.basename(resolvedDataDir) || "workspace");
  const hash = shortStableHash(resolvedDataDir.toLowerCase());
  return path.join(resolveCacheRoot(), "runtime-stats", `${slug}-${hash}.json`);
}

function createEmptyState(): RuntimeStatsState {
  return {
    startedAtMs: Date.now(),
    tools: new Map<string, ToolAccumulator>(),
    recentCalls: [],
  };
}

function resolveCacheRoot(): string {
  const override = process.env.CODEATLAS_CACHE_DIR;
  if (override && override.trim() !== "") {
    return path.resolve(override);
  }
  if (process.platform === "win32") {
    const localAppData = process.env.LOCALAPPDATA;
    if (localAppData && localAppData.trim() !== "") {
      return path.join(path.resolve(localAppData), "CodeAtlas");
    }
  }
  if (process.platform === "darwin") {
    return path.join(os.homedir(), "Library", "Caches", "CodeAtlas");
  }
  const xdgCacheHome = process.env.XDG_CACHE_HOME;
  if (xdgCacheHome && xdgCacheHome.trim() !== "") {
    return path.join(path.resolve(xdgCacheHome), "CodeAtlas");
  }
  return path.join(os.homedir(), ".cache", "CodeAtlas");
}

function migrateLegacyStatsFile(dataDir: string, statsFilePath: string): void {
  const legacyPath = path.join(dataDir, STATS_FILENAME);
  if (legacyPath === statsFilePath || fs.existsSync(statsFilePath) || !fs.existsSync(legacyPath)) {
    return;
  }
  try {
    const raw = fs.readFileSync(legacyPath, "utf-8");
    const parsed = JSON.parse(raw) as McpRuntimeStatsSnapshot;
    fs.mkdirSync(path.dirname(statsFilePath), { recursive: true });
    fs.writeFileSync(statsFilePath, JSON.stringify(parsed, null, 2));
  } catch {
    // Ignore invalid legacy snapshots and keep running.
  }
}

function snapshotFromState(current: RuntimeStatsState): McpRuntimeStatsSnapshot {
  const tools = Array.from(current.tools.entries())
    .map(([toolName, value]) => ({
      toolName,
      count: value.count,
      errorCount: value.errorCount,
      totalLatencyMs: value.totalLatencyMs,
      avgLatencyMs: value.count > 0 ? round2(value.totalLatencyMs / value.count) : 0,
      ...(value.lastCalledAt ? { lastCalledAt: value.lastCalledAt } : {}),
      ...(value.lastErrorAt ? { lastErrorAt: value.lastErrorAt } : {}),
    }))
    .sort((left, right) =>
      right.count - left.count
      || right.totalLatencyMs - left.totalLatencyMs
      || left.toolName.localeCompare(right.toolName),
    );
  const totalToolCalls = tools.reduce((sum, tool) => sum + tool.count, 0);
  const totalErrors = tools.reduce((sum, tool) => sum + tool.errorCount, 0);
  const totalLatencyMs = tools.reduce((sum, tool) => sum + tool.totalLatencyMs, 0);

  return {
    startedAt: new Date(current.startedAtMs).toISOString(),
    uptimeSeconds: round2((Date.now() - current.startedAtMs) / 1000),
    totalToolCalls,
    totalErrors,
    avgLatencyMs: totalToolCalls > 0 ? round2(totalLatencyMs / totalToolCalls) : 0,
    tools,
    recentCalls: [...current.recentCalls],
  };
}

function persistStatsIfConfigured(): void {
  if (!state.statsFilePath) {
    return;
  }
  const snapshot = snapshotFromState(state);
  try {
    fs.mkdirSync(path.dirname(state.statsFilePath), { recursive: true });
    fs.writeFileSync(state.statsFilePath, JSON.stringify(snapshot, null, 2));
  } catch {
    // Best-effort persistence only.
  }
}

function mergeAllStatsInDirectory(statsDir: string): McpRuntimeStatsSnapshot {
  let entries: string[];
  try {
    entries = fs.readdirSync(statsDir).filter((e) => e.endsWith(".json"));
  } catch {
    return emptySnapshot();
  }
  if (entries.length === 0) {
    return emptySnapshot();
  }

  const toolMap = new Map<string, ToolAccumulator>();
  let allRecentCalls: ToolCallEvent[] = [];
  let earliestStart = Infinity;
  let totalToolCalls = 0;
  let totalErrors = 0;
  let totalLatencyMs = 0;

  for (const entry of entries) {
    const snap = readStatsFile(path.join(statsDir, entry));
    if (!snap) continue;
    const startMs = Date.parse(snap.startedAt);
    if (startMs && startMs < earliestStart) earliestStart = startMs;
    for (const tool of snap.tools) {
      const existing = toolMap.get(tool.toolName);
      if (existing) {
        existing.count += tool.count;
        existing.errorCount += tool.errorCount;
        existing.totalLatencyMs += tool.totalLatencyMs;
        if (tool.lastCalledAt && (!existing.lastCalledAt || tool.lastCalledAt > existing.lastCalledAt)) {
          existing.lastCalledAt = tool.lastCalledAt;
        }
        if (tool.lastErrorAt && (!existing.lastErrorAt || tool.lastErrorAt > existing.lastErrorAt)) {
          existing.lastErrorAt = tool.lastErrorAt;
        }
      } else {
        toolMap.set(tool.toolName, {
          count: tool.count,
          errorCount: tool.errorCount,
          totalLatencyMs: tool.totalLatencyMs,
          lastCalledAt: tool.lastCalledAt,
          lastErrorAt: tool.lastErrorAt,
        });
      }
    }
    allRecentCalls = allRecentCalls.concat(snap.recentCalls);
  }

  allRecentCalls.sort((a, b) => (b.timestamp > a.timestamp ? 1 : b.timestamp < a.timestamp ? -1 : 0));
  allRecentCalls = allRecentCalls.slice(0, MAX_RECENT_CALLS);

  const tools = Array.from(toolMap.entries())
    .map(([toolName, v]) => ({
      toolName,
      count: v.count,
      errorCount: v.errorCount,
      totalLatencyMs: v.totalLatencyMs,
      avgLatencyMs: v.count > 0 ? round2(v.totalLatencyMs / v.count) : 0,
      ...(v.lastCalledAt ? { lastCalledAt: v.lastCalledAt } : {}),
      ...(v.lastErrorAt ? { lastErrorAt: v.lastErrorAt } : {}),
    }))
    .sort((a, b) => b.count - a.count || b.totalLatencyMs - a.totalLatencyMs || a.toolName.localeCompare(b.toolName));

  for (const t of tools) { totalToolCalls += t.count; totalErrors += t.errorCount; totalLatencyMs += t.totalLatencyMs; }
  const startedAtMs = earliestStart === Infinity ? Date.now() : earliestStart;

  return {
    startedAt: new Date(startedAtMs).toISOString(),
    uptimeSeconds: round2((Date.now() - startedAtMs) / 1000),
    totalToolCalls,
    totalErrors,
    avgLatencyMs: totalToolCalls > 0 ? round2(totalLatencyMs / totalToolCalls) : 0,
    tools,
    recentCalls: allRecentCalls,
  };
}

function readStatsFile(statsFilePath: string): McpRuntimeStatsSnapshot | null {
  try {
    if (!fs.existsSync(statsFilePath)) {
      return null;
    }
    const raw = fs.readFileSync(statsFilePath, "utf-8");
    return JSON.parse(raw) as McpRuntimeStatsSnapshot;
  } catch {
    return null;
  }
}

function sanitizeSegment(value: string): string {
  const sanitized = value
    .trim()
    .replace(/[^A-Za-z0-9._-]+/g, "-")
    .replace(/-+/g, "-")
    .replace(/^-|-$/g, "");
  return sanitized || "workspace";
}

function shortStableHash(value: string): string {
  let hash = 2166136261;
  for (let index = 0; index < value.length; index += 1) {
    hash ^= value.charCodeAt(index);
    hash = Math.imul(hash, 16777619);
  }
  return (hash >>> 0).toString(16);
}

function emptySnapshot(): McpRuntimeStatsSnapshot {
  return {
    startedAt: new Date().toISOString(),
    uptimeSeconds: 0,
    totalToolCalls: 0,
    totalErrors: 0,
    avgLatencyMs: 0,
    tools: [],
    recentCalls: [],
  };
}

function round2(value: number): number {
  return Math.round(value * 100) / 100;
}
