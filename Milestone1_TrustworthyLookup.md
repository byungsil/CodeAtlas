# Milestone 1. Trustworthy Lookup

## 1. Objective

Make symbol lookup and relationship resolution trustworthy enough that AI agents can use CodeAtlas as a primary structural source for large C++ repositories.

This milestone focuses on:

- exact symbol targeting
- better overload and scope disambiguation
- receiver-aware method resolution
- header/source unification
- confidence and ambiguity surfacing

Success outcome:

- exact symbol lookup works
- call resolution is materially more reliable
- ambiguity is surfaced clearly

---

## 2. Recommended Order

1. M1-E1. Exact lookup contract
2. M1-E2. Storage and query support
3. M1-E3. Parser metadata enrichment
4. M1-E4. Resolver ranking improvements
5. M1-E5. Header/source unification
6. M1-E6. Confidence and ambiguity surfacing
7. M1-E7. Tool and API updates
8. M1-E8. Fixture expansion and regression tests

---

## 3. Epics

### M1-E1. Exact Lookup Contract

Goal:

- define the canonical exact identity for a symbol
- distinguish exact lookup from heuristic short-name lookup

Implementation tasks:

- define whether `id` or `qualifiedName` is the long-term canonical exact symbol key
- define MCP request/response shape for exact lookup
- define HTTP request/response shape for exact lookup
- define ambiguity behavior for legacy name-only lookup
- document lookup modes:
  - exact lookup
  - exploratory search
  - heuristic name lookup

Expected touch points:

- `docs/API_CONTRACT.md`
- `docs/AGENT_WORKFLOW.md`
- `README.md`
- `server/src/models/*`

Validation checklist:

- exact lookup contract is written before implementation begins
- legacy first-match behavior is explicitly documented as heuristic

Exit criteria:

- there is one agreed exact lookup contract for server, MCP, and storage work

---

Ticket breakdown:

#### M1-T1. Define canonical exact symbol identity

Type:

- design

Depends on:

- none

Tasks:

- decide whether `id` is the canonical exact lookup key
- decide whether `qualifiedName` is identical to `id` for now or a separate stable field
- document acceptable future divergence between `id` and `qualifiedName`
- define how exact identity behaves for:
  - namespaces
  - free functions
  - methods
  - overloaded methods
  - declaration/definition merged symbols

Decision:

- `id` is the canonical exact symbol identity for CodeAtlas.
- `qualifiedName` is the canonical human-readable exact name and, for Milestone 1, must be identical to `id`.
- Exact lookup must target `id`/`qualifiedName`, never the short `name` field.
- The short `name` field remains exploratory only and must be treated as heuristic when multiple symbols share it.
- Namespace separators remain `::`.
- For Milestone 1, overloaded callables are distinguished only when their canonical qualified identity is distinct in the index. If exact overload identity cannot yet be represented safely, CodeAtlas must treat those cases as ambiguous rather than inventing a false exact identity.
- Declaration and definition pairs must share one logical exact identity. Header/source location is metadata attached to that identity, not a reason to create separate exact symbols.
- If `id` and `qualifiedName` diverge in a future milestone, `id` remains the storage and relation key, and `qualifiedName` remains the user-facing exact lookup alias. That divergence must be introduced only with an explicit migration.

Done when:

- one written rule exists for exact symbol identity across parser, DB, and API layers

#### M1-T2. Define exact lookup MCP contract

Type:

- API design

Depends on:

- M1-T1

Tasks:

- define request fields for exact symbol lookup
- define success response shape
- define error shape for not found
- define error or warning shape for ambiguous legacy lookup
- decide whether exact lookup is added to existing tools or introduced as a new tool

Decision:

- Exact MCP lookup will be introduced as a new tool named `lookup_symbol`.
- `lookup_symbol` is the canonical MCP path for exact symbol identity lookup.
- Existing `lookup_function` and `lookup_class` remain available for backward compatibility and exploratory name-based use.
- `lookup_symbol` accepts:
  - `id?: string`
  - `qualifiedName?: string`
- At least one of `id` or `qualifiedName` is required.
- In Milestone 1, `id` and `qualifiedName` are exact-identity aliases for the same logical symbol. Clients may supply either field.
- If both `id` and `qualifiedName` are supplied, they must resolve to the same logical symbol identity. Otherwise the tool returns `BAD_REQUEST`.
- `lookup_symbol` must never fall back to short-name heuristics.
- `lookup_symbol` success payload must always include:
  - `lookupMode: "exact"`
  - `symbol`
- `lookup_symbol` may also include:
  - `callers`
  - `callees`
  - `members`
  depending on the resolved symbol kind.
- `lookup_symbol` not-found behavior:
  - `isError: true`
  - payload `{ "error": "Symbol not found", "code": "NOT_FOUND" }`
- `lookup_symbol` invalid-request behavior:
  - `isError: true`
  - payload `{ "error": "Invalid exact lookup request", "code": "BAD_REQUEST" }`
- `lookup_function` and `lookup_class` remain name-based and heuristic when duplicate short names exist.

Done when:

- MCP exact lookup contract is documented and stable enough for implementation

#### M1-T3. Define exact lookup HTTP contract

Type:

- API design

Depends on:

- M1-T1

Tasks:

- define HTTP route or query parameter strategy
- define response shape parity with MCP where practical
- define backward compatibility behavior for current name-only endpoints

Decision:

- HTTP exact lookup will be introduced as a new endpoint: `GET /symbol`.
- `GET /symbol` is the canonical HTTP path for exact symbol identity lookup.
- `GET /function/:name` and `GET /class/:name` remain available for backward compatibility and heuristic name-based lookup.
- `GET /symbol` accepts:
  - `id` query parameter, optional
  - `qualifiedName` query parameter, optional
- At least one of `id` or `qualifiedName` is required.
- In Milestone 1, `id` and `qualifiedName` are exact-identity aliases for the same logical symbol.
- If both are supplied and do not identify the same logical symbol, the endpoint returns:
  - HTTP 400
  - payload `{ "error": "Invalid exact lookup request", "code": "BAD_REQUEST" }`
- `GET /symbol` must never fall back to short-name heuristics.
- `GET /symbol` success response must always include:
  - `lookupMode: "exact"`
  - `symbol`
- `GET /symbol` may also include:
  - `callers`
  - `callees`
  - `members`
  depending on the resolved symbol kind.
