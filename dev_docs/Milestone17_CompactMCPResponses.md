# Milestone 17. Compact MCP Responses

Status:

- Complete

## 1. Objective

Reduce MCP tool response sizes by 75-80% for large-result queries through file-grouped compact formatting, enabling AI agents to consume reference and caller data without excessive token cost.

This milestone focuses on:

- upgrading the existing `compact: true` mode on `find_references` from per-record field stripping (36% reduction) to file-grouped formatting (79% reduction)
- extending `compact` support to all remaining large-result MCP tools: `find_callers`, `search_symbols`, `impact_analysis`, `list_class_members`, `list_namespace_symbols`
- using clear, self-explanatory field names (`file`, `refs`, `symbol`, `line`) so agents need zero schema inference

Success outcome:

- `find_references` compact response for 98 references drops from 42KB to ~9KB
- `find_references` compact response for 641 references drops from ~95KB to ~16KB
- all large-result MCP tools support `compact: true` with consistent formatting
- existing `compact: false` behavior is unchanged
- all existing tests pass, new compact tests are added

Positioning note:

- MS15 added macro-sensitive recovery and enum flag coverage
- MS16 made incremental indexing scale-aware for large projects
- MS17 addresses the remaining token-cost problem when agents consume query results

Scope note:

- this milestone changes only server-side response formatting
- no indexer changes, no DB schema changes
- existing non-compact response shapes are preserved exactly

---

## 2. Applicability Review

Real-world measurement from `F:\dev\dev_future\client` project (35,000+ files):

`find_references(qualifiedName="Gameplay::ShotFlags")` returned 98 typeUsage references across 33 unique files:

| format | size | reduction |
|---|---|---|
| full response | 42,468 bytes | baseline |
| current compact (field strip) | 27,389 bytes | 36% |
| **file-grouped compact (symbol/line)** | **~9,000 bytes** | **79%** |

With `includeEnumValueUsage=true` (641 references), full response was ~95KB. File-grouped compact would reduce to ~16KB.

Root cause of bloat:

- `filePath` strings average 80 characters, repeated per reference (33 unique files × 3 refs each → 98 × 80 = 7,840 bytes of duplicated paths)
- each reference carries `sourceSymbolId`, `targetSymbolId`, `confidence`, `matchReasons` — unnecessary for agent navigation

Current `compact: true` tools:

| tool | current compact behavior |
|---|---|
| `find_references` | strips confidence/matchReasons per record (36% reduction) |
| `list_file_symbols` | strips filePath/language/subsystem from symbols |
| `get_callgraph` | strips type from root node symbol |

Tools without `compact` support:

| tool | response type | large arrays |
|---|---|---|
| `find_callers` | `CallerQueryResponse` | `callers: CallReference[]` |
| `search_symbols` | `SearchResponse` | `results: Symbol[]` |
| `impact_analysis` | `ImpactAnalysisResponse` | `directCallers`, `directCallees`, `directReferences` |
| `list_class_members` | `ClassMembersOverviewResponse` | `members: Symbol[]` |
| `list_namespace_symbols` | `NamespaceSymbolsResponse` | `symbols: Symbol[]` |

---

## 3. Recommended Order

1. M17-E1. File-Grouped Compact for find_references
2. M17-E2. Compact Mode for find_callers and search_symbols
3. M17-E3. Compact Mode for impact_analysis, list_class_members, list_namespace_symbols
4. M17-E4. Validation and Release Readiness

Why this order:

- E1 delivers the highest-value change (find_references is the most token-heavy tool) and establishes the file-grouped pattern
- E2 applies the same pattern to the next most commonly used tools
- E3 covers the remaining tools using the proven pattern
- E4 validates end-to-end on real projects

Execution rule:

- finish each Epic to the point of measurable acceptance before starting broad polish on the next one
- allow small support changes across Epics only when an earlier Epic cannot be validated without them

---

## 4. Epic Breakdown

### M17-E1. File-Grouped Compact for find_references

Status:

- Complete

Goal:

- upgrade `find_references` compact mode from per-record field stripping to file-grouped formatting with self-explanatory field names

Problem being solved:

- current compact mode reduces 42KB to 27KB (36%) — insufficient for agent token budgets
- `filePath` strings are duplicated across references to the same file

Design:

Current compact output per reference:
```json
{ "sourceSymbolId": "...", "sourceQualifiedName": "...", "targetSymbolId": "...", "targetQualifiedName": "...", "category": "typeUsage", "filePath": "gameplay/.../ballhandler.cpp", "line": 12758 }
```

New file-grouped compact output:
```json
{
  "responseMode": "compact",
  "symbol": { "qualifiedName": "Gameplay::ShotFlags", "type": "enum" },
  "window": { "returnedCount": 98, "totalCount": 98, "truncated": false },
  "groupedBySubsystem": [{ "key": "gameplay", "count": 79 }, { "key": "audio", "count": 4 }],
  "fileGroups": [
    {
      "file": "gameplay/.../ballhandler.cpp",
      "refs": [
        { "symbol": "BallHandler::AddShotFlags", "line": 12758 },
        { "symbol": "BallHandler::RemoveShotFlags", "line": 12763 }
      ]
    }
  ]
}
```

