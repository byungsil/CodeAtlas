# CodeAtlas API Contract

Base URL: `http://localhost:3000`

---

## Data Models

### Symbol

| Field         | Type     | Description                          |
|---------------|----------|--------------------------------------|
| id            | string   | Canonical exact symbol identifier    |
| name          | string   | Short name                           |
| qualifiedName | string   | Canonical exact qualified name       |
| type          | string   | One of: function, method, class, struct, enum, namespace, variable, typedef |
| filePath      | string   | Relative path from workspace root    |
| line          | number   | Start line (1-based)                 |
| endLine       | number   | End line (1-based)                   |
| signature     | string?  | Full signature (optional)            |
| parentId      | string?  | Parent symbol ID (optional)          |

**Exact identity rules:**

- `id` is the canonical exact identity for a symbol.
- `qualifiedName` is the canonical exact human-readable name.
- In the current contract, `id` and `qualifiedName` are expected to be identical for exact-match purposes.
- `name` is not an exact identifier and must be treated as exploratory or heuristic only.
- Declaration and definition pairs must resolve to one logical symbol identity, with file/line metadata attached to that logical symbol.
- Declaration-only and definition-only symbols are both valid exact lookup targets as long as they have a canonical `id`.
- When declaration and definition coexist, the API contract is one logical symbol, not two separate exact symbols.
- Inline/header-only callable implementations are represented as one logical symbol with inline-definition lifecycle semantics.

### Representative Symbol Direction

This is the Milestone 9 contract direction for choosing the representative anchor of a logical symbol on very large repositories.

Problem statement:

- a symbol may be resolved correctly while still exposing an unhelpful representative anchor
- on giant monorepos, representative anchors can drift toward:
  - test files
  - secondary declarations
  - inline helper locations
  - generated or compatibility paths
- this is a usability problem for agents because they usually continue reasoning from the first anchor returned

Representative anchor intent:

- every logical symbol should have one representative file and line anchor for default lookup responses
- representative selection must prefer the location a human engineer would most likely consider canonical
- declaration and definition metadata must still remain available even when only one anchor is chosen as representative

First-release representative ranking inputs:

- definition vs declaration
- out-of-line definition vs inline definition
- artifact kind:
  - runtime
  - editor
  - tool
  - test
  - generated
- header role:
  - public
  - private
  - internal
- path quality:
  - production/runtime path preferred over test/sample/benchmark/generated path
- scope quality:
  - symbol anchors closer to the canonical owning scope should outrank structurally weaker duplicates

First-release declaration and definition preference order:

- out-of-line definition in an implementation translation unit is preferred when present
- inline or header-only definition is the next structural fallback
- declaration-only anchor is the last structural fallback
- declaration and definition metadata must remain attached even when only one anchor becomes representative
- representative choice must not depend on incidental raw-merge order

Representative confidence vocabulary:

- `canonical`
  - the selected representative anchor is strongly preferred by current structure and path evidence
- `acceptable`
  - the selected representative anchor is usable, but not obviously the only canonical choice
- `weak`
  - the symbol is valid, but the representative anchor is selected from a duplicate-heavy or structurally noisy cluster

Representative selection reason vocabulary:

- `outOfLineDefinitionPreferred`
- `inlineDefinitionFallback`
- `declarationOnlyFallback`
- `runtimeArtifactPreferred`
- `publicHeaderPreferred`
- `nonTestPathPreferred`
- `nonGeneratedPathPreferred`
- `scopeCanonicalityPreferred`
- `duplicateClusterWeakCanonicality`

First-release path and artifact-aware canonicality guidance:

- runtime anchors should outrank test, sample, benchmark, and generated anchors when structural preference is otherwise comparable
- public headers should outrank private or internal headers when the symbol remains declaration-shaped on both sides
- path- and artifact-aware scoring is a tie-shaping policy layered on top of declaration/definition structure, not a replacement for structural preference
- representative choice should stay deterministic and explainable from persisted metadata such as:
  - `artifactKind`
  - `headerRole`
  - file path classification

Milestone 9 intent:

- representative selection quality is a separate concern from symbol existence and exact identity
- a symbol may be exact while its representative anchor is only `acceptable` or `weak`
- representative confidence should be surfaced separately from symbol lookup confidence when that metadata becomes part of public responses
- duplicate-heavy repositories such as LLVM and Unreal-engine-class game repositories are the primary validation targets for this contract

### Call

| Field    | Type   | Description                |
|----------|--------|----------------------------|
| callerId | string | Caller symbol ID           |
| calleeId | string | Callee symbol ID           |
| filePath | string | File where the call occurs |
| line     | number | Line of the call site      |

### FileRecord

| Field        | Type   | Description                    |
|--------------|--------|--------------------------------|
| path         | string | Relative file path             |
| contentHash  | string | SHA-256 of file content        |
| lastIndexed  | string | ISO 8601 timestamp             |
| symbolCount  | number | Number of symbols in this file |

### Internal Normalized Relation Event