- `GET /symbol` not-found behavior:
  - HTTP 404
  - payload `{ "error": "Symbol not found", "code": "NOT_FOUND" }`
- Response shape should remain close to the MCP `lookup_symbol` payload to minimize divergence across transports.
- Existing `/function/:name` and `/class/:name` endpoints remain heuristic and are not upgraded into exact identity endpoints in Milestone 1.

Done when:

- HTTP exact lookup behavior is documented

#### M1-T4. Document heuristic lookup behavior

Type:

- docs

Depends on:

- M1-T1
- M1-T2
- M1-T3

Tasks:

- update docs to label current short-name lookup as heuristic
- define how ambiguity is surfaced to callers
- add usage examples for exact vs exploratory lookup

Decision:

- Documentation now consistently labels short-name lookup as heuristic across API, README, and workflow docs.
- Heuristic ambiguity behavior is explicitly documented:
  - backward-compatible name lookup may still return one selected symbol
  - that selected symbol must not be treated as exact when duplicate short names exist
  - ambiguity is surfaced through `confidence`, `matchReasons`, and `ambiguity.candidateCount`
- Usage guidance now distinguishes three modes:
  - exploratory discovery via search
  - heuristic convenience lookup via name-based function/class endpoints or tools
  - deterministic exact lookup via canonical identity
- The docs now include an explicit progression from exploratory search to heuristic lookup to exact lookup so agents and users do not over-trust legacy first-match behavior.

Done when:

- docs no longer imply name-only lookup is exact

---

### M1-E2. Storage and Query Support

Goal:

- support exact lookup efficiently at the storage and query layer

Implementation tasks:

- add `getSymbolByQualifiedName` to the store interface
- add SQLite implementation for qualified lookup
- add JSON store parity if JSON-backed tests still matter
- optionally add `getSymbolsByQualifiedPrefix` for namespace/class browsing
- evaluate whether qualified lookup needs an additional DB index
- keep short-name lookup backward compatible

Expected touch points:

- `server/src/storage/store.ts`
- `server/src/storage/sqlite-store.ts`
- `server/src/storage/json-store.ts`

Validation checklist:

- qualified lookup returns the intended symbol when duplicate short names exist
- exact lookup remains fast on non-trivial datasets

Exit criteria:

- storage layer supports exact symbol lookup as a first-class path

---

Ticket breakdown:

#### M1-T5. Extend store interface for exact lookup

Type:

- implementation

Depends on:

- M1-T2
- M1-T3

Tasks:

- add `getSymbolByQualifiedName` to the store interface
- decide whether a prefix/container lookup helper is needed now or deferred
- update any affected type contracts

Done when:

- the server can call a first-class exact lookup method through the abstract store

#### M1-T6. Implement SQLite exact lookup path

Type:

- implementation

Depends on:

- M1-T5

Tasks:

- add qualified-name lookup query to SQLite store
- ensure query uses indexed or indexable access path
- handle not-found and duplicate-row edge cases safely

Done when:

- SQLite-backed exact lookup returns one intended symbol deterministically

#### M1-T7. Implement JSON-store parity for exact lookup

Type:

- implementation

Depends on:

- M1-T5

Tasks:

- add exact lookup behavior to JSON store
- keep parity with SQLite response semantics where possible

Done when:

- JSON-backed tests and fallback paths support exact lookup

#### M1-T8. Add or verify DB index support for exact lookup

Type:

- storage/performance

Depends on:

- M1-T6

Tasks:

- inspect current SQLite schema and indexes
- add a qualified-name index if needed
- verify index use on realistic lookup queries

Done when:

- exact lookup path remains efficient on larger symbol tables

#### M1-T9. Add storage-level tests for duplicate short names

Type:

- tests

Depends on:

- M1-T6
- M1-T7

Tasks:

- add fixtures with same short name in multiple scopes
- assert exact lookup returns the intended symbol
- assert heuristic lookup remains distinct from exact lookup

Done when:

- storage behavior is protected by deterministic tests

---

### M1-E3. Parser Metadata Enrichment

Goal:

- provide enough metadata to rank symbol candidates more intelligently

Implementation tasks:

- extend parsed symbol metadata with:
  - parameter count
  - signature text
  - containing namespace
  - containing class
  - declaration vs definition role
- extend raw call-site extraction to distinguish:
  - unqualified calls
  - `obj.method()`
  - `ptr->method()`
  - `this->method()`
  - class-qualified calls
  - namespace-qualified calls
- decide whether new fields live only in raw parse output or also in merged symbol rows

Expected touch points:

- `indexer/src/parser.rs`
- `indexer/src/models.rs`
- `server/src/parser/cpp-parser.ts` if parity is still useful for fixtures

Validation checklist:

- parser fixtures cover each call-site form
- repeated indexing produces stable metadata

Exit criteria:

- parser output contains the metadata needed for ranked resolution

---

Ticket breakdown:

#### M1-T10. Extend symbol model with disambiguation metadata

Type:

- schema/model

Depends on:

- M1-T1

Tasks:

- add fields for parameter count and normalized signature
- add containing scope metadata if not already materialized sufficiently
- add declaration/definition role metadata
- decide which fields are raw-only vs persisted

Decision:

- The canonical symbol model is extended with optional metadata fields:
  - `parameterCount`
  - `scopeQualifiedName`
  - `scopeKind`
  - `symbolRole`
- `signature` remains part of the model and continues to serve as the normalized textual signature field for Milestone 1.
- These new fields are persisted in the Rust/SQLite symbol model so later resolver work can use them without reparsing source text.
- All new fields remain optional in Milestone 1 to preserve compatibility with existing sample data and databases during migration.
- `scopeQualifiedName` represents the normalized exact container scope identity for the symbol.
- `scopeKind` is intentionally limited to:
  - `namespace`
  - `class`
  - `struct`
- `symbolRole` is intentionally limited to:
  - `declaration`
  - `definition`
  - `inline_definition`

Done when:

- symbol model can represent the minimum metadata needed for ranking

#### M1-T11. Extend raw call-site model

Type:

- schema/model

Depends on:

- M1-T10

Tasks:

- enrich raw call-site representation with call expression kind
- preserve receiver token or receiver expression kind
- distinguish class-qualified and namespace-qualified call forms

Decision:

