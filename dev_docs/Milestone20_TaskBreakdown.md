# Milestone 20: Implementation Task Breakdown & Dependencies

**Purpose**: Concrete task list for implementing Semgrep-style static analysis upgrade. Organized by phase with dependencies, estimated effort, and implementation notes.

## Phase 1: Enhanced Type Inference (P0 — Highest Priority)

### Task 1.1: Database Schema Migration
- **Files to modify**: `indexer/src/storage.rs`, `server/src/constants.ts`  
- **New tables**: 
  ```sql
  CREATE TABLE symbol_type_inferences (
      symbol_id TEXT PRIMARY KEY REFERENCES symbols(id),
      inferred_type TEXT NOT NULL,           
      confidence TEXT CHECK(confidence IN ('high', 'partial', 'unresolved')),
      evidence_sources JSON NOT NULL         
  );
  
  CREATE INDEX idx_symbol_inference_type ON symbol_type_inferences(inferred_type);
  ```
- **Implementation notes**: 
  - Add schema version bump in `constants.rs` (current: v21 → new: v22)
  - Handle migration for existing databases by checking table existence and creating if missing  
  - Follow incremental indexing pattern already established in storage.rs

### Task 1.2: C++ Parser Enhancement (`indexer/src/parser.rs`)
- **New functions to add**:
  ```rust
  pub fn extract_return_expression_types(root_node: &Node, source: &[u8]) -> Vec<TypeEvidence> {
      // Walk return statements and extract expression types from AST nodes
      // Example patterns: 
      //   - "return new Widget()" → TypeEvidence::ReturnExpression("Widget*")
      //   - "return std::string(\"hello\")" → TypeEvidence::ReturnExpression("std::string")  
      //   - "return x + y" where x,y are strings → TypeEvidence::ReturnExpression("int") or complex type
  }

  pub fn infer_variable_type_from_assignment(assignment_node: &Node, symbols: &[Symbol]) -> Option<String> {
      // Analyze assignment expression to infer variable's actual type
      // Example patterns:  
      //   - "$VAR = some_function()" → match function return type with symbol lookup
      //   - "$OBJ->method()" → method return type inference via call graph resolution
  }

  pub fn compute_call_site_type_inference(call_node: &Node, source: &[u8], symbols: &[Symbol]) -> Vec<TypeEvidence> {
      // Reverse-infer types from how caller uses callee result
      // Example: "auto widget = createWidget()" → inferred type for 'createWidget' is Widget*
  }
  ```
- **Implementation notes**: 
  - Leverage existing tree-sitter AST traversal infrastructure already in parser.rs (visit_tree, extract_local_propagation_data)
  - Return `Vec<TypeEvidence>` where TypeEvidence = { expression_text: String, inferred_type_hint: Option<String>, confidence: High/Partial }

### Task 1.3: Multi-Language Type Inference Support  
- **Files to modify**: 
  - `indexer/src/python_parser.rs` — Python type inference
  - `indexer/src/typescript_parser.rs` — TypeScript/TSX type extraction (leverage existing TSX parser which already has rich type info)
  - `indexer/src/rust_parser.rs` — Rust inferred types from expressions  
- **Python-specific patterns**:
  ```python
  # Pattern: $VAR = request.args.get('key') → tag=user_input, type=str
  # Pattern: result = json.loads(data) → type=dict or list (depending on content analysis)
  # Pattern: obj.method() where method returns self → inferred_type=class_name  
  ```

### Task 1.4: Resolver Integration (`indexer/src/resolver.rs`)
- **New function**: `merge_type_inferences(raw_symbols, inferences)` — merge type inference results into representative symbols
- **Implementation notes**: 
  - Handle overloaded functions (multiple possible return types from different overloads)  
  - Confidence scoring based on evidence quality: High if single clear expression, Partial if multiple conflicting hints

### Task 1.5: Storage & Query Support (`indexer/src/storage.rs`)
- **New methods to add**:
  ```rust
  pub fn write_type_inferences(&self, inferences: &[TypeInferenceResult]) -> Result<(), String> { ... }
  pub fn read_all_type_inferences_for_symbol(&self, symbol_id: &str) -> Result<Vec<TypeInference>, String> { ... }  
  pub fn read_all_type_inferences_by_confidence(&self, min_confidence: &str) -> Result<HashMap<String, Vec<TypeInference>>, String> { ... }
  ```

---

## Phase 2: Cross-Boundary Flow Tracking (P0 — Highest Priority)

### Task 2.1: Source/Sink Detection Rules Engine (`indexer/src/flow_tags.rs` — **새 파일**)
- **Define semantic tagging rules per language**:  
  - C++ sources: `cin >> $X`, `$VAR = file.read()`, `scanf("%s", $X)` → tag=user_input, confidence=high  
  - Python sinks: `exec($CODE)`, `os.system($CMD)`, `sql.execute(query)` → tag=code_execution/sql_exec
  - TypeScript sources: `req.query['param']` → tag=user_input