This is an internal indexer contract for Milestone 2. It is not yet part of the public HTTP or MCP response surface.

| Field      | Type   | Description |
|------------|--------|-------------|
| relationKind | string | One of: `call`, `typeUsage`, `inheritance` |
| source     | string | Extraction origin: `legacyAst` or `treeSitterGraph` |
| confidence | string | Extraction confidence: `high` or `partial` |
| callerId   | string? | Source symbol ID for relation events that originate from a symbol body |
| targetName | string? | Unresolved target token for call-like events |
| callKind   | string? | One of: `unqualified`, `memberAccess`, `pointerMemberAccess`, `thisPointerAccess`, `qualified` |
| argumentCount | number? | Argument count hint for call-like events |
| receiver   | string? | Receiver token or normalized receiver text |
| receiverKind | string? | Receiver classification hint |
| qualifier  | string? | Namespace or type qualifier text |
| qualifierKind | string? | Qualifier classification hint |
| filePath   | string | Relative file path |
| line       | number | 1-based source line |

Milestone 2 intent:

- `RawRelationEvent` is the normalized extraction contract between parser-like extraction and later resolver/storage stages.
- During the transition, only `relationKind = call` is projected into the existing `RawCallSite` resolver input.
- `source` and `confidence` are tracked so graph-derived extraction can be compared against legacy AST extraction without changing public API behavior yet.

### First-Release Reference Model

This is the Milestone 3 normalized contract that future extraction, storage, and query work will target. It is partly represented in code already, but it is not yet exposed as a full HTTP or MCP query surface.

Supported first-release reference categories:

- `functionCall`
  - a direct call whose resolved target is a free function or namespace function
- `methodCall`
  - a direct call whose resolved target is a class or struct member function
- `classInstantiation`
  - reserved for constructor-like or object-creation references once extraction is added
- `moduleImport`
  - a structural module import relation such as Lua `require(...)`
- `typeUsage`
  - a structural mention of a type in declarations, fields, parameters, or local declarations
- `inheritanceMention`
  - a structural base-class mention in an inheritance clause

Intentional first-release limits:

- macro-sensitive semantic interpretation is out of scope
- template-dependent compiler-grade meaning is out of scope
- unresolved name mentions are not promoted to first-release references
- only resolved source and target symbol identities qualify for persisted references

Normalized reference payload:

| Field          | Type   | Description |
|----------------|--------|-------------|
| sourceSymbolId | string | Canonical symbol ID that owns or originates the reference |
| targetSymbolId | string | Canonical symbol ID being referenced |
| category       | string | One of: `functionCall`, `methodCall`, `classInstantiation`, `moduleImport`, `typeUsage`, `inheritanceMention` |
| filePath       | string | Relative file path |
| line           | number | 1-based source line |
| confidence     | string | Extraction confidence: `high` or `partial` |

Modeling decisions for Milestone 3:

- `Call` storage remains the canonical source for direct call edges that have already been resolved
- generalized references will live in their own storage surface rather than overloading the `calls` table
- `functionCall` and `methodCall` may be derived from existing resolved call edges when reference queries are introduced
- `moduleImport` is the first shared non-call structural relation added for mixed-language workspace support
- `typeUsage` and `inheritanceMention` will be promoted from normalized extraction events into persisted references in later Milestone 3 steps
- `classInstantiation` is part of the first-release vocabulary now so extraction and query code can grow into it without renaming the contract later

### First-Release Propagation Model

This is the Milestone 6 normalized contract for bounded value-propagation analysis. It is intentionally narrower than compiler-grade data-flow analysis and is designed to be explainable to AI agents.

Supported first-release propagation kinds:

- `assignment`
  - direct local or member assignment such as `a = b`
- `initializerBinding`
  - declaration-time binding such as `T x = y` or `auto x = y`
- `argumentToParameter`
  - value movement from a caller-side argument anchor into a callee parameter anchor on supported resolved calls
- `returnValue`
  - value movement from a callee return anchor into a caller-side assignment or initializer anchor on supported resolved calls
- `fieldWrite`
  - value movement into object state such as `this->member = value` or `obj.member = value`
- `fieldRead`
  - value movement out of object state such as `local = this->member` or `return member`

Explicit first-release non-goals:

- full alias analysis
- compiler-grade template instantiation semantics
- exact macro-expanded data flow
- whole-program pointer analysis
- pretending that propagation confidence is stronger than the underlying call or symbol resolution confidence

Normalized propagation payload:

| Field            | Type   | Description |
|------------------|--------|-------------|
| ownerSymbolId    | string? | Canonical symbol ID for the callable or scope that owns this propagation event |
| sourceAnchor     | object | Source propagation anchor |
| targetAnchor     | object | Target propagation anchor |
| propagationKind  | string | One of: `assignment`, `initializerBinding`, `argumentToParameter`, `returnValue`, `fieldWrite`, `fieldRead` |
| filePath         | string | Relative file path |
| line             | number | 1-based source line |
| confidence       | string | Structural extraction confidence: `high` or `partial` |
| risks            | array  | Zero or more bounded risk markers |

