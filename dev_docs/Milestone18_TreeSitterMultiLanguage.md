# Milestone 18. Tree-Sitter Multi-Language Parser Migration

Status:

- Completed (2026-04-22)

## 1. Objective

Migrate all non-C++ language parsers (Lua, Python, TypeScript, Rust) from hand-written regex-based extraction to tree-sitter + tree-sitter-graph based extraction, using the same proven architecture that the C++ parser already uses.

This milestone focuses on:

- adding tree-sitter grammar dependencies for Lua, Python, TypeScript, and Rust
- writing `.tsg` (tree-sitter-graph) DSL rules for call extraction in each language
- replacing regex-based symbol extraction with tree-sitter AST walking for each language
- sharing the existing `extract_graph_relation_events()` infrastructure from the C++ parser
- preserving all existing ParseResult output shapes (symbols, raw_calls, normalized_references, relation_events)
- running legacy (regex) and graph (tree-sitter) extraction in parallel during transition, comparing results

Success outcome:

- all four non-C++ language parsers use tree-sitter for AST parsing instead of line-by-line regex
- call extraction uses `.tsg` graph rules consistent with the C++ pattern
- symbol extraction uses tree-sitter AST nodes for accurate scope boundaries
- import/reference extraction uses tree-sitter for reliable multi-line pattern matching
- regex parsers are fully replaced — no regex extraction code remains in production paths
- all existing tests pass with identical or improved extraction results
- indexing performance remains comparable (tree-sitter may be marginally faster or slower per file, but overall impact is negligible given these files are small relative to C++)

Positioning note:

- MS2 introduced tree-sitter-graph integration for C++ call extraction
- MS18 extends the same architecture to all supported languages
- the C++ parser's dual-extraction (legacy + graph) comparison pattern is reused during migration

Scope note:

- this milestone changes only indexer parsing code
- no DB schema changes, no server changes
- the ParseResult shape is preserved — downstream code sees the same data
- local propagation extraction (C++ only) is not replicated for other languages in this milestone

---

## 2. Applicability Review

### Current parser architecture

| language | parser file | strategy | lines | symbol types | call patterns |
|---|---|---|---|---|---|
| C++ | `parser.rs` | tree-sitter + tree-sitter-graph + AST walk | 5,030 | class, struct, enum, enumMember, namespace, function, method, variable, typedef | unqualified, qualified, member_access, pointer_member_access, this_pointer_access |
| Lua | `lua_parser.rs` | regex, line-by-line, block depth | 466 | namespace, function, method | qualified, unqualified |
| Python | `python_parser.rs` | regex, line-by-line, indentation | 656 | namespace, class, function, method | member_access, qualified, unqualified |
| Rust | `rust_parser.rs` | regex, line-by-line, brace depth | 765 | namespace, struct, enum, trait, function, method | qualified, member_access, unqualified |
| TypeScript | `typescript_parser.rs` | regex, line-by-line, brace depth | 739 | namespace, class, interface, function, method | member_access, qualified, unqualified |

### Known regex parser weaknesses

| language | weakness | example |
|---|---|---|
| TypeScript | arrow function as variable | `const handler = (x: T) => { ... }` — not captured as function |
| TypeScript | destructuring import rename | `import { foo as bar } from 'm'` — alias tracking partial |
| TypeScript | multi-line string containing braces | `template literal with { }` — corrupts brace depth |
| Python | multi-line decorator | `@decorator(\narg\n)` — breaks line-by-line parsing |
| Python | nested comprehension + lambda | `[f(x) for x in xs]` — call inside comprehension missed |
| Rust | impl Trait for Type | `impl Display for Foo { fn fmt(...) }` — trait impl method scoping fragile |
| Rust | macro-generated function | `#[test] fn ...` — macro annotations not parsed |
| Rust | where clause spanning lines | brace depth off when `where` clause has `{` in trait bounds |
| Lua | multi-line function assignment | `local f = function() ... end` in some patterns |

### tree-sitter grammar availability (crates.io)

