import * as fs from "fs";
import * as os from "os";
import * as path from "path";
import { __testables } from "../storage/sqlite-store";

describe("sqlite-store active DB pointer helpers", () => {
  it("resolves a versioned database through current-db.json", () => {
    const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), "codeatlas-pointer-test-"));
    const dataDir = path.join(tempRoot, ".codeatlas");
    fs.mkdirSync(dataDir, { recursive: true });
    const generationPath = path.join(dataDir, "index-20260420T131500123Z.db");
    fs.writeFileSync(generationPath, "sqlite placeholder");
    fs.writeFileSync(
      path.join(dataDir, "current-db.json"),
      JSON.stringify({
        active_db_filename: path.basename(generationPath),
        published_at: "2026-04-20T13:15:00Z",
        format_version: 1,
      }, null, 2),
    );

    expect(__testables.resolveActiveDatabasePath(dataDir)).toBe(generationPath);

    fs.rmSync(tempRoot, { recursive: true, force: true });
  });

  it("falls back to legacy index.db when the pointer is absent", () => {
    const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), "codeatlas-pointer-test-"));
    const dataDir = path.join(tempRoot, ".codeatlas");
    fs.mkdirSync(dataDir, { recursive: true });
    const legacyPath = path.join(dataDir, "index.db");
    fs.writeFileSync(legacyPath, "sqlite placeholder");

    expect(__testables.resolveActiveDatabasePath(dataDir)).toBe(legacyPath);

    fs.rmSync(tempRoot, { recursive: true, force: true });
  });
});
