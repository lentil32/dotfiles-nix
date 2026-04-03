# `smear_cursor` testing taxonomy

This crate uses a layered test strategy so each invariant has one clear owner.

## Ownership rules

- Leaf modules own exact parsing, deterministic math, cache keys, one-step transforms, and other pure invariants.
- Runtime and integration modules own wiring boundaries, invalidation edges, and a small number of smoke tests that prove the leaf logic is called correctly.
- Reducer and state-machine suites own scenario semantics, lifecycle transitions, failure and retry behavior, and user-visible state evolution.
- Snapshot tests own formatting and bridge-contract output only.
- Goldens own curated motion and trajectory behavior only.

## What belongs where

### Leaf property tests

Use property tests for modules that are pure, finite, and algebraic.

Examples:

- `src/config.rs` owns mode-family classification and cursor-color sampling flags.
- `src/types.rs` owns one-based cell validation and visual-anchor math.
- `src/events/probe_cache.rs` owns cache-key sensitivity and invalidation partitions.
- `src/events/lru_cache.rs` owns eviction order and promotion semantics.

Leaf tests should cover the full combinatorial matrix for the invariant they own. Higher layers should not restate that same matrix.

### Runtime and wrapper tests

Use small example or smoke tests for adapters that read state, route events, or forward to leaf logic.

Examples:

- `src/events/runtime/ingress_snapshot.rs` may keep one thin smoke test for snapshot wiring.
- `src/events/handlers/observation/cursor_color.rs` may keep one invalidation or reuse smoke once leaf cache validation owns the matrix.
- `src/events/runtime/tests.rs` should keep runtime-specific telemetry, reentry, and wrapper behavior, not repeat timer-handle or policy combinatorics already owned elsewhere.

These tests should answer "does the wrapper call the right leaf and preserve the right boundary conditions?" rather than "have we re-proved every leaf invariant?"

### Reducer and state-machine tests

Keep scenario-driven tests for lifecycle sequencing and user-visible contracts.

Examples:

- `src/core/reducer/tests/` owns observation, planning, failure, and retry sequences.
- `src/core/runtime_reducer/tests/` owns motion semantics, transition behavior, and curated runtime regressions.
- `src/state/machine/tests.rs` owns local state evolution when the behavior is best expressed as operation sequences.

These tests should focus on state transitions that define truth, reducer purity, deferred effect planning, and retry-as-lifecycle behavior.

### Snapshots and goldens

Snapshots are for stable rendered or serialized output. Goldens are for intentionally curated motion scenarios.

Examples:

- Runtime and render-apply telemetry snapshots own output formatting.
- `trajectory_goldens` owns curated motion trajectories that are easier to review as named scenarios than as generated cases.

Do not use snapshots or goldens to carry large invariant matrices that belong in leaf or model properties.

## Refactor guardrails

- When adding a new property test, first identify the lowest layer that can own the invariant without mocks.
- When a lower layer gains full combinatorial coverage, trim higher-layer tests down to one or two boundary smokes.
- When a failure comes from a generated case that exposes a durable bug, promote it into a named regression test or checked-in seed.
- Review `plugins/smear-cursor/proptest-regressions/` as part of the fix, keep seed files in git when they still add value, and prefer adding a named regression beside the owning module once the failing shape is understood.
