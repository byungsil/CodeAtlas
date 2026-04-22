# Milestone 19. Field Access Tracking & Signature Type Reference Coverage

Status:

- In Progress

## 1. Objective

Extend the indexer to capture **field access expressions** (`obj.field`, `ptr->mField`, `this->mField`) and **function signature type references** (parameter types, return types, template type arguments) that are currently missing from reference tracking, closing the largest remaining coverage gap in `find_references`.

This milestone focuses on:

- adding tree-sitter-graph (`.tsg`) rules for standalone field access expressions (not wrapped in `call_expression`)
- extending the legacy AST type-usage walker to cover template type arguments and function return types in declarations
- storing field access records in `raw_calls` with new `call_kind` values (`fieldAccess`, `pointerFieldAccess`, `thisFieldAccess`)
- surfacing field access references in `find_references` via the existing `getMemberAccessReferences()` path
- adding a composite index on `raw_calls(called_name, call_kind)` for query performance
- validating coverage improvement on real projects (client DB: ShotFlags enum test case)

Success outcome:

- `pEvent->mShotFlags` style field access expressions appear in `find_references` results with `category: "memberAccess"` and `confidence: "partial"`
- function signatures in `.h` files that use a type as parameter or return type appear as `typeUsage` references
- template type arguments like `vector<ShotFlags>` are tracked as `typeUsage`
- existing method-call member access extraction is unchanged (no regression)
- `find_references` coverage on the ShotFlags test case improves from 77.8% to 90%+
- indexing performance impact is within 10% of current times
- all existing tests pass, new tests added for field access and signature type extraction

Positioning note:

- MS2 introduced tree-sitter-graph integration for C++ call extraction (method calls only)
- MS18 extended tree-sitter to all supported languages
- MS19 adds the last two major extraction categories that MS2 intentionally deferred: field access and expanded type usage

Scope note:

- this milestone changes indexer (Rust) `.tsg` rules, parser, models, and storage
- server (TypeScript) changes are limited to extending `call_kind` filter lists and adding the DB index
- no new MCP tools are added — field access flows through existing `find_references` with `includeMemberAccess=true`
- field symbols are NOT promoted to first-class symbols (no `type=field` in symbols table) — field access remains unresolved in `raw_calls`
- Python/TypeScript/Rust/Lua field access is out of scope (C/C++ only in this milestone)

---

## 2. Applicability Review

### Current reference tracking architecture

```
Indexing pipeline:
  Source files
    → tree-sitter parse → AST
    → .tsg rules → graph → extract_graph_relation_events() → RawRelationEvent[]
       ↓ (call events)
       → normalize → RawCallSite[] → write_raw_calls() → raw_calls table
    → legacy AST walk → extract_type_usage_relation_events() → RawRelationEvent[]
       ↓ (type/inheritance events)
       → resolve target → NormalizedReference[] → write_references() → symbol_references table

Query pipeline:
  find_references(qualifiedName)
    → getReferences(symbolId) → symbol_references [typeUsage, inheritanceMention, enumValueUsage]
    → getMemberAccessReferences(name, ownerNames) → raw_calls [memberAccess, pointerMemberAccess]
    → merge + deduplicate → ResolvedReference[]
```

### Measured data — current extraction volumes

| table | client DB (EA) | opencv DB | notes |
|---|---|---|---|
| symbols | 312,401 | 65,195 | |
| calls (resolved) | 507,284 | 108,592 | |
| raw_calls (all kinds) | 1,765,880 | 352,457 | |
| raw_calls (memberAccess) | 414,712 | 90,486 | method calls via `.member()` |
| raw_calls (ptrMemberAccess) | 409,358 | 14,422 | method calls via `->member()` |
| symbol_references | 436,310 | 32,684 | |
| symbol_references (typeUsage) | 218,739 | 16,154 | |
| propagation_events | 1,620,000 | 358,009 | has FieldWrite/FieldRead kinds |

### Coverage gap analysis — ShotFlags test case

Total known references: ~860. `find_references` returns 666 (77.8%).

