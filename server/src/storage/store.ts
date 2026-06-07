import { SourceLanguage, Symbol } from "../models/symbol";
import { Call } from "../models/call";
import {
  BaseMethodRecord,
  OverrideRecord,
  PropagationEventRecord,
  PropagationKind,
  ReferenceCategory,
  ReferenceRecord,
} from "../models/responses";

export interface MetadataFilters {
  language?: SourceLanguage;
  subsystem?: string;
  module?: string;
  projectArea?: string;
  artifactKind?: "runtime" | "editor" | "tool" | "test" | "generated";
}

export interface WorkspaceLanguageSummaryRecord {
  language: SourceLanguage;
  fileCount: number;
  symbolCount: number;
}

export interface RawCallCandidateRecord {
  callerId: string;
  calledName: string;
  callKind?: string;
  filePath: string;
  line: number;
  receiver?: string;
  qualifier?: string;
}

export interface IndexCountsRecord {
  symbols: number;
  calls: number;
  references: number;
  propagation: number;
  files: number;
}

export interface FileRiskCountsRecord {
  elevatedParseFragility: number;
  macroSensitive: number;
  includeHeavy: number;
}

export interface IndexDetailsRecord {
  backend: "sqlite" | "json";
  dataPath: string;
  workspaceRoot?: string;
  workspaceName?: string;
  formatVersion?: string;
  indexerVersion?: string;
  extensionsCsv?: string;
  sqliteUserVersion?: number;
  databaseSizeBytes?: number;
  updatedAt?: string;
  counts: IndexCountsRecord;
  fileRiskCounts: FileRiskCountsRecord;
}

export interface Store {
  getSymbolsByName(name: string): Symbol[];
  getSymbolById(id: string): Symbol | undefined;
  getSymbolsByIds(ids: string[]): Symbol[];
  getRepresentativeCandidates(symbolId: string): Symbol[];
  getSymbolByQualifiedName(qualifiedName: string): Symbol | undefined;
  searchSymbols(query: string, type?: string, limit?: number, metadataFilters?: MetadataFilters): { results: Symbol[]; totalCount: number };
  getFileSymbols(filePath: string): Symbol[];
  getNamespaceSymbols(namespaceQualifiedName: string): Symbol[];
  getCallers(symbolId: string): Call[];
  getCallees(symbolId: string): Call[];
  getRawCallersByCalledName?(calledName: string): RawCallCandidateRecord[];
  getRawCallsByCallerId?(callerId: string): RawCallCandidateRecord[];
  getReferences(targetSymbolId: string, category?: ReferenceCategory, filePath?: string): ReferenceRecord[];
  getMemberAccessReferences?(symbolName: string, ownerNames?: string[], filePath?: string): ReferenceRecord[];
  getMembers(parentId: string): Symbol[];
  getDirectBases(symbolId: string): Symbol[];
  getDirectDerived(symbolId: string): Symbol[];
  getBaseMethods(symbolId: string): BaseMethodRecord[];
  getOverrides(symbolId: string): OverrideRecord[];
  getIncomingPropagation(symbolId: string, propagationKinds?: PropagationKind[], filePath?: string): PropagationEventRecord[];
  getOutgoingPropagation(symbolId: string, propagationKinds?: PropagationKind[], filePath?: string): PropagationEventRecord[];
  getWorkspaceLanguageSummary(): WorkspaceLanguageSummaryRecord[];
  getIndexDetails?(): IndexDetailsRecord;

  // Phase 1-3: Enhanced Symbol Queries
  readTypeInferencesForSymbol?(symbolId: string): Array<{ inferred_type?: string; confidence: string }>;
  readFlowTagsForSymbol?(symbolId: string): Array<{ tagKind: string; label?: string }>;
  readFlowPathsFromSource?(sourceSymbolId: string): Array<{ target_symbol_id: string; hops_json?: string; semantic_tags_json?: string }>;
  readAnalysisRules?(category?: string | null, language?: string | null): Array<{ rule_id: string; ruleset_name: string; category: string; severity: string }>;
  readAnalysisResultsForFile?(filePath: string): Array<{ rule_id: string; file_path: string; line_start: number; line_end: number; match_text?: string }>;
}
