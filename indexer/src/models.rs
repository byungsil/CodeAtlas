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
pub struct RawCallSite {
    pub caller_id: String,
    pub called_name: String,
    pub receiver: Option<String>,
    pub file_path: String,
    pub line: usize,
}

#[derive(Debug)]
pub struct ParseResult {
    pub symbols: Vec<Symbol>,
    pub raw_calls: Vec<RawCallSite>,
}
