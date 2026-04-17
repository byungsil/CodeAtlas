# Milestone 3. Production-Grade Incremental Operation

## 1. Objective

Make CodeAtlas safe to trust during active repository development.

This milestone focuses on:

- incremental correctness
- robust file and dependency change planning
- watcher stability
- failure recovery
- branch switch and mass-change resilience

Success outcome:

- CodeAtlas stays correct and current during real development workflows

---

## 2. Recommended Order

1. M3-E1. Incremental correctness matrix
2. M3-E2. Regression fixture suite
3. M3-E3. File identity and planning upgrades
4. M3-E4. Header-change fanout policy
5. M3-E5. Watcher event hardening
6. M3-E6. Failure recovery and DB safety
7. M3-E7. Branch switch and mass-change handling

---

## 3. Epics

### M3-E1. Incremental Correctness Matrix

Goal:

- define the expected behavior for every important change scenario

Implementation tasks:

- enumerate scenarios:
  - edit
  - add
  - delete
  - rename/move
  - header-only change
  - branch switch
  - mass generated-file churn
- record expected outcomes for:
  - updated symbols
  - removed symbols
  - refreshed relations
  - untouched records

Expected touch points:

- `docs/`
- `NestTask.md` or a dedicated incremental design doc

Validation checklist:

- each change scenario has an explicit expected result contract

Exit criteria:

- incremental behavior is documented before deeper implementation changes

---

### M3-E2. Regression Fixture Suite

Goal:

- make incremental correctness testable and repeatable

Implementation tasks:

- build scenario fixtures for each change case
- add helpers to compare DB state before and after incremental updates
- ensure fixtures are deterministic and small enough for frequent testing

Expected touch points:

- `samples/`
- `indexer/src/incremental.rs`
- `indexer/src/storage.rs`

Validation checklist:

- each change scenario has a regression test

Exit criteria:

- the incremental test suite can catch stale or incorrect update behavior

---

### M3-E3. File Identity and Planning Upgrades

Goal:

- improve the planner that decides what needs to be refreshed

Implementation tasks:

- audit current content-hash and file-record behavior
- decide whether rename and move detection should use path only or content-assisted heuristics
- make planner decisions inspectable in logs and tests
- avoid hidden planning behavior

Expected touch points:

- `indexer/src/incremental.rs`
- `indexer/src/models.rs`
- `indexer/src/storage.rs`

Validation checklist:

- planner output can be asserted directly in tests

Exit criteria:

- incremental planning is understandable, testable, and more robust

---

### M3-E4. Header-Change Fanout Policy

Goal:

- prevent stale state after declaration-heavy file changes

Implementation tasks:

- define what happens when a header changes
- choose a conservative correctness-first policy
- record why a broader refresh or sweep was triggered
- ensure merged symbols and relation edges stay correct

Expected touch points:

- `indexer/src/incremental.rs`
- `indexer/src/indexing.rs`
- `indexer/src/storage.rs`

Validation checklist:

- header changes do not leave stale symbol or relation state behind

Exit criteria:

- declaration-heavy edits are handled safely

---

### M3-E5. Watcher Event Hardening

Goal:

- make watch mode stable under real editor and filesystem behavior

Implementation tasks:

- normalize noisy event patterns on Windows-heavy workflows
- handle temp-file replacement and repeated write bursts
- improve debounce and queue deduplication
- add diagnostic tracing for event handling

Expected touch points:

- `indexer/src/watcher.rs`
- `indexer/src/incremental.rs`

Validation checklist:

- burst saves do not trigger redundant large work

Exit criteria:

- watcher behavior is stable under common save patterns

---

### M3-E6. Failure Recovery and DB Safety

Goal:

- ensure partial failures do not leave the index unusable

Implementation tasks:

- define transactional boundaries for incremental writes
- prevent parser failures from corrupting DB state
- add startup integrity checks for existing DBs
- improve recovery logging and actionable failure reporting

Expected touch points:

- `indexer/src/storage.rs`
- `indexer/src/indexing.rs`
- `indexer/src/watcher.rs`

Validation checklist:

- interrupted or failed runs leave the DB readable and recoverable

Exit criteria:

- failures degrade safely instead of corrupting the index

---

### M3-E7. Branch Switch and Mass-Change Handling

Goal:

- fail safe when the repository changes too much for ordinary incremental assumptions

Implementation tasks:

- detect branch-like event bursts
- define thresholds for:
  - continue incrementally
  - run a consistency sweep
  - recommend or force rebuild
- log diagnostics for why a heavier recovery path was chosen

Expected touch points:

- `indexer/src/watcher.rs`
- `indexer/src/incremental.rs`
- docs and README sections if user guidance is needed

Validation checklist:

- branch-like churn no longer risks silent stale state

Exit criteria:

- CodeAtlas remains trustworthy after large repository state transitions

---

## 4. Final Exit Criteria

- every important incremental change scenario has a documented expected outcome
- those scenarios are covered by regression tests
- watcher mode is stable under noisy local workflows
- DB state remains safe and recoverable after partial failures
- large repository transitions fail safe instead of silently drifting stale
