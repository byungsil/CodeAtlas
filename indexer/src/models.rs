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

#[derive(Debug, Clone)]
pub struct RawCallSite {
    pub caller_id: String,
    pub called_name: String,
    pub call_kind: RawCallKind,
    pub argument_count: Option<usize>,
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

#[derive(Debug)]
pub struct ParseResult {
    pub symbols: Vec<Symbol>,
    pub relation_events: Vec<RawRelationEvent>,
    pub normalized_references: Vec<NormalizedReference>,
    pub raw_calls: Vec<RawCallSite>,
}
