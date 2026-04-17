# CodeAtlas Development Tasks

## 1. Objective

Build a local AI code intelligence system that indexes large C++ codebases and serves structured query results to AI agents without requiring direct source-file scanning during query time.

Target architecture from the spec:

- Rust indexer using Tree-sitter
- SQLite index database
- Node.js MCP server
- File watcher for incremental indexing

---

## 2. Spec Clarifications

The source spec is high-level. The following interpretations will be used during development unless the spec is updated:

- Phase 1 (`Node MVP + JSON storage`) is a validation phase, not the final architecture.
- The final production path remains `Rust Indexer -> SQLite -> MCP Server`.
- `AI must NOT read raw source files directly` applies to runtime query flow for agents, not to the indexer itself.
- `call graph: lazy evaluation` means full graph expansion should not be precomputed if a cheaper symbol-level relationship model is sufficient for the initial release.
- `full semantic C++ analysis` is out of scope. Tree-sitter structure extraction is sufficient for the first usable version.
- Python 3 may be used for auxiliary tooling, fixture generation, validation scripts, and local developer utilities, but it is not part of the core production query/indexing architecture.
- All persisted file paths in JSON and SQLite must be stored as workspace-root-relative paths, never as machine-specific absolute paths.
- Server startup must account for local port conflicts. Port selection and fallback behavior must be configurable rather than assuming one fixed port is always free.
- Templates are in scope for structural extraction of declarations/usages where the syntax tree exposes them cleanly, but not for full semantic resolution or exact specialization binding.
- Macros are in scope as parser-tolerance inputs and optional metadata only. Early phases must not depend on accurate expansion-level understanding of preprocessor output.
- `F:\dev\dev_future\client\gameplay` is the primary real-project reference source for sample selection, but early phases must use a curated subset rather than indexing the full tree.
- The repository-owned `samples/` workspace remains the deterministic fixture set for tests and endpoint contracts; real-project files are used to choose and validate realistic sample patterns.
- A human-facing web dashboard is out of scope for the MVP and core indexing pipeline, but may be added later as an optional post-MVP capability on top of the stable query APIs.
- If a web dashboard is added later, whether it auto-launches when MCP starts must be controlled by the user-managed MCP setup configuration used to register/run the MCP server, rather than by hardcoded startup behavior or a separate app-local dashboard setting.
- Watcher startup may also be controlled through the user-managed MCP setup configuration. When enabled, MCP server startup may launch the watcher as a child process instead of requiring a separate manual watcher session.
- File watching must be scoped to the active indexed workspace only. Watchers must not monitor or react to filesystem changes outside that workspace root.
- The active workspace should be explicitly provided in MCP setup configuration. Automatic workspace inference is allowed only as a fallback, with explicit MCP configuration taking precedence.
- `.codeatlas` is the workspace-local data directory for generated index artifacts such as SQLite/JSON outputs and runtime metadata; it is not itself the source workspace.
- Environment- and policy-level values should be centralized in configuration or constants modules rather than scattered as hardcoded literals. Small local algorithmic constants may remain near the code that uses them when that keeps intent clearer.
- Indexing must support regex-based ignore rules so teams can exclude irrelevant folders/files under the workspace without moving them out of the tree.
- Ignore rules should be provided through a workspace-root `.codeatlasignore` file by default, interpreted against workspace-relative paths.

---

## 3. Delivery Strategy

Build in thin vertical slices. Do not start with performance optimization before the end-to-end indexing/query path is working.

Recommended order:

1. Define query contract and storage schema.
2. Ship a Node-based MVP with JSON-backed structured queries.
3. Replace the indexing layer with Rust + Tree-sitter.
4. Replace JSON storage with SQLite.
5. Add incremental indexing and watcher.
6. Optimize for scale and latency.

---

## 4. Phase Tasks

### Phase 0. Project Baseline

Goal:
- Establish repository structure and contracts before implementation starts.

