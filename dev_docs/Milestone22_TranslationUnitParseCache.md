# Milestone 22. Translation-Unit Parse Cache (Content-Addressable)

Status:

- Not started

## 1. Objective

Eliminate redundant libclang re-parsing during incremental and restart indexing by caching
each C++ translation unit's `ParseResult` under a content-addressable key, mirroring Kythe's
`.kzip` compilation-record model.

Kythe captures each compilation as a record digested by SHA256 over the compiler arguments
plus all `required_input` file contents (`kythe/docs/kythe-kzip.txt`,
`kythe/proto/analysis.proto` `CompilationUnit`), so an unchanged compilation never needs
re-processing. CodeAtlas currently re-runs libclang on every parse
(`indexing.rs:CppLanguageAdapter::parse_file`, line 127), even when a translation unit's
inputs are byte-for-byte identical to a previous run — the dominant cost during header
fanout, where most dependent TUs are unchanged apart from the edited header.

Reference: `dev_docs/Kythe_Glean_Applicability_Review.md` 제안 A.

Success outcome:

- a TU whose normalized compile args + source content + direct-include header contents are
  unchanged is served from cache, skipping the libclang parse entirely (and its parse permit)
- incremental re-index after a header signature change re-parses only the TUs whose inputs
  actually changed; the rest hit the cache
- restarting the indexer on an unchanged tree hits the cache for all TUs
- correctness is identical to a fresh parse (cache is a pure memoization, never a substitute
  for changed inputs)

Positioning note:

- complements MS16: MS16 reduces *how many* files are re-parsed (symbol-level fanout
  narrowing); MS22 reduces *the cost of each* re-parse that still happens
- libclang parsing is the heaviest stage and is already concurrency-limited by
  `acquire_cpp_parse_permit()` (`indexing.rs:90`, 200–500 MB RSS per TU) — cache hits avoid
  the permit and the RSS spike entirely, directly serving the "minimize PC performance impact"
  goal

Scope note:

- C++ only (the libclang path). Tree-sitter languages are already fast and out of scope.
- cache is an optimization layer; any miss, error, or version mismatch falls back to a real parse
- no DB schema change — cache lives in a separate on-disk store under `.codeatlas/`

---

## 2. Applicability Review

The C++ parse entry point is `CppLanguageAdapter::parse_file()` (`indexing.rs:127-179`):

- builds `args` (`-I` include dirs + `-D` defines) from `BuildMetadataContext`
  (`indexing.rs:144-157`)
- falls back to tree-sitter when no build entry exists (`indexing.rs:137-139`)
- acquires a parse permit (`indexing.rs:176`) then calls
  `clang_parser::parse_cpp_file(file_path, source, &args, workspace_root)` (`indexing.rs:178`)