- **Implementation notes**: 
  - Keep rules simple and pattern-based (no full taint engine complexity)  
  - Focus on common patterns only (~5 source types, ~5 sink types per language initially)

### Task 2.2: Call Resolution Enhancement (`indexer/src/resolver.rs`)
- **Modify existing function**: `resolve_calls_with_db()` → add flow tag tracking during resolution
- **New logic to integrate**:  
  ```rust
  // In resolve_one() or similar call resolution step, after matching caller→callee:
  let mut flow_tags = Vec::new();
  
  // Check if argument expression matches source patterns (e.g., user_input)
  for arg in raw_call.argument_texts {
      if is_source_pattern(&arg, &language) { 
          flow_tags.push(FlowTag { kind: FlowKind::UserInput, label: extract_label_from_arg(&arg) });  
      }
      
      // Check if result target matches sink patterns (e.g., sql_exec)
      if raw_call.result_target.is_some() && is_sink_pattern(&raw_call.called_name, &language) {
          flow_tags.push(FlowTag { kind: FlowKind::Sink(sql_exec), label: "sql_execution" });  
      }
  }

  // Store with call resolution result for cross-boundary analysis later
  ```
- **Implementation notes**: 
  - Reuse existing `RawCallSite` structure, add optional flow_tags field to it temporarily during indexing phase only (don't modify persistent Call model yet)

### Task 2.3: Cross-Boundary Flow Storage (`indexer/src/storage.rs`)  
- **New tables & methods** (see schema in Milestone20_SemgrepStyleStaticAnalysis.md):
  ```sql
  CREATE TABLE symbol_flow_tags (
      symbol_id TEXT NOT NULL REFERENCES symbols(id),
      tag_kind TEXT CHECK(tag_kind IN ('user_input', 'config_value', 'computed')),  
      label TEXT,                                    
      confidence TEXT CHECK(confidence IN ('high', 'partial')) 
  );

  CREATE TABLE cross_boundary_flow_paths (  
      source_symbol_id TEXT NOT NULL REFERENCES symbols(id),
      target_symbol_id TEXT NOT NULL REFERENCES symbols(id),
      hops JSON NOT NULL,                        
      semantic_tags JSON NOT NULL               
  );
  ```
- **Implementation notes**: 
  - Pre-compute common flow paths during indexing phase for fast query responses  
  - Use `JSON` type in SQLite to store hop arrays and tag lists (simpler than relational normalization)

### Task 2.4: MCP Tool Integration (`server/src/mcp-runtime.ts`)
- **New tool definition**: 
  ```typescript
  server.tool(
    'trace_cross_function_flow',  
    {
      sourceSymbolId: z.string(),
      maxDepth: z.number().optional()
    },
    async (args, context) => { ... } // implementation below
  );
  ```

- **Implementation logic**: 
  - Query `cross_boundary_flow_paths` table for paths starting from `source_symbol_id`  
  - Traverse hops up to `maxDepth`, collecting flow details and semantic tags
  - Return structured response with hop-by-hop breakdown (see MCP tool specs in Milestone20 doc)

---

## Phase 3: Pattern-Based Structural Analysis (P1 — High Priority)

### Task 3.1: Ruleset YAML Parser (`indexer/src/ruleset_parser.rs` — **새 파일**)
- **YAML parsing library**: Use `serde_yaml` crate (already available in Rust ecosystem, no new deps needed if added to Cargo.toml)  
- **Implementation notes**: 
  - Parse `.codeatlas/rules/*.yaml` files from workspace root during indexing phase
  - Validate rule definitions against schema before loading into analysis_rules table

### Task 3.2: Pattern Detection Functions (`indexer/src/analysis_patterns.rs` — **새 파일**)
- **C++ patterns (start with these ~10 high-value rules)**:  
  | Rule ID | Category | Severity | Description | Check Logic |
  |---------|----------|----------|-------------|------------| 
  | `cpp-no-virtual-destructor` | code_smell | warning | Derived class without virtual destructor | has_base_class && !has_virtual_destructor |
  | `raw-pointer-member` | memory_risk | info | Class member uses raw pointer (potential leak) | has_member_of_type(PointerType) && !has_smart_ptr_wrapper  
  | `missing-include-guard` | build_risk | warning | Header file without include guard | is_header && !has_guard_macro
  | `cpp-use-after-free-risk` | security_risk | error | Pointer used after potential free/delete in same scope | contains both free() and dereference of freed ptr

- **Python patterns**:  
  | Rule ID | Category | Severity | Description | Check Logic |
  |---------|----------|----------|-------------|------------| 
  | `python-mutable-default-arg` | bug_risk | error | Mutable default argument — shared state between calls | param with list/dict literal as default value

### Task 3.3: Integration with Indexing Pipeline (`indexer/src/main.rs`)
- **Add new stage**: After existing parse/resolve stages, run pattern analysis phase  
- **Implementation notes**: 
  - Pattern detection is embarrassingly parallel — distribute across worker threads like existing batch parsing does (see `run_full` in main.rs)
  - Store results to SQLite tables created by Task 3.1

### Task 3.4: MCP Tool Enhancement (`server/src/mcp-runtime.ts`)  
- **New tools**: 
  ```typescript
  server.tool('analyze_file', { filePath, categories? }, async (args, context) => { ... });
  server.tool('list_analysis_rules', { category?, language? }, async (args, context) => { ... });  
  ```

### Task 3.5: HTTP API Enhancement (`server/src/app.ts`)
- **New endpoints**: 
  - `GET /analysis/rules` — list available rulesets with filters
  - `POST /analysis/evaluate/{ruleIds}` — run analysis on workspace/file against specified rule(s)  
  - `GET /analysis/results?symbolId=&severityMin=warning&category=code_smell` — query analysis results

---

## Cross-Phase Dependencies & Execution Order

```mermaid
graph TD
    A[Task 1.1: DB Schema Migration] --> B[Task 1.2: C++ Parser Enhancement]
    A --> C[Task 3.1: Ruleset YAML Parser]  
    B --> D[Task 1.4: Resolver Integration]
    D --> E[Phase 1 Complete — Type Inference Ready]
    
    F[Task 2.1: Source/Sink Detection Rules] --> G[Task 2.2: Call Resolution Enhancement]
    A --> H[Task 3.5: HTTP API Endpoints]  
    G --> I[Task 2.3: Cross-Boundary Flow Storage]
    
    E & I --> J[Phase 2 Complete — Flow Tracking Ready]
    C & F & K[Task 3.2: Pattern Detection Functions] --> L[Task 3.3: Pipeline Integration]
    
    H & M[Task 1.5: Query Support] & N[Task 2.4: MCP Tool Integration] & L & O[Task 3.4: Analysis Tools] → P[All Phases Complete — Full Semgrep-Style Analysis Operational]
```

**Execution order summary**:  
1. Start with Phase 1 Type Inference (highest ROI, lowest risk) — Tasks 1.1→1.5 in sequence
2. Begin Phase 3 Pattern Analysis alongside Phase 1 completion — Tasks 3.1→3.4 parallel to 1.x tasks
3. Complete Phase 2 Cross-Boundary Flow last — depends on existing call resolution infrastructure but integrates with both Type Inference and Pattern Results

---

## Estimated Effort Breakdown (Developer Hours)

| Task Group | C++ Parser Focus | Multi-Language Extension | Server/MCP Integration | Total Estimate |
|------------|-----------------|-------------------------|----------------------|---------------|  
| Phase 1: Type Inference | ~40 hours | +20 hours (Python/TS/Rust extensions) | +15 hours (enhanced lookup tools, HTTP endpoints) | **~75 hours** |
| Phase 2: Cross-Boundary Flow | ~30 hours (flow tag detection in C++) | +15 hours (language-specific source/sink rules) | +20 hours (trace tool implementation, flow path traversal logic) | **~65 hours**  
| Phase 3: Pattern Analysis | ~25 hours (~10 high-value C++ patterns) | +10 hours (Python/TS pattern sets) | +25 hours (ruleset management tools, dashboard UI integration) | **~60 hours**  

**Total estimated effort**: ~200 developer hours across all three phases

---

## Validation Checklist for Each Phase Completion

### Phase 1 (Type Inference) — Success Criteria:
- [ ] >60% of common function symbols have at least one inferred type after indexing
- [ ] `get_enhanced_symbol()` MCP tool returns `inferred_types` field correctly populated  
- [ ] Type inference confidence scoring works accurately (High for clear expressions, Partial/Unresolved when evidence is weak)

### Phase 2 (Cross-Boundary Flow) — Success Criteria:
- [ ] User input flow traced end-to-end across at least 3 function boundaries in test project
- [ ] `trace_cross_function_flow()` MCP tool returns structured hop-by-hop path with semantic tags  
- [ ] Performance impact <15% on indexing time for typical projects (<20k symbols)

### Phase 3 (Pattern Analysis) — Success Criteria:
- [ ] At least 8 high-value C++ patterns detected correctly during indexing phase
- [ ] `analyze_file()` MCP tool returns violations grouped by category/severity as expected  
- [ ] Ruleset YAML parsing works for custom user-defined rules in `.codeatlas/rules/` directory

---

## Notes for Implementation Team

1. **Reuse existing infrastructure**: All phases build on tree-sitter-graph (tsg) pattern matching already proven working in `cpp_call_relations.tsg`. No new AST traversal libraries needed — leverage current tsg compilation and execution pipeline.

2. **Keep initial ruleset lean**: Start with ~10 high-value patterns per language, validate effectiveness before expanding to larger sets. Quality > quantity for agent usefulness.

3. **Agent-facing responses must be structured & bounded**: All new MCP tools return compact first (following Milestone 17 direction). Avoid dumping raw analysis results — provide summaries + optional detail expansion via follow-up queries.

4. **Performance optimization strategy**: Phase 2 flow tracking can be deferred to on-demand computation for large repos (>50k symbols) instead of pre-computing during indexing. This keeps initial rollout fast while still providing value when agent explicitly requests flow tracing.
