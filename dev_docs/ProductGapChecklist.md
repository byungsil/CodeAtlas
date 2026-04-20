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

### 3. Watcher Publish Robustness Under External Readers

Current status:

- watcher burst handling and incremental behavior improved in MS13
- DB integrity remains good after real-project rebuilds

Remaining gaps:

- publish can still fail if another local process holds the target DB file during final replacement
- the product currently relies on stopping local readers or retrying later rather than using a more graceful publish protocol

Priority:

- high

Why it matters:

- this is one of the remaining operational sharp edges in real local usage

### 4. Long-Running Operational Confidence

Current status:

- large workspace rebuilds and watcher smoke tests have been validated
- dashboard runtime stats exist

Remaining gaps:

- limited soak-style validation for multi-hour or day-scale MCP + watcher sessions
- no explicit long-run operational budget for memory drift, stale snapshot accumulation, or repeated fallback behavior

Priority:

- medium

Why it matters:

- current validation is strong, but long-running operational confidence is still lighter than a mature product would ideally have

### 5. Multi-Language Depth Consistency

Current status:

- C/C++, Lua, Python, TypeScript, and Rust are all supported

Remaining gaps:

- language support depth is not yet equally mature across all languages
- C/C++ has the deepest quality and scale validation
- non-C++ languages have less extensive real-workspace coverage and fewer advanced semantics

Priority:

- medium

Why it matters:

- current messaging should stay accurate about “supported” versus “equally feature-complete”

### 6. Product Surface Consolidation

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

### 7. Final Release Governance

Current status:

- milestones and validation evidence exist

Remaining gaps:

- no single release gate document previously stated what must be true before calling the product release-ready
- acceptance has been proven through milestone progress rather than one consolidated release standard

Priority:

- high

Why it matters:

- release decisions should be driven by one explicit acceptance contract

---

## Suggested Priority Order

1. watcher publish robustness under external readers
2. final release governance
3. long-running operational confidence
4. cross-platform release story
5. multi-language depth consistency
6. installation and first-run UX
7. product surface consolidation

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
- Windows packaging basics

---

## Release Readiness Summary

If the question is:

- “Is CodeAtlas a real usable product already?”
  - yes

- “Is every product-quality gap fully closed?”
  - no

- “What still looks most release-critical?”
  - graceful watcher publish under active readers
  - a single explicit release acceptance contract
