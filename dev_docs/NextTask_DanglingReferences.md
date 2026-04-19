# Next Task: Reduce Dangling `symbol_references`

Date: 2026-04-20

## 1. Why This Matters

The current index database is structurally healthy:

- `PRAGMA integrity_check = ok`
- `dangling_calls = 0`
- `dangling_propagation = 0`
- MCP tool tests and live smoke checks passed

The remaining quality issue is confined to `symbol_references`.

Observed on `E:\Dev\CodeAtlas\tmp\opencv_4k\.codeatlas\index.db`:

- `references = 6208`
- `dangling_references = 1055`

Breakdown:

- `moduleImport = 891`
- `inheritanceMention = 164`

This does not currently look like database corruption or a memory-optimization regression. It looks like reference-modeling debt that should be cleaned up before we rely more heavily on generalized reference queries.

## 2. Current Diagnosis

### 2.1 External Python Imports

Most dangling references are `moduleImport` entries such as:

- `python::os`
- `python::json`
- `python::numpy`
- `python::argparse`

These are not symbols defined inside the indexed workspace, so a direct `target_symbol_id -> symbols.id` join will naturally fail.

Interpretation:

- This is expected for external or standard-library modules.
- These references are still semantically useful.
- They should not be treated the same as broken internal references.

### 2.2 Inheritance Targets Normalized To Constructor-Like Symbols

The remaining dangling references are `inheritanceMention` entries shaped like:

- `ThreadPoolProvider::ThreadPoolProvider`

This suggests the inheritance reference normalizer is sometimes selecting a constructor-like callable identity instead of the type/class symbol that should own the inheritance edge.

Interpretation:

- The source side is likely correct.
- The target-side normalization is too permissive for inheritance edges.
- This is a true modeling bug and should be fixed.

## 3. Desired End State

We want `symbol_references` to separate three cases cleanly:

1. Valid internal references
- `target_symbol_id` resolves to a workspace symbol.

2. Valid external references
- the reference intentionally points outside the workspace.
- it remains queryable, but is not flagged as dangling.

3. Invalid references
- normalization selected the wrong identity.
- these should be removed or corrected.

## 4. Proposed Fix Plan

### Step 1. Classify External Module Imports Explicitly

For `moduleImport` references, add a way to distinguish:

- internal module symbol resolved in workspace
- external module import with no in-workspace symbol

Recommended implementation direction:

- extend `symbol_references` with an `is_external_target` boolean, default `0`
- optionally add `target_qualified_name` for references that do not resolve to an internal symbol

Expected rule:

- if a Python import resolves to no workspace symbol, persist it as:
  - `target_symbol_id = NULL` or empty-by-convention only if schema requires transition handling
  - `target_qualified_name = python::<module>`
  - `is_external_target = 1`

Notes:

- prefer `NULL` over fake internal IDs for unresolved external targets
- do not count `is_external_target = 1` rows as dangling in validation queries

### Step 2. Tighten Inheritance Target Normalization

For `inheritanceMention`, the target must resolve to a type-like symbol only:

- `class`
- `struct`
- possibly `interface` equivalent if represented separately later

Recommended implementation direction:

- audit the inheritance reference extraction path in the parser and reference normalizer
- when normalizing inheritance targets, reject callable symbols such as:
  - constructors
  - methods
  - free functions
- if both a callable and a type share a name, prefer the type symbol unconditionally for `inheritanceMention`

Expected rule:

- `Base : public Foo` must target `Foo` the type symbol, never `Foo::Foo`

### Step 3. Update Store/Server Semantics

After the indexer schema/model change, update query semantics so MCP and HTTP consumers behave predictably.

Recommended behavior:

- internal references remain unchanged
- external references may still be returned by `find_references`
- external references should be visibly marked in payloads, for example:
  - `isExternalTarget: true`
  - `targetQualifiedName`
- server-side impact/ranking logic should ignore external targets when an operation requires an internal symbol graph

### Step 4. Refresh Validation Rules

Current DB checks should be split into:

- `dangling_internal_references`
- `external_references`

Target acceptance:

- `dangling_internal_references = 0`
- external module imports are preserved and no longer treated as invalid
- inheritance mentions target only type symbols

## 5. Suggested Code Touch Points

Likely files to inspect first:

- `indexer/src/parser.rs`
- `indexer/src/resolver.rs`
- `indexer/src/storage.rs`
- `server/src/storage/sqlite-store.ts`
- `server/src/models/responses.ts`
- `server/src/mcp-runtime.ts`

## 6. Acceptance Criteria

The task is complete when all of the following are true:

1. Rebuilt OpenCV 4k DB reports:
- `dangling_calls = 0`
- `dangling_propagation = 0`
- `dangling_internal_references = 0`

2. `moduleImport` rows for external libraries are preserved and explicitly marked as external.

3. `inheritanceMention` rows no longer point at constructor-like targets.

4. MCP tests still pass.

5. At least one real-DB smoke check confirms:
- `find_references` still returns useful module import references
- exact symbol hierarchy queries are unaffected

## 7. Recommended Execution Order

1. Add schema/model support for external references.
2. Fix inheritance normalization to force type targets.
3. Update DB validation queries and example inspection scripts.
4. Update server/store payloads for external-reference visibility.
5. Rebuild `tmp/opencv_4k` and re-run MCP smoke checks.

## 8. Non-Goals For This Task

- full third-party package resolution beyond workspace scope
- semantic Python environment introspection
- redesign of generalized reference categories beyond what is needed to remove false dangling rows

## 9. Handoff Summary

Today’s performance work appears stable. The remaining issue is not memory pressure or DB corruption. The next task should focus narrowly on making `symbol_references` distinguish:

- internal exact targets
- external-but-valid targets
- truly invalid normalized targets

That should eliminate the misleading dangling-reference count while improving reference query quality.