Field name rationale:
- `file` instead of `filePath` — context makes meaning unambiguous, saves 4 chars per group
- `symbol` instead of `sourceQualifiedName` — natural language, saves 15 chars per ref
- `line` unchanged — already minimal
- `category` removed from individual refs — available as top-level filter; all refs in a response share the same or no category filter

Implementation tasks:

- M17-E1-T1. Add `FileGroupedRef` and `FileGroup` types to `models/responses.ts`
- M17-E1-T2. Add `CompactFileGroupedReferenceResponse` type to `models/responses.ts`
- M17-E1-T3. Add `toFileGroupedReferences()` function to `compact-responses.ts`
- M17-E1-T4. Add `toFileGroupedReferenceQueryResponse()` function to `compact-responses.ts`
- M17-E1-T5. Update `find_references` handler in `mcp-runtime.ts` to use file-grouped compact
- M17-E1-T6. Update existing compact test in `mcp.test.ts`
- M17-E1-T7. Add file-grouped compact test in `endpoints.test.ts`

Expected touch points:

- `server/src/models/responses.ts`
- `server/src/compact-responses.ts`
- `server/src/mcp-runtime.ts`
- `server/src/__tests__/mcp.test.ts`
- `server/src/__tests__/endpoints.test.ts`

Acceptance:

- `find_references(compact=true)` returns `fileGroups` instead of flat `references` array
- response size for the ShotFlags case drops from 42KB to ~9KB
- `find_references(compact=false)` behavior is unchanged
- existing compact test updated to validate new structure
- `npm test -- --runInBand` passes
- `npm run build` passes

### M17-E2. Compact Mode for find_callers and search_symbols

Status:

- Complete

Goal:

- add `compact: true` parameter to `find_callers` and `search_symbols` with file-grouped formatting

Problem being solved:

- `find_callers` returns full `CallReference[]` with confidence, matchReasons, ambiguity, resolutionKind, provenanceKind per caller — most fields unnecessary for agent navigation
- `search_symbols` returns full `Symbol[]` with 25+ fields per symbol

Design for find_callers compact:

```json
{
  "responseMode": "compact",
  "symbol": { "qualifiedName": "...", "type": "..." },
  "window": { ... },
  "groupedBySubsystem": [...],
  "fileGroups": [
    {
      "file": "gameplay/.../juego.cpp",
      "refs": [
        { "symbol": "Juego::ProcessNormalShot", "line": 3173 }
      ]
    }
  ]
}
```

Design for search_symbols compact:

```json
{
  "responseMode": "compact",
  "query": "ShotFlags",
  "window": { ... },
  "groupedByLanguage": [...],
  "results": [
    { "id": "...", "name": "ShotFlags", "qualifiedName": "Gameplay::ShotFlags", "type": "enum", "file": "gameplay/.../util.h", "line": 12 }
  ]
}
```

Implementation tasks:

- M17-E2-T1. Add `CompactCallerQueryResponse` type to `models/responses.ts`
- M17-E2-T2. Add `CompactSearchResponse` type to `models/responses.ts`
- M17-E2-T3. Add `toCompactCallerQueryResponse()` to `compact-responses.ts`
- M17-E2-T4. Add `toCompactSearchResponse()` to `compact-responses.ts`
- M17-E2-T5. Add `compact` parameter to `find_callers` in `mcp-runtime.ts` (line ~1435)
- M17-E2-T6. Add `compact` parameter to `search_symbols` in `mcp-runtime.ts` (line ~1985)
- M17-E2-T7. Add compact tests for both tools in `mcp.test.ts`

Expected touch points:

- `server/src/models/responses.ts`
- `server/src/compact-responses.ts`
- `server/src/mcp-runtime.ts`
- `server/src/__tests__/mcp.test.ts`

Acceptance:

- `find_callers(name="imread", compact=true)` returns file-grouped format
- `search_symbols(query="ShotFlags", compact=true)` returns compact symbol list
- both tools unchanged when `compact` is omitted or false
- `npm test -- --runInBand` passes
- `npm run build` passes

### M17-E3. Compact Mode for impact_analysis, list_class_members, list_namespace_symbols

Status:

- Complete

Goal:

- extend compact support to all remaining large-result tools

Design for impact_analysis compact:

```json
{
  "responseMode": "compact",
  "symbol": { "qualifiedName": "...", "type": "..." },
  "maxDepth": 2,
  "totalAffectedSymbols": 45,
  "totalAffectedFiles": 12,
  "affectedSubsystems": [...],
  "callerFileGroups": [{ "file": "...", "refs": [...] }],
  "calleeFileGroups": [{ "file": "...", "refs": [...] }],
  "referenceFileGroups": [{ "file": "...", "refs": [...] }],
  "topAffectedFiles": [...],
  "suggestedFollowUpQueries": [...]
}
```

Design for list_class_members / list_namespace_symbols compact:

Reuse existing `toCompactFileSymbol()` pattern — strip to `{ id, name, qualifiedName, type, line, endLine }`.

Implementation tasks:

- M17-E3-T1. Add `CompactImpactAnalysisResponse` type to `models/responses.ts`
- M17-E3-T2. Add `toCompactImpactAnalysisResponse()` to `compact-responses.ts`
- M17-E3-T3. Add `compact` parameter to `impact_analysis` in `mcp-runtime.ts` (line ~1736)
- M17-E3-T4. Add `compact` parameter to `list_class_members` in `mcp-runtime.ts` (line ~1835)
- M17-E3-T5. Add `compact` parameter to `list_namespace_symbols` in `mcp-runtime.ts` (line ~1807)
- M17-E3-T6. Add `CompactClassMembersResponse` and `CompactNamespaceSymbolsResponse` types
- M17-E3-T7. Add compact formatter functions for class members and namespace symbols
- M17-E3-T8. Add compact tests for all three tools in `mcp.test.ts`

Expected touch points:

- `server/src/models/responses.ts`
- `server/src/compact-responses.ts`
- `server/src/mcp-runtime.ts`
- `server/src/__tests__/mcp.test.ts`

Acceptance:

- all three tools support `compact: true`
- `impact_analysis` compact uses `callerFileGroups` / `calleeFileGroups` / `referenceFileGroups`
- `list_class_members` and `list_namespace_symbols` compact use stripped symbol arrays
- `npm test -- --runInBand` passes
- `npm run build` passes

### M17-E4. Validation and Release Readiness

Status:

- Complete

Goal:

- validate all compact improvements against real project data and document results

Implementation tasks:

- M17-E4-T1. Run full test suites (`npm test -- --runInBand`, `npm run build`)
- M17-E4-T2. Run MCP tool tests against opencv DB (`find_references(compact=true)`, `find_callers(compact=true)`)
- M17-E4-T3. Measure actual response sizes before and after compact for representative queries
- M17-E4-T4. Update milestone documentation with completion evidence

Expected touch points:

- `dev_docs/Milestone17_CompactMCPResponses.md`

Acceptance:

- all tests pass
- measurable size reduction documented
- no regressions in non-compact responses

---

## 5. Detailed Execution Plan

### Phase 1. Define File-Grouped Types

Primary outcome:

- type system supports the new compact response shapes

Tasks:

1. add to `server/src/models/responses.ts`:

```typescript
export interface FileGroupedRef {
  symbol: string;
  line: number;
}

export interface FileGroup {
  file: string;
  refs: FileGroupedRef[];
}

export interface CompactFileGroupedReferenceResponse extends ConfidenceMetadata, ReliabilityMetadata {
  responseMode: "compact";
  lookupMode: LookupMode;
  symbol: Symbol;
  window: ResultWindow;
  fileGroups: FileGroup[];
  totalCount: number;
  truncated: boolean;
  category?: ReferenceCategory;
  groupedBySubsystem?: MetadataGroupSummary[];
  groupedByModule?: MetadataGroupSummary[];
  groupedByLanguage?: MetadataGroupSummary[];
}
```

Why first:

- formatter functions and handler code depend on these types

Completion gate:

- `npm run build` passes with new types

### Phase 2. Implement File-Grouped Reference Formatter

Primary outcome:

- `compact-responses.ts` can transform a flat reference array into file-grouped format

Tasks:

1. add `toFileGroupedReferences()` to `server/src/compact-responses.ts`:

```typescript
export function toFileGroupedReferences(references: ResolvedReference[]): FileGroup[] {
  const groups = new Map<string, FileGroupedRef[]>();
  for (const ref of references) {
    const existing = groups.get(ref.filePath) ?? [];
    existing.push({ symbol: ref.sourceQualifiedName, line: ref.line });
    groups.set(ref.filePath, existing);
  }
  return Array.from(groups.entries())
    .map(([file, refs]) => ({ file, refs: refs.sort((a, b) => a.line - b.line) }))
    .sort((a, b) => a.file.localeCompare(b.file));
}
```

2. add `toFileGroupedReferenceQueryResponse()`:

```typescript
export function toFileGroupedReferenceQueryResponse(
  response: ReferenceQueryResponse,
  references: ResolvedReference[],
): CompactFileGroupedReferenceResponse {
  return {
    responseMode: "compact" as const,
    lookupMode: response.lookupMode,
    symbol: response.symbol,
    confidence: response.confidence,
    matchReasons: response.matchReasons,
    reliability: response.reliability,
    window: response.window,
    fileGroups: toFileGroupedReferences(references),
    totalCount: response.totalCount,
    truncated: response.truncated,
    ...(response.category ? { category: response.category } : {}),
    ...(response.groupedBySubsystem ? { groupedBySubsystem: response.groupedBySubsystem } : {}),
    ...(response.groupedByModule ? { groupedByModule: response.groupedByModule } : {}),
    ...(response.groupedByLanguage ? { groupedByLanguage: response.groupedByLanguage } : {}),
  };
}
```

Why second:

- types must exist before formatter functions

Completion gate:

- `npm run build` passes

### Phase 3. Wire find_references Compact

Primary outcome:

- `find_references(compact=true)` returns file-grouped response

Tasks:

1. update `mcp-runtime.ts` at the compact branch (line ~1621):

Replace:
```typescript
const response: ReferenceQueryResponse | CompactReferenceQueryResponse = compact
  ? toCompactReferenceQueryResponse(payload, buildCompactResolvedReferences(references.results))
  : payload;
```

With:
```typescript
const response: ReferenceQueryResponse | CompactFileGroupedReferenceResponse = compact
  ? toFileGroupedReferenceQueryResponse(payload, references.results)
  : payload;
```

