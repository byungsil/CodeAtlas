# Milestone 20: Implementation Checklist & Progress Tracking

**Status**: Planning Complete → Ready for Implementation  
**Target Start Date**: Next session (after review and approval of this plan)  

## Pre-Implementation Prerequisites ✅/❌

| Item | Status | Notes |
|------|--------|-------|
| [ ] Review Milestone20_SemgrepStyleStaticAnalysis.md with team | ⬜ Pending | Confirm architecture direction before coding starts |  
| [ ] Validate database schema extensions for backward compatibility | ⬜ Pending | Ensure existing indexes/queries won't break during migration |
| [ ] Set up test workspace with representative C++/Python projects | ⬜ Pending | Need sample code covering common patterns (factory, observer) + anti-patterns for validation |

## Phase 1: Enhanced Type Inference — Implementation Checklist

### Database Schema Migration
- [ ] Add `symbol_type_inferences` table to storage.rs  
- [ ] Update schema version in constants.rs (v21 → v22)
- [ ] Handle migration path for existing databases running incremental indexing

### C++ Parser Enhancement (`parser.rs`)
- [ ] Implement `extract_return_expression_types()` function
  - Test cases: `return new Widget()`, `return std::string("hello")`, complex expressions  
- [ ] Implement `infer_variable_type_from_assignment()` function  
  - Test cases: `$VAR = some_function()`, assignment with type coercion scenarios
- [ ] Integrate into existing parse pipeline (add call in `parse_cpp_file`)

### Multi-Language Support
- [ ] Python parser (`python_parser.rs`): infer types from variable assignments and return statements  
  - Pattern coverage: `request.args.get()` → str, `json.loads(data)` → dict/list detection
- [ ] TypeScript parser (`typescript_parser.tsx`): leverage existing TSX type information (should be trivial since TS already has rich type metadata)
- [ ] Rust parser (`rust_parser.rs`): extract inferred types from expression analysis

### Resolver Integration (`resolver.rs`)  
- [ ] Merge type inference results with symbol resolution phase in `merge_symbols()` function
- [ ] Handle overloaded functions — store multiple possible inferred types when evidence is ambiguous
- [ ] Confidence scoring logic: High if single clear source, Partial if conflicting hints exist

### Storage & Query Support (`storage.rs`)
- [ ] Implement write/read methods for type inference results to SQLite  
  - `write_type_inferences()`, `read_all_type_inferences_for_symbol()`
- [ ] Add query endpoints in server/src/app.ts for enhanced symbol lookup
  - GET /enhanced-symbol/{qualifiedName} endpoint with optional includeTypeInference flag

### Testing & Validation (Phase 1)
- [ ] Unit tests: Verify type inference accuracy on sample code snippets per language  
- [ ] Integration test: Index small project → verify >60% of common symbols have inferred types populated
- [ ] MCP tool test: Call `get_enhanced_symbol()` with various symbol IDs, validate response includes correct inferred_types field

## Phase 2: Cross-Boundary Flow Tracking — Implementation Checklist  

### Source/Sink Detection Rules Engine (`flow_tags.rs` — **new file**)  
- [ ] Define semantic tagging rules per language (C++, Python, TypeScript)
  - C++ sources: `cin >> $X`, `$VAR = file.read()` → tag=user_input
  - Python sinks: `exec($CODE)`, `os.system($CMD)` → tag=code_execution
  - SQL-related patterns across all languages for sink detection

### Call Resolution Enhancement (`resolver.rs`)  
- [ ] Modify existing `resolve_calls_with_db()` function to track argument→parameter flow with semantic tags during call graph construction phase
- [ ] Add value_transformed flag computation based on expression analysis (is argument passed through unchanged or modified?)
- [ ] Implement SplitBranch detection for conditional flows in if/else structures

### Cross-Boundary Flow Storage (`storage.rs`)  
- [ ] Create new tables: `symbol_flow_tags`, `cross_boundary_flow_paths` 
  - Follow existing schema migration pattern established in storage.rs
- [ ] Pre-compute common flow paths during indexing phase (store in SQLite for fast query retrieval)
- [ ] Handle incremental indexing correctly — only recompute affected functions when files change

### MCP Tool Integration (`mcp-runtime.ts`)  
- [ ] Define new tool: `trace_cross_function_flow({ sourceSymbolId, maxDepth })`
  - Query `cross_boundary_flow_paths` table for paths starting from source symbol
  - Traverse hops up to specified depth, collect flow details and semantic tags
  - Return structured response with hop-by-hop breakdown (see specs in Milestone20 doc)

### Testing & Validation (Phase 2)  
- [ ] Unit tests: Verify argument→parameter tagging works correctly across call boundaries for sample C++/Python code
- [ ] Integration test: Trace user input → SQL execution flow end-to-end across at least 3 function hops in test project
- [ ] Performance benchmark: Confirm <15% additional indexing overhead on typical projects (<20k symbols)

## Phase 3: Pattern-Based Structural Analysis — Implementation Checklist  

### Ruleset YAML Parser (`ruleset_parser.rs` — **new file**)  
- [ ] Implement ruleset.yaml file parser using serde_yaml crate
- [ ] Validate pattern definitions against schema before loading into analysis_rules table  
- [ ] Handle language-specific rule sets (cpp, python, typescript) with separate directories under .codeatlas/rules/

