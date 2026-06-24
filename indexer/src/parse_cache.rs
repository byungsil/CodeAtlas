//! MS22 — Translation-Unit Parse Cache (content-addressable).
//!
//! Caches each C++ translation unit's [`ParseResult`] under a content-addressable
//! key so an unchanged TU is served from disk instead of being re-parsed by
//! libclang. Mirrors Kythe's `.kzip` compilation-record model: a SHA256 digest
//! over the normalized compiler args plus the contents of every input file
//! (the TU source and its direct includes) uniquely identifies a compilation,
//! so a byte-identical compilation never needs re-processing.
//!
//! The cache is a pure memoization layer: any miss, error, or version mismatch
//! falls back to a real parse, and a stored entry is only ever returned for a
//! byte-identical input set. Correctness is therefore identical to parsing every
//! time; the cache only removes redundant work (and the heavy libclang parse
//! permit / 200–500 MB RSS spike that comes with it).
//!
//! Layout (content-addressable, like Kythe `files/<sha256>`):
//! ```text
//! <data_dir>/parse-cache/
//!   v<CACHE_FORMAT_VERSION>/<key[0:2]>/<key>.json   # serialized ParseResult
//! ```
//! Bumping [`CACHE_FORMAT_VERSION`] moves the active directory, so old-format
//! entries are never read and are cleaned up on the next init. A
//! `parser_version_tag` change (see [`crate::clang_parser::PARSER_VERSION_TAG`])
//! is folded into the key instead, so those entries simply never match again and
//! are reclaimed by the size cap.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use sha2::{Digest, Sha256};

use crate::clang_parser::PARSER_VERSION_TAG;
use crate::models::ParseResult;

/// On-disk format version for cached entries. Bump when the serialization
/// scheme or directory layout changes; old `v*` directories are then ignored
/// and cleaned up. (Extraction-shape changes are handled by the parser version
/// tag baked into the key, not by this constant.)
pub const CACHE_FORMAT_VERSION: u32 = 1;

/// Env var to disable the cache entirely (always parse). Set to `0`/`false`/`off`.
const CACHE_ENABLED_ENV: &str = "CODEATLAS_PARSE_CACHE";
/// Env var: soft size cap in megabytes. Oldest entries are evicted on init when
/// the active generation exceeds this. Unset/0 = unbounded.
const CACHE_MAX_MB_ENV: &str = "CODEATLAS_PARSE_CACHE_MAX_MB";

/// Process-global cache handle, initialised once via [`init`]. `None` means the
/// cache is disabled (env switch) or was never initialised — both make
/// [`lookup`]/[`store`] silent no-ops, reproducing pre-MS22 behaviour exactly.
static CACHE: OnceLock<Option<ParseCache>> = OnceLock::new();

/// A direct include of a TU: its path (as written / resolved) and the SHA256 of
/// its contents. Order-independent — [`compute_key`] sorts by path.
pub type DirectInclude = (String, String);

/// Returns whether the cache is enabled per the `CODEATLAS_PARSE_CACHE` env var.
/// Default: enabled. Recognises `0`, `false`, `off`, `no` (case-insensitive) as off.
fn cache_enabled_from_env() -> bool {
    match std::env::var(CACHE_ENABLED_ENV) {
        Ok(v) => {
            let v = v.trim().to_ascii_lowercase();
            !matches!(v.as_str(), "0" | "false" | "off" | "no")
        }
        Err(_) => true,
    }
}

fn max_bytes_from_env() -> Option<u64> {
    let raw = std::env::var(CACHE_MAX_MB_ENV).ok()?;
    let mb = raw.trim().parse::<u64>().ok()?;
    if mb == 0 {
        None
    } else {
        Some(mb * 1024 * 1024)
    }
}

