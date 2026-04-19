# Representative Regression List

Status:

- Active regression checklist for Milestone 9 representative-symbol work

## Purpose

This document tracks real-project symbols whose representative anchor quality is known to matter for agent usability.

The goal is not only to verify that the symbol exists, but to verify that the default anchor shown to the agent is a location a human engineer would likely consider canonical.

## Evaluation Method

For each tracked symbol, record:

- workspace
- exact lookup target
- current representative anchor
- why that anchor is weak or acceptable
- expected better anchor family
- current status

Statuses:

- `weak`
  - current representative is technically valid but agent-unfriendly
- `acceptable`
  - current representative is usable but still not ideal
- `canonical`
  - current representative is the place a human engineer would most likely start

## Tracked Symbols

### LLVM

1. `llvm::StringRef`

- workspace: `E:\Dev\llvm-project-llvmorg-18.1.8`
- current representative anchor:
  - `clang/test/Analysis/llvm-conventions.cpp:34`
- expected better anchor family:
  - runtime or public library anchor under `llvm/include/llvm/...`
- why this matters:
  - this is a high-frequency, high-visibility type
  - a test-heavy representative wastes tokens and attention immediately
- current status:
  - `weak`

### OpenCV

2. `cv::Mat::Mat`

- workspace: `E:\Dev\opencv`
- current representative anchor:
  - `modules/core/include/opencv2/core/cuda.inl.hpp:752`
- expected better anchor family:
  - primary `cv::Mat` declaration or canonical core header location
- why this matters:
  - `cv::Mat` is one of the most important OpenCV types
  - CUDA-specialized inline locations are valid but not the most useful default starting point
- current status:
  - `weak`

### nlohmann/json

3. `parse_error`

- workspace: `E:\Dev\nlohmann`
- current representative anchor family:
  - `tests/abi/include/...`
- expected better anchor family:
  - public library header or core runtime implementation tree
- why this matters:
  - test-shadowed representatives make even small library repositories feel noisy
- current status:
  - `weak`

## Future Targets

When available, add at least one Unreal-engine-class or similarly structured large game repository with:

- engine/runtime symbols
- editor/tooling duplicates
- test or generated shadows
- public-vs-private header ambiguity

Suggested future target classes:

- engine core string/type utilities
- actor/component hierarchy anchors
- gameplay-facing API that also appears in editor/test helpers

## Exit Signal For Milestone 9

Milestone 9 regression work should be considered materially successful when:

- the tracked symbols above no longer default to obviously test-shadowed or duplicate-heavy anchors
- representative confidence aligns with the observed quality
- before/after notes can be recorded for OpenCV, LLVM, and at least one game-project-class repository
