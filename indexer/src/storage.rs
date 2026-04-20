use std::collections::{HashMap, HashSet};
use std::env;
use std::path::Path;
use rusqlite::{params, params_from_iter, Connection, Result as SqlResult};
use crate::models::{
    Call, CallableFlowSummary, FileRecord, NormalizedReference, PropagationAnchorKind,
    PropagationEvent, PropagationKind, RawExtractionConfidence, RawCallKind, RawCallSite,
    RawQualifierKind, RawReceiverKind, ReferenceCategory, Symbol,
};
#[cfg(test)]
use crate::models::InheritanceEdge;
use crate::resolver;

const DB_SCHEMA_VERSION: i64 = 1;
const INDEX_FORMAT_VERSION: u32 = 1;

const SYMBOL_SELECT_COLUMNS: &str = "id, name, qualified_name, type, file_path, line, end_line, signature, parameter_count, scope_qualified_name, scope_kind, symbol_role, declaration_file_path, declaration_line, declaration_end_line, definition_file_path, definition_line, definition_end_line, parent_id, module, subsystem, project_area, artifact_kind, header_role, parse_fragility, macro_sensitivity, include_heaviness";

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
    parameter_count INTEGER,
    scope_qualified_name TEXT,
    scope_kind  TEXT,
    symbol_role TEXT,
    declaration_file_path TEXT,
    declaration_line INTEGER,
    declaration_end_line INTEGER,
    definition_file_path TEXT,
    definition_line INTEGER,
    definition_end_line INTEGER,
    parent_id   TEXT,
    module      TEXT,
    subsystem   TEXT,
    project_area TEXT,
    artifact_kind TEXT,
    header_role TEXT,
    parse_fragility TEXT,
    macro_sensitivity TEXT,
    include_heaviness TEXT
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
    parameter_count INTEGER,
    scope_qualified_name TEXT,
    scope_kind  TEXT,
    symbol_role TEXT,
    declaration_file_path TEXT,
    declaration_line INTEGER,
    declaration_end_line INTEGER,
    definition_file_path TEXT,
    definition_line INTEGER,
    definition_end_line INTEGER,
    parent_id   TEXT,
    module      TEXT,
    subsystem   TEXT,
    project_area TEXT,
    artifact_kind TEXT,
    header_role TEXT,
    parse_fragility TEXT,
    macro_sensitivity TEXT,
    include_heaviness TEXT
);

