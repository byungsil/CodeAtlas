import * as fs from "fs";
import * as path from "path";
import { Symbol } from "../models/symbol";
import { Call } from "../models/call";
import { FileRecord } from "../models/file-record";
import {
  BaseMethodRecord,
  MatchReason,
  OverrideRecord,
  PropagationEventRecord,
  PropagationKind,
  ReferenceCategory,
  ReferenceRecord,
  TypeHierarchyNode,
} from "../models/responses";
import { SEARCH_DEFAULT_LIMIT, SEARCH_MIN_QUERY_LENGTH } from "../constants";
import { MetadataFilters } from "./store";

export interface IndexData {
  symbols: Symbol[];
  calls: Call[];
  references: ReferenceRecord[];
  propagationEvents: PropagationEventRecord[];
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
    fs.writeFileSync(this.propagationPath(), JSON.stringify(data.propagationEvents, null, 2));
    fs.writeFileSync(this.filesPath(), JSON.stringify(data.files, null, 2));
  }

  load(): IndexData {
    return {
      symbols: this.readJson(this.symbolsPath(), []),
      calls: this.readJson(this.callsPath(), []),
      references: this.readJson(this.referencesPath(), []),
      propagationEvents: this.readJson(this.propagationPath(), []),
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

  getSymbolsByIds(ids: string[]): Symbol[] {
    if (ids.length === 0) {
      return [];
    }
    const wanted = new Set(ids);
    const data = this.load();
    return data.symbols.filter((symbol) => wanted.has(symbol.id));
  }

  getSymbolByQualifiedName(qualifiedName: string): Symbol | undefined {
    const data = this.load();
    return data.symbols.find((s) => s.qualifiedName === qualifiedName);
  }

  getSymbolsByType(type: string): Symbol[] {
    const data = this.load();
    return data.symbols.filter((s) => s.type === type);
  }

  searchSymbols(query: string, type?: string, limit = SEARCH_DEFAULT_LIMIT, metadataFilters?: MetadataFilters): { results: Symbol[]; totalCount: number } {
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
    if (metadataFilters) {
      matches = matches.filter((symbol) => matchesMetadataFilters(symbol, metadataFilters));
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

  getDirectBases(symbolId: string): Symbol[] {
    const data = this.load();
    const baseIds = data.references
      .filter((reference) => reference.category === "inheritanceMention" && reference.sourceSymbolId === symbolId)
      .map((reference) => reference.targetSymbolId);
    return baseIds
      .map((id) => data.symbols.find((symbol) => symbol.id === id))
      .filter((symbol): symbol is Symbol => symbol !== undefined);
  }

  getDirectDerived(symbolId: string): Symbol[] {
    const data = this.load();
    const derivedIds = data.references
      .filter((reference) => reference.category === "inheritanceMention" && reference.targetSymbolId === symbolId)
      .map((reference) => reference.sourceSymbolId);
    return derivedIds
      .map((id) => data.symbols.find((symbol) => symbol.id === id))
      .filter((symbol): symbol is Symbol => symbol !== undefined);
  }

  getBaseMethods(symbolId: string): BaseMethodRecord[] {
    const data = this.load();
    const method = data.symbols.find((symbol) => symbol.id === symbolId);
    if (!method || method.type !== "method" || !method.parentId) {
      return [];
    }

    const results: BaseMethodRecord[] = [];
    for (const base of this.getDirectBases(method.parentId)) {
      for (const candidate of data.symbols.filter((symbol) => symbol.parentId === base.id && symbol.type === "method")) {
        if (candidate.name !== method.name) {
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
    const data = this.load();
    const method = data.symbols.find((symbol) => symbol.id === symbolId);
    if (!method || method.type !== "method" || !method.parentId) {
      return [];
    }

    const results: OverrideRecord[] = [];
    for (const derived of this.getDirectDerived(method.parentId)) {
      for (const candidate of data.symbols.filter((symbol) => symbol.parentId === derived.id && symbol.type === "method")) {
        if (candidate.name !== method.name) {
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
    const data = this.load();
    return data.references.filter((reference) =>
      reference.targetSymbolId === targetSymbolId
      && (!category || reference.category === category)
      && (!filePath || reference.filePath === filePath));
  }

  getIncomingPropagation(symbolId: string, propagationKinds?: PropagationKind[], filePath?: string): PropagationEventRecord[] {
    return this.load().propagationEvents
      .filter((event) => matchesPropagationDirection(event, symbolId, "incoming"))
      .filter((event) => !propagationKinds || propagationKinds.includes(event.propagationKind))
      .filter((event) => !filePath || event.filePath === filePath)
      .sort(comparePropagationEvents);
  }

  getOutgoingPropagation(symbolId: string, propagationKinds?: PropagationKind[], filePath?: string): PropagationEventRecord[] {
    return this.load().propagationEvents
      .filter((event) => matchesPropagationDirection(event, symbolId, "outgoing"))
      .filter((event) => !propagationKinds || propagationKinds.includes(event.propagationKind))
      .filter((event) => !filePath || event.filePath === filePath)
      .sort(comparePropagationEvents);
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

  private propagationPath(): string {
    return path.join(this.dataDir, "propagation.json");
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

function matchesMetadataFilters(symbol: Symbol, filters: MetadataFilters): boolean {
  if (filters.subsystem && symbol.subsystem !== filters.subsystem) return false;
  if (filters.module && symbol.module !== filters.module) return false;
  if (filters.projectArea && symbol.projectArea !== filters.projectArea) return false;
  if (filters.artifactKind && symbol.artifactKind !== filters.artifactKind) return false;
  return true;
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

function matchesPropagationDirection(
  event: PropagationEventRecord,
  symbolId: string,
  direction: "incoming" | "outgoing",
): boolean {
  const ownedAnchorPrefix = `${symbolId}::`;
  if (direction === "incoming") {
    return event.targetAnchor.symbolId === symbolId
      || event.targetAnchor.anchorId?.startsWith(ownedAnchorPrefix)
      || event.ownerSymbolId === symbolId && event.propagationKind === "argumentToParameter";
  }

  return event.sourceAnchor.symbolId === symbolId
    || event.sourceAnchor.anchorId?.startsWith(ownedAnchorPrefix)
    || event.ownerSymbolId === symbolId;
}

function comparePropagationEvents(left: PropagationEventRecord, right: PropagationEventRecord): number {
  return left.filePath.localeCompare(right.filePath)
    || left.line - right.line
    || left.propagationKind.localeCompare(right.propagationKind)
    || (left.sourceAnchor.anchorId ?? left.sourceAnchor.expressionText ?? "").localeCompare(
      right.sourceAnchor.anchorId ?? right.sourceAnchor.expressionText ?? "",
    )
    || (left.targetAnchor.anchorId ?? left.targetAnchor.expressionText ?? "").localeCompare(
      right.targetAnchor.anchorId ?? right.targetAnchor.expressionText ?? "",
    );
}