| gap category | count | root cause | solution |
|---|---|---|---|
| field access (`pEvent->mShotFlags`) | 5 | `.tsg` only captures `field_expression` inside `call_expression`; standalone field access ignored | **Epic 1**: add .tsg rules for standalone field_expression |
| header type usage (`.h` signatures) | 9 | legacy AST walker handles `parameter_declaration` type but misses template args (`vector<ShotFlags>`) and some return types | **Epic 2**: extend type_usage extraction to template_argument_list and return types |
| enum definition files | 3 | definition is not a "reference" — structurally correct behavior | out of scope |
| unindexed modules | 3 | workspace config gap | out of scope |
| utility/testbed | 6 | combination of above | covered by Epic 1+2 |

### Tree-sitter node structure for field access

Standalone field access (NOT a function call):
```
; obj.fieldName (assignment target or read)
(field_expression          ; ← NOT wrapped in call_expression
  argument: (identifier)   ; receiver: "obj"
  operator: "."
  field: (field_identifier)) ; field: "fieldName"

; ptr->mShotFlags
(field_expression
  argument: (identifier)   ; receiver: "ptr"
  operator: "->"
  field: (field_identifier)) ; field: "mShotFlags"
```

Method call (already captured):
```
; obj.method() — field_expression INSIDE call_expression
(call_expression
  function: (field_expression
    argument: (identifier)
    operator: "."
    field: (field_identifier))
  arguments: (argument_list))
```

**Key challenge**: tree-sitter-graph `.tsg` rules lack negative predicates (`#not-parent?`). Both patterns match `field_expression`. Solution: capture ALL field_expression in new rules, then deduplicate against call_expression matches in Rust parser code.

### Tree-sitter node structure for template type arguments

```
; vector<ShotFlags>
(template_type
  name: (type_identifier)              ; "vector"
  arguments: (template_argument_list
    (type_descriptor
      type: (type_identifier))))        ; "ShotFlags" ← NOT captured by current walker

; map<string, ShotFlags>
(template_type
  arguments: (template_argument_list
    (type_descriptor type: (type_identifier))   ; "string"
    (type_descriptor type: (type_identifier)))) ; "ShotFlags" ← NOT captured
```

Current `type_usage_target()` (parser.rs:925-938) only matches `type_identifier` and `qualified_identifier` at the top level. It does NOT recurse into `template_argument_list`.

---

## 3. Recommended Order

1. M19-E1. Field access extraction (indexer .tsg + Rust parser)
2. M19-E2. Signature type reference enhancement (indexer Rust AST walker)
3. M19-E3. Server integration & performance optimization
4. M19-E4. Validation and coverage measurement

Why this order:

- E1 is the highest-impact change (5 direct gaps + broader field access coverage across all files) and involves the most complex .tsg work
- E2 is independent of E1 but smaller — extends the legacy AST walker, no .tsg changes
- E3 integrates both E1 and E2 results into the server query path and adds the performance index
- E4 measures actual coverage improvement on real projects

Execution rule:

- finish each Epic to the point of measurable acceptance before starting the next
- `cargo test` and `npm test` must pass after each Epic
- field access records must be verified in the DB before E3 server changes begin

---

## 4. Epic Breakdown

### M19-E1. Field Access Extraction

Status:

- Pending

Goal:

- capture standalone field access expressions (`obj.field`, `ptr->mField`, `this->mField`) in `raw_calls` table with new `call_kind` values, without double-counting method calls

Problem being solved:

- `pEvent->mShotFlags` is a field read, not a method call — `field_expression` node exists but only `call_expression > field_expression` patterns are captured in current `.tsg` rules
- 5 ShotFlags references and potentially thousands of other field accesses are invisible to `find_references`

Design:

**Step 1 — new .tsg rules** in `indexer/graph/cpp_call_relations.tsg`:

Add three new patterns that match standalone field_expression nodes. These will fire for ALL field_expression nodes (including those inside call_expression), so deduplication is needed.