Tasks:
- Create top-level modules/directories for `indexer`, `server`, `storage`, `docs`, and `samples` as needed.
- Define how the active workspace is resolved for indexing, MCP startup, watcher scope, and dashboard features.
- Define which values belong in centralized configuration/constants versus which remain local implementation details.
- Define how regex-based ignore patterns are configured and applied consistently across discovery, incremental indexing, and watcher flows.
- Define the `.codeatlasignore` file format, precedence rules, and workspace-relative regex matching semantics.
- Define the canonical symbol model:
  - symbol id
  - symbol name
  - symbol type
  - file path relative to workspace root
  - line number
- Define the canonical relationship model:
  - caller
  - callee
- Define the canonical file-tracking model:
  - path relative to workspace root
  - content hash
  - last indexed timestamp
- Define the role of the workspace-local `.codeatlas` directory as the generated data root under each indexed workspace.
- Write a small sample dataset and expected query outputs for local validation.
- Define a curated sample-selection list derived from `F:\dev\dev_future\client\gameplay`.
- Include fixture coverage for template syntax and macro-bearing files that must parse without destabilizing symbol extraction.
- Add a baseline validation checklist for schema and sample data consistency.

Validation:
- Unit tests:
  - Contract/schema validation tests for symbol, relationship, and file-tracking payload shapes.
  - Sample dataset fixture tests that assert required fields are present and deterministic.
  - Fixture tests that include template declarations/usages and macro-bearing files.
  - Path normalization tests that reject or rewrite absolute paths before persistence.
  - Workspace-resolution tests that assert explicit MCP configuration overrides fallback inference.
  - Configuration-loading tests for centralized operational defaults and overrides.
  - Ignore-rule tests that assert configured regex patterns exclude matching workspace paths from indexing inputs.
  - Ignore-file parsing tests for `.codeatlasignore`, including blank lines and comments.
- Behavior verification process:
  - Create/update the sample dataset.
  - Review the curated real-project reference files and confirm the local fixture still covers representative gameplay patterns.
  - Run contract validation against stored sample JSON/output fixtures.
  - Verify that all persisted `filePath` and `path` fields are workspace-root-relative.
  - Verify that `.codeatlas` is treated as workspace-local generated data and not as a second source tree.
  - Verify that operational values such as ports, limits, watcher behavior, and dashboard startup policy are sourced from centralized configuration.
  - Verify that ignored paths under the workspace are excluded deterministically from indexing inputs.
  - Verify that `.codeatlasignore` in the workspace root is loaded and applied consistently.
  - Manually compare documented expected endpoint outputs against the sample fixtures before advancing to Phase 1.

Done when:
- The repository has an agreed module layout.
- Data contracts are written down and stable enough for both indexer and MCP work.
- At least one sample query result is documented for each target endpoint.

---

### Phase 1. Node MVP With JSON Storage

Goal:
- Prove the end-to-end query flow before building the production indexer.

Tasks:
- Implement a lightweight parser path that extracts a minimal structure from a limited C++ sample set.
- Store extracted symbol/file/call data in JSON files.
- Implement MCP/HTTP endpoints:
  - `GET /function/:name`
  - `GET /class/:name`
  - `GET /search?q=`
  - `GET /callgraph/:name`
- Return structured JSON only.
- Support partial-result responses where the full result set would be expensive.
- Add basic logging and request timing.
- Add automated tests for parser extraction and HTTP response contracts.
- Validate the parser against a small curated subset selected from `F:\dev\dev_future\client\gameplay` after the fixture-based flow is stable.
- Verify that template-heavy inputs still produce stable symbol output and that macro-bearing inputs fail per-file rather than destabilizing the full run.
- Ensure JSON-backed symbol, call, and file records persist only workspace-relative paths.
- Add configurable server port selection and clear startup behavior when the configured port is already in use.

Validation:
- Unit tests:
  - Parser tests for function/class extraction on the sample C++ workspace.
  - Parser tests for template declarations, template methods, and simple template instantiations.
  - Parser-tolerance tests for files containing `#define`, `#if`, and other preprocessor directives.
  - Storage serialization tests for symbol/file/call JSON outputs.
  - Path normalization tests that assert JSON output never contains absolute file paths.
  - Endpoint tests for `GET /function/:name`, `GET /class/:name`, `GET /search?q=`, and `GET /callgraph/:name`.
  - Server startup tests for configured port binding and port-conflict handling.
