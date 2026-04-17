use std::collections::HashMap;
use std::path::Path;
use rusqlite::{params, params_from_iter, Connection, Result as SqlResult};
use crate::models::{Call, FileRecord, Symbol};
use crate::resolver;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS symbols_raw (
    id          TEXT NOT NULL,
    name        TEXT NOT NULL,
    qualified_name TEXT NOT NULL,
    type        TEXT NOT NULL,
    file_path   TEXT NOT NULL,
    line        INTEGER NOT NULL,
    end_line    INTEGER NOT NULL,
    signature   TEXT,
    parent_id   TEXT
);

CREATE TABLE IF NOT EXISTS symbols (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    qualified_name TEXT NOT NULL,
    type        TEXT NOT NULL,
    file_path   TEXT NOT NULL,
    line        INTEGER NOT NULL,
    end_line    INTEGER NOT NULL,
    signature   TEXT,
    parent_id   TEXT
);

CREATE TABLE IF NOT EXISTS calls (
    caller_id   TEXT NOT NULL,
    callee_id   TEXT NOT NULL,
    file_path   TEXT NOT NULL,
    line        INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS files (
    path            TEXT PRIMARY KEY,
    content_hash    TEXT NOT NULL,
    last_indexed    TEXT NOT NULL,
    symbol_count    INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_symbols_raw_id ON symbols_raw(id);
CREATE INDEX IF NOT EXISTS idx_symbols_raw_name ON symbols_raw(name);
CREATE INDEX IF NOT EXISTS idx_symbols_raw_file ON symbols_raw(file_path);
CREATE INDEX IF NOT EXISTS idx_symbols_name ON symbols(name);
CREATE INDEX IF NOT EXISTS idx_symbols_file ON symbols(file_path);
CREATE INDEX IF NOT EXISTS idx_symbols_parent ON symbols(parent_id);
CREATE INDEX IF NOT EXISTS idx_calls_caller ON calls(caller_id);
CREATE INDEX IF NOT EXISTS idx_calls_callee ON calls(callee_id);
CREATE INDEX IF NOT EXISTS idx_calls_file ON calls(file_path);
CREATE INDEX IF NOT EXISTS idx_files_path ON files(path);

CREATE VIRTUAL TABLE IF NOT EXISTS symbols_fts USING fts5(id, name, qualified_name, tokenize='trigram');
"#;

pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn open(path: &Path) -> SqlResult<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;
        conn.execute_batch(SCHEMA)?;
        Self::migrate_symbol_storage(&conn)?;
        Self::migrate_fts(&conn)?;
        Ok(Database { conn })
    }

    fn migrate_symbol_storage(conn: &Connection) -> SqlResult<()> {
        let raw_count: i64 = conn.query_row("SELECT COUNT(*) FROM symbols_raw", [], |r| r.get(0))?;
        let symbol_count: i64 = conn.query_row("SELECT COUNT(*) FROM symbols", [], |r| r.get(0))?;

        if raw_count == 0 && symbol_count > 0 {
            conn.execute_batch(
                "INSERT INTO symbols_raw(id, name, qualified_name, type, file_path, line, end_line, signature, parent_id)
                 SELECT id, name, qualified_name, type, file_path, line, end_line, signature, parent_id FROM symbols;",
            )?;
        }

        Ok(())
    }

    fn migrate_fts(conn: &Connection) -> SqlResult<()> {
        let has_fts: bool = conn.query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='symbols_fts'",
            [], |r| r.get(0),
        ).unwrap_or(false);

        if !has_fts {
            return Ok(());
        }

        let has_id_col: bool = conn.prepare("SELECT id FROM symbols_fts LIMIT 0")
            .is_ok();

        if !has_id_col {
            conn.execute_batch("DROP TABLE symbols_fts;")?;
            conn.execute_batch("CREATE VIRTUAL TABLE IF NOT EXISTS symbols_fts USING fts5(id, name, qualified_name, tokenize='trigram');")?;
            conn.execute_batch("INSERT INTO symbols_fts(id, name, qualified_name) SELECT id, name, qualified_name FROM symbols;")?;
        }

        Ok(())
    }

    pub fn clear(&self) -> SqlResult<()> {
        self.conn.execute_batch(
            "DELETE FROM symbols_raw; DELETE FROM symbols; DELETE FROM calls; DELETE FROM files; DELETE FROM symbols_fts;",
        )
    }

    pub fn rebuild_fts(&self) -> SqlResult<()> {
        self.conn.execute_batch(
            "DELETE FROM symbols_fts; INSERT INTO symbols_fts(id, name, qualified_name) SELECT id, name, qualified_name FROM symbols;",
        )
    }

    pub fn write_raw_symbols(&self, symbols: &[Symbol]) -> SqlResult<()> {
        let mut stmt = self.conn.prepare(
            "INSERT INTO symbols_raw (id, name, qualified_name, type, file_path, line, end_line, signature, parent_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        )?;
        for s in symbols {
            stmt.execute(params![
                s.id,
                s.name,
                s.qualified_name,
                s.symbol_type,
                s.file_path,
                s.line,
                s.end_line,
                s.signature,
                s.parent_id,
            ])?;
        }
        Ok(())
    }

    pub fn write_symbols(&self, symbols: &[Symbol]) -> SqlResult<()> {
        let mut stmt = self.conn.prepare(
            "INSERT OR REPLACE INTO symbols (id, name, qualified_name, type, file_path, line, end_line, signature, parent_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        )?;
        for s in symbols {
            stmt.execute(params![
                s.id,
                s.name,
                s.qualified_name,
                s.symbol_type,
                s.file_path,
                s.line,
                s.end_line,
                s.signature,
                s.parent_id,
            ])?;
        }
        Ok(())
    }

    pub fn write_calls(&self, calls: &[Call]) -> SqlResult<()> {
        let mut stmt = self.conn.prepare(
            "INSERT INTO calls (caller_id, callee_id, file_path, line) VALUES (?1, ?2, ?3, ?4)",
        )?;
        for c in calls {
            stmt.execute(params![c.caller_id, c.callee_id, c.file_path, c.line])?;
        }
        Ok(())
    }

    pub fn write_files(&self, files: &[FileRecord]) -> SqlResult<()> {
        let mut stmt = self.conn.prepare(
            "INSERT OR REPLACE INTO files (path, content_hash, last_indexed, symbol_count) VALUES (?1, ?2, ?3, ?4)",
        )?;
        for f in files {
            stmt.execute(params![f.path, f.content_hash, f.last_indexed, f.symbol_count])?;
        }
        Ok(())
    }

    pub fn write_all(
        &self,
        raw_symbols: &[Symbol],
        representative_symbols: &[Symbol],
        calls: &[Call],
        files: &[FileRecord],
    ) -> SqlResult<()> {
        self.conn.execute_batch("BEGIN TRANSACTION;")?;
        self.clear()?;
        self.write_raw_symbols(raw_symbols)?;
        self.write_symbols(representative_symbols)?;
        self.write_calls(calls)?;
        self.write_files(files)?;
        self.rebuild_fts()?;
        self.conn.execute_batch("COMMIT;")?;
        Ok(())
    }

    pub fn read_file_records(&self) -> SqlResult<Vec<FileRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT path, content_hash, last_indexed, symbol_count FROM files",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(FileRecord {
                path: row.get(0)?,
                content_hash: row.get(1)?,
                last_indexed: row.get(2)?,
                symbol_count: row.get(3)?,
            })
        })?;
        rows.collect()
    }

    pub fn delete_file_record(&self, file_path: &str) -> SqlResult<()> {
        self.conn.execute("DELETE FROM files WHERE path = ?1", params![file_path])?;
        Ok(())
    }

    pub fn delete_raw_symbols_for_file(&self, file_path: &str) -> SqlResult<()> {
        self.conn.execute("DELETE FROM symbols_raw WHERE file_path = ?1", params![file_path])?;
        Ok(())
    }

    pub fn cleanup_dangling_calls(&self) -> SqlResult<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT file_path FROM calls WHERE callee_id NOT IN (SELECT id FROM symbols)",
        )?;
        let affected: Vec<String> = stmt.query_map([], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();

        self.conn.execute(
            "DELETE FROM calls WHERE callee_id NOT IN (SELECT id FROM symbols)",
            [],
        )?;

        Ok(affected)
    }

    pub fn delete_calls_for_file(&self, file_path: &str) -> SqlResult<()> {
        self.conn.execute("DELETE FROM calls WHERE file_path = ?1", params![file_path])?;
        Ok(())
    }

    pub fn read_symbols_for_paths(&self, file_paths: &[String]) -> SqlResult<Vec<Symbol>> {
        if file_paths.is_empty() {
            return Ok(Vec::new());
        }

        let placeholders = vec!["?"; file_paths.len()].join(", ");
        let sql = format!(
            "SELECT id, name, qualified_name, type, file_path, line, end_line, signature, parent_id
             FROM symbols_raw WHERE file_path IN ({})",
            placeholders,
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(file_paths.iter()), |row| {
            Ok(Symbol {
                id: row.get(0)?,
                name: row.get(1)?,
                qualified_name: row.get(2)?,
                symbol_type: row.get(3)?,
                file_path: row.get(4)?,
                line: row.get(5)?,
                end_line: row.get(6)?,
                signature: row.get(7)?,
                parent_id: row.get(8)?,
            })
        })?;
        rows.collect()
    }

    fn read_all_raw_symbols(&self) -> SqlResult<Vec<Symbol>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, qualified_name, type, file_path, line, end_line, signature, parent_id FROM symbols_raw",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(Symbol {
                id: row.get(0)?,
                name: row.get(1)?,
                qualified_name: row.get(2)?,
                symbol_type: row.get(3)?,
                file_path: row.get(4)?,
                line: row.get(5)?,
                end_line: row.get(6)?,
                signature: row.get(7)?,
                parent_id: row.get(8)?,
            })
        })?;
        rows.collect()
    }

    fn read_raw_symbols_by_ids(&self, symbol_ids: &[String]) -> SqlResult<Vec<Symbol>> {
        if symbol_ids.is_empty() {
            return Ok(Vec::new());
        }

        let placeholders = vec!["?"; symbol_ids.len()].join(", ");
        let sql = format!(
            "SELECT id, name, qualified_name, type, file_path, line, end_line, signature, parent_id
             FROM symbols_raw WHERE id IN ({})",
            placeholders,
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(symbol_ids.iter()), |row| {
            Ok(Symbol {
                id: row.get(0)?,
                name: row.get(1)?,
                qualified_name: row.get(2)?,
                symbol_type: row.get(3)?,
                file_path: row.get(4)?,
                line: row.get(5)?,
                end_line: row.get(6)?,
                signature: row.get(7)?,
                parent_id: row.get(8)?,
            })
        })?;
        rows.collect()
    }

    pub fn find_symbols_by_name(&self, name: &str) -> SqlResult<Vec<Symbol>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, qualified_name, type, file_path, line, end_line, signature, parent_id FROM symbols WHERE name = ?1 AND (type = 'function' OR type = 'method')",
        )?;
        let rows = stmt.query_map(params![name], |row| {
            Ok(Symbol {
                id: row.get(0)?,
                name: row.get(1)?,
                qualified_name: row.get(2)?,
                symbol_type: row.get(3)?,
                file_path: row.get(4)?,
                line: row.get(5)?,
                end_line: row.get(6)?,
                signature: row.get(7)?,
                parent_id: row.get(8)?,
            })
        })?;
        rows.collect()
    }

    pub fn find_parent_ids(&self, symbol_ids: &[String]) -> SqlResult<HashMap<String, String>> {
        let mut parents = HashMap::new();
        if symbol_ids.is_empty() {
            return Ok(parents);
        }

        let placeholders = vec!["?"; symbol_ids.len()].join(", ");
        let sql = format!(
            "SELECT id, parent_id FROM symbols WHERE id IN ({}) AND parent_id IS NOT NULL",
            placeholders,
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(symbol_ids.iter()), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;

        for row in rows {
            let (id, parent_id) = row?;
            parents.insert(id, parent_id);
        }

        Ok(parents)
    }

    pub fn find_symbols_by_ids(&self, symbol_ids: &[String]) -> SqlResult<Vec<Symbol>> {
        if symbol_ids.is_empty() {
            return Ok(Vec::new());
        }

        let placeholders = vec!["?"; symbol_ids.len()].join(", ");
        let sql = format!(
            "SELECT id, name, qualified_name, type, file_path, line, end_line, signature, parent_id
             FROM symbols WHERE id IN ({})",
            placeholders,
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(symbol_ids.iter()), |row| {
            Ok(Symbol {
                id: row.get(0)?,
                name: row.get(1)?,
                qualified_name: row.get(2)?,
                symbol_type: row.get(3)?,
                file_path: row.get(4)?,
                line: row.get(5)?,
                end_line: row.get(6)?,
                signature: row.get(7)?,
                parent_id: row.get(8)?,
            })
        })?;
        rows.collect()
    }

    pub fn refresh_symbols_for_ids(&self, symbol_ids: &[String]) -> SqlResult<()> {
        if symbol_ids.is_empty() {
            return Ok(());
        }

        let placeholders = vec!["?"; symbol_ids.len()].join(", ");
        let delete_sql = format!("DELETE FROM symbols WHERE id IN ({})", placeholders);
        self.conn.execute(&delete_sql, params_from_iter(symbol_ids.iter()))?;

        let raw_symbols = self.read_raw_symbols_by_ids(symbol_ids)?;
        let merged = resolver::merge_symbols(&raw_symbols);
        if !merged.is_empty() {
            self.write_symbols(&merged)?;
        }
        Ok(())
    }

    pub fn refresh_fts_for_symbol_ids(&self, symbol_ids: &[String]) -> SqlResult<()> {
        if symbol_ids.is_empty() {
            return Ok(());
        }

        let placeholders = vec!["?"; symbol_ids.len()].join(", ");
        let delete_sql = format!("DELETE FROM symbols_fts WHERE id IN ({})", placeholders);
        let insert_sql = format!(
            "INSERT INTO symbols_fts(id, name, qualified_name)
             SELECT id, name, qualified_name FROM symbols WHERE id IN ({})",
            placeholders,
        );

        self.conn.execute(&delete_sql, params_from_iter(symbol_ids.iter()))?;
        self.conn.execute(&insert_sql, params_from_iter(symbol_ids.iter()))?;
        Ok(())
    }

    pub fn count_symbols(&self) -> SqlResult<i64> {
        self.conn.query_row("SELECT COUNT(*) FROM symbols", [], |r| r.get(0))
    }

    pub fn count_calls(&self) -> SqlResult<i64> {
        self.conn.query_row("SELECT COUNT(*) FROM calls", [], |r| r.get(0))
    }

    pub fn count_files(&self) -> SqlResult<i64> {
        self.conn.query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0))
    }

    pub fn has_data(&self) -> bool {
        self.count_files().unwrap_or(0) > 0
    }

    pub fn begin(&self) -> SqlResult<()> {
        self.conn.execute_batch("BEGIN TRANSACTION;")
    }

    pub fn commit(&self) -> SqlResult<()> {
        self.conn.execute_batch("COMMIT;")
    }

    pub fn rollback(&self) -> SqlResult<()> {
        self.conn.execute_batch("ROLLBACK;")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_sym(id: &str, name: &str) -> Symbol {
        Symbol {
            id: id.into(), name: name.into(), qualified_name: id.into(),
            symbol_type: "function".into(), file_path: "test.cpp".into(),
            line: 1, end_line: 5, signature: Some("void foo()".into()), parent_id: None,
        }
    }

    #[test]
    fn creates_schema_and_writes() {
        let db = Database::open(Path::new(":memory:")).unwrap();
        let symbols = vec![make_sym("foo", "foo"), make_sym("bar", "bar")];
        let calls = vec![Call {
            caller_id: "foo".into(), callee_id: "bar".into(),
            file_path: "test.cpp".into(), line: 3,
        }];
        let files = vec![FileRecord {
            path: "test.cpp".into(), content_hash: "abc".into(),
            last_indexed: "2026-01-01T00:00:00Z".into(), symbol_count: 2,
        }];

        db.write_all(&symbols, &symbols, &calls, &files).unwrap();

        let count: i64 = db.conn.query_row("SELECT COUNT(*) FROM symbols", [], |r| r.get(0)).unwrap();
        assert_eq!(count, 2);
        let call_count: i64 = db.conn.query_row("SELECT COUNT(*) FROM calls", [], |r| r.get(0)).unwrap();
        assert_eq!(call_count, 1);
        let file_count: i64 = db.conn.query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0)).unwrap();
        assert_eq!(file_count, 1);
    }

    #[test]
    fn clear_removes_all_data() {
        let db = Database::open(Path::new(":memory:")).unwrap();
        db.write_raw_symbols(&[make_sym("foo", "foo")]).unwrap();
        db.refresh_symbols_for_ids(&["foo".into()]).unwrap();
        db.clear().unwrap();
        let count: i64 = db.conn.query_row("SELECT COUNT(*) FROM symbols", [], |r| r.get(0)).unwrap();
        assert_eq!(count, 0);
        let raw_count: i64 = db.conn.query_row("SELECT COUNT(*) FROM symbols_raw", [], |r| r.get(0)).unwrap();
        assert_eq!(raw_count, 0);
    }

    #[test]
    fn indexes_exist() {
        let db = Database::open(Path::new(":memory:")).unwrap();
        let idx_count: i64 = db.conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name LIKE 'idx_%'",
            [], |r| r.get(0),
        ).unwrap();
        assert_eq!(idx_count, 10);
    }

    #[test]
    fn representative_symbol_falls_back_to_header_when_cpp_variant_is_removed() {
        let db = Database::open(Path::new(":memory:")).unwrap();
        let header = Symbol {
            id: "Foo::Bar".into(),
            name: "Bar".into(),
            qualified_name: "Foo::Bar".into(),
            symbol_type: "method".into(),
            file_path: "foo.h".into(),
            line: 5,
            end_line: 5,
            signature: Some("void Bar()".into()),
            parent_id: Some("Foo".into()),
        };
        let source = Symbol {
            id: "Foo::Bar".into(),
            name: "Bar".into(),
            qualified_name: "Foo::Bar".into(),
            symbol_type: "method".into(),
            file_path: "foo.cpp".into(),
            line: 10,
            end_line: 15,
            signature: Some("void Foo::Bar()".into()),
            parent_id: Some("Foo".into()),
        };

        db.write_raw_symbols(&[header.clone(), source]).unwrap();
        db.refresh_symbols_for_ids(&["Foo::Bar".into()]).unwrap();
        let before = db.find_symbols_by_ids(&["Foo::Bar".into()]).unwrap();
        assert_eq!(before.len(), 1);
        assert_eq!(before[0].file_path, "foo.cpp");

        db.delete_raw_symbols_for_file("foo.cpp").unwrap();
        db.refresh_symbols_for_ids(&["Foo::Bar".into()]).unwrap();

        let after = db.find_symbols_by_ids(&["Foo::Bar".into()]).unwrap();
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].file_path, "foo.h");
        assert_eq!(after[0].line, 5);
    }
}