Propagation anchor shape:

| Field          | Type   | Description |
|----------------|--------|-------------|
| anchorId       | string? | Stable local anchor identity for scoped locals, parameters, or expression-backed propagation nodes when available |
| symbolId       | string? | Canonical symbol ID when the anchor resolves to a known symbol or field |
| expressionText | string? | Normalized source text when only an expression anchor is available |
| anchorKind     | string | One of: `localVariable`, `parameter`, `returnValue`, `field`, `expression` |

Propagation risk vocabulary:

- `aliasHeavyCode`
- `pointerHeavyFlow`
- `macroSensitiveRegion`
- `unresolvedOverload`
- `receiverAmbiguity`
- `unsupportedFlowShape`

Modeling decisions for Milestone 6:

- propagation is a dedicated analysis surface and does not overload `calls` or generalized `references`
- propagation edges may use either symbol anchors or expression anchors because many useful first-release flows are expression-shaped rather than declaration-shaped
- local and parameter anchors may carry a scoped `anchorId` even when they are not first-class global symbols
- function-boundary propagation depends on two internal layers:
  - callable flow summaries for parameter and return anchors
  - resolved call edges with recoverable caller-side argument and result-target anchors
- member-state propagation uses receiver-form-sensitive field anchors:
  - `this->member` is the strongest first-release field shape
  - `obj.member` is emitted as a weaker field anchor with receiver ambiguity
  - `ptr->member` is emitted only with explicit pointer-heavy risk signaling
- first release favors structurally inspectable events over broad but noisy inference
- unsupported cases should degrade into absent events or explicit risk markers, not guessed propagation paths
- agent-facing propagation queries should prefer compact summaries and bounded traversals rather than full raw graphs

### First-Release Multi-Language Capability Model

This is the Milestone 8 contract for extending CodeAtlas beyond C++ without diluting its large-repository and agent-usable focus.

Design intent:

- multi-language support exists to help agents answer real workspace structure, flow, and impact questions
- the contract prefers bounded shared capabilities over shallow promises of semantic parity
- the C/C++ family remains the deepest native language surface in first release

Mixed-workspace operating assumption:

- a single workspace may contain C++, Lua, Python, TypeScript, and Rust together
- this mixed-language state is a first-class target, not an exception path
- file discovery and parsing may dispatch by language, but stored symbols, relations, and query surfaces must still compose into one language-aware workspace model
- when a query crosses a language boundary, the response should preserve the shared workflow and surface explicit boundary notes when direct structural continuity is unavailable

Explicit first-release format boundary:

- `.c` is included with the existing C/C++ family support because large native repositories commonly mix C and C++
- XML is out of scope for Milestone 8 first release because it is not yet shown to be a consistently high-value structural code-intelligence format for the product's target questions

Shared first-release language metadata:

| Field               | Type   | Description |
|---------------------|--------|-------------|
| language            | string | One of: `cpp`, `lua`, `python`, `typescript`, `rust` |
| symbolRole          | string? | Language-specific structural role such as `module`, `class`, `method`, `function`, `trait`, `tableMember`, `interface` |
| modulePath          | string? | Normalized module or container path when the language has one |
| exportVisibility    | string? | Visibility or export hint when structurally meaningful |
| languageVersionHint | string? | Reserved optional version hint when a parser or workspace adapter can provide it cheaply |

Shared first-release capabilities:

| Capability       | C++ | Lua | Python | TypeScript | Rust | Notes |
|------------------|-----|-----|--------|------------|------|-------|
| exactLookup      | yes | yes | yes | yes | yes | Exact lookup remains the anchor workflow across all languages. |
| search           | yes | yes | yes | yes | yes | Name-oriented exploration remains available everywhere. |
| directCallers    | yes | yes | yes | yes | yes | Only direct structurally recoverable call edges are required in first release. |
| references       | yes | yes | yes | yes | yes | Categories vary by language but the query surface remains shared. |
| impactAnalysis   | yes | yes | yes | yes | yes | Summary-first responses remain required even when depth is language-limited. |
| fileOrModuleOverview | yes | yes | yes | yes | yes | Agents must be able to browse structure without opening raw files. |

Shared capability requirements for all first-release languages:

- exact symbol targeting must exist
- exploratory search must exist
- direct caller queries must exist for supported direct-call shapes
- generalized reference queries must exist for the language's supported structural relations
- impact analysis must return bounded summary-first results
- file or module overview must let the agent browse structure progressively

Language-specific structural reference focus:

| Language   | First-release emphasis |
|------------|------------------------|
| C++        | calls, type usage, inheritance, propagation-aware navigation |
| Lua        | module functions, table-attached functions, `require(...)` relations, direct calls |
| Python     | imports, free functions, classes, methods, simple direct calls |
| TypeScript | import/export chains, exported functions, classes, methods, interfaces where cheap |
| Rust       | modules, `use` relations, functions, structs, enums, traits, impl-attached methods |

