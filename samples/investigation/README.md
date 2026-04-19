# Investigation Fixtures

This fixture workspace backs Milestone 10 "Agent Investigation Workflow" development.

It is intentionally separate from:

- `samples/src`
- `samples/propagation`
- `samples/ambiguity`

so the MS10 workflow scenarios can grow without disturbing earlier milestone fixtures.

## Coverage Goals

This fixture set is designed to exercise:

- end-to-end workflow reconstruction through calls plus field handoff
- flag set and consume flows
- short carrier-object propagation
- intentionally pointer-heavy or weak-coverage-style investigation segments
- ambiguous short-name startup candidates across different path neighborhoods

## Current Scenarios

- `src/workflow.h`
- `src/workflow.cpp`
  - source input to launch action through:
    - direct calls
    - carrier-object construction
    - member write
    - member read
  - helper-produced event hint workflow through:
    - helper call adjacency
    - staged hint application
    - member-backed emit step
  - nested helper-carrier hint workflow through:
    - helper-returned nested envelope staging
    - nested field handoff into hint application
    - member-backed emit step
  - nested relay workflow through:
    - helper-returned nested envelope staging
    - relay helper extraction through argument and return boundaries
    - forwarded launch handoff
  - nested relay-to-forwarder workflow through:
    - helper-returned nested envelope staging
    - extraction helper boundary
    - one extra forwarding helper before launch
  - constructor-seeded hint workflow through:
    - helper-returned temporary state used in constructor initialization
    - seeded member readout into emit step
  - nested constructor-seeded hint workflow through:
    - nested helper-carrier return used in constructor initialization
    - seeded member readout into owner-aware emit step
  - field-backed relay workflow through:
    - field write from hint application
    - field-backed owner method
    - relay helper handoff before launch
- `src/partial_flow.h`
- `src/partial_flow.cpp`
  - pointer-heavy handoff that should remain a weaker investigation region
- `runtime/update_shot.cpp`
- `editor/update_shot.cpp`
  - duplicate short-name `UpdateShot` symbols in different path neighborhoods

## Intended Use

- API-contract examples for MS10 investigation responses
- fixture-backed MCP and server tests for stitched workflow summaries
- follow-on propagation and disambiguation regression coverage
