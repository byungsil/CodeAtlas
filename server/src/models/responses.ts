import { SourceLanguage, Symbol } from "./symbol";
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
  | "base_parent_match"
  | "same_namespace_match"
  | "same_file_match"
  | "same_directory_match"
  | "same_file_stem_match"
  | "receiver_parent_name_match"
  | "owner_factory_type_match"
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

export type RepresentativeConfidence =
  | "canonical"
  | "acceptable"
  | "weak";

export type RepresentativeSelectionReason =
  | "outOfLineDefinitionPreferred"
  | "inlineDefinitionFallback"
  | "declarationOnlyFallback"
  | "runtimeArtifactPreferred"
  | "publicHeaderPreferred"
  | "nonTestPathPreferred"
  | "nonGeneratedPathPreferred"
  | "scopeCanonicalityPreferred"
  | "duplicateClusterWeakCanonicality";

export interface RepresentativeMetadata {
  representativeConfidence: RepresentativeConfidence;
  representativeSelectionReasons: RepresentativeSelectionReason[];
  alternateCanonicalCandidateCount?: number;
}

export interface AmbiguityInfo {
  candidateCount: number;
}

export interface HeuristicTopCandidate {
  id: string;
  qualifiedName: string;
  filePath: string;
  line: number;
  signature?: string;
  ownerQualifiedName?: string;
  artifactKind?: Symbol["artifactKind"];
  module?: string;
  subsystem?: string;
  exactQuery?: string;
  discriminator?: string;
  rankScore: number;
}

export interface HeuristicSelectionMetadata {
  selectedReason?: string;
  bestNextDiscriminator?: string;
  suggestedExactQueries?: string[];
  topCandidates?: HeuristicTopCandidate[];
}

export interface ConfidenceMetadata {
  confidence: ConfidenceLevel;
  matchReasons: MatchReason[];
  ambiguity?: AmbiguityInfo;
}

export type ReliabilityLevel = "full" | "partial" | "low";

export type ReliabilityFactor =
  | "elevated_parse_fragility"
  | "macro_sensitive"
  | "include_heavy";

export type IndexCoverageLevel = "full" | "partial" | "low";

export type CallResolutionKind = "resolved" | "recovered";

export type CallProvenanceKind = "resolved_call_edge" | "raw_call";

export interface ReliabilitySummary {
  level: ReliabilityLevel;
  factors: ReliabilityFactor[];
  suggestion?: string;
}

export interface ReliabilityMetadata {
  reliability: ReliabilitySummary;
  indexCoverage?: IndexCoverageLevel;
  recoveredResultCount?: number;
  coverageWarning?: string;
}

export interface SymbolLookupResponse extends ConfidenceMetadata {
  lookupMode: LookupMode;
  symbol: Symbol;
  representativeConfidence?: RepresentativeConfidence;
  representativeSelectionReasons?: RepresentativeSelectionReason[];
  alternateCanonicalCandidateCount?: number;
  callers?: CallReference[];
  callees?: CallReference[];
  members?: Symbol[];
}

export interface FunctionResponse extends HeuristicSelectionMetadata, ReliabilityMetadata {
  lookupMode: LookupMode;
  symbol: Symbol;
  confidence: ConfidenceLevel;
  matchReasons: MatchReason[];
  ambiguity?: AmbiguityInfo;
  callers: CallReference[];
  callees: CallReference[];
}

export interface CallerQueryResponse extends HeuristicSelectionMetadata, ReliabilityMetadata {
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
  groupedByLanguage?: MetadataGroupSummary[];
}

export interface ClassResponse extends HeuristicSelectionMetadata {
  lookupMode: LookupMode;
  symbol: Symbol;
  confidence: ConfidenceLevel;
  matchReasons: MatchReason[];
  ambiguity?: AmbiguityInfo;
  members: Symbol[];
}

export interface OverloadCandidate {
  id: string;
  qualifiedName: string;
  filePath: string;
  line: number;
  signature?: string;
}

export interface OverloadGroup {
  qualifiedName: string;
  type: Symbol["type"];
  count: number;
  candidates: OverloadCandidate[];
}

export interface OverloadQueryResponse {
  query: string;
  totalCount: number;
  groupCount: number;
  groups: OverloadGroup[];
}

export interface SearchResponse {
  query: string;
  window: ResultWindow;
  results: Symbol[];
  totalCount: number;
  truncated: boolean;
  language?: SourceLanguage;
  subsystem?: string;
  module?: string;
  projectArea?: string;
  artifactKind?: "runtime" | "editor" | "tool" | "test" | "generated";
  groupedByLanguage?: MetadataGroupSummary[];
}

export interface CallGraphNode {
  symbol: Pick<Symbol, "id" | "name" | "qualifiedName" | "type" | "filePath" | "line">;
  callees: CallGraphEdge[];
  callers?: CallGraphEdge[];
}

export interface CompactCallGraphSymbol {
  id: string;
  name: string;
  qualifiedName: string;
  filePath: string;
  line: number;
}

export interface CompactCallGraphNode {
  symbol: CompactCallGraphSymbol;
  callees: CallGraphEdge[];
  callers?: CallGraphEdge[];
}

