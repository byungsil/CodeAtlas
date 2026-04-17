# CodeAtlas - AI Code Intelligence System

---

# 1. Overview

## 1.1 Purpose

CodeAtlas is a local AI-powered code intelligence system designed for very large-scale C++ codebases (e.g. FIFA-scale projects).

The system enables AI agents (Claude, Codex) to query structured code information instead of scanning raw source files.

---

## 1.2 Goals

- Parse entire C++ codebase using Tree-sitter
- Build a scalable code structure index
- Provide a local MCP (Model Context Protocol) server
- Enable fast AI queries
- Support incremental indexing for large repositories

---

## 1.3 Core Principles

- Performance-first
- Incremental-first
- Query-first (AI must not scan raw code)
- Separation of concerns

---

# 2. Architecture

[ C++ Source Code ]
        ↓
[ Rust Indexer (Tree-sitter) ]
        ↓
[ SQLite Index DB ]
        ↓
[ MCP Server (Node.js) ]
        ↓
[ AI Agents (Claude / Codex) ]

---

# 3. Components

## 3.1 Indexer (Rust)

Responsibilities:
- Parse C++ files using Tree-sitter
- Extract functions, classes, call relationships
- Store into database
- Support incremental indexing

Requirements:
- Multi-threaded (parallel processing)
- Scalable to 100K+ files
- Avoid full rebuild

Input:
- Workspace root path

Output:
- SQLite database

---

## 3.2 Database (SQLite)

Schema:

symbols:
- id, name, type, file, line

Identity rules:
- `id` is the canonical exact symbol identity
- `qualifiedName` is the canonical exact human-readable alias
- in the current model, `id` and `qualifiedName` are expected to be identical for exact-match behavior
- declaration and definition pairs must share one logical symbol identity
- declaration-only symbols remain valid logical symbols; they are not dropped just because no definition is present yet
- definition-only symbols remain valid logical symbols; they are not forced to invent a declaration record
- when declaration and definition coexist under the same canonical `id`, CodeAtlas exposes one logical symbol and prefers the definition as the representative view
- inline/header-only implementations are treated as `inline_definition` lifecycle variants, not as incomplete declaration/definition pairs

calls:
- caller, callee

files:
- path, hash, last_indexed

---

## 3.3 MCP Server (Node.js)

Responsibilities:
- Provide API for AI agents
- Query database
- Return structured JSON

Endpoints:
- GET /symbol?id=&qualifiedName=
- GET /function/:name
- GET /class/:name
- GET /search?q=
- GET /callgraph/:name

Constraints:
- <50ms response
- partial results
- scalable
- exact lookup must not fall back to short-name heuristics

Exact and heuristic lookup behavior:
- `GET /symbol` is the canonical exact lookup route
- exact success responses include:
  - `lookupMode = exact`
  - `confidence = exact`
  - `matchReasons`
- `GET /function/:name` and `GET /class/:name` remain heuristic
- heuristic lookup responses include:
  - `lookupMode = heuristic`
  - `confidence`
  - `matchReasons`
  - optional `ambiguity`

MCP tools:
- `lookup_symbol`
- `lookup_function`
- `lookup_class`
- `search_symbols`
- `get_callgraph`

MCP exact lookup behavior:
- `lookup_symbol` is the canonical exact MCP lookup tool
- accepts `id` and/or `qualifiedName`
- returns `BAD_REQUEST` when neither is supplied
- returns `BAD_REQUEST` when both are supplied but identify different logical symbols
- returns `NOT_FOUND` when the exact symbol is missing

Confidence semantics:
- CodeAtlas confidence is structural query confidence, not compiler-complete semantic certainty
- `exact` means canonical identity targeting succeeded
- `high_confidence_heuristic` means one structural best candidate was selected
- `ambiguous` means multiple candidates remain plausible
- `unresolved` means no viable structural answer is currently available
- agents should prefer exact lookup when exact identity is known and treat heuristic results as progressively weaker evidence

---

## 3.4 Watcher

Responsibilities:
- Monitor file changes
- Trigger incremental indexing

Behavior:
file change → hash check → re-index file

---

# 4. Indexing Strategy

Initial:
- full scan of all .cpp / .h files

Incremental:
- hash-based updates
- only changed files processed

Call graph:
- lazy evaluation

---

# 5. AI Integration

AI must NOT read raw source files directly.

Flow:
AI → MCP → structured data → reasoning

Example:
User: Analyze impact of UpdateAI

AI:
1. Query MCP
2. Retrieve structure
3. Analyze

---

# 6. Performance Requirements

- 100K+ files
- millions LOC
- incremental updates only
- minimal memory usage

---

# 7. Non-Goals

- full semantic C++ analysis
- compiler replacement
- macro/template full resolution

---

# 8. Implementation Phases

Phase 1:
- Node MVP
- JSON storage

Phase 2:
- Rust indexer
- SQLite DB

Phase 3:
- performance optimization
- advanced queries

Phase 4:
- AI automation
- multi-agent workflows

---

# 9. Success Criteria

- AI answers without scanning code
- fast query (<50ms)
- scalable to large projects
- reliable incremental updates

---

# 10. Key Concept

Tree-sitter = structure generator  
Database = storage  
MCP = query layer  
AI = reasoning engine  

---

# Final Summary

CodeAtlas is not a parser.

It is:
- A local AI-powered code search engine
- Optimized for large-scale game development
- Designed for incremental and real-time usage