The returned `ParseResult` (`models.rs:554-571`) is fully serializable — its members
(`Symbol`, `RawCallSite`, `PropagationEvent`, `IncludeDependency`, `MacroDefinition`, etc.)
already derive `Serialize`/`Deserialize`. `ParseResult.include_dependencies`
(`Vec<IncludeDependency>`) gives the **direct includes** of the TU, which is exactly the
input set needed to key the cache (Kythe's `required_input`).

Gap: there is no memoization. Every call re-parses. For a header edit that fans out to N
dependent TUs (even after MS16 narrowing), each unchanged TU pays full libclang cost.

Why content-addressable (not mtime): mtime is unreliable across branch switches / p4 sync
(MS16 §2 already notes this). Kythe uses SHA256 over contents precisely to be reproducible
regardless of timestamps and across machines.

Included in MS22:

1. a CAS key derived from normalized args + source hash + direct-include header hashes
2. an on-disk cache store under `.codeatlas/parse-cache/` (Kythe `files/<sha256>` layout)
3. a cache gate in `CppLanguageAdapter::parse_file()` before the libclang call
4. conservative bypass for macro-sensitive TUs
5. generation-based eviction reusing the existing DB generation pattern

Explicitly not in scope:

- transitive-include hashing (cost-prohibitive; direct includes + macro bypass cover it — see Risk 1)
- caching tree-sitter languages
- cross-machine cache sharing (local only for now; CAS design keeps the door open)

---

## 3. Recommended Order

1. M22-E1. CAS key + cache store (Rust, standalone module, unit-testable in isolation)
2. M22-E2. Wire cache gate into the C++ adapter
3. M22-E3. Eviction + operational controls
4. M22-E4. Validation and release readiness

Why this order:

- the key/store is pure and testable without touching the parse path
- the gate depends on the store
- eviction depends on the store existing and being populated
- validation measures the integrated hit-rate and correctness

---

## 4. Epic Breakdown

### M22-E1. CAS Key and Cache Store

Status:

- Not started

Goal:

- compute a reproducible cache key for a TU and persist/load a `ParseResult` by that key

Design:

Cache key (hex SHA256), composed in canonical order (Kythe digest style):

```
SHA256(
  CACHE_FORMAT_VERSION  ||
  parser_version_tag    ||   // bump when clang_parser output shape changes
  normalized_args       ||   // sorted, path-normalized -I/-D list
  source_content_hash   ||   // SHA256 of the TU source bytes
  for each direct include (sorted by path):
     include_path || include_content_hash
)
```

- `normalized_args`: take the `args` built at `indexing.rs:144-157`, normalize path
  separators and sort `-I`/`-D` independently so argument order never changes the key
- direct includes come from a first cheap pass or from the prior `ParseResult.include_dependencies`;
  on a cold TU with no prior record, the key still includes source + args (a header content
  change then forces a miss via the dependent TU's own source hash and macro bypass — see Risk 1)
- store layout (CAS, content-addressable like Kythe `files/<sha256>`):
  ```
  .codeatlas/parse-cache/
    <gen>/<key[0:2]>/<key>.bin    # serialized ParseResult (e.g. bincode/postcard)
  ```

Implementation tasks:

- M22-E1-T1. New module `indexer/src/parse_cache.rs` with `ParseCacheKey` and
  `compute_key(args, source, direct_includes) -> String`
- M22-E1-T2. Define `CACHE_FORMAT_VERSION` const and a `parser_version_tag` sourced from a
  single constant in `clang_parser.rs` (bumped whenever extraction output changes)
- M22-E1-T3. `fn load(key) -> Option<ParseResult>` and `fn store(key, &ParseResult)` using a
  compact binary serde format; corrupt/unreadable entries return `None` (treated as miss)
- M22-E1-T4. Unit tests: identical inputs → identical key; arg reorder → identical key;
  changed `-D` → different key; changed source → different key; changed include content →
  different key; round-trip store/load equals original

Expected touch points:

- new `indexer/src/parse_cache.rs`
- `indexer/src/clang_parser.rs` (expose `parser_version_tag`)
- `indexer/Cargo.toml` (binary serde dep if not already present)

Acceptance:

- key function is deterministic and sensitive to every input class
- store/load round-trips a `ParseResult` losslessly

### M22-E2. Wire Cache Gate Into the C++ Adapter

Status:

- Not started

Goal:

- serve unchanged TUs from cache and skip the libclang parse + permit

Design:

Insert the gate in `CppLanguageAdapter::parse_file()` after `args` are built
(`indexing.rs:157`) and **before** `acquire_cpp_parse_permit()` (`indexing.rs:176`):

```
build args
if cache enabled and not macro-sensitive:
    direct_includes = cheap include scan of `source`   // or reuse prior record
    key = parse_cache::compute_key(args, source, direct_includes)
    if let Some(cached) = parse_cache::load(key): return Ok(cached)   // skip permit + clang
let _permit = acquire_cpp_parse_permit()
let result = clang_parser::parse_cpp_file(...)
if cache enabled and not macro-sensitive: parse_cache::store(key, &result)
return result
```

- the cheap include scan only needs the TU's own `#include "..."`/`<...>` lines resolved
  against `args` include dirs — far cheaper than a libclang parse; reuse logic already present
  in `clang_parser`/`parser` include extraction if available
- macro-sensitive bypass: if the file is known macro-sensitive (the `files` table /
  `FileRiskSignals` carry `macro_sensitivity = "High"`), skip the cache entirely and always
  parse (conservative, matches MS16's macro fallback philosophy)

Implementation tasks:

- M22-E2-T1. Add a cheap direct-include resolver (or reuse existing include extraction) to
  produce `(path, content_hash)` pairs for the TU's direct includes
- M22-E2-T2. Add the cache gate to `CppLanguageAdapter::parse_file()` before the permit
- M22-E2-T3. Store the fresh `ParseResult` on a miss (after a successful parse)
- M22-E2-T4. Macro-sensitive bypass wired from existing risk signals
- M22-E2-T5. Tests: second parse of an unchanged TU returns the cached result without
  invoking libclang (assert via a parse-count probe or by populating cache then corrupting a
  sentinel that a real parse would touch); changing a `-D` or the source forces a re-parse

Expected touch points:

- `indexer/src/indexing.rs`
- `indexer/src/clang_parser.rs` or `indexer/src/parser.rs` (include extraction reuse)

Acceptance:

- unchanged TU → cache hit, no permit acquired, no libclang invocation
- any input change → miss → real parse → fresh store
- macro-sensitive TU → always parsed, never cached

### M22-E3. Eviction and Operational Controls

Status:

- Not started

Goal:

- bound disk usage and give operators control without risking correctness

Design:

- generation-based layout: cache entries live under `<gen>/`; on a full rebuild, bump the
  generation and lazily delete stale generations (reuse the generation/`current-db.json`
  pattern already in `storage.rs`)
- env controls:
  - `CODEATLAS_PARSE_CACHE=0` disables the cache (always parse) — escape hatch
  - `CODEATLAS_PARSE_CACHE_MAX_MB` soft cap; when exceeded, evict oldest entries
- a `parser_version_tag` mismatch makes all old-format keys naturally miss (no explicit purge needed)

Implementation tasks:

- M22-E3-T1. Generation directory selection + stale-generation cleanup
- M22-E3-T2. `CODEATLAS_PARSE_CACHE` disable switch (default on)
- M22-E3-T3. `CODEATLAS_PARSE_CACHE_MAX_MB` size cap with oldest-first eviction
- M22-E3-T4. Startup log line: cache enabled?, generation, current size
- M22-E3-T5. Tests: disable switch forces parse; size cap evicts; version bump invalidates

Expected touch points:

- `indexer/src/parse_cache.rs`
- `indexer/src/main.rs` (startup wiring + log)

Acceptance:

- disabling the cache reproduces pre-MS22 behavior exactly
- cache size stays under the configured cap
- a parser version bump transparently invalidates old entries

### M22-E4. Validation and Release Readiness

Status:

- Not started

Goal:

- prove correctness equivalence and measure the speed/RSS win on a real project

Implementation tasks:

- M22-E4-T1. `cargo test` + `cargo build --release`
- M22-E4-T2. Correctness equivalence: full index with cache OFF vs cache ON on a real project
  (F:\dev\opencv) → identical Symbols / Calls / References / Propagation counts and identical
  DB content for a sampled set of files
- M22-E4-T3. Incremental win: full index (populate cache), edit one header signature, run
  incremental → measure wall-clock and libclang invocation count vs cache OFF
- M22-E4-T4. Restart win: re-run full index on an unchanged tree → near-100% hit rate, large
  wall-clock reduction
- M22-E4-T5. Record hit-rate, elapsed, and peak RSS deltas in this doc

Expected touch points:

- test modules in `indexer/src/`
- `dev_docs/Milestone22_TranslationUnitParseCache.md`

Acceptance:

- cache ON vs OFF produce byte-identical DB content (memoization is pure)
- incremental header-change re-index shows reduced parse count + wall-clock
- unchanged-tree restart shows near-100% hit rate
- peak RSS during cache-hit-heavy runs is lower (fewer concurrent libclang TUs)

---

## 5. Task Breakdown By File

### new `indexer/src/parse_cache.rs`

- `ParseCacheKey`, `compute_key()` (M22-E1-T1, T2)
- `load()` / `store()` with binary serde (M22-E1-T3)
- generation selection, size cap, disable switch (M22-E3-T1, T2, T3)
- key + round-trip + eviction tests (M22-E1-T4, M22-E3-T5)

### `indexer/src/indexing.rs`

- cache gate before `acquire_cpp_parse_permit()` in `CppLanguageAdapter::parse_file()`
  (M22-E2-T2, T3)
- macro-sensitive bypass (M22-E2-T4)

### `indexer/src/clang_parser.rs` (or `parser.rs`)

- expose `parser_version_tag` (M22-E1-T2)
- reuse/expose direct-include extraction (M22-E2-T1)

### `indexer/src/main.rs`

- startup wiring + cache status log (M22-E3-T4)

### `dev_docs/Milestone22_TranslationUnitParseCache.md`

- completion evidence (M22-E4-T5)

---

## 6. Risks

### Risk 1. Transitive include changes not captured by direct-include hashing

- Why: keying on direct includes only means a change two headers deep might not change the key.
- Mitigation: (a) each dependent TU is itself keyed on its own source + direct includes, so a
  changed deep header changes the key of whichever TU directly includes it, and incremental
  planning (MS16) re-parses that includer — refreshing the chain; (b) macro-sensitive files
  bypass the cache entirely; (c) `CODEATLAS_PARSE_CACHE=0` is an escape hatch. Document that the
  cache assumes the incremental planner re-parses files whose direct inputs changed. Full
  transitive hashing is explicitly deferred (cost-prohibitive, Kythe captures it only because
  it has the full compilation unit in hand at extraction time).

### Risk 2. Stale cache producing wrong results

- Why: a correctness bug here is silent and corrupts the index.
- Mitigation: cache is pure memoization keyed on all inputs; `parser_version_tag` invalidates
  on any extraction-shape change; E4-T2 asserts byte-identical DB content cache ON vs OFF as a
  release gate.

### Risk 3. Serialization overhead exceeds parse savings for tiny TUs

- Why: very small files may parse faster than deserialize.
- Mitigation: libclang parse (200–500 MB RSS, permit-gated) dominates even for small TUs;
  if measured otherwise, add a min-source-size threshold below which caching is skipped.

### Risk 4. Disk growth on large projects

- Why: 300k TUs × serialized ParseResult could be large.
- Mitigation: `CODEATLAS_PARSE_CACHE_MAX_MB` cap + oldest-first eviction + generation cleanup;
  compact binary format; cache is disposable (miss just re-parses).

---

## 7. Definition of Done

1. `parse_cache` computes a deterministic, input-sensitive CAS key and round-trips `ParseResult`
2. `CppLanguageAdapter::parse_file()` serves unchanged C++ TUs from cache, skipping libclang
   and the parse permit
3. macro-sensitive TUs always parse and are never cached
4. cache ON vs OFF produce byte-identical DB content (release gate)
5. eviction, size cap, disable switch, and version-tag invalidation all work
6. all suites pass; release builds succeed
7. measured incremental + restart wall-clock and peak-RSS improvements recorded

---

## 8. Suggested First Implementation Slice

1. implement `parse_cache::compute_key()` + `store()`/`load()` (M22-E1)
2. add the gate in `CppLanguageAdapter::parse_file()` guarded behind
   `CODEATLAS_PARSE_CACHE` (default on), macro-sensitive bypass included
3. run E4-T2 correctness equivalence (cache ON vs OFF, identical DB) on a small project

Why first: this slice delivers the core win (skip re-parse on unchanged TUs) and immediately
establishes the correctness-equivalence gate before adding eviction/operational polish.