/// Compute the content-addressable cache key (hex SHA256) for a translation unit.
///
/// The digest is composed in a fixed canonical order so it is reproducible
/// regardless of argument order or timestamps (Kythe digest style):
///
/// ```text
/// SHA256(
///   CACHE_FORMAT_VERSION || PARSER_VERSION_TAG ||
///   normalized_args ||           // path-separator-normalized, sorted
///   source_content_hash ||       // SHA256 of the TU source bytes
///   for each direct include (sorted by path): include_path || include_content_hash
/// )
/// ```
///
/// `args` are the `-I`/`-D`/`-x` flags built for the TU; they are normalized
/// (`\` → `/`) and sorted so flag order never changes the key. `source` is the
/// raw TU text. `direct_includes` are `(path, content_hash)` pairs for the TU's
/// first-degree includes (a changed deep header changes the key of whichever TU
/// directly includes it — see MS22 Risk 1).
pub fn compute_key(args: &[String], source: &str, direct_includes: &[DirectInclude]) -> String {
    let mut hasher = Sha256::new();

    hasher.update(b"cas-v");
    hasher.update(CACHE_FORMAT_VERSION.to_le_bytes());
    hasher.update(b"\x1f");
    hasher.update(PARSER_VERSION_TAG.as_bytes());
    hasher.update(b"\x1f");

    // Normalized, sorted args. Separator normalization keeps Windows/Unix path
    // forms identical; sorting makes the key insensitive to flag order.
    let mut norm_args: Vec<String> = args.iter().map(|a| a.replace('\\', "/")).collect();
    norm_args.sort();
    hasher.update(b"args\x1f");
    for arg in &norm_args {
        hasher.update(arg.as_bytes());
        hasher.update(b"\x1e");
    }

    // Source content hash.
    let source_hash = format!("{:x}", Sha256::digest(source.as_bytes()));
    hasher.update(b"src\x1f");
    hasher.update(source_hash.as_bytes());
    hasher.update(b"\x1f");

    // Direct includes, sorted by path for a stable order.
    let mut includes: Vec<&DirectInclude> = direct_includes.iter().collect();
    includes.sort_by(|a, b| a.0.cmp(&b.0));
    hasher.update(b"inc\x1f");
    for (path, content_hash) in includes {
        let norm_path = path.replace('\\', "/");
        hasher.update(norm_path.as_bytes());
        hasher.update(b"\x1e");
        hasher.update(content_hash.as_bytes());
        hasher.update(b"\x1e");
    }

    format!("{:x}", hasher.finalize())
}

/// A handle to the on-disk parse cache rooted at one active generation directory.
pub struct ParseCache {
    /// `<data_dir>/parse-cache/v<CACHE_FORMAT_VERSION>`
    dir: PathBuf,
}

impl ParseCache {
    /// Open (creating if needed) the active cache generation under `data_dir`.
    /// Cleans up stale format-version directories and enforces the size cap.
    /// Returns `None` only when the active directory cannot be created.
    pub fn open(data_dir: &Path) -> Option<ParseCache> {
        let root = data_dir.join("parse-cache");
        let active_name = format!("v{}", CACHE_FORMAT_VERSION);
        let dir = root.join(&active_name);
        if let Err(e) = fs::create_dir_all(&dir) {
            eprintln!("  Parse cache: disabled (cannot create {}: {})", dir.display(), e);
            return None;
        }
        cleanup_stale_versions(&root, &active_name);
        if let Some(max) = max_bytes_from_env() {
            enforce_size_cap(&dir, max);
        }
        Some(ParseCache { dir })
    }

    fn entry_path(&self, key: &str) -> PathBuf {
        let shard = if key.len() >= 2 { &key[0..2] } else { "00" };
        self.dir.join(shard).join(format!("{}.json", key))
    }

