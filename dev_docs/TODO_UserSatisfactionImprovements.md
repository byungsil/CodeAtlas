# TODO: User Satisfaction Improvements

Status:

- Planned

Origin:

- Derived from direct observation during a real-world investigation session on a large C++ game project (32K files, 254K symbols).
- Investigation tasks: (1) shot input processing call chain, (2) ShotFlags enum full usage path.
- Three tools were compared: ast-grep, Serena, CodeAtlas.
- The findings below reflect concrete pain points that reduced agent effectiveness and user trust.

---

## 1. Objective

Reduce repeated queries, surface index coverage gaps honestly, and strengthen enum and macro-heavy symbol tracking so agents can complete real investigations with fewer round-trips and higher confidence in results.

Success outcome:

- an agent investigating a large C++ codebase can reach a satisfactory answer with fewer queries
- zero-result responses for real symbols trigger a coverage warning rather than silent empty results
- ambiguous lookups surface ranked alternatives instead of forcing a blind re-query
- enum member value usages are findable without falling back to text search

---

## 2. Recommended Order

1. M11-E1. Index coverage gaps: macro-heavy callers and missing call edges
2. M11-E2. Ambiguous lookup: ranked candidate list on first response
3. M11-E3. Upstream call chain traversal (reverse callgraph)
4. M11-E4. Enum member value usage indexing
5. M11-E5. Large result inline delivery and response compaction
6. M11-E6. Reliability metadata surfacing

---

## 3. Epics

---

### M11-E1. Index Coverage Gaps: Macro-Heavy Callers and Missing Call Edges

Priority: Critical

#### Background

During the investigation session, `find_callers("SetShotFlags")` and
`find_references("ShotSubSystem::SetShotFlags")` both returned `totalCount: 0`.
The actual call site `shotnormal.cpp:1605` was confirmed to exist via direct source read.
Affected symbols share `parseFragility: "elevated"` and `macroSensitivity: "high"`.

This is the highest-impact issue because silent zero results destroy agent trust in the index.
An agent receiving zero callers assumes no callers exist, not that the index is incomplete.

#### Goal

- reduce false-zero caller results for macro-heavy C++ files
- when a zero-result cannot be ruled out as a coverage gap, tell the agent explicitly

#### Implementation Tasks

- [ ] M11-E1-T1: Audit which symbol categories produce false-zero callers in the FIFA-scale benchmark
  - query: `SELECT s.id, s.parse_fragility, s.macro_sensitivity, COUNT(cr.caller_id) FROM symbols s LEFT JOIN call_relationships cr ON s.id = cr.callee_id GROUP BY s.id HAVING COUNT(cr.caller_id) = 0 AND s.parse_fragility = 'elevated'`
  - record count and percentage affected
  - acceptance: audit report produced, scope confirmed

- [ ] M11-E1-T2: Add fallback text-reference extraction pass for elevated-fragility symbols
  - when tree-sitter parse yields no call edges for an elevated-fragility symbol, run a secondary token-presence scan over files that include the symbol's header
  - store results in a separate `fallback_call_relationships` table with `confidence: "low"`
  - acceptance: at least one known false-zero symbol gains callers via fallback pass

- [ ] M11-E1-T3: Expose `indexCoverage` field in all caller/reference query responses
  - values: `"full"` | `"partial"` | `"unknown"`
  - set to `"partial"` when the target symbol has `parseFragility: elevated` or `macroSensitivity: high`
  - acceptance: `find_callers` and `find_references` responses include `indexCoverage` field

- [ ] M11-E1-T4: Add `coverageWarning` message to zero-result responses for fragile symbols
  - when `totalCount = 0` AND `indexCoverage != "full"`, include:
    ```json
    "coverageWarning": "This symbol has elevated parse fragility. Zero callers may reflect an index gap rather than an absence of callers. Consider cross-checking with ast-grep."
    ```
  - acceptance: zero-result response for `SetShotFlags`-class symbols includes the warning

- [ ] M11-E1-T5: Add FIFA-scale regression test for known caller relationships
  - record at least 5 ground-truth caller pairs from the real project
  - add to CI benchmark suite
  - acceptance: regression test passes after E1 changes land

#### Validation Checklist

- [ ] `find_callers("SetShotFlags")` no longer returns silent zero on the FIFA project
- [ ] OR if still zero, response includes `coverageWarning`
- [ ] `indexCoverage` field present in all `find_callers` and `find_references` responses
- [ ] No regression on existing caller tests

