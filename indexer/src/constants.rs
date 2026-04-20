use std::collections::HashSet;

pub const EXTENSIONS: &[&str] = &[
    "c", "cpp", "h", "hpp", "cc", "cxx", "inl", "inc", "lua", "py", "ts", "tsx", "rs",
];
pub const INDEX_EXTENSIONS_ENV: &str = "CODEATLAS_INDEX_EXTENSIONS";
pub const DATA_DIR_NAME: &str = ".codeatlas";
pub const DB_FILENAME: &str = "index.db";
pub const ACTIVE_DB_POINTER_FILENAME: &str = "current-db.json";

pub fn configured_extensions() -> HashSet<String> {
    match std::env::var(INDEX_EXTENSIONS_ENV) {
        Ok(value) => parse_extension_list(&value)
            .unwrap_or_else(|| EXTENSIONS.iter().map(|ext| (*ext).to_string()).collect()),
        Err(_) => EXTENSIONS.iter().map(|ext| (*ext).to_string()).collect(),
    }
}

pub fn is_indexed_extension(extension: &str) -> bool {
    configured_extensions().contains(&extension.to_ascii_lowercase())
}

pub fn parse_extension_list(raw: &str) -> Option<HashSet<String>> {
    let mut extensions = HashSet::new();

    for part in raw.split(',') {
        let normalized = part.trim().trim_start_matches('.').to_ascii_lowercase();
        if normalized.is_empty() {
            continue;
        }
        if !EXTENSIONS.contains(&normalized.as_str()) {
            return None;
        }
        extensions.insert(normalized);
    }

    if extensions.is_empty() {
        None
    } else {
        Some(extensions)
    }
}
