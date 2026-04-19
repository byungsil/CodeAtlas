# Milestone 8. Multi-Language Workspace Foundation

Status:

- Completed

## 1. Objective

Extend CodeAtlas from a C++-first intelligence engine into a practical mixed-workspace tool that can also reason about the most common companion languages found around large native codebases.

This milestone focuses on:

- a shared language-capability contract
- a common symbol and relation surface across languages
- first-release support for:
  - Lua
  - Python
  - TypeScript
  - Rust
- language-aware routing so existing query surfaces remain usable in mixed repositories
- explicit capability and confidence boundaries per language

Success outcome:

- agents can use the same structured lookup, caller, reference, impact, and overview workflows across mixed-language repositories instead of falling back to raw file reading whenever the workspace is not pure C++

Positioning note:

- this milestone does not redefine CodeAtlas as a universal language platform
- it extends the existing C/C++-first architecture into a small, deliberate language set that commonly appears beside large native systems
- the first goal is shared structure and navigation, not full parity with the current C++ propagation depth
- this milestone should preserve the product's original reason to exist:
  - very large repositories must remain tractable
  - agent-facing answers must stay structured and easy to use
  - multi-language support is only valuable if it helps agents follow real workspace flow, ownership, and impact across boundaries they already struggle with

Guiding philosophy for this milestone:

- optimize for mixed-workspace usefulness, not broad language-count marketing
- keep indexing and query behavior realistic for very large repositories
- prioritize language features that help agents answer flow and impact questions
- expose confidence and capability boundaries explicitly per language
- avoid language support that looks impressive but does not materially improve agent behavior

Mixed-workspace assumption:

- Milestone 8 assumes that a real repository may contain C++, Lua, Python, TypeScript, and Rust at the same time
- this is the normal target scenario, not an edge case
- language support must therefore converge into one shared indexing and query experience instead of behaving like separate per-language products
- each file should route to its language adapter, while stored symbols, relations, and query surfaces remain part of one language-aware workspace model
- when architectural or flow questions cross language boundaries, CodeAtlas should preserve the shared workflow and surface explicit boundary notes whenever direct structural continuity cannot be recovered

Format boundary note:

- first-release native support includes `.c` together with the existing C/C++ family because large native repositories commonly mix C and C++
- XML is intentionally out of scope for Milestone 8 first release
- XML may be reconsidered later only if it proves to be a high-value structural boundary format for real architecture or flow questions rather than a generic configuration artifact

---

## 2. Recommended Order

1. M8-E1. Shared multi-language capability model
2. M8-E2. Language adapter interface and storage generalization
3. M8-E3. Lua thin-slice support
4. M8-E4. Python thin-slice support
5. M8-E5. TypeScript thin-slice support
6. M8-E6. Rust thin-slice support
7. M8-E7. Mixed-workspace query integration and language-aware response shaping

Recommended language order:

1. Lua
2. Python
3. TypeScript
4. Rust

Reasoning:

- Lua is especially valuable for game and embedded scripting workflows, where structural navigation and require-call visibility can unlock immediate agent usefulness
- Python and TypeScript most often appear as tooling, automation, UI, and glue layers around large C++ systems, so they remain the next two practical expansion targets
- Rust is strategically useful for systems tooling and for dogfooding the architecture on a modern systems language, but it can safely follow after the higher-frequency workspace companion languages
- this order reflects likely agent pain points in large native-code repositories, not a generic popularity ranking

---

## 3. Epics

### M8-E1. Shared Multi-Language Capability Model

Goal:

- define what "language support" means in CodeAtlas before any per-language parser work starts

Status:

- Completed

Implementation tasks:

- define the shared first-release capability matrix:
  - exact lookup
  - short-name or fuzzy search
  - direct callers
  - generalized references
  - impact analysis
  - file/module overview
- define which of those capabilities are required specifically because they improve agent behavior on architecture and flow questions in mixed workspaces
- define which capabilities are required for all first-release languages
- define which capabilities remain C++-only for now:
  - advanced propagation depth
  - build-metadata-driven refinement
  - macro/include risk semantics
- define language metadata fields such as:
  - `language`
  - `languageVersionHint` if ever needed
  - `symbolRole`
  - `modulePath`
  - `exportVisibility` where meaningful
- document language-specific confidence boundaries instead of pretending all languages have equal semantic depth

Expected touch points:

- `dev_docs/API_CONTRACT.md`
- `indexer/src/models.rs`
- `server/src/models/*`

Validation checklist:

- there is a written capability matrix for C++, Lua, Python, TypeScript, and Rust
- the first-release contract makes clear what stays shared and what remains language-specific

Completion summary:

- The shared first-release capability matrix is now defined in `dev_docs/API_CONTRACT.md`.
- The contract distinguishes:
  - capabilities required for all first-release languages
  - C++-only advanced capabilities that remain intentionally language-specific
  - language metadata and confidence boundaries that agents can rely on
- Multi-language support is now defined in product terms that match CodeAtlas's reason to exist:
  - large repositories remain the priority
  - structured agent-usable answers remain the goal
  - added languages must improve real architecture and flow questions, not just widen the language count

Exit criteria:

- CodeAtlas has a stable multi-language contract that future adapters can implement without redefining the API every time
- that contract is explicitly shaped around what agents actually use in large mixed-language repositories

---

### M8-E2. Language Adapter Interface and Storage Generalization

Goal:

- make the indexer and query layers capable of accepting more than one language without forking the product into separate systems

Status:

- Completed

Implementation tasks:

- define a parser or extractor adapter interface for language-specific frontends
- decide how discovery maps file extensions to language handlers
- generalize storage and query assumptions that currently imply C++:
  - symbol types
  - call extraction origin
  - module/container naming
  - file metadata
- preserve current C++ behavior while making shared queries language-aware
- ensure the generalized path does not significantly regress the large-repository indexing and query behavior that motivated CodeAtlas in the first place
- add language-aware fixture loading and test helpers

Expected touch points:

- `indexer/src/discovery.rs`
- `indexer/src/indexing.rs`
- `indexer/src/models.rs`
- `indexer/src/storage.rs`
- `server/src/storage/*`

Validation checklist:

- one language-independent indexing path can ingest at least a mock second language without special-case query code
- existing C++ tests continue to pass unchanged

Completion summary:

- The indexer now has an explicit `SourceLanguage` model and generic source discovery APIs that can classify files by language without changing current C++ behavior.
- A language-adapter seam now exists in `indexer/src/indexing.rs`:
  - `LanguageAdapter`
  - `LanguageRegistry`
  - generic parse paths for discovered files
- Current production behavior still registers only the C++ adapter by default, so large-repository C++ workflows do not regress while the multi-language seam is being prepared.
- Regression coverage now includes:
  - mixed-language discovery classification
  - mock non-C++ adapter ingestion through the shared indexing path
  - unchanged C++ indexing behavior through the existing test suite

Exit criteria:

- CodeAtlas has a stable adapter seam for language-specific extraction and a shared storage/query surface that no longer assumes every indexed file is C++
- the multi-language seam does not weaken the product's large-workspace operating posture

---

### M8-E3. Lua Thin-Slice Support

Goal:

- support lightweight scripting workflows that commonly appear in game and embedded environments

Status:

- Completed

Implementation tasks:

- add discovery for:
  - `.lua`
- parse and index:
  - modules
  - functions
  - table-attached functions where structurally clear
  - `require(...)` relations
  - simple direct calls
- define a deliberately small first-release Lua model:
  - module-level functions
  - table or namespace-like members where syntactically obvious
  - require graph
- prioritize the Lua patterns that help agents follow gameplay or runtime script flow instead of broad dynamic-language cleverness
- keep dynamic runtime constructs explicit as out of scope:
  - metatable-driven resolution
  - dynamic global mutation
  - runtime code generation

Expected touch points:

- language-specific parser files under `indexer/src/`
- `indexer/src/discovery.rs`
- `samples/`
- `server/src/storage/*`

Validation checklist:

- fixtures cover:
  - `require`
  - module functions
  - table-member calls
- unsupported dynamic cases degrade honestly instead of pretending exactness

Completion summary:

- Lua file discovery is now active through the shared language registry and source discovery path.
- First-release Lua parsing now extracts:
  - module symbols
  - module-level functions
  - table-attached functions
  - `require(...)` relations as `moduleImport`
  - simple direct calls
- Existing shared query surfaces can now work with Lua-produced symbols and relations through the common storage model.
- Dynamic Lua semantics remain explicitly out of scope:
  - metatables
  - dynamic globals
  - runtime-generated code

Exit criteria:

- Lua becomes usable for structure, require relations, and direct navigation in mixed repositories
- agents can follow the script-side half of mixed native/script flows more reliably than raw grep or blind file reading

---

### M8-E4. Python Thin-Slice Support

Goal:

- add a first broadly useful scripting and automation language whose structure and imports are immediately valuable in mixed repositories

Status:

- Completed

Implementation tasks:

- add discovery for:
  - `.py`
- parse and index:
  - module-level functions
  - classes
  - methods
  - imports
  - simple direct calls
- support first-release Python relation categories:
  - import usage
  - direct call
  - type or base-class mention where cheap and structurally reliable
- expose Python symbols through the existing lookup/search/caller/reference/impact/overview surfaces
- prioritize the Python patterns most likely to appear in tooling, build orchestration, tests, data pipelines, and automation around large native codebases
- mark unsupported Python semantics honestly:
  - monkey patching
  - heavy dynamic dispatch
  - reflection-driven import behavior

Expected touch points:

- language-specific parser files under `indexer/src/`
- `indexer/src/discovery.rs`
- `samples/`
- `server/src/storage/*`

Validation checklist:

- fixtures cover:
  - module imports
  - free functions
  - classes and methods
  - simple caller and reference queries

Exit criteria:

- agents can navigate Python structure and direct relations using the same core query model as C++
- Python support materially reduces fallback-to-raw-code behavior in mixed tooling and automation areas

Completion summary:

- Python file discovery is now active through the shared language registry and source discovery path.
- First-release Python parsing now extracts:
  - module symbols
  - free functions
  - classes
  - methods
  - import relations as `moduleImport`
  - direct calls for:
    - unqualified local calls
    - imported-call aliases
    - simple `self.method()` calls
- Cheap structural inheritance mentions are now emitted for syntactically obvious base classes.
- Dynamic Python semantics remain explicitly out of scope:
  - monkey patching
  - reflection-driven imports
  - heavy dynamic dispatch

---

### M8-E5. TypeScript Thin-Slice Support

Goal:

- support the most common UI, tooling, and service-layer language that often appears beside C++ backends and engines

Status:

- Completed

Implementation tasks:

- add discovery for:
  - `.ts`
  - `.tsx`
  - optionally `.js` and `.jsx` only if the same thin slice stays manageable
- parse and index:
  - exported functions
  - classes
  - methods
  - interfaces where useful
  - import and export relations
  - simple direct calls
- add response metadata that helps agents tell:
  - runtime code
  - UI code
  - test code
  - script/tooling code
- keep the first release structural and bounded:
  - avoid promising full typechecker-grade semantics
  - avoid module-resolution certainty beyond what the chosen parser can support cheaply
- prioritize the TS patterns that help agents understand UI/tooling boundaries and cross-module impact in mixed repositories

Expected touch points:

- language-specific parser files under `indexer/src/`
- `indexer/src/discovery.rs`
- `server/src/storage/*`
- `samples/`

Validation checklist:

- fixtures cover import/export chains, class methods, and UI-oriented modules
- agents can use search, callers, references, and file overview without reading raw TS files first

Exit criteria:

- TypeScript becomes a first-class structured language in CodeAtlas for navigation and impact-style workflows
- agents can use TS support to answer mixed backend/tooling/UI architecture questions with less raw file inspection

Completion summary:

- TypeScript and TSX file discovery now flow through the shared language registry and common indexing path.
- First-release TypeScript parsing now extracts:
  - module symbols
  - exported and non-exported free functions
  - classes
  - interfaces
  - methods
  - import relations as `moduleImport`
  - direct calls for:
    - unqualified local calls
    - imported-call aliases
    - namespace-import member calls
    - simple `this.method()` calls
- Cheap structural inheritance mentions are now emitted for `extends` clauses on classes and interfaces.
- TypeScript remains intentionally bounded:
  - no typechecker-grade semantics
  - no broad runtime-dynamic resolution claims

---

### M8-E6. Rust Thin-Slice Support

Goal:

- support systems tooling and backend code in a language that is both strategically common and useful for dogfooding CodeAtlas's own architecture assumptions

Status:

- Completed

Implementation tasks:

- add discovery for:
  - `.rs`
- parse and index:
  - modules
  - functions
  - structs
  - enums
  - traits
  - impl methods
  - `use` relations
  - simple direct calls
- add first-release Rust hierarchy-like reasoning for:
  - trait declarations
  - impl blocks
  - impl method attachment