| crate | version | maturity | notes |
|---|---|---|---|
| `tree-sitter-python` | 0.23 | High | widely used, stable grammar |
| `tree-sitter-typescript` | 0.23 | High | includes both TypeScript and TSX grammars |
| `tree-sitter-rust` | 0.23 | High | official Rust grammar |
| `tree-sitter-lua` | 0.2 | Medium | lower version, but functional for core Lua 5.x constructs |
| `tree-sitter` | 0.24 | High | already used for C++ |
| `tree-sitter-graph` | 0.12 | High | already used for C++ |

### C++ parser pattern to reuse

The C++ parser (`parser.rs`) establishes a proven dual-extraction pattern:

1. **tree-sitter AST walking** (`visit_tree()`) — extracts symbols, scope hierarchy, risk signals
2. **tree-sitter-graph `.tsg` rules** (`extract_graph_relation_events()`) — extracts call relations
3. **Legacy regex extraction** — runs in parallel for comparison
4. **Merge logic** (`graph_matches_legacy_calls()`) — uses graph results when they match legacy, falls back to legacy otherwise
5. **Normalization** (`normalize_relation_events()`) — converts raw events to NormalizedReference

For MS18, each language replicates steps 1-2 using tree-sitter, step 3 uses the existing regex parser as legacy, and steps 4-5 are reused as-is.

---

## 3. Recommended Order

1. M18-E1. Infrastructure: shared tree-sitter extraction framework
2. M18-E2. TypeScript tree-sitter parser
3. M18-E3. Python tree-sitter parser
4. M18-E4. Rust tree-sitter parser
5. M18-E5. Lua tree-sitter parser
6. M18-E6. Validation and legacy parser removal

Why this order:

- E1 establishes the shared framework that all languages use (graph rule loading, event extraction, merge logic)
- E2 TypeScript first — most structurally similar to C++ (brace-delimited, class-based OOP), highest ROI
- E3 Python second — mature tree-sitter grammar, significant regex weakness (indentation edge cases)
- E4 Rust third — complex type system (impl blocks, traits), but tree-sitter-rust is very stable
- E5 Lua last — `tree-sitter-lua` is lowest version, simplest language with fewest regex weaknesses
- E6 validates all languages end-to-end, then removes legacy regex code

Execution rule:

- finish each Epic to the point of measurable acceptance before starting the next
- each language epic produces a working parser that passes all existing tests before moving on
- the legacy regex parser remains as fallback until E6 removes it

---

## 4. Epic Breakdown

### M18-E1. Infrastructure: Shared Tree-Sitter Extraction Framework

Status:

- Completed

Goal:

- extract reusable tree-sitter-graph infrastructure from the C++ parser into shared modules so that all language parsers can use the same graph event extraction pattern

Problem being solved:

- `extract_graph_relation_events()` in `parser.rs` is C++ specific — it hardcodes `tree_sitter_cpp::LANGUAGE` and `CPP_CALL_RELATIONS`
- each language needs the same pattern: compile `.tsg` rules, execute against tree, extract events
- thread-local caching of compiled rules should work for all languages

Design:

Create a generic `execute_graph_rules()` function:

```rust
// in parser.rs or a new shared module
pub fn execute_graph_rules(
    language: tree_sitter::Language,
    tsg_source: &str,
    tree: &Tree,
    source: &str,
    file_path: &str,
    symbols: &[Symbol],
) -> (Vec<RawRelationEvent>, u128, u128)
```

This function encapsulates:
- thread-local rule compilation and caching (keyed by language name)
- graph execution with lazy mode
- event extraction from graph nodes (read attributes, determine enclosing caller)
- compile/execute timing metrics

Implementation tasks:

