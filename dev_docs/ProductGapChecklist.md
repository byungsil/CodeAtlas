# Product Gap Checklist

## Purpose

This document tracks the remaining product-level gaps between the current CodeAtlas implementation and a fully polished production-ready release.

It is intentionally broader than a milestone document.

Use this when deciding:

- whether CodeAtlas is ready for general user release
- what still needs hardening after the current milestone sequence
- which remaining issues are product blockers versus post-release improvements

---

## Current Product State

CodeAtlas already provides the core product shape:

- local workspace indexing
- SQLite-backed query model
- MCP server for agent integration
- HTTP API and dashboard
- incremental updates and watcher operation
- Windows release packaging
- real-project validation on `opencv` and `llvm`

At this stage, CodeAtlas is a usable product.

However, not every product-quality concern is fully closed.

---

## Gap Categories

### 1. Installation And First-Run UX

Current status:

- source setup exists
- Windows release packaging exists
- release bundle smoke tests have passed

Remaining gaps:

- release setup still assumes external Node installation rather than a self-contained runtime
- first-run experience is Windows-centric and not yet equally polished for other environments
- MCP client configuration still requires some manual editing

Priority:

- low

Why it matters:

- product capability is present and usable today, but onboarding friction is still higher than ideal for a more polished broad release

### 2. Cross-Platform Release Story

Current status:

- Windows packaging flow exists and was validated

Remaining gaps:

- no equivalent polished packaging flow for macOS or Linux
- no unified cross-platform release process document

Priority:

- medium

Why it matters:

- the current release story is strong for Windows but not yet broad enough for a general multi-platform product claim

### 3. Long-Running Operational Confidence

Current status:

- large workspace rebuilds, watcher smoke, and active-reader publish validation have all passed
- versioned DB publishing and active pointer resolution were completed in MS14

Remaining gaps:

- limited soak-style validation for multi-hour or day-scale MCP + watcher sessions
- no explicit long-run operational budget for memory drift, stale snapshot accumulation, or repeated fallback behavior

Priority:

- medium

Why it matters:

- current validation is strong, but long-running operational confidence is still lighter than a mature product would ideally have

### 4. Multi-Language Depth Consistency

Current status:

- C/C++, Lua, Python, TypeScript, and Rust are all supported

Remaining gaps:

- language support depth is not yet equally mature across all languages
- C/C++ has the deepest quality and scale validation
- non-C++ languages have less extensive real-workspace coverage and fewer advanced semantics

Priority:

- medium

Why it matters:

- current messaging should stay accurate about "supported" versus "equally feature-complete"

### 5. Product Surface Consolidation

Current status:

- README is now more release-oriented
- dev docs are cleaner than before

Remaining gaps:

- product-facing docs and developer-facing docs could still be separated more cleanly
- HTTP, MCP, dashboard, and packaging guidance is spread across several documents

Priority:

- low to medium

Why it matters:

- this is less about correctness and more about reducing user confusion

### 6. Final Release Governance

Current status:

- milestones and validation evidence exist
- release acceptance criteria now exist as a dedicated document

Remaining gaps:

- release decisions are still driven more by milestone completion than by a routine release checklist
- the current acceptance bar is documented, but not yet institutionalized into a repeated release process

Priority:

- high

Why it matters:

- release decisions should be driven by one explicit acceptance contract and a repeatable habit around it

---

## Suggested Priority Order

1. final release governance
2. long-running operational confidence
3. cross-platform release story
4. multi-language depth consistency
5. installation and first-run UX
6. product surface consolidation

---

## What Is Already Strong Enough

These areas are no longer major product gaps:

- exact and heuristic lookup behavior
- reference integrity
- core MCP query usability
- propagation and callgraph substrate
- dashboard observability
- large-project indexing capability
- burst-aware watcher behavior
- active-reader-safe versioned DB publishing
- Windows packaging basics

---

## Release Readiness Summary

If the question is:

- "Is CodeAtlas a real usable product already?"
  - yes

- "Is every product-quality gap fully closed?"
  - no

- "What still looks most release-critical?"
  - a single explicit release acceptance contract in day-to-day use
  - stronger long-run operational proof