- `RawCallSite` is extended with explicit structural fields instead of relying only on `called_name` plus a loose `receiver` string.
- The raw call-site model now includes:
  - `callKind`
  - `receiver`
  - `receiverKind`
  - `qualifier`
  - `qualifierKind`
- `callKind` is intentionally limited to:
  - `Unqualified`
  - `MemberAccess`
  - `PointerMemberAccess`
  - `ThisPointerAccess`
  - `Qualified`
- `receiver` preserves the textual receiver token or expression summary already available from the parser.
- `receiverKind` distinguishes the structural shape of the receiver expression and is intentionally limited to:
  - `Identifier`
  - `This`
  - `PointerExpression`
  - `FieldExpression`
  - `QualifiedIdentifier`
  - `Other`
- `qualifier` preserves the explicit left-side qualifier text for qualified calls such as `Gameplay::Update()`.
- `qualifierKind` is intentionally limited to:
  - `Namespace`
  - `Type`
- In `M1-T11`, `qualifierKind` may remain unset when the parser has not yet promoted a qualified call into a namespace-vs-type distinction.
- The goal of this ticket is to make the raw model able to represent the important resolution cases even before all parser extraction heuristics are complete.

Done when:

- raw call-site data can represent the important resolution cases

#### M1-T12. Parse receiver and qualified call forms

Type:

- parser implementation

Depends on:

- M1-T11

Tasks:

- add extraction for `obj.method()`
- add extraction for `ptr->method()`
- add extraction for `this->method()`
- add extraction for class-qualified calls
- add extraction for namespace-qualified calls

Decision:

- `M1-T12` uses the `RawCallSite` structure introduced in `M1-T11` as the parser output contract.
- The parser now emits distinct raw call-site forms for:
  - `obj.method()`
  - `ptr->method()`
  - `this->method()`
  - `Namespace::foo()`
  - `Type::foo()`
- `receiverKind` is populated directly from the receiver expression shape when the call uses member access syntax.
- `qualifierKind` is populated only when the parser can classify the qualifier from currently known file-local context.
- `qualifierKind = Namespace` is assigned when the qualifier matches a known namespace scope observed during the parse walk.
- `qualifierKind = Type` is assigned when the qualifier matches a known `class` or `struct` symbol already materialized in the current file parse context.
- When the parser cannot classify a qualified call safely, it keeps `qualifierKind` unset instead of inventing a type-vs-namespace distinction.
- This milestone intentionally prefers reliable structural distinction over aggressive semantic guessing.

Done when:

- parser emits structurally distinct raw call-site forms

#### M1-T13. Parse declaration/definition role metadata

Type:

- parser implementation

Depends on:

- M1-T10

Tasks:

- tag symbols as declaration, definition, or inline/header-only implementation
- verify this works for normal methods, free functions, and templates where possible

Decision:

- `symbolRole` is assigned by the parser at symbol creation time rather than deferred to merge logic.
- The parser assigns:
  - `declaration` for function and method declarations without bodies
  - `definition` for out-of-class or free-function definitions with bodies
  - `inline_definition` for method definitions that appear inline inside a class or struct body
- In Milestone 1, class, struct, and enum container symbols remain untagged for `symbolRole`; the role metadata is focused on callable merge behavior.
- Template free-function definitions follow the same role rule as ordinary free functions and are tagged as `definition`.
- Template declarations inside class bodies follow the same role rule as ordinary member declarations and inline member definitions.
- The milestone intentionally prefers simple deterministic role tagging over trying to infer linker-level semantics such as `inline` keywords or ODR nuances.

Done when:

- parser can distinguish declaration and definition roles for merge logic

#### M1-T14. Add parser regression fixtures for new metadata

Type:

- tests

Depends on:

- M1-T12
- M1-T13

Tasks:

- create parser fixtures covering each call-site form
- create fixtures covering declaration/definition split
- assert metadata stability across repeated runs

Done when:

- parser metadata behavior is locked by tests

---

### M1-E4. Resolver Ranking Improvements

Goal:

- replace simplistic first-candidate behavior with explicit ranking logic

Implementation tasks:

- refactor resolver into clear ranking stages
- add ranking signals:
  - same parent/class
  - same namespace
  - receiver-aware preference
  - parameter-count hint
  - declaration/definition preference
  - file-local proximity
- define tie-breaking rules
- define unresolved and ambiguous result behavior
- decide whether low-confidence edges are stored, filtered, or both

Expected touch points:

- `indexer/src/resolver.rs`
- `indexer/src/indexing.rs`
- `indexer/src/models.rs`

Validation checklist:

- overloaded function fixtures resolve better than current fallback logic
- ambiguous cases are not silently treated as exact

Exit criteria:

- resolver uses ranked structural evidence instead of mostly first-candidate fallback

---

Ticket breakdown:

#### M1-T15. Refactor resolver into explicit ranking stages

Type:

- implementation/refactor

Depends on:

- M1-T10
- M1-T11

Tasks:

- separate candidate collection from candidate scoring
- separate scoring from tie-breaking
- make ranking decisions inspectable in tests

Decision:

- Resolver flow is split into three explicit stages:
  - candidate collection
  - candidate scoring
  - tie-breaking
- Candidate collection remains name-based in `M1-T15`; this ticket is about structure, not yet about stronger ranking signals.
- Candidate scoring now produces an inspectable ranked candidate list with:
  - total score
  - individual scoring reasons
- Tie-breaking consumes ranked candidates instead of being embedded inside candidate collection logic.
- `same_parent` is preserved as the first explicit scoring reason so current behavior stays stable while becoming easier to extend in `M1-T16` and later tickets.
- Internal resolver tests are allowed to inspect ranking reasons directly so future ranking changes can be validated without relying only on final chosen callee IDs.

Done when:

- resolver no longer hides decision logic in ad hoc fallback branches

#### M1-T16. Implement same-parent and same-namespace ranking

Type:

- implementation

Depends on:

- M1-T15

Tasks:

- score same parent/class matches
- score same namespace matches
- define precedence between the two

Decision:

- Resolver ranking now uses two explicit structural proximity signals:
  - `same_parent`
  - `same_namespace`
- `same_parent` remains the stronger signal and is scored above `same_namespace`.
- In Milestone 1, the relative precedence is:
  - `same_parent = 100`
  - `same_namespace = 50`