- M18-E1-T1. Add tree-sitter dependencies to `Cargo.toml`: `tree-sitter-python`, `tree-sitter-typescript`, `tree-sitter-rust`, `tree-sitter-lua`
- M18-E1-T2. Create `indexer/graph/` directory structure for language-specific `.tsg` files (already exists for `cpp_call_relations.tsg`)
- M18-E1-T3. Extract `execute_graph_rules()` from `parser.rs` `extract_graph_relation_events()` into a generic function
- M18-E1-T4. Refactor `extract_graph_relation_events()` in `parser.rs` to call `execute_graph_rules()` with `tree_sitter_cpp::LANGUAGE` and `CPP_CALL_RELATIONS`
- M18-E1-T5. Create `graph_rules.rs` constants for each language: `TYPESCRIPT_CALL_RELATIONS`, `PYTHON_CALL_RELATIONS`, `RUST_CALL_RELATIONS`, `LUA_CALL_RELATIONS` (initially empty strings)
- M18-E1-T6. Verify C++ parser still works identically after refactoring — `cargo test` must pass

Expected touch points:

- `indexer/Cargo.toml`
- `indexer/src/parser.rs`
- `indexer/src/graph_rules.rs`

Acceptance:

- `execute_graph_rules()` is callable from any language parser with any tree-sitter language + `.tsg` rules
- C++ parser behavior unchanged — `cargo test` passes
- `cargo build` compiles with new tree-sitter dependencies

### M18-E2. TypeScript Tree-Sitter Parser

Status:

- Completed

Goal:

- replace `typescript_parser.rs` regex extraction with tree-sitter AST walking + tree-sitter-graph call rules

Problem being solved:

- regex brace depth tracking fails on template literals with embedded braces
- arrow functions assigned to variables not captured
- multi-line patterns not reliably matched

Design:

Three-phase parser function `parse_typescript_file_treesitter()`:

Phase 1 — Symbol extraction via AST walk:
- Walk tree-sitter-typescript AST nodes
- Extract: `class_declaration`, `interface_declaration`, `function_declaration`, `method_definition`, `arrow_function` (when assigned to variable/export)
- Build qualified names using scope stack (class/namespace nesting)
- Track `import_statement` nodes for module references

Phase 2 — Call extraction via `.tsg` rules:
- `typescript_call_relations.tsg` — patterns for:
  - `func()` — unqualified call
  - `obj.method()` — member access
  - `this.method()` — this pointer access
  - `Module.func()` — qualified call
  - `new ClassName()` — class instantiation
  - `makeX().method()` — chained call

Phase 3 — Reference extraction via AST walk:
- Import statements → `ModuleImport` references
- Class extends/implements → `InheritanceMention` references
- Type annotations → `TypeUsage` references

Implementation tasks:

- M18-E2-T1. Create `indexer/graph/typescript_call_relations.tsg` with call patterns matching TypeScript AST node names from tree-sitter-typescript grammar
- M18-E2-T2. Add `TYPESCRIPT_CALL_RELATIONS` constant to `graph_rules.rs` pointing to the `.tsg` file
- M18-E2-T3. Create `parse_typescript_file_treesitter()` function in `typescript_parser.rs`:
  - Initialize `Parser` with `tree_sitter_typescript::LANGUAGE_TYPESCRIPT`
  - Parse source into tree
  - Walk AST for symbols (class, interface, function, method, arrow function, variable)
  - Walk AST for imports and references
  - Call `execute_graph_rules()` for call extraction
  - Normalize events
- M18-E2-T4. Wire dual-extraction: call both `parse_typescript_file()` (legacy) and `parse_typescript_file_treesitter()` (new)
- M18-E2-T5. Add comparison logic: compare symbol counts, call counts, reference counts between legacy and tree-sitter
- M18-E2-T6. When tree-sitter results are >= legacy (no regressions), use tree-sitter results
- M18-E2-T7. Add TypeScript tree-sitter parser tests in `typescript_parser.rs`:
  - test basic function/class extraction
  - test arrow function extraction
  - test import extraction
  - test call extraction (unqualified, member access, this, chained)
  - test class inheritance extraction
- M18-E2-T8. Test against real TypeScript project files to verify no symbol regression

Expected touch points:

- `indexer/graph/typescript_call_relations.tsg` (new)
- `indexer/src/graph_rules.rs`
- `indexer/src/typescript_parser.rs`

Acceptance:

