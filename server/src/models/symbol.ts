export type SourceLanguage =
  | "cpp"
  | "lua"
  | "python"
  | "typescript"
  | "rust";

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
  language: SourceLanguage;
  type: SymbolType;
  filePath: string;
  line: number;
  endLine: number;
  signature?: string;
  parameterCount?: number;
  scopeQualifiedName?: string;
  scopeKind?: "namespace" | "class" | "struct";
  symbolRole?: "declaration" | "definition" | "inline_definition";
  declarationFilePath?: string;
  declarationLine?: number;
  declarationEndLine?: number;
  definitionFilePath?: string;
  definitionLine?: number;
  definitionEndLine?: number;
  parentId?: string;
  module?: string;
  subsystem?: string;
  projectArea?: string;
  artifactKind?: "runtime" | "editor" | "tool" | "test" | "generated";
  headerRole?: "public" | "private" | "internal";
  parseFragility?: "low" | "elevated";
  macroSensitivity?: "low" | "high";
  includeHeaviness?: "light" | "heavy";
}