    /// Load a cached `ParseResult` for `key`, or `None` on miss / corrupt entry.
    /// A corrupt or unreadable entry is treated as a miss (the caller re-parses),
    /// never an error.
    pub fn load(&self, key: &str) -> Option<ParseResult> {
        let bytes = fs::read(self.entry_path(key)).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    /// Persist `result` under `key`. Best-effort: any IO/serialization error is
    /// swallowed (a failed store just means a future miss). Writes via a temp
    /// file + rename so a concurrent reader never observes a partial entry.
    pub fn store(&self, key: &str, result: &ParseResult) {
        let path = self.entry_path(key);
        if let Some(parent) = path.parent() {
            if fs::create_dir_all(parent).is_err() {
                return;
            }
        }
        let Ok(bytes) = serde_json::to_vec(result) else {
            return;
        };
        let tmp = path.with_extension("json.tmp");
        if fs::write(&tmp, &bytes).is_err() {
            return;
        }
        // rename is atomic on the same filesystem; on Windows it fails if the
        // destination exists, so remove first (best-effort).
        let _ = fs::remove_file(&path);
        let _ = fs::rename(&tmp, &path);
    }
}

/// Remove sibling format-version directories that are not the active one, so a
/// `CACHE_FORMAT_VERSION` bump reclaims old entries.
fn cleanup_stale_versions(root: &Path, active_name: &str) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('v') && name != active_name {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

/// Enforce a soft size cap by evicting oldest entries (by modified time) until
/// the active generation is under `max_bytes`. Runs at init so the parse hot
/// path stays cheap.
fn enforce_size_cap(dir: &Path, max_bytes: u64) {
    let mut files: Vec<(PathBuf, u64, std::time::SystemTime)> = Vec::new();
    let mut total: u64 = 0;
    collect_entries(dir, &mut files, &mut total);
    if total <= max_bytes {
        return;
    }
    // Oldest first.
    files.sort_by(|a, b| a.2.cmp(&b.2));
    let mut evicted = 0u64;
    for (path, size, _) in files {
        if total <= max_bytes {
            break;
        }
        if fs::remove_file(&path).is_ok() {
            total = total.saturating_sub(size);
            evicted += size;
        }
    }
    if evicted > 0 {
        eprintln!(
            "  Parse cache: evicted {} MB to stay under {} MB cap",
            evicted / (1024 * 1024),
            max_bytes / (1024 * 1024)
        );
    }
}

/// Recursively collect `.json` cache entries with their size and mtime.
fn collect_entries(
    dir: &Path,
    out: &mut Vec<(PathBuf, u64, std::time::SystemTime)>,
    total: &mut u64,
) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if meta.is_dir() {
            collect_entries(&path, out, total);
        } else if path.extension().and_then(|e| e.to_str()) == Some("json") {
            let size = meta.len();
            let mtime = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
            *total += size;
            out.push((path, size, mtime));
        }
    }
}

/// Wipe the entire parse-cache tree under `data_dir` (`<data_dir>/parse-cache/`),
/// including every format-version generation. Used by `--rebuild-cache` to force
/// a from-scratch re-parse pass without flipping the env switch. Must be called
/// BEFORE [`init`] — once `init` runs, the active `v*` directory has been
/// recreated and a wipe afterward would race with concurrent stores.
///
/// Best-effort: a missing tree is a no-op; an unremovable subdir surfaces a
/// warning but never aborts the run (the cache simply rebuilds whatever it can).
pub fn wipe(data_dir: &Path) {
    let root = data_dir.join("parse-cache");
    if !root.exists() {
        return;
    }
    match fs::remove_dir_all(&root) {
        Ok(_) => eprintln!("  Parse cache: wiped {} (--rebuild-cache)", root.display()),
        Err(e) => eprintln!(
            "  Parse cache: warning — failed to wipe {}: {} (continuing; stale entries may remain)",
            root.display(),
            e
        ),
    }
}

/// Initialise the process-global parse cache rooted at `data_dir`. Idempotent
/// (first call wins). When the `CODEATLAS_PARSE_CACHE` env switch disables the
/// cache, this stores `None` and the cache becomes a transparent no-op.
pub fn init(data_dir: &Path) {
    CACHE.get_or_init(|| {
        if !cache_enabled_from_env() {
            eprintln!("  Parse cache: disabled (CODEATLAS_PARSE_CACHE)");
            return None;
        }
        let cache = ParseCache::open(data_dir);
        match &cache {
            Some(c) => {
                let cap = match max_bytes_from_env() {
                    Some(b) => format!("{} MB cap", b / (1024 * 1024)),
                    None => "no cap".to_string(),
                };
                eprintln!(
                    "  Parse cache: enabled (v{}, {}) at {}",
                    CACHE_FORMAT_VERSION,
                    cap,
                    c.dir.display()
                );
            }
            None => {}
        }
        cache
    });
}