- `parse_typescript_file_treesitter()` produces symbols matching or exceeding regex parser output
- call extraction captures arrow functions and template literal edge cases that regex missed
- all existing tests pass
- `cargo build` and `cargo test` pass

### M18-E3. Python Tree-Sitter Parser

Status:

- Completed

Goal:

- replace `python_parser.rs` regex extraction with tree-sitter AST walking + tree-sitter-graph call rules

Problem being solved:

- indentation-based regex scope tracking fails on complex nesting
- multi-line decorators and comprehensions not reliably parsed
- lambda calls inside list comprehensions missed

Design:

Three-phase parser function `parse_python_file_treesitter()`:

Phase 1 — Symbol extraction via AST walk:
- `class_definition`, `function_definition` nodes
- Scope determined by AST parent chain (not indentation counting)
- Decorators captured from `decorator` nodes

Phase 2 — Call extraction via `.tsg` rules:
- `python_call_relations.tsg` — patterns for:
  - `func()` — unqualified call
  - `self.method()` — self member access
  - `obj.method()` — member access
  - `Module.func()` — qualified call
  - `ClassName()` — class instantiation

Phase 3 — Reference extraction:
- `import_statement`, `import_from_statement` → `ModuleImport`
- Class bases → `InheritanceMention`

Implementation tasks:

- M18-E3-T1. Create `indexer/graph/python_call_relations.tsg`
- M18-E3-T2. Add `PYTHON_CALL_RELATIONS` constant to `graph_rules.rs`
- M18-E3-T3. Create `parse_python_file_treesitter()` function in `python_parser.rs`
- M18-E3-T4. Wire dual-extraction comparison
- M18-E3-T5. Add Python tree-sitter parser tests
- M18-E3-T6. Test against real Python project files

Expected touch points:

- `indexer/graph/python_call_relations.tsg` (new)
- `indexer/src/graph_rules.rs`
- `indexer/src/python_parser.rs`

Acceptance:

- `parse_python_file_treesitter()` produces symbols matching or exceeding regex parser output
- decorator and comprehension edge cases correctly handled
- all existing tests pass

### M18-E4. Rust Tree-Sitter Parser

Status:

- Completed

Goal:

- replace `rust_parser.rs` regex extraction with tree-sitter AST walking + tree-sitter-graph call rules

Problem being solved:

- `impl Trait for Type` method scoping unreliable with brace depth
- `where` clause brace counting corruption
- macro-generated functions not captured

Design:

Three-phase parser function `parse_rust_file_treesitter()`:

Phase 1 — Symbol extraction via AST walk:
- `struct_item`, `enum_item`, `trait_item`, `function_item`, `impl_item` nodes
- impl block scope tracked via AST parent (not brace depth)
- Method parent determined by `impl_item` → type identifier

Phase 2 — Call extraction via `.tsg` rules:
- `rust_call_relations.tsg` — patterns for:
  - `func()` — unqualified call
  - `self.method()` — self member access
  - `obj.method()` — member access
  - `Type::method()` — qualified call (path expression)
  - `Module::func()` — qualified call

Phase 3 — Reference extraction:
- `use_declaration` → `ModuleImport`
- `impl` type + trait → `InheritanceMention`

Implementation tasks:

- M18-E4-T1. Create `indexer/graph/rust_call_relations.tsg`
- M18-E4-T2. Add `RUST_CALL_RELATIONS` constant to `graph_rules.rs`
- M18-E4-T3. Create `parse_rust_file_treesitter()` function in `rust_parser.rs`
- M18-E4-T4. Wire dual-extraction comparison
- M18-E4-T5. Add Rust tree-sitter parser tests
- M18-E4-T6. Test against CodeAtlas indexer source itself (self-hosting test)

Expected touch points:

- `indexer/graph/rust_call_relations.tsg` (new)
- `indexer/src/graph_rules.rs`
- `indexer/src/rust_parser.rs`

Acceptance:

- `parse_rust_file_treesitter()` produces symbols matching or exceeding regex parser output
- impl block methods correctly parented
- `where` clause edge cases no longer corrupt scope
- all existing tests pass

