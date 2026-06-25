import * as fs from "fs";
import * as os from "os";
import * as path from "path";
import Database from "better-sqlite3";
import { SqliteStore } from "../storage/sqlite-store";

// MS24: getSymbolByQualifiedName must apply a deterministic, declaration-
// preferring ORDER BY when multiple USR clusters share a qualified_name.
// Cross-USR anchor selection is the high-leverage win for lookup_function
// stability on real codebases (e.g. cv::imread overloads).

interface SymbolRow {
  id: string;
  qualified_name: string;
  type: string;
  file_path: string;
  line: number;
  end_line?: number;
  symbol_role?: string | null;
  header_role?: string | null;
  artifact_kind?: string | null;
}

function makeAnchorDb(rows: SymbolRow[]): string {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "codeatlas-anchor-"));
  const dbPath = path.join(dir, "index.db");
  const db = new Database(dbPath);

  db.exec(`
    CREATE TABLE symbols (
      id TEXT NOT NULL,
      name TEXT NOT NULL,
      qualified_name TEXT NOT NULL,
      type TEXT NOT NULL,
      file_path TEXT NOT NULL,
      line INTEGER NOT NULL,
      end_line INTEGER NOT NULL,
      signature TEXT,
      parameter_count INTEGER,
      scope_qualified_name TEXT,
      scope_kind TEXT,
      symbol_role TEXT,
      declaration_file_path TEXT,
      declaration_line INTEGER,
      declaration_end_line INTEGER,
      definition_file_path TEXT,
      definition_line INTEGER,
      definition_end_line INTEGER,
      parent_id TEXT,
      module TEXT,
      subsystem TEXT,
      project_area TEXT,
      artifact_kind TEXT,
      header_role TEXT,
      parse_fragility TEXT,
      macro_sensitivity TEXT,
      include_heaviness TEXT
    );
  `);

  const ins = db.prepare(`
    INSERT INTO symbols
      (id, name, qualified_name, type, file_path, line, end_line,
       symbol_role, header_role, artifact_kind)
    VALUES (?,?,?,?,?,?,?,?,?,?)
  `);
  for (const r of rows) {
    const name = r.qualified_name.split("::").pop() ?? r.qualified_name;
    ins.run(
      r.id,
      name,
      r.qualified_name,
      r.type,
      r.file_path,
      r.line,
      r.end_line ?? r.line,
      r.symbol_role ?? null,
      r.header_role ?? null,
      r.artifact_kind ?? null,
    );
  }

  db.close();
  return dbPath;
}