CREATE TABLE IF NOT EXISTS calls (
    caller_id   TEXT NOT NULL,
    callee_id   TEXT NOT NULL,
    file_path   TEXT NOT NULL,
    line        INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS raw_calls (
    caller_id   TEXT NOT NULL,
    called_name TEXT NOT NULL,
    call_kind   TEXT NOT NULL,
    argument_count INTEGER,
    argument_texts_json TEXT NOT NULL,
    result_target_json TEXT,
    receiver    TEXT,
    receiver_kind TEXT,
    qualifier   TEXT,
    qualifier_kind TEXT,
    file_path   TEXT NOT NULL,
    line        INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS symbol_references (
    source_symbol_id TEXT NOT NULL,
    target_symbol_id TEXT NOT NULL,
    category    TEXT NOT NULL,
    file_path   TEXT NOT NULL,
    line        INTEGER NOT NULL,
    confidence  TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS propagation_events (
    owner_symbol_id TEXT,
    source_anchor_id TEXT,
    source_symbol_id TEXT,
    source_expression_text TEXT,
    source_anchor_kind TEXT NOT NULL,
    target_anchor_id TEXT,
    target_symbol_id TEXT,
    target_expression_text TEXT,
    target_anchor_kind TEXT NOT NULL,
    propagation_kind TEXT NOT NULL,
    file_path   TEXT NOT NULL,
    line        INTEGER NOT NULL,
    confidence  TEXT NOT NULL,
    risks       TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS callable_flow_summaries (
    callable_symbol_id TEXT PRIMARY KEY,
    file_path   TEXT NOT NULL,
    parameter_anchors_json TEXT NOT NULL,
    return_anchors_json TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS files (
    path            TEXT PRIMARY KEY,
    content_hash    TEXT NOT NULL,
    last_indexed    TEXT NOT NULL,
    symbol_count    INTEGER NOT NULL,
    module          TEXT,
    subsystem       TEXT,
    project_area    TEXT,
    artifact_kind   TEXT,
    header_role     TEXT,
    parse_fragility TEXT,
    macro_sensitivity TEXT,
    include_heaviness TEXT
);

CREATE TABLE IF NOT EXISTS db_metadata (
    key         TEXT PRIMARY KEY,
    value       TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_symbols_raw_id ON symbols_raw(id);
CREATE INDEX IF NOT EXISTS idx_symbols_raw_name ON symbols_raw(name);
CREATE INDEX IF NOT EXISTS idx_symbols_raw_file ON symbols_raw(file_path);
CREATE INDEX IF NOT EXISTS idx_symbols_name ON symbols(name);
CREATE INDEX IF NOT EXISTS idx_symbols_qualified ON symbols(qualified_name);
CREATE INDEX IF NOT EXISTS idx_symbols_file ON symbols(file_path);
CREATE INDEX IF NOT EXISTS idx_symbols_file_order ON symbols(file_path, line, end_line, qualified_name);
CREATE INDEX IF NOT EXISTS idx_symbols_parent ON symbols(parent_id);
CREATE INDEX IF NOT EXISTS idx_symbols_scope ON symbols(scope_kind, scope_qualified_name, line, end_line, qualified_name);
CREATE INDEX IF NOT EXISTS idx_calls_caller ON calls(caller_id);
CREATE INDEX IF NOT EXISTS idx_calls_callee ON calls(callee_id);
CREATE INDEX IF NOT EXISTS idx_calls_caller_order ON calls(caller_id, file_path, line, callee_id);
CREATE INDEX IF NOT EXISTS idx_calls_callee_order ON calls(callee_id, file_path, line, caller_id);
CREATE INDEX IF NOT EXISTS idx_calls_file ON calls(file_path);
CREATE INDEX IF NOT EXISTS idx_raw_calls_caller_order ON raw_calls(caller_id, file_path, line, called_name);
CREATE INDEX IF NOT EXISTS idx_raw_calls_file ON raw_calls(file_path);
CREATE INDEX IF NOT EXISTS idx_references_source ON symbol_references(source_symbol_id);
CREATE INDEX IF NOT EXISTS idx_references_target ON symbol_references(target_symbol_id);
CREATE INDEX IF NOT EXISTS idx_references_target_category_file ON symbol_references(target_symbol_id, category, file_path, line, source_symbol_id);
CREATE INDEX IF NOT EXISTS idx_references_category ON symbol_references(category);
CREATE INDEX IF NOT EXISTS idx_references_file ON symbol_references(file_path);
CREATE INDEX IF NOT EXISTS idx_propagation_owner ON propagation_events(owner_symbol_id);
CREATE INDEX IF NOT EXISTS idx_propagation_source_symbol ON propagation_events(source_symbol_id);
CREATE INDEX IF NOT EXISTS idx_propagation_target_symbol ON propagation_events(target_symbol_id);
CREATE INDEX IF NOT EXISTS idx_propagation_source_anchor ON propagation_events(source_anchor_id);
CREATE INDEX IF NOT EXISTS idx_propagation_target_anchor ON propagation_events(target_anchor_id);
CREATE INDEX IF NOT EXISTS idx_propagation_source_kind_file ON propagation_events(source_symbol_id, propagation_kind, file_path, line);
CREATE INDEX IF NOT EXISTS idx_propagation_target_kind_file ON propagation_events(target_symbol_id, propagation_kind, file_path, line);
CREATE INDEX IF NOT EXISTS idx_propagation_file ON propagation_events(file_path);
CREATE INDEX IF NOT EXISTS idx_callable_flow_summaries_file ON callable_flow_summaries(file_path);
CREATE INDEX IF NOT EXISTS idx_files_path ON files(path);

CREATE VIRTUAL TABLE IF NOT EXISTS symbols_fts USING fts5(id, name, qualified_name, tokenize='trigram');
"#;

pub struct Database {
    conn: Connection,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexMetadata {
    pub format_version: u32,
    pub indexer_version: String,
    pub workspace_root: String,
    pub extensions_csv: String,
}

impl IndexMetadata {
    pub fn mismatch_reason(&self, expected: &Self) -> Option<String> {
        if self.format_version != expected.format_version {
            return Some(format!(
                "index format version mismatch (db={}, current={})",
                self.format_version, expected.format_version
            ));
        }
        if self.workspace_root != expected.workspace_root {
            return Some(format!(
                "workspace root mismatch (db={}, current={})",
                self.workspace_root, expected.workspace_root
            ));
        }
        if self.extensions_csv != expected.extensions_csv {
            return Some(format!(
                "indexed extensions changed (db={}, current={})",
                self.extensions_csv, expected.extensions_csv
            ));
        }
        None
    }
}

impl Database {
    pub fn open(path: &Path) -> SqlResult<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=DELETE; PRAGMA synchronous=NORMAL;")?;
        conn.execute_batch(SCHEMA)?;
        Self::migrate_symbol_storage(&conn)?;
        Self::migrate_symbol_metadata(&conn)?;
        Self::migrate_fts(&conn)?;
        conn.pragma_update(None, "user_version", DB_SCHEMA_VERSION)?;
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

    fn migrate_symbol_metadata(conn: &Connection) -> SqlResult<()> {
        Self::ensure_column(conn, "symbols_raw", "parameter_count", "INTEGER")?;
        Self::ensure_column(conn, "symbols_raw", "scope_qualified_name", "TEXT")?;
        Self::ensure_column(conn, "symbols_raw", "scope_kind", "TEXT")?;
        Self::ensure_column(conn, "symbols_raw", "symbol_role", "TEXT")?;
        Self::ensure_column(conn, "symbols_raw", "declaration_file_path", "TEXT")?;
        Self::ensure_column(conn, "symbols_raw", "declaration_line", "INTEGER")?;
        Self::ensure_column(conn, "symbols_raw", "declaration_end_line", "INTEGER")?;
        Self::ensure_column(conn, "symbols_raw", "definition_file_path", "TEXT")?;
        Self::ensure_column(conn, "symbols_raw", "definition_line", "INTEGER")?;
        Self::ensure_column(conn, "symbols_raw", "definition_end_line", "INTEGER")?;
        Self::ensure_column(conn, "symbols_raw", "module", "TEXT")?;
        Self::ensure_column(conn, "symbols_raw", "subsystem", "TEXT")?;
        Self::ensure_column(conn, "symbols_raw", "project_area", "TEXT")?;
        Self::ensure_column(conn, "symbols_raw", "artifact_kind", "TEXT")?;
        Self::ensure_column(conn, "symbols_raw", "header_role", "TEXT")?;
        Self::ensure_column(conn, "symbols_raw", "parse_fragility", "TEXT")?;
        Self::ensure_column(conn, "symbols_raw", "macro_sensitivity", "TEXT")?;
        Self::ensure_column(conn, "symbols_raw", "include_heaviness", "TEXT")?;

        Self::ensure_column(conn, "symbols", "parameter_count", "INTEGER")?;
        Self::ensure_column(conn, "symbols", "scope_qualified_name", "TEXT")?;
        Self::ensure_column(conn, "symbols", "scope_kind", "TEXT")?;
        Self::ensure_column(conn, "symbols", "symbol_role", "TEXT")?;
        Self::ensure_column(conn, "symbols", "declaration_file_path", "TEXT")?;
        Self::ensure_column(conn, "symbols", "declaration_line", "INTEGER")?;
        Self::ensure_column(conn, "symbols", "declaration_end_line", "INTEGER")?;
        Self::ensure_column(conn, "symbols", "definition_file_path", "TEXT")?;
        Self::ensure_column(conn, "symbols", "definition_line", "INTEGER")?;
        Self::ensure_column(conn, "symbols", "definition_end_line", "INTEGER")?;
        Self::ensure_column(conn, "symbols", "module", "TEXT")?;
        Self::ensure_column(conn, "symbols", "subsystem", "TEXT")?;
        Self::ensure_column(conn, "symbols", "project_area", "TEXT")?;
        Self::ensure_column(conn, "symbols", "artifact_kind", "TEXT")?;
        Self::ensure_column(conn, "symbols", "header_role", "TEXT")?;
        Self::ensure_column(conn, "symbols", "parse_fragility", "TEXT")?;
        Self::ensure_column(conn, "symbols", "macro_sensitivity", "TEXT")?;
        Self::ensure_column(conn, "symbols", "include_heaviness", "TEXT")?;

        Self::ensure_column(conn, "files", "module", "TEXT")?;
        Self::ensure_column(conn, "files", "subsystem", "TEXT")?;
        Self::ensure_column(conn, "files", "project_area", "TEXT")?;
        Self::ensure_column(conn, "files", "artifact_kind", "TEXT")?;
        Self::ensure_column(conn, "files", "header_role", "TEXT")?;
        Self::ensure_column(conn, "files", "parse_fragility", "TEXT")?;
        Self::ensure_column(conn, "files", "macro_sensitivity", "TEXT")?;
        Self::ensure_column(conn, "files", "include_heaviness", "TEXT")?;

        Ok(())
    }

    fn ensure_column(conn: &Connection, table: &str, column: &str, column_def: &str) -> SqlResult<()> {
        let pragma = format!("PRAGMA table_info({})", table);
        let mut stmt = conn.prepare(&pragma)?;
        let existing: Vec<String> = stmt
            .query_map([], |row| row.get(1))?
            .filter_map(|r| r.ok())
            .collect();

        if !existing.iter().any(|c| c == column) {
            let alter = format!("ALTER TABLE {} ADD COLUMN {} {}", table, column, column_def);
            conn.execute(&alter, [])?;
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
            "DELETE FROM symbols_raw; DELETE FROM symbols; DELETE FROM calls; DELETE FROM raw_calls; DELETE FROM symbol_references; DELETE FROM propagation_events; DELETE FROM callable_flow_summaries; DELETE FROM files; DELETE FROM symbols_fts;",
        )
    }

    pub fn rebuild_fts(&self) -> SqlResult<()> {
        self.conn.execute_batch(
            "DELETE FROM symbols_fts; INSERT INTO symbols_fts(id, name, qualified_name) SELECT id, name, qualified_name FROM symbols;",
        )
    }

    pub fn write_raw_symbols(&self, symbols: &[Symbol]) -> SqlResult<()> {
        let mut stmt = self.conn.prepare(
            "INSERT INTO symbols_raw (id, name, qualified_name, type, file_path, line, end_line, signature, parameter_count, scope_qualified_name, scope_kind, symbol_role, declaration_file_path, declaration_line, declaration_end_line, definition_file_path, definition_line, definition_end_line, parent_id, module, subsystem, project_area, artifact_kind, header_role, parse_fragility, macro_sensitivity, include_heaviness)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27)",
        )?;
        for s in symbols {
            let normalized = normalize_dual_locations(s);
            stmt.execute(params![
                normalized.id,
                normalized.name,
                normalized.qualified_name,
                normalized.symbol_type,
                normalized.file_path,
                normalized.line,
                normalized.end_line,
                normalized.signature,
                normalized.parameter_count,
                normalized.scope_qualified_name,
                normalized.scope_kind,
                normalized.symbol_role,
                normalized.declaration_file_path,
                normalized.declaration_line,
                normalized.declaration_end_line,
                normalized.definition_file_path,
                normalized.definition_line,
                normalized.definition_end_line,
                normalized.parent_id,
                normalized.module,
                normalized.subsystem,
                normalized.project_area,
                normalized.artifact_kind,
                normalized.header_role,
                normalized.parse_fragility,
                normalized.macro_sensitivity,
                normalized.include_heaviness,
            ])?;
        }
        Ok(())
    }

    pub fn write_symbols(&self, symbols: &[Symbol]) -> SqlResult<()> {
        let mut stmt = self.conn.prepare(
            "INSERT OR REPLACE INTO symbols (id, name, qualified_name, type, file_path, line, end_line, signature, parameter_count, scope_qualified_name, scope_kind, symbol_role, declaration_file_path, declaration_line, declaration_end_line, definition_file_path, definition_line, definition_end_line, parent_id, module, subsystem, project_area, artifact_kind, header_role, parse_fragility, macro_sensitivity, include_heaviness)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27)",
        )?;
        for s in symbols {
            let normalized = normalize_dual_locations(s);
            stmt.execute(params![
                normalized.id,
                normalized.name,
                normalized.qualified_name,
                normalized.symbol_type,
                normalized.file_path,
                normalized.line,
                normalized.end_line,
                normalized.signature,
                normalized.parameter_count,
                normalized.scope_qualified_name,
                normalized.scope_kind,
                normalized.symbol_role,
                normalized.declaration_file_path,
                normalized.declaration_line,
                normalized.declaration_end_line,
                normalized.definition_file_path,
                normalized.definition_line,
                normalized.definition_end_line,
                normalized.parent_id,
                normalized.module,
                normalized.subsystem,
                normalized.project_area,
                normalized.artifact_kind,
                normalized.header_role,
                normalized.parse_fragility,
                normalized.macro_sensitivity,
                normalized.include_heaviness,
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

    pub fn write_raw_calls(&self, raw_calls: &[RawCallSite]) -> SqlResult<()> {
        let mut stmt = self.conn.prepare(
            "INSERT INTO raw_calls (
                caller_id, called_name, call_kind, argument_count, argument_texts_json,
                result_target_json, receiver, receiver_kind, qualifier, qualifier_kind,
                file_path, line
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        )?;
        for raw_call in raw_calls {
            stmt.execute(params![
                raw_call.caller_id,
                raw_call.called_name,
                raw_call_kind_key(&raw_call.call_kind),
                raw_call.argument_count,
                serde_json::to_string(&raw_call.argument_texts).unwrap_or_else(|_| "[]".to_string()),
                raw_call
                    .result_target
                    .as_ref()
                    .map(|anchor| serde_json::to_string(anchor).unwrap_or_else(|_| "null".to_string())),
                raw_call.receiver,
                raw_call.receiver_kind.as_ref().map(raw_receiver_kind_key),
                raw_call.qualifier,
                raw_call.qualifier_kind.as_ref().map(raw_qualifier_kind_key),
                raw_call.file_path,
                raw_call.line,
            ])?;
        }
        Ok(())
    }

    pub fn write_references(&self, references: &[NormalizedReference]) -> SqlResult<()> {
        let mut stmt = self.conn.prepare(
            "INSERT INTO symbol_references (source_symbol_id, target_symbol_id, category, file_path, line, confidence) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )?;
        for reference in references {
            stmt.execute(params![
                reference.source_symbol_id,
                reference.target_symbol_id,
                reference_category_key(&reference.category),
                reference.file_path,
                reference.line,
                extraction_confidence_key(&reference.confidence),
            ])?;
        }
        Ok(())
    }

    pub fn replace_references(&self, references: &[NormalizedReference]) -> SqlResult<()> {
        self.conn.execute("DELETE FROM symbol_references", [])?;
        self.write_references(references)
    }

    pub fn write_propagation_events(&self, events: &[PropagationEvent]) -> SqlResult<()> {
        let mut stmt = self.conn.prepare(
            "INSERT INTO propagation_events (
                owner_symbol_id, source_anchor_id, source_symbol_id, source_expression_text, source_anchor_kind,
                target_anchor_id, target_symbol_id, target_expression_text, target_anchor_kind,
                propagation_kind, file_path, line, confidence, risks
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        )?;
        for event in events {
            stmt.execute(params![
                event.owner_symbol_id,
                event.source_anchor.anchor_id,
                event.source_anchor.symbol_id,
                event.source_anchor.expression_text,
                propagation_anchor_kind_key(&event.source_anchor.anchor_kind),
                event.target_anchor.anchor_id,
                event.target_anchor.symbol_id,
                event.target_anchor.expression_text,
                propagation_anchor_kind_key(&event.target_anchor.anchor_kind),
                propagation_kind_key(&event.propagation_kind),
                event.file_path,
                event.line,
                extraction_confidence_key(&event.confidence),
                serde_json::to_string(&event.risks).unwrap_or_else(|_| "[]".to_string()),
            ])?;
        }
        Ok(())
    }

    pub fn write_callable_flow_summaries(
        &self,
        summaries: &[CallableFlowSummary],
        symbols: &[Symbol],
    ) -> SqlResult<()> {
        if summaries.is_empty() {
            return Ok(());
        }

        let symbol_paths: HashMap<&str, &str> = symbols
            .iter()
            .map(|symbol| (symbol.id.as_str(), symbol.file_path.as_str()))
            .collect();

        let mut stmt = self.conn.prepare(
            "INSERT OR REPLACE INTO callable_flow_summaries (
                callable_symbol_id, file_path, parameter_anchors_json, return_anchors_json
            ) VALUES (?1, ?2, ?3, ?4)",
        )?;
        for summary in summaries {
            let Some(file_path) = symbol_paths.get(summary.callable_symbol_id.as_str()) else {
                continue;
            };
            stmt.execute(params![
                summary.callable_symbol_id,
                *file_path,
                serde_json::to_string(&summary.parameter_anchors).unwrap_or_else(|_| "[]".to_string()),
                serde_json::to_string(&summary.return_anchors).unwrap_or_else(|_| "[]".to_string()),
            ])?;
        }
        Ok(())
    }

    pub fn write_files(&self, files: &[FileRecord]) -> SqlResult<()> {
        let mut stmt = self.conn.prepare(
            "INSERT OR REPLACE INTO files (path, content_hash, last_indexed, symbol_count, module, subsystem, project_area, artifact_kind, header_role, parse_fragility, macro_sensitivity, include_heaviness) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        )?;
        for f in files {
            stmt.execute(params![
                f.path,
                f.content_hash,
                f.last_indexed,
                f.symbol_count,
                f.module,
                f.subsystem,
                f.project_area,
                f.artifact_kind,
                f.header_role,
                f.parse_fragility,
                f.macro_sensitivity,
                f.include_heaviness,
            ])?;
        }
        Ok(())
    }

    pub fn write_all(
        &self,
        raw_symbols: &[Symbol],
        representative_symbols: &[Symbol],
        calls: &[Call],
        references: &[NormalizedReference],
        propagation_events: &[PropagationEvent],
        callable_flow_summaries: &[CallableFlowSummary],
        files: &[FileRecord],
    ) -> SqlResult<()> {
        self.conn.execute_batch("BEGIN TRANSACTION;")?;
        let tx_result: SqlResult<()> = (|| {
            self.clear()?;
            self.write_raw_symbols(raw_symbols)?;
            self.write_symbols(representative_symbols)?;
            self.write_calls(calls)?;
            self.write_references(references)?;
            self.write_propagation_events(propagation_events)?;
            self.write_callable_flow_summaries(callable_flow_summaries, representative_symbols)?;
            self.write_files(files)?;
            self.rebuild_fts()?;
            Ok(())
        })();

        if let Err(err) = tx_result {
            let _ = self.conn.execute_batch("ROLLBACK;");
            return Err(err);
        }

        self.conn.execute_batch("COMMIT;")?;
        Ok(())
    }

    pub fn checkpoint(&self) -> SqlResult<()> {
        self.conn.execute_batch("PRAGMA optimize;")
    }

    pub fn write_index_metadata(&self, metadata: &IndexMetadata) -> SqlResult<()> {
        let mut stmt = self.conn.prepare(
            "INSERT OR REPLACE INTO db_metadata(key, value) VALUES (?1, ?2)",
        )?;
        for (key, value) in [
            ("format_version", metadata.format_version.to_string()),
            ("indexer_version", metadata.indexer_version.clone()),
            ("workspace_root", metadata.workspace_root.clone()),
            ("extensions_csv", metadata.extensions_csv.clone()),
        ] {
            stmt.execute(params![key, value])?;
        }
        Ok(())
    }

    pub fn read_index_metadata(&self) -> SqlResult<Option<IndexMetadata>> {
        let mut stmt = self.conn.prepare("SELECT key, value FROM db_metadata")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;

        let mut values = HashMap::new();
        for row in rows {
            let (key, value) = row?;
            values.insert(key, value);
        }

        if values.is_empty() {
            return Ok(None);
        }

        let format_version = values
            .get("format_version")
            .and_then(|value| value.parse::<u32>().ok());
        let indexer_version = values.get("indexer_version").cloned();
        let workspace_root = values.get("workspace_root").cloned();
        let extensions_csv = values.get("extensions_csv").cloned();

        match (
            format_version,
            indexer_version,
            workspace_root,
            extensions_csv,
        ) {
            (
                Some(format_version),
                Some(indexer_version),
                Some(workspace_root),
                Some(extensions_csv),
            ) => Ok(Some(IndexMetadata {
                format_version,
                indexer_version,
                workspace_root,
                extensions_csv,
            })),
            _ => Ok(None),
        }
    }

    pub fn quick_check(&self) -> SqlResult<Vec<String>> {
        let mut stmt = self.conn.prepare("PRAGMA quick_check(1)")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        let issues: Vec<String> = rows
            .filter_map(|row| row.ok())
            .filter(|value| value != "ok")
            .collect();
        Ok(issues)
    }

    pub fn read_file_records(&self) -> SqlResult<Vec<FileRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT path, content_hash, last_indexed, symbol_count, module, subsystem, project_area, artifact_kind, header_role, parse_fragility, macro_sensitivity, include_heaviness FROM files",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(FileRecord {
                path: row.get(0)?,
                content_hash: row.get(1)?,
                last_indexed: row.get(2)?,
                symbol_count: row.get(3)?,
                module: row.get(4)?,
                subsystem: row.get(5)?,
                project_area: row.get(6)?,
                artifact_kind: row.get(7)?,
                header_role: row.get(8)?,
                parse_fragility: row.get(9)?,
                macro_sensitivity: row.get(10)?,
                include_heaviness: row.get(11)?,
            })
        })?;
        rows.collect()
    }

    pub fn read_all_raw_symbols(&self) -> SqlResult<Vec<Symbol>> {
        let sql = format!("SELECT {} FROM symbols_raw", SYMBOL_SELECT_COLUMNS);
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map([], row_to_symbol)?;
        rows.collect()
    }

    pub fn read_raw_calls_for_paths(&self, file_paths: &[String]) -> SqlResult<Vec<RawCallSite>> {
        if file_paths.is_empty() {
            return Ok(Vec::new());
        }

        let placeholders = vec!["?"; file_paths.len()].join(", ");
        let sql = format!(
            "SELECT caller_id, called_name, call_kind, argument_count, argument_texts_json, result_target_json, receiver, receiver_kind, qualifier, qualifier_kind, file_path, line
             FROM raw_calls
             WHERE file_path IN ({})
             ORDER BY file_path, line, caller_id, called_name",
            placeholders,
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(file_paths.iter()), |row| {
            let argument_texts_json: String = row.get(4)?;
            let result_target_json: Option<String> = row.get(5)?;
            Ok(RawCallSite {
                caller_id: row.get(0)?,
                called_name: row.get(1)?,
                call_kind: raw_call_kind_from_key(&row.get::<_, String>(2)?),
                argument_count: row.get(3)?,
                argument_texts: serde_json::from_str(&argument_texts_json).unwrap_or_default(),
                result_target: result_target_json
                    .as_deref()
                    .and_then(|json| serde_json::from_str(json).ok()),
                receiver: row.get(6)?,
                receiver_kind: row
                    .get::<_, Option<String>>(7)?
                    .as_deref()
                    .map(raw_receiver_kind_from_key),
                qualifier: row.get(8)?,
                qualifier_kind: row
                    .get::<_, Option<String>>(9)?
                    .as_deref()
                    .map(raw_qualifier_kind_from_key),
                file_path: row.get(10)?,
                line: row.get(11)?,
            })
        })?;
        rows.collect()
    }

    pub fn read_calls_for_paths(&self, file_paths: &[String]) -> SqlResult<Vec<Call>> {
        if file_paths.is_empty() {
            return Ok(Vec::new());
        }

        let placeholders = vec!["?"; file_paths.len()].join(", ");
        let sql = format!(
            "SELECT caller_id, callee_id, file_path, line
             FROM calls
             WHERE file_path IN ({})
             ORDER BY file_path, line, caller_id, callee_id",
            placeholders,
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(file_paths.iter()), |row| {
            Ok(Call {
                caller_id: row.get(0)?,
                callee_id: row.get(1)?,
                file_path: row.get(2)?,
                line: row.get(3)?,
            })
        })?;
        rows.collect()
    }

    pub fn read_all_calls(&self) -> SqlResult<Vec<Call>> {
        let mut stmt = self.conn.prepare(
            "SELECT caller_id, callee_id, file_path, line
             FROM calls
             ORDER BY file_path, line, caller_id, callee_id",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(Call {
                caller_id: row.get(0)?,
                callee_id: row.get(1)?,
                file_path: row.get(2)?,
                line: row.get(3)?,
            })
        })?;
        rows.collect()
    }

    pub fn read_all_references(&self) -> SqlResult<Vec<NormalizedReference>> {
        let mut stmt = self.conn.prepare(
            "SELECT source_symbol_id, target_symbol_id, category, file_path, line, confidence
             FROM symbol_references
             ORDER BY file_path, line, source_symbol_id, target_symbol_id",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(NormalizedReference {
                source_symbol_id: row.get(0)?,
                target_symbol_id: row.get(1)?,
                category: reference_category_from_key(&row.get::<_, String>(2)?),
                file_path: row.get(3)?,
                line: row.get(4)?,
                confidence: extraction_confidence_from_key(&row.get::<_, String>(5)?),
            })
        })?;
        rows.collect()
    }

    pub fn read_all_symbol_ids(&self) -> SqlResult<Vec<String>> {
        let mut stmt = self.conn.prepare("SELECT id FROM symbols ORDER BY id")?;
        let rows = stmt.query_map([], |row| row.get(0))?;
        rows.collect()
    }

    pub fn read_all_symbol_types(&self) -> SqlResult<HashMap<String, String>> {
        let mut stmt = self.conn.prepare("SELECT id, type FROM symbols ORDER BY id")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut result = HashMap::new();
        for row in rows {
            let (id, symbol_type) = row?;
            result.insert(id, symbol_type);
        }
        Ok(result)
    }

    pub fn read_all_propagation_events(&self) -> SqlResult<Vec<PropagationEvent>> {
        let mut stmt = self.conn.prepare(
            "SELECT
                owner_symbol_id, source_anchor_id, source_symbol_id, source_expression_text, source_anchor_kind,
                target_anchor_id, target_symbol_id, target_expression_text, target_anchor_kind,
                propagation_kind, file_path, line, confidence, risks
             FROM propagation_events
             ORDER BY file_path, line, propagation_kind, source_anchor_id, target_anchor_id",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(PropagationEvent {
                owner_symbol_id: row.get(0)?,
                source_anchor: crate::models::PropagationAnchor {
                    anchor_id: row.get(1)?,
                    symbol_id: row.get(2)?,
                    expression_text: row.get(3)?,
                    anchor_kind: propagation_anchor_kind_from_key(&row.get::<_, String>(4)?),
                },
                target_anchor: crate::models::PropagationAnchor {
                    anchor_id: row.get(5)?,
                    symbol_id: row.get(6)?,
                    expression_text: row.get(7)?,
                    anchor_kind: propagation_anchor_kind_from_key(&row.get::<_, String>(8)?),
                },
                propagation_kind: propagation_kind_from_key(&row.get::<_, String>(9)?),
                file_path: row.get(10)?,
                line: row.get(11)?,
                confidence: extraction_confidence_from_key(&row.get::<_, String>(12)?),
                risks: serde_json::from_str(&row.get::<_, String>(13)?).unwrap_or_default(),
            })
        })?;
        rows.collect()
    }

    pub fn read_all_propagation_event_keys(&self) -> SqlResult<HashSet<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT
                owner_symbol_id, source_anchor_id, source_expression_text,
                target_anchor_id, target_expression_text,
                propagation_kind, file_path, line
             FROM propagation_events",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(propagation_event_key_from_parts(
                row.get::<_, Option<String>>(0)?.as_deref(),
                row.get::<_, Option<String>>(1)?.as_deref(),
                row.get::<_, Option<String>>(2)?.as_deref(),
                row.get::<_, Option<String>>(3)?.as_deref(),
                row.get::<_, Option<String>>(4)?.as_deref(),
                &row.get::<_, String>(5)?,
                &row.get::<_, String>(6)?,
                row.get::<_, usize>(7)?,
            ))
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

    pub fn cleanup_dangling_references(&self) -> SqlResult<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT file_path FROM symbol_references
             WHERE source_symbol_id NOT IN (SELECT id FROM symbols)
                OR target_symbol_id NOT IN (SELECT id FROM symbols)
                OR (
                    category = 'inheritanceMention'
                    AND target_symbol_id IN (SELECT id FROM symbols)
                    AND target_symbol_id NOT IN (
                        SELECT id FROM symbols WHERE type IN ('class', 'struct')
                    )
                )",
        )?;
        let affected: Vec<String> = stmt.query_map([], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();

        self.conn.execute(
            "DELETE FROM symbol_references
             WHERE source_symbol_id NOT IN (SELECT id FROM symbols)
                OR target_symbol_id NOT IN (SELECT id FROM symbols)
                OR (
                    category = 'inheritanceMention'
                    AND target_symbol_id IN (SELECT id FROM symbols)
                    AND target_symbol_id NOT IN (
                        SELECT id FROM symbols WHERE type IN ('class', 'struct')
                    )
                )",
            [],
        )?;

        Ok(affected)
    }

    pub fn cleanup_dangling_propagation(&self) -> SqlResult<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT file_path FROM propagation_events
             WHERE (owner_symbol_id IS NOT NULL AND owner_symbol_id NOT IN (SELECT id FROM symbols))
                OR (source_symbol_id IS NOT NULL AND source_symbol_id NOT IN (SELECT id FROM symbols))
                OR (target_symbol_id IS NOT NULL AND target_symbol_id NOT IN (SELECT id FROM symbols))",
        )?;
        let affected: Vec<String> = stmt.query_map([], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();

        self.conn.execute(
            "DELETE FROM propagation_events
             WHERE (owner_symbol_id IS NOT NULL AND owner_symbol_id NOT IN (SELECT id FROM symbols))
                OR (source_symbol_id IS NOT NULL AND source_symbol_id NOT IN (SELECT id FROM symbols))
                OR (target_symbol_id IS NOT NULL AND target_symbol_id NOT IN (SELECT id FROM symbols))",
            [],
        )?;

        Ok(affected)
    }

    pub fn delete_calls_for_file(&self, file_path: &str) -> SqlResult<()> {
        self.conn.execute("DELETE FROM calls WHERE file_path = ?1", params![file_path])?;
        Ok(())
    }

    pub fn delete_references_for_file(&self, file_path: &str) -> SqlResult<()> {
        self.conn.execute("DELETE FROM symbol_references WHERE file_path = ?1", params![file_path])?;
        Ok(())
    }

    pub fn delete_propagation_for_file(&self, file_path: &str) -> SqlResult<()> {
        self.conn.execute("DELETE FROM propagation_events WHERE file_path = ?1", params![file_path])?;
        Ok(())
    }

    pub fn clear_raw_calls(&self) -> SqlResult<()> {
        self.conn.execute("DELETE FROM raw_calls", [])?;
        Ok(())
    }

    pub fn delete_callable_flow_summaries_for_file(&self, file_path: &str) -> SqlResult<()> {
        self.conn.execute(
            "DELETE FROM callable_flow_summaries WHERE file_path = ?1",
            params![file_path],
        )?;
        Ok(())
    }

    pub fn read_callable_flow_summaries_for_ids(
        &self,
        callable_symbol_ids: &[String],
    ) -> SqlResult<Vec<CallableFlowSummary>> {
        const SQLITE_VARIABLE_LIMIT: usize = 900;

        if callable_symbol_ids.is_empty() {
            return Ok(Vec::new());
        }

        let mut summaries = Vec::new();
        for chunk in callable_symbol_ids.chunks(SQLITE_VARIABLE_LIMIT) {
            let placeholders = vec!["?"; chunk.len()].join(", ");
            let sql = format!(
                "SELECT callable_symbol_id, parameter_anchors_json, return_anchors_json
                 FROM callable_flow_summaries
                 WHERE callable_symbol_id IN ({})",
                placeholders,
            );
            let mut stmt = self.conn.prepare(&sql)?;
            let rows = stmt.query_map(params_from_iter(chunk.iter()), |row| {
                let parameter_json: String = row.get(1)?;
                let return_json: String = row.get(2)?;
                Ok(CallableFlowSummary {
                    callable_symbol_id: row.get(0)?,
                    parameter_anchors: serde_json::from_str(&parameter_json).unwrap_or_default(),
                    return_anchors: serde_json::from_str(&return_json).unwrap_or_default(),
                })
            })?;
            summaries.extend(rows.collect::<SqlResult<Vec<_>>>()?);
        }

        Ok(summaries)
    }

    pub fn read_symbols_for_paths(&self, file_paths: &[String]) -> SqlResult<Vec<Symbol>> {
        if file_paths.is_empty() {
            return Ok(Vec::new());
        }

        let placeholders = vec!["?"; file_paths.len()].join(", ");
        let sql = format!(
            "SELECT {}
             FROM symbols_raw WHERE file_path IN ({})",
            SYMBOL_SELECT_COLUMNS, placeholders,
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(file_paths.iter()), row_to_symbol)?;
        rows.collect()
    }

    fn read_raw_symbols_by_ids(&self, symbol_ids: &[String]) -> SqlResult<Vec<Symbol>> {
        if symbol_ids.is_empty() {
            return Ok(Vec::new());
        }

        let placeholders = vec!["?"; symbol_ids.len()].join(", ");
        let sql = format!(
            "SELECT {}
             FROM symbols_raw WHERE id IN ({})",
            SYMBOL_SELECT_COLUMNS, placeholders,
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(symbol_ids.iter()), row_to_symbol)?;
        rows.collect()
    }

    pub fn find_symbols_by_name(&self, name: &str) -> SqlResult<Vec<Symbol>> {
        let sql = format!(
            "SELECT {} FROM symbols WHERE name = ?1 AND (type = 'function' OR type = 'method')",
            SYMBOL_SELECT_COLUMNS
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params![name], row_to_symbol)?;
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
            "SELECT {}
             FROM symbols WHERE id IN ({})",
            SYMBOL_SELECT_COLUMNS, placeholders,
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(symbol_ids.iter()), row_to_symbol)?;
        rows.collect()
    }

    #[cfg(test)]
    pub fn get_direct_base_edges(&self, derived_symbol_id: &str) -> SqlResult<Vec<InheritanceEdge>> {
        let mut stmt = self.conn.prepare(
            "SELECT source_symbol_id, target_symbol_id, file_path, line, confidence
             FROM symbol_references
             WHERE category = 'inheritanceMention' AND source_symbol_id = ?1
             ORDER BY line ASC, target_symbol_id ASC",
        )?;
        let rows = stmt.query_map(params![derived_symbol_id], |row| {
            Ok(InheritanceEdge {
                derived_symbol_id: row.get(0)?,
                base_symbol_id: row.get(1)?,
                file_path: row.get(2)?,
                line: row.get(3)?,
                confidence: extraction_confidence_from_key(&row.get::<_, String>(4)?),
            })
        })?;
        rows.collect()
    }

    #[cfg(test)]
    pub fn get_direct_derived_edges(&self, base_symbol_id: &str) -> SqlResult<Vec<InheritanceEdge>> {
        let mut stmt = self.conn.prepare(
            "SELECT source_symbol_id, target_symbol_id, file_path, line, confidence
             FROM symbol_references
             WHERE category = 'inheritanceMention' AND target_symbol_id = ?1
             ORDER BY line ASC, source_symbol_id ASC",
        )?;
        let rows = stmt.query_map(params![base_symbol_id], |row| {
            Ok(InheritanceEdge {
                derived_symbol_id: row.get(0)?,
                base_symbol_id: row.get(1)?,
                file_path: row.get(2)?,
                line: row.get(3)?,
                confidence: extraction_confidence_from_key(&row.get::<_, String>(4)?),
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

    pub fn count_references(&self) -> SqlResult<i64> {
        self.conn.query_row("SELECT COUNT(*) FROM symbol_references", [], |r| r.get(0))
    }

    pub fn count_propagation_events(&self) -> SqlResult<i64> {
        self.conn.query_row("SELECT COUNT(*) FROM propagation_events", [], |r| r.get(0))
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

pub fn validate_existing_database(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }

    let db = Database::open(path).map_err(|e| format!("open failed: {}", e))?;
    let issues = db
        .quick_check()
        .map_err(|e| format!("quick_check failed: {}", e))?;

    if issues.is_empty() {
        Ok(())
    } else {
        Err(format!("integrity check failed: {}", issues.join("; ")))
    }
}

pub fn expected_index_metadata(workspace_root: &Path) -> IndexMetadata {
    let mut extensions: Vec<String> = crate::constants::configured_extensions().into_iter().collect();
    extensions.sort();
    IndexMetadata {
        format_version: INDEX_FORMAT_VERSION,
        indexer_version: env!("CARGO_PKG_VERSION").to_string(),
        workspace_root: normalized_workspace_root(workspace_root),
        extensions_csv: extensions.join(","),
    }
}

pub fn existing_database_metadata_issue(
    path: &Path,
    expected: &IndexMetadata,
) -> Result<Option<String>, String> {
    if !path.exists() {
        return Ok(None);
    }

    let db = Database::open(path).map_err(|e| format!("open failed: {}", e))?;
    let actual = db
        .read_index_metadata()
        .map_err(|e| format!("read metadata failed: {}", e))?;
    match actual {
        Some(actual) => Ok(actual.mismatch_reason(expected)),
        None => Ok(Some("index metadata missing".to_string())),
    }
}

fn normalized_workspace_root(workspace_root: &Path) -> String {
    workspace_root
        .canonicalize()
        .unwrap_or_else(|_| workspace_root.to_path_buf())
        .to_string_lossy()
        .replace('\\', "/")
}

fn row_to_symbol(row: &rusqlite::Row<'_>) -> SqlResult<Symbol> {
    Ok(Symbol {
        id: row.get(0)?,
        name: row.get(1)?,
        qualified_name: row.get(2)?,
        symbol_type: row.get(3)?,
        file_path: row.get(4)?,
        line: row.get(5)?,
        end_line: row.get(6)?,
        signature: row.get(7)?,
        parameter_count: row.get(8)?,
        scope_qualified_name: row.get(9)?,
        scope_kind: row.get(10)?,
        symbol_role: row.get(11)?,
        declaration_file_path: row.get(12)?,
        declaration_line: row.get(13)?,
        declaration_end_line: row.get(14)?,
        definition_file_path: row.get(15)?,
        definition_line: row.get(16)?,
        definition_end_line: row.get(17)?,
        parent_id: row.get(18)?,
        module: row.get(19)?,
        subsystem: row.get(20)?,
        project_area: row.get(21)?,
        artifact_kind: row.get(22)?,
        header_role: row.get(23)?,
        parse_fragility: row.get(24)?,
        macro_sensitivity: row.get(25)?,
        include_heaviness: row.get(26)?,
    })
}

fn reference_category_key(category: &ReferenceCategory) -> &'static str {
    match category {
        ReferenceCategory::FunctionCall => "functionCall",
        ReferenceCategory::MethodCall => "methodCall",
        ReferenceCategory::ClassInstantiation => "classInstantiation",
        ReferenceCategory::ModuleImport => "moduleImport",
        ReferenceCategory::TypeUsage => "typeUsage",
        ReferenceCategory::InheritanceMention => "inheritanceMention",
        ReferenceCategory::EnumValueUsage => "enumValueUsage",
    }
}

fn reference_category_from_key(value: &str) -> ReferenceCategory {
    match value {
        "methodCall" => ReferenceCategory::MethodCall,
        "classInstantiation" => ReferenceCategory::ClassInstantiation,
        "moduleImport" => ReferenceCategory::ModuleImport,
        "typeUsage" => ReferenceCategory::TypeUsage,
        "inheritanceMention" => ReferenceCategory::InheritanceMention,
        "enumValueUsage" => ReferenceCategory::EnumValueUsage,
        _ => ReferenceCategory::FunctionCall,
    }
}

fn extraction_confidence_key(confidence: &RawExtractionConfidence) -> &'static str {
    match confidence {
        RawExtractionConfidence::High => "high",
        RawExtractionConfidence::Partial => "partial",
    }
}

fn extraction_confidence_from_key(value: &str) -> RawExtractionConfidence {
    match value {
        "partial" => RawExtractionConfidence::Partial,
        _ => RawExtractionConfidence::High,
    }
}

fn propagation_kind_key(kind: &PropagationKind) -> &'static str {
    match kind {
        PropagationKind::Assignment => "assignment",
        PropagationKind::InitializerBinding => "initializerBinding",
        PropagationKind::ArgumentToParameter => "argumentToParameter",
        PropagationKind::ReturnValue => "returnValue",
        PropagationKind::FieldWrite => "fieldWrite",
        PropagationKind::FieldRead => "fieldRead",
    }
}

fn propagation_kind_from_key(value: &str) -> PropagationKind {
    match value {
        "initializerBinding" => PropagationKind::InitializerBinding,
        "argumentToParameter" => PropagationKind::ArgumentToParameter,
        "returnValue" => PropagationKind::ReturnValue,
        "fieldWrite" => PropagationKind::FieldWrite,
        "fieldRead" => PropagationKind::FieldRead,
        _ => PropagationKind::Assignment,
    }
}

fn propagation_anchor_kind_key(kind: &PropagationAnchorKind) -> &'static str {
    match kind {
        PropagationAnchorKind::LocalVariable => "localVariable",
        PropagationAnchorKind::Parameter => "parameter",
        PropagationAnchorKind::ReturnValue => "returnValue",
        PropagationAnchorKind::Field => "field",
        PropagationAnchorKind::Expression => "expression",
    }
}

fn propagation_anchor_kind_from_key(value: &str) -> PropagationAnchorKind {
    match value {
        "parameter" => PropagationAnchorKind::Parameter,
        "returnValue" => PropagationAnchorKind::ReturnValue,
        "field" => PropagationAnchorKind::Field,
        "expression" => PropagationAnchorKind::Expression,
        _ => PropagationAnchorKind::LocalVariable,
    }
}

pub fn propagation_event_storage_key(event: &PropagationEvent) -> String {
    propagation_event_key_from_parts(
        event.owner_symbol_id.as_deref(),
        event.source_anchor.anchor_id.as_deref(),
        event.source_anchor.expression_text.as_deref(),
        event.target_anchor.anchor_id.as_deref(),
        event.target_anchor.expression_text.as_deref(),
        propagation_kind_key(&event.propagation_kind),
        event.file_path.as_str(),
        event.line,
    )
}

pub fn filter_persistable_references(
    references: &mut Vec<NormalizedReference>,
    valid_symbol_ids: &HashSet<String>,
    symbol_types: &HashMap<String, String>,
) -> usize {
    let before = references.len();
    references.retain(|reference| {
        if !valid_symbol_ids.contains(&reference.source_symbol_id)
            || !valid_symbol_ids.contains(&reference.target_symbol_id)
        {
            return false;
        }
        if reference.category == ReferenceCategory::InheritanceMention {
            return matches!(
                symbol_types.get(&reference.target_symbol_id).map(|value| value.as_str()),
                Some("class" | "struct")
            );
        }
        true
    });
    before.saturating_sub(references.len())
}

fn propagation_event_key_from_parts(
    owner_symbol_id: Option<&str>,
    source_anchor_id: Option<&str>,
    source_expression_text: Option<&str>,
    target_anchor_id: Option<&str>,
    target_expression_text: Option<&str>,
    propagation_kind: &str,
    file_path: &str,
    line: usize,
) -> String {
    format!(
        "{}|{}|{}|{}|{}|{}",
        owner_symbol_id.unwrap_or_default(),
        source_anchor_id.or(source_expression_text).unwrap_or_default(),
        target_anchor_id.or(target_expression_text).unwrap_or_default(),
        propagation_kind,
        file_path,
        line,
    )
}

fn raw_call_kind_key(kind: &RawCallKind) -> &'static str {
    match kind {
        RawCallKind::Unqualified => "unqualified",
        RawCallKind::MemberAccess => "memberAccess",
        RawCallKind::PointerMemberAccess => "pointerMemberAccess",
        RawCallKind::ThisPointerAccess => "thisPointerAccess",
        RawCallKind::Qualified => "qualified",
    }
}