### M18-E5. Lua Tree-Sitter Parser

Status:

- Completed

Goal:

- replace `lua_parser.rs` regex extraction with tree-sitter AST walking + tree-sitter-graph call rules

Problem being solved:

- block depth tracking for `if/for/while/repeat/do` is fragile
- multi-line function assignments not always captured

Design:

Three-phase parser function `parse_lua_file_treesitter()`:

Phase 1 — Symbol extraction via AST walk:
- `function_declaration`, `local_function`, `function` (anonymous assigned to variable)
- Table-based method detection via `function_name` with dot/colon

Phase 2 — Call extraction via `.tsg` rules:
- `lua_call_relations.tsg` — patterns for:
  - `func()` — unqualified call
  - `obj.method()` — dot member access
  - `obj:method()` — colon method access (Lua self-call)
  - `Module.func()` — qualified call

Phase 3 — Reference extraction:
- `require()` calls → `ModuleImport`

Implementation tasks:

- M18-E5-T1. Create `indexer/graph/lua_call_relations.tsg`
- M18-E5-T2. Add `LUA_CALL_RELATIONS` constant to `graph_rules.rs`
- M18-E5-T3. Create `parse_lua_file_treesitter()` function in `lua_parser.rs`
- M18-E5-T4. Wire dual-extraction comparison
- M18-E5-T5. Add Lua tree-sitter parser tests
- M18-E5-T6. Test against real Lua project files

Expected touch points:

- `indexer/graph/lua_call_relations.tsg` (new)
- `indexer/src/graph_rules.rs`
- `indexer/src/lua_parser.rs`

Acceptance:

- `parse_lua_file_treesitter()` produces symbols matching or exceeding regex parser output
- colon method syntax correctly identified as self-call
- all existing tests pass

### M18-E6. Validation and Legacy Parser Removal

Status:

- Completed

Goal:

- validate all four tree-sitter parsers against real project data
- remove legacy regex extraction code from all four language parsers
- make tree-sitter the sole extraction path

Implementation tasks:

- M18-E6-T1. Run full test suite: `cargo test` — all tests pass
- M18-E6-T2. Run `cargo build` — zero warnings
- M18-E6-T3. Index a mixed-language project (e.g. opencv with Python bindings, or CodeAtlas itself with TS + Rust)
- M18-E6-T4. Compare symbol/call/reference counts before and after migration for each language
- M18-E6-T5. Remove legacy regex extraction code from `typescript_parser.rs`, `python_parser.rs`, `rust_parser.rs`, `lua_parser.rs`
- M18-E6-T6. Remove dual-extraction comparison logic
- M18-E6-T7. Final `cargo test` + `cargo build` — zero warnings, all tests pass
- M18-E6-T8. Update milestone documentation with completion evidence

Expected touch points:

- `indexer/src/typescript_parser.rs`
- `indexer/src/python_parser.rs`
- `indexer/src/rust_parser.rs`
- `indexer/src/lua_parser.rs`
- `dev_docs/Milestone18_TreeSitterMultiLanguage.md`

Acceptance:

- no regex extraction code remains in production parser paths
- all existing tests pass
- indexing results for all languages are identical or improved

---

## 5. Detailed Implementation Guide

### 5.1 `.tsg` Rule File Conventions

Each `.tsg` file follows the C++ pattern (`cpp_call_relations.tsg`):

```scheme
; Each matched call creates one graph node with normalized attributes.
; Required attributes for every call event:
;   relation_kind = "call"
;   call_kind = one of: "unqualified", "member_access", "qualified", "this_pointer_access"
;   target_name = string (function/method name)
;   argument_count = integer
;   line = integer (1-based)
; Optional attributes:
;   receiver = string (for member access calls)
;   qualifier = string (for qualified calls)
```

Language-specific node names must be looked up in each tree-sitter grammar's `node-types.json`. Key node types per language:

**TypeScript** (tree-sitter-typescript):
- `call_expression` → call
- `member_expression` → `object` + `property`
- `identifier`, `property_identifier` → names
- `arguments` → argument list
- `this` → self reference
- `class_declaration`, `function_declaration`, `arrow_function`, `method_definition`
- `import_statement`, `import_clause`

**Python** (tree-sitter-python):
- `call` → call expression
- `attribute` → `object` + `attribute` (member access)
- `identifier` → names
- `argument_list` → arguments
- `class_definition`, `function_definition`
- `import_statement`, `import_from_statement`

**Rust** (tree-sitter-rust):
- `call_expression` → call
- `field_expression` → `value` + `field` (member access)
- `identifier`, `field_identifier` → names
- `arguments` → argument list
- `self` → self reference
- `struct_item`, `enum_item`, `trait_item`, `function_item`, `impl_item`
- `use_declaration`

**Lua** (tree-sitter-lua):
- `function_call` → call
- `dot_index_expression` → member access with `.`
- `method_index_expression` → method access with `:`
- `identifier` → names
- `arguments` → argument list
- `function_declaration`, `local_function`

### 5.2 AST Walking Pattern for Symbols

Each language parser's `visit_tree()` equivalent should:

1. Start from root node
2. Use recursive descent or cursor-based traversal
3. For each symbol-producing node:
   - Extract name from the appropriate child node
   - Compute qualified name using scope stack
   - Determine symbol type (function, method, class, etc.)
   - Record line and end_line from node range
   - Extract signature from the parameter list node
   - Count parameters from the parameter list
4. Maintain scope stack (class/impl/namespace nesting) via AST parent chain

### 5.3 Dual-Extraction Comparison

During the transition period, each language parser:

1. Runs legacy regex extraction → `legacy_result: ParseResult`
2. Runs tree-sitter extraction → `ts_result: ParseResult`
3. Compares:
   - `ts_result.symbols.len() >= legacy_result.symbols.len() * 0.9` (allow 10% tolerance for dedup differences)
   - `ts_result.raw_calls.len() >= legacy_result.raw_calls.len() * 0.8` (allow 20% tolerance)
4. If tree-sitter meets threshold, use tree-sitter results
5. Otherwise, fall back to legacy results and log a warning

This mirrors the C++ parser's `graph_matches_legacy_calls()` pattern.

### 5.4 Thread-Local Rule Caching

The `execute_graph_rules()` function should use `thread_local!` with a `HashMap<&'static str, Arc<GraphDslFile>>` keyed by language name string, so each language's compiled rules are cached separately per thread.

---

## 6. Task Breakdown By File

### `indexer/Cargo.toml`

- add `tree-sitter-python = "0.23"` (M18-E1-T1)
- add `tree-sitter-typescript = "0.23"` (M18-E1-T1)
- add `tree-sitter-rust = "0.23"` (M18-E1-T1)
- add `tree-sitter-lua = "0.2"` (M18-E1-T1)

### `indexer/src/parser.rs`

- extract `execute_graph_rules()` from `extract_graph_relation_events()` (M18-E1-T3)
- refactor `extract_graph_relation_events()` to call `execute_graph_rules()` (M18-E1-T4)

### `indexer/src/graph_rules.rs`

- add `TYPESCRIPT_CALL_RELATIONS` (M18-E2-T2)
- add `PYTHON_CALL_RELATIONS` (M18-E3-T2)
- add `RUST_CALL_RELATIONS` (M18-E4-T2)
- add `LUA_CALL_RELATIONS` (M18-E5-T2)

### `indexer/graph/typescript_call_relations.tsg` (new)

- call patterns for TypeScript (M18-E2-T1)

### `indexer/graph/python_call_relations.tsg` (new)

- call patterns for Python (M18-E3-T1)

### `indexer/graph/rust_call_relations.tsg` (new)

- call patterns for Rust (M18-E4-T1)

### `indexer/graph/lua_call_relations.tsg` (new)

- call patterns for Lua (M18-E5-T1)

### `indexer/src/typescript_parser.rs`

