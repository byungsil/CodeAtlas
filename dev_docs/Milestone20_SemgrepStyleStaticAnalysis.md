# Milestone 20: Semgrep-Style Static Analysis Upgrade

**Goal**: Enhance CodeAtlas indexing capabilities to provide deeper code understanding for AI agents via MCP tools. Not a full Semgrep clone — focused on structural analysis that complements existing symbol/indexing infrastructure.

## Problem Statement

Current CodeAtlas provides excellent **structural index** (symbols, call edges, propagation) but lacks:
1. Type inference from usage context (variables' actual types remain unknown beyond declaration)
2. Cross-boundary data flow tracking (propagation stops at function boundaries)
3. Pattern-based structural analysis (factory/observer/singleton patterns not detected during indexing)

Agents querying via MCP tools get `call` edges but cannot answer: "What type does this variable hold?", "How does user input flow to SQL execution?", or "Is this a factory method?".

## Architecture Overview

```
┌───────────── Layer 5 ─────────────┐
│ Pattern Analysis       │ Factory, Observer, Singleton detection     │
│ (Structural Patterns)  │ Code smell / anti-pattern recognition      │
├───────────── Layer 4 ─────────────┤
│ Cross-Boundary Flow    │ argument→parameter + return value tagging   │
│                        │ source/sink semantic markers                │
├───────────── Layer 3 ─────────────┤
│ Type Inference         │ Variable type from assignment context       │
│ (Semgrep-style)        │ Return type from callee signature           │
├───────────── Layer 2 ─────────────┤
│ Data Flow              │ Existing propagation (assignment, fieldWrite)│
│                        │ Bounded within function scope               │
├───────────── Layer 1 ─────────────┤
│ Symbol Index           │ Functions, classes, methods, variables       │
│ Call Graph             │ Direct call edges                          │
│ References             │ Type usage, inheritance                    │
└───────────────────────────────────┘
```

## Implementation Phases

### Phase 1: Enhanced Type Inference (Layer 3) — Priority P0

**Problem**: Variable `x` declared as unknown type. Agent cannot reason about what methods are available on it.

**Solution**: Indexing phase extracts type information from assignment expressions and return statements.

#### New Model Fields

```rust
// indexer/src/models.rs - 추가 필드들

#[derive(Debug, Clone)]
pub struct TypeInferenceResult {
    pub symbol_id: String,          // 함수/변수 ID  
    pub inferred_type: Option<String>,  // "std::string", "Widget*", etc.
    pub confidence: InferredTypeConfidence,
    pub evidence_sources: Vec<TypeEvidenceSource>,
}

pub enum TypeEvidenceSource {
    ReturnExpression(String),       // return expression에서 추출 (new Widget() → "Widget*")
    ParameterSignature(String),     // parameter type signature
    AssignmentContext(Vec<String>),  // 할당 문맥 분석 결과
    CallSiteInference(String),      // 호출 결과로 역추론된 타입
}

pub enum InferredTypeConfidence {
    High,       // 명확한 expression 기반 추론 가능
    Partial,    // 여러 evidence가 일관되지만 직접 확인 못 함  
    Unresolved, // insufficient data
}

// Symbol 모델에 추가 필드들:
#[serde(skip_serializing_if = "Option::is_none")]
pub inferred_types: Option<Vec<String>>,  // 다중 타입 가능 (overload 등)
#[serde(skip_serializing_if = "Option::is_none")]  
pub type_inference_confidence: Option<InferredTypeConfidence>,
```

#### Database Schema Extension

```sql
-- indexer/src/storage.rs - 새 테이블 추가

CREATE TABLE symbol_type_inferences (
    symbol_id TEXT PRIMARY KEY REFERENCES symbols(id),
    inferred_type TEXT NOT NULL,           -- "std::string", "Widget*", etc.  
    confidence TEXT CHECK(confidence IN ('high', 'partial', 'unresolved')),
    evidence_sources JSON NOT NULL         -- ["return_expr:new Widget()", ...]
);

CREATE INDEX idx_symbol_inference_type ON symbol_type_inferences(inferred_type);
```

#### Parser Enhancement (indexer/src/parser.rs)

C++ parser에 type inference logic 추가:

1. **Return Expression Analysis**: 함수의 return expression에서 타입 추출
   - `return new Widget()` → inferred_type = "Widget*"
   - `return std::string("hello")` → inferred_type = "std::string"  
   - `return x + y` (where x,y are strings) → inferred_type = "int" or complex

2. **Assignment Context Analysis**: 변수 할당문에서 타입 추출
   - `$VAR = some_function()` → function return type과 일치시 tagging
   - `$OBJ->method()` → method return type으로 역추론 가능

3. **Call Site Inference**: 호출 결과로 받은 변수가 어떤 메서드를 호출하는지로 reverse-infer

#### Implementation Tasks (Phase 1)

```markdown
## Phase 1: Type Inference Implementation Tasks

### Task 1.1: Database Schema Migration
- [ ] Add `symbol_type_inferences` table to storage.rs  
- [ ] Update schema version in constants.rs
- [ ] Handle migration for existing databases

### Task 1.2: C++ Parser Enhancement (parser.rs)
- [ ] Extract return expression types from AST nodes
- [ ] Implement assignment context type analysis  
- [ ] Add call site inference logic

### Task 1.3: Multi-Language Type Inference
- [ ] Python parser: infer types from variable assignments and function returns
- [ ] TypeScript parser: leverage existing TSX/TS type information
- [ ] Rust parser: extract inferred types from expression analysis

### Task 1.4: Resolver Integration  
- [ ] Merge type inference results with symbol resolution phase
- [ ] Handle overloaded functions (multiple possible return types)
- [ ] Confidence scoring based on evidence quality

### Task 1.5: Storage & Query Support
- [ ] Write/read type inferences to/from SQLite database
- [ ] Add query endpoints for enhanced symbol lookup
```

---

### Phase 2: Cross-Boundary Flow Tracking (Layer 4) — Priority P0

**Problem**: Propagation stops at function boundaries. Agent cannot answer "How does user input flow through multiple functions?".

**Solution**: Tag propagation events with semantic markers and track across call graph edges.

#### New Model Fields

```rust
// indexer/src/models.rs - 새로운 타입들

#[derive(Debug, Clone)]  
pub enum FlowKind {
    UserInput,                    // stdin, file.read(), request param 등
    ConfigValue,                  // 설정 파일에서 읽은 값
    ComputedValue,                // 계산 결과 (intermediate)
    ConstantLiteral,              // 하드코딩된 리터럴 ("SELECT * FROM users")  
}

#[derive(Debug)]
pub struct FlowTag {
    pub kind: FlowKind,
    pub label: String,            // "user_id", "config_path" 등 human-readable
}

// 기존 propagation event에 flow tag 추가
#[derive(Debug, Clone)]
pub struct PropagationEventWithTags {  // 또는 새 테이블로 분리  
    pub base_event: PropagationEvent,   // 기존 구조 유지
    pub source_flow_tags: Vec<FlowTag>, // NEW! 
    pub target_flow_tags: Vec<FlowTag>, // NEW!
}

// Cross-boundary flow path representation (server-side)
pub struct DataFlowPath {
    pub source_id: String,        // 시작 심볼 ID  
    pub sink_id: Option<String>,  // 끝 심볼 ID (optional - trace without specific target)
    pub hops: Vec<FlowHop>,       // call boundary를 넘는 hop들
    pub semantic_tags: Vec<String>,  // "user_input", "config_data" 등
}

pub struct FlowHop {
    from_symbol: String,         // 호출한 함수/변수  
    to_symbol: String,           // 호출받은 함수/변수
    transfer_kind: TransferKind, // argumentToParameter / returnValue / fieldWrite...
    value_transformed: bool,     // 값이 변형되었는가 (true면 단순 pass-through 아님)
}

pub enum TransferKind {
    DirectPassThrough,           // 인자 그대로 전달 (arg→param)
    Transformed,                 // 계산/변환 후 전달  
    FieldStorage,                // 객체 상태에 저장 (fieldWrite → fieldRead)
    SplitBranch,                 // 조건부 분기 (if-else로 다른 flow) — agent에게 중요!
}
```

#### Database Schema Extension

```sql
-- indexer/src/storage.rs - 새 테이블 추가  

CREATE TABLE symbol_flow_tags (
    symbol_id TEXT NOT NULL REFERENCES symbols(id),  -- 함수/변수 ID
    tag_kind TEXT CHECK(tag_kind IN ('user_input', 'config_value', 'computed')),
    label TEXT,                                      // "username", "file_path" 등  
    confidence TEXT CHECK(confidence IN ('high', 'partial'))
);

CREATE TABLE cross_boundary_flow_paths (  -- pre-computed flow paths for fast queries
    source_symbol_id TEXT NOT NULL REFERENCES symbols(id),
    target_symbol_id TEXT NOT NULL REFERENCES symbols(id),
    hops JSON NOT NULL,                       // [{from, to, kind, transformed}] array  
    semantic_tags JSON NOT NULL               // ["user_input", "sql_exec"] 등
);

CREATE INDEX idx_flow_source ON cross_boundary_flow_paths(source_symbol_id);
```

#### Cross-Boundary Flow Analysis Module (indexer/src/cross_flow.rs) — **새 파일**

Function call resolution 단계에서 argument→parameter flow를 semantic tag와 함께 기록:

1. Call resolution 시 `resolve_calls_with_db()` 함수에 enhancement
2. Argument expression 분석 → source/sink detection rules 적용  
3. Cross-boundary hop 생성 및 저장

#### Implementation Tasks (Phase 2)

```markdown
## Phase 2: Cross-Boundary Flow Tracking Tasks  

### Task 2.1: Source/Sink Detection Rules Engine
- [ ] Define semantic tagging rules for each language (C++, Python, TypeScript)
- [ ] Implement source detection patterns (user_input, config_value, etc.)  
- [ ] Implement sink detection patterns (sql_exec, system_call, file_write)

### Task 2.2: Call Resolution Enhancement (resolver.rs)
- [ ] Modify resolve_calls_with_db() to track argument→parameter flow with tags
- [ ] Add value_transformed flag based on expression analysis
- [ ] Handle SplitBranch detection for conditional flows

### Task 2.3: Cross-Boundary Flow Storage  
- [ ] Implement cross_flow.rs module for flow path computation
- [ ] Pre-compute common flow paths during indexing phase (Layer 4)
- [ ] Store in symbol_flow_tags and cross_boundary_flow_paths tables

### Task 2.4: MCP Tool Integration (server/src/mcp.ts)
- [ ] Add trace_cross_function_flow() tool  
- [ ] Implement source→sink path finding logic
- [ ] Return structured flow paths with hop details for agent consumption
```

---

### Phase 3: Pattern-Based Structural Analysis (Layer 5) — Priority P1

**Problem**: Agent cannot answer "Is this function a factory?", "Does this class follow observer pattern?".

**Solution**: During indexing, detect structural patterns from AST and tag symbols accordingly.

#### New Model Fields

```rust
// indexer/src/models.rs - 패턴 분석 결과 모델

#[derive(Debug)]  
pub struct StructuralPattern {
    pub id: String,              // "factory_pattern", "singleton" 등
    pub language: SourceLanguage,  // cpp, python, typescript...
    pub confidence: PatternConfidence, 
    pub symbol_id: Option<String>,   // 적용된 심볼 (optional - class-wide patterns)  
    pub file_path: String,           // 패턴이 감지된 파일 경로
}

pub enum PatternCategory {
    DesignPattern,         // Factory, Observer, Singleton 등
    CodeSmell,            // Raw pointer usage, mutable default args  
    SecurityRisk,          // Use-after-free risk, buffer overflow patterns
    PerformanceHint,       // Unnecessary copies in loops
    StyleViolation,        // Missing include guard, const-correctness
}

#[derive(Debug)]
pub struct PatternResult {
    pub pattern_id: String,           // "cpp-no-virtual-destructor" 등  
    pub category: PatternCategory,     // 코드 냄새 / 보안 위험 등 분류
    pub symbol_id: Option<String>,     // 관련 심볼 (optional)
    pub file_path: String,            // 위반/패턴이 감지된 파일 경로
    pub line_start: usize,            // 시작 라인  
    pub line_end: usize,              // 종료 라인
    pub match_text: String,           // 실제 코드 매칭 텍스트 (snippet)
    pub description: String,          // 사람이 읽을 수 있는 설명
}

#[derive(Debug)] 
pub struct AnalysisRuleset {  // ruleset 정의 파일용  
    pub name: String,              // "cpp-anti-patterns", "python-security" 등
    pub language: SourceLanguage,   // 적용 언어
    pub patterns: Vec<PatternSpec>, // 패턴 스펙 배열
}

pub struct PatternSpec {  // 각 패턴 정의  
    pub id: String,                // 규칙 ID (예: "no-virtual-destructor")
    pub category: PatternCategory,  // 코드 분류
    pub severity: SeverityLevel,     // info/warning/error
    pub description: String,         // 설명 텍스트
    pub tsg_pattern: Option<String>, // tree-sitter-graph pattern string  
    pub check_fn_name: String,       // Rust 함수명 (AST 스캔용)
}

pub enum SeverityLevel {
    Info,      // informational hint only
    Warning,   // should be reviewed but not critical
    Error,     // definite bug or security risk
}
```

#### Ruleset Management System

YAML-based ruleset 파일로 패턴 정의 관리 (Semgrep 스타일):

```yaml
# .codeatlas/rules/cpp/anti_patterns.yaml - Semgrep-like YAML 규칙셋  

ruleset_name: "cpp-anti-patterns"  
language: cpp
patterns:
  - id: no-virtual-destructor
    severity: warning
    category: code_smell
    description: "Derived class without virtual destructor — undefined behavior on delete base*"
    
  - id: raw-pointer-member
    severity: info
    category: memory_risk  
    description: "Class member uses raw pointer (potential leak)"

  - id: missing-include-guard
    severity: warning
    category: build_risk
    description: "Header file without include guard — potential multiple definition errors"
```

#### Database Schema Extension

```sql
-- indexer/src/storage.rs - 새 테이블들  

CREATE TABLE analysis_rules (
    rule_id TEXT PRIMARY KEY,       -- "cpp-no-virtual-destructor" 등  
    ruleset_name TEXT NOT NULL,     -- 규칙셋 이름 ("cpp-anti-patterns")
    language TEXT CHECK(language IN ('cpp', 'python', 'typescript', 'rust')),  -- 적용 언어
    category TEXT NOT NULL,         // code_smell / security_risk / build_risk 등  
    severity TEXT NOT NULL,         // info/warning/error  
    description TEXT NOT NULL,      // 사람이 읽을 수 있는 설명
    pattern_tsg TEXT                -- tsg rule source (inline) 또는 check_fn_name  
);

CREATE TABLE symbol_analysis_results (  -- 분석 결과 저장용
    result_id INTEGER PRIMARY KEY AUTOINCREMENT,
    rule_id TEXT REFERENCES analysis_rules(rule_id),
    file_path TEXT NOT NULL,            // 위반이 발생한 파일 경로
    line_start INTEGER NOT NULL,        // 시작 라인
    line_end INTEGER NOT NULL,          // 종료 라인  
    match_text TEXT NOT NULL,           // 실제 코드 매칭 텍스트 (snippet)
    symbol_id TEXT REFERENCES symbols(id),  -- 관련 심볼 ID (optional - class-wide patterns 등)
);

CREATE INDEX idx_analysis_rule ON symbol_analysis_results(rule_id);
```

#### Pattern Detection Implementation (indexer/src/analysis_patterns.rs) — **새 파일**

AST 스캔 기반 패턴 감지 로직:

1. `check_fn_name`에 따라 Rust 함수 호출하여 AST 노드 분석  
2. Semgrep-style pattern 매칭 → tsg rule compilation으로 변환
3. 감지된 패턴을 symbol_analysis_results 테이블에 저장

#### Implementation Tasks (Phase 3)

```markdown
## Phase 3: Pattern-Based Structural Analysis Tasks

### Task 3.1: Ruleset YAML Parser  
- [ ] Implement ruleset.yaml file parser in indexer/src/ruleset_parser.rs
- [ ] Validate pattern definitions and map to check functions
- [ ] Handle language-specific rule sets (cpp, python, typescript)  

### Task 3.2: Pattern Detection Functions (analysis_patterns.rs)
- [ ] C++ patterns: no-virtual-destructor, raw-pointer-member, missing-guard  
- [ ] Python patterns: mutable-default-arg, bare-except, use-after-free-risk
- [ ] TypeScript patterns: any-type-overuse, unhandled-promise-rejection

### Task 3.3: Integration with Indexing Pipeline  
- [ ] Add pattern detection phase after symbol merge step in main.rs run_full()
- [ ] Store results to analysis_rules and symbol_analysis_results tables
- [ ] Handle incremental indexing for changed files (re-run patterns)

### Task 4.1: MCP Tool Enhancement (server/src/mcp.ts)  
- [ ] Add analyze_file(filePath, categories?) tool — 파일별 분석 결과 반환
- [ ] Add list_analysis_rules(category?, language?) tool — 사용 가능한 규칙 목록 조회  
- [ ] Add get_symbol_analysis(symbolId) tool — 심볼 관련 패턴 분석 결과 통합 조회

### Task 4.2: HTTP API Enhancement (server/src/app.ts)
- [ ] GET /analysis/rules?category=&language= — ruleset 조회 endpoint
- [ ] POST /analysis/evaluate/{ruleIds} — workspace against rule(s) 실행  
- [ ] GET /analysis/results?filePath=&symbolId=&severityMin= — 분석 결과 조회

### Task 4.3: Dashboard UI Enhancement (server/public/)
- [ ] Analysis Results tab in dashboard showing violations by severity/category
- [ ] Pattern detection results viewable alongside symbol lookup
```

---

## MCP Tool Specifications (Enhanced)

기존 도구들에 enhancement + 신규 tools 추가:

### Enhanced Existing Tools

| 기존 Tool | Enhancement Description | Agent Benefit |
|-----------|------------------------|---------------|
| `lookup_symbol({ qualifiedName })` | **inferred_types** field 추가 — 함수의 실제 반환 타입, 변수의 할당된 타입 등. Semgrep-style type inference 결과 포함. | "이 함수가 실제로 무엇을 반환하는지" 확신 가능 |
| `find_references({ symbolId })` | **flow_tags** 필드 추가 — 해당 심볼에 붙은 semantic flow tags (user_input, config_value 등) 표시 | 데이터 흐름의 시작점/중간점을 쉽게 파악 |

### New MCP Tools

#### 1. `get_enhanced_symbol()` — Enhanced Symbol Lookup with Type Inference + Analysis Tags

```typescript
// server/src/mcp.ts 에 추가할 tool definition

MCP_TOOL: get_enhanced_symbol({ 
    qualifiedName: z.string(),           // 필수 - 심볼 ID  
    includeTypeInference?: z.boolean().optional(),  // default true — 타입 추론 결과 포함 여부
    includeAnalysisTags?: z.boolean().optional(),   // default true — 패턴 분석 태그 포함 여부  
})

// Response structure (enhanced Symbol):
{
    lookupMode: "exact",
    symbol: { 
        id: "...", qualifiedName: "...", type: "...", ...existing fields...
        
        // NEW from Phase 1: Type Inference
        inferred_types?: ["std::string", "const char*"],  
        type_inference_confidence?: "high" | "partial" | "unresolved",
        
        // NEW from Phase 3: Analysis Tags (pattern detection)
        analysis_tags?: [
            { pattern_id: "cpp-no-virtual-destructor", severity: "warning", description: "..."},  
            { pattern_id: "factory_pattern", category: "design_pattern" }
        ]
    },
    
    // NEW from Phase 2: Flow Tags (if symbol is a data source/sink)
    flow_tags?: [
        { kind: "user_input", label: "username" },  
        { kind: "config_value", label: "db_connection_string" }
    ]
}

// Agent use case example:
// User: "createUser 함수가 무엇을 반환하는지 알려줘"
// MCP tool call → get_enhanced_symbol({ qualifiedName: "UserService::createUser" })  
// Response includes inferred_types=["std::string"] + analysis_tags=[factory_pattern]
// Agent can now confidently explain the function's purpose and return value.
```

#### 2. `trace_cross_function_flow()` — End-to-End Data Flow Tracing (Phase 2)

```typescript
MCP_TOOL: trace_cross_function_flow({ 
    sourceSymbolId: z.string(),          // 시작 심볼 ID (함수/변수)  
    maxDepth?: z.number().optional(),    // default 5, 최대 추적 깊이
})

// Response structure — Semgrep taint flow style but adapted for code understanding:
{
    pathFound: true | false,             // flow path가 발견되었는지 여부  
    sourceSymbolId: "...",               // 시작 심볼 ID
    sinkSymbolId?: "...",                // 끝 심볼 (optional - 특정 target 없이 trace)
    
    hops: [                             // call boundary를 넘는 hop들
        { 
            from_symbol: "parseUserInput()",  // 호출한 함수  
            to_symbol: "validateUsername()",   // 호출받은 함수,
            transfer_kind: "DirectPassThrough",  // 인자 그대로 전달
            value_transformed: false              // 값이 변형되지 않음 (pure pass-through)
        },
        { 
            from_symbol: "validateUsername()",  
            to_symbol: "db.execute(sql)",   
            transfer_kind: "Transformed",          // SQL query 생성으로 변환됨  
            value_transformed: true                 // user input이 SQL string으로 transform됨
        }
    ],
    
    semantic_tags: ["user_input", "sql_exec"],  // 전체 flow의 시맨틱 태그들 (source→sink)
    risks: [                    // flow 중 발견된 위험 요소  
        { 
            hop_index: 1,       // 두 번째 hop에서 발생  
            risk_type: "sql_injection_risk",  // SQL injection 가능성  
            description: "User input directly interpolated into SQL query"  // agent에게 설명 제공
        }
    ]
}

// Agent use case example:
// User: "user_id가 시스템 전체를 어떻게 흐르는지 tracing 해줘"
// MCP tool call → trace_cross_function_flow({ sourceSymbolId: "getUserId", maxDepth: 10 })  
// Response shows full path from input→validation→storage→output with semantic tags and risk markers.
```

#### 3. `analyze_file()` — Pattern-Based File Analysis (Phase 3)

```typescript
MCP_TOOL: analyze_file({ 
    filePath: z.string(),                // 필수 - 분석할 파일 경로  
    categories?: z.array(z.enum(['code_smell', 'security_risk', 'build_risk'])).optional(),
                                    // 카테고리 필터링 (omit=all)
})

// Response structure — Semgrep-like analysis results for a single file:
{
    filePath: "...",              // 분석된 파일 경로  
    total_violations: 3,          // 총 위반 수
    violations_by_severity: {     // severity별 분류
        error: 1, warning: 2, info: 0
    },
    
    results: [                    // 감지된 패턴/위반 목록
        { 
            rule_id: "cpp-no-virtual-destructor",  
            category: "code_smell", severity: "warning",
            line_start: 15, line_end: 42,      // 위반 위치 (범위)
            match_text: "class DerivedWidget : public Widget {\n    void update();\n};",  // 실제 코드 snippet  
            description: "Derived class without virtual destructor — undefined behavior on delete base*"  // 사람이 읽을 수 있는 설명
        },
        
        { 
            rule_id: "missing-include-guard",
            category: "build_risk", severity: "warning",
            line_start: 1, line_end: 5, match_text: "#pragma once (not detected)", description: "..."}  
    ]
}

// Agent use case example:
// User: "widget.cpp 파일에 code smell이나 security risk가 있는지 확인해줘"
// MCP tool call → analyze_file({ filePath: "src/widget.cpp", categories: ['code_smell', 'security_risk'] })
// Response returns detected patterns with descriptions — agent can explain issues to user.
```

#### 4. `list_analysis_rules()` — Available Analysis Ruleset (Phase 3)

```typescript  
MCP_TOOL: list_analysis_rules({ 
    category?: z.enum(['code_smell', 'security_risk', 'build_risk']).optional(), // 카테고리 필터링
    language?: z.enum(['cpp', 'python', 'typescript', 'rust', 'lua']).optional(),  // 언어 필터링  
})

// Response structure — available rulesets for agent to query:
{
    total_rules: 15,                  // 총 규칙 수 (filtered)
    
    rules_by_category: {              // 카테고리별 분류
        code_smell: [
            { rule_id: "cpp-no-virtual-destructor", severity: "warning", description: "..."},  
            { rule_id: "python-mutable-default-arg", severity: "error",  description: "..."}
        ],
        security_risk: [...],         // 보안 관련 규칙들
        build_risk: [...]             // 빌드/구성 관련 규칙들
    }
}

// Agent use case example:  
// User: "C++ 파일에서 발견할 수 있는 코드 냄새 패턴들이 뭐가 있어?"
// MCP tool call → list_analysis_rules({ language: 'cpp', category: 'code_smell' })
// Response lists available rules — agent can then suggest specific checks.
```

---

## Server-Side API Enhancements (server/src/app.ts)

기존 HTTP endpoints에 enhancement + new analysis endpoints 추가:

### GET /enhanced-symbol/{qualifiedName} — Phase 1 & 2 Integration

```typescript
// 기존 /symbol endpoint 대신 enhanced version 제공  
GET /enhanced-symbol?qualifiedName=UserService::createUser&includeTypeInference=true&includeAnalysisTags=true

Response (enhanced Symbol): {
    lookupMode: "exact", 
    symbol: { ...existing fields... },
    
    // NEW from Phase 1: Type Inference Results  
    inferred_types?: ["std::string"],   // 함수가 string을 반환하는지 확신 가능
    type_inference_confidence?: "high",
    
    // NEW from Phase 3: Pattern Analysis Tags  
    analysis_tags?: [                   // 구조적 패턴 인식 결과 (factory, singleton 등)
        { pattern_id: "cpp-no-virtual-destructor", severity: "warning" },
        { pattern_id: "factory_pattern_detected", category: "design_pattern" }
    ]
}

// Agent use case example:  
// User: "createUser 함수의 타입과 패턴 분석 결과 알려줘" → enhanced-symbol endpoint 호출
```

### POST /analysis/evaluate — Phase 3 Ruleset Evaluation  

```typescript
POST /analysis/evaluate/{ruleIds?}   // ruleIds optional (omit=all rulesets)  
Body: { filePath?: string, categories?: ['code_smell', 'security_risk'] }

Response: { 
    evaluated_rules_count: 5,         // 평가된 규칙 수  
    violations_found: [               // 감지된 위반 목록
        { rule_id: "cpp-no-virtual-destructor", file_path: "...", line_start: 15, ... },
        { rule_id: "missing-include-guard", file_path: "...", line_start: 1, ... }  
    ]
}

// Agent use case example:  
// User: "widget.cpp 파일에 모든 보안 관련 규칙 적용해줘" → POST /analysis/evaluate?ruleIds=security-rules&filePath=... 호출
```

### GET /analysis/results — Phase 3 Analysis Results Query  

```typescript
GET /analysis/results?symbolId=&severityMin=warning&category=code_smell

Response: { 
    total_results: 12,                // 필터링된 결과 수  
    results_by_category: [            // 카테고리별 분류 (agent가 쉽게 이해 가능)  
        { category: "code_smell", count: 8, samples: [...] },
        { category: "security_risk", count: 4, samples: [...] }
    ]
}

// Agent use case example: 
// User: "이 프로젝트에 code smell violations가 얼마나 있는지 요약해줘" → /analysis/results?category=code_smell 호출  
```

---

## Indexer Pipeline Integration Points (main.rs)

기존 indexing pipeline에 새로운 analysis phases 추가:

```rust
fn run_full(
    db: &storage::Database, 
    workspace_root: &Path,
    discovered_files: &[language::DiscoveredSourceFile],
    // ... existing params ...
) -> IndexStageTimings {
    
    let mut timings = IndexStageTimings::default();
    let registry = default_language_registry();

    println!("  Stage: parse files");  
    let (parsed_symbols, raw_calls, relation_events, ...) = 
        parse_discovered_files_with_progress(...);
        
    // ── NEW PHASES START HERE ────────────────────────
    
    println!("  Stage: type inference");      // Phase 1 — 심볼별 타입 추론  
    let type_inferences = infer_types_for_symbols(&parsed_symbols, &registry);
    db.write_type_inferences(&type_inferences)
        .expect("Failed to write batched type inferences");

    println!("  Stage: cross-boundary flow analysis"); // Phase 2 — argument→parameter flow tagging  
    let flow_tags = compute_cross_boundary_flow(
        &raw_calls, 
        &relation_events,      // 기존 relation events reuse (reuse infrastructure)
        &parsed_symbols        
    );
    db.write_flow_tags(&flow_tags)
        .expect("Failed to write batched cross-boundary flows");

    println!("  Stage: pattern-based structural analysis"); // Phase 3 — 패턴 감지  
    let pattern_results = detect_structural_patterns(
        &discovered_files, 
        workspace_root,
        registry                // language-specific patterns apply here
    );
    db.write_analysis_rules(&pattern_results)      // ruleset 정의 + 결과 저장  
        .expect("Failed to write analysis results");

    // ── NEW PHASES END HERE ────────────────────────  
    
    println!("  Stage: merge symbols");     // 기존 단계 — type inference / flow tags 병렬로 처리 가능
    let merged_symbols = resolver::merge_symbols(&parsed_symbols);  
    db.write_symbols(&merged_symbols)
        .expect("Failed to write merged representative symbols");

    // ... existing resolve calls, propagation, persist stages continue as before ...
}
```

**Pipeline Integration Strategy**:
- Type inference (Phase 1): `parse_files` 단계 직후 — parsed_symbols 기반으로 추론 가능
- Cross-boundary flow (Phase 2): `resolve_calls` 단계와 병렬로 실행 — call resolution 결과 활용  
- Pattern analysis (Phase 3): 마지막 stage — discovered_files 전체 스캔으로 패턴 감지

---

## Risk Signals & Confidence Semantics

Semgrep의 severity levels를 CodeAtlas에 adaptation:

| Severity Level | Meaning for Agent | Example |
|----------------|------------------|---------|
| **Error** | Definite bug or critical risk. Must fix before deployment. | `python-bare-except`, `cpp-use-after-free-risk` |  
| **Warning** | Likely issue requiring review. May be false positive but should verify. | `cpp-no-virtual-destructor`, `missing-include-guard`
| **Info** | Informational hint only — good practice suggestion, not urgent. | `raw-pointer-member`, `const-correctness-violation`

**Confidence Semantics**:  
- Pattern detection confidence = how strongly AST structure matches the pattern definition  
- High: clear structural evidence (e.g., class definitely has no virtual destructor)
- Partial: heuristic-based inference (e.g., likely factory method but not 100% certain from signature alone)

---

## Performance Considerations & Optimization Strategy  

### Indexing Time Impact Assessment

| Phase | Additional Processing Cost | Expected Overhead (%) | Notes |  
|-------|--------------------------|----------------------|-------|
| Type Inference (Phase 1) | Low-Medium — AST traversal already done for symbol extraction. Reuse existing parse tree, just extract type info from expressions. | +5-10% | Minimal overhead since parser walks entire AST anyway |  
| Cross-Boundary Flow (Phase 2) | Medium — requires call graph analysis across function boundaries. Leverage existing propagation infrastructure but extend scope. | +10-15% | Can be optimized with incremental indexing (only re-compute affected functions when files change) |
| Pattern Analysis (Phase 3) | Low-Medium — tsg-based pattern matching is efficient for small rule sets (~20 rules per language). Pre-compile patterns into AST matchers. | +8-12% | Rule set size scales linearly with overhead; keep initial ruleset lean (focus on high-value detections only) |

### Optimization Strategies

1. **Incremental Indexing Support**: Phase 2 & 3 must work correctly during incremental mode (`watch` or file-change-triggered reindex). Only affected functions/files need re-analysis, not entire workspace.
   
2. **Lazy Type Inference for Large Projects**: For repositories >50k symbols (OpenCV/LLVM scale), type inference can be deferred to query time via MCP tool calls instead of pre-computing everything during indexing.

3. **Parallel Pattern Detection**: Phase 3 pattern analysis runs per-file and is embarrassingly parallel — distribute across worker threads like existing parse batches do.

4. **Cache Analysis Results**: Store results in SQLite with proper indexes (see schema above) so MCP queries don't re-compute on each request. Existing query infrastructure handles caching automatically via DB lookups.

---

## Evaluation Criteria & Success Metrics  

### Quantitative Metrics  
1. **Inferred Type Coverage**: % of variables/functions that have at least one inferred type after Phase 1 completion (target: >60% for common patterns)
2. **Cross-Boundary Flow Accuracy**: precision/recall of detected flow paths vs manual verification on sample projects (OpenCV, nlohmann/json)  
3. **Pattern Detection Precision**: % of reported violations that are actual issues when manually verified by developer review

### Qualitative Agent Benefits  
1. **Reduced Fallback-to-Raw-Code Behavior**: agents rely less on reading raw source files because analysis results provide structured understanding
2. **Improved Code Comprehension Depth**: agent can explain "why" (pattern context) not just "what" (symbol existence) — e.g., "This is a factory method that returns Widget* and follows the Observer pattern for event handling."

---

## Dependencies & Prerequisites  

| Dependency | Status | Notes |
|------------|--------|-------|  
| Existing tree-sitter infrastructure | ✅ Ready | parser.rs, resolver.rs already use tsg successfully. Type inference builds on this foundation without new dependencies. |
| SQLite storage layer | ✅ Ready | New tables integrate with existing schema versioning system (storage.rs handles migrations automatically). No external DB needed. |
| MCP runtime infrastructure | ✅ Ready | server/src/mcp-runtime.ts provides tool registration pattern — new tools follow same structure as existing ones. |  
| Build metadata support (compile_commands.json) | ⚠️ Optional but helpful | compile_commands.json provide type hints for better inference confidence. Not required for basic functionality. |

---

## Testing Strategy  

### Unit Tests (indexer/src/)
1. **Type Inference**: test cases per language covering common patterns  
   - C++: `return new Widget()` → inferred_type="Widget*"  
   - Python: `$VAR = request.args.get('key')` → tag=user_input, type=str  
   - TypeScript: existing TSX/TS types can be extracted directly from parser

2. **Flow Tracking**: verify argument→parameter flow across call boundaries with semantic tags
3. **Pattern Detection**: validate rule set parsing and pattern matching on sample code snippets  

### Integration Tests (server/src/__tests__/)
1. MCP tool response format verification for new tools  
2. HTTP endpoint integration testing for enhanced symbol lookup + analysis results queries

---

## Migration & Rollout Plan  

| Step | Action | Expected Timeline | Risk Level |
|------|--------|------------------|------------|
| 0 | **Schema migration** — Add new tables to SQLite DB without breaking existing indexes. Run `storage.rs` version check → auto-migrate if needed. | Day 1-2 | Low (SQLite is flexible, backward-compatible) |
| 1 | **Type inference implementation** — Start with C++ parser only. Validate on small test project first before scaling to other languages. | Week 1-2 | Medium (requires AST expression analysis expertise for each language) |  
| 2 | **Cross-boundary flow tracking** — Build on existing propagation infrastructure in summary.rs/flow_analysis.rs. Reuse function boundary detection logic already implemented. | Week 3-4 | Low-Medium (leverages existing code, extends scope rather than building new system from scratch) |
| 3 | **Pattern analysis ruleset + MCP tools** — Start with high-value C++ patterns only (~10 rules). Expand to other languages once validation succeeds on first language. | Week 5-6 | Medium-High (requires YAML parsing infrastructure for external rule management, new MCP tool definitions) |
| 4 | **Dashboard UI integration** — Display analysis results alongside existing symbol lookup views in server/public/dashboard/ HTML pages. | Week 7 | Low-Medium (UI changes only, no backend logic risk) |

---

## Risk Mitigation Strategies  

### Technical Risks  
1. **Type inference false positives**: Confidence scoring prevents over-stating certainty. Agent responses will always include `type_inference_confidence` field so downstream systems can weigh results appropriately.
2. **Flow tracking performance impact on large repos (>50k symbols)**: Lazy evaluation for Phase 2 — cross-boundary flow paths computed on-demand via MCP tool calls instead of pre-computing during indexing (only compute when agent queries).
3. **Pattern detection rule complexity**: Start with ~10 high-value rules per language, validate effectiveness before expanding to larger sets. Keep initial ruleset focused on critical code smells/security risks only.

### Agent Experience Risks  
4. **Overwhelming analysis results for agents**: MCP tools include optional `severityMin` and `categories` filters so agent can request specific result types rather than dumping all violations at once. Response structures are compact-first (Milestone 17 direction) to avoid large payloads.
5. **Confusion between static analysis vs runtime behavior**: All analysis results clearly labeled as structural/static-only. Agent prompts will explicitly state "based on code structure" when referencing pattern detections or inferred types.

---

## Future Extension Opportunities (Post-M20)  

1. **Taint-aware security scanning** — Extend Phase 2 flow tracking to detect actual vulnerability patterns (e.g., user_input → sql_exec without sanitization).
2. **Auto-fix suggestions for common violations** — MCP tool `suggest_fix({ resultId })` generates code patch recommendations based on pattern context.
3. **CI/CD integration via SARIF output** — Export analysis results to Security Analysis Interchange Format (SARIF) standard format compatible with GitHub/GitLab Code Scanning tools.
