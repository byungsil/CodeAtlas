import { loadConfig, resolveWorkspace } from "../config";
import * as net from "net";
import * as path from "path";

describe("Config from environment variables", () => {
  const originalEnv = process.env;

  beforeEach(() => {
    process.env = { ...originalEnv };
    delete process.env.CODEATLAS_PORT;
    delete process.env.CODEATLAS_DASHBOARD_AUTOOPEN;
    delete process.env.CODEATLAS_WATCHER;
    delete process.env.CODEATLAS_INDEXER_PATH;
  });

  afterAll(() => {
    process.env = originalEnv;
  });

  it("returns defaults when no env vars set", () => {
    const config = loadConfig();
    expect(config.dashboard.autoOpen).toBe(false);
    expect(config.dashboard.port).toBe(3000);
  });

  it("reads CODEATLAS_PORT", () => {
    process.env.CODEATLAS_PORT = "4567";
    const config = loadConfig();
    expect(config.dashboard.port).toBe(4567);
  });

  it("reads CODEATLAS_DASHBOARD_AUTOOPEN=true", () => {
    process.env.CODEATLAS_DASHBOARD_AUTOOPEN = "true";
    const config = loadConfig();
    expect(config.dashboard.autoOpen).toBe(true);
  });

  it("reads CODEATLAS_DASHBOARD_AUTOOPEN=1", () => {
    process.env.CODEATLAS_DASHBOARD_AUTOOPEN = "1";
    const config = loadConfig();
    expect(config.dashboard.autoOpen).toBe(true);
  });

  it("CODEATLAS_DASHBOARD_AUTOOPEN=false stays false", () => {
    process.env.CODEATLAS_DASHBOARD_AUTOOPEN = "false";
    const config = loadConfig();
    expect(config.dashboard.autoOpen).toBe(false);
  });

  it("ignores invalid port, uses default", () => {
    process.env.CODEATLAS_PORT = "not_a_number";
    const config = loadConfig();
    expect(config.dashboard.port).toBe(3000);
  });

  it("empty env var uses default", () => {
    process.env.CODEATLAS_PORT = "";
    const config = loadConfig();
    expect(config.dashboard.port).toBe(3000);
  });

  it("watcher disabled by default", () => {
    const config = loadConfig();
    expect(config.watcher.enabled).toBe(false);
    expect(config.watcher.indexerPath).toBe("codeatlas-indexer");
  });

  it("reads CODEATLAS_WATCHER=true", () => {
    process.env.CODEATLAS_WATCHER = "true";
    const config = loadConfig();
    expect(config.watcher.enabled).toBe(true);
  });

  it("reads CODEATLAS_WATCHER=1", () => {
    process.env.CODEATLAS_WATCHER = "1";
    const config = loadConfig();
    expect(config.watcher.enabled).toBe(true);
  });

  it("reads CODEATLAS_INDEXER_PATH", () => {
    process.env.CODEATLAS_INDEXER_PATH = "/custom/path/indexer";
    const config = loadConfig();
    expect(config.watcher.indexerPath).toBe("/custom/path/indexer");
  });
});

describe("Workspace resolution", () => {
  const originalEnv = process.env;

  beforeEach(() => {
    process.env = { ...originalEnv };
    delete process.env.CODEATLAS_WORKSPACE;
  });

  afterAll(() => {
    process.env = originalEnv;
  });

  it("explicit CODEATLAS_WORKSPACE takes precedence", () => {
    process.env.CODEATLAS_WORKSPACE = "/explicit/workspace";
    const result = resolveWorkspace("/some/project/.codeatlas");
    expect(result).toBe(path.resolve("/explicit/workspace"));
  });

  it("falls back to dataDir parent when CODEATLAS_WORKSPACE is unset", () => {
    const result = resolveWorkspace("/some/project/.codeatlas");
    expect(result).toBe(path.resolve("/some/project"));
  });

  it("falls back to dataDir parent when CODEATLAS_WORKSPACE is empty", () => {
    process.env.CODEATLAS_WORKSPACE = "";
    const result = resolveWorkspace("/some/project/.codeatlas");
    expect(result).toBe(path.resolve("/some/project"));
  });
});

describe("Port conflict handling", () => {
  it("createApp server emits EADDRINUSE on occupied port", (done) => {
    const blocker = net.createServer();
    blocker.listen(0, () => {
      const port = (blocker.address() as net.AddressInfo).port;

      const { createApp } = require("../app");
      const { SqliteStore } = require("../storage/sqlite-store");
      const path = require("path");
      const store = new SqliteStore(path.resolve(__dirname, "../../../samples/.codeatlas/index.db"));
      const app = createApp(store);

      const server = app.listen(port);
      server.on("error", (err: NodeJS.ErrnoException) => {
        expect(err.code).toBe("EADDRINUSE");
        store.close();
        blocker.close();
        done();
      });
    });
  });
});