Intentional C++-only advanced capabilities for now:

- advanced propagation depth
- build-metadata-driven refinement
- macro and include risk semantics
- compiler-adjacent confidence shaping that depends on C++-specific structure

Language-specific confidence boundaries:

- Lua:
  - strong for module-level functions, table members when syntactically obvious, and `require(...)`
  - weak or unsupported for metatable-driven resolution, dynamic globals, and runtime-generated code
- Python:
  - strong for imports, free functions, classes, simple methods, and direct call shapes that stay structurally obvious
  - weak or unsupported for monkey patching, reflection-driven imports, and heavy dynamic dispatch
- TypeScript:
  - strong for import/export structure, direct callable/class structure, and simple namespace or `this`-based call shapes
  - weak or unsupported for full typechecker-grade meaning, complex module-resolution certainty, and runtime-dynamic patterns
- Rust:
  - strong for module trees, traits, impl blocks, impl methods, `use` structure, and simple path-qualified or `self`-based direct calls
  - weak or unsupported for macro expansion semantics, full trait resolution, and compiler-grade type inference

Agent-facing contract implications:

- every multi-language response should surface `language`
- shared query names stay stable even when per-language capability depth differs
- unsupported semantic depth must degrade into bounded omission, lower confidence, or explicit limits rather than false certainty
- mixed-workspace support is successful only if it reduces fallback-to-raw-code behavior in large real repositories
- mixed-workspace query integration should also expose:
  - optional `language` filters on shared query surfaces
  - grouped language summaries where result sets span multiple languages
  - workspace-level language distribution summaries for files and symbols

---

## Endpoints

Note:

- The legacy HTTP API remains name-oriented for `/function/:name` and `/class/:name`.
- The canonical exact lookup path is `GET /symbol`.
- `search` is the exploratory discovery path when exact identity is not yet known.

---

### GET /symbol

Retrieve one symbol by canonical exact identity.

**Parameters:**

- `id` (query, optional) — Canonical exact symbol identity.
- `qualifiedName` (query, optional) — Canonical exact human-readable symbol identity.

**Parameter rules:**

- At least one of `id` or `qualifiedName` is required.
- In the current contract, `id` and `qualifiedName` identify the same logical symbol.
- If both are provided and do not identify the same logical symbol, the request is invalid.
- `GET /symbol` must not fall back to short-name matching.

**Response 200:**

```json
{
  "lookupMode": "exact",
  "symbol": { "id": "Game::GameObject::Update", "qualifiedName": "Game::GameObject::Update" }
}
```

Depending on symbol kind, the payload may also include:

- `callers`
- `callees`
- `members`

Representative-symbol direction:

- future exact-lookup responses may also include representative metadata such as:
  - `representativeConfidence`
  - `representativeSelectionReasons`
  - `alternateCanonicalCandidateCount`

Current Milestone 9 status:

- exact symbol lookup now may include representative metadata when available
- this metadata is separate from exact symbol identity and lookup confidence
- `alternateCanonicalCandidateCount` is only present when multiple top-ranked representative candidates remain plausible

Optional repository-tunable representative rules:

- first release supports an optional workspace-root `.codeatlasrepresentative.json`
- the file is not required for normal operation
- it applies a bounded repository-specific bias on top of the generic representative scorer
- supported first-release inputs:
  - `preferredPathPrefixes`
  - `demotedPathPrefixes`
  - `favoredArtifactKinds`
  - `favoredHeaderRoles`
- repository-specific tuning must not replace structural preference:
  - out-of-line definition vs inline definition vs declaration fallback remains primary
  - repository rules only help on structurally similar or duplicate-heavy cases
- this metadata is intended to explain the quality of the chosen anchor, not the existence of the symbol itself

**Response 400:**

```json
{ "error": "Invalid exact lookup request", "code": "BAD_REQUEST" }
```

**Response 404:**

```json
{ "error": "Symbol not found", "code": "NOT_FOUND" }
```

### GET /function/:name

Retrieve a function or method by name.

**Parameters:**
- `name` (path) — Symbol name to look up. Matches against `name` field.

**Response 200:**

```json
{
  "lookupMode": "heuristic",
  "symbol": { Symbol },
  "confidence": "high_confidence_heuristic",
  "matchReasons": [],
  "callers": [ CallReference ],
  "callees": [ CallReference ]
}
```

**CallReference:**

| Field         | Type   | Description             |
|---------------|--------|-------------------------|
| symbolId      | string | Related symbol ID       |
| symbolName    | string | Short name              |
| qualifiedName | string | Fully qualified name    |
| filePath      | string | File path               |
| line          | number | Line number             |
| confidence    | string | Confidence level        |
| matchReasons  | array  | Structural match reasons |

**Response 404:**

```json
{ "error": "Symbol not found", "code": "NOT_FOUND" }
```