- This keeps member/sibling-method disambiguation ahead of broader namespace proximity.
- Namespace proximity is derived from current symbol identity structure:
  - for methods, the namespace is inferred from the parent type identity
  - for free functions, the namespace is inferred from the callable qualified identity
- If namespace cannot be determined safely from currently available identity data, no namespace score is added.
- `M1-T16` does not yet use receiver context or parameter-count hints; those remain for later tickets.

Done when:

- sibling-class and same-namespace ambiguity cases improve measurably

#### M1-T17. Implement receiver-aware ranking

Type:

- implementation

Depends on:

- M1-T12
- M1-T15

Tasks:

- prefer containing class methods for `this->foo()`
- prefer likely member methods for member-access calls
- prefer class-qualified matches for `ClassName::foo()`

Decision:

- Resolver now uses raw call-site shape from the parser as an explicit ranking signal.
- Receiver-aware ranking adds these signals:
  - `this_receiver_match = 80`
  - `member_call_prefers_method = 30`
  - `qualified_type_match = 90`
  - `qualified_namespace_match = 70`
- In Milestone 1, these signals are additive with the existing structural proximity scores from `M1-T16`.
- Intended precedence:
  - `same_parent` remains the strongest general structural signal
  - explicit type-qualified matches should strongly favor methods on the named type
  - namespace-qualified matches should strongly favor callables in that namespace
  - generic member-access syntax should prefer methods over free functions, but with a lower weight than exact structural matches
- `this->foo()` still benefits from `same_parent`; `this_receiver_match` is added so the reason is explicit and inspectable.
- Receiver-aware ranking remains conservative:
  - it does not attempt full type inference for arbitrary receiver expressions
  - it uses only call-shape evidence already emitted by the parser
- This ticket is intentionally limited to safe structural signals; overload signature hints remain for `M1-T18`.

Done when:

- receiver context changes candidate ranking in covered cases

#### M1-T18. Implement signature and parameter-count hints

Type:

- implementation

Depends on:

- M1-T10
- M1-T15

Tasks:

- use parameter count where available
- use normalized signature as a hint when candidates remain close
- define behavior when signature evidence is partial

Decision:

- The parser now populates callable `parameterCount` and raw call-site `argumentCount` where those counts can be read structurally.
- Resolver overload hints use this precedence:
  - exact `parameterCount` match on the candidate
  - fallback arity inference from the candidate `signature`
- In Milestone 1, the scoring weights are:
  - `parameter_count_match = 60`
  - `signature_arity_hint = 40`
- Signature-based arity is used only when structured `parameterCount` is missing on the candidate.
- If the call-site `argumentCount` is unknown, no arity-based score is applied.
- If a candidate has a known but non-matching parameter count, Milestone 1 does not yet apply a negative penalty; it simply withholds the positive hint.
- This keeps overload ranking conservative while still improving cases where the arity evidence is clearly aligned.

Done when:

- overload ranking uses more than parent/name coincidence

#### M1-T19. Define unresolved and ambiguous result policy

Type:

- design/implementation

Depends on:

- M1-T15

Tasks:

- decide whether unresolved edges are dropped or persisted separately
- decide whether ambiguous edges are stored with low confidence
- ensure policy is consistent with later API surfacing

Decision:

- Resolver now distinguishes three internal outcomes:
  - `Resolved`
  - `Ambiguous`
  - `Unresolved`
- `Unresolved` means no viable candidates were collected for the raw call-site.
- `Ambiguous` means more than one candidate shares the top ranking score after scoring and tie-breaking.
- In Milestone 1, both `Ambiguous` and `Unresolved` call edges are dropped from persisted/emitted `Call` edges.
- Only `Resolved` outcomes produce stored/emitted call edges.
- This is intentionally conservative: CodeAtlas prefers omitting uncertain edges over inventing false structural certainty.
- The internal resolver decision still retains ranked candidates so ambiguity remains inspectable in tests and can later feed API confidence surfacing.
- Persisting ambiguous edges with explicit confidence is deferred to the later confidence/response work rather than introduced into storage in `M1-T19`.

Done when:

- ambiguous resolution behavior is explicit and testable

#### M1-T20. Add resolver regression suite

Type:

- tests

Depends on:

- M1-T16
- M1-T17
- M1-T18
- M1-T19

Tasks:

- add overloaded free-function cases
- add sibling-class same-method-name cases
- add global helper vs member-method cases
- add unresolved/ambiguous expectation cases

Decision:

- Resolver regression coverage is now anchored to the dedicated ambiguity fixture workspace in `samples/ambiguity/`.
- `M1-T20` locks in these behaviors with parser-plus-resolver tests:
  - sibling class methods prefer the containing class target
  - namespace-qualified duplicate short names resolve to the qualified namespace target
  - declaration/definition split member calls continue to resolve to the logical method symbol
  - same-arity overloads without enough type evidence remain ambiguous and do not emit edges
- Explicit qualified-call syntax now overrides generic same-parent locality bias during ranking.
- The regression suite intentionally checks both:
  - internal resolver decision state
  - emitted `Call` edges
- This keeps the milestone guarded against regressions where a ranking change still “chooses something” but silently reintroduces false certainty.

Done when:

- resolver ranking improvements are protected by focused tests

---

### M1-E5. Header/Source Unification

Goal:

- represent a single logical symbol consistently across declaration and definition files

Implementation tasks:

- define merge policy for:
  - declaration only
  - definition only
  - declaration plus definition
  - inline/header-only implementations
- extend schema if needed to preserve:
  - declaration file/line
  - definition file/line
  - representative file/line
- ensure references and call edges point to the logical symbol, not duplicate rows
- keep symbol IDs stable through merge behavior

Expected touch points:

- `indexer/src/resolver.rs`
- `indexer/src/storage.rs`
- `server/src/models/symbol.ts`
- `server/src/storage/sqlite-store.ts`

Validation checklist:

- `.h` declaration and `.cpp` definition merge into one logical symbol
- template/header-only implementations remain represented correctly

Exit criteria:

- agents see one symbol with consistent declaration/definition context

---

Ticket breakdown:

#### M1-T21. Define declaration/definition merge policy

Type:

- design

Depends on:

- M1-T13

Tasks:

- define merge rules for declaration-only symbols
- define merge rules for definition-only symbols
- define merge rules for declaration plus definition pairs
- define merge rules for inline/header-only implementations

Decision:

- Merge policy is defined around one logical symbol identity per canonical `id`.
- Declaration and definition variants with the same canonical `id` must collapse into one logical symbol view.
- Common lifecycle rules for Milestone 1:
  - declaration-only:
    - preserve the declaration as the representative symbol
    - `symbolRole = declaration`
    - header location remains the visible location
  - definition-only:
    - preserve the definition as the representative symbol
    - `symbolRole = definition`
    - source location remains the visible location
  - declaration + definition pair:
    - keep one logical symbol with the shared canonical `id`
    - prefer the out-of-class/source definition as the representative symbol when both are present
    - declaration context is still considered part of the same symbol lifecycle, not a duplicate symbol
  - inline/header-only implementation:
    - preserve the inline callable as the representative symbol
    - `symbolRole = inline_definition`
    - do not force a synthetic declaration/definition split when only inline code exists
- Representative symbol precedence for Milestone 1:
  - `definition` beats `declaration`
  - `inline_definition` beats `declaration`
  - when both `definition` and `inline_definition` somehow coexist for the same canonical `id`, prefer the out-of-class `definition`
- Representative file preference remains consistent with the current implementation:
  - source (`.cpp`) definitions outrank header declarations when both exist
  - if the source-side raw symbol disappears, representation falls back to the remaining header-side declaration
- The merge policy is intentionally conservative:
  - it assumes `id` equality is the precondition for merge
  - it does not attempt fuzzy merge across different canonical IDs
  - it does not create multiple logical symbols just because declaration and definition live in different files
- This policy is the contract for later schema work in `M1-T22` and merge implementation refinements in `M1-T23`.

Done when:

- there is one documented merge policy covering common C++ symbol lifecycles

#### M1-T22. Extend persisted symbol model for dual-location metadata

Type:

- schema/storage

Depends on:

- M1-T21

Tasks:

- decide whether to add declaration/definition file and line fields
- update Rust storage schema if needed
- update TypeScript symbol model if exposed to the server

Decision:

- The persisted symbol model is extended to preserve both declaration-side and definition-side location metadata.
- Added persisted fields:
  - `declarationFilePath`
  - `declarationLine`
  - `declarationEndLine`
  - `definitionFilePath`
  - `definitionLine`
  - `definitionEndLine`
- Existing representative fields remain unchanged:
  - `filePath`
  - `line`
  - `endLine`
- Milestone 1 interpretation:
  - representative location answers “what symbol view should the user see first?”
  - declaration/definition locations answer “where are the declaration and implementation lifecycle anchors?”
- Storage normalization rules in `M1-T22`:
  - a `declaration` symbol auto-populates declaration-side location fields from its representative location if those fields are absent
  - a `definition` or `inline_definition` symbol auto-populates definition-side location fields from its representative location if those fields are absent
- Full dual-location merge population across declaration/definition pairs is still refined in `M1-T23`; `M1-T22` ensures the schema and read/write paths can already preserve that data.

Done when:

- schema can preserve both declaration and definition context

#### M1-T23. Implement merge logic for header/source pairs

Type:

- implementation

Depends on:

- M1-T21
- M1-T22

Tasks:

- update merge logic to unify `.h` and `.cpp` representations
- preserve one logical symbol ID
- keep representative symbol selection stable

Decision:

- Merge logic now combines declaration-side and definition-side metadata while still producing one representative symbol per canonical `id`.
- Representative selection rules implemented in `M1-T23`:
  - `definition` outranks `inline_definition`
  - `inline_definition` outranks `declaration`
  - if roles are absent or tied, `.cpp` representatives outrank `.h` representatives
- Dual-location merge behavior:
  - declaration-side fields are preserved from declaration variants
  - definition-side fields are preserved from definition variants
  - when an out-of-class `definition` is present, it becomes the authoritative definition-side location even if an earlier inline definition existed
- Merge remains strict on identity:
  - only rows with the same canonical `id` are merged
  - merge does not attempt cross-id fuzzy pairing
- The representative symbol remains the user-facing structural view, while dual-location fields preserve lifecycle anchors for later API exposure.

Done when:

- one logical method is not duplicated just because declaration and definition live in different files

#### M1-T24. Verify reference and call-edge remapping after merge

Type:

- implementation/tests

Depends on:

- M1-T23

Tasks:

- ensure call edges target logical merged symbol IDs
- ensure future reference edges are compatible with merged symbols
- add tests for merged lookup behavior

Decision:

- In Milestone 1, merge consistency is enforced primarily through canonical ID stability rather than a separate post-merge edge remapping table.
- Call edges are considered correct after merge when:
  - they continue to reference the merged symbol's canonical `id`
  - representative symbol location can change from source to header fallback without invalidating those edges
- `M1-T24` verifies that declaration/definition representative changes do not strand call edges as dangling references as long as the logical symbol `id` remains stable.
- Future reference-edge support should follow the same contract:
  - relations must target canonical logical symbol IDs
  - representative file changes must not require relation ID rewrites
- This keeps relationship consistency anchored to the same exact-identity rule defined earlier in Milestone 1.

Done when:

- merged symbols do not break relationship consistency

---

### M1-E6. Confidence and Ambiguity Surfacing

Goal:

- make uncertainty visible instead of silently overstating precision

Implementation tasks:

- define confidence levels:
  - exact
  - high-confidence heuristic
  - ambiguous
  - unresolved
- define match-reason values such as:
  - `exact_qualified_match`
  - `same_parent_match`
  - `namespace_proximity_match`
  - `fallback_first_candidate`
- decide whether confidence is stored, computed at query time, or both
- extend response models to expose ambiguity where useful
- document structural-confidence semantics

Expected touch points:

- `server/src/models/responses.ts`
- `server/src/mcp.ts`
- `server/src/app.ts`
- `docs/API_CONTRACT.md`

Validation checklist:

- ambiguous fixtures return explicit uncertainty markers
- tests assert confidence and match reason values

Exit criteria:

- CodeAtlas can clearly communicate exact vs heuristic vs ambiguous results

---

Ticket breakdown:

#### M1-T25. Define confidence taxonomy

Type:

- design

Depends on:

- M1-T19

Tasks:

- finalize confidence levels
- define when each level is assigned
- define match-reason vocabulary

Decision:

- Milestone 1 adopts one shared structural-confidence taxonomy across resolver, storage policy, and future API surfacing.
- Confidence levels:
  - `exact`
  - `high_confidence_heuristic`
  - `ambiguous`
  - `unresolved`
