# Milestone 14. Versioned Database Publishing

Status:

- Planned

## 1. Objective

Replace direct `index.db` file replacement with a versioned database publishing model that remains safe under active readers.

This milestone focuses on:

- removing the need to overwrite or delete the live database file during publish
- allowing readers to continue using the previous database while a new one becomes active
- making watcher publish robust on Windows when dashboard/MCP readers are already open
- defining explicit lifecycle and cleanup rules for multiple published database generations

Success outcome:

- watcher no longer fails just because a reader is holding the previous database file
- MCP and dashboard can continue serving reads while a new database generation is published
- the active database is selected through a small pointer file rather than a fixed `index.db` overwrite

Positioning note:

- MS13 improved burst resilience and read/write coexistence
- one remaining operational sharp edge is final publish when readers still hold the live DB file
- MS14 should solve that by changing the publication model itself rather than retrying the same fragile replacement pattern

---

## 2. Applicability Review

Current publish behavior still assumes that the final DB path can be replaced in place.

That is fragile on Windows because:

- readers may keep the file open
- a successful rebuild can still fail at the final replace step
- this makes publish correctness depend on external process timing rather than explicit product behavior

A versioned publish model fixes the right layer of the problem:

- writers publish a new immutable DB generation
- readers keep using the old generation until they reopen
- a pointer file selects which generation is currently active

Included in MS14:

1. versioned DB filename model
2. pointer file for current active database
3. reader path resolution through the pointer file
4. watcher/full rebuild publish rewrite
5. generation retention and cleanup policy
6. validation under active readers

Explicitly not in scope for MS14:

- remote or distributed storage
- live multi-version diffing
- automatic historical querying across generations
- generalized cache eviction for unrelated runtime files

---

## 3. Recommended Order

1. M14-E1. Active Database Pointer Contract
2. M14-E2. Writer Publish Flow For Versioned Generations
3. M14-E3. Reader Resolution And Reload Semantics
4. M14-E4. Generation Retention And Cleanup
5. M14-E5. Real-Reader Validation And Acceptance

Why this order:

- the pointer contract must exist before either side can depend on it
- writers should publish new immutable generations before readers switch to them
- readers should then resolve through the pointer rather than fixed `index.db`
- cleanup should come after the multi-generation model exists
- real-reader validation should happen after the full lifecycle is in place

---

## 4. Epic Breakdown

### M14-E1. Active Database Pointer Contract

Goal:

- define how the active database is represented inside `.codeatlas`

Problems to solve:

- current product assumes one fixed live file: `index.db`
- readers and writers need a shared stable contract for selecting the active generation

Target behavior:

- `.codeatlas/current-db.json` or equivalent metadata points to the active DB generation
- the pointer file is small, explicit, and cheap to rewrite atomically

Recommended contract:

- active pointer file lives in the workspace data directory
- it stores:
  - active DB filename
  - published timestamp
  - format/version metadata when useful

Implementation areas:

- [indexer/src/storage.rs](/E:/Dev/CodeAtlas/indexer/src/storage.rs)
- [indexer/src/watcher.rs](/E:/Dev/CodeAtlas/indexer/src/watcher.rs)
- [server/src/storage/sqlite-store.ts](/E:/Dev/CodeAtlas/server/src/storage/sqlite-store.ts)
- [server/src/index.ts](/E:/Dev/CodeAtlas/server/src/index.ts)
- [server/src/mcp-runtime.ts](/E:/Dev/CodeAtlas/server/src/mcp-runtime.ts)

Acceptance:

- the active DB is no longer inferred only from `index.db`
- the pointer format is documented and testable

### M14-E2. Writer Publish Flow For Versioned Generations

Goal:

- publish new rebuild results as immutable versioned DB files instead of replacing the previous live file

Problems to solve:

- direct replace/delete of `index.db` is the current failure point

Target behavior:

- full rebuild and watcher publish produce a new versioned file such as:
  - `index-<timestamp>.db`
  - or another deterministic generation name
- once fully written and checkpointed, the writer updates the pointer file
- older DB generations remain untouched during publish

Implementation areas:

- [indexer/src/main.rs](/E:/Dev/CodeAtlas/indexer/src/main.rs)
- [indexer/src/watcher.rs](/E:/Dev/CodeAtlas/indexer/src/watcher.rs)
- [indexer/src/storage.rs](/E:/Dev/CodeAtlas/indexer/src/storage.rs)

Expected changes:

