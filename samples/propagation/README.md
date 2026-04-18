# Propagation Fixtures

This fixture set backs Milestone 6 local propagation tests.

Current scenarios:

- `local_flows.cpp`
  - local initializer binding
  - local assignment
  - chained assignment
  - pointer-heavy initializer that should stay partial
- `shadowing.cpp`
  - nested-block shadowing
  - separate local anchor identities for same short name
- `function_boundary.cpp`
  - argument-to-parameter propagation
  - return-value propagation into caller-side local initialization
- `member_state.cpp`
  - parameter/local into member write
  - member read into local and return
  - `this->member`, `obj.member`, and `ptr->member` confidence differences