- Level meanings:
  - `exact`
    - used for exact identity lookup by canonical `id` / `qualifiedName`
    - not a claim of compiler-complete semantic certainty; it means exact symbol targeting succeeded
  - `high_confidence_heuristic`
    - used when a relation or lookup result is chosen by structural ranking signals rather than exact identity input
    - requires one unique top-ranked candidate
  - `ambiguous`
    - used when multiple candidates share the top ranking score
    - no candidate should be emitted as if it were certain
  - `unresolved`
    - used when no viable candidate exists
    - no candidate should be emitted
- Resolver status mapping:
  - `ResolutionStatus::Resolved` -> `high_confidence_heuristic` for relation resolution
  - `ResolutionStatus::Ambiguous` -> `ambiguous`
  - `ResolutionStatus::Unresolved` -> `unresolved`
- Exact lookup transport mapping:
  - successful `lookup_symbol` / `GET /symbol` -> `exact`
  - exact lookup not found -> `unresolved`
- Match-reason vocabulary for Milestone 1:
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
- Reason usage rules:
  - a result may carry multiple positive reasons
  - `ambiguous_top_score` and `no_viable_candidate` are terminal status reasons, not additive ranking boosts
  - absence of an exact reason does not imply low quality by itself; it only means the result came from heuristic structural ranking
- Milestone 1 intentionally avoids introducing more granular confidence bands such as medium/low. That can be revisited later if real usage shows a need.

Done when:

- one shared confidence language exists across resolver and API layers

#### M1-T26. Decide persistence strategy for confidence data

Type:

- design

Depends on:

- M1-T25

Tasks:

- decide whether confidence lives in DB, query-time output, or both
- document tradeoffs and chosen approach

Decision:

- Milestone 1 chooses `query-time confidence derivation` as the primary strategy.
- Confidence and match reasons are not persisted as first-class DB columns in Milestone 1.
- Persisted data remains focused on:
  - canonical symbols
  - canonical call edges
  - lifecycle metadata needed to reproduce structural reasoning
- Confidence is derived at query time from:
  - exact lookup mode
  - resolver status (`Resolved`, `Ambiguous`, `Unresolved`)
  - ranked reason set already available inside resolver decision logic
- Storage policy implications:
  - only resolved call edges are persisted
  - ambiguous and unresolved outcomes remain non-persisted query-time states
  - exact lookup confidence is transport/query derived rather than stored on the symbol row
- Why this strategy was chosen:
  - avoids schema churn while the confidence model is still stabilizing
  - prevents stale confidence values when resolver heuristics change
  - keeps the DB centered on canonical structural facts instead of derived presentation metadata
  - matches the current architecture, where ranking reasons already exist transiently in resolver logic
- Tradeoffs accepted in Milestone 1:
  - confidence for persisted relationships cannot be reconstructed perfectly from the DB alone unless the query path recomputes or reattaches reasoning context
  - historical reasoning snapshots are not preserved
  - API layers must call a confidence-aware query path rather than blindly reading raw stored edges
- Deferred possibility:
  - later milestones may persist compact confidence summaries for performance or auditing, but only after the vocabulary and API contract have stabilized.

Done when:

- implementation knows where confidence data should come from

#### M1-T27. Extend response models with confidence and ambiguity

Type:

- implementation

Depends on:

- M1-T25
- M1-T26

Tasks:

- update response types for symbol lookup, callers, and callees
- add ambiguity flags or fields where appropriate
- keep payload shape compact

Decision:

- Server response models now carry a compact confidence envelope for symbol lookup results and related call references.
- `FunctionResponse` and `ClassResponse` include:
  - `lookupMode`
  - `confidence`
  - `matchReasons`
  - optional `ambiguity`
- `CallReference` includes:
  - `confidence`
  - `matchReasons`
  - optional `ambiguity`
- In Milestone 1, persisted caller/callee edges are surfaced as:
  - `confidence = high_confidence_heuristic`
  - `matchReasons = []`
  because only resolved edges are stored and richer ranked reasons are not yet query-reconstructed from storage.
- Legacy name-based lookup responses derive top-level confidence at query time:
  - one matching candidate -> `high_confidence_heuristic`
  - multiple matching candidates -> `ambiguous`
- When legacy short-name lookup sees multiple candidates but still returns one selected symbol for backward compatibility, the response also carries:
  - `matchReasons = ["ambiguous_top_score"]`
  - `ambiguity.candidateCount`
- Payload compactness rule:
  - omit `ambiguity` unless ambiguity is actually present
  - keep `matchReasons` as an array so MCP and HTTP stay shape-compatible with future exact lookup exposure in `M1-T30` and `M1-T31`

Done when:

- API models can express exact vs heuristic vs ambiguous results

#### M1-T28. Document structural-confidence semantics

Type:

- docs

Depends on:

- M1-T25
- M1-T27

Tasks:

- explain what confidence means and does not mean
- clarify that CodeAtlas is structural, not compiler-complete
- add examples of ambiguous and exact results

Decision:

- Documentation now explicitly defines confidence as structural query confidence rather than compiler-complete semantic truth.
- The docs now clarify:
  - what `exact`, `high_confidence_heuristic`, `ambiguous`, and `unresolved` mean
  - what those labels do not mean in a macro-heavy, overload-heavy, or build-sensitive C++ environment
  - how agents should behave when each confidence level appears
- Exact and ambiguous examples are now written into the contract and workflow docs so users can see the intended interpretation instead of inferring it.
- Milestone 1 messaging now consistently frames CodeAtlas as:
  - structurally useful
  - confidence-aware
  - not a compiler replacement

Done when:

- users and agents can interpret confidence markers correctly

#### M1-T29. Add confidence and ambiguity tests

Type:

- tests

Depends on:

- M1-T27

Tasks:

- assert confidence values for exact lookup fixtures
- assert ambiguity markers for heuristic duplicate-name fixtures
- assert unresolved cases are represented consistently

Decision:

