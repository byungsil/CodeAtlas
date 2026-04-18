import * as fs from "fs";
import * as os from "os";
import * as path from "path";
import Database from "better-sqlite3";
import { Symbol } from "../models/symbol";
import { Call } from "../models/call";
import {
  BaseMethodRecord,
  MatchReason,
  OverrideRecord,
  ReferenceCategory,
  ReferenceRecord,
  TypeHierarchyNode,
} from "../models/responses";
import { SEARCH_DEFAULT_LIMIT, SEARCH_MIN_QUERY_LENGTH } from "../constants";
import { MetadataFilters } from "./store";

export class SqliteStore {
  private db: Database.Database;
  private snapshotPath?: string;

  constructor(dbPath: string) {
    const opened = openReadonlyStore(dbPath);
    this.db = opened.db;
    this.snapshotPath = opened.snapshotPath;
  }

  getSymbolsByName(name: string): Symbol[] {
    const rows = this.db
      .prepare("SELECT * FROM symbols WHERE name = ?")
      .all(name) as RawRow[];
    return rows.map(toSymbol);
  }

  getSymbolById(id: string): Symbol | undefined {
    const row = this.db
      .prepare("SELECT * FROM symbols WHERE id = ?")
      .get(id) as RawRow | undefined;
    return row ? toSymbol(row) : undefined;
  }

  getSymbolByQualifiedName(qualifiedName: string): Symbol | undefined {
    const row = this.db
      .prepare("SELECT * FROM symbols WHERE qualified_name = ?")
      .get(qualifiedName) as RawRow | undefined;
    return row ? toSymbol(row) : undefined;
  }

  searchSymbols(query: string, type?: string, limit = SEARCH_DEFAULT_LIMIT, metadataFilters?: MetadataFilters): { results: Symbol[]; totalCount: number } {
    if (query.length < SEARCH_MIN_QUERY_LENGTH) {
      return { results: [], totalCount: 0 };
    }

    if (metadataFilters && !this.hasMetadataColumns()) {
      return { results: [], totalCount: 0 };
    }

    if (query.length >= 3 && this.hasFts()) {
      return this.searchWithFts(query, type, limit, metadataFilters);
    }
    return this.searchWithLike(query, type, limit, metadataFilters);
  }

  getFileSymbols(filePath: string): Symbol[] {
    const rows = this.db
      .prepare(`
        SELECT *
        FROM symbols
        WHERE file_path = ?
        ORDER BY line, end_line, qualified_name
      `)
      .all(filePath) as RawRow[];
    return rows.map(toSymbol);
  }

  getNamespaceSymbols(namespaceQualifiedName: string): Symbol[] {
    const rows = this.db
      .prepare(`
        SELECT *
        FROM symbols
        WHERE scope_kind = 'namespace' AND scope_qualified_name = ?
        ORDER BY line, end_line, qualified_name
      `)
      .all(namespaceQualifiedName) as RawRow[];
    return rows.map(toSymbol);
  }

  private hasFts(): boolean {
    try {
      const row = this.db
        .prepare("SELECT COUNT(*) as cnt FROM sqlite_master WHERE type='table' AND name='symbols_fts'")
        .get() as { cnt: number };
      return row.cnt > 0;
    } catch {
      return false;
    }
  }

  private searchWithFts(query: string, type: string | undefined, limit: number, metadataFilters?: MetadataFilters): { results: Symbol[]; totalCount: number } {
    const ftsQuery = `"${query.replace(/"/g, '""')}"`;
    const whereClauses = ["symbols_fts MATCH ?"];
    const values: Array<string | number> = [ftsQuery];
    if (type) {
      whereClauses.push("s.type = ?");
      values.push(type);
    }
    appendMetadataFilters(whereClauses, values, metadataFilters, "s");
    let totalCount: number;
    let rows: RawRow[];
    totalCount = (this.db
      .prepare(`SELECT COUNT(*) as cnt FROM symbols s JOIN symbols_fts f ON s.id = f.id WHERE ${whereClauses.join(" AND ")}`)
      .get(...values) as { cnt: number }).cnt;
    rows = this.db
      .prepare(`SELECT s.* FROM symbols s JOIN symbols_fts f ON s.id = f.id WHERE ${whereClauses.join(" AND ")} LIMIT ?`)
      .all(...values, limit) as RawRow[];

    return { results: rows.map(toSymbol), totalCount };
  }