**Notes:**
- If multiple symbols match the name, returns the first match. Use qualified name for precision.
- Name-only lookup is heuristic. Exact lookup should use canonical symbol identity once exposed by the API surface.
- `GET /function/:name` is not the canonical exact lookup path.
- Heuristic lookup responses now expose:
  - `lookupMode`
  - `confidence`
  - `matchReasons`
  - optional `ambiguity` when multiple short-name candidates exist
- Current ambiguity behavior:
  - the endpoint remains backward compatible and still returns one selected symbol payload
  - when multiple short-name candidates exist, callers must treat the result as heuristic rather than exact
  - ambiguity is surfaced through `confidence = "ambiguous"`, `matchReasons = ["ambiguous_top_score"]`, and `ambiguity.candidateCount`

---

### GET /class/:name

Retrieve a class or struct and its members.

**Parameters:**
- `name` (path) — Class name to look up.

**Response 200:**

```json
{
  "lookupMode": "heuristic",
  "symbol": { Symbol },
  "confidence": "high_confidence_heuristic",
  "matchReasons": [],
  "members": [ Symbol ]
}
```

**Response 404:**

```json
{ "error": "Symbol not found", "code": "NOT_FOUND" }
```

**Notes:**

- `GET /class/:name` is name-based and heuristic when duplicate short names exist.
- `GET /class/:name` is not the canonical exact lookup path.
- Heuristic lookup responses may include `ambiguity` metadata when multiple short-name candidates exist.

**Lookup mode guidance:**

- Use `GET /symbol` when exact symbol identity is already known.
- Use `GET /search` when the exact identity is not yet known.
- Use `GET /function/:name` and `GET /class/:name` as convenience heuristics for exploratory workflows, then switch to exact identity once the intended `qualifiedName` is known.

---

### GET /search?q=

Search symbols by name substring.

**Parameters:**
- `q` (query) — Search query string. Minimum length: 3 characters.
- `type` (query, optional) — Filter by symbol type.
- `limit` (query, optional) — Max results. Default: 50. Max: 200.

**Response 200:**

```json
{
  "query": "Update",
  "window": {
    "returnedCount": 3,
    "totalCount": 3,
    "truncated": false,
    "limitApplied": 50
  },
  "results": [ Symbol ],
  "totalCount": 3,
  "truncated": false
}
```

**Notes:**
- `truncated: true` when `totalCount > limit`.
- Queries shorter than 3 characters return an empty result set.
- Case-insensitive substring match on `name` and `qualifiedName`.

---

### GET /callgraph/:name

Retrieve the call graph rooted at a symbol.

**Parameters:**
- `name` (path) — Root symbol name.
- `depth` (query, optional) — Max traversal depth. Default: 3. Max: 10.

**Response 200:**

```json
{
  "root": {
    "symbol": { id, name, qualifiedName, type, filePath, line },
    "callees": [
      {
        "targetId": "string",
        "targetName": "string",
        "targetQualifiedName": "string",
        "filePath": "string",
        "line": 0
      }
    ]
  },
  "depth": 1,
  "maxDepth": 3,
  "truncated": false
}
```

### GET /references

Retrieve generalized references for one exact target symbol identity.

**Parameters:**
- `id` (query, optional)
- `qualifiedName` (query, optional)
- `category` (query, optional)
- `filePath` (query, optional)
- `limit` (query, optional)

**Response 200:**

```json
{
  "lookupMode": "exact",
  "symbol": { "id": "Game::Worker::Update", "qualifiedName": "Game::Worker::Update" },
  "window": {
    "returnedCount": 1,
    "totalCount": 1,
    "truncated": false,
    "limitApplied": 50
  },
  "references": [
    {
      "sourceSymbolId": "Game::Worker::Tick",
      "sourceQualifiedName": "Game::Worker::Tick",
      "targetSymbolId": "Game::Worker::Update",
      "category": "methodCall",
      "filePath": "src/worker.cpp",
      "line": 8,
      "confidence": "high"
    }
  ],
  "totalCount": 1,
  "truncated": false
}
```

**Notes:**
- `truncated: true` when the graph was cut short at `maxDepth` and unexplored edges remain.
- Depth 1 returns only direct callees. Depth 2 includes callees-of-callees, etc.

**Response 404:**

```json
{ "error": "Symbol not found", "code": "NOT_FOUND" }
```

### GET /file-symbols

Retrieve all symbols declared in one exact workspace-relative file path.

**Parameters:**
- `filePath` (query, required)
- `limit` (query, optional)

**Response 200:**

```json
{
  "filePath": "src/game.h",
  "summary": {
    "totalCount": 3,
    "typeCounts": { "namespace": 1, "class": 1, "method": 1 }
  },
  "window": {
    "returnedCount": 3,
    "totalCount": 3,
    "truncated": false,
    "limitApplied": 50
  },
  "symbols": [ Symbol ]
}
```

### GET /namespace-symbols

Retrieve direct symbols enclosed by one exact namespace qualified name.

**Parameters:**
- `qualifiedName` (query, required)
- `limit` (query, optional)

**Response 200:**