- `M1-T29` locks Milestone 1 confidence semantics with both unit-level and contract-level tests.
- Exact confidence is tested at the response-builder layer so `M1-T30` and `M1-T31` can reuse the same exact metadata contract without redefining it in transport-specific tests.
- Heuristic ambiguity is tested through HTTP contract behavior using duplicate short-name fixtures/stubs.
- Unresolved behavior is tested by asserting that HTTP and MCP `NOT_FOUND` payloads stay error-shaped and do not accidentally include lookup confidence fields.
- Current coverage focus:
  - exact lookup response builder -> `lookupMode = exact`, `confidence = exact`, exact match reasons
  - heuristic unique lookup -> `high_confidence_heuristic`
  - heuristic duplicate lookup -> `ambiguous` + `ambiguous_top_score`
  - unresolved public contract -> `NOT_FOUND` without misleading confidence payload
  - persisted caller/callee references -> default `high_confidence_heuristic`

Done when:

- confidence behavior is covered by deterministic tests

---

### M1-E7. Tool and API Updates

Goal:

- expose the new exact and confidence-aware lookup behavior to users and agents

Implementation tasks:

- add exact lookup support to MCP
- decide whether to extend existing lookup tools or add a generic `lookup_symbol`
- add backward-compatible HTTP support
- update contracts and examples
- add migration notes for current users relying on name-only behavior

Expected touch points:

- `server/src/mcp.ts`
- `server/src/app.ts`
- `server/src/__tests__/mcp.test.ts`
- `server/src/__tests__/endpoints.test.ts`

Validation checklist:

- old behavior continues to function where intended
- new exact lookup path is fully tested

Exit criteria:

- agents can perform exact and heuristic lookup through official interfaces

---

Ticket breakdown:

#### M1-T30. Add exact lookup support to MCP tools

Type:

- implementation

Depends on:

- M1-T2
- M1-T6
- M1-T27

Tasks:

- implement exact lookup request handling in MCP
- preserve backward compatibility for current tool usage
- expose confidence or ambiguity information where applicable

Decision:

- MCP exact lookup is now implemented as the new `lookup_symbol` tool.
- `lookup_symbol` accepts:
  - `id?: string`
  - `qualifiedName?: string`
- Validation rules implemented in `M1-T30`:
  - if neither field is supplied -> `BAD_REQUEST`
  - if both are supplied and resolve to different logical symbols -> `BAD_REQUEST`
  - if the requested exact symbol cannot be found -> `NOT_FOUND`
- `lookup_symbol` never falls back to short-name lookup.
- `lookup_symbol` returns query-time exact confidence metadata:
  - `lookupMode = exact`
  - `confidence = exact`
  - `matchReasons = ["exact_id_match"]`, `["exact_qualified_name_match"]`, or both
- Exact lookup payload shape by symbol kind:
  - callable symbol -> `symbol`, `callers`, `callees`
  - class/struct symbol -> `symbol`, `members`
  - other symbol kinds -> `symbol`
- Existing `lookup_function` and `lookup_class` remain available and continue to expose heuristic confidence metadata for backward compatibility.

Done when:

- MCP clients can perform exact symbol lookup directly

#### M1-T31. Add exact lookup support to HTTP API

Type:

- implementation

Depends on:

- M1-T3
- M1-T6
- M1-T27

Tasks:

- implement exact lookup in HTTP route or query handling
- preserve current endpoint compatibility
- return consistent error shapes

Decision:

- HTTP exact lookup is now implemented at `GET /symbol`.
- `GET /symbol` accepts:
  - `id`
  - `qualifiedName`
- Validation rules implemented in `M1-T31`:
  - if neither query field is supplied -> HTTP `400`, `BAD_REQUEST`
  - if both are supplied and resolve to different logical symbols -> HTTP `400`, `BAD_REQUEST`
  - if the requested exact symbol cannot be found -> HTTP `404`, `NOT_FOUND`
- `GET /symbol` never falls back to short-name matching.
- `GET /symbol` returns query-time exact confidence metadata:
  - `lookupMode = exact`
  - `confidence = exact`
  - `matchReasons = ["exact_id_match"]`, `["exact_qualified_name_match"]`, or both
- Exact payload shape by symbol kind:
  - callable symbol -> `symbol`, `callers`, `callees`
  - class/struct symbol -> `symbol`, `members`
  - other symbol kinds -> `symbol`
- Existing `GET /function/:name` and `GET /class/:name` remain backward-compatible heuristic endpoints.

Done when:

- HTTP clients can perform exact symbol lookup directly

#### M1-T32. Update MCP and endpoint contract tests

Type:

- tests

Depends on:

- M1-T30
- M1-T31

Tasks:

- add MCP tests for exact lookup success and not-found behavior
- add HTTP tests for exact lookup success and ambiguity behavior
- keep legacy tests green

Decision:

- `M1-T32` treats MCP and HTTP exact lookup as one public contract surface and locks the shared semantics with transport-specific tests.
- MCP contract coverage now asserts:
  - `lookup_symbol` appears in `tools/list`
  - exact success by `id`
  - exact success by `qualifiedName`
  - exact success with both aliases supplied
  - `BAD_REQUEST` for missing arguments
  - `BAD_REQUEST` for mismatched `id` and `qualifiedName`
  - `NOT_FOUND` for unknown exact symbol
- HTTP contract coverage now asserts:
  - exact success by `id`
  - exact success by `qualifiedName`
  - exact success with both aliases supplied
  - `BAD_REQUEST` for missing query parameters
  - `BAD_REQUEST` for mismatched `id` and `qualifiedName`
  - `NOT_FOUND` for unknown exact symbol
  - heuristic duplicate-name lookup exposes `ambiguous` confidence and `ambiguity.candidateCount`
- Legacy heuristic endpoints and tools remain part of the contract test suite so exact-lookup work does not silently regress existing behavior.

Done when:

- public interfaces are covered by contract tests

#### M1-T33. Update docs and usage examples

Type:

- docs

Depends on:

- M1-T30
- M1-T31

Tasks:

- update README examples
- update workflow docs
- add migration notes for users who currently rely on short-name lookup only

Decision:

- `M1-T33` updates the user-facing docs so they reflect the now-implemented exact lookup paths instead of describing them as planned work.
- README updates:
  - `lookup_symbol` is documented as a live MCP tool, not a future addition
  - exact and heuristic lookup response semantics are summarized
  - MCP tool count reflects the new exact lookup tool
- Agent workflow updates:
  - exact lookup examples now mention `confidence = exact` and exact match reasons
  - heuristic lookup guidance now mentions `lookupMode`, `confidence`, `matchReasons`, and optional `ambiguity`
  - error handling now includes `BAD_REQUEST` for invalid exact lookup input
