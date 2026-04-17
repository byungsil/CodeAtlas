# Real Project Evaluation: nlohmann/json

## Overview

This note captures a real-project validation run of CodeAtlas against the `nlohmann/json` workspace at `E:\Dev\nlohmann`.

Goal:

- verify that exact lookup works on a real C++ project
- inspect how heuristic lookup behaves with realistic repository noise
- identify product improvements needed for large-project onboarding

## Workspace

- Target: `E:\Dev\nlohmann`
- Output database: `E:\Dev\nlohmann\.codeatlas`
- Indexer: local debug build of `codeatlas-indexer`
- Query surface used for validation:
  - MCP `lookup_symbol`
  - MCP `lookup_function`

## Baseline Indexing Result

Initial full indexing completed successfully.

- files: `481`
- symbols: `1496`
- call edges: `4157`

The largest signal problem was not indexing failure, but symbol pollution from non-library directories.

Directory symbol distribution before filtering:

- `tests`: `1166`
- `include`: `241`
- `single_include`: `51`
- `docs`: `36`
- `tools`: `1`
- `cmake`: `1`

This meant heuristic lookup was dominated by test fixtures and ABI-compatibility copies rather than the main library surface.

## Baseline Query Findings

### Exact Lookup

Exact lookup behaved well on the real project.

Example:

- query: `lookup_symbol({ qualifiedName: "binary_reader::parse_bson_array" })`
- result:
  - `lookupMode = "exact"`
  - `confidence = "exact"`
  - `matchReasons = ["exact_qualified_name_match"]`

Returned relationships were also structurally sensible:

- caller: `binary_reader::parse_bson_element_internal`
- callee: `binary_reader::parse_bson_element_list`

### Heuristic Lookup

Heuristic lookup correctly surfaced ambiguity, but the candidate set was too noisy before filtering.

Example:

- query: `lookup_function({ name: "parse_error" })`
- result:
  - `confidence = "ambiguous"`
  - `matchReasons = ["ambiguous_top_score"]`
  - `ambiguity.candidateCount = 16`

The selected representative came from `tests/abi/include/...`, which is technically valid but not what an agent usually wants when asking about the library itself.

This is a good sign for Milestone 1 correctness, but a weak default experience for real repositories.

## Filtering With `.codeatlasignore`

To focus the index on the actual library, the following ignore file was added to the target workspace:

```gitignore
# Focus the index on the library implementation and public headers
^tests/
^docs/
^single_include/
^tools/
^cmake/
```

After reindexing:

- files: `47`
- symbols: `247`
- call edges: `978`

Ignored-file cleanup removed `434` previously indexed files from the database.

## Post-Filter Query Findings

### Exact Lookup

Exact lookup remained stable after filtering.

Example:

- query: `lookup_symbol({ qualifiedName: "binary_reader::parse_bson_array" })`
- result:
  - `lookupMode = "exact"`
  - `confidence = "exact"`
  - `matchReasons = ["exact_qualified_name_match"]`

### Heuristic Lookup

Heuristic lookup became materially more useful after filtering.

`parse_error`:

- before filtering: `ambiguity.candidateCount = 16`
- after filtering: `ambiguity.candidateCount = 4`

The selected symbol moved into the actual library tree:

- `include/nlohmann/detail/input/json_sax.hpp`
- `json_sax_dom_callback_parser::parse_error`

Another positive example:

- query: `lookup_function({ name: "dump" })`
- result:
  - `confidence = "high_confidence_heuristic"`
  - `qualifiedName = "dump"`
  - `filePath = "include/nlohmann/json.hpp"`

Returned callers were sensible for library usage:

- `operator<<`
- `patch_inplace`
- `std::string to_string`

## Product Conclusions

### What Worked

- exact lookup is already strong enough for real-project use
- ambiguity surfacing is doing the right thing
- heuristic lookup improves significantly when the workspace is curated
- call relationships remain useful on a real library codebase

### What This Exposed

- repository composition has a huge effect on heuristic quality
- large C++ repositories need an explicit indexing scope strategy
- test trees, vendored copies, generated outputs, and docs can drown the useful symbol set

### Recommended Product Follow-ups

1. Treat `.codeatlasignore` as a first-class onboarding tool.
2. Provide recommended ignore presets for common large-repo layouts:
   - `tests/`
   - `docs/`
   - generated code
   - vendored mirrors
   - build output
3. Add a quick post-index summary highlighting symbol distribution by top-level directory.
4. Consider a future query option that prefers production/include paths over tests when confidence is otherwise tied.

## Practical Guidance

For real C++ projects, the recommended workflow is:

1. Index the repository once.
2. Inspect high-volume top-level directories.
3. Add `.codeatlasignore` to remove irrelevant trees.
4. Reindex.
5. Use `search_symbols` for discovery.
6. Use `lookup_symbol` for deterministic follow-up.

## Bottom Line

CodeAtlas passed this real-project check.

The strongest result is that exact lookup already works well on a production C++ codebase. The clearest product lesson is that heuristic quality depends heavily on index scope, so workspace curation should become part of the standard CodeAtlas workflow rather than an optional afterthought.