2. update import in `mcp-runtime.ts` to include `toFileGroupedReferenceQueryResponse`

Why third:

- formatter and types must exist before the handler can use them

Completion gate:

- `find_references(compact=true)` returns `fileGroups` array instead of `references` array

### Phase 4. Add Compact to find_callers and search_symbols

Primary outcome:

- both tools support `compact: true` with file-grouped or stripped formatting

Tasks:

1. add types to `models/responses.ts`:

```typescript
export interface CompactCallerQueryResponse extends HeuristicSelectionMetadata, ReliabilityMetadata {
  responseMode: "compact";
  lookupMode: LookupMode;
  symbol: Symbol;
  confidence: ConfidenceLevel;
  matchReasons: MatchReason[];
  window: ResultWindow;
  fileGroups: FileGroup[];
  totalCount: number;
  truncated: boolean;
  groupedBySubsystem?: MetadataGroupSummary[];
  groupedByModule?: MetadataGroupSummary[];
  groupedByLanguage?: MetadataGroupSummary[];
}

export interface CompactSearchResponse {
  responseMode: "compact";
  query: string;
  window: ResultWindow;
  results: CompactFileSymbol[];
  totalCount: number;
  truncated: boolean;
  groupedByLanguage?: MetadataGroupSummary[];
}
```

2. add formatters to `compact-responses.ts`:

```typescript
export function toCompactCallerQueryResponse(response: CallerQueryResponse): CompactCallerQueryResponse
export function toCompactSearchResponse(response: SearchResponse): CompactSearchResponse
```

- `toCompactCallerQueryResponse`: converts `callers: CallReference[]` to `fileGroups: FileGroup[]` using the same file-grouping logic
- `toCompactSearchResponse`: converts `results: Symbol[]` to `results: CompactFileSymbol[]` (reuse existing `toCompactFileSymbol`)

3. add `compact` parameter to both tools in `mcp-runtime.ts`:
   - `find_callers` (line ~1435): add `compact: z.boolean().optional().describe("When true, return file-grouped compact format")`
   - `search_symbols` (line ~1985): add `compact: z.boolean().optional().describe("When true, return compact symbol records")`

4. add compact branch in each handler

Why fourth:

- the file-grouped pattern from Phase 2-3 is proven before extending it

Completion gate:

- both tools respond with compact format when `compact=true`

### Phase 5. Add Compact to impact_analysis, list_class_members, list_namespace_symbols

Primary outcome:

- all remaining large-result tools support compact

Tasks:

1. `impact_analysis` compact:
   - new type `CompactImpactAnalysisResponse` with `callerFileGroups`, `calleeFileGroups`, `referenceFileGroups`
   - formatter converts three arrays to file-grouped form
   - add `compact` parameter at line ~1736

2. `list_class_members` compact:
   - new type `CompactClassMembersResponse` with `members: CompactFileSymbol[]`
   - reuse `toCompactFileSymbol()` (already exists)
   - add `compact` parameter at line ~1835

3. `list_namespace_symbols` compact:
   - new type `CompactNamespaceSymbolsResponse` with `symbols: CompactFileSymbol[]`
   - reuse `toCompactFileSymbol()` (already exists)
   - add `compact` parameter at line ~1807

Why fifth:

- lower-priority tools use the proven patterns from earlier phases

Completion gate:

- all five tools respond with compact format when `compact=true`

### Phase 6. Tests and Validation

Primary outcome:

- all new compact behavior is covered by tests

Tasks:

1. update existing test `find_references supports compact mode` in `mcp.test.ts` (line ~220):
   - assert `payload.fileGroups` is an array
   - assert `payload.fileGroups[0].file` is a string
   - assert `payload.fileGroups[0].refs[0].symbol` is a string
   - assert `payload.fileGroups[0].refs[0].line` is a number
   - assert `payload.references` is undefined (no longer present in compact)

2. add new tests in `mcp.test.ts`:
   - `find_callers supports compact mode`
   - `search_symbols supports compact mode`
   - `impact_analysis supports compact mode`
   - `list_class_members supports compact mode`
   - `list_namespace_symbols supports compact mode`

3. add HTTP endpoint tests in `endpoints.test.ts`:
   - `/references?qualifiedName=...&compact=true` returns file-grouped format

4. run full test suite: `npm test -- --runInBand`
5. run build: `npm run build`
6. run MCP smoke against opencv DB to measure actual size reduction

Why last:

- tests should validate the final integrated behavior

Completion gate:

- all tests pass
- measurable size reduction documented

---

## 6. Task Breakdown By Epic

### M17-E1. File-Grouped Compact for find_references

#### M17-E1-T1. Add FileGroupedRef and FileGroup types

Deliverable:

- `FileGroupedRef` and `FileGroup` interfaces in `server/src/models/responses.ts`

Implementation detail:

- `FileGroupedRef`: `{ symbol: string; line: number }`
- `FileGroup`: `{ file: string; refs: FileGroupedRef[] }`
- add after the existing `CompactReferenceRecord` interface (line ~322)

Dependencies:

- none

#### M17-E1-T2. Add CompactFileGroupedReferenceResponse type

Deliverable:

- `CompactFileGroupedReferenceResponse` interface in `server/src/models/responses.ts`

Implementation detail:

- extends `ConfidenceMetadata, ReliabilityMetadata`
- has `responseMode: "compact"`, `symbol`, `window`, `fileGroups: FileGroup[]`
- has `totalCount`, `truncated`, optional `category`, `groupedBySubsystem`, `groupedByModule`, `groupedByLanguage`
- replaces `CompactReferenceQueryResponse` as the compact return type for `find_references`

Dependencies:

- M17-E1-T1

#### M17-E1-T3. Add toFileGroupedReferences() function

Deliverable:

- `toFileGroupedReferences(references: ResolvedReference[]): FileGroup[]` in `server/src/compact-responses.ts`

Implementation detail:

- groups references by `filePath`
- each ref becomes `{ symbol: ref.sourceQualifiedName, line: ref.line }`
- sorts refs within each group by line number
- sorts groups by file path alphabetically
- import `ResolvedReference`, `FileGroup`, `FileGroupedRef` from `./models/responses`

Dependencies:

- M17-E1-T1

#### M17-E1-T4. Add toFileGroupedReferenceQueryResponse() function

Deliverable:

- `toFileGroupedReferenceQueryResponse(response: ReferenceQueryResponse, references: ResolvedReference[]): CompactFileGroupedReferenceResponse` in `server/src/compact-responses.ts`

Implementation detail:

- calls `toFileGroupedReferences(references)` for the `fileGroups` field
- preserves all metadata: `lookupMode`, `symbol`, `confidence`, `matchReasons`, `reliability`, `window`, `totalCount`, `truncated`, `groupedBy*`
- sets `responseMode: "compact"`

Dependencies:

- M17-E1-T2, M17-E1-T3

#### M17-E1-T5. Update find_references handler compact branch

Deliverable:

- `find_references` handler in `mcp-runtime.ts` uses `toFileGroupedReferenceQueryResponse()` when `compact=true`

Implementation detail:

- replace the compact branch at line ~1621:
  - before: `toCompactReferenceQueryResponse(payload, buildCompactResolvedReferences(references.results))`
  - after: `toFileGroupedReferenceQueryResponse(payload, references.results)`
- update the response type union: `ReferenceQueryResponse | CompactFileGroupedReferenceResponse`
- update imports to include `toFileGroupedReferenceQueryResponse` from `./compact-responses`
- the old `toCompactReferenceQueryResponse` and `buildCompactResolvedReferences` can remain for backward compatibility but are no longer called from this handler

Dependencies:

- M17-E1-T4

#### M17-E1-T6. Update existing compact test

Deliverable:

- `find_references supports compact mode` test in `mcp.test.ts` (line ~220) validates file-grouped structure

Implementation detail:

- assert `payload.responseMode === "compact"`
- assert `payload.fileGroups` is an array
- assert `payload.references` is undefined
- if `fileGroups.length > 0`: assert `fileGroups[0].file` is a string, `fileGroups[0].refs` is an array
- if `fileGroups[0].refs.length > 0`: assert `refs[0].symbol` is a string, `refs[0].line` is a number

Dependencies:

- M17-E1-T5

#### M17-E1-T7. Add HTTP endpoint compact test

Deliverable:

- test in `endpoints.test.ts` for `/references?qualifiedName=...&compact=true`

Implementation detail:

- validate the same file-grouped structure through the HTTP surface
- verify `responseMode === "compact"`, `fileGroups` present, `references` absent

Dependencies:

- M17-E1-T5

### M17-E2. Compact Mode for find_callers and search_symbols

#### M17-E2-T1. Add CompactCallerQueryResponse type

Deliverable:

- `CompactCallerQueryResponse` interface in `server/src/models/responses.ts`

Implementation detail:

- `responseMode: "compact"`, `lookupMode`, `symbol`, `confidence`, `matchReasons`, `window`
- `fileGroups: FileGroup[]` (replaces `callers: CallReference[]`)
- `totalCount`, `truncated`, optional `groupedBySubsystem`, `groupedByModule`, `groupedByLanguage`
- extends `HeuristicSelectionMetadata, ReliabilityMetadata`

Dependencies:

- M17-E1-T1

#### M17-E2-T2. Add CompactSearchResponse type

Deliverable:

- `CompactSearchResponse` interface in `server/src/models/responses.ts`

Implementation detail:

- `responseMode: "compact"`, `query`, `window`
- `results: CompactFileSymbol[]` (reuse existing type)
- `totalCount`, `truncated`, optional `groupedByLanguage`

Dependencies:

- none (uses existing `CompactFileSymbol`)

#### M17-E2-T3. Add toCompactCallerQueryResponse() formatter

Deliverable:

- `toCompactCallerQueryResponse(response: CallerQueryResponse): CompactCallerQueryResponse` in `compact-responses.ts`

Implementation detail:

- converts `callers: CallReference[]` to `FileGroup[]`:
  - each caller becomes `{ symbol: caller.qualifiedName, line: caller.line }` grouped by `caller.filePath`
- preserves all metadata fields
- reuses file-grouping logic from `toFileGroupedReferences` — extract shared helper `groupByFile(items, getFile, getSymbol, getLine)`

Dependencies:

- M17-E2-T1, M17-E1-T3