describe("SqliteStore.getSymbolByQualifiedName (MS24 anchor weighting)", () => {
  it("prefers a public-header declaration over out-of-line .cpp definitions when USRs differ", () => {
    // Mirrors the real cv::imread shape: 1 declaration in the public hpp
    // (different USR from each definition) + 2 out-of-line definitions in
    // a .cpp. Without ORDER BY the .cpp rows can come first.
    const dbPath = makeAnchorDb([
      {
        id: "c:@N@cv@F@imread#&1I#I#",
        qualified_name: "cv::imread",
        type: "function",
        file_path: "modules/imgcodecs/src/loadsave.cpp",
        line: 754,
        symbol_role: "definition",
        artifact_kind: "runtime",
      },
      {
        id: "c:@N@cv@F@imread#&1I#I#I#",
        qualified_name: "cv::imread",
        type: "function",
        file_path: "modules/imgcodecs/src/loadsave.cpp",
        line: 785,
        symbol_role: "definition",
        artifact_kind: "runtime",
      },
      {
        id: "cv::imread",
        qualified_name: "cv::imread",
        type: "function",
        file_path: "modules/imgcodecs/include/opencv2/imgcodecs.hpp",
        line: 384,
        symbol_role: "declaration",
        header_role: "public",
        artifact_kind: "runtime",
      },
    ]);
    const store = new SqliteStore(dbPath);
    try {
      const sym = store.getSymbolByQualifiedName("cv::imread");
      expect(sym).toBeDefined();
      expect(sym?.filePath).toBe("modules/imgcodecs/include/opencv2/imgcodecs.hpp");
      expect(sym?.symbolRole).toBe("declaration");
    } finally {
      store.close();
    }
  });

  it("is deterministic across repeated calls", () => {
    // 10 calls must return the same row. Catches any future ORDER-BY-less
    // regression (which sqlite would expose as nondeterministic ordering).
    const dbPath = makeAnchorDb([
      {
        id: "u1",
        qualified_name: "ns::Sym",
        type: "function",
        file_path: "src/a.cpp",
        line: 1,
        symbol_role: "definition",
      },
      {
        id: "u2",
        qualified_name: "ns::Sym",
        type: "function",
        file_path: "src/b.cpp",
        line: 1,
        symbol_role: "definition",
      },
      {
        id: "u3",
        qualified_name: "ns::Sym",
        type: "function",
        file_path: "include/api.hpp",
        line: 1,
        symbol_role: "declaration",
        header_role: "public",
      },
    ]);
    const store = new SqliteStore(dbPath);
    try {
      const first = store.getSymbolByQualifiedName("ns::Sym")?.filePath;
      expect(first).toBe("include/api.hpp");
      for (let i = 0; i < 9; i++) {
        expect(store.getSymbolByQualifiedName("ns::Sym")?.filePath).toBe(first);
      }
    } finally {
      store.close();
    }
  });

  it("demotes .inl.hpp inline definitions below regular .hpp definitions", () => {
    // No declaration+public candidate exists, so tier (1) is moot.
    // Both rows are definitions with header_role=public, so tier (2)/(3)
    // also tie. Tier (4) — inline-impl demotion — must pick the .hpp.
    const dbPath = makeAnchorDb([
      {
        id: "u1",
        qualified_name: "cv::Mat::Mat",
        type: "method",
        file_path: "modules/core/include/opencv2/core/cuda.inl.hpp",
        line: 752,
        symbol_role: "definition",
        header_role: "public",
        artifact_kind: "runtime",
      },
      {
        id: "u2",
        qualified_name: "cv::Mat::Mat",
        type: "method",
        file_path: "modules/core/include/opencv2/core/mat.hpp",
        line: 100,
        symbol_role: "definition",
        header_role: "public",
        artifact_kind: "runtime",
      },
    ]);
    const store = new SqliteStore(dbPath);
    try {
      const sym = store.getSymbolByQualifiedName("cv::Mat::Mat");
      expect(sym?.filePath).toBe("modules/core/include/opencv2/core/mat.hpp");
    } finally {
      store.close();
    }
  });

  it("falls back to file_path/line tie-break when all weighted keys tie", () => {
    // Two definitions, same tier (no public declaration, both non-test,
    // neither is .inl.hpp). Final tie-break must be lexical file_path ASC.
    const dbPath = makeAnchorDb([
      {
        id: "u1",
        qualified_name: "ns::Plain",
        type: "function",
        file_path: "src/z.cpp",
        line: 1,
        symbol_role: "definition",
      },
      {
        id: "u2",
        qualified_name: "ns::Plain",
        type: "function",
        file_path: "src/a.cpp",
        line: 1,
        symbol_role: "definition",
      },
    ]);
    const store = new SqliteStore(dbPath);
    try {
      expect(store.getSymbolByQualifiedName("ns::Plain")?.filePath).toBe("src/a.cpp");
    } finally {
      store.close();
    }
  });

  it("returns undefined for an unknown qualified name", () => {
    const dbPath = makeAnchorDb([]);
    const store = new SqliteStore(dbPath);
    try {
      expect(store.getSymbolByQualifiedName("nope::missing")).toBeUndefined();
    } finally {
      store.close();
    }
  });
});