- Behavior verification process:
  - Run the Node server against the sample dataset.
  - Exercise each endpoint with known sample queries.
  - Confirm the agent-facing responses are structured JSON only and that partial-result markers are explicit.
  - Verify that starting the server with an occupied port yields the configured fallback or a clear actionable error.
  - Run the parser on the curated real-project subset and record unsupported constructs before Phase 2.

Done when:
- A local agent can answer symbol and callgraph questions through the server without opening source files.
- All listed endpoints return stable JSON for the sample dataset.
- The MVP is explicitly marked as non-production storage/indexing.

Notes:
- Keep this phase narrow. Do not overbuild the parser if Rust indexer work is next.
- Do not expand the MVP target to the full `gameplay` tree. Use only the curated subset for realism checks.

---

### Phase 2. Rust Indexer

Goal:
- Replace the MVP extraction path with a scalable parser/indexer.

Tasks:
- Set up a Rust indexer project.
- Integrate Tree-sitter C++ parsing.
- Implement file discovery for `.cpp` and `.h` inputs.
- Add `.codeatlasignore`-driven regex-based ignore support for workspace-relative paths so configured folders/files are excluded before parsing.
- Extract:
  - functions
  - classes
  - file-level metadata
  - direct caller/callee relationships where syntactically detectable
- Add parallel file processing.
- Ensure the indexer can run from a workspace root path.
- Produce output in the canonical storage format first, even if SQLite integration is still in progress.
- Add automated tests for extraction correctness and file-level failure isolation.
- Add explicit coverage for template-bearing code and macro-heavy files from the curated real-project subset.

Validation:
- Unit tests:
  - Parser wrapper tests for functions, classes, and file metadata extraction.
  - Template extraction tests for class/function templates and nested template types.
  - File discovery tests for supported extensions, workspace-root traversal, `.codeatlasignore` loading, and regex-based ignore filtering.
  - Failure-isolation tests that assert one malformed or macro-hostile file does not abort the run.
- Behavior verification process:
  - Run the indexer on the sample workspace and a larger multi-file fixture.
  - Run the indexer on curated real-project files that include templates and preprocessor directives.
  - Verify that files under ignore-matched workspace paths are not parsed or persisted.
  - Inspect emitted canonical records for stable IDs, paths, and line numbers.
  - Confirm parallel execution does not change output shape or suppress per-file errors.

Done when:
- The Rust indexer can process a real workspace and emit stable symbol/call/file records.
- Multi-file indexing works correctly.
- Extraction failures on unsupported syntax do not abort the whole run.

Notes:
- Favor predictable structural extraction over aggressive semantic guesses.
- Avoid whole-program semantic resolution in this phase.

---

### Phase 3. SQLite Storage

Goal:
- Move from JSON-backed storage to the target database layer.

Tasks:
- Create SQLite schema for:
  - `symbols`
  - `calls`
  - `files`
- Add indexes for the expected hot queries:
  - symbol name lookups
  - file path lookups
  - caller/callee relationship lookups
- Evaluate a search-specific index strategy for `search_symbols` so large SQLite datasets do not depend on `LIKE '%...%'` full scans.
- Implement Rust-side write path into SQLite.
- Implement Node-side read/query path from SQLite.
- Keep response shapes compatible with the MVP API contract.
- Add automated tests for schema integrity and query compatibility with the MVP contract.
- Preserve workspace-relative path storage in all SQLite tables and API responses.

Validation:
- Unit tests:
  - Schema migration tests for table/index creation.
  - Repository/query tests for symbol, search, class, and call lookups.
  - Compatibility tests that compare SQLite-backed responses with the Phase 1 JSON contract.
  - Persistence tests that assert `symbols.file_path`, `calls.file_path`, and `files.path` contain only workspace-relative paths.
  - Search-index tests that assert large-result symbol search avoids unbounded full-table behavior while preserving the API contract.
