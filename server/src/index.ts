import * as fs from "fs";
import * as path from "path";
import { Store } from "./storage/store";
import { SqliteStore } from "./storage/sqlite-store";
import { JsonStore } from "./storage/json-store";
import { createApp } from "./app";
import { loadConfig } from "./config";
import { DATA_DIR_NAME, DB_FILENAME } from "./constants";

const dataDir = process.argv[2] || process.env.CODEATLAS_DATA || DATA_DIR_NAME;
const config = loadConfig();
const PORT = config.dashboard.port;

function openStore(dataDir: string): Store {
  const dbPath = path.join(dataDir, DB_FILENAME);
  if (fs.existsSync(dbPath)) {
    console.log(`Using SQLite: ${dbPath}`);
    return new SqliteStore(dbPath);
  }
  console.log(`Using JSON: ${dataDir}`);
  return new JsonStore(dataDir);
}

const store = openStore(dataDir);
const app = createApp(store);

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
