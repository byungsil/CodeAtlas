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
- GET /function/:name
- GET /class/:name
- GET /search?q=
- GET /callgraph/:name

Constraints:
- <50ms response
- partial results
- scalable

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
