import { Symbol } from "../models/symbol";
import { Call } from "../models/call";
import { ReferenceCategory, ReferenceRecord } from "../models/responses";

export interface Store {
  getSymbolsByName(name: string): Symbol[];
  getSymbolById(id: string): Symbol | undefined;
  getSymbolByQualifiedName(qualifiedName: string): Symbol | undefined;
  searchSymbols(query: string, type?: string, limit?: number): { results: Symbol[]; totalCount: number };
  getFileSymbols(filePath: string): Symbol[];
  getNamespaceSymbols(namespaceQualifiedName: string): Symbol[];
  getCallers(symbolId: string): Call[];
  getCallees(symbolId: string): Call[];
  getReferences(targetSymbolId: string, category?: ReferenceCategory, filePath?: string): ReferenceRecord[];
  getMembers(parentId: string): Symbol[];
}