/// Look up a cached `ParseResult` via the global cache. Returns `None` when the
/// cache is uninitialised, disabled, on a miss, or on a corrupt entry.
pub fn lookup(key: &str) -> Option<ParseResult> {
    match CACHE.get() {
        Some(Some(cache)) => cache.load(key),
        _ => None,
    }
}

/// Store a `ParseResult` via the global cache. No-op when uninitialised or
/// disabled. Best-effort — never surfaces an error to the caller.
pub fn store(key: &str, result: &ParseResult) {
    if let Some(Some(cache)) = CACHE.get() {
        cache.store(key, result);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{
        FileRiskSignals, IncludeDependency, IncludeHeaviness, MacroSensitivity, ParseFragility,
        ParseMetrics, ParseResult, RawCallKind, RawCallSite,
    };
    use tempfile::tempdir;

    fn sample_result() -> ParseResult {
        ParseResult {
            symbols: Vec::new(),
            file_risk_signals: FileRiskSignals {
                parse_fragility: ParseFragility::Low,
                macro_sensitivity: MacroSensitivity::Low,
                include_heaviness: IncludeHeaviness::Light,
            },
            relation_events: Vec::new(),
            normalized_references: Vec::new(),
            propagation_events: Vec::new(),
            callable_flow_summaries: Vec::new(),
            raw_calls: vec![RawCallSite {
                caller_id: "a::b".into(),
                called_name: "foo".into(),
                call_kind: RawCallKind::Unqualified,
                argument_count: Some(2),
                argument_texts: vec!["x".into(), "y".into()],
                result_target: None,
                receiver: None,
                receiver_kind: None,
                qualifier: None,
                qualifier_kind: None,
                pre_resolved_callee_id: Some("usr#123".into()),
                file_path: "src/a.cpp".into(),
                line: 42,
            }],
            metrics: ParseMetrics::default(),
            include_dependencies: vec![IncludeDependency {
                source_file: "src/a.cpp".into(),
                included_file: "a.h".into(),
                line: 1,
                is_system_include: false,
            }],
            macro_definitions: Vec::new(),
            conditional_blocks: Vec::new(),
            dependency_metrics: crate::models::DependencyMetrics::default(),
            conditional_symbols: Vec::new(),
        }
    }

    #[test]
    fn identical_inputs_produce_identical_key() {
        let args = vec!["-Isrc".to_string(), "-DNDEBUG".to_string()];
        let incs = vec![("a.h".to_string(), "hashA".to_string())];
        let k1 = compute_key(&args, "int main(){}", &incs);
        let k2 = compute_key(&args, "int main(){}", &incs);
        assert_eq!(k1, k2);
        assert_eq!(k1.len(), 64); // hex SHA256
    }

    #[test]
    fn arg_reorder_produces_identical_key() {
        let a = vec!["-Isrc".to_string(), "-DNDEBUG".to_string(), "-Iinclude".to_string()];
        let b = vec!["-Iinclude".to_string(), "-DNDEBUG".to_string(), "-Isrc".to_string()];
        let incs: Vec<DirectInclude> = vec![];
        assert_eq!(compute_key(&a, "src", &incs), compute_key(&b, "src", &incs));
    }

    #[test]
    fn path_separator_normalization_makes_key_stable() {
        let a = vec!["-Ic:\\proj\\src".to_string()];
        let b = vec!["-Ic:/proj/src".to_string()];
        let incs: Vec<DirectInclude> = vec![];
        assert_eq!(compute_key(&a, "s", &incs), compute_key(&b, "s", &incs));
    }

    #[test]
    fn changed_define_produces_different_key() {
        let incs: Vec<DirectInclude> = vec![];
        let k1 = compute_key(&["-DA=1".to_string()], "s", &incs);
        let k2 = compute_key(&["-DA=2".to_string()], "s", &incs);
        assert_ne!(k1, k2);
    }

    #[test]
    fn changed_source_produces_different_key() {
        let args = vec!["-Isrc".to_string()];
        let incs: Vec<DirectInclude> = vec![];
        assert_ne!(compute_key(&args, "v1", &incs), compute_key(&args, "v2", &incs));
    }

    #[test]
    fn changed_include_content_produces_different_key() {
        let args = vec!["-Isrc".to_string()];
        let k1 = compute_key(&args, "s", &[("a.h".to_string(), "hash1".to_string())]);
        let k2 = compute_key(&args, "s", &[("a.h".to_string(), "hash2".to_string())]);
        assert_ne!(k1, k2);
    }

    #[test]
    fn include_order_does_not_change_key() {
        let args = vec!["-Isrc".to_string()];
        let a = vec![("a.h".to_string(), "h1".to_string()), ("b.h".to_string(), "h2".to_string())];
        let b = vec![("b.h".to_string(), "h2".to_string()), ("a.h".to_string(), "h1".to_string())];
        assert_eq!(compute_key(&args, "s", &a), compute_key(&args, "s", &b));
    }

    #[test]
    fn store_then_load_round_trips_losslessly() {
        let dir = tempdir().unwrap();
        let cache = ParseCache::open(dir.path()).unwrap();
        let key = compute_key(&["-Isrc".to_string()], "int main(){}", &[]);
        assert!(cache.load(&key).is_none(), "cold cache must miss");

        let original = sample_result();
        cache.store(&key, &original);

        let loaded = cache.load(&key).expect("warm cache must hit");
        assert_eq!(loaded.raw_calls.len(), 1);
        assert_eq!(loaded.raw_calls[0].called_name, "foo");
        assert_eq!(loaded.raw_calls[0].pre_resolved_callee_id.as_deref(), Some("usr#123"));
        assert_eq!(loaded.include_dependencies.len(), 1);
        assert_eq!(loaded.include_dependencies[0].included_file, "a.h");
    }

    #[test]
    fn corrupt_entry_is_treated_as_miss() {
        let dir = tempdir().unwrap();
        let cache = ParseCache::open(dir.path()).unwrap();
        let key = compute_key(&[], "x", &[]);
        let path = cache.entry_path(&key);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"{not valid json").unwrap();
        assert!(cache.load(&key).is_none());
    }

    #[test]
    fn cleanup_stale_versions_removes_old_dirs() {
        let dir = tempdir().unwrap();
        let root = dir.path().join("parse-cache");
        fs::create_dir_all(root.join("v0")).unwrap();
        fs::write(root.join("v0").join("old.json"), b"{}").unwrap();
        // Opening creates the active vN dir and should drop v0.
        let _cache = ParseCache::open(dir.path()).unwrap();
        assert!(!root.join("v0").exists(), "stale version dir should be removed");
        assert!(root.join(format!("v{}", CACHE_FORMAT_VERSION)).exists());
    }

    #[test]
    fn enforce_size_cap_evicts_oldest() {
        let dir = tempdir().unwrap();
        let active = dir.path().join("v1");
        fs::create_dir_all(active.join("aa")).unwrap();
        // Two ~1KB entries; cap of 0 MB rounds to unbounded, so use a tiny byte
        // budget directly via enforce_size_cap.
        let big = vec![b'x'; 1500];
        fs::write(active.join("aa").join("one.json"), &big).unwrap();
        fs::write(active.join("aa").join("two.json"), &big).unwrap();
        // Cap at 2000 bytes — one of the two must be evicted.
        enforce_size_cap(&active, 2000);
        let remaining = fs::read_dir(active.join("aa")).unwrap().count();
        assert_eq!(remaining, 1, "size cap should evict down under budget");
    }
}