#### M17-E2-T4. Add toCompactSearchResponse() formatter

Deliverable:

- `toCompactSearchResponse(response: SearchResponse): CompactSearchResponse` in `compact-responses.ts`

Implementation detail:

- converts `results: Symbol[]` to `CompactFileSymbol[]` using existing `toCompactFileSymbol()`
- preserves `query`, `window`, `totalCount`, `truncated`, `groupedByLanguage`

Dependencies:

- M17-E2-T2

#### M17-E2-T5. Add compact parameter to find_callers

Deliverable:

- `find_callers` in `mcp-runtime.ts` accepts `compact: z.boolean().optional()` and returns `CompactCallerQueryResponse` when true

Implementation detail:

- add parameter at line ~1435 schema
- add compact branch after building the payload (line ~1486):
  ```typescript
  const response = compact ? toCompactCallerQueryResponse(payload) : payload;
  ```
- update imports

Dependencies:

- M17-E2-T3

#### M17-E2-T6. Add compact parameter to search_symbols

Deliverable:

- `search_symbols` in `mcp-runtime.ts` accepts `compact: z.boolean().optional()` and returns `CompactSearchResponse` when true

Implementation detail:

- add parameter at line ~1985 schema
- add compact branch after building the payload (line ~2017):
  ```typescript
  const response = compact ? toCompactSearchResponse(payload) : payload;
  ```
- update imports

Dependencies:

- M17-E2-T4

#### M17-E2-T7. Add compact tests for both tools

Deliverable:

- `find_callers supports compact mode` test in `mcp.test.ts`
- `search_symbols supports compact mode` test in `mcp.test.ts`

Implementation detail:

- `find_callers` test: assert `responseMode === "compact"`, `fileGroups` is array, `callers` is undefined
- `search_symbols` test: assert `responseMode === "compact"`, `results` array has compact symbol fields, no `filePath`/`language` in results

Dependencies:

- M17-E2-T5, M17-E2-T6

### M17-E3. Compact for impact_analysis, list_class_members, list_namespace_symbols

#### M17-E3-T1. Add CompactImpactAnalysisResponse type

Deliverable:

- `CompactImpactAnalysisResponse` interface in `models/responses.ts`

Implementation detail:

- `responseMode: "compact"`, `lookupMode`, `symbol`, `maxDepth`
- `callerFileGroups: FileGroup[]`, `calleeFileGroups: FileGroup[]`, `referenceFileGroups: FileGroup[]`
- `totalAffectedSymbols`, `totalAffectedFiles`, `topAffectedFiles`, `suggestedFollowUpQueries`
- `truncated`, optional `affectedSubsystems`, `affectedModules`, `affectedLanguages`

Dependencies:

- M17-E1-T1

#### M17-E3-T2. Add toCompactImpactAnalysisResponse() formatter

Deliverable:

- formatter in `compact-responses.ts`

Implementation detail:

- converts `directCallers: CallReference[]` → `callerFileGroups: FileGroup[]`
- converts `directCallees: CallReference[]` → `calleeFileGroups: FileGroup[]`
- converts `directReferences: ResolvedReference[]` → `referenceFileGroups: FileGroup[]`
- reuses shared file-grouping helper
- strips `topAffectedSymbols` to compact form (id, qualifiedName, type)

Dependencies:

- M17-E3-T1

#### M17-E3-T3. Add compact parameter to impact_analysis

Deliverable:

- `impact_analysis` in `mcp-runtime.ts` accepts `compact` and returns compact response when true

Implementation detail:

- add `compact: z.boolean().optional()` at line ~1736
- add compact branch after payload construction

Dependencies:

- M17-E3-T2

#### M17-E3-T4. Add compact parameter to list_class_members

Deliverable:

- `list_class_members` accepts `compact` and returns members as `CompactFileSymbol[]` when true

Implementation detail:

- add `compact: z.boolean().optional()` at line ~1835
- use existing `toCompactFileSymbol()` to strip member symbols
- new type: `CompactClassMembersResponse` with `responseMode: "compact"`, `symbol`, `summary`, `window`, `members: CompactFileSymbol[]`

Dependencies:

- none (uses existing `toCompactFileSymbol`)

#### M17-E3-T5. Add compact parameter to list_namespace_symbols

Deliverable:

- `list_namespace_symbols` accepts `compact` and returns symbols as `CompactFileSymbol[]` when true

Implementation detail:

- add `compact: z.boolean().optional()` at line ~1807
- use existing `toCompactFileSymbol()` to strip symbols
- new type: `CompactNamespaceSymbolsResponse` with `responseMode: "compact"`, `symbol`, `summary`, `window`, `symbols: CompactFileSymbol[]`

Dependencies:

- none (uses existing `toCompactFileSymbol`)

#### M17-E3-T6. Add compact tests for all three tools

Deliverable:

- tests in `mcp.test.ts` for compact mode on `impact_analysis`, `list_class_members`, `list_namespace_symbols`

Dependencies:

- M17-E3-T3, M17-E3-T4, M17-E3-T5

### M17-E4. Validation and Release Readiness

#### M17-E4-T1. Run full test suites

Deliverable:

- `npm test -- --runInBand` passes all tests
- `npm run build` passes

Dependencies:

- M17-E3-T6