```json
{
  "lookupMode": "exact",
  "symbol": { "qualifiedName": "Game", "type": "namespace" },
  "summary": {
    "totalCount": 2,
    "typeCounts": { "class": 1, "function": 1 }
  },
  "window": {
    "returnedCount": 2,
    "totalCount": 2,
    "truncated": false,
    "limitApplied": 50
  },
  "symbols": [ Symbol ]
}
```

### GET /class-members

Retrieve direct members for one exact class or struct qualified name.

**Parameters:**
- `qualifiedName` (query, required)
- `limit` (query, optional)

**Response 200:**

```json
{
  "lookupMode": "exact",
  "symbol": { "qualifiedName": "Game::GameObject", "type": "class" },
  "summary": {
    "totalCount": 2,
    "typeCounts": { "method": 2 }
  },
  "window": {
    "returnedCount": 2,
    "totalCount": 2,
    "truncated": false,
    "limitApplied": 50
  },
  "members": [ Symbol ]
}
```

---

## Common Behavior

- All responses are `Content-Type: application/json`.
- All file paths are relative to the indexed workspace root.
- Line numbers are 1-based.
- Symbol IDs use a canonical exact qualified-path format such as `Namespace::Class::Method`.
- `qualifiedName` uses the same canonical exact format as `id` in the current contract.
- Partial results are always explicitly marked with `truncated: true`.

## Lookup Mode Guidance

Recommended progression:

1. Use `search_symbols` / `GET /search` to discover likely candidates.
2. Use `lookup_function` / `lookup_class` or their HTTP equivalents only as heuristic convenience lookups.
3. Once the intended symbol identity is known, switch to `lookup_symbol` / `GET /symbol`.

In Milestone 1, name-based lookup remains intentionally backward compatible and may still return a selected symbol even when duplicate short names exist. That selected result must not be treated as exact unless it came from the exact lookup path.

## Confidence Taxonomy

- `exact`
  - canonical exact identity lookup succeeded
- `high_confidence_heuristic`
  - a unique top-ranked structural candidate was chosen
- `ambiguous`
  - multiple candidates share the top score
- `unresolved`
  - no viable candidate was found

Representative quality is separate from this lookup-confidence taxonomy:

- lookup confidence answers:
  - "did CodeAtlas identify the symbol or relation confidently?"
- representative confidence answers:
  - "is the chosen default file/line anchor the canonical place a human would most likely start?"

Milestone 1 match-reason vocabulary:

- `exact_id_match`
- `exact_qualified_name_match`
- `same_parent_match`
- `same_namespace_match`
- `this_receiver_match`
- `member_call_prefers_method`
- `qualified_type_match`
- `qualified_namespace_match`
- `parameter_count_match`
- `signature_arity_hint`
- `ambiguous_top_score`
- `no_viable_candidate`

Persistence note:

- Milestone 1 treats confidence and match reasons as query-time derived metadata, not persisted source-of-truth fields in the database.
- Persisted storage remains canonical-symbol and canonical-edge oriented.
- API lookup payloads surface confidence at the top level for the selected symbol lookup result.
- `CallReference` payloads surface confidence for persisted resolved relations; in Milestone 1 these are emitted as `high_confidence_heuristic` unless a future confidence-aware query path can reattach richer reasons.

## Structural Confidence Semantics

What confidence means:

- CodeAtlas confidence describes how strongly the current structural evidence supports a returned result.
- Confidence is about lookup and relation-selection semantics inside CodeAtlas, not about full C++ semantic completeness.
- `exact` means canonical symbol targeting succeeded through exact identity input.
- `high_confidence_heuristic` means CodeAtlas selected one structurally strongest candidate from heuristic lookup or persisted resolved relations.
- `ambiguous` means multiple candidates remain plausible at the current query surface.
- `unresolved` means CodeAtlas does not currently have a viable structural answer for that query path.

What confidence does not mean:

- `exact` does not mean compiler-grade truth about templates, macros, overload sets, or build-configuration-specific semantics.
- `high_confidence_heuristic` does not mean the answer is guaranteed correct in every C++ edge case.
- `ambiguous` does not mean the index is broken; it means the current structural evidence does not justify pretending there is one exact winner.
- `unresolved` does not necessarily mean the symbol is absent from the codebase; it may mean the current index or query path cannot safely resolve it.

Interpretation guidance:

- prefer exact identity lookup whenever a canonical `qualifiedName` or `id` is known
- treat heuristic lookup as a navigation aid, not as canonical truth
- when `confidence = ambiguous`, refine the query or switch to exact identity lookup
- when `confidence = unresolved`, search for nearby candidates or refresh the index before drawing conclusions

Examples:

- exact result:
  - `GET /symbol?qualifiedName=Game::GameObject::Update`
  - `lookupMode = exact`
  - `confidence = exact`
- heuristic ambiguous result:
  - `GET /function/Update`
  - `lookupMode = heuristic`
  - `confidence = ambiguous`
  - `matchReasons = ["ambiguous_top_score"]`
