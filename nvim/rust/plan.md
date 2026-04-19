# Position Model Refactor Plan Index

This index turns [plans/position-spec.md](/Users/lentil32/.nixpkgs/nvim/rust/plans/position-spec.md) into an implementation plan for the `smear-cursor` crate.

The spec remains the rationale and ownership document for the target model. The files below are intentionally narrower: they describe how to land the refactor, which modules move in each phase, and what must be true before the phase is considered complete.

## Inputs

- Normative target model: [plans/position-spec.md](/Users/lentil32/.nixpkgs/nvim/rust/plans/position-spec.md)
- Current ownership reference: [docs/state_ownership.md](/Users/lentil32/.nixpkgs/nvim/rust/docs/state_ownership.md)
- Testing guidance: [docs/smear-cursor-testing-taxonomy.md](/Users/lentil32/.nixpkgs/nvim/rust/docs/smear-cursor-testing-taxonomy.md)

## Progress

- [x] [plans/position-foundations.md](/Users/lentil32/.nixpkgs/nvim/rust/plans/position-foundations.md)
      Introduce the shared position primitives and remove type-level duplication.
- [x] [plans/position-observation.md](/Users/lentil32/.nixpkgs/nvim/rust/plans/position-observation.md)
      Unify host reads and reshape observation storage around semantic facts.
- [x] [plans/position-runtime.md](/Users/lentil32/.nixpkgs/nvim/rust/plans/position-runtime.md)
      Align planning, runtime target ownership, and viewport math with the new model.
- [x] [plans/position-validation.md](/Users/lentil32/.nixpkgs/nvim/rust/plans/position-validation.md)
      Define the verification matrix, doc updates, and rollout checks for the whole refactor.

## Rewrite Rules

- [x] Treat `plans/position-spec.md` as the normative contract for semantic ownership and invariants; do not restate that content in implementation PRs unless the contract changes.
- [x] Do not introduce backward-compatibility layers, migration shims, transparent wrappers, or dual-write paths. Each phase rewrites its scope directly and deletes the old owner in the same phase.
- [x] Do not introduce new zero-valued sentinel states. Use `Option` or an explicit enum for absence.
- [x] Keep reducers pure and effect boundaries explicit. Host reads and fallback policy stay in event-layer observation code; reducer state should only receive normalized semantic facts.
- [x] Remove duplicate readers and duplicate math only when the replacement owner is landing in the same phase.
- [x] Update ownership docs and targeted tests in the same change that reassigns a semantic fact.

## Quality Gates

- [x] Encode domain invariants in types and constructors, not comments or call-site conventions.
- [x] Make illegal states unrepresentable where the type system can express them cleanly; prefer newtypes, enums, and compile-time distinctions over runtime flag bundles.
- [x] Keep raw numeric fields private when they carry invariants; expose validated constructors and accessors instead.
- [x] Prefer small `Copy` newtypes by value and borrow larger snapshots instead of cloning them.
- [x] Do not add ambiguous boolean or `Option` parameters to new APIs when an enum, newtype, or named method would make the call site self-documenting.
- [x] Do not add `unwrap()`, `expect()`, or panic-driven control flow to touched production paths; use typed `Result` and explicit fallback states.
- [x] Avoid redundant clones in observation and runtime hot paths. Any retained clone should have a clear ownership reason and should happen at the last responsible boundary.
- [x] Match exhaustively on domain enums that model position state. Do not use wildcard arms for `ObservedCell`, target-state, or viewport-state transitions.
- [x] Update debug invariant hooks when ownership moves. New owners should expose or extend `debug_assert_invariants()` checks in the same phase.
- [x] Add `//!` module docs and `///` item docs for new shared modules and externally-relevant shared types when they become part of the maintained architecture.
- [x] Keep modules small enough to preserve local reasoning. If a rewritten module grows beyond the repo guidance, split by owned fact rather than adding more helper clutter.

## Suggested Order

- [x] Land the shared position module and replace leaf types first.
- [x] Move the observation boundary to the new surface and cursor facts.
- [x] Rewire planning and runtime target ownership after the new facts are available.
- [x] Tighten invariants and update docs/tests once the rewritten owners are in place.

## Exit Condition

- [x] `smear-cursor` has one shared position vocabulary.
- [x] `smear-cursor` has one surface reader.
- [x] `smear-cursor` has one cursor observation result.
- [x] `smear-cursor` has one runtime target owner.
- [x] `smear-cursor` has one command-row formula.
- [x] No backward-compatibility layer, migration shim, or dual-owner bridge remains.
- [x] Touched production paths satisfy the quality gates above.
- [x] Tests and docs are updated to match.
