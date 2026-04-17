# Ambiguity Fixture Workspace

This workspace is dedicated to Milestone 1 "Trustworthy Lookup" regression work.

It is intentionally separate from `samples/src` so ambiguity-specific fixtures do not disturb the baseline sample workspace used by the current API and compatibility tests.

## Coverage Goals

This fixture set includes:

- duplicate short names across namespaces
- overloaded free functions
- same method names in sibling classes
- declaration/definition split across header and source
- `this->` and pointer-member (`ptr->`) calls

## Files

- `src/namespace_dupes.h`
- `src/namespace_dupes.cpp`
- `src/overloads.h`
- `src/overloads.cpp`
- `src/sibling_methods.h`
- `src/sibling_methods.cpp`
- `src/split_update.h`
- `src/split_update.cpp`

## Intended Use

- parser metadata tests
- resolver ranking tests
- exact-lookup ambiguity tests
- header/source unification tests
