import { Symbol } from "./symbol";
import { Call } from "./call";

export type LookupMode = "exact" | "heuristic";

export type ConfidenceLevel =
  | "exact"
  | "high_confidence_heuristic"
  | "ambiguous"
  | "unresolved";

export type MatchReason =
  | "exact_id_match"
  | "exact_qualified_name_match"
  | "same_parent_match"
  | "same_namespace_match"
  | "this_receiver_match"
  | "member_call_prefers_method"
  | "qualified_type_match"
  | "qualified_namespace_match"
  | "parameter_count_match"
  | "signature_arity_hint"
  | "ambiguous_top_score"
  | "no_viable_candidate";

export interface AmbiguityInfo {
  candidateCount: number;
}

export interface ConfidenceMetadata {
  confidence: ConfidenceLevel;
  matchReasons: MatchReason[];
  ambiguity?: AmbiguityInfo;
}

export interface SymbolLookupResponse extends ConfidenceMetadata {
  lookupMode: LookupMode;
  symbol: Symbol;
  callers?: CallReference[];
  callees?: CallReference[];
  members?: Symbol[];
}

export interface FunctionResponse {
  lookupMode: LookupMode;
  symbol: Symbol;
  confidence: ConfidenceLevel;
  matchReasons: MatchReason[];
  ambiguity?: AmbiguityInfo;
  callers: CallReference[];
  callees: CallReference[];
}

export interface ClassResponse {
  lookupMode: LookupMode;
  symbol: Symbol;
  confidence: ConfidenceLevel;
  matchReasons: MatchReason[];
  ambiguity?: AmbiguityInfo;
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
  confidence: ConfidenceLevel;
  matchReasons: MatchReason[];
  ambiguity?: AmbiguityInfo;
}

export interface ErrorResponse {
  error: string;
  code: string;
}