```scheme
; Standalone pointer field access: ptr->field
(field_expression
  argument: (identifier) @receiver
  operator: "->"
  field: (field_identifier) @field_name) @field_access
{
  node @field_access.event
  attr (@field_access.event) relation_kind = "call"
  attr (@field_access.event) call_kind = "pointer_field_access"
  attr (@field_access.event) target_name = (source-text @field_name)
  attr (@field_access.event) receiver = (source-text @receiver)
  attr (@field_access.event) line = (plus (start-row @field_access) 1)
}

; Standalone dot field access: obj.field
(field_expression
  argument: (identifier) @receiver
  operator: "."
  field: (field_identifier) @field_name) @field_access
{
  node @field_access.event
  attr (@field_access.event) relation_kind = "call"
  attr (@field_access.event) call_kind = "field_access"
  attr (@field_access.event) target_name = (source-text @field_name)
  attr (@field_access.event) receiver = (source-text @receiver)
  attr (@field_access.event) line = (plus (start-row @field_access) 1)
}

; this->field access
(field_expression
  argument: (this) @receiver
  field: (field_identifier) @field_name) @field_access
{
  node @field_access.event
  attr (@field_access.event) relation_kind = "call"
  attr (@field_access.event) call_kind = "this_field_access"
  attr (@field_access.event) target_name = (source-text @field_name)
  attr (@field_access.event) receiver = (source-text @receiver)
  attr (@field_access.event) line = (plus (start-row @field_access) 1)
}
```

Note: `relation_kind = "call"` is reused so the existing `extract_graph_relation_events()` infrastructure handles these events without modification to the extraction loop.

**Step 2 — new RawCallKind variants** in `indexer/src/models.rs`:

```rust
pub enum RawCallKind {
    Unqualified,
    MemberAccess,
    PointerMemberAccess,
    ThisPointerAccess,
    Qualified,
    FieldAccess,         // NEW: obj.field
    PointerFieldAccess,  // NEW: ptr->field
    ThisFieldAccess,     // NEW: this->field
}
```

**Step 3 — parse_call_kind mapping** in `indexer/src/parser.rs:2417`:

```rust
fn parse_call_kind(value: &str) -> Option<RawCallKind> {
    match value {
        // ... existing ...
        "field_access" => Some(RawCallKind::FieldAccess),
        "pointer_field_access" => Some(RawCallKind::PointerFieldAccess),
        "this_field_access" => Some(RawCallKind::ThisFieldAccess),
        _ => None,
    }
}
```

**Step 4 — storage serialization** in `indexer/src/storage.rs`:

```rust
fn raw_call_kind_key(kind: &RawCallKind) -> &'static str {
    match kind {
        // ... existing ...
        RawCallKind::FieldAccess => "fieldAccess",
        RawCallKind::PointerFieldAccess => "pointerFieldAccess",
        RawCallKind::ThisFieldAccess => "thisFieldAccess",
    }
}

fn raw_call_kind_from_key(value: &str) -> RawCallKind {
    match value {
        // ... existing ...
        "fieldAccess" => RawCallKind::FieldAccess,
        "pointerFieldAccess" => RawCallKind::PointerFieldAccess,
        "thisFieldAccess" => RawCallKind::ThisFieldAccess,
        _ => RawCallKind::Unqualified,
    }
}
```

**Step 5 — deduplication**: `.tsg` rules will fire for field_expression nodes both inside and outside call_expression. The existing method-call rules (`pointer_member_access`, `member_access`, `this_pointer_access`) ALSO fire for these same nodes. This produces duplicate events at the same (line, target_name, receiver) triplet — one with `call_kind = "pointer_member_access"` and one with `call_kind = "pointer_field_access"`.

Deduplication strategy in `extract_graph_relation_events()`:
- after collecting all events, group by `(line, target_name, receiver)`
- if a group has both a method-call kind AND a field-access kind, keep only the method-call kind
- this preserves backward compatibility: method calls are not affected

