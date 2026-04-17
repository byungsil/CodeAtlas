import * as path from "path";

export interface CodeAtlasConfig {
  dashboard: {
    autoOpen: boolean;
    port: number;
  };
  watcher: {
    enabled: boolean;
    indexerPath: string;
  };
}

export function loadConfig(): CodeAtlasConfig {
  return {
    dashboard: {
      autoOpen: envBool("CODEATLAS_DASHBOARD_AUTOOPEN", false),
      port: envInt("CODEATLAS_PORT", 3000),
    },
    watcher: {
      enabled: envBool("CODEATLAS_WATCHER", false),
      indexerPath: process.env.CODEATLAS_INDEXER_PATH || "codeatlas-indexer",
    },
  };
}

export function resolveWorkspace(dataDir: string): string {
  const explicit = process.env.CODEATLAS_WORKSPACE;
  if (explicit && explicit !== "") {
    return path.resolve(explicit);
  }
  return path.resolve(dataDir, "..");
}

function envBool(key: string, fallback: boolean): boolean {
  const val = process.env[key];
  if (val === undefined || val === "") return fallback;
  return val === "1" || val.toLowerCase() === "true";
}

function envInt(key: string, fallback: number): number {
  const val = process.env[key];
  if (val === undefined || val === "") return fallback;
  const n = parseInt(val, 10);
  return isNaN(n) ? fallback : n;
}