- heuristic resolved relation:
  - persisted caller/callee reference
  - `confidence = high_confidence_heuristic`

---

## MCP Exact Lookup Contract

### Tool: `lookup_symbol`

Retrieve one symbol by canonical exact identity.

**Arguments:**

- `id` (optional) — canonical exact symbol identity
- `qualifiedName` (optional) — canonical exact human-readable symbol identity

**Argument rules:**

- At least one of `id` or `qualifiedName` is required.
- In the current contract, `id` and `qualifiedName` identify the same logical symbol.
- If both are provided and do not identify the same logical symbol, the request is invalid.
- `lookup_symbol` must not fall back to short-name matching.

**Success payload:**

```json
{
  "lookupMode": "exact",
  "symbol": {
    "id": "Game::GameObject::Update",
    "qualifiedName": "Game::GameObject::Update"
  }
}
```

Depending on symbol kind, the payload may also include:

- `callers`
- `callees`
- `members`

**Error payloads:**

```json
{ "error": "Symbol not found", "code": "NOT_FOUND" }
```

```json
{ "error": "Invalid exact lookup request", "code": "BAD_REQUEST" }
```

### Tool: `find_references`

Retrieve generalized references for one exact target symbol identity.

**Arguments:**

- `id` (optional)
- `qualifiedName` (optional)
- `category` (optional)
- `filePath` (optional)
- `limit` (optional)

**Current behavior:**

- uses exact symbol targeting, not short-name heuristics
- returns resolved source-symbol metadata together with category and confidence
- currently surfaces persisted references stored in `symbol_references`
- includes a shared `window` metadata block for bounded result sets

### Tool: `impact_analysis`

Summarize likely change impact for one exact target symbol.

**Arguments:**

- `id` (optional)
- `qualifiedName` (optional)
- `depth` (optional)
- `limit` (optional)

**Current behavior:**

- uses exact symbol targeting, not short-name heuristics
- summarizes direct callers, direct callees, and direct generalized references
- adds bounded caller/callee traversal to produce:
  - `topAffectedSymbols`
  - `topAffectedFiles`
  - `suggestedFollowUpQueries`

### Tool: `list_file_symbols`

Retrieve all symbols for one exact workspace-relative file path in stable declaration order.

- accepts optional `limit`
- returns `summary` plus shared `window` metadata before the symbol list

### Tool: `list_namespace_symbols`

Retrieve direct enclosed symbols for one exact namespace qualified name.

- accepts optional `limit`
- returns `summary` plus shared `window` metadata before the symbol list

### Tool: `list_class_members`

Retrieve direct members for one exact class or struct qualified name.

- accepts optional `limit`
- returns `summary` plus shared `window` metadata before the member list

### GET /symbol-propagation

Summarize bounded incoming and outgoing propagation events for one exact symbol identity.

**Parameters:**

- `id` (query, optional)
- `qualifiedName` (query, optional)
- `filePath` (query, optional)
- `limit` (query, optional)
- `propagationKinds` (query, optional, comma-separated)

**Response 200:**

```json
  {
    "lookupMode": "exact",
    "symbol": { "qualifiedName": "Game::Worker::Update" },
    "window": {
    "returnedCount": 4,
    "totalCount": 4,
      "truncated": false,
      "limitApplied": 50
    },
    "propagationConfidence": "high",
    "incoming": [PropagationEventRecord],
    "outgoing": [PropagationEventRecord],
    "riskMarkers": [],
    "confidenceNotes": [
      "All returned propagation hops come from supported structural patterns without additional risk markers."
    ],
    "summary": ["incoming: 2 event(s)", "outgoing: 2 event(s)"]
  }
  ```

### GET /trace-variable-flow

Trace one bounded propagation path for one exact symbol identity.

**Parameters:**

- `id` (query, optional)
- `qualifiedName` (query, optional)
- `filePath` (query, optional)
- `maxDepth` (query, optional)
- `maxEdges` (query, optional)
- `propagationKinds` (query, optional, comma-separated)

**Response 200:**

