import * as fs from "fs";
import * as path from "path";
import { Symbol } from "../models/symbol";
import { Call } from "../models/call";
import { FileRecord } from "../models/file-record";
import { ReferenceCategory, ReferenceRecord } from "../models/responses";
import { SEARCH_DEFAULT_LIMIT, SEARCH_MIN_QUERY_LENGTH } from "../constants";

export interface IndexData {
  symbols: Symbol[];
  calls: Call[];
  references: ReferenceRecord[];
  files: FileRecord[];
}

export class JsonStore {
  private dataDir: string;

  constructor(dataDir: string) {
    this.dataDir = dataDir;
    if (!fs.existsSync(dataDir)) {
      fs.mkdirSync(dataDir, { recursive: true });
    }
  }

  save(data: IndexData): void {
    fs.writeFileSync(this.symbolsPath(), JSON.stringify(data.symbols, null, 2));
    fs.writeFileSync(this.callsPath(), JSON.stringify(data.calls, null, 2));
    fs.writeFileSync(this.referencesPath(), JSON.stringify(data.references, null, 2));
    fs.writeFileSync(this.filesPath(), JSON.stringify(data.files, null, 2));
  }

  load(): IndexData {
    return {
      symbols: this.readJson(this.symbolsPath(), []),
      calls: this.readJson(this.callsPath(), []),
      references: this.readJson(this.referencesPath(), []),
      files: this.readJson(this.filesPath(), []),
    };
  }

  getSymbolsByName(name: string): Symbol[] {
    const data = this.load();
    return data.symbols.filter((s) => s.name === name);
  }

  getSymbolById(id: string): Symbol | undefined {
    const data = this.load();
    return data.symbols.find((s) => s.id === id);
  }

  getSymbolByQualifiedName(qualifiedName: string): Symbol | undefined {
    const data = this.load();
    return data.symbols.find((s) => s.qualifiedName === qualifiedName);
  }

  getSymbolsByType(type: string): Symbol[] {
    const data = this.load();
    return data.symbols.filter((s) => s.type === type);
  }

  searchSymbols(query: string, type?: string, limit = SEARCH_DEFAULT_LIMIT): { results: Symbol[]; totalCount: number } {
    if (query.length < SEARCH_MIN_QUERY_LENGTH) {
      return { results: [], totalCount: 0 };
    }

    const data = this.load();
    const q = query.toLowerCase();
    let matches = data.symbols.filter(
      (s) => s.name.toLowerCase().includes(q) || s.qualifiedName.toLowerCase().includes(q),
    );
    if (type) {
      matches = matches.filter((s) => s.type === type);
    }
    const totalCount = matches.length;
    return { results: matches.slice(0, limit), totalCount };
  }

  getFileSymbols(filePath: string): Symbol[] {
    const data = this.load();
    return data.symbols
      .filter((symbol) => symbol.filePath === filePath)
      .sort(compareSymbolsForOverview);
  }

  getNamespaceSymbols(namespaceQualifiedName: string): Symbol[] {
    const data = this.load();
    return data.symbols
      .filter((symbol) => symbol.scopeKind === "namespace" && symbol.scopeQualifiedName === namespaceQualifiedName)
      .sort(compareSymbolsForOverview);
  }

  getCallers(symbolId: string): Call[] {
    const data = this.load();
    return data.calls.filter((c) => c.calleeId === symbolId);
  }

  getCallees(symbolId: string): Call[] {
    const data = this.load();
    return data.calls.filter((c) => c.callerId === symbolId);
  }

  getMembers(parentId: string): Symbol[] {
    const data = this.load();
    return data.symbols.filter((s) => s.parentId === parentId);
  }

  getReferences(targetSymbolId: string, category?: ReferenceCategory, filePath?: string): ReferenceRecord[] {
    const data = this.load();
    return data.references.filter((reference) =>
      reference.targetSymbolId === targetSymbolId
      && (!category || reference.category === category)
      && (!filePath || reference.filePath === filePath));
  }

  private symbolsPath(): string {
    return path.join(this.dataDir, "symbols.json");
  }

  private callsPath(): string {
    return path.join(this.dataDir, "calls.json");
  }

  private filesPath(): string {
    return path.join(this.dataDir, "files.json");
  }

  private referencesPath(): string {
    return path.join(this.dataDir, "references.json");
  }

  private readJson<T>(filePath: string, fallback: T): T {
    if (!fs.existsSync(filePath)) return fallback;
    const raw = fs.readFileSync(filePath, "utf-8");
    return JSON.parse(raw) as T;
  }
}

function compareSymbolsForOverview(a: Symbol, b: Symbol): number {
  return a.line - b.line
    || a.endLine - b.endLine
    || a.qualifiedName.localeCompare(b.qualifiedName);
}