  private searchWithLike(query: string, type: string | undefined, limit: number, metadataFilters?: MetadataFilters): { results: Symbol[]; totalCount: number } {
    const pattern = `%${query}%`;
    const whereClauses = ["(name LIKE ? OR qualified_name LIKE ?)"];
    const values: Array<string | number> = [pattern, pattern];
    if (type) {
      whereClauses.push("type = ?");
      values.push(type);
    }
    appendMetadataFilters(whereClauses, values, metadataFilters);

    let totalCount: number;
    let rows: RawRow[];
    totalCount = (this.db
      .prepare(`SELECT COUNT(*) as cnt FROM symbols WHERE ${whereClauses.join(" AND ")}`)
      .get(...values) as { cnt: number }).cnt;
    rows = this.db
      .prepare(`SELECT * FROM symbols WHERE ${whereClauses.join(" AND ")} LIMIT ?`)
      .all(...values, limit) as RawRow[];

    return { results: rows.map(toSymbol), totalCount };
  }

  getCallers(symbolId: string): Call[] {
    const rows = this.db
      .prepare("SELECT * FROM calls WHERE callee_id = ?")
      .all(symbolId) as RawCallRow[];
    return rows.map(toCall);
  }

  getCallees(symbolId: string): Call[] {
    const rows = this.db
      .prepare("SELECT * FROM calls WHERE caller_id = ?")
      .all(symbolId) as RawCallRow[];
    return rows.map(toCall);
  }

  getMembers(parentId: string): Symbol[] {
    const rows = this.db
      .prepare("SELECT * FROM symbols WHERE parent_id = ?")
      .all(parentId) as RawRow[];
    return rows.map(toSymbol);
  }

  getDirectBases(symbolId: string): Symbol[] {
    if (!this.hasReferencesTable()) {
      return [];
    }

    const rows = this.db
      .prepare(`
        SELECT s.*
        FROM symbol_references r
        JOIN symbols s ON s.id = r.target_symbol_id
        WHERE r.category = 'inheritanceMention' AND r.source_symbol_id = ?
        ORDER BY r.line, s.qualified_name
      `)
      .all(symbolId) as RawRow[];
    return rows.map(toSymbol);
  }

  getDirectDerived(symbolId: string): Symbol[] {
    if (!this.hasReferencesTable()) {
      return [];
    }

    const rows = this.db
      .prepare(`
        SELECT s.*
        FROM symbol_references r
        JOIN symbols s ON s.id = r.source_symbol_id
        WHERE r.category = 'inheritanceMention' AND r.target_symbol_id = ?
        ORDER BY r.line, s.qualified_name
      `)
      .all(symbolId) as RawRow[];
    return rows.map(toSymbol);
  }

  getBaseMethods(symbolId: string): BaseMethodRecord[] {
    const method = this.getSymbolById(symbolId);
    if (!method || method.type !== "method" || !method.parentId) {
      return [];
    }

    const results: BaseMethodRecord[] = [];
    for (const base of this.getDirectBases(method.parentId)) {
      for (const candidate of this.getMembers(base.id)) {
        if (candidate.type !== "method" || candidate.name !== method.name) {
          continue;
        }
        const inferred = inferOverrideConfidence(method, candidate);
        results.push({
          method: candidate,
          owner: toHierarchyNode(base),
          confidence: inferred.confidence,
          matchReasons: inferred.matchReasons,
        });
      }
    }

    return results.sort(compareOverrideRecords);
  }

  getOverrides(symbolId: string): OverrideRecord[] {
    const method = this.getSymbolById(symbolId);
    if (!method || method.type !== "method" || !method.parentId) {
      return [];
    }

    const results: OverrideRecord[] = [];
    for (const derived of this.getDirectDerived(method.parentId)) {
      for (const candidate of this.getMembers(derived.id)) {
        if (candidate.type !== "method" || candidate.name !== method.name) {
          continue;
        }
        const inferred = inferOverrideConfidence(candidate, method);
        results.push({
          method: candidate,
          owner: toHierarchyNode(derived),
          confidence: inferred.confidence,
          matchReasons: inferred.matchReasons,
        });
      }
    }

    return results.sort(compareOverrideRecords);
  }

  getReferences(targetSymbolId: string, category?: ReferenceCategory, filePath?: string): ReferenceRecord[] {
    if (!this.hasReferencesTable()) {
      return [];
    }

    const filters = ["target_symbol_id = ?"];
    const values: Array<string> = [targetSymbolId];
    if (category) {
      filters.push("category = ?");
      values.push(category);
    }
    if (filePath) {
      filters.push("file_path = ?");
      values.push(filePath);
    }

    const sql = `
      SELECT source_symbol_id, target_symbol_id, category, file_path, line, confidence
      FROM symbol_references
      WHERE ${filters.join(" AND ")}
      ORDER BY category, file_path, line, source_symbol_id
    `;
    const rows = this.db.prepare(sql).all(...values) as RawReferenceRow[];
    return rows.map(toReference);
  }

