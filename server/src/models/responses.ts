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
  | "override_inheritance_match"
  | "override_name_match"
  | "override_parameter_count_match"
  | "override_signature_arity_match"
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
  subsystem?: string;
  module?: string;
  projectArea?: string;
  artifactKind?: "runtime" | "editor" | "tool" | "test" | "generated";
  groupedBySubsystem?: MetadataGroupSummary[];
  groupedByModule?: MetadataGroupSummary[];
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
  subsystem?: string;
  module?: string;
  projectArea?: string;
  artifactKind?: "runtime" | "editor" | "tool" | "test" | "generated";
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

export interface MetadataGroupSummary {
  key: string;
  count: number;
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
  subsystem?: string;
  module?: string;
  projectArea?: string;
  artifactKind?: "runtime" | "editor" | "tool" | "test" | "generated";
  groupedBySubsystem?: MetadataGroupSummary[];
  groupedByModule?: MetadataGroupSummary[];
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
  subsystem?: string;
  module?: string;
  projectArea?: string;
  artifactKind?: "runtime" | "editor" | "tool" | "test" | "generated";
  affectedSubsystems?: MetadataGroupSummary[];
  affectedModules?: MetadataGroupSummary[];
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

export interface OverrideCandidateRecord {
  derivedMethodId: string;
  baseMethodId: string;
  confidence: "high" | "partial";
  matchReasons: MatchReason[];
}

export interface TypeHierarchyNode {
  symbolId: string;
  qualifiedName: string;
  type: Symbol["type"];
  filePath: string;
  line: number;
}

export interface TypeHierarchyResponse extends ConfidenceMetadata {
  lookupMode: LookupMode;
  symbol: Symbol;
  directBases: TypeHierarchyNode[];
  directDerived: TypeHierarchyNode[];
  window: ResultWindow;
}

export interface BaseMethodRecord {
  method: Symbol;
  owner: TypeHierarchyNode;
  confidence: "high" | "partial";
  matchReasons: MatchReason[];
}

export interface BaseMethodsResponse extends ConfidenceMetadata {
  lookupMode: LookupMode;
  symbol: Symbol;
  window: ResultWindow;
  baseMethods: BaseMethodRecord[];
}

export interface OverrideRecord {
  method: Symbol;
  owner: TypeHierarchyNode;
  confidence: "high" | "partial";
  matchReasons: MatchReason[];
}

export interface OverrideQueryResponse extends ConfidenceMetadata {
  lookupMode: LookupMode;
  symbol: Symbol;
  window: ResultWindow;
  overrides: OverrideRecord[];
}

export interface CallPathStep {
  callerId: string;
  callerQualifiedName: string;
  calleeId: string;
  calleeQualifiedName: string;
  filePath: string;
  line: number;
}

export interface TraceCallPathResponse {
  source: Symbol;
  target: Symbol;
  maxDepth: number;
  pathFound: boolean;
  truncated: boolean;
  steps: CallPathStep[];
}

export interface ErrorResponse {
  error: string;
  code: string;
}