#### M17-E4-T2. MCP smoke test against opencv DB

Deliverable:

- run `find_references(qualifiedName="cv::imread", compact=true)` and verify file-grouped output
- run `find_callers(name="imread", compact=true)` and verify file-grouped output
- run `search_symbols(query="Mat", compact=true)` and verify compact results

Dependencies:

- M17-E4-T1

#### M17-E4-T3. Measure and document size reduction

Deliverable:

- compare response sizes for representative queries in compact vs non-compact mode
- document actual measurements

Dependencies:

- M17-E4-T2

#### M17-E4-T4. Update milestone documentation

Deliverable:

- `dev_docs/Milestone17_CompactMCPResponses.md` updated with completion evidence

Dependencies:

- M17-E4-T3

---

## 7. Task Breakdown By File

### `server/src/models/responses.ts`

Planned tasks:

- add `FileGroupedRef`, `FileGroup` interfaces (M17-E1-T1)
- add `CompactFileGroupedReferenceResponse` interface (M17-E1-T2)
- add `CompactCallerQueryResponse` interface (M17-E2-T1)
- add `CompactSearchResponse` interface (M17-E2-T2)
- add `CompactImpactAnalysisResponse` interface (M17-E3-T1)
- add `CompactClassMembersResponse`, `CompactNamespaceSymbolsResponse` interfaces (M17-E3-T4, M17-E3-T5)

### `server/src/compact-responses.ts`

Planned tasks:

- add `toFileGroupedReferences()` function (M17-E1-T3)
- add `toFileGroupedReferenceQueryResponse()` function (M17-E1-T4)
- add `toCompactCallerQueryResponse()` function (M17-E2-T3)
- add `toCompactSearchResponse()` function (M17-E2-T4)
- add `toCompactImpactAnalysisResponse()` function (M17-E3-T2)
- extract shared file-grouping helper for reuse across formatters

### `server/src/mcp-runtime.ts`

Planned tasks:

- update `find_references` compact branch to use file-grouped formatter (M17-E1-T5)
- add `compact` parameter to `find_callers` (M17-E2-T5)
- add `compact` parameter to `search_symbols` (M17-E2-T6)
- add `compact` parameter to `impact_analysis` (M17-E3-T3)
- add `compact` parameter to `list_class_members` (M17-E3-T4)
- add `compact` parameter to `list_namespace_symbols` (M17-E3-T5)

### `server/src/__tests__/mcp.test.ts`

Planned tasks:

- update `find_references supports compact mode` test (M17-E1-T6)
- add `find_callers supports compact mode` test (M17-E2-T7)
- add `search_symbols supports compact mode` test (M17-E2-T7)
- add `impact_analysis supports compact mode` test (M17-E3-T6)
- add `list_class_members supports compact mode` test (M17-E3-T6)
- add `list_namespace_symbols supports compact mode` test (M17-E3-T6)

### `server/src/__tests__/endpoints.test.ts`

Planned tasks:

- add `/references?compact=true` HTTP endpoint test (M17-E1-T7)

---

## 8. Cross-Epic Risks

### Risk 1. Existing compact tests expect old field structure

Why it matters:

- `mcp.test.ts` line ~230 asserts `payload.references[0].sourceQualifiedName` — this will break when compact returns `fileGroups` instead

Mitigation:

- update the test in M17-E1-T6 as part of the same commit that changes the handler

### Risk 2. Old CompactReferenceRecord type becomes orphaned

Why it matters:

- `CompactReferenceRecord` and `CompactReferenceQueryResponse` will no longer be used by `find_references`

Mitigation:

- keep them for backward compatibility (other code may import them)
- mark as deprecated if desired, but do not delete in this milestone

### Risk 3. File-grouping changes sort order

Why it matters:

- current compact returns references in the same order as the full response
- file-grouped format groups by file then sorts by line within each group

Mitigation:

- this is intentional and beneficial — file-grouped order is more useful for navigation
- document the sort order in the response description

---

## 9. Definition of Done

MS17 is complete when:

1. `find_references(compact=true)` returns file-grouped format with `fileGroups` array
2. `find_callers(compact=true)` returns file-grouped format
3. `search_symbols(compact=true)` returns compact symbol records
4. `impact_analysis(compact=true)` returns file-grouped format for callers/callees/references
5. `list_class_members(compact=true)` returns compact member records
6. `list_namespace_symbols(compact=true)` returns compact symbol records
7. all non-compact responses are unchanged
8. all existing tests pass, new compact tests added
9. response size reduction documented with real measurements

Validation snapshot:

- `server`: `npm test -- --runInBand` passed
- `server`: `npm run build` passed
- MCP smoke test against opencv DB passed

---

## 10. Suggested First Implementation Slice

Start with the smallest slice that proves the milestone is worth doing:

1. add `FileGroupedRef`, `FileGroup`, `CompactFileGroupedReferenceResponse` types
2. add `toFileGroupedReferences()` and `toFileGroupedReferenceQueryResponse()`
3. update `find_references` compact branch
4. update the existing compact test

Why this slice first:

- it delivers the highest-value change immediately (79% size reduction for the most common large query)
- it proves the file-grouped pattern before extending to other tools
- it is self-contained to 4 files

---

## 11. Completion Evidence

Date: 2026-04-22