  private hasReferencesTable(): boolean {
    try {
      const row = this.db
        .prepare("SELECT COUNT(*) as cnt FROM sqlite_master WHERE type='table' AND name='symbol_references'")
        .get() as { cnt: number };
      return row.cnt > 0;
    } catch {
      return false;
    }
  }

  private hasMetadataColumns(): boolean {
    try {
      this.db.prepare("SELECT module, subsystem, project_area, artifact_kind FROM symbols LIMIT 0").get();
      return true;
    } catch {
      return false;
    }
  }

  close(): void {
    this.db.close();
    if (this.snapshotPath) {
      try {
        fs.unlinkSync(this.snapshotPath);
      } catch {
        // Best-effort cleanup for read-only snapshot fallback.
      }
    }
  }
}

function openReadonlyStore(dbPath: string): { db: Database.Database; snapshotPath?: string } {
  const retryDelaysMs = [0, 100, 250, 500];

  for (const delayMs of retryDelaysMs) {
    if (delayMs > 0) {
      sleepMs(delayMs);
    }
    try {
      const db = openReadonlyDatabase(dbPath);
      verifyDatabaseReadable(db);
      return { db };
    } catch {
      // Retry the original file first to absorb short-lived file-indexer locks.
    }
  }

  const snapshotPath = createSnapshotPath(dbPath);
  fs.copyFileSync(dbPath, snapshotPath);
  const db = openReadonlyDatabase(snapshotPath);
  verifyDatabaseReadable(db);
  return { db, snapshotPath };
}

function openReadonlyDatabase(dbPath: string): Database.Database {
  const db = new Database(dbPath, { readonly: true, fileMustExist: true });
  // Read-only consumers do not need to force WAL mode, and some external
  // databases reject changing journal mode during open. Keep lookup usable.
  try {
    db.pragma("journal_mode = WAL");
  } catch {
    // Ignore pragma failures for read-only query workloads.
  }
  return db;
}

function verifyDatabaseReadable(db: Database.Database): void {
  db.prepare("SELECT COUNT(*) as cnt FROM sqlite_master").get();
}

function createSnapshotPath(dbPath: string): string {
  const snapshotDir = path.join(os.tmpdir(), "codeatlas-sqlite-snapshots");
  fs.mkdirSync(snapshotDir, { recursive: true });
  const baseName = path.basename(dbPath, path.extname(dbPath));
  return path.join(snapshotDir, `${baseName}-${process.pid}-${Date.now()}.db`);
}

function sleepMs(delayMs: number): void {
  Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, delayMs);
}

interface RawRow {
  id: string;
  name: string;
  qualified_name: string;
  type: string;
  file_path: string;
  line: number;
  end_line: number;
  signature: string | null;
  parameter_count: number | null;
  scope_qualified_name: string | null;
  scope_kind: string | null;
  symbol_role: string | null;
  declaration_file_path: string | null;
  declaration_line: number | null;
  declaration_end_line: number | null;
  definition_file_path: string | null;
  definition_line: number | null;
  definition_end_line: number | null;
  parent_id: string | null;
  module: string | null;
  subsystem: string | null;
  project_area: string | null;
  artifact_kind: "runtime" | "editor" | "tool" | "test" | "generated" | null;
  header_role: "public" | "private" | "internal" | null;
}

interface RawCallRow {
  caller_id: string;
  callee_id: string;
  file_path: string;
  line: number;
}

interface RawReferenceRow {
  source_symbol_id: string;
  target_symbol_id: string;
  category: ReferenceCategory;
  file_path: string;
  line: number;
  confidence: "high" | "partial";
}

