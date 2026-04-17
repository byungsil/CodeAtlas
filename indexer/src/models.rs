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
}

#[derive(Debug, Clone)]
pub enum RawCallKind {
    Unqualified,
    MemberAccess,
    PointerMemberAccess,
    ThisPointerAccess,
    Qualified,
}

#[derive(Debug, Clone, PartialEq, Eq)]
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RawQualifierKind {
    Namespace,
    Type,
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

#[derive(Debug)]
pub struct ParseResult {
    pub symbols: Vec<Symbol>,
    pub raw_calls: Vec<RawCallSite>,
}