fn raw_call_kind_from_key(value: &str) -> RawCallKind {
    match value {
        "memberAccess" => RawCallKind::MemberAccess,
        "pointerMemberAccess" => RawCallKind::PointerMemberAccess,
        "thisPointerAccess" => RawCallKind::ThisPointerAccess,
        "qualified" => RawCallKind::Qualified,
        _ => RawCallKind::Unqualified,
    }
}

fn raw_receiver_kind_key(kind: &RawReceiverKind) -> &'static str {
    match kind {
        RawReceiverKind::Identifier => "identifier",
        RawReceiverKind::This => "this",
        RawReceiverKind::PointerExpression => "pointerExpression",
        RawReceiverKind::FieldExpression => "fieldExpression",
        RawReceiverKind::QualifiedIdentifier => "qualifiedIdentifier",
        RawReceiverKind::Other => "other",
    }
}

fn raw_receiver_kind_from_key(value: &str) -> RawReceiverKind {
    match value {
        "this" => RawReceiverKind::This,
        "pointerExpression" => RawReceiverKind::PointerExpression,
        "fieldExpression" => RawReceiverKind::FieldExpression,
        "qualifiedIdentifier" => RawReceiverKind::QualifiedIdentifier,
        "other" => RawReceiverKind::Other,
        _ => RawReceiverKind::Identifier,
    }
}

