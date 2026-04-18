use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Symbol {
    pub id: String,
    pub name: String,
    pub qualified_name: String,
    #[serde(rename = "type")]
    pub symbol_type: String,
    pub file_path: String,
    pub line: usize,
    pub end_line: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameter_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope_qualified_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol_role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub declaration_file_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub declaration_line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub declaration_end_line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub definition_file_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub definition_line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub definition_end_line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subsystem: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_area: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifact_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub header_role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parse_fragility: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub macro_sensitivity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_heaviness: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Call {
    pub caller_id: String,
    pub callee_id: String,
    pub file_path: String,
    pub line: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileRecord {
    pub path: String,
    pub content_hash: String,
    pub last_indexed: String,
    pub symbol_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subsystem: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_area: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifact_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub header_role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parse_fragility: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub macro_sensitivity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_heaviness: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ParseFragility {
    Low,
    Elevated,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MacroSensitivity {
    Low,
    High,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum IncludeHeaviness {
    Light,
    Heavy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileRiskSignals {
    pub parse_fragility: ParseFragility,
    pub macro_sensitivity: MacroSensitivity,
    pub include_heaviness: IncludeHeaviness,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RawCallKind {
    Unqualified,
    MemberAccess,
    PointerMemberAccess,
    ThisPointerAccess,
    Qualified,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RawReceiverKind {
    Identifier,
    This,
    PointerExpression,
    FieldExpression,
    QualifiedIdentifier,
    Other,
}

// `qualifier_kind` is introduced ahead of parser classification work in M1-T12.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RawQualifierKind {
    Namespace,
    Type,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RawRelationKind {
    Call,
    TypeUsage,
    Inheritance,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ReferenceCategory {
    FunctionCall,
    MethodCall,
    ClassInstantiation,
    TypeUsage,
    InheritanceMention,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PropagationKind {
    Assignment,
    InitializerBinding,
    ArgumentToParameter,
    ReturnValue,
    FieldWrite,
    FieldRead,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RawEventSource {
    LegacyAst,
    TreeSitterGraph,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RawExtractionConfidence {
    High,
    Partial,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PropagationAnchorKind {
    LocalVariable,
    Parameter,
    ReturnValue,
    Field,
    Expression,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PropagationRisk {
    AliasHeavyCode,
    PointerHeavyFlow,
    MacroSensitiveRegion,
    UnresolvedOverload,
    ReceiverAmbiguity,
    UnsupportedFlowShape,
}

#[derive(Debug, Clone)]
pub struct RawCallSite {
    pub caller_id: String,
    pub called_name: String,
    pub call_kind: RawCallKind,
    pub argument_count: Option<usize>,
    pub argument_texts: Vec<String>,
    pub result_target: Option<PropagationAnchor>,
    pub receiver: Option<String>,
    pub receiver_kind: Option<RawReceiverKind>,
    pub qualifier: Option<String>,
    pub qualifier_kind: Option<RawQualifierKind>,
    pub file_path: String,
    pub line: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawRelationEvent {
    pub relation_kind: RawRelationKind,
    pub source: RawEventSource,
    pub confidence: RawExtractionConfidence,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caller_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub call_kind: Option<RawCallKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub argument_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub receiver: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub receiver_kind: Option<RawReceiverKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub qualifier: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub qualifier_kind: Option<RawQualifierKind>,
    pub file_path: String,
    pub line: usize,
}

impl RawRelationEvent {
    pub fn from_raw_call_site(raw_call: &RawCallSite, source: RawEventSource) -> Self {
        Self {
            relation_kind: RawRelationKind::Call,
            source,
            confidence: RawExtractionConfidence::High,
            caller_id: Some(raw_call.caller_id.clone()),
            target_name: Some(raw_call.called_name.clone()),
            call_kind: Some(raw_call.call_kind.clone()),
            argument_count: raw_call.argument_count,
            receiver: raw_call.receiver.clone(),
            receiver_kind: raw_call.receiver_kind.clone(),
            qualifier: raw_call.qualifier.clone(),
            qualifier_kind: raw_call.qualifier_kind.clone(),
            file_path: raw_call.file_path.clone(),
            line: raw_call.line,
        }
    }

    pub fn to_raw_call_site(&self) -> Option<RawCallSite> {
        if self.relation_kind != RawRelationKind::Call {
            return None;
        }

        Some(RawCallSite {
            caller_id: self.caller_id.clone()?,
            called_name: self.target_name.clone()?,
            call_kind: self.call_kind.clone()?,
            argument_count: self.argument_count,
            argument_texts: Vec::new(),
            result_target: None,
            receiver: self.receiver.clone(),
            receiver_kind: self.receiver_kind.clone(),
            qualifier: self.qualifier.clone(),
            qualifier_kind: self.qualifier_kind.clone(),
            file_path: self.file_path.clone(),
            line: self.line,
        })
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NormalizedReference {
    pub source_symbol_id: String,
    pub target_symbol_id: String,
    pub category: ReferenceCategory,
    pub file_path: String,
    pub line: usize,
    pub confidence: RawExtractionConfidence,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InheritanceEdge {
    pub derived_symbol_id: String,
    pub base_symbol_id: String,
    pub file_path: String,
    pub line: usize,
    pub confidence: RawExtractionConfidence,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum OverrideMatchReason {
    InheritanceEdge,
    MatchingMethodName,
    ParameterCountMatch,
    SignatureArityMatch,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OverrideCandidate {
    pub derived_method_id: String,
    pub base_method_id: String,
    pub confidence: RawExtractionConfidence,
    pub reasons: Vec<OverrideMatchReason>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PropagationAnchor {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anchor_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expression_text: Option<String>,
    pub anchor_kind: PropagationAnchorKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PropagationEvent {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner_symbol_id: Option<String>,
    pub source_anchor: PropagationAnchor,
    pub target_anchor: PropagationAnchor,
    pub propagation_kind: PropagationKind,
    pub file_path: String,
    pub line: usize,
    pub confidence: RawExtractionConfidence,
    pub risks: Vec<PropagationRisk>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CallableFlowSummary {
    pub callable_symbol_id: String,
    pub parameter_anchors: Vec<PropagationAnchor>,
    pub return_anchors: Vec<PropagationAnchor>,
}

#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ParseMetrics {
    pub tree_sitter_parse_ms: u128,
    pub syntax_walk_ms: u128,
    pub local_propagation_ms: u128,
    pub local_function_discovery_ms: u128,
    pub local_owner_lookup_ms: u128,
    pub local_seed_ms: u128,
    pub local_event_walk_ms: u128,
    pub local_declaration_ms: u128,
    pub local_expression_statement_ms: u128,
    pub local_return_statement_ms: u128,
    pub local_nested_block_ms: u128,
    pub local_return_collection_ms: u128,
    pub graph_relation_ms: u128,
    pub graph_rule_compile_ms: u128,
    pub graph_rule_execute_ms: u128,
    pub reference_normalization_ms: u128,
}

#[derive(Debug)]
pub struct ParseResult {
    pub symbols: Vec<Symbol>,
    pub file_risk_signals: FileRiskSignals,
    pub relation_events: Vec<RawRelationEvent>,
    pub normalized_references: Vec<NormalizedReference>,
    #[allow(dead_code)]
    pub propagation_events: Vec<PropagationEvent>,
    pub callable_flow_summaries: Vec<CallableFlowSummary>,
    pub raw_calls: Vec<RawCallSite>,
    pub metrics: ParseMetrics,
}