```rust
// After collecting all events, deduplicate field access vs method call
let mut seen_calls: HashSet<(usize, String, Option<String>)> = HashSet::new();
for event in &events {
    if matches!(event.call_kind, Some(RawCallKind::MemberAccess | RawCallKind::PointerMemberAccess | RawCallKind::ThisPointerAccess)) {
        let key = (event.line, event.target_name.clone().unwrap_or_default(), event.receiver.clone());
        seen_calls.insert(key);
    }
}
events.retain(|event| {
    if matches!(event.call_kind, Some(RawCallKind::FieldAccess | RawCallKind::PointerFieldAccess | RawCallKind::ThisFieldAccess)) {
        let key = (event.line, event.target_name.clone().unwrap_or_default(), event.receiver.clone());
        !seen_calls.contains(&key)
    } else {
        true
    }
});
```

Implementation tasks:

- M19-E1-T1. Add three new `.tsg` rules for field access in `indexer/graph/cpp_call_relations.tsg`
- M19-E1-T2. Add `FieldAccess`, `PointerFieldAccess`, `ThisFieldAccess` variants to `RawCallKind` enum in `indexer/src/models.rs`
- M19-E1-T3. Update `parse_call_kind()` in `indexer/src/parser.rs` to map new `.tsg` call_kind strings
- M19-E1-T4. Update `raw_call_kind_key()` and `raw_call_kind_from_key()` in `indexer/src/storage.rs`
- M19-E1-T5. Add deduplication logic in `extract_graph_relation_events()` in `indexer/src/parser.rs` to suppress field access events that overlap with method call events at the same (line, target_name, receiver)
- M19-E1-T6. Add Rust unit tests: parse a C++ sample with both `ptr->method()` and `ptr->field` to verify method call is kept and field access is added without duplication
- M19-E1-T7. Run `cargo test` — all existing tests pass + new tests pass
- M19-E1-T8. Test on opencv: full index, verify new `fieldAccess`/`pointerFieldAccess` records in `raw_calls` — `SELECT call_kind, COUNT(*) FROM raw_calls GROUP BY call_kind`

Expected touch points:

- `indexer/graph/cpp_call_relations.tsg`
- `indexer/src/models.rs`
- `indexer/src/parser.rs`
- `indexer/src/storage.rs`

Acceptance:

- `cargo test` passes with all existing + new tests
- opencv DB shows new `fieldAccess` and `pointerFieldAccess` call_kind records in `raw_calls`
- existing `memberAccess` and `pointerMemberAccess` counts are unchanged (no regression)
- deduplication: `SELECT COUNT(*) FROM raw_calls WHERE call_kind = 'pointerFieldAccess' AND (called_name, file_path, line) IN (SELECT called_name, file_path, line FROM raw_calls WHERE call_kind = 'pointerMemberAccess')` returns 0

---

### M19-E2. Signature Type Reference Enhancement

Status:

- Pending

Goal:

- extend the legacy AST type-usage walker to capture template type arguments and function return types that are currently missed, adding them to `symbol_references` as `typeUsage`

Problem being solved:

- `vector<ShotFlags>` in a parameter declaration: current walker finds `vector` as type_identifier but does not recurse into `template_argument_list` to find `ShotFlags`
- function return types in declarations (not definitions) may be missed if the declaration node structure differs
- 9 header files with ShotFlags in function signatures are missing from `find_references`

Design:

**Change 1 — recursive type descent** in `push_type_usage_event()` (parser.rs:872):

Currently `type_usage_target()` (parser.rs:925) only matches `type_identifier` and `qualified_identifier`. It needs to also recurse into:

- `template_type` → extract the template name AND recurse into `template_argument_list`
- `template_argument_list` → for each child `type_descriptor`, recurse into its `type` field
- `pointer_declarator`, `reference_declarator` → unwrap to inner type
- `sized_type_specifier` → skip (built-in types like `unsigned int`)