export interface CallGraphEdge {
  targetId: string;
  targetName: string;
  targetQualifiedName: string;
  filePath: string;
  line: number;
  resolutionKind?: CallResolutionKind;
  provenanceKind?: CallProvenanceKind;
  children?: CallGraphEdge[];
}

export type CallGraphDirection = "callees" | "callers" | "both";

export interface CallGraphResponse extends ReliabilityMetadata {
  root: CallGraphNode;
  direction: CallGraphDirection;
  depth: number;
  maxDepth: number;
  nodeCount: number;
  nodeCap: number;
  truncated: boolean;
}

export interface CompactCallGraphResponse extends ReliabilityMetadata {
  responseMode: "compact";
  root: CompactCallGraphNode;
  direction: CallGraphDirection;
  depth: number;
  maxDepth: number;
  nodeCount: number;
  nodeCap: number;
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
  resolutionKind?: CallResolutionKind;
  provenanceKind?: CallProvenanceKind;
}

export type ReferenceCategory =
  | "functionCall"
  | "methodCall"
  | "classInstantiation"
  | "moduleImport"
  | "typeUsage"
  | "inheritanceMention"
  | "enumValueUsage";

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

export interface ReferenceQueryResponse extends ConfidenceMetadata, ReliabilityMetadata {
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
  groupedByLanguage?: MetadataGroupSummary[];
}

export interface FileGroupedRef {
  symbol: string;
  line: number;
}

export interface FileGroup {
  file: string;
  refs: FileGroupedRef[];
}

export interface CompactFileGroupedReferenceResponse extends ConfidenceMetadata, ReliabilityMetadata {
  responseMode: "compact";
  lookupMode: LookupMode;
  symbol: Symbol;
  window: ResultWindow;
  fileGroups: FileGroup[];
  totalCount: number;
  truncated: boolean;
  category?: ReferenceCategory;
  groupedBySubsystem?: MetadataGroupSummary[];
  groupedByModule?: MetadataGroupSummary[];
  groupedByLanguage?: MetadataGroupSummary[];
}

export interface CompactCallerQueryResponse extends HeuristicSelectionMetadata, ReliabilityMetadata {
  responseMode: "compact";
  lookupMode: LookupMode;
  symbol: Symbol;
  confidence: ConfidenceLevel;
  matchReasons: MatchReason[];
  ambiguity?: AmbiguityInfo;
  window: ResultWindow;
  fileGroups: FileGroup[];
  totalCount: number;
  truncated: boolean;
  groupedBySubsystem?: MetadataGroupSummary[];
  groupedByModule?: MetadataGroupSummary[];
  groupedByLanguage?: MetadataGroupSummary[];
}

export interface CompactSearchResponse {
  responseMode: "compact";
  query: string;
  window: ResultWindow;
  results: CompactFileSymbol[];
  totalCount: number;
  truncated: boolean;
  groupedByLanguage?: MetadataGroupSummary[];
}

export interface CompactImpactAnalysisResponse extends ConfidenceMetadata {
  responseMode: "compact";
  lookupMode: LookupMode;
  symbol: Symbol;
  maxDepth: number;
  callerFileGroups: FileGroup[];
  calleeFileGroups: FileGroup[];
  referenceFileGroups: FileGroup[];
  totalAffectedSymbols: number;
  totalAffectedFiles: number;
  topAffectedFiles: ImpactedFileSummary[];
  suggestedFollowUpQueries: string[];
  truncated: boolean;
  affectedSubsystems?: MetadataGroupSummary[];
  affectedModules?: MetadataGroupSummary[];
  affectedLanguages?: MetadataGroupSummary[];
}

export interface CompactClassMembersResponse extends ConfidenceMetadata {
  responseMode: "compact";
  lookupMode: LookupMode;
  symbol: Symbol;
  summary: StructureOverviewSummary;
  window: ResultWindow;
  members: CompactFileSymbol[];
}

export interface CompactNamespaceSymbolsResponse extends ConfidenceMetadata {
  responseMode: "compact";
  lookupMode: LookupMode;
  symbol: Symbol;
  summary: StructureOverviewSummary;
  window: ResultWindow;
  symbols: CompactFileSymbol[];
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
  affectedLanguages?: MetadataGroupSummary[];
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
  offset?: number;
  hasMore?: boolean;
}

export interface FileSymbolsResponse {
  filePath: string;
  summary: StructureOverviewSummary;
  window: ResultWindow;
  symbols: Symbol[];
}

export interface CompactFileSymbol {
  id: string;
  name: string;
  qualifiedName: string;
  type: Symbol["type"];
  line: number;
  endLine: number;
}