---

### M11-E2. Ambiguous Lookup: Ranked Candidate List on First Response

Priority: High

#### Background

Common method names (`Update`, `Init`, `IsShot`, `AddShotFlag`) triggered ambiguous resolution
with `candidateCount` ranging from 4 to 1785. The tool returned one guess, often wrong,
and the agent had to re-query with additional hints.

Observed pattern:
- `lookup_function("Update", anchorQualifiedName="ShotSubSystem::SetShotFlags")` → returned `WallReactionSys::Update` (anchor ignored)
- `lookup_function("Init")` → `candidateCount: 516`, wrong symbol returned
- Each wrong guess costs one full query round-trip

#### Goal

- surface the top-N ranked candidates on the first response instead of forcing a follow-up query
- make `anchorQualifiedName` and `filePath` hints meaningfully affect ranking

#### Implementation Tasks

- [ ] M11-E2-T1: Return top-5 alternative candidates when `confidence` is `"ambiguous"`
  - add `"topCandidates": [...]` array to ambiguous responses
  - each entry: `{ "id", "qualifiedName", "filePath", "line", "signature", "rankScore" }`
  - cap at 5 entries
  - acceptance: ambiguous `lookup_function("Update")` response includes `topCandidates`

- [ ] M11-E2-T2: Strengthen `filePath` hint ranking weight
  - when `filePath` hint is provided, symbols defined in or near that file should rank significantly higher
  - acceptance: `lookup_function("Update", filePath="shootingsubsys.cpp")` returns a `ShotSubSystem::Update`-class result, not `WallReactionSys::Update`

- [ ] M11-E2-T3: Strengthen `anchorQualifiedName` hint propagation
  - when anchor is provided, prefer symbols in the same class, file, or subsystem as the anchor
  - acceptance: `lookup_function("Update", anchorQualifiedName="ShotSubSystem::SetShotFlags")` ranks `ShotSubSystem`-related `Update` higher

- [ ] M11-E2-T4: Add `selectedReason` field to all heuristic responses
  - explain why the top symbol was chosen
  - example: `"selectedReason": "highest file-path proximity score; anchor context partially matched"`
  - acceptance: all `lookupMode: heuristic` responses include `selectedReason`

- [ ] M11-E2-T5: Add `find_all_overloads(name)` tool
  - returns all symbols sharing a short name, grouped by qualified name
  - useful when agent needs `AddShotFlag` across all four event classes at once
  - acceptance: `find_all_overloads("AddShotFlag")` returns all 4 variants in one call

#### Validation Checklist

- [ ] Ambiguous responses include `topCandidates` (5 entries)
- [ ] `filePath` and `anchorQualifiedName` hints visibly affect which candidate is ranked first
- [ ] `selectedReason` present in heuristic responses
- [ ] `find_all_overloads` tool registered and functional

---

### M11-E3. Upstream Call Chain Traversal (Reverse Callgraph)

Priority: High

#### Background

`get_callgraph` only expands callees (downward). Tracing who calls a function
requires manually chaining `find_callers` calls:
`find_callers(A)` → `find_callers(B)` → `find_callers(C)` ...

During the ShotFlags investigation, tracing `ShotNormal::SetAutoFlags` upstream
required 3 separate `find_callers` queries. In a deeper chain this becomes impractical.

#### Goal

- support bounded upstream traversal in a single query
- symmetric with existing `get_callgraph` (callee direction)

#### Implementation Tasks

- [ ] M11-E3-T1: Add `direction` parameter to `get_callgraph`
  - values: `"callees"` (current default) | `"callers"` | `"both"`
  - `"callers"` mode: recursively expand who calls the root function up to `depth`
  - acceptance: `get_callgraph("SetAutoFlags", direction="callers", depth=3)` returns a tree rooted at `SetAutoFlags` with callers as children

- [ ] M11-E3-T2: Add `find_callers_recursive` convenience tool
  - wraps `get_callgraph(direction="callers")`
  - parameters: `name`, `depth` (default 3), standard hint parameters
  - returns same structure as `get_callgraph`
  - acceptance: `find_callers_recursive("SetAutoFlags", depth=3)` returns 3-level upstream chain

- [ ] M11-E3-T3: Add cycle detection and truncation in upstream traversal
  - mutual recursion or shared utility callers can create large graphs
  - cap total nodes at 200; mark truncated branches with `"truncated": true`
  - acceptance: upstream traversal on a known cycle terminates cleanly

