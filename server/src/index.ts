import * as fs from "fs";
import * as path from "path";
import { Store } from "./storage/store";
import { resolveActiveDatabasePath, SqliteStore } from "./storage/sqlite-store";
import { JsonStore } from "./storage/json-store";
import { createApp } from "./app";
import { loadConfig } from "./config";
import { DATA_DIR_NAME } from "./constants";
import { initRuntimeStats, prepareRuntimeStatsPath } from "./runtime-stats";

const dataDir = process.argv[2] || process.env.CODEATLAS_DATA || DATA_DIR_NAME;
const config = loadConfig();
const PORT = config.dashboard.port;

function openStore(dataDir: string): Store {
  const dbPath = resolveActiveDatabasePath(dataDir);
  if (dbPath && fs.existsSync(dbPath)) {
    console.log(`Using SQLite: ${dbPath}`);
    return new SqliteStore(dbPath);
  }
  console.log(`Using JSON: ${dataDir}`);
  return new JsonStore(dataDir);
}

function buildDashboardWorkspaceId(dataDir: string): string {
  const normalized = path.resolve(dataDir);
  const workspaceRoot = path.basename(path.resolve(normalized, ".."));
  return workspaceRoot || path.basename(normalized) || "workspace";
}

function buildDashboardWorkspaceSource(dataDir: string) {
  const id = buildDashboardWorkspaceId(dataDir);
  return {
    id,
    label: path.basename(path.resolve(dataDir, "..")) || id,
    dataDir,
    store: openStore(dataDir),
    statsPath: prepareRuntimeStatsPath(dataDir),
  };
}

const dataDirs = Array.from(new Set([path.resolve(dataDir), ...config.dashboard.dataDirs]));
const workspaceSources = dataDirs.map((dirPath) => buildDashboardWorkspaceSource(dirPath));
const primary = workspaceSources[0];
initRuntimeStats(primary.statsPath);
const app = createApp(primary.store, {
  dashboardWorkspaces: workspaceSources.map((source, index) => ({
    ...source,
    isPrimary: index === 0,
  })),
});

const server = app.listen(PORT, () => {
  console.log(`CodeAtlas server listening on http://localhost:${PORT}`);
  console.log(`Dashboard: http://localhost:${PORT}/dashboard/`);
});

server.on("error", (err: NodeJS.ErrnoException) => {
  if (err.code === "EADDRINUSE") {
    console.error(`ERROR: Port ${PORT} is already in use.`);
    console.error(`Set CODEATLAS_PORT to use a different port.`);
    process.exit(1);
  }
  throw err;
});

function closeStores() {
  for (const source of workspaceSources) {
    const closable = source.store as Store & { close?: () => void };
    closable.close?.();
  }
}

process.on("SIGINT", () => { closeStores(); process.exit(0); });
process.on("SIGTERM", () => { closeStores(); process.exit(0); });
process.on("exit", closeStores);