export interface CompactFileSymbolsResponse {
  responseMode: "compact";
  filePath: string;
  summary: StructureOverviewSummary;
  window: ResultWindow;
  symbols: CompactFileSymbol[];
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

export type PropagationKind =
  | "assignment"
  | "initializerBinding"
  | "argumentToParameter"
  | "returnValue"
  | "fieldWrite"
  | "fieldRead";

export type PropagationAnchorKind =
  | "localVariable"
  | "parameter"
  | "returnValue"
  | "field"
  | "expression";

export type PropagationConfidence = "high" | "partial";

export type PropagationRisk =
  | "aliasHeavyCode"
  | "pointerHeavyFlow"
  | "macroSensitiveRegion"
  | "unresolvedOverload"
  | "receiverAmbiguity"
  | "unsupportedFlowShape";

export interface PropagationAnchor {
  anchorId?: string;
  symbolId?: string;
  expressionText?: string;
  anchorKind: PropagationAnchorKind;
}

export interface PropagationEventRecord {
  ownerSymbolId?: string;
  sourceAnchor: PropagationAnchor;
  targetAnchor: PropagationAnchor;
  propagationKind: PropagationKind;
  filePath: string;
  line: number;
  confidence: PropagationConfidence;
  risks: PropagationRisk[];
}

export interface PropagationPathStep extends PropagationEventRecord {
  hop: number;
}

export interface TraceVariableFlowResponse extends ConfidenceMetadata {
  lookupMode: LookupMode;
  symbol: Symbol;
  window: ResultWindow;
  propagationConfidence: PropagationConfidence;
  riskMarkers: PropagationRisk[];
  confidenceNotes: string[];
  pathFound: boolean;
  truncated: boolean;
  maxDepth: number;
  maxEdges: number;
  propagationKinds?: PropagationKind[];
  steps: PropagationPathStep[];
  suggestedFollowUpQueries: string[];
}

export interface ExplainSymbolPropagationResponse extends ConfidenceMetadata {
  lookupMode: LookupMode;
  symbol: Symbol;
  window: ResultWindow;
  propagationConfidence: PropagationConfidence;
  incoming: PropagationEventRecord[];
  outgoing: PropagationEventRecord[];
  riskMarkers: PropagationRisk[];
  confidenceNotes: string[];
  summary: string[];
  suggestedFollowUpQueries: string[];
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

export type WorkflowPathConfidence = "high" | "partial";

export type WorkflowCoverageConfidence = "high" | "partial" | "weak";

export type InvestigationHandoffKind =
  | "call"
  | "assignment"
  | "initializerBinding"
  | "argumentToParameter"
  | "returnValue"
  | "fieldWrite"
  | "fieldRead"
  | "reference";

export interface InvestigationAnchorSummary {
  symbolId?: string;
  symbolName?: string;
  qualifiedName?: string;
  filePath?: string;
  line?: number;
  anchorKind?: PropagationAnchorKind;
  expressionText?: string;
}

export interface InvestigationWorkflowStep {
  hop: number;
  handoffKind: InvestigationHandoffKind;
  ownerSymbolId?: string;
  ownerQualifiedName?: string;
  from: InvestigationAnchorSummary;
  to: InvestigationAnchorSummary;
  filePath: string;
  line: number;
  confidence: WorkflowPathConfidence;
  risks: PropagationRisk[];
}

export type InvestigationEvidenceKind =
  | "adjacentCall"
  | "boundaryArgument"
  | "boundaryReturn"
  | "fieldAssignment"
  | "fieldRead"
  | "ownerContext";

export interface InvestigationEvidence {
  kind: InvestigationEvidenceKind;
  summary: string;
  filePath: string;
  line: number;
  relatedSymbolId?: string;
  relatedQualifiedName?: string;
  confidence: WorkflowPathConfidence;
  risks: PropagationRisk[]; 
}

export interface InvestigationSuggestedLookupCandidate {
  shortName: string;
  symbol: Symbol;
  query: string;
  confidence: ConfidenceLevel;
  advisory?: string;
  supportingEvidence?: Array<{
    kind: InvestigationEvidenceKind;
    relatedQualifiedName?: string;
  }>;
  contextSummary?: {
    qualifiedName: string;
    artifactKind?: Symbol["artifactKind"];
    subsystem?: string;
    module?: string;
    projectArea?: string;
    filePath?: string;
  };
  ambiguity?: AmbiguityInfo;
}

export interface InvestigateWorkflowResponse extends ConfidenceMetadata {
  lookupMode: LookupMode;
  source: Symbol;
  target?: Symbol;
  window: ResultWindow;
  targetConfidence: ConfidenceLevel;
  pathConfidence: WorkflowPathConfidence;
  coverageConfidence: WorkflowCoverageConfidence;
  pathFound: boolean;
  truncated: boolean;
  entry: InvestigationAnchorSummary;
  mainPath: InvestigationWorkflowStep[];
  handoffPoints: InvestigationWorkflowStep[];
  evidence: InvestigationEvidence[];
  sink?: InvestigationAnchorSummary;
  uncertainSegments: string[];
  diagnostics: string[];
  suggestedFollowUpQueries: string[];
  suggestedLookupCandidates?: InvestigationSuggestedLookupCandidate[];
}

export interface ErrorResponse {
  error: string;
  code: string;
}

export interface WorkspaceLanguageSummary {
  language: SourceLanguage;
  fileCount: number;
  symbolCount: number;
}

export interface WorkspaceSummaryResponse {
  languages: WorkspaceLanguageSummary[];
  totalFiles: number;
  totalSymbols: number;
}
