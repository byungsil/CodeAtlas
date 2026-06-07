# Milestone 20: Quick Start Guide for Implementation Team

## Context & Goal

Upgrade CodeAtlas indexing capabilities to provide deeper code understanding for AI agents via MCP tools. Inspired by Semgrep's pattern-based static analysis but adapted to leverage existing symbol/indexing infrastructure — not a full clone, just the impactful parts that enhance agent code comprehension.

**Core value proposition**: Agents can now answer questions like:
- "What type does this variable hold?" (Type inference from usage context)  
- "How does user input flow through multiple functions?" (Cross-boundary data tracking)
- "Is there any anti-pattern or security risk in this file?" (Pattern-based structural analysis)

## Key Files to Modify

### Indexer Layer (`indexer/src/`)
| File | Purpose | Phase(s) Affected |
|------|---------|-------------------|  
| `storage.rs` | Add new tables: `symbol_type_inferences`, `symbol_flow_tags`, `cross_boundary_flow_paths`, `analysis_rules`, `symbol_analysis_results` | All phases (1, 2, 3) |
| `parser.rs` | Enhance C++ parser with type inference from return expressions and assignment context | Phase 1 |  
| `resolver.rs` | Modify call resolution to track argument→parameter flow with semantic tags | Phase 2 |

### New Files to Create (`indexer/src/`)
| File | Purpose | Phase(s) Affected |
|------|---------|-------------------|
| `type_inference.rs` | Core type inference logic for C++ (and multi-language variants in respective parsers) | Phase 1 |  
| `flow_tags.rs` | Source/sink detection rules + semantic tagging infrastructure | Phase 2 |
| `cross_flow.rs` | Cross-boundary flow path computation & storage integration | Phase 2 |
| `analysis_patterns.rs` | Pattern-based structural analysis (factory, singleton, anti-patterns) | Phase 3 |

### Server Layer (`server/src/`)  
| File | Purpose | Phase(s) Affected |
|------|---------|-------------------|
| `mcp-runtime.ts` | Add new MCP tools: `get_enhanced_symbol()`, `trace_cross_function_flow()`, `analyze_file()` | All phases (1, 2, 3) |  
| `app.ts` | HTTP endpoints for analysis results query and ruleset management | Phase 3 |

## Execution Order Recommendation

Start with **Phase 1: Type Inference** — highest ROI, lowest risk. Leverages existing tree-sitter AST traversal already in parser.rs without needing new infrastructure. Then proceed to Phases 2 & 3 as dependencies allow (see Milestone20_TaskBreakdown.md for detailed dependency graph).

## Reference Documents
- `dev_docs/Milestone20_SemgrepStyleStaticAnalysis.md` — Full architecture, specs, and implementation details  
- `dev_docs/Milestone20_TaskBreakdown.md` — Concrete task list with dependencies and effort estimates  