- keep non-goals explicit:
  - macro expansion semantics
  - trait resolution parity with `rustc`
  - full type inference
- focus first on the Rust structures that help agents reason about architecture, ownership boundaries, and direct call/reference relationships

Expected touch points:

- language-specific parser files under `indexer/src/`
- `indexer/src/models.rs`
- `samples/`
- `server/src/storage/*`

Validation checklist:

- fixtures cover module trees, trait/impl structure, and direct-call behavior
- overview queries can browse Rust modules and impl-attached methods in a usable way

Exit criteria:

- Rust structure and direct relations are queryable with the same shared navigation surface as other supported languages
- Rust support improves real mixed-workspace navigation rather than existing only as a checkbox language

Completion summary:

- Rust file discovery is now active through the shared language registry and common indexing path.
- First-release Rust parsing now extracts:
  - module symbols
  - functions
  - structs
  - enums
  - traits
  - impl-attached methods
  - `use` relations as `moduleImport`
  - direct calls for:
    - unqualified local calls
    - `self.method()` calls
    - path-qualified calls such as `foo::bar()`
- Cheap hierarchy-like structure is now surfaced through:
  - trait declarations
  - impl block attachment
  - `impl Trait for Type` as `inheritanceMention`
- Rust remains intentionally bounded:
  - no macro expansion semantics
  - no `rustc`-grade trait resolution
  - no full type inference

---

### M8-E7. Mixed-Workspace Query Integration and Language-Aware Response Shaping

Goal:

- make the existing query experience feel coherent in mixed-language repositories instead of like separate products stitched together

Status:

- Completed

Implementation tasks:

- add language fields and optional language filters to shared query responses
- ensure search and overview queries can group or filter by language
- add workspace-level summaries for:
  - language distribution
  - per-language symbol counts
  - per-language file counts
- decide how impact and trace queries behave when they encounter cross-language gaps:
  - stop with explicit boundary notes
  - continue only when direct shared relations exist
- ensure the response shape helps the agent understand when it has crossed a language boundary in an architectural flow
- document how the agent should interpret mixed-language results and language-specific confidence limits

Expected touch points:

- `server/src/models/responses.ts`
- `server/src/app.ts`
- `server/src/mcp-runtime.ts`
- `dev_docs/API_CONTRACT.md`
- `README.md`

Validation checklist:

- mixed fixtures with at least two languages can be queried without ambiguous response shape changes
- the same MCP workflow remains usable with language-aware filters and summary fields

Exit criteria:

- CodeAtlas can present a coherent mixed-workspace navigation experience across its supported first-release languages
- agents can follow architecture and responsibility boundaries across languages without losing the structured query workflow

Completion summary:

- Shared query surfaces are now language-aware without forking the product into per-language APIs.
- Search, callers, references, and impact responses now support:
  - optional `language` filtering
  - grouped language summaries
- A workspace-level mixed-language summary surface now exists through:
  - HTTP `GET /workspace-summary`
  - MCP `workspace_summary`
- Server-side storage paths now derive language from indexed file paths so:
  - existing SQLite and JSON data remain usable
  - mixed-language summaries can be surfaced without a schema reset
- Existing C++ workflows remain intact while mixed-language repositories now receive a coherent shared response model.

---

## 4. Final Exit Criteria

- a shared capability contract exists for C++, Lua, Python, TypeScript, and Rust
- the indexer has a stable language-adapter seam instead of hard-coding C++ assumptions end to end
- Lua, Python, TypeScript, and Rust each support a first-release structural thin slice:
  - lookup
  - search
  - callers
  - references
  - impact
  - overview
- mixed-workspace queries can filter and summarize by language without breaking existing C++ workflows
- unsupported dynamic or compiler-grade semantics are surfaced as explicit limits, not implied certainty
- the product still feels like a large-repository intelligence engine, not a diluted generic multi-language demo

Milestone completion target:

- CodeAtlas is no longer only a large-C++ intelligence tool
- it becomes a practical structured workspace tool for real mixed-language repositories while preserving its large-codebase, agent-usable, C++-first depth and identity

Final completion note:

- Milestone 8 is complete.
- CodeAtlas now supports a first-release mixed-workspace model across:
  - C/C++ family
  - Lua
  - Python
  - TypeScript
  - Rust
- The shared query workflow remains stable while responses can now expose language-aware filtering, grouping, and workspace summaries for real mixed-language repositories.
