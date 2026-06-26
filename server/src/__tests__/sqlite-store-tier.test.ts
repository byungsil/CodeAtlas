import * as fs from "fs";
import * as os from "os";
import * as path from "path";
import Database from "better-sqlite3";
import { SqliteStore } from "../storage/sqlite-store";

// MS21: verify resolution_tier mapping and the confirmedOnly filter on call edges.

function makeTempDb(withTierColumn: boolean): string {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "codeatlas-tier-"));
  const dbPath = path.join(dir, "index.db");
  const db = new Database(dbPath);

  // Minimal schema sufficient for getCallers/getCallees + getSymbolById.
  db.exec(`
    CREATE TABLE symbols (
      id TEXT PRIMARY KEY, name TEXT, qualified_name TEXT, type TEXT,
      file_path TEXT, line INTEGER, end_line INTEGER
    );
  `);
  if (withTierColumn) {
    db.exec(`
      CREATE TABLE calls (
        caller_id TEXT NOT NULL, callee_id TEXT NOT NULL,
        file_path TEXT NOT NULL, line INTEGER NOT NULL,
        resolution_tier TEXT NOT NULL DEFAULT 'heuristic'
      );
    `);
  } else {
    // Pre-MS21 shape: no resolution_tier column.
    db.exec(`
      CREATE TABLE calls (
        caller_id TEXT NOT NULL, callee_id TEXT NOT NULL,
        file_path TEXT NOT NULL, line INTEGER NOT NULL
      );
    `);
  }

  const sym = db.prepare(
    "INSERT INTO symbols (id, name, qualified_name, type, file_path, line, end_line) VALUES (?,?,?,?,?,?,?)",
  );
  sym.run("caller", "caller", "ns::caller", "function", "a.cpp", 1, 2);
  sym.run("confirmed", "confirmed", "ns::confirmed", "function", "a.cpp", 10, 11);
  sym.run("heuristic", "heuristic", "ns::heuristic", "function", "a.cpp", 20, 21);
  sym.run("chaTarget", "chaTarget", "ns::chaTarget", "function", "a.cpp", 30, 31);

  if (withTierColumn) {
    const ins = db.prepare(
      "INSERT INTO calls (caller_id, callee_id, file_path, line, resolution_tier) VALUES (?,?,?,?,?)",
    );
    ins.run("caller", "confirmed", "a.cpp", 3, "compiler_confirmed");
    ins.run("caller", "heuristic", "a.cpp", 4, "heuristic");
    ins.run("caller", "chaTarget", "a.cpp", 5, "cha_virtual");
  } else {
    const ins = db.prepare(
      "INSERT INTO calls (caller_id, callee_id, file_path, line) VALUES (?,?,?,?)",
    );
    ins.run("caller", "confirmed", "a.cpp", 3);
    ins.run("caller", "heuristic", "a.cpp", 4);
  }

  db.close();
  return dbPath;
}

describe("SqliteStore resolution tier", () => {
  it("maps stored tiers onto call edges", () => {
    const store = new SqliteStore(makeTempDb(true));
    try {
      const callees = store.getCallees("caller");
      expect(callees).toHaveLength(3);
      const byId = new Map(callees.map((c) => [c.calleeId, c]));
      expect(byId.get("confirmed")?.resolutionTier).toBe("compilerConfirmed");
      expect(byId.get("heuristic")?.resolutionTier).toBe("heuristic");
      // MS27: cha_virtual rows map onto the chaVirtual tier.
      expect(byId.get("chaTarget")?.resolutionTier).toBe("chaVirtual");
    } finally {
      store.close();
    }
  });

  it("confirmedOnly returns only compiler-confirmed edges", () => {
    const store = new SqliteStore(makeTempDb(true));
    try {
      const all = store.getCallees("caller");
      expect(all).toHaveLength(3);

      const confirmed = store.getCallees("caller", true);
      expect(confirmed).toHaveLength(1);
      expect(confirmed[0].calleeId).toBe("confirmed");
      expect(confirmed[0].resolutionTier).toBe("compilerConfirmed");

      // Same filtering applies to the callers direction.
      const callers = store.getCallers("confirmed", true);
      expect(callers).toHaveLength(1);
      expect(callers[0].callerId).toBe("caller");
    } finally {
      store.close();
    }
  });

  it("pre-MS21 DB: edges default to heuristic and confirmedOnly returns none", () => {
    const store = new SqliteStore(makeTempDb(false));
    try {
      const all = store.getCallees("caller");
      expect(all).toHaveLength(2);
      expect(all.every((c) => c.resolutionTier === "heuristic")).toBe(true);

      // No tier column means no compiler-confirmed edges exist.
      const confirmed = store.getCallees("caller", true);
      expect(confirmed).toHaveLength(0);
    } finally {
      store.close();
    }
  });
});