- Full spec updates:
  - HTTP and MCP exact lookup surfaces are listed explicitly
  - exact-vs-heuristic behavior is described as current contract, not roadmap intent
- Migration guidance for current users:
  - keep using `lookup_function` / `lookup_class` for exploratory short-name discovery
  - switch to `lookup_symbol` or `GET /symbol` when deterministic exact targeting matters

Done when:

- official docs reflect the new lookup paths and semantics

---

### M1-E8. Fixture Expansion and Regression Tests

Goal:

- lock in the new lookup guarantees with realistic ambiguity fixtures

Implementation tasks:

- create a dedicated ambiguity fixture set including:
  - duplicate short names across namespaces
  - overloaded free functions
  - same method names in sibling classes
  - declaration/definition split
  - `this->` and `ptr->` calls
- add tests at:
  - parser layer
  - resolver layer
  - storage layer
  - MCP/API contract layer

Expected touch points:

- `samples/`
- `indexer/src/*`
- `server/src/__tests__/*`

Validation checklist:

- all ambiguity fixtures have explicit expected outcomes
- no regression to silent first-match behavior in covered cases

Exit criteria:

- milestone behavior is protected by deterministic fixtures and tests

---

Ticket breakdown:

#### M1-T34. Create ambiguity fixture workspace

Type:

- fixtures

Depends on:

- none

Tasks:

- add duplicate short names across namespaces
- add overloaded free functions
- add same method names in sibling classes
- add header/source declaration-definition split
- add `this->` and `ptr->` examples

Done when:

- one dedicated fixture workspace exists for lookup ambiguity scenarios

#### M1-T35. Add parser-layer fixture tests

Type:

- tests

Depends on:

- M1-T34
- M1-T14

Tasks:

- assert metadata extraction on ambiguity fixtures
- assert call-site form extraction on ambiguity fixtures

Decision:

- Parser fixture tests now read directly from `samples/ambiguity/src/*` instead of relying only on inline synthetic snippets.
- Fixture coverage added in `M1-T35`:
  - `namespace_dupes.cpp` -> qualified namespace-call extraction
  - `overloads.h/.cpp` -> declaration arity metadata and qualified overload call arity
  - `sibling_methods.h/.cpp` -> declaration roles and `this->Run()` extraction
  - `split_update.h/.cpp` -> declaration/definition role split plus `this->` and `ptr->` calls
- The goal of these tests is to lock parser behavior on the same realistic fixture workspace already used for resolver regression coverage.

Done when:

- parser behavior is locked on the new fixture workspace

#### M1-T36. Add resolver-layer fixture tests

Type:

- tests

Depends on:

- M1-T34
- M1-T20

Tasks:

- assert ranking outcomes on ambiguity fixtures
- assert ambiguous and unresolved outcomes where expected

Decision:

- `M1-T36` is satisfied by the resolver fixture regression tests added during `M1-T20`.
- Existing resolver fixture coverage on `samples/ambiguity` includes:
  - sibling class method disambiguation
  - namespace-qualified duplicate short names
  - declaration/definition split with `this->` and pointer-member calls
  - same-arity overload ambiguity
  - explicit qualifier precedence over caller locality bias
- No additional resolver behavior change was required in `E8`; the fixture workspace is already the regression anchor for resolver ranking.

Done when:

- resolver behavior is locked on the new fixture workspace

#### M1-T37. Add storage and API fixture tests

Type:

- tests

Depends on:

- M1-T34
- M1-T32

Tasks:

- assert exact lookup through storage
- assert exact lookup through MCP
- assert exact lookup through HTTP
- assert heuristic lookup ambiguity behavior

Decision:

- `M1-T37` adds end-to-end fixture-backed contract tests on a dedicated ambiguity dataset instead of depending only on the baseline sample workspace.
- Storage coverage now includes fixture-backed duplicate-short-name exact lookup for:
  - `JsonStore`
  - `SqliteStore`
- API coverage now includes fixture-backed tests for:
  - exact HTTP lookup through `GET /symbol`
  - heuristic HTTP ambiguity through `/function/:name`
  - exact MCP lookup through `lookup_symbol`
  - heuristic MCP ambiguity through `lookup_function`
- The fixture-backed server tests use the same logical identities and file paths as `samples/ambiguity` so the transport contract is anchored to the milestone ambiguity scenarios.

Done when:

- end-to-end lookup behavior is locked by tests

#### M1-T38. Create milestone acceptance checklist

Type:

- QA/docs

Depends on:

- M1-T37

Tasks:

- create a short milestone acceptance checklist
- include exact lookup, ambiguity handling, confidence surfacing, and header/source unification checks

Decision:

- `M1-T38` adds a short release-style checklist document at `docs/Milestone1_Acceptance_Checklist.md`.
- The checklist covers:
  - exact lookup
  - ambiguity handling
  - confidence surfacing
  - header/source unification
  - fixture/test coverage expectations
- The intent is to give Milestone 1 a lightweight final verification pass before closure, without requiring a reader to reconstruct completion criteria from the full planning document.

Done when:

- milestone completion can be verified consistently before closing work

---

## 4. Suggested Ticket Execution Order

Recommended ticket sequence:

1. M1-T1
2. M1-T2
3. M1-T3
4. M1-T5
5. M1-T6
6. M1-T10
7. M1-T11
8. M1-T12
9. M1-T13
10. M1-T15
11. M1-T16
12. M1-T17
13. M1-T18
14. M1-T19
15. M1-T21
16. M1-T22
17. M1-T23
18. M1-T24
19. M1-T25
20. M1-T26
21. M1-T27
22. M1-T30
23. M1-T31
24. M1-T34
25. M1-T14
26. M1-T20
27. M1-T29
28. M1-T32
29. M1-T35
30. M1-T36
31. M1-T37
32. M1-T4
33. M1-T28
34. M1-T33
35. M1-T38

Reasoning:

- define contracts first
- add storage and parser prerequisites next
- improve resolver behavior before surfacing it to APIs
- add confidence semantics before public exposure
- lock everything with fixtures and contract tests
- finish with docs and milestone acceptance

---

## 5. Final Exit Criteria

- exact symbol lookup is available and documented
- duplicate short names no longer rely on opaque first-match behavior
- receiver and scope context improve call resolution
- header/source pairs unify into one logical symbol view
- confidence and ambiguity are visible in query responses where appropriate
