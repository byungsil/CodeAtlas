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

export interface CallerQueryResponse {
  lookupMode: LookupMode;
  symbol: Symbol;
  confidence: ConfidenceLevel;
  matchReasons: MatchReason[];
  ambiguity?: AmbiguityInfo;
  window: ResultWindow;
  callers: CallReference[];
  totalCount: number;
  truncated: boolean;
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
  window: ResultWindow;
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

export type ReferenceCategory =
  | "functionCall"
  | "methodCall"
  | "classInstantiation"
  | "typeUsage"
  | "inheritanceMention";

export interface ReferenceRecord {
  sourceSymbolId: string;
  targetSymbolId: string;
  category: ReferenceCategory;
  filePath: string;
  line: number;
  confidence: "high" | "partial";
}

export interface ResolvedReference extends ReferenceRecord {
  sourceSymbolName: string;
  sourceQualifiedName: string;
}

export interface ReferenceQueryResponse extends ConfidenceMetadata {
  lookupMode: LookupMode;
  symbol: Symbol;
  window: ResultWindow;
  references: ResolvedReference[];
  totalCount: number;
  truncated: boolean;
  category?: ReferenceCategory;
  filePath?: string;
}

export interface ImpactedSymbolSummary {
  symbolId: string;
  symbolName: string;
  qualifiedName: string;
  type: Symbol["type"];
  filePath: string;
  count: number;
}

export interface ImpactedFileSummary {
  filePath: string;
  count: number;
}

export interface ImpactAnalysisResponse extends ConfidenceMetadata {
  lookupMode: LookupMode;
  symbol: Symbol;
  maxDepth: number;
  directCallers: CallReference[];
  directCallees: CallReference[];
  directReferences: ResolvedReference[];
  totalAffectedSymbols: number;
  totalAffectedFiles: number;
  topAffectedSymbols: ImpactedSymbolSummary[];
  topAffectedFiles: ImpactedFileSummary[];
  suggestedFollowUpQueries: string[];
  truncated: boolean;
}

export interface StructureOverviewSummary {
  totalCount: number;
  typeCounts: Record<string, number>;
}

export interface ResultWindow {
  returnedCount: number;
  totalCount: number;
  truncated: boolean;
  limitApplied?: number;
}

export interface FileSymbolsResponse {
  filePath: string;
  summary: StructureOverviewSummary;
  window: ResultWindow;
  symbols: Symbol[];
}

export interface NamespaceSymbolsResponse extends ConfidenceMetadata {
  lookupMode: LookupMode;
  symbol: Symbol;
  summary: StructureOverviewSummary;
  window: ResultWindow;
  symbols: Symbol[];
}

export interface ClassMembersOverviewResponse extends ConfidenceMetadata {
  lookupMode: LookupMode;
  symbol: Symbol;
  summary: StructureOverviewSummary;
  window: ResultWindow;
  members: Symbol[];
}

export interface ErrorResponse {
  error: string;
  code: string;
}
