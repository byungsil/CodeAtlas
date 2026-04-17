import { Symbol } from "./symbol";
import { Call } from "./call";

export interface FunctionResponse {
  symbol: Symbol;
  callers: CallReference[];
  callees: CallReference[];
}

export interface ClassResponse {
  symbol: Symbol;
  members: Symbol[];
}

export interface SearchResponse {
  query: string;
  results: Symbol[];
  totalCount: number;
  truncated: boolean;
}

export interface CallGraphNode {
  symbol: Pick<Symbol, "id" | "name" | "qualifiedName" | "type" | "filePath" | "line">;
  callees: CallGraphEdge[];
}

export interface CallGraphEdge {
  targetId: string;
  targetName: string;
  targetQualifiedName: string;
  filePath: string;
  line: number;
  children?: CallGraphEdge[];
}

export interface CallGraphResponse {
  root: CallGraphNode;
  depth: number;
  maxDepth: number;
  truncated: boolean;
}

export interface CallReference {
  symbolId: string;
  symbolName: string;
  qualifiedName: string;
  filePath: string;
  line: number;
}

export interface ErrorResponse {
  error: string;
  code: string;
}