```rust
fn push_type_usage_events_recursive(node: Node, ctx: &Ctx, events: &mut Vec<RawRelationEvent>) {
    match node.kind() {
        "type_identifier" | "qualified_identifier" => {
            push_type_usage_event(node, ctx, events);
        }
        "template_type" => {
            // Extract the template name itself (e.g., "vector")
            if let Some(name_node) = node.child_by_field_name("name") {
                push_type_usage_events_recursive(name_node, ctx, events);
            }
            // Recurse into template arguments (e.g., "<ShotFlags>")
            if let Some(args) = node.child_by_field_name("arguments") {
                for child in named_children(args) {
                    push_type_usage_events_recursive(child, ctx, events);
                }
            }
        }
        "type_descriptor" => {
            if let Some(type_node) = node.child_by_field_name("type") {
                push_type_usage_events_recursive(type_node, ctx, events);
            }
        }
        "pointer_declarator" | "reference_declarator" | "abstract_pointer_declarator"
        | "abstract_reference_declarator" => {
            for child in named_children(node) {
                push_type_usage_events_recursive(child, ctx, events);
            }
        }
        _ => {}
    }
}
```

**Change 2 — expand walker scope** in `extract_type_usage_relation_events()` (parser.rs:620):

Add `function_definition` and `declaration` return type extraction:

```rust
match node.kind() {
    "declaration" | "field_declaration" | "parameter_declaration" => {
        if let Some(type_node) = node.child_by_field_name("type") {
            push_type_usage_events_recursive(type_node, ctx, &mut events);
        }
    }
    "function_definition" => {
        // Return type
        if let Some(type_node) = node.child_by_field_name("type") {
            push_type_usage_events_recursive(type_node, ctx, &mut events);
        }
        // Continue recursion into function body
        for child in named_children(node) {
            stack.push(child);
        }
    }
    _ => {
        for child in named_children(node) {
            stack.push(child);
        }
    }
}
```

Note: `function_definition` already matches the walker's `_ =>` branch (it recurses into children), but the explicit match ensures we capture the return type at the function_definition level, not just at deeper declaration nodes.

**Change 3 — replace `push_type_usage_event` call with `push_type_usage_events_recursive`** in the walker:

Replace the single `push_type_usage_event(type_node, ...)` call with `push_type_usage_events_recursive(type_node, ...)` so that `vector<ShotFlags>` yields both `vector` and `ShotFlags` as typeUsage events.

Implementation tasks:

- M19-E2-T1. Add `push_type_usage_events_recursive()` function in `indexer/src/parser.rs` that handles `template_type`, `type_descriptor`, `pointer_declarator`, `reference_declarator`, and terminal `type_identifier`/`qualified_identifier` nodes
- M19-E2-T2. Update `extract_type_usage_relation_events()` to use `push_type_usage_events_recursive()` instead of `push_type_usage_event()` for all type nodes
- M19-E2-T3. Add explicit `function_definition` match arm in the walker to capture return types
- M19-E2-T4. Add Rust unit tests: parse C++ sample with `void foo(vector<ShotFlags> param)` and `ShotFlags bar()` — verify both `ShotFlags` and `vector` appear as typeUsage events
- M19-E2-T5. Run `cargo test` — all existing tests pass + new tests pass
- M19-E2-T6. Test on opencv: full index, compare `symbol_references` typeUsage count before and after — expect increase

Expected touch points:

- `indexer/src/parser.rs`

Acceptance:

- `cargo test` passes with all existing + new tests
- opencv DB: `SELECT COUNT(*) FROM symbol_references WHERE category = 'typeUsage'` increases from 16,154
- template type arguments like `vector<ShotFlags>` produce typeUsage references for both `vector` AND `ShotFlags`
- function return types in `.h` declarations produce typeUsage references
- no regression in existing typeUsage counts for non-template types

---

### M19-E3. Server Integration & Performance Optimization

Status:

- Pending

Goal:

- extend server-side queries to include new field access call_kinds in `find_references`
- add composite index on `raw_calls` for query performance
- ensure compact mode and pagination work correctly with field access references

Problem being solved:

- `getMemberAccessReferences()` in `sqlite-store.ts` currently filters `call_kind IN ('memberAccess', 'pointerMemberAccess')` — new field access kinds are excluded
- no index on `(called_name, call_kind)` — as field access adds 2-5x more records, unindexed queries will degrade
- MCP schema `category` enum does not include field access kinds

Design:

**Change 1 — extend call_kind filter** in `sqlite-store.ts:getMemberAccessReferences()`:

```typescript
const filters = [
  "called_name = ?",
  "call_kind IN ('memberAccess', 'pointerMemberAccess', 'fieldAccess', 'pointerFieldAccess', 'thisFieldAccess')",
];
```

**Change 2 — add composite index** in `indexer/src/storage.rs`:

```sql
CREATE INDEX IF NOT EXISTS idx_raw_calls_called_name_kind
ON raw_calls(called_name, call_kind);
```

Add this after line 174 (existing raw_calls indexes).

**Change 3 — update MCP category enum** in `mcp-runtime.ts`:

The `find_references` schema already accepts `category: "memberAccess"` which covers all member/field access. No additional category values needed — field access is surfaced under the same `memberAccess` umbrella.

**Change 4 — reference query safety cap** in `constants.ts` and `sqlite-store.ts`:

Both `getReferences()` and `getMemberAccessReferences()` now apply `LIMIT ${REFERENCE_QUERY_CAP}` (default 2000) to prevent memory pressure on very common symbols. The cap is configurable via MCP env `CODEATLAS_REFERENCE_QUERY_CAP` with a floor of 100.

**Change 5 — O(n) deduplication** in `query-helpers.ts`:

`deduplicateReferences()` replaces the O(n²) `findIndex` dedup pattern in both `mcp-runtime.ts` and `app.ts` with a Set-based O(n) approach keyed on `(sourceSymbolId, targetSymbolId, category, filePath, line)`.

Implementation tasks:

- M19-E3-T1. Add composite index `idx_raw_calls_called_name_kind` in `indexer/src/storage.rs` after line 174
- M19-E3-T2. Update `getMemberAccessReferences()` in `server/src/storage/sqlite-store.ts` to include `fieldAccess`, `pointerFieldAccess`, `thisFieldAccess` in the `call_kind IN (...)` filter
- M19-E3-T3. Apply same change in `server/src/storage/json-store.ts` (stub — no behavior change needed, just signature compatibility)
- M19-E3-T4. Add `REFERENCE_QUERY_CAP` constant (env-configurable) and apply `LIMIT` to both DB queries in `sqlite-store.ts`
- M19-E3-T5. Extract `deduplicateReferences()` to `query-helpers.ts` and replace O(n²) `findIndex` dedup in both `mcp-runtime.ts` and `app.ts`
- M19-E3-T6. Run `npm run build` + `npm test -- --runInBand` — all tests pass
- M19-E3-T7. Run full index on opencv to create DB with new index
- M19-E3-T8. Verify query performance: `find_references` for a common field name returns in <100ms

Expected touch points:

- `indexer/src/storage.rs`
- `server/src/storage/sqlite-store.ts`
- `server/src/constants.ts`
- `server/src/query-helpers.ts`
- `server/src/mcp-runtime.ts`
- `server/src/app.ts`

Acceptance:

- `npm run build` + `npm test` passes (161/161)
- opencv DB has `idx_raw_calls_called_name_kind` index — `SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name='idx_raw_calls_called_name_kind'` returns 1
- `find_references` for a method with field access returns both method calls AND field accesses
- compact mode correctly groups field access references into fileGroups
- pagination (`offset`/`limit`/`nextOffset`) works across mixed reference types

---

### M19-E4. Validation and Coverage Measurement

Status:

- Pending

Goal:

- validate end-to-end coverage improvement on real projects
- run all existing MCP smoke tests
- measure ShotFlags test case improvement

Problem being solved:

- need to prove the indexer + server changes actually close the coverage gap measured at 77.8%
- need to ensure no regression in existing functionality

Design:

Run the following validation sequence:

1. **Indexer tests**: `cargo test` — all pass
2. **Server tests**: `npm test -- --runInBand` — all pass
3. **Full index on opencv**: verify field access and typeUsage counts
4. **MCP smoke test on opencv**: run `tmp-opencv-smoke.js` — 30/30 pass
5. **Full index on client DB**: verify coverage on ShotFlags
6. **Watcher test**: verify incremental indexing captures field access for new/modified files

Implementation tasks:

