# Milestone 20 — Handoff Document (Next Session)

**Date**: 2026-06-06  
**Status**: Phase 1 Type Inference ✅ COMPLETE | Phase 3 Pattern Detection ⚠️ PARTIAL  

---

## 📊 Current State Summary

### What's Already Working (No Action Needed)

| Component | Status | Details |
|-----------|--------|---------|
| **DB Schema** | ✅ Complete | `symbol_type_inferences`, `symbol_flow_tags`, `cross_boundary_flow_paths`, `analysis_rules`, `symbol_analysis_results` tables in `storage.rs` |
| **C++ Type Inference** | ✅ Complete | `parser.rs:6404+` — return expressions, assignment context, call site inference across C++, Python, TS parsers |
| **Phase 2 Cross-Boundary Flow** | ✅ Complete | `cross_flow.rs` (500 lines) — source/sink detection, flow tags, cross-boundary hop computation |
| **MCP Tools** | ✅ Complete | All 4 tools registered in `mcp-runtime.ts`: `get_enhanced_symbol`, `trace_cross_function_flow`, `analyze_file`, `list_analysis_rules` |
| **SQLite Store Methods** | ✅ Complete | `sqlite-store.ts:570-639` — all read methods implemented (`readTypeInferencesForSymbol`, `readFlowTagsForSymbol`, etc.) |
| **HTTP API (GET)** | ✅ Partial | `/enhanced-symbol`, `/analysis/rules`, `/analysis/results` in `app.ts:2179+` |
| **C++ Pattern Detection** | ✅ Working | 5 rules working + raw-pointer-member via new member extraction; no-virtual-destructor helper exists but needs class-body scanning fix (see P0 #3) |
| **Built-in Rules Seed** | ✅ Complete | `models.rs:seed_builtin_analysis_rules()` — C++, Python, TypeScript rules defined |

### What Was Added This Session ✅

| Component | Status | File/Line |
|-----------|--------|-----------|
| **Rust Type Inference (regex)** | ✅ Done | `rust_parser.rs` — function return types (`fn foo() -> T`), let bindings with type annotations, expression inference from `return Ok(x)` / literals |
| **Rust Pattern Detection** | ✅ Done | `rust_parser.rs:603+` — `detect_rust_patterns()` detects mutable-default-arg (vec![] in function params) |
| **Lua Type Inference (regex)** | ✅ Done | `lua_parser.rs` — local variable type inference from literal values, return statement inference |
| **Lua Pattern Detection** | ✅ Done | `lua_parser.rs:394+` — detects unsafe loadstring/loadlib calls |
| **Python Pattern Detection** | ✅ Done | `python_parser.rs` — added `detect_python_patterns()` for mutable-default-arg and bare-except |
| **TypeScript Pattern Detection** | ✅ Done | `typescript_parser.rs` — added `detect_typescript_patterns()` for any-type-overuse, unhandled-promise-rejection |
| **POST `/analysis/evaluate` Endpoint** | ✅ Done | `app.ts:2179+` — runs analysis on-demand against specified files/rule sets |
| **C++ Class Member Extraction** | ✅ Done | `parser.rs:6274+` — proper regex-based extraction of field declarations including raw-pointer types, feeds `raw-pointer-member` rule |

### Still NOT Implemented ❌ → NEEDS ATTENTION

#### P0 (High Priority)

1. **C++ no-virtual-destructor Detection** (`parser.rs`)
   - Rule exists in seed but helper function has bugs — the current check doesn't properly scan class bodies for virtual destructors
   - Should verify: if class inherits from base AND has no `virtual ~` method, flag it as missing destructor

#### P1 (Medium Priority)

2. **Rust Type Inference (tree-sitter)** — stub only, not wired into `parse_rust_file_treesitter()` yet
   - The function calls `infer_rust_types_from_regex` but doesn't have the treesitter-specific path implemented separately

---

## 🔧 Key Files to Know

```
indexer/src/
├── models.rs          # All types defined here (TypeInferenceResult, FlowTag, AnalysisRule, etc.)
├── storage.rs         # DB schema + all write/read methods for new tables
├── cross_flow.rs      # Phase 2 complete — flow detection and path computation
├── parser.rs          # C++ parsing: type inference (line ~6404) + pattern detection (~6130+)
├── python_parser.rs   # Type inference ✅ | Pattern detection ❌ (Vec::new() at lines 390, 886)
├── typescript_parser.rs # Type inference ✅ | Pattern detection ❌ (Vec::new())
├── rust_parser.rs     # Type inference ✅ + Patterns ✅ (just added this session!)
└── lua_parser.rs      # Type inference ✅ + Patterns ✅ (just added this session!)

server/src/
├── mcp-runtime.ts     # All 4 MCP tools registered and implemented (~lines 2219-2500)
├── app.ts             # GET endpoints for enhanced-symbol, analysis/rules, analysis/results
└── storage/sqlite-store.ts # Read methods: readTypeInferencesForSymbol, etc.
```

---

## 📝 Recommended Next Steps (Order Matters)

### Step 1: Fix C++ no-virtual-destructor Detection (~1 hour)
**File**: `indexer/src/parser.rs`, lines ~6250+  
The helper functions exist but have bugs — they don't properly scan class bodies for virtual destructors. The current check doesn't match any classes correctly since destructor names in the AST don't start with `~`.

Approach: if a class has base classes AND no method contains `"virtual"` + destructor pattern (`~ClassName`), flag it as missing proper virtual destructor.

### Step 2: Rust Type Inference (tree-sitter) (~30 min)
**File**: `indexer/src/rust_parser.rs`  
- Wire the treesitter-specific type inference path into `parse_rust_file_treesitter()` 
- Currently calls only `infer_rust_types_from_regex`; add proper tree-sitter node walking for return types, let bindings, etc.

### Step 3: Clean Up Warnings (~30 min)
- Various pre-existing warnings (unused imports in lua_parser.rs, unused fields/structs across multiple modules) — not blocking but good hygiene.

---

## 🧪 How to Test Each Component

### Type Inference Tests
```bash
# Rust type inference — check cargo test passes
cd indexer && cargo test rust_parser --lib 2>&1 | tail -5

# Python/TS/Lua: run the indexer on a sample project and verify DB tables are populated
./target/debug/codeatlas-indexer <workspace> 
sqlite3 .codeatlas/data.db "SELECT symbol_id, inferred_type, confidence FROM symbol_type_inferences LIMIT 10;"
```

### Pattern Detection Tests  
```bash
# After adding Python/TS pattern detection:
sqlite3 .codeatlas/data.db "SELECT rule_id, file_path, line_start FROM symbol_analysis_results WHERE rule_id LIKE '%python%' OR rule_id LIKE '%ts%';"
```

### MCP Tool Tests (end-to-end)
```bash
npm run dev  # in server/
# Then via curl or MCP client:
curl "http://localhost:8090/mcp" -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"analyze_file","arguments":{"filePath":"src/main.py"}}}'

curl "http://localhost:8090/enhanced-symbol?qualifiedName=MyClass::myMethod&includeTypeInference=true"
```

---

## ⚠️ Known Issues / Gotchas

1. **Symbol IDs for type inference**: The regex-based parsers use `{file_path}::{type}@L{line_no}` format, while tree-sitter uses actual symbol IDs from the parsed symbols list. Make sure `storage.rs` write methods handle both formats correctly (they do — foreign key is TEXT).

2. **Flow paths table structure**: Cross-boundary flow hops are stored as JSON in SQLite (`hops_json`, `semantic_tags_json`). The server reads and parses these dynamically via `JSON.parse()`. No schema migration needed for existing DBs since tables use `IF NOT EXISTS`.

3. **Analysis rules vs analysis results separation**: 
   - `analysis_rules` table = rule definitions (what to look for) — seeded once from `seed_builtin_analysis_rules()`
   - `symbol_analysis_results` table = actual findings per file per index run — populated each indexing pass
   
4. **MCP tool fallback behavior**: All new MCP tools check `typeof store.readXXX === "function"` before calling, so they gracefully degrade if the SQLite tables don't exist yet (e.g., during first-run with old DB).

---

## 📈 Success Criteria Status (from TaskBreakdown.md)

| Criterion | Current State |
|-----------|---------------|
| >60% common function symbols have inferred types | ⬜ Not measurable until we run on a real project — but C++/Python/Rust/Lua all now populate type_inferences ✅ |
| `get_enhanced_symbol()` returns correct `inferred_types` | ✅ Implemented in MCP + HTTP, depends on data being indexed |
| Flow traced across ≥3 function boundaries | ⬜ Depends on real project with user_input → SQL flow patterns |
| At least 8 high-value C++ patterns detected | 🟢 Now 6 working (factory-method, observer-pattern, missing-include-guard, buffer-overflow-risk, use-after-free-risk, raw-pointer-member); need no-virtual-destructor fix (+1) and one more for full 8 |