- Behavior verification process:
  - Build a fresh SQLite database from the sample dataset.
  - Run endpoint queries against SQLite-backed reads.
  - Inspect stored rows directly to confirm no absolute machine-local paths are written.
  - Measure symbol search latency on a larger dataset and verify the chosen index strategy preserves expected search semantics.
  - Verify that response payloads remain contract-compatible while query latency improves directionally.

Done when:
- JSON storage is no longer required for the normal development path.
- The server answers the existing endpoints from SQLite.
- Symbol and search queries are measurably faster and more stable than the JSON MVP path.

Notes:
- Response contract stability matters more than internal storage refactoring.

---

### Phase 4. Incremental Indexing

Goal:
- Re-index only what changed.

Tasks:
- Implement file hashing.
- Compare current file state against stored file records.
- Re-index changed files only.
- Remove or invalidate records for deleted files.
- Ensure symbol and call relationships are updated safely when a file changes.
- Add a CLI or service command for incremental refresh.
- Ensure incremental planning respects `.codeatlasignore` rules for newly added, changed, and deleted paths.
- Remove designs that require loading the full symbols/calls dataset into memory during each incremental update.
- Move toward DB-assisted incremental relationship refresh so changed files update affected call data without whole-database materialization.
- Add automated tests for unchanged, changed, and deleted file flows.

Validation:
- Unit tests:
  - Hash comparison tests for unchanged vs modified content.
  - Incremental planner tests for changed/new/deleted file classification.
  - Ignore-rule tests that assert ignored paths never enter the incremental work queue.
  - Relationship invalidation tests for symbol/call updates after file edits.
  - Incremental-memory tests that assert update logic does not require full symbols/calls loading for normal changed-file flows.
- Behavior verification process:
  - Run a full index pass, then rerun without changes and confirm near-zero useful work.
  - Modify one fixture file and verify only affected records are rewritten.
  - Add or modify files under ignore-matched paths and confirm no indexing work is scheduled for them.
  - Delete one fixture file and confirm stale symbols/calls are removed.
  - Measure incremental update memory and verify changed-file refresh scales with affected scope rather than total database size.

Done when:
- A second indexing run on unchanged input performs near-zero useful work.
- Single-file edits update only affected records.
- Deleted-file cleanup does not leave stale symbol entries behind.

Notes:
- Relationship invalidation must be correct before optimizing for speed.

---

### Phase 5. Watcher

Goal:
- Trigger incremental indexing automatically from filesystem events.

Tasks:
- Add file change monitoring for supported source file extensions.
- Restrict watcher scope to the active workspace root and ignore events from paths outside the indexed workspace.
- Apply the same `.codeatlasignore` rules to watcher events so ignored workspace paths never schedule indexing work.
- Debounce noisy event bursts.
- Route change events through the incremental indexing path.
- Log queued, skipped, and completed updates.
- Ensure watcher failures do not corrupt the database.
- Add support for watcher startup through the MCP setup configuration so MCP server launch can optionally start the watcher as a child process.
- If an existing valid index database is present, start watcher mode from an incremental catch-up path instead of always forcing a full rebuild.
- Add automated tests for debounce behavior and event-to-index routing.

Validation:
- Unit tests:
  - Watch event normalization tests for create/change/delete flows.
  - Scope tests that assert out-of-workspace filesystem events are ignored.
  - Ignore-filter tests that assert events from regex-ignored workspace paths are ignored.
  - Debounce tests for repeated save bursts.
  - Queueing tests for safe handoff into incremental indexing.
  - Process-management tests for MCP-configured watcher child-process startup and shutdown behavior.
  - Startup-mode tests that assert watcher uses incremental startup when an existing valid DB is present.