### Test results

```
npm test -- --runInBand
Tests: 161 passed, 161 total (16 test suites)
npm run build: passed (tsc, no errors)
```

### New tests added

- `mcp.test.ts`: find_references compact (updated), find_callers compact, search_symbols compact, impact_analysis compact, list_class_members compact, list_namespace_symbols compact
- `endpoints.test.ts`: /references compact (updated to file-grouped), list_class_members compact, list_namespace_symbols compact (mock store)

### opencv DB smoke test (4,602 files, 64,877 symbols)

| query | count | full size | compact size | reduction |
|---|---|---|---|---|
| find_callers: cv::imshow | 500 callers, 216 files | 80,320 bytes | 43,055 bytes | 46% |
| find_callers: cv::waitKey | 400 callers, 290 files | 65,227 bytes | 44,995 bytes | 31% |
| find_callers: cv::Mat::Mat | 400 callers, 114 files | 69,461 bytes | 27,636 bytes | 60% |
| find_callers: cv::resize | 179 callers, 73 files | 30,329 bytes | 14,509 bytes | 52% |
| search_symbols: "Mat" | 100 results | 80,451 bytes | 24,021 bytes | 70% |
| search_symbols: "calc" | 100 results | 88,962 bytes | 23,251 bytes | 74% |

### opencv full evaluation (fresh index: 4,602 files, 64,877 symbols)

Direct store evaluation (28/28 passed):

| check | result |
|---|---|
| index integrity | 4602 files, 64877 symbols, 105532 calls, 360969 propagation |
| cv::imread lookup | function @ modules/imgcodecs/include/opencv2/imgcodecs.hpp:384 |
| cv::resize callers | 179 callers, 73 file groups — 24% reduction |
| cv::imshow callers | 734 callers — 26% reduction |
| search "Mat" compact | 100 results — 71% reduction, filePath/language absent |
| search "calc" compact | 100 results — 75% reduction |
| list_class_members compact | cv::VideoCapture_LibRealsense 12 members — 75% reduction |
| type hierarchy | calib::FrameProcessor 2 derived types |

HTTP app evaluation (50/50 passed):

| endpoint | compact result |
|---|---|
| GET /references?compact=true | fileGroups array, references absent, reliability present |
| GET /callers/resize?compact=true | fileGroups=3 groups, callers absent |
| GET /search?q=imread&compact=true | 68% reduction, filePath/language absent |
| GET /impact?qualifiedName=cv::resize&compact=true | 56% reduction, callerFileGroups=18 groups |
| GET /class-members?qualifiedName=calib::FrameProcessor&compact=true | 4 members, filePath/language absent |
| non-compact regressions | all unchanged |

### llvm-project evaluation (fresh index: 71,617 files, 651,937 symbols)

Full indexing: discovery 23s | parse 587s | resolve 15,111s | propagation 201s — 15,773s total

DB consistency (all pass):

- 651,937 symbols, 1,000,324 calls, 800,061 references, 2,462,468 propagation, 71,617 files
- orphan checks: 0 orphan caller_id / callee_id / source_symbol_id / target_symbol_id / file_path / owner_symbol_id
- no duplicate symbol ids or file paths
- all required fields non-null (type, line, caller_id, callee_id)
- 28 anonymous enums with empty name (macro-heavy headers — known parser limitation, no calls/refs linked)
- line range sanity: no negative lines, no end_line < line
- metadata 100% coverage: module, subsystem, parse_fragility on all 651,937 symbols
- all files have content_hash

Compact format (70/70 passed):

| query | result |
|---|---|
| search "Function" compact | 70% reduction (96KB → 29KB, 100 results) |
| search "Builder" compact | 71% reduction (95KB → 28KB, 100 results) |
| search "parse" compact | 73% reduction (84KB → 23KB, 100 results) |
| GET /references compact | fileGroups=10 groups, references absent |
| GET /callers compact | fileGroups=2 groups, callers absent |
| GET /search compact | 70% reduction, filePath/language absent |
| GET /impact compact | 70% reduction (13KB → 4KB), callerFileGroups present |
| GET /class-members compact | responseMode=compact, members array present |
| non-compact regressions | all unchanged |

### Files changed

- `server/src/models/responses.ts` — added FileGroupedRef, FileGroup, CompactFileGroupedReferenceResponse, CompactCallerQueryResponse, CompactSearchResponse, CompactImpactAnalysisResponse, CompactClassMembersResponse, CompactNamespaceSymbolsResponse
- `server/src/compact-responses.ts` — added groupByFile helper, toFileGroupedReferences, toFileGroupedReferenceQueryResponse, toCompactCallerQueryResponse, toCompactSearchResponse, toCompactImpactAnalysisResponse, toCompactClassMembersResponse, toCompactNamespaceSymbolsResponse
- `server/src/mcp-runtime.ts` — upgraded find_references compact branch; added compact parameter to find_callers, search_symbols, impact_analysis, list_class_members, list_namespace_symbols
- `server/src/app.ts` — upgraded /references compact branch; added compact to /callers/:name, /search, /impact, /namespace-symbols, /class-members
- `server/src/__tests__/mcp.test.ts` — updated and added compact tests
- `server/src/__tests__/endpoints.test.ts` — updated and added compact tests
