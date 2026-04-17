import { Symbol } from "../models/symbol";
import { Call } from "../models/call";

export interface Store {
  getSymbolsByName(name: string): Symbol[];
  getSymbolById(id: string): Symbol | undefined;
  searchSymbols(query: string, type?: string, limit?: number): { results: Symbol[]; totalCount: number };
  getCallers(symbolId: string): Call[];
  getCallees(symbolId: string): Call[];
  getMembers(parentId: string): Symbol[];
}