function toSymbol(row: RawRow): Symbol {
  return {
    id: row.id,
    name: row.name,
    qualifiedName: row.qualified_name,
    type: row.type as any,
    filePath: row.file_path,
    line: row.line,
    endLine: row.end_line,
    ...(row.signature ? { signature: row.signature } : {}),
    ...(row.parameter_count !== null ? { parameterCount: row.parameter_count } : {}),
    ...(row.scope_qualified_name ? { scopeQualifiedName: row.scope_qualified_name } : {}),
    ...(row.scope_kind ? { scopeKind: row.scope_kind as "namespace" | "class" | "struct" } : {}),
    ...(row.symbol_role ? { symbolRole: row.symbol_role as "declaration" | "definition" | "inline_definition" } : {}),
    ...(row.declaration_file_path ? { declarationFilePath: row.declaration_file_path } : {}),
    ...(row.declaration_line !== null ? { declarationLine: row.declaration_line } : {}),
    ...(row.declaration_end_line !== null ? { declarationEndLine: row.declaration_end_line } : {}),
    ...(row.definition_file_path ? { definitionFilePath: row.definition_file_path } : {}),
    ...(row.definition_line !== null ? { definitionLine: row.definition_line } : {}),
    ...(row.definition_end_line !== null ? { definitionEndLine: row.definition_end_line } : {}),
    ...(row.parent_id ? { parentId: row.parent_id } : {}),
    ...(row.module ? { module: row.module } : {}),
    ...(row.subsystem ? { subsystem: row.subsystem } : {}),
    ...(row.project_area ? { projectArea: row.project_area } : {}),
    ...(row.artifact_kind ? { artifactKind: row.artifact_kind } : {}),
    ...(row.header_role ? { headerRole: row.header_role } : {}),
  };
}

function toCall(row: RawCallRow): Call {
  return {
    callerId: row.caller_id,
    calleeId: row.callee_id,
    filePath: row.file_path,
    line: row.line,
  };
}

function toReference(row: RawReferenceRow): ReferenceRecord {
  return {
    sourceSymbolId: row.source_symbol_id,
    targetSymbolId: row.target_symbol_id,
    category: row.category,
    filePath: row.file_path,
    line: row.line,
    confidence: row.confidence,
  };
}

function toHierarchyNode(symbol: Symbol): TypeHierarchyNode {
  return {
    symbolId: symbol.id,
    qualifiedName: symbol.qualifiedName,
    type: symbol.type,
    filePath: symbol.filePath,
    line: symbol.line,
  };
}

function compareOverrideRecords(left: BaseMethodRecord | OverrideRecord, right: BaseMethodRecord | OverrideRecord): number {
  return left.owner.qualifiedName.localeCompare(right.owner.qualifiedName)
    || left.method.qualifiedName.localeCompare(right.method.qualifiedName);
}

function inferOverrideConfidence(
  derivedMethod: Symbol,
  baseMethod: Symbol,
): { confidence: "high" | "partial"; matchReasons: MatchReason[] } {
  const matchReasons: MatchReason[] = [
    "override_inheritance_match",
    "override_name_match",
  ];

  if (
    derivedMethod.parameterCount !== undefined
    && baseMethod.parameterCount !== undefined
    && derivedMethod.parameterCount === baseMethod.parameterCount
  ) {
    matchReasons.push("override_parameter_count_match");
    return { confidence: "high", matchReasons };
  }

  const derivedArity = inferSignatureArity(derivedMethod.signature);
  const baseArity = inferSignatureArity(baseMethod.signature);
  if (derivedArity !== undefined && derivedArity === baseArity) {
    matchReasons.push("override_signature_arity_match");
    return { confidence: "high", matchReasons };
  }

  return { confidence: "partial", matchReasons };
}

function inferSignatureArity(signature?: string): number | undefined {
  if (!signature) return undefined;
  const start = signature.indexOf("(");
  const end = signature.lastIndexOf(")");
  if (start < 0 || end <= start) return undefined;
  const params = signature.slice(start + 1, end).trim();
  if (!params || params === "void") return 0;
  return params.split(",").length;
}

function appendMetadataFilters(
  whereClauses: string[],
  values: Array<string | number>,
  metadataFilters?: MetadataFilters,
  alias?: string,
): void {
  if (!metadataFilters) {
    return;
  }

  const prefix = alias ? `${alias}.` : "";
  if (metadataFilters.subsystem) {
    whereClauses.push(`${prefix}subsystem = ?`);
    values.push(metadataFilters.subsystem);
  }
  if (metadataFilters.module) {
    whereClauses.push(`${prefix}module = ?`);
    values.push(metadataFilters.module);
  }
  if (metadataFilters.projectArea) {
    whereClauses.push(`${prefix}project_area = ?`);
    values.push(metadataFilters.projectArea);
  }
  if (metadataFilters.artifactKind) {
    whereClauses.push(`${prefix}artifact_kind = ?`);
    values.push(metadataFilters.artifactKind);
  }
}