```json
  {
    "lookupMode": "exact",
    "symbol": { "qualifiedName": "Game::Worker::Update" },
    "window": {
    "returnedCount": 2,
    "totalCount": 2,
      "truncated": false,
      "limitApplied": 50
    },
    "propagationConfidence": "high",
    "riskMarkers": [],
    "confidenceNotes": [
      "All returned propagation hops come from supported structural patterns without additional risk markers."
    ],
    "pathFound": true,
    "truncated": false,
    "maxDepth": 3,
  "maxEdges": 50,
  "steps": [PropagationPathStep],
  "suggestedFollowUpQueries": [
    "explain_symbol_propagation qualifiedName=Game::Worker::Update"
  ]
}

---

## Project Metadata Direction

Milestone 5 introduces an optional path-derived metadata layer for symbols and file records.

First-release metadata fields:

- `subsystem`
- `module`
- `projectArea`
- `artifactKind`
- `headerRole`

First-release rules:

- metadata is derived deterministically from workspace-relative file paths
- no repository-specific config is required in the initial version
- absent or weak path evidence should leave fields unset rather than guessed

Milestone 7 extends this with an optional build-metadata overlay.

Build-metadata overlay rules:

- `compile_commands.json` ingestion is optional and auto-detected when present
- baseline indexing and query behavior remain fully usable without compile DB presence
- first-release build metadata is used only for lightweight metadata refinement, not compiler-grade parsing
- build metadata may:
  - promote `headerRole` to `public` when a header lives under a workspace include directory discovered from compile commands
  - refine `artifactKind` when compile output paths or cheap define hints indicate test/editor/tool/generated intent
- build metadata must not silently replace the path-derived baseline with speculative semantics
- compiler-grade include resolution, macro expansion, and configuration-specific parsing remain out of scope

Milestone 7 also introduces lightweight C++ fragility signals.

File and symbol risk-signal fields:

- `parseFragility`
  - `low`
  - `elevated`
- `macroSensitivity`
  - `low`
  - `high`
- `includeHeaviness`
  - `light`
  - `heavy`

Risk-signal rules:

- these are lightweight structural heuristics, not compiler-grade diagnostics
- risk signals are attached at file level and copied onto symbols declared in that file
- `parseFragility = elevated` may be raised by tree-sitter error recovery, macro density, or unusually heavy include load
- `macroSensitivity = high` indicates macro-heavy regions where build-configuration-specific meaning may diverge from the structural index
- `includeHeaviness = heavy` indicates files whose include volume makes build-context interactions more likely
- risk signals are meant to guide agent caution and follow-up exploration, not to suppress otherwise useful results

Metadata-aware query behavior:

- `GET /search` and MCP `search_symbols` accept optional `subsystem`, `module`, `projectArea`, and `artifactKind` filters
- `GET /callers/:name`, `GET /references`, and `GET /impact` accept the same optional metadata filters
- caller and reference responses may include compact grouped summaries such as `groupedBySubsystem` and `groupedByModule`
- impact-analysis responses may include `affectedSubsystems` and `affectedModules` summaries
- when metadata filters are requested against an older SQLite snapshot that does not expose metadata columns, filtered search returns an empty result set rather than silently ignoring the filter

## Propagation Confidence Direction

Milestone 6 extends structural confidence into propagation-specific guidance.

Interpretation rules:

- `high`
  - the propagation step is supported by an intentionally modeled structural pattern with no immediate ambiguity marker
- `partial`
  - the propagation step is structurally plausible but bounded by weaker evidence, unsupported adjacent syntax, or inherited uncertainty from call resolution

Propagation confidence should be read together with risk markers:

- a `high` propagation step is still not compiler-grade proof
- a `partial` propagation step should guide the agent toward focused follow-up queries rather than broad conclusions
- risk markers exist to explain why a propagation answer is limited, not merely to decorate the payload

Response-level propagation guidance:

- exact symbol lookup still uses `confidence = exact`; this only means the symbol target is exact
- propagation-specific strength is carried separately as `propagationConfidence`
- `propagationConfidence = high` means every returned hop is high-confidence and no aggregate risk markers were raised
- `propagationConfidence = partial` means at least one returned hop is partial, the answer was truncated, or aggregate risk markers indicate weaker evidence

---

## Reference Query Direction

Milestone 3 contract direction:

- direct caller queries are now available separately from full function lookup
- generalized reference queries will use the first-release reference model above
- future `find_references` responses should return category and confidence explicitly
- future impact-analysis responses may combine:
  - callers
  - callees
  - generalized references

```json
{ "error": "Invalid exact lookup request", "code": "BAD_REQUEST" }
```

**Legacy MCP tools:**

- `lookup_function` and `lookup_class` remain available
- they are name-based and heuristic when duplicate short names exist
- they are not the canonical exact lookup path

## Error Codes

| Code           | HTTP Status | Description               |
|----------------|-------------|---------------------------|
| NOT_FOUND      | 404         | Symbol not found          |
| BAD_REQUEST    | 400         | Invalid query parameters  |
| INTERNAL_ERROR | 500         | Server-side failure       |
### Tool: `trace_variable_flow`

Current behavior:

- uses exact symbol targeting
- returns one deterministic bounded propagation path instead of a raw full graph
- supports `maxDepth`, `maxEdges`, optional `propagationKinds`, and optional `filePath`
- each hop carries propagation kind, confidence, and risk markers
- response-level `propagationConfidence`, `riskMarkers`, and `confidenceNotes` summarize whether the returned path should be treated as strong or weak guidance

### Tool: `explain_symbol_propagation`

Current behavior:

- summarizes incoming and outgoing propagation events for one exact symbol identity
- surfaces aggregate risk markers together with compact summary lines
- supports `limit`, optional `propagationKinds`, and optional `filePath`
- response-level `propagationConfidence` and `confidenceNotes` explain whether the propagation answer is structurally strong, partial, or limited by bounds