- Behavior verification process:
  - Run watch mode on the sample workspace.
  - Start the MCP server with watcher startup enabled in MCP configuration and confirm the watcher is launched as a child process.
  - Save the same file repeatedly and confirm redundant full re-index work is avoided.
  - Trigger file events under ignore-matched workspace paths and confirm no indexing work is scheduled.
  - Trigger or simulate filesystem events outside the indexed workspace and confirm no indexing work is scheduled.
  - Stop the MCP server and confirm watcher child-process lifecycle is handled cleanly.
  - Simulate watcher errors and verify the database remains readable and recoverable.
  - Verify watcher startup latency remains practical when a reusable index database already exists.

Done when:
- Editing a tracked file triggers a safe incremental update.
- Repeated save events do not cause redundant full work.
- Watcher startup can be enabled from MCP setup configuration without requiring a separate manual launch path.
- Filesystem changes outside the active workspace do not trigger indexing work.
- Existing valid index data can be reused at watcher startup without forcing an unnecessary full rebuild.
- Watch mode is stable enough for normal local development.

---

### Phase 6. Query Performance And Scale

Goal:
- Meet the operational requirements from the spec.

Tasks:
- Benchmark indexing and query performance on larger datasets.
- Identify hot queries and add missing DB indexes or query-path optimizations.
- Reduce memory spikes during large indexing runs.
- Confirm partial-result behavior for expensive searches.
- Add guardrails for pathological queries.
- Add automated performance regression checks for core query paths.
- Benchmark search latency, incremental update memory, and watcher startup time on large datasets before making scale claims.

Validation:
- Unit tests:
  - Query planner/repository tests for partial-result and guardrail behavior.
  - Regression tests for pathological query handling and bounded response size.
- Behavior verification process:
  - Run benchmark suites on a representative larger dataset.
  - Capture indexing time, common query latency, and memory snapshots before/after optimizations.
  - Record dedicated measurements for `search_symbols`, incremental changed-file updates, and watcher startup with/without an existing DB.
  - Verify optimizations do not change API response shape or partial-result semantics.

Done when:
- Query latency for common lookups is within the spec target directionally and can be measured.
- The system handles large repository input without obvious architecture breaks.
- Memory behavior is bounded enough for continued large-scale testing.

Notes:
- Do not claim `100K+ files` support without benchmark evidence.

---

### Phase 7. AI Workflow Integration

Goal:
- Make the system usable by AI agents as the default structured code-query backend.

Tasks:
- Finalize MCP-facing query surface and payload shapes.
- Document agent usage flow:
  - query MCP
  - receive structured data
  - reason from structured data
- Add example prompts/workflows for impact analysis, symbol lookup, and callgraph inspection.
- Define failure behavior when results are partial, missing, or stale.
- Add automated contract tests for final MCP payloads and integration examples.

Validation:
- Unit tests:
  - MCP payload contract tests for each supported query surface.
  - Error-shape tests for partial, missing, and stale-result cases.
  - Integration fixture tests for documented example workflows.
- Behavior verification process:
  - Execute the documented agent workflows against the live local service.
  - Confirm agents can complete symbol lookup, impact analysis, and callgraph inspection without raw file reads.
  - Verify failure responses are explicit and actionable for agent-side handling.

Done when:
- An AI agent can perform common code intelligence tasks through structured queries only.
- Integration docs are sufficient for local agent adoption.

---

### Phase 8. Optional Web Dashboard

Goal:
- Add a human-facing browser UI on top of the existing query APIs without changing the core indexing/storage architecture.

Tasks:
- Define the minimum dashboard scope:
  - symbol lookup
  - class/function detail view
  - search results
  - callgraph inspection
- Reuse the existing HTTP/MCP-backed query contracts instead of introducing a separate data path.
- Add a lightweight web server route or separate frontend package for dashboard delivery.
- Add support for dashboard startup behavior in the MCP setup configuration used to launch the MCP server, including whether the dashboard should auto-open on the first MCP launch.
- Add MCP setup configuration for dashboard/server port selection so dashboard launch does not collide with an already occupied local port.
- Design the UI for local developer workflows first, not public deployment.
- Keep dashboard state and rendering decoupled from index generation/storage concerns.
- Add automated tests for dashboard query integration and basic rendering flows.