- [ ] M11-E3-T4: Update `investigate_workflow` to use upstream traversal when tracing entry points
  - when `investigate_workflow` is asked to find the entry point for a symbol, use reverse callgraph internally
  - acceptance: `investigate_workflow` for `ShotNormal::SetAutoFlags` returns an upstream chain reaching the AI objective layer

#### Validation Checklist

- [ ] `get_callgraph` accepts `direction` parameter
- [ ] `find_callers_recursive` tool registered and returns multi-level chain
- [ ] Cycle detection prevents infinite expansion
- [ ] `investigate_workflow` benefits from upstream traversal internally

---

### M11-E4. Enum Member Value Usage Indexing

Priority: Medium-High

#### Background

`find_references("Gameplay::ShotFlags")` returned only 8 results (type declaration references).
Direct usage of enum member values (`SHOTFLAG_CHIP`, `SHOTFLAG_FINESSE`, etc.) via bitwise OR
in source code was not indexed. ast-grep found 110+ `shotFlags` variable usages and 19
`AddShotFlag(SHOTFLAG_*)` call sites that CodeAtlas could not surface.

This matters for any investigation involving flags, states, or bitmask enums — common in game code.

#### Goal

- index enum member values as first-class symbols
- make individual flag values queryable via `find_references`

#### Implementation Tasks

- [ ] M11-E4-T1: Index enum member values as individual symbols in the `symbols` table
  - type: `"enumMember"`
  - qualified name: `Gameplay::SHOTFLAG_CHIP`, `Gameplay::SHOTFLAG_FINESSE`, etc.
  - acceptance: `lookup_symbol("Gameplay::SHOTFLAG_CHIP")` returns a valid symbol entry

- [ ] M11-E4-T2: Index usage sites of enum member values in `symbol_references`
  - category: `"enumValueUsage"`
  - detect patterns: direct assignment, bitwise OR (`|`, `|=`), function argument
  - acceptance: `find_references("Gameplay::SHOTFLAG_CHIP")` returns call sites in `shotnormal.cpp`, `evaluateshot.cpp`, etc.

- [ ] M11-E4-T3: Add `includeEnumValueUsage: boolean` parameter to `find_references`
  - when `true`, include `enumValueUsage` references for all members of the enum type
  - default: `false` (backward compatible)
  - acceptance: `find_references("Gameplay::ShotFlags", includeEnumValueUsage=true)` returns both type-level and value-level usages

- [ ] M11-E4-T4: Validate enum member indexing on FIFA-scale project
  - run against `gameplay/interface/GameplayInterface/dev-gameplay/include/gameplay/util/util.h`
  - confirm all 32 `SHOTFLAG_*` members indexed
  - acceptance: all 32 members queryable; at least 50 usage sites surfaced across the codebase

#### Validation Checklist

- [ ] `SHOTFLAG_*` enum members appear as individual symbols
- [ ] `find_references` with `includeEnumValueUsage=true` returns value-level usages
- [ ] No significant indexing performance regression on large enum-heavy headers
- [ ] Backward compatible: existing `find_references` calls unaffected

---

### M11-E5. Large Result Inline Delivery and Response Compaction

Priority: Medium

#### Background

`list_file_symbols` for `shotnormal.cpp` (33 symbols) and `shootingsubsys.cpp` produced
responses written to disk files with the message:
`"Large tool result (39KB) written to file. Use read_file to access..."`

This forces an additional `read_file` round-trip for every large query.
One logical query becomes two network calls, doubling latency and token cost.

#### Goal

- allow agents to receive large results inline when they choose to
- add a compact summary mode for symbol lists

#### Implementation Tasks

- [ ] M11-E5-T1: Add `maxInlineBytes` parameter to `list_file_symbols`, `get_callgraph`, and `find_references`
  - when response size ≤ `maxInlineBytes`, return inline
  - when exceeded, keep current file-write behavior
  - default: current threshold (backward compatible)
  - recommended agent-facing default suggestion: 50000
  - acceptance: `list_file_symbols(filePath=..., maxInlineBytes=60000)` returns inline for shotnormal.cpp

