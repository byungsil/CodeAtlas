import * as fs from "fs";
import * as path from "path";

const SAMPLES_DIR = path.resolve(__dirname, "../../../samples/.codeatlas");

describe("Schema and contract validation", () => {
  let symbols: any[];
  let calls: any[];
  let files: any[];

  beforeAll(() => {
    symbols = JSON.parse(fs.readFileSync(path.join(SAMPLES_DIR, "symbols.json"), "utf-8"));
    calls = JSON.parse(fs.readFileSync(path.join(SAMPLES_DIR, "calls.json"), "utf-8"));
    files = JSON.parse(fs.readFileSync(path.join(SAMPLES_DIR, "files.json"), "utf-8"));
  });

  describe("Symbol model", () => {
    it("has required fields", () => {
      for (const s of symbols) {
        expect(s).toHaveProperty("id");
        expect(s).toHaveProperty("name");
        expect(s).toHaveProperty("qualifiedName");
        expect(s).toHaveProperty("type");
        expect(s).toHaveProperty("filePath");
        expect(s).toHaveProperty("line");
        expect(s).toHaveProperty("endLine");
      }
    });

    it("has valid symbol types", () => {
      const validTypes = ["function", "method", "class", "struct", "enum", "enumMember", "namespace", "variable", "typedef"];
      for (const s of symbols) {
        expect(validTypes).toContain(s.type);
      }
    });

    it("has positive line numbers", () => {
      for (const s of symbols) {
        expect(s.line).toBeGreaterThan(0);
        expect(s.endLine).toBeGreaterThanOrEqual(s.line);
      }
    });
  });

  describe("Call model", () => {
    it("has required fields", () => {
      for (const c of calls) {
        expect(c).toHaveProperty("callerId");
        expect(c).toHaveProperty("calleeId");
        expect(c).toHaveProperty("filePath");
        expect(c).toHaveProperty("line");
      }
    });

    it("references existing symbols", () => {
      const symbolIds = new Set(symbols.map((s: any) => s.id));
      for (const c of calls) {
        expect(symbolIds.has(c.callerId)).toBe(true);
        expect(symbolIds.has(c.calleeId)).toBe(true);
      }
    });
  });

  describe("FileRecord model", () => {
    it("has required fields", () => {
      for (const f of files) {
        expect(f).toHaveProperty("path");
        expect(f).toHaveProperty("contentHash");
        expect(f).toHaveProperty("lastIndexed");
        expect(f).toHaveProperty("symbolCount");
      }
    });
  });

  describe("Path normalization", () => {
    it("symbols contain no absolute paths", () => {
      for (const s of symbols) {
        expect(s.filePath).not.toMatch(/^[A-Z]:/i);
        expect(s.filePath).not.toMatch(/^\//);
      }
    });

    it("calls contain no absolute paths", () => {
      for (const c of calls) {
        expect(c.filePath).not.toMatch(/^[A-Z]:/i);
        expect(c.filePath).not.toMatch(/^\//);
      }
    });

    it("files contain no absolute paths", () => {
      for (const f of files) {
        expect(f.path).not.toMatch(/^[A-Z]:/i);
        expect(f.path).not.toMatch(/^\//);
      }
    });

    it("paths use forward slashes", () => {
      for (const s of symbols) {
        expect(s.filePath).not.toContain("\\");
      }
      for (const f of files) {
        expect(f.path).not.toContain("\\");
      }
    });
  });

  describe("Sample dataset determinism", () => {
    it("has expected symbol count", () => {
      expect(symbols.length).toBeGreaterThanOrEqual(20);
    });

    it("has expected call count", () => {
      expect(calls.length).toBeGreaterThanOrEqual(15);
    });

    it("has 5 indexed files", () => {
      expect(files.length).toBe(5);
    });
  });
});