Validation:
- Unit tests:
  - API client tests for dashboard-side query usage.
  - Rendering tests for symbol detail, search, and callgraph views.
  - Contract tests that assert the dashboard consumes the same response shapes exposed to agents.
  - Configuration tests that assert dashboard auto-launch behavior follows the MCP setup configuration used by the client/runtime.
  - Port configuration tests that assert dashboard startup behavior handles occupied ports predictably.
- Behavior verification process:
  - Launch the dashboard against a local indexed sample workspace.
  - Verify that first-run dashboard auto-open happens only when enabled in MCP configuration.
  - Verify that dashboard launch respects MCP-configured ports and responds predictably when the requested port is already occupied.
  - Confirm symbol lookup, search, and callgraph navigation work without direct source scanning in the UI layer.
  - Verify the dashboard remains functional when results are partial, missing, or stale.

Done when:
- A developer can inspect indexed symbols and call relationships from a browser UI.
- The dashboard uses the existing structured query backend rather than bypassing it.
- Dashboard auto-launch behavior is configurable through the MCP setup configuration and does not require code changes.
- Adding the dashboard does not change the core API contracts or indexing pipeline responsibilities.

Notes:
- This phase is optional and should start only after the structured query backend is stable enough to support UI consumption.

---

## 5. Cross-Cutting Requirements

These apply to all phases:

- Keep source scanning inside the indexer only.
- Keep API responses structured and deterministic.
- Keep persisted and returned file paths workspace-root-relative across JSON, SQLite, and API payloads.
- Keep server and dashboard startup behavior resilient to local port conflicts, with configuration-driven binding and clear failure reporting.
- Keep optional watcher startup driven by MCP setup configuration, with predictable child-process lifecycle management from the MCP server.
- Keep watcher scope constrained to the active workspace root and reject out-of-workspace filesystem events.
- Keep workspace resolution explicit in MCP setup whenever possible, with fallback inference only when configuration is absent and with deterministic precedence rules.
- Keep operational defaults and shared limits centralized, and avoid scattering policy-level magic numbers across the codebase.
- Keep `.codeatlasignore`-driven ignore behavior consistent across full indexing, incremental indexing, and watcher-triggered updates.
- Avoid performance-critical designs that require whole-database scans or full in-memory materialization on hot paths when a bounded or indexed alternative is available.
- Preserve compatibility of endpoint response shapes once exposed.
- Prefer incremental-safe designs over fast-but-rebuild-heavy shortcuts.
- Measure performance before claiming optimization success.
- Fail per file where possible, not per repository.

---

## 6. Validation Checklist

- Can the system answer `function`, `class`, `search`, and `callgraph` queries without AI-side raw file reads?
- Can the indexer run against a workspace root path repeatedly?
- Does unchanged input avoid unnecessary re-indexing?
- Are deleted or renamed files reflected correctly in storage?
- Are query responses fast enough to remain interactive?
- Are partial results explicit rather than silently truncated?

---

## 7. Open Questions

These should be resolved early because they affect implementation shape:

- Will the MCP server be exposed as HTTP endpoints only, or as a strict MCP transport with tool/resource semantics?
- What is the minimum acceptable accuracy for caller/callee extraction in the presence of overloads, macros, and templates?
- How much macro metadata, if any, should be surfaced explicitly in the stored model before a real preprocessor-integrated pipeline exists?
- Should namespaces, methods, and free functions share one symbol table with a type discriminator, or separate query paths?
- What exact response format should `/callgraph/:name` use for partial or lazy-expanded results?
- What benchmark dataset will be used to validate the `100K+ files` requirement?

---

## 8. Recommended First Execution Slice

Start with the smallest vertical slice that proves the architecture:

1. Define JSON response contracts for the four endpoints.
2. Index a tiny sample C++ workspace.
3. Serve those results from a Node MVP.
4. Replace the extractor with Rust + Tree-sitter.
5. Replace JSON storage with SQLite without changing the external API.

This keeps risk low and prevents premature optimization before the query model is proven.