- M19-E4-T1. Run `cargo test` — all existing + new M19 tests pass
- M19-E4-T2. Run `npm test -- --runInBand` — 161+ tests pass
- M19-E4-T3. Full index on opencv, verify: `SELECT call_kind, COUNT(*) FROM raw_calls GROUP BY call_kind` — shows fieldAccess/pointerFieldAccess/thisFieldAccess records; `SELECT category, COUNT(*) FROM symbol_references GROUP BY category` — typeUsage count increased
- M19-E4-T4. Run MCP smoke test against opencv — all tests pass
- M19-E4-T5. Full index on client DB, run ShotFlags coverage test: `find_references` for ShotFlags-related symbols, measure total count improvement from 666
- M19-E4-T6. Watcher incremental test: start watcher, add file with field access + template types, verify new records appear in DB
- M19-E4-T7. Performance measurement: compare full index time on opencv before/after (target: within 10% of current 110s)

Expected touch points:

- test scripts only (no production code changes)

Acceptance:

- all unit tests pass (cargo test + npm test)
- MCP smoke test 30/30 on opencv
- opencv raw_calls shows fieldAccess records — estimated 50K-200K new records
- opencv symbol_references typeUsage count > 16,154 (current baseline)
- ShotFlags test case coverage > 90% (up from 77.8%)
- full index time on opencv < 125s (within 10% of current 110s)
- watcher incremental correctly tracks field access in new/modified files

---

## 5. Detailed Execution Plan

### Phase 1: Indexer field access (M19-E1)

Primary outcome: raw_calls table populated with fieldAccess/pointerFieldAccess/thisFieldAccess records

Tasks: M19-E1-T1 through M19-E1-T8

Why first: highest-impact gap (field access), requires `.tsg` rule design + dedup logic which is the most complex part

Completion gate: `cargo test` passes, opencv DB shows new call_kind records, no duplicate entries with existing method call records

### Phase 2: Indexer type usage (M19-E2)

Primary outcome: symbol_references table contains template type argument references

Tasks: M19-E2-T1 through M19-E2-T6

Why second: independent of E1, extends proven AST walker pattern, lower risk

Completion gate: `cargo test` passes, opencv typeUsage count increases, `vector<T>` template args tracked

### Phase 3: Server + performance (M19-E3)

Primary outcome: find_references returns field access references, query performance optimized

Tasks: M19-E3-T1 through M19-E3-T6

Why third: depends on E1 (new call_kinds in DB) and benefits from E2 (more typeUsage)

Completion gate: `npm test` passes, find_references includes fieldAccess results, index exists in DB

### Phase 4: Validation (M19-E4)

Primary outcome: measured coverage improvement, no regressions

Tasks: M19-E4-T1 through M19-E4-T7

Why last: validates all previous work end-to-end

Completion gate: ShotFlags coverage > 90%, all smoke tests pass, performance within bounds

---

## 6. Task Breakdown By File

### `indexer/graph/cpp_call_relations.tsg`

Planned tasks:
- Add pointer field access rule: `(field_expression operator: "->" ...)` → `call_kind = "pointer_field_access"` (M19-E1-T1)
- Add dot field access rule: `(field_expression operator: "." ...)` → `call_kind = "field_access"` (M19-E1-T1)
- Add this field access rule: `(field_expression argument: (this) ...)` → `call_kind = "this_field_access"` (M19-E1-T1)

### `indexer/src/models.rs`

Planned tasks:
- Add `FieldAccess`, `PointerFieldAccess`, `ThisFieldAccess` to `RawCallKind` enum (M19-E1-T2)

### `indexer/src/parser.rs`

Planned tasks:
- Update `parse_call_kind()` to map `"field_access"` → `FieldAccess`, `"pointer_field_access"` → `PointerFieldAccess`, `"this_field_access"` → `ThisFieldAccess` (M19-E1-T3)
- Add deduplication logic after event collection: suppress field access events that share (line, target_name, receiver) with method call events (M19-E1-T5)
- Add `push_type_usage_events_recursive()` function for template type descent (M19-E2-T1)
- Replace `push_type_usage_event()` calls with `push_type_usage_events_recursive()` (M19-E2-T2)
- Add `function_definition` match arm for return type extraction (M19-E2-T3)
- Add unit tests for field access extraction and deduplication (M19-E1-T6)
- Add unit tests for template type usage extraction (M19-E2-T4)

