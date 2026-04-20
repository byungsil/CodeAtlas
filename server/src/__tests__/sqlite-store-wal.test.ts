import * as fs from "fs";
import * as os from "os";
import * as path from "path";
import Database from "better-sqlite3";
import { __testables } from "../storage/sqlite-store";

describe("sqlite-store WAL snapshot helpers", () => {
  it("copies and removes WAL sidecar files with the snapshot family", () => {
    const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), "codeatlas-wal-test-"));
    const sourcePath = path.join(tempRoot, "source.db");
    const snapshotPath = path.join(tempRoot, "snapshot.db");
    const db = new Database(sourcePath);

    db.pragma("journal_mode = WAL");
    db.exec("CREATE TABLE sample(id INTEGER PRIMARY KEY, value TEXT);");
    db.exec("INSERT INTO sample(value) VALUES ('x');");
    db.close();

    __testables.copySnapshotDatabaseFamily(sourcePath, snapshotPath);

    expect(fs.existsSync(snapshotPath)).toBe(true);
    for (const sidecar of __testables.snapshotCompanionPaths(sourcePath)) {
      if (fs.existsSync(sidecar)) {
        const snapshotSidecar = sidecar.replace(sourcePath, snapshotPath);
        expect(fs.existsSync(snapshotSidecar)).toBe(true);
      }
    }

    __testables.deleteSnapshotDatabaseFamily(snapshotPath);
    expect(fs.existsSync(snapshotPath)).toBe(false);
    for (const sidecar of __testables.snapshotCompanionPaths(snapshotPath)) {
      expect(fs.existsSync(sidecar)).toBe(false);
    }

    fs.rmSync(tempRoot, { recursive: true, force: true });
  });
});
