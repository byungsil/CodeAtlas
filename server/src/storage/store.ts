import { Symbol } from "../models/symbol";
import { Call } from "../models/call";
import {
  BaseMethodRecord,
  OverrideRecord,
  ReferenceCategory,
  ReferenceRecord,
} from "../models/responses";

export interface MetadataFilters {
  subsystem?: string;
  module?: string;
  projectArea?: string;
  artifactKind?: "runtime" | "editor" | "tool" | "test" | "generated";
}

export interface Store {
  getSymbolsByName(name: string): Symbol[];
  getSymbolById(id: string): Symbol | undefined;
  getSymbolByQualifiedName(qualifiedName: string): Symbol | undefined;
  searchSymbols(query: string, type?: string, limit?: number, metadataFilters?: MetadataFilters): { results: Symbol[]; totalCount: number };
  getFileSymbols(filePath: string): Symbol[];
  getNamespaceSymbols(namespaceQualifiedName: string): Symbol[];
  getCallers(symbolId: string): Call[];
  getCallees(symbolId: string): Call[];
  getReferences(targetSymbolId: string, category?: ReferenceCategory, filePath?: string): ReferenceRecord[];
  getMembers(parentId: string): Symbol[];
  getDirectBases(symbolId: string): Symbol[];
  getDirectDerived(symbolId: string): Symbol[];
  getBaseMethods(symbolId: string): BaseMethodRecord[];
  getOverrides(symbolId: string): OverrideRecord[];
}