### `indexer/src/storage.rs`

Planned tasks:
- Update `raw_call_kind_key()` with three new variants (M19-E1-T4)
- Update `raw_call_kind_from_key()` with three new variants (M19-E1-T4)
- Add `CREATE INDEX IF NOT EXISTS idx_raw_calls_called_name_kind ON raw_calls(called_name, call_kind)` (M19-E3-T1)

### `server/src/storage/sqlite-store.ts`

Planned tasks:
- Extend `call_kind IN (...)` filter in `getMemberAccessReferences()` to include `'fieldAccess', 'pointerFieldAccess', 'thisFieldAccess'` (M19-E3-T2)

---

## 7. Cross-Epic Risks

### Risk 1. Field access volume explosion

Why it matters:
- field access is far more common than method calls — client DB has ~824K method-access records, field access could add 2-5x more
- raw_calls table could grow from 1.77M to 3-5M records
- DB size and indexing time will increase

Mitigation:
- composite index `(called_name, call_kind)` ensures query time remains O(log N) regardless of volume
- `argument_count` is NULL for field access (no arguments) — saves storage
- `argument_texts_json` defaults to `"[]"` for field access — minimal JSON overhead
- monitor indexing time: if > 10% regression, consider batching or deferred field access extraction

### Risk 2. .tsg rule duplication with method calls

Why it matters:
- new field access `.tsg` rules match the same `field_expression` nodes as existing method call rules
- without deduplication, every `obj.method()` call would produce TWO raw_calls records

Mitigation:
- deduplication in `extract_graph_relation_events()` runs after all events are collected
- method call events take priority (they have richer data: argument_count, argument_texts)
- unit test explicitly verifies no duplication for `obj.method()` pattern

### Risk 3. Template type recursion depth

Why it matters:
- deeply nested templates like `map<string, vector<shared_ptr<ShotFlags>>>` could produce excessive recursion
- unlikely to cause stack overflow (C++ templates rarely exceed 5-6 levels) but could produce many typeUsage events per declaration

Mitigation:
- the recursive function is bounded by AST depth (tree-sitter guarantees finite trees)
- add a simple depth limit (e.g., 8 levels) as safety guard
- each typeUsage event is small (source_id, target_id, category, file, line) — volume impact is modest

---

## 8. Definition of Done

1. `cargo test` passes — all existing + new M19 tests
2. `npm run build` + `npm test -- --runInBand` passes — 161+ tests
3. opencv full index: `raw_calls` shows `fieldAccess`/`pointerFieldAccess`/`thisFieldAccess` records
4. opencv full index: `symbol_references` typeUsage count > 16,154 (baseline)
5. opencv MCP smoke test: 30/30 pass
6. `find_references` with `includeMemberAccess=true` returns field access references
7. composite index `idx_raw_calls_called_name_kind` exists in DB
8. no duplicate raw_calls records for method calls that also have field access patterns
9. watcher incremental test: field access and template type references tracked for new files
10. full index time on opencv < 125s

Validation snapshot:

```
cargo test               → 234+ tests passed
npm test -- --runInBand  → 161+ tests passed
opencv raw_calls:
  fieldAccess:        N records (NEW)
  pointerFieldAccess: N records (NEW)
  thisFieldAccess:    N records (NEW)
  memberAccess:       90,486 (UNCHANGED)
opencv symbol_references:
  typeUsage:          > 16,154 (INCREASED)
MCP smoke test:         30/30 pass
Full index time:        < 125s
```

---

## 9. Suggested First Implementation Slice

Start with M19-E1-T1 through M19-E1-T4 (add .tsg rules + model/parser/storage changes) WITHOUT deduplication, then immediately run on a small sample file to verify field access events appear. Then add deduplication (M19-E1-T5) and verify no method call duplication.

Why this slice first: it proves the .tsg rules fire correctly and the full pipeline (parse → extract → store) works end-to-end before investing in the more nuanced deduplication logic.
