# Milestone 20: Estimated Impact on Agent Code Understanding Capabilities

## Before M20 (Current State)

| Question Type | Can Agent Answer? | How It Answers Today | Limitation |  
|---------------|-------------------|---------------------|------------|  
| "What does this function do?" | Partially | Lookup symbol → get signature + callers/callees | No type inference on parameters/return values. Must read raw source to understand actual behavior. |
| "How is data flowing through these functions?" | No | Propagation only within single function scope. Cannot trace across call boundaries. | Agent must manually follow each hop in call graph — tedious and error-prone for long chains. |  
| "Is there any code smell or anti-pattern here?" | No | Not available at all. Agent reads raw source to spot issues manually. | Requires human-level pattern recognition that agent lacks structured data for. |
| "What type does this variable hold?" | Sometimes (if declared explicitly) | Symbol lookup shows declaration but no inferred actual usage type. | Variable's real-world type remains unknown unless agent parses code context itself. |

## After M20 (Enhanced State)

| Question Type | Can Agent Answer? | How It Answers Today | Improvement Source |
|---------------|-------------------|---------------------|------------------|  
| "What does this function do?" | **Yes — deeply** | Enhanced symbol lookup returns inferred return type + analysis tags (e.g., factory_pattern detected) | Phase 1 Type Inference + Phase 3 Pattern Analysis |
| "How is data flowing through these functions?" | **Yes — end-to-end** | `trace_cross_function_flow()` MCP tool shows hop-by-hop path with semantic tags and risk markers. Agent explains entire flow in one response instead of stitching multiple queries together. | Phase 2 Cross-Boundary Flow Tracking |  
| "Is there any code smell or anti-pattern here?" | **Yes — structured analysis** | `analyze_file()` returns violations grouped by category/severity with descriptions agent can explain to user (e.g., "Derived class lacks virtual destructor which causes undefined behavior when deleting via base pointer"). | Phase 3 Pattern-Based Structural Analysis |
| "What type does this variable hold?" | **Yes — inferred from usage** | Enhanced symbol lookup includes `inferred_types` field populated from assignment context and call site analysis. Agent confidently explains actual runtime types without reading raw code. | Phase 1 Type Inference |

## Concrete Example: Before vs After M20

### Scenario: User asks "How does user input reach the database in this project?"

#### Before (Current):
```
Agent must manually stitch together multiple queries:
1. lookup_symbol("getUserInput()") → returns signature only  
2. find_callers("processUserInput()") → shows call chain but no data flow semantics
3. trace_variable_flow("user_input_var") → propagation stops at function boundary
4. Agent reads raw source code to understand what happens next in each function
5. Manually verify if SQL injection risk exists by inspecting string interpolation patterns

→ Takes 10+ separate tool calls + manual reasoning = slow, error-prone response  
```

#### After (M20):
```
Agent uses enhanced MCP tools:
1. trace_cross_function_flow({ sourceSymbolId: "getUserInput" }) → 
   Returns full path with semantic tags: user_input → ... → sql_exec sink detected!
   
2. analyze_file({ filePath: "src/db_handler.cpp", categories: ['security_risk'] }) →  
   Returns violation: "User input directly interpolated into SQL query without sanitization (CWE-89 risk)"

3. get_enhanced_symbol({ qualifiedName: "buildQuery()" }) → 
   Shows inferred_type=["std::string"] + analysis_tags=[sql_injection_vulnerability_detected]
   
→ Takes 2 tool calls = fast, structured response with clear security context
```

## Expected Agent Behavior Changes Post-M20

1. **Reduced fallback-to-raw-code behavior**: Agents rely less on reading raw source files because semantic annotations provide immediate understanding of types, patterns, and risks without manual parsing by LLM.

2. **Proactive risk detection in code reviews**: When agent helps user understand a new file/module, it can automatically flag potential issues (missing virtual destructor, mutable default arguments, etc.) instead of waiting for user to ask about problems.

3. **Deeper architectural understanding**: Cross-boundary flow tracking enables agents to explain data lifecycle end-to-end across multiple functions/modules — critical for debugging complex systems or explaining how features interact.

4. **Structured pattern recognition**: Agent can identify design patterns (factory, observer) and anti-patterns during code exploration without requiring explicit user questions about structure quality. This shifts agent from passive information retriever to active code analyst.

## Performance Impact on Indexing Time

| Phase | Additional Overhead | Notes |
|-------|-------------------|-------|  
| Type Inference (Phase 1) | +5-10% indexing time | Minimal — reuses existing AST traversal, just extracts type info from expressions already walked during symbol extraction. |
| Cross-Boundary Flow (Phase 2) | +10-15% for small projects (<20k symbols), deferred to on-demand query for large repos (>50k) | Can be optimized by lazy evaluation: pre-compute common paths only, defer complex flow tracing until agent explicitly queries via MCP tool. |
| Pattern Analysis (Phase 3) | +8-12% indexing time | Scales linearly with ruleset size; keep initial set lean (~10 high-value patterns per language). Pre-compile tsg rules into efficient AST matchers for fast execution. |

**Total estimated overhead**: ~25-37% additional indexing time initially, but can be reduced through lazy evaluation strategies (Phase 2 deferred computation) and incremental indexing support (only affected files re-analyzed on changes).