- add `parse_typescript_file_treesitter()` (M18-E2-T3)
- add dual-extraction comparison (M18-E2-T4, T5, T6)
- add tree-sitter tests (M18-E2-T7)
- remove legacy regex code (M18-E6-T5)

### `indexer/src/python_parser.rs`

- add `parse_python_file_treesitter()` (M18-E3-T3)
- add dual-extraction comparison (M18-E3-T4)
- add tree-sitter tests (M18-E3-T5)
- remove legacy regex code (M18-E6-T5)

### `indexer/src/rust_parser.rs`

- add `parse_rust_file_treesitter()` (M18-E4-T3)
- add dual-extraction comparison (M18-E4-T4)
- add tree-sitter tests (M18-E4-T5)
- remove legacy regex code (M18-E6-T5)

### `indexer/src/lua_parser.rs`

- add `parse_lua_file_treesitter()` (M18-E5-T3)
- add dual-extraction comparison (M18-E5-T4)
- add tree-sitter tests (M18-E5-T5)
- remove legacy regex code (M18-E6-T5)

---

## 7. Cross-Epic Risks

### Risk 1. tree-sitter-lua grammar maturity

Why it matters:

- `tree-sitter-lua` is at version 0.2, significantly lower than other grammars at 0.23
- may have missing or incorrect node types for some Lua 5.x constructs

Mitigation:

- Lua is scheduled last (E5) so issues surface after other languages are proven
- if tree-sitter-lua proves too unstable, keep regex parser for Lua only and document the exception
- test against actual Lua files from target projects before committing

### Risk 2. tree-sitter-graph `.tsg` syntax differences across grammars

Why it matters:

- each tree-sitter grammar uses different node type names (e.g. `call_expression` vs `function_call` vs `call`)
- `.tsg` rules must match the exact grammar node types

Mitigation:

- consult each grammar's `node-types.json` before writing `.tsg` rules
- write comprehensive tests for each `.tsg` file with known input/output pairs
- use `tree-sitter parse` CLI tool to inspect AST node types for sample files

### Risk 3. Performance regression on large files

Why it matters:

- tree-sitter parsing adds overhead vs simple regex line scanning
- some very large Lua/Python files could be slower

Mitigation:

- measure per-file parse time for each language before and after
- tree-sitter's incremental parsing capability means re-parses are cheap
- the C++ parser already uses tree-sitter on much larger files without issues

### Risk 4. Dual-extraction comparison may always prefer legacy

Why it matters:

- if the comparison threshold is too strict, tree-sitter results are never used
- if too loose, regressions slip through

Mitigation:

- use 90% symbol threshold and 80% call threshold (same as plan in 5.3)
- add metrics logging to identify which files differ
- manually inspect mismatches to determine if they are improvements or regressions

---

## 8. Definition of Done

MS18 is complete when:

1. `tree-sitter-python`, `tree-sitter-typescript`, `tree-sitter-rust`, `tree-sitter-lua` dependencies added
2. each language has a `.tsg` call relations file
3. each language parser uses tree-sitter AST walking for symbol extraction
4. each language parser uses tree-sitter-graph for call extraction
5. dual-extraction comparison validates tree-sitter results >= legacy
6. legacy regex extraction code removed from all four parsers
7. `cargo build` passes with zero warnings
8. `cargo test` passes with all tests
9. indexing a mixed-language project produces equivalent or improved results

Validation snapshot (2026-04-22):

- `indexer`: `cargo build --release` passed, zero warnings
- `indexer`: `cargo test` passed, 234/234 (21 new tree-sitter tests added)
- `server`: `npm test` passed, 161/161
- opencv workspace indexed: 65,195 symbols, 108,592 calls, 4,602 files
- DB integrity: ok, 0 empty names (excluding 1 anonymous struct), 0 zero-line symbols
- watcher/incremental: add file (+2 symbols), modify file, delete file (−2 symbols) all correct
- dual-extraction: tree-sitter used when symbol count ≥ 90% of legacy, call count ≥ 80%
- TypeScript, Python, Rust, Lua parsers all use tree-sitter + .tsg graph rules via indexing.rs