- replace fixed publish target assumptions
- unify full rebuild and watch publish around the same generation model
- preserve compatibility for existing workspaces that still only have `index.db`

Acceptance:

- a successful rebuild never requires deleting the DB currently used by an active reader
- a new generation becomes active by pointer update, not live DB replacement

### M14-E3. Reader Resolution And Reload Semantics

Goal:

- make HTTP/MCP/dashboard readers resolve the active DB through the pointer contract

Problems to solve:

- server code currently assumes a direct DB path
- once versioned DBs exist, readers need a stable resolution rule

Target behavior:

- on startup, readers resolve the active DB via the pointer file
- if the product later chooses to support periodic reload, that logic can build on the same contract
- current open handles continue using the old DB generation safely until reopened

Implementation areas:

- [server/src/storage/sqlite-store.ts](/E:/Dev/CodeAtlas/server/src/storage/sqlite-store.ts)
- [server/src/storage/store.ts](/E:/Dev/CodeAtlas/server/src/storage/store.ts)
- [server/src/index.ts](/E:/Dev/CodeAtlas/server/src/index.ts)
- [server/src/mcp-runtime.ts](/E:/Dev/CodeAtlas/server/src/mcp-runtime.ts)

Acceptance:

- readers can open the correct active DB without relying on a mutable fixed file
- old readers remain valid when a new generation is published

### M14-E4. Generation Retention And Cleanup

Goal:

- prevent unbounded accumulation of historical DB generations while keeping rollback-safe behavior

Problems to solve:

- versioned publishing naturally creates multiple DB files
- cleanup policy must avoid deleting a generation that could still be in use

Target behavior:

- retain a small number of recent generations
- cleanup only older inactive generations
- never delete the currently active generation
- ideally avoid deleting the immediately previous generation too aggressively

Recommended initial policy:

- keep active generation
- keep previous generation
- optionally keep one extra recent generation
- delete older inactive generations on successful publish or on startup maintenance

Implementation areas:

- [indexer/src/watcher.rs](/E:/Dev/CodeAtlas/indexer/src/watcher.rs)
- [indexer/src/storage.rs](/E:/Dev/CodeAtlas/indexer/src/storage.rs)

Acceptance:

- `.codeatlas` does not grow without bound
- cleanup never breaks the active pointer

### M14-E5. Real-Reader Validation And Acceptance

Goal:

- prove that versioned publishing solves the active-reader publish failure in real operation

Problems to solve:

- this is the exact behavior that motivated the milestone
- validation must include actual readers, not just unit tests

Target behavior:

- dashboard and/or MCP can hold the old DB open
- watcher/full rebuild can publish a new generation without failure
- readers can continue and new readers can open the new generation

Implementation areas:

- `samples/`
- real workspaces such as `E:\Dev\opencv` and `E:\Dev\llvm-project-llvmorg-18.1.8`
- [server/src/__tests__](/E:/Dev/CodeAtlas/server/src/__tests__)

Acceptance:

- active-reader publish no longer fails with the previous `index.db` replacement issue
- pointer file always resolves to a valid DB generation
- real-workspace validation reproduces the old scenario and confirms the new behavior

---

## 5. Recommended First Slice

Start with M14-E1 and M14-E2 together as one narrow writer-first slice.

Suggested first implementation slice:

1. introduce active DB pointer metadata in `.codeatlas`
2. teach the indexer to publish full rebuild output as a versioned DB file
3. update the pointer file after successful publish
4. keep existing `index.db` support as a backward-compatibility fallback for readers not yet migrated

Why this is the right first slice:

- it gives the new publish model a real output path quickly
- it minimizes initial reader-side changes
- it creates a concrete artifact contract before broader cleanup/reload work

---

## 6. Risks And Guardrails

Risks:

- pointer corruption could leave the workspace without a discoverable active DB
- cleanup could accidentally remove a generation still needed by a live reader
- partial migration could leave writer and reader assumptions out of sync

Guardrails:

- pointer updates must be atomic and validated
- the previous active DB generation should remain intact during publish
- cleanup must never delete the active generation
- reader fallback for legacy `index.db` should remain until migration is complete

---

## 7. Completion Criteria

MS14 can be considered complete when all of the following are true:

- writer publish no longer depends on replacing the currently open DB file
- the active DB is selected through an explicit pointer contract
- server and MCP can resolve the active DB from that contract
- old readers remain usable during publish
- generation cleanup is bounded and safe
- the original active-reader publish failure is resolved in real-workspace validation