- [ ] M11-E5-T2: Add `compact: true` mode to `list_file_symbols`
  - returns only: `id`, `name`, `qualifiedName`, `line`, `endLine`, `type`
  - omits: `signature`, `declarationFilePath`, `parseFragility`, `macroSensitivity`, `includeHeaviness`, `headerRole`
  - target: ≤ 8KB for a 33-symbol file
  - acceptance: `list_file_symbols(..., compact=true)` response is under 10KB for shotnormal.cpp

- [ ] M11-E5-T3: Add `compact: true` mode to `get_callgraph`
  - returns only: node `id`, `name`, `filePath`, `line` per node
  - omits all extended metadata
  - acceptance: compact callgraph for `SetAutoFlags` (depth=4) stays under 15KB

- [ ] M11-E5-T4: Document recommended parameter values in tool descriptions
  - update MCP tool description strings to suggest `maxInlineBytes=50000` and `compact=true` for large files
  - acceptance: tool description strings updated

#### Validation Checklist

- [ ] `list_file_symbols` with `maxInlineBytes=60000` returns inline for 33-symbol file
- [ ] `compact: true` reduces response size by at least 60%
- [ ] Existing calls without new parameters behave identically (backward compatible)

---

### M11-E6. Reliability Metadata Surfacing

Priority: Medium-Low

#### Background

Responses already include `parseFragility` and `macroSensitivity` fields, but these are
buried in per-symbol metadata without connecting to response-level trust signals.
An agent reading `"parseFragility": "elevated"` must know to be suspicious; currently
nothing in the response explicitly says "this result may be incomplete."

#### Goal

- surface per-response reliability signals at the top level
- connect existing fragility metadata to actionable guidance

#### Implementation Tasks

- [ ] M11-E6-T1: Add top-level `reliability` object to `lookup_function`, `find_callers`, `find_references`
  - structure:
    ```json
    "reliability": {
      "level": "full" | "partial" | "low",
      "factors": ["elevated_parse_fragility", "macro_sensitive"],
      "suggestion": "Cross-check call sites with ast-grep for complete coverage."
    }
  ```
  - `level` derived from: symbol's `parseFragility` + `macroSensitivity` + presence of `indexCoverage: partial`
  - `suggestion` is omitted when `level: "full"`
  - acceptance: `lookup_function("SetShotFlags")` response includes `reliability` with `level: "partial"`

- [ ] M11-E6-T2: Add `reliability` to `get_callgraph` root node
  - surface aggregate reliability across all nodes in the graph
  - if any node in the expanded graph is `parseFragility: elevated`, mark root as `partial`
  - acceptance: callgraph rooted at `SetAutoFlags` includes root-level `reliability`

- [ ] M11-E6-T3: Add reliability propagation test
  - confirm that a known elevated-fragility root symbol produces `level: "partial"`
  - confirm that a known low-fragility symbol produces `level: "full"`
  - acceptance: test passes in CI

#### Validation Checklist

- [ ] `reliability` field present in `lookup_function`, `find_callers`, `find_references` responses
- [ ] `level: "partial"` correctly identifies macro-heavy symbols
- [ ] `suggestion` field provides actionable guidance
- [ ] `level: "full"` symbols do not include a `suggestion` field

---

## 4. Acceptance Criteria Summary

| Epic | Exit Gate |
|------|-----------|
| M11-E1 | Zero-result for real callers produces `coverageWarning`; `indexCoverage` field present |
| M11-E2 | Ambiguous responses include `topCandidates`; `filePath` hint visibly affects ranking |
| M11-E3 | `get_callgraph(direction="callers")` returns upstream chain; cycle-safe |
| M11-E4 | `SHOTFLAG_*` members individually queryable; `includeEnumValueUsage` option works |
| M11-E5 | `maxInlineBytes` param eliminates mandatory file round-trip; `compact` mode ≤60% size |
| M11-E6 | `reliability` field present; `level: "partial"` fires for fragile symbols |

---

## 5. Non-Goals

- Full C++ macro expansion (out of scope; fallback heuristic only)
- User input button-to-game-action tracing across binary protocol boundaries
- Web dashboard or visual call graph rendering
- Any change to the existing `investigate_workflow` contract beyond E3 upstream integration

---

## 6. Reference

- Observed investigation session: `F:\dev\docs\MCP_Tool_Comparison_Shot_Analysis.md`
- Affected real project: `F:\dev\dev_future\client` (FIFA-scale C++ game codebase)
- Comparison baseline: ast-grep (31 queries), Serena (6 queries, all timeout), CodeAtlas (20 queries)
