export type SymbolType =
  | "function"
  | "method"
  | "class"
  | "struct"
  | "enum"
  | "namespace"
  | "variable"
  | "typedef";

export interface Symbol {
  id: string;
  name: string;
  qualifiedName: string;
  type: SymbolType;
  filePath: string;
  line: number;
  endLine: number;
  signature?: string;
  parentId?: string;
}