### Pattern Detection Functions (`analysis_patterns.rs` — **new file**)
- [ ] C++ patterns (~10 high-value rules): no-virtual-destructor, raw-pointer-member, missing-guard, use-after-free-risk  
  - Implement check functions that scan AST nodes for structural pattern matches (leverage existing tsg infrastructure)
- [ ] Python patterns: mutable-default-arg detection in function parameter analysis phase

### Integration with Indexing Pipeline (`main.rs`)
- [ ] Add new stage after resolve_calls phase but before persist step to run pattern-based structural analysis  
  - Pattern detection is embarrassingly parallel — distribute across worker threads like existing batch parsing does (see `run_full` implementation)
- [ ] Store results to SQLite tables created by ruleset parser task above

### MCP Tool Enhancement (`mcp-runtime.ts`)
- [ ] Add tool: `analyze_file({ filePath, categories? })`  
  - Query symbol_analysis_results table for violations matching file path and optional category filters
  - Return structured response with violation details grouped by severity (see specs in Milestone20 doc)
- [ ] Add tool: `list_analysis_rules({ category?, language? })` — query available rulesets

### HTTP API Enhancement (`app.ts`)  
- [ ] GET /analysis/rules endpoint — list available rulesets with filters for category/language/severity
- [ ] POST /analysis/evaluate/{ruleIds} endpoint — run analysis on workspace/file against specified rule(s) 
- [ ] GET /analysis/results endpoint — query analysis results by symbolId, severityMin, and category filters

### Dashboard UI Enhancement (server/public/)  
- [ ] Add Analysis Results tab in dashboard showing violations grouped by severity/category
  - Reuse existing table rendering infrastructure from current dashboard views
- [ ] Display pattern detection results alongside existing symbol lookup when viewing file details  

### Testing & Validation (Phase 3)  
- [ ] Unit tests: Validate rule set parsing and pattern matching on sample code snippets per language  
- [ ] Integration test: Index representative project → verify at least 8 high-value C++ patterns detected correctly during indexing phase
- [ ] MCP tool test: Call `analyze_file()` on known anti-pattern files, confirm violations reported with accurate descriptions

## Post-M20 Validation & Rollout  

### Success Criteria Checklist (All Phases Combined)  
- [ ] >60% of common function symbols have at least one inferred type after indexing Phase 1 completion
- [ ] User input flow traced end-to-end across ≥3 function boundaries in test project for Phase 2 validation  
- [ ] At least 8 high-value C++ patterns detected correctly during Phase 3 analysis phase
- [ ] All new MCP tools return structured, bounded responses following Milestone 17 compact-first direction

### Rollout Plan (Phased Deployment)
| Step | Action | Timeline Estimate | Risk Level |  
|------|--------|------------------|------------|
| Day 1-2 | Schema migration + C++ type inference implementation | ~8 hours dev time | Low — isolated changes to parser.rs/storage.rs only |  
| Week 1-2 | Multi-language extension (Python/TS/Rust) for Phase 1 | ~40 hours total across languages | Medium-High — requires AST expertise per language family
| Week 3-4 | Cross-boundary flow tracking + MCP tool integration | ~65 hours dev time including testing | Low-Medium — leverages existing propagation infrastructure, extends scope rather than building new system from scratch |  
| Week 5-6 | Pattern analysis ruleset + dashboard UI enhancement | ~60 hours total (rules + tools + UI) | Medium-High — requires YAML parsing infrastructure for external rule management plus MCP tool definitions

### Post-Rollout Monitoring & Feedback Collection
- [ ] Track agent fallback-to-raw-code behavior rate before/after M20 deployment → should decrease significantly  
- [ ] Collect user feedback on new analysis tools via dashboard UI usage analytics (if implemented) or direct issue reports in repository tracking system
- [ ] Measure indexing time overhead across representative projects to confirm actual impact matches estimates (~25-37% total initially, reducible through lazy evaluation strategies for Phase 2 deferred computation)

## Notes for Implementation Team  

1. **Reuse existing infrastructure aggressively**: All phases build on tree-sitter-graph (tsg) pattern matching already proven working in `cpp_call_relations.tsg`. No new AST traversal libraries needed — leverage current tsg compilation and execution pipeline established during Milestone 2 work.

2. **Keep initial ruleset lean for Phase 3**: Start with ~10 high-value patterns per language, validate effectiveness before expanding to larger sets. Quality > quantity for agent usefulness in code understanding context. Focus on critical code smells/security risks only initially.

3. **Agent-facing responses must be structured & bounded**: All new MCP tools return compact first (following Milestone 17 direction). Avoid dumping raw analysis results — provide summaries + optional detail expansion via follow-up queries as established by existing investigation workflow design in Milestone 10 docs.

4. **Performance optimization strategy for Phase 2**: Cross-boundary flow tracking can be deferred to on-demand computation for large repos (>50k symbols) instead of pre-computing during indexing. This keeps initial rollout fast while still providing value when agent explicitly queries via MCP tool calls (lazy evaluation pattern).
