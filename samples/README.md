# Samples

This directory contains the repository-owned deterministic fixture workspace used by tests, contracts, and local endpoint validation.

## Real-Project Reference Source

Primary reference root:

- `F:\dev\dev_future\client\gameplay`

This path is not the default MVP fixture workspace. It is a read-only source for selecting realistic C++ patterns and for validating that the local sample fixtures are not toy-only examples.

## Selection Rules

- Keep the checked-in `samples/src` dataset small and deterministic.
- Prefer files that expose real symbol shapes, includes, classes, free functions, and direct call relationships without dragging in the full engine graph.
- Ensure the fixture set includes at least one template-oriented case and at least one macro-bearing case.
- Start from test-oriented or self-contained files before touching deeper gameplay implementation units.
- Promote new real-project patterns into `samples/src` only when they reveal a parser or contract gap that the existing fixtures do not cover.
- Do not treat the full `gameplay` tree as a Phase 0 or Phase 1 indexing target.

## Recommended Reference Candidates

Good early candidates from `F:\dev\dev_future\client\gameplay`:

- `interface\teammanagement\dev\tests\main.cpp`
- `interface\teammanagement\dev\tests\teamcontrollertest.h`
- `interface\teammanagement\dev\tests\teamcontrollertest.cpp`
- `interface\teammanagement\dev\tests\testteamutils.h`
- `interface\teammanagement\dev\tests\testteamutils.cpp`
- `internal\unittest\dev\source\sample_test.cpp`
- `internal\unittest\dev\source\memoryfill_test.cpp`
- `internal\unittest\dev\source\allocator.cpp`

These are better Phase 0/1 references than larger files such as `math_test.cpp` or broader `testbed` sources because they are smaller, more isolated, and still representative enough to validate symbol extraction and call relationships.

## How To Use Them

1. Keep `samples/src` as the executable fixture set for automated tests.
2. Compare the fixture coverage against a few curated files under the real-project reference root.
3. If a real-project construct breaks the parser or contract assumptions, add the smallest possible equivalent fixture into `samples/src` and document the expected output under `samples/expected`.
4. For macros, the early expectation is parser tolerance and non-catastrophic behavior, not exact preprocessor expansion semantics.
