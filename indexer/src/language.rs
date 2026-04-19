use serde::{Deserialize, Serialize};
use std::path::Path;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SourceLanguage {
    Cpp,
    Lua,
    Python,
    TypeScript,
    Rust,
}

impl SourceLanguage {
    pub fn display_name(self) -> &'static str {
        match self {
            SourceLanguage::Cpp => "cpp",
            SourceLanguage::Lua => "lua",
            SourceLanguage::Python => "python",
            SourceLanguage::TypeScript => "typescript",
            SourceLanguage::Rust => "rust",
        }
    }

    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            SourceLanguage::Cpp => &["c", "cpp", "h", "hpp", "cc", "cxx", "inl", "inc"],
            SourceLanguage::Lua => &["lua"],
            SourceLanguage::Python => &["py"],
            SourceLanguage::TypeScript => &["ts", "tsx"],
            SourceLanguage::Rust => &["rs"],
        }
    }

    pub fn from_extension(extension: &str) -> Option<Self> {
        let normalized = extension.to_ascii_lowercase();
        [
            SourceLanguage::Cpp,
            SourceLanguage::Lua,
            SourceLanguage::Python,
            SourceLanguage::TypeScript,
            SourceLanguage::Rust,
        ]
        .into_iter()
        .find(|language| language.extensions().contains(&normalized.as_str()))
    }

    pub fn from_path(path: &Path) -> Option<Self> {
        path.extension()
            .and_then(|ext| ext.to_str())
            .and_then(Self::from_extension)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredSourceFile {
    pub path: PathBuf,
    pub language: SourceLanguage,
}
