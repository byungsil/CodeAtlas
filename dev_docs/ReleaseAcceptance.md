# Release Acceptance

## Purpose

This document defines the release gate for CodeAtlas after MS13.

It answers one question:

- what must be true before we can honestly say the current product is release-ready?

This is not a milestone plan.

It is a release decision checklist.

---

## Release Decision Levels

### Level A. Core Product Ready

Meaning:

- the product is useful, operationally credible, and can be released to early real users

### Level B. Broad General Release Ready

Meaning:

- the product is polished enough that remaining issues are mostly convenience or ecosystem gaps rather than operational sharp edges

Current assessment:

- CodeAtlas after MS13 meets **Level A**
- CodeAtlas does **not yet fully meet Level B**

---

## Acceptance Checklist

### 1. Core Indexing Capability

Requirement:

- full rebuild indexing must succeed on representative real projects

Current state:

- pass

Evidence:

- `E:\Dev\opencv`
- `E:\Dev\llvm-project-llvmorg-18.1.8`

### 2. Incremental And Watcher Correctness

Requirement:

- incremental updates must preserve DB correctness
- watcher operation must remain correct under bursty edit patterns

Current state:

- pass

Evidence:

- MS4 incremental correctness work
- MS13 burst resilience work
- final watcher smoke on real workspaces

### 3. Query Surface Usability

Requirement:

- MCP and HTTP query surfaces must be practically usable for investigations

Current state:

- pass

Evidence:

- exact/heuristic lookup quality
- upstream traversal
- enum value usage support
- compact response mode
- reliability and coverage signaling

### 4. Reference And DB Integrity

Requirement:

- persisted DB must not retain known broken core relationships after indexing

Current state:

- pass

Evidence:

- dangling reference cleanup from MS11
- SQLite integrity checks on real workspaces

### 5. Large-Project Operational Credibility

Requirement:

- the product must show stable behavior on large real repositories

Current state:

- pass

Evidence:

- OpenCV and LLVM validation
- memory-pressure improvements from earlier milestones
- watcher burst resilience improvements in MS13

### 6. Dashboard And Operational Visibility

Requirement:

- users must be able to observe index state and runtime behavior without opaque black-box operation

Current state:

- pass

Evidence:

- dashboard overview
- runtime stats
- workspace switching
- coverage and reliability surfacing

### 7. Windows Release Packaging

Requirement:

- a Windows release bundle must be buildable and smoke-testable

Current state:

- pass

Evidence:

- `scripts/package-release.ps1`
- validated extracted release flow
- setup, indexing, dashboard, and MCP smoke checks

Note:

- first-run polish can still improve, but it is not currently treated as a release blocker

### 8. Graceful Operation Under Active Readers

Requirement:

- normal product operation should tolerate active dashboard/MCP readers while index updates publish

Current state:

- partial

Why partial:

- DB contents remain valid
- reader fallback behavior is stronger than before
- but publish can still fail when another process holds the final DB path during replacement

Release impact:

- acceptable for Level A
- not acceptable for Level B without further hardening

### 9. Cross-Platform Release Completeness

Requirement:

- release story should be comparably mature across supported developer environments

Current state:

- partial

Why partial:

- Windows release path is defined
- non-Windows packaging is not equally complete

Release impact:

- acceptable for Level A
- not enough for Level B

### 10. Long-Run Operational Proof

Requirement:

- product should have confidence against long-running operational drift

Current state:

- partial

Why partial:

- current validation is strong for rebuild, watcher smoke, and burst handling
- soak-style long-session validation is still limited

Release impact:

- acceptable for Level A
- not enough for Level B

---

## Acceptance Result

### Level A Result

Accepted.

Why:

- the product has crossed the threshold from milestone-driven prototype to genuinely usable release candidate
- core indexing, query, watcher, dashboard, and packaging behaviors are all real and validated

### Level B Result

Not yet accepted.

Main blockers:

- publish robustness when active readers hold the DB file
- incomplete cross-platform packaging story
- limited soak-style operational validation

---

## Immediate Recommendation

Current recommendation:

- CodeAtlas can be treated as ready for controlled release / early-user release
- do not yet describe it as fully polished general release software without caveats

Best next work after MS13:

1. harden DB publish behavior under active readers
2. run longer operational soak validation
3. decide whether non-Windows packaging is required for the next release bar
