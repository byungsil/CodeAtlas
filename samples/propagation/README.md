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
  - helper-produced event object returned into local staging
  - staged object passed across method boundary into member sink
  - member state later forwarded across a helper boundary
  - helper-produced temporary carrier object returned into local staging
  - nested carrier field passed across method boundary into member sink
  - nested temporary carrier returned into local staging and unpacked across method boundary
  - nested temporary carrier relayed across helper argument and return boundaries
  - extraction helper return forwarded through one more helper before sink consumption
- `member_state.cpp`
  - parameter/local into member write
  - member read into local and return
  - carrier-object field into member write
  - member into carrier-object field write
  - staged local carrier handoff into member write
  - constructor initializer parameter into member write
  - constructor initializer carrier-field into member write
  - helper-returned carrier-field into constructor member write
  - helper-returned carrier-field into constructor seed followed by member readout
  - nested helper-returned carrier-field into constructor seed followed by member readout
  - pointer-carrier constructor initializer into partial member write
  - `this->member`, `obj.member`, and `ptr->member` confidence differences
