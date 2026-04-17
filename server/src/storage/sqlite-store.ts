import Database from "better-sqlite3";
import { Symbol } from "../models/symbol";
import { Call } from "../models/call";
import { FileRecord } from "../models/file-record";
import { SEARCH_DEFAULT_LIMIT, SEARCH_MIN_QUERY_LENGTH } from "../constants";

export class SqliteStore {
  private db: Database.Database;

  constructor(dbPath: string) {
    this.db = new Database(dbPath, { readonly: true });
    this.db.pragma("journal_mode = WAL");
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

  searchSymbols(query: string, type?: string, limit = SEARCH_DEFAULT_LIMIT): { results: Symbol[]; totalCount: number } {
    if (query.length < SEARCH_MIN_QUERY_LENGTH) {
      return { results: [], totalCount: 0 };
    }

    if (query.length >= 3 && this.hasFts()) {
      return this.searchWithFts(query, type, limit);
    }
    return this.searchWithLike(query, type, limit);
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

  private searchWithFts(query: string, type: string | undefined, limit: number): { results: Symbol[]; totalCount: number } {
    const ftsQuery = `"${query.replace(/"/g, '""')}"`;

    let totalCount: number;
    let rows: RawRow[];
    if (type) {
      totalCount = (this.db
        .prepare("SELECT COUNT(*) as cnt FROM symbols s JOIN symbols_fts f ON s.id = f.id WHERE symbols_fts MATCH ? AND s.type = ?")
        .get(ftsQuery, type) as { cnt: number }).cnt;
      rows = this.db
        .prepare("SELECT s.* FROM symbols s JOIN symbols_fts f ON s.id = f.id WHERE symbols_fts MATCH ? AND s.type = ? LIMIT ?")
        .all(ftsQuery, type, limit) as RawRow[];
    } else {
      totalCount = (this.db
        .prepare("SELECT COUNT(*) as cnt FROM symbols s JOIN symbols_fts f ON s.id = f.id WHERE symbols_fts MATCH ?")
        .get(ftsQuery) as { cnt: number }).cnt;
      rows = this.db
        .prepare("SELECT s.* FROM symbols s JOIN symbols_fts f ON s.id = f.id WHERE symbols_fts MATCH ? LIMIT ?")
        .all(ftsQuery, limit) as RawRow[];
    }

    return { results: rows.map(toSymbol), totalCount };
  }

  private searchWithLike(query: string, type: string | undefined, limit: number): { results: Symbol[]; totalCount: number } {
    const pattern = `%${query}%`;

    let totalCount: number;
    let rows: RawRow[];
    if (type) {
      totalCount = (this.db
        .prepare("SELECT COUNT(*) as cnt FROM symbols WHERE (name LIKE ? OR qualified_name LIKE ?) AND type = ?")
        .get(pattern, pattern, type) as { cnt: number }).cnt;
      rows = this.db
        .prepare("SELECT * FROM symbols WHERE (name LIKE ? OR qualified_name LIKE ?) AND type = ? LIMIT ?")
        .all(pattern, pattern, type, limit) as RawRow[];
    } else {
      totalCount = (this.db
        .prepare("SELECT COUNT(*) as cnt FROM symbols WHERE (name LIKE ? OR qualified_name LIKE ?)")
        .get(pattern, pattern) as { cnt: number }).cnt;
      rows = this.db
        .prepare("SELECT * FROM symbols WHERE (name LIKE ? OR qualified_name LIKE ?) LIMIT ?")
        .all(pattern, pattern, limit) as RawRow[];
    }

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

  close(): void {
    this.db.close();
  }
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
}

interface RawCallRow {
  caller_id: string;
  callee_id: string;
  file_path: string;
  line: number;
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
