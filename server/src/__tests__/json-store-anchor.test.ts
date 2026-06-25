import * as fs from "fs";
import * as os from "os";
import * as path from "path";
import { JsonStore } from "../storage/json-store";
import { Symbol } from "../models/symbol";

// MS24: JsonStore must mirror SqliteStore's deterministic anchor selection
// for cross-USR qualified names. Keeping the two stores in lockstep matters
// because the JSON fallback is used in dev / tests where the SQLite path is
// unavailable.

function makeStore(symbols: Symbol[]): JsonStore {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "codeatlas-json-anchor-"));
  const store = new JsonStore(dir);
  store.save({
    symbols,
    calls: [],
    references: [],
    propagationEvents: [],
    files: [],
  });
  return store;
}

function sym(partial: Partial<Symbol> & Pick<Symbol, "id" | "qualifiedName" | "filePath">): Symbol {
  return {
    name: partial.qualifiedName.split("::").pop() ?? partial.qualifiedName,
    type: "function",
    line: 1,
    endLine: 1,
    ...partial,
  } as Symbol;
}

describe("JsonStore.getSymbolByQualifiedName (MS24 anchor weighting)", () => {
  it("prefers public-header declaration over out-of-line definitions", () => {
    const store = makeStore([
      sym({
        id: "c:@N@cv@F@imread#&1I#I#",
        qualifiedName: "cv::imread",
        filePath: "modules/imgcodecs/src/loadsave.cpp",
        line: 754,
        symbolRole: "definition",
        artifactKind: "runtime",
      }),
      sym({
        id: "cv::imread",
        qualifiedName: "cv::imread",
        filePath: "modules/imgcodecs/include/opencv2/imgcodecs.hpp",
        line: 384,
        symbolRole: "declaration",
        headerRole: "public",
        artifactKind: "runtime",
      }),
      sym({
        id: "c:@N@cv@F@imread#&1I#I#I#",
        qualifiedName: "cv::imread",
        filePath: "modules/imgcodecs/src/loadsave.cpp",
        line: 785,
        symbolRole: "definition",
        artifactKind: "runtime",
      }),
    ]);

    const got = store.getSymbolByQualifiedName("cv::imread");
    expect(got?.filePath).toBe("modules/imgcodecs/include/opencv2/imgcodecs.hpp");
    expect(got?.symbolRole).toBe("declaration");
  });

  it("demotes .inl.hpp inline definitions below primary .hpp definitions", () => {
    const store = makeStore([
      sym({
        id: "u1",
        qualifiedName: "cv::Mat::Mat",
        filePath: "modules/core/include/opencv2/core/cuda.inl.hpp",
        line: 752,
        symbolRole: "definition",
        headerRole: "public",
      }),
      sym({
        id: "u2",
        qualifiedName: "cv::Mat::Mat",
        filePath: "modules/core/include/opencv2/core/mat.hpp",
        line: 100,
        symbolRole: "definition",
        headerRole: "public",
      }),
    ]);
    expect(store.getSymbolByQualifiedName("cv::Mat::Mat")?.filePath).toBe(
      "modules/core/include/opencv2/core/mat.hpp",
    );
  });

  it("is deterministic across repeated calls", () => {
    const store = makeStore([
      sym({ id: "u1", qualifiedName: "ns::S", filePath: "src/c.cpp", symbolRole: "definition" }),
      sym({ id: "u2", qualifiedName: "ns::S", filePath: "src/a.cpp", symbolRole: "definition" }),
      sym({ id: "u3", qualifiedName: "ns::S", filePath: "src/b.cpp", symbolRole: "definition" }),
    ]);
    const first = store.getSymbolByQualifiedName("ns::S")?.filePath;
    expect(first).toBe("src/a.cpp"); // lexical tie-break
    for (let i = 0; i < 9; i++) {
      expect(store.getSymbolByQualifiedName("ns::S")?.filePath).toBe(first);
    }
  });

  it("returns undefined when no symbol matches", () => {
    const store = makeStore([]);
    expect(store.getSymbolByQualifiedName("nope")).toBeUndefined();
  });
});