fn raw_qualifier_kind_key(kind: &RawQualifierKind) -> &'static str {
    match kind {
        RawQualifierKind::Namespace => "namespace",
        RawQualifierKind::Type => "type",
    }
}

fn raw_qualifier_kind_from_key(value: &str) -> RawQualifierKind {
    match value {
        "type" => RawQualifierKind::Type,
        _ => RawQualifierKind::Namespace,
    }
}

fn normalize_dual_locations(symbol: &Symbol) -> Symbol {
    let mut normalized = symbol.clone();

    match normalized.symbol_role.as_deref() {
        Some("declaration") => {
            if normalized.declaration_file_path.is_none() {
                normalized.declaration_file_path = Some(normalized.file_path.clone());
            }
            if normalized.declaration_line.is_none() {
                normalized.declaration_line = Some(normalized.line);
            }
            if normalized.declaration_end_line.is_none() {
                normalized.declaration_end_line = Some(normalized.end_line);
            }
        }
        Some("definition") | Some("inline_definition") => {
            if normalized.definition_file_path.is_none() {
                normalized.definition_file_path = Some(normalized.file_path.clone());
            }
            if normalized.definition_line.is_none() {
                normalized.definition_line = Some(normalized.line);
            }
            if normalized.definition_end_line.is_none() {
                normalized.definition_end_line = Some(normalized.end_line);
            }
        }
        _ => {}
    }

    normalized
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::PropagationAnchor;

    fn make_sym(id: &str, name: &str) -> Symbol {
        Symbol {
            id: id.into(), name: name.into(), qualified_name: id.into(),
            symbol_type: "function".into(), file_path: "test.cpp".into(),
            line: 1, end_line: 5, signature: Some("void foo()".into()),
            parameter_count: None, scope_qualified_name: None, scope_kind: None, symbol_role: None,
            declaration_file_path: None, declaration_line: None, declaration_end_line: None,
            definition_file_path: None, definition_line: None, definition_end_line: None,
            parent_id: None,
            module: None, subsystem: None, project_area: None, artifact_kind: None, header_role: None,
            parse_fragility: None, macro_sensitivity: None, include_heaviness: None,
        }
    }

    fn make_propagation_event() -> PropagationEvent {
        PropagationEvent {
            owner_symbol_id: Some("Game::Worker::Tick".into()),
            source_anchor: PropagationAnchor {
                anchor_id: Some("Game::Worker::Tick::param:value@7".into()),
                symbol_id: None,
                expression_text: None,
                anchor_kind: PropagationAnchorKind::Parameter,
            },
            target_anchor: PropagationAnchor {
                anchor_id: Some("Game::Worker::Tick::field:stored".into()),
                symbol_id: None,
                expression_text: None,
                anchor_kind: PropagationAnchorKind::Field,
            },
            propagation_kind: PropagationKind::FieldWrite,
            file_path: "worker.cpp".into(),
            line: 8,
            confidence: RawExtractionConfidence::High,
            risks: Vec::new(),
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
            module: None, subsystem: None, project_area: None, artifact_kind: None, header_role: None,
            parse_fragility: None, macro_sensitivity: None, include_heaviness: None,
        }];

        db.write_all(&symbols, &symbols, &calls, &[], &[], &[], &files).unwrap();

        let count: i64 = db.conn.query_row("SELECT COUNT(*) FROM symbols", [], |r| r.get(0)).unwrap();
        assert_eq!(count, 2);
        let call_count: i64 = db.conn.query_row("SELECT COUNT(*) FROM calls", [], |r| r.get(0)).unwrap();
        assert_eq!(call_count, 1);
        let reference_count: i64 = db.conn.query_row("SELECT COUNT(*) FROM symbol_references", [], |r| r.get(0)).unwrap();
        assert_eq!(reference_count, 0);
        let propagation_count: i64 = db.conn.query_row("SELECT COUNT(*) FROM propagation_events", [], |r| r.get(0)).unwrap();
        assert_eq!(propagation_count, 0);
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
        assert_eq!(idx_count, 31);
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
            parameter_count: None,
            scope_qualified_name: None,
            scope_kind: None,
            symbol_role: None,
            declaration_file_path: None,
            declaration_line: None,
            declaration_end_line: None,
            definition_file_path: None,
            definition_line: None,
            definition_end_line: None,
            parent_id: Some("Foo".into()),
            module: None,
            subsystem: None,
            project_area: None,
            artifact_kind: None,
            header_role: None,
            parse_fragility: None,
            macro_sensitivity: None,
            include_heaviness: None,
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
            parameter_count: None,
            scope_qualified_name: None,
            scope_kind: None,
            symbol_role: None,
            declaration_file_path: None,
            declaration_line: None,
            declaration_end_line: None,
            definition_file_path: None,
            definition_line: None,
            definition_end_line: None,
            parent_id: Some("Foo".into()),
            module: None,
            subsystem: None,
            project_area: None,
            artifact_kind: None,
            header_role: None,
            parse_fragility: None,
            macro_sensitivity: None,
            include_heaviness: None,
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

    #[test]
    fn dual_location_metadata_is_normalized_and_round_tripped() {
        let db = Database::open(Path::new(":memory:")).unwrap();

        let declaration = Symbol {
            id: "Foo::Bar".into(),
            name: "Bar".into(),
            qualified_name: "Foo::Bar".into(),
            symbol_type: "method".into(),
            file_path: "foo.h".into(),
            line: 5,
            end_line: 5,
            signature: Some("void Bar()".into()),
            parameter_count: None,
            scope_qualified_name: None,
            scope_kind: None,
            symbol_role: Some("declaration".into()),
            declaration_file_path: None,
            declaration_line: None,
            declaration_end_line: None,
            definition_file_path: None,
            definition_line: None,
            definition_end_line: None,
            parent_id: Some("Foo".into()),
            module: None,
            subsystem: None,
            project_area: None,
            artifact_kind: None,
            header_role: None,
            parse_fragility: None,
            macro_sensitivity: None,
            include_heaviness: None,
        };

        let definition = Symbol {
            id: "Foo::Bar".into(),
            name: "Bar".into(),
            qualified_name: "Foo::Bar".into(),
            symbol_type: "method".into(),
            file_path: "foo.cpp".into(),
            line: 10,
            end_line: 15,
            signature: Some("void Foo::Bar()".into()),
            parameter_count: None,
            scope_qualified_name: None,
            scope_kind: None,
            symbol_role: Some("definition".into()),
            declaration_file_path: Some("foo.h".into()),
            declaration_line: Some(5),
            declaration_end_line: Some(5),
            definition_file_path: None,
            definition_line: None,
            definition_end_line: None,
            parent_id: Some("Foo".into()),
            module: None,
            subsystem: None,
            project_area: None,
            artifact_kind: None,
            header_role: None,
            parse_fragility: None,
            macro_sensitivity: None,
            include_heaviness: None,
        };

        db.write_raw_symbols(&[declaration]).unwrap();
        db.write_symbols(&[definition]).unwrap();

        let stored = db.find_symbols_by_ids(&["Foo::Bar".into()]).unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].declaration_file_path.as_deref(), Some("foo.h"));
        assert_eq!(stored[0].declaration_line, Some(5));
        assert_eq!(stored[0].definition_file_path.as_deref(), Some("foo.cpp"));
        assert_eq!(stored[0].definition_line, Some(10));
        assert_eq!(stored[0].definition_end_line, Some(15));
    }

    #[test]
    fn merged_symbol_refresh_keeps_call_edges_on_logical_id() {
        let db = Database::open(Path::new(":memory:")).unwrap();

        let caller = make_sym("Game::Tick", "Tick");
        let declaration = Symbol {
            id: "Foo::Bar".into(),
            name: "Bar".into(),
            qualified_name: "Foo::Bar".into(),
            symbol_type: "method".into(),
            file_path: "foo.h".into(),
            line: 5,
            end_line: 5,
            signature: Some("void Bar()".into()),
            parameter_count: None,
            scope_qualified_name: None,
            scope_kind: None,
            symbol_role: Some("declaration".into()),
            declaration_file_path: None,
            declaration_line: None,
            declaration_end_line: None,
            definition_file_path: None,
            definition_line: None,
            definition_end_line: None,
            parent_id: Some("Foo".into()),
            module: None,
            subsystem: None,
            project_area: None,
            artifact_kind: None,
            header_role: None,
            parse_fragility: None,
            macro_sensitivity: None,
            include_heaviness: None,
        };
        let definition = Symbol {
            id: "Foo::Bar".into(),
            name: "Bar".into(),
            qualified_name: "Foo::Bar".into(),
            symbol_type: "method".into(),
            file_path: "foo.cpp".into(),
            line: 10,
            end_line: 15,
            signature: Some("void Foo::Bar()".into()),
            parameter_count: None,
            scope_qualified_name: None,
            scope_kind: None,
            symbol_role: Some("definition".into()),
            declaration_file_path: None,
            declaration_line: None,
            declaration_end_line: None,
            definition_file_path: None,
            definition_line: None,
            definition_end_line: None,
            parent_id: Some("Foo".into()),
            module: None,
            subsystem: None,
            project_area: None,
            artifact_kind: None,
            header_role: None,
            parse_fragility: None,
            macro_sensitivity: None,
            include_heaviness: None,
        };

        db.write_raw_symbols(&[caller.clone(), declaration.clone(), definition]).unwrap();
        db.refresh_symbols_for_ids(&["Game::Tick".into(), "Foo::Bar".into()]).unwrap();
        db.write_calls(&[Call {
            caller_id: "Game::Tick".into(),
            callee_id: "Foo::Bar".into(),
            file_path: "game.cpp".into(),
            line: 42,
        }]).unwrap();

        db.delete_raw_symbols_for_file("foo.cpp").unwrap();
        db.refresh_symbols_for_ids(&["Foo::Bar".into()]).unwrap();

        let affected = db.cleanup_dangling_calls().unwrap();
        assert!(affected.is_empty());

        let representative = db.find_symbols_by_ids(&["Foo::Bar".into()]).unwrap();
        assert_eq!(representative.len(), 1);
        assert_eq!(representative[0].file_path, "foo.h");

        let call_count: i64 = db.conn.query_row(
            "SELECT COUNT(*) FROM calls WHERE callee_id = 'Foo::Bar'",
            [],
            |r| r.get(0),
        ).unwrap();
        assert_eq!(call_count, 1);
    }

    #[test]
    fn writes_references() {
        let db = Database::open(Path::new(":memory:")).unwrap();
        let references = vec![NormalizedReference {
            source_symbol_id: "Game::Controller".into(),
            target_symbol_id: "Game::Actor".into(),
            category: ReferenceCategory::TypeUsage,
            file_path: "controller.cpp".into(),
            line: 7,
            confidence: RawExtractionConfidence::Partial,
        }];

        db.write_references(&references).unwrap();

        let count: i64 = db.conn.query_row("SELECT COUNT(*) FROM symbol_references", [], |r| r.get(0)).unwrap();
        assert_eq!(count, 1);
        let category: String = db.conn.query_row("SELECT category FROM symbol_references LIMIT 1", [], |r| r.get(0)).unwrap();
        assert_eq!(category, "typeUsage");
    }

    #[test]
    fn writes_enum_value_usage_references() {
        let db = Database::open(Path::new(":memory:")).unwrap();
        let references = vec![NormalizedReference {
            source_symbol_id: "Game::Controller::Update".into(),
            target_symbol_id: "Game::AIState::Idle".into(),
            category: ReferenceCategory::EnumValueUsage,
            file_path: "controller.cpp".into(),
            line: 12,
            confidence: RawExtractionConfidence::Partial,
        }];

        db.write_references(&references).unwrap();

        let category: String = db.conn.query_row(
            "SELECT category FROM symbol_references LIMIT 1",
            [],
            |r| r.get(0),
        ).unwrap();
        assert_eq!(category, "enumValueUsage");
    }

    #[test]
    fn cleanup_dangling_references_removes_non_workspace_targets() {
        let db = Database::open(Path::new(":memory:")).unwrap();
        let symbols = vec![
            make_sym("Game::Controller", "Controller"),
            make_sym("Game::Actor", "Actor"),
        ];
        db.write_symbols(&symbols).unwrap();
        db.write_references(&[
            NormalizedReference {
                source_symbol_id: "Game::Controller".into(),
                target_symbol_id: "Game::Actor".into(),
                category: ReferenceCategory::TypeUsage,
                file_path: "controller.cpp".into(),
                line: 7,
                confidence: RawExtractionConfidence::High,
            },
            NormalizedReference {
                source_symbol_id: "Game::Controller".into(),
                target_symbol_id: "python::json".into(),
                category: ReferenceCategory::ModuleImport,
                file_path: "controller.py".into(),
                line: 3,
                confidence: RawExtractionConfidence::High,
            },
            NormalizedReference {
                source_symbol_id: "Game::Controller".into(),
                target_symbol_id: "Game::Action".into(),
                category: ReferenceCategory::InheritanceMention,
                file_path: "controller.cpp".into(),
                line: 9,
                confidence: RawExtractionConfidence::High,
            },
        ]).unwrap();
        db.write_symbols(&[Symbol {
            id: "Game::Action".into(),
            name: "Action".into(),
            qualified_name: "Game::Action".into(),
            symbol_type: "function".into(),
            file_path: "controller.cpp".into(),
            line: 8,
            end_line: 8,
            signature: None,
            parameter_count: None,
            scope_qualified_name: None,
            scope_kind: None,
            symbol_role: None,
            declaration_file_path: None,
            declaration_line: None,
            declaration_end_line: None,
            definition_file_path: None,
            definition_line: None,
            definition_end_line: None,
            parent_id: None,
            module: None,
            subsystem: None,
            project_area: None,
            artifact_kind: None,
            header_role: None,
            parse_fragility: None,
            macro_sensitivity: None,
            include_heaviness: None,
        }]).unwrap();

        let mut affected = db.cleanup_dangling_references().unwrap();
        affected.sort();
        assert_eq!(affected, vec!["controller.cpp".to_string(), "controller.py".to_string()]);

        let remaining = db.read_all_references().unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].target_symbol_id, "Game::Actor");
        assert_eq!(remaining[0].category, ReferenceCategory::TypeUsage);
    }

    #[test]
    fn filter_persistable_references_drops_unresolved_targets() {
        let mut references = vec![
            NormalizedReference {
                source_symbol_id: "Game::Controller".into(),
                target_symbol_id: "Game::Actor".into(),
                category: ReferenceCategory::TypeUsage,
                file_path: "controller.cpp".into(),
                line: 7,
                confidence: RawExtractionConfidence::High,
            },
            NormalizedReference {
                source_symbol_id: "python::scripts::main".into(),
                target_symbol_id: "python::tools::build".into(),
                category: ReferenceCategory::ModuleImport,
                file_path: "scripts/main.py".into(),
                line: 1,
                confidence: RawExtractionConfidence::High,
            },
            NormalizedReference {
                source_symbol_id: "python::scripts::main".into(),
                target_symbol_id: "python::json".into(),
                category: ReferenceCategory::ModuleImport,
                file_path: "scripts/main.py".into(),
                line: 2,
                confidence: RawExtractionConfidence::High,
            },
            NormalizedReference {
                source_symbol_id: "python::game::test::Runner".into(),
                target_symbol_id: "python::unittest::TestCase".into(),
                category: ReferenceCategory::InheritanceMention,
                file_path: "game/test_runner.py".into(),
                line: 4,
                confidence: RawExtractionConfidence::High,
            },
        ];
        let valid_symbol_ids = HashSet::from([
            "Game::Actor".to_string(),
            "python::tools::build".to_string(),
            "Game::Controller".to_string(),
            "python::scripts::main".to_string(),
        ]);
        let symbol_types = HashMap::from([
            ("Game::Actor".to_string(), "class".to_string()),
            ("Game::Controller".to_string(), "class".to_string()),
            ("python::tools::build".to_string(), "namespace".to_string()),
            ("python::scripts::main".to_string(), "namespace".to_string()),
        ]);

        let dropped = filter_persistable_references(&mut references, &valid_symbol_ids, &symbol_types);

        assert_eq!(dropped, 2);
        assert_eq!(references.len(), 2);
        assert!(references.iter().any(|reference| {
            reference.category == ReferenceCategory::TypeUsage
                && reference.target_symbol_id == "Game::Actor"
        }));
        assert!(references.iter().any(|reference| {
            reference.category == ReferenceCategory::ModuleImport
                && reference.target_symbol_id == "python::tools::build"
        }));
        assert!(!references.iter().any(|reference| {
            reference.category == ReferenceCategory::ModuleImport
                && reference.target_symbol_id == "python::json"
        }));
        assert!(!references.iter().any(|reference| {
            reference.category == ReferenceCategory::InheritanceMention
                && reference.target_symbol_id == "python::unittest::TestCase"
        }));
    }

    #[test]
    fn writes_propagation_events_and_callable_summaries() {
        let db = Database::open(Path::new(":memory:")).unwrap();
        let symbols = vec![make_sym("Game::Worker::Tick", "Tick")];
        let propagation = vec![make_propagation_event()];
        let summaries = vec![CallableFlowSummary {
            callable_symbol_id: "Game::Worker::Tick".into(),
            parameter_anchors: vec![PropagationAnchor {
                anchor_id: Some("Game::Worker::Tick::param:value@7".into()),
                symbol_id: None,
                expression_text: None,
                anchor_kind: PropagationAnchorKind::Parameter,
            }],
            return_anchors: vec![PropagationAnchor {
                anchor_id: Some("Game::Worker::Tick::return@9".into()),
                symbol_id: None,
                expression_text: None,
                anchor_kind: PropagationAnchorKind::ReturnValue,
            }],
        }];

        db.write_symbols(&symbols).unwrap();
        db.write_propagation_events(&propagation).unwrap();
        db.write_callable_flow_summaries(&summaries, &symbols).unwrap();

        let propagation_count: i64 = db.conn.query_row("SELECT COUNT(*) FROM propagation_events", [], |r| r.get(0)).unwrap();
        assert_eq!(propagation_count, 1);
        let summary_count: i64 = db.conn.query_row("SELECT COUNT(*) FROM callable_flow_summaries", [], |r| r.get(0)).unwrap();
        assert_eq!(summary_count, 1);

        let stored = db.read_callable_flow_summaries_for_ids(&["Game::Worker::Tick".into()]).unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].parameter_anchors.len(), 1);
        assert_eq!(stored[0].return_anchors.len(), 1);
    }

    #[test]
    fn persists_symbol_and_file_metadata_fields() {
        let db = Database::open(Path::new(":memory:")).unwrap();
        let symbol = Symbol {
            id: "Game::UI::Panel".into(),
            name: "Panel".into(),
            qualified_name: "Game::UI::Panel".into(),
            symbol_type: "class".into(),
            file_path: "src/ui/public/panel.h".into(),
            line: 3,
            end_line: 20,
            signature: None,
            parameter_count: None,
            scope_qualified_name: None,
            scope_kind: None,
            symbol_role: None,
            declaration_file_path: None,
            declaration_line: None,
            declaration_end_line: None,
            definition_file_path: None,
            definition_line: None,
            definition_end_line: None,
            parent_id: None,
            module: Some("ui".into()),
            subsystem: Some("runtime".into()),
            project_area: Some("ui".into()),
            artifact_kind: Some("runtime".into()),
            header_role: Some("public".into()),
            parse_fragility: Some("low".into()),
            macro_sensitivity: Some("low".into()),
            include_heaviness: Some("light".into()),
        };
        let file = FileRecord {
            path: "src/ui/public/panel.h".into(),
            content_hash: "abc".into(),
            last_indexed: "2026-01-01T00:00:00Z".into(),
            symbol_count: 1,
            module: Some("ui".into()),
            subsystem: Some("runtime".into()),
            project_area: Some("ui".into()),
            artifact_kind: Some("runtime".into()),
            header_role: Some("public".into()),
            parse_fragility: Some("low".into()),
            macro_sensitivity: Some("low".into()),
            include_heaviness: Some("light".into()),
        };

        db.write_symbols(&[symbol]).unwrap();
        db.write_files(&[file]).unwrap();

        let stored_symbols = db.find_symbols_by_ids(&["Game::UI::Panel".into()]).unwrap();
        assert_eq!(stored_symbols.len(), 1);
        assert_eq!(stored_symbols[0].module.as_deref(), Some("ui"));
        assert_eq!(stored_symbols[0].subsystem.as_deref(), Some("runtime"));
        assert_eq!(stored_symbols[0].project_area.as_deref(), Some("ui"));
        assert_eq!(stored_symbols[0].artifact_kind.as_deref(), Some("runtime"));
        assert_eq!(stored_symbols[0].header_role.as_deref(), Some("public"));

        let stored_files = db.read_file_records().unwrap();
        assert_eq!(stored_files.len(), 1);
        assert_eq!(stored_files[0].module.as_deref(), Some("ui"));
        assert_eq!(stored_files[0].subsystem.as_deref(), Some("runtime"));
        assert_eq!(stored_files[0].project_area.as_deref(), Some("ui"));
        assert_eq!(stored_files[0].artifact_kind.as_deref(), Some("runtime"));
        assert_eq!(stored_files[0].header_role.as_deref(), Some("public"));
    }

    #[test]
    fn reads_direct_inheritance_edges_for_base_and_derived_queries() {
        let db = Database::open(Path::new(":memory:")).unwrap();
        let references = vec![
            NormalizedReference {
                source_symbol_id: "Game::Player".into(),
                target_symbol_id: "Game::Actor".into(),
                category: ReferenceCategory::InheritanceMention,
                file_path: "player.h".into(),
                line: 12,
                confidence: RawExtractionConfidence::Partial,
            },
            NormalizedReference {
                source_symbol_id: "Game::Enemy".into(),
                target_symbol_id: "Game::Actor".into(),
                category: ReferenceCategory::InheritanceMention,
                file_path: "enemy.h".into(),
                line: 18,
                confidence: RawExtractionConfidence::Partial,
            },
            NormalizedReference {
                source_symbol_id: "Game::Enemy".into(),
                target_symbol_id: "Game::ISerializable".into(),
                category: ReferenceCategory::InheritanceMention,
                file_path: "enemy.h".into(),
                line: 18,
                confidence: RawExtractionConfidence::High,
            },
        ];

        db.write_references(&references).unwrap();

        let player_bases = db.get_direct_base_edges("Game::Player").unwrap();
        assert_eq!(player_bases.len(), 1);
        assert_eq!(player_bases[0].base_symbol_id, "Game::Actor");
        assert_eq!(player_bases[0].confidence, RawExtractionConfidence::Partial);

        let actor_derived = db.get_direct_derived_edges("Game::Actor").unwrap();
        assert_eq!(actor_derived.len(), 2);
        assert_eq!(actor_derived[0].derived_symbol_id, "Game::Player");
        assert_eq!(actor_derived[1].derived_symbol_id, "Game::Enemy");

        let serializable_derived = db.get_direct_derived_edges("Game::ISerializable").unwrap();
        assert_eq!(serializable_derived.len(), 1);
        assert_eq!(serializable_derived[0].derived_symbol_id, "Game::Enemy");
        assert_eq!(serializable_derived[0].confidence, RawExtractionConfidence::High);
    }

    #[test]
    fn quick_check_reports_clean_database() {
        let db = Database::open(Path::new(":memory:")).unwrap();
        assert!(db.quick_check().unwrap().is_empty());
    }

    #[test]
    fn validate_existing_database_reports_invalid_sqlite_file() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("broken.db");
        std::fs::write(&db_path, b"not a sqlite database").unwrap();

        let err = validate_existing_database(&db_path).unwrap_err();
        assert!(err.contains("open failed") || err.contains("quick_check failed"));
    }

    #[test]
    fn index_metadata_round_trips() {
        let db = Database::open(Path::new(":memory:")).unwrap();
        let metadata = IndexMetadata {
            format_version: 1,
            indexer_version: "1.2.3".into(),
            workspace_root: "E:/Dev/project".into(),
            extensions_csv: "cpp,h,hpp".into(),
        };

        db.write_index_metadata(&metadata).unwrap();

        let stored = db.read_index_metadata().unwrap().unwrap();
        assert_eq!(stored, metadata);
    }

    #[test]
    fn existing_database_metadata_issue_detects_missing_and_changed_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("index.db");
        let db = Database::open(&db_path).unwrap();
        let expected = IndexMetadata {
            format_version: 1,
            indexer_version: "1.2.3".into(),
            workspace_root: "E:/Dev/project".into(),
            extensions_csv: "cpp,h,hpp".into(),
        };

        let missing = existing_database_metadata_issue(&db_path, &expected).unwrap();
        assert_eq!(missing, Some("index metadata missing".to_string()));

        db.write_index_metadata(&expected).unwrap();
        let healthy = existing_database_metadata_issue(&db_path, &expected).unwrap();
        assert_eq!(healthy, None);

        let changed = IndexMetadata {
            extensions_csv: "cpp,h,hpp,py".into(),
            ..expected.clone()
        };
        let mismatch = existing_database_metadata_issue(&db_path, &changed).unwrap();
        assert_eq!(
            mismatch,
            Some("indexed extensions changed (db=cpp,h,hpp, current=cpp,h,hpp,py)".to_string())
        );
    }
}
