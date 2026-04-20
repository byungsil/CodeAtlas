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
const resolvedDataDir = path.resolve(dataDir);
const statsPath = prepareRuntimeStatsPath(resolvedDataDir);
const store = openStore(resolvedDataDir);
initRuntimeStats(statsPath);
const app = createApp(store, {
  dashboardWorkspaces: [{
    id: "primary",
    label: "primary",
    dataDir: resolvedDataDir,
    store,
    statsPath,
    isPrimary: true,
  }],
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
  const closable = store as Store & { close?: () => void };
  closable.close?.();
}

process.on("SIGINT", () => { closeStores(); process.exit(0); });
process.on("SIGTERM", () => { closeStores(); process.exit(0); });
process.on("exit", closeStores);
