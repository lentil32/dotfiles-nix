# Smear Cursor: SSOT Audit and Normative State Spec

This audit is based on the current code, not the existing ownership doc.

Relevant code paths inspected:

- `plugins/smear-cursor/src/events.rs`
- `plugins/smear-cursor/src/core/state/protocol.rs`
- `plugins/smear-cursor/src/core/state/observation/snapshot.rs`
- `plugins/smear-cursor/src/core/state/policy.rs`
- `plugins/smear-cursor/src/config.rs`
- `plugins/smear-cursor/src/config/derived.rs`
- `plugins/smear-cursor/src/state/machine/{mod.rs,accessors.rs,preview.rs}`
- `docs/state_ownership.md`

---

## 1. Current architectural split

The code already has the right top-level split:

- `EngineState.shell`: host-facing caches, bridge status, probe caches, scratch buffers, telemetry-like shell state.
- `EngineState.core_state`: reducer-owned semantic state.

That split is good.

The problem is not the top-level architecture. The problem is that several facts are still stored twice inside the semantic half, and one ownership document has drifted badly enough that it is now a second, conflicting “truth”.

---

## 2. Actual SSOT violations and redundant state

### 2.1 `RuntimeConfig` stores both `fps` and `time_interval`

Current code:

- `RuntimeConfig { fps, time_interval, ... }` in `src/config.rs`
- option application keeps them manually synchronized in `src/state/options_patch.rs`

This is a direct SSOT violation.

Those two fields are the same fact in two units.

```text
fps <-> 1000 / time_interval_ms
```

Today the code relies on discipline to keep them in sync:

- setting `time_interval` rewrites `fps`
- setting `fps` rewrites `time_interval`

That is not an ownership model. That is two owners plus a repair function.

### 2.2 `RuntimeState` stores both `config_revision` and `DerivedConfigCache.source_revision`

Current code:

- `RuntimeState { config_revision, derived_config, ... }` in `src/state/machine/mod.rs`
- `DerivedConfigCache { source_revision, ... }` in `src/config/derived.rs`
- `debug_assert_eq!(self.derived_config.source_revision(), self.config_revision, ...)` in `src/state/machine/accessors.rs`

This is another direct SSOT violation.

The code itself proves the duplication by asserting equality between the two fields.

A field whose only role is to equal another field is not independent state.

### 2.3 `TimerState` stores timer generation twice for active timers

Current code:

- per-slot generation fields:
  - `animation_generation`
  - `ingress_generation`
  - `recovery_generation`
  - `cleanup_generation`
- plus active tokens:
  - `active_animation: Option<TimerToken>` etc.
- `TimerToken` itself already contains `(id, generation)`

So when a timer is armed, generation is present in two places:

- slot generation field
- active token generation

That is redundant state.

Also, timer identity is encoded twice:

- in the field name (`active_animation`)
- inside `TimerToken.id()`

### 2.4 `RenderCleanupState` encodes phase twice and stores derived budgets as state

Current code:

- `thermal: RenderThermalState`
- `next_compaction_due_at: Option<Millis>`
- `entered_cooling_at: Option<Millis>`
- `hard_purge_due_at: Option<Millis>`
- `idle_target_budget`
- `max_prune_per_tick`
- `max_kept_windows`

Problems:

1. **Phase is encoded twice.**
   `thermal` says whether cleanup is `Hot`, `Cooling`, or `Cold`, but the optional timestamps also imply phase.

2. **Invalid combinations are representable.**
   Example: `thermal == Cold` with non-`None` deadlines, or `thermal == Hot` with `entered_cooling_at = Some(_)`.

3. **Budget fields are stored even though they are functions of `max_kept_windows`.**
   Today:

   - `idle_target_budget = min(max_kept_windows, 2)`
   - `max_prune_per_tick = max(max_kept_windows, 1)`

   These are derivations, not primary state.

4. **`max_kept_windows` inside cleanup state shadows runtime config.**
   Cleanup policy is config policy. Duplicating it into scheduled cleanup state means config changes can race against stale cleanup policy.

This should be a sum type, not a product struct with partially meaningful optional fields.

### 2.5 `docs/state_ownership.md` is currently a conflicting source of truth

This is a process-level SSOT failure.

The document names facts that the current code does not actually store.

Examples:

- It says `ObservationSnapshot.basis` owns `cursor_color_witness`.
  Actual code: `ObservationBasis` stores `buffer_revision`; `cursor_color_probe_witness` is derived from `ObservationBasis + CursorColorProbeGenerations`.
- It says `RuntimeState.render_static_config` exists.
  Actual code: static render config is produced from `derived_config.static_render_config()`.
- It says `RuntimeState.preview_particles_source`, `preview_baseline`, and `preview_particles_materialized` exist.
  Actual code: preview baseline is a local struct inside `preview.rs`, not runtime state.

A stale ownership doc is not harmless. It creates a second mental model and invites future duplicate state because people code to the doc instead of the reducer.

---

## 3. Things that are already single-sourced and should stay that way

These are good and should remain derived, not stored twice:

- `ProtocolPhaseKind` is derived from `ProtocolPhase`.
- `Lifecycle` is derived from `ProtocolPhase`.
- `ObservationId` is derived from `demand.seq()`.
- target corners are derived from `CursorTarget`.
- `ScenePatchKind` is derived from `PatchBasis`.
- active requested probe set is reconstructed from active probe slots.
- cursor-color probe witness is derived from observation basis plus probe generations.

Do not “optimize” any of those by adding stored copies.

---

## 4. Normative replacement spec

This section is the target spec. It is intentionally stricter than the current implementation.

### 4.1 Ownership classes

Every field must be exactly one of:

- **authoritative**: semantic truth
- **cache**: derived, purgeable, rebuildable from authoritative state
- **snapshot**: retained copy of an external read or previous authoritative state used for comparison/reuse
- **telemetry**: operational measurement only

Rules:

1. A semantic fact has exactly one authoritative owner.
2. A cache may not own freshness separate from the thing it caches.
3. A snapshot may not be mutated as if it were truth.
4. Telemetry may influence heuristics, but may not shadow semantic state.
5. If a fact can be computed from another field with no information loss, it must not be stored as authoritative state.

### 4.1.1 Type-level representability rules

The target is not merely "document the valid combinations". The target is:

- illegal states are hard or impossible to represent in the types
- if a state cannot be made unrepresentable because of serialization or boundary compatibility, it must be rejected at the boundary and normalized before entering reducer-owned state

Rules:

1. Phase-legal payloads live inside the phase enum variant that makes them legal.
2. A field that is meaningless in some modes must not be present in those modes behind `Option`.
3. If two fields must appear together, represent them as one struct/enum or one `Option<PairLike>`, not sibling `Option`s.
4. Prefer enums, dedicated structs, and newtypes over booleans or primitive tuples when they make invalid combinations unconstructable.
5. Constructors that create reducer-owned state should enforce invariants once, rather than relying on later repair code.
6. If a state must temporarily exist in a weaker boundary form, convert it immediately into the stronger reducer-owned representation.

### 4.1.2 Cache proof obligations

The target is not merely "caches seem harmless". The target is:

- caches are provably non-semantic

For every cache, the code should be able to answer all of these questions:

1. What authoritative inputs is this cache derived from?
2. What operation invalidates it when those inputs change?
3. What semantic comparison ignores it?
4. What purge path can drop it without mutating semantic state?
5. What test demonstrates that cache hit vs miss does not change reducer semantics or user-visible output for the same external inputs?

If a cache cannot answer those questions, it is not yet specified rigorously enough.

### 4.2 Canonical top-level state

Conceptual shape:

```rust
struct EngineState {
    core: CoreState,          // authoritative reducer state
    shell_cache: ShellCache,  // purgeable host/cache state only
}
```

Conceptual core shape:

```rust
struct CoreState {
    generation: Generation,
    protocol: ProtocolState,
    ids: EntropyState,
    latest_exact_cursor_position: Option<CursorPosition>,
    runtime: RuntimeState,
    scene: SceneState,
    realization: RealizationLedger,
}
```

Notes:

- `generation` is reducer lifecycle freshness, not a bag-of-everything revision.
- `ids` owns proposal id allocation and ingress sequence allocation.
- `latest_exact_cursor_position` is authoritative fallback state.
- `runtime`, `scene`, and `realization` are reducer-owned semantic subtrees.

A wrapper like `CoreStatePayload` is acceptable only as grouping. It must not introduce alternate ownership boundaries.

### 4.3 Protocol state

Normative shape:

```rust
struct ProtocolState {
    shared: ProtocolSharedState,
    phase: ProtocolPhase,
}

struct ProtocolSharedState {
    demand_queue: DemandQueue,
    timers: TimerSlots,
    recovery: RecoveryPolicyState,
    ingress_delay: IngressPolicyState,
    cleanup: CleanupPhase,
}
```

Normative phase enum:

```rust
enum ProtocolPhase {
    Idle,
    Primed,
    Collecting {
        retained: Option<ObservationSnapshot>,
        pending: PendingObservation,
        probe_refresh: ProbeRefreshState,
    },
    Observing {
        active: ObservationSnapshot,
        probe_refresh: ProbeRefreshState,
        prepared_plan: Option<PreparedObservationPlan>, // cache
    },
    Ready {
        active: ObservationSnapshot,
    },
    Planning {
        active: ObservationSnapshot,
        proposal_id: ProposalId,
    },
    Applying {
        active: ObservationSnapshot,
        proposal: InFlightProposal,
    },
    Recovering {
        retained: Option<ObservationSnapshot>,
    },
}
```

Rules:

- `ProtocolPhase` is the only workflow owner.
- `ProtocolPhaseKind` and `Lifecycle` are derived accessors only.
- Exactly one observation payload is phase-legal at a time.
- `prepared_plan` is cache only and must be dropped whenever its inputs change.

### 4.4 Timer state

Normative shape:

```rust
struct TimerSlots {
    animation: TimerSlot,
    ingress: TimerSlot,
    recovery: TimerSlot,
    cleanup: TimerSlot,
}

struct TimerSlot {
    generation: TimerGeneration,
    armed: bool,
}
```

Rules:

- The slot owns the latest issued generation.
- `TimerToken` is produced on demand from `(timer_id, generation)` when arming or comparing.
- No separate `active_*: Option<TimerToken>` fields.
- No duplicated generation inside both slot state and token storage.

Semantics:

- `arm(slot)` increments generation and marks `armed = true`.
- `is_active(token)` is derived from the slot for `token.id()`.
- `clear(slot)` sets `armed = false` and preserves latest generation.

### 4.5 Cleanup state

Normative shape:

```rust
enum CleanupPhase {
    Cold,
    Hot {
        soft_clear_due_at: Millis,
        hard_purge_due_at: Millis,
    },
    Cooling {
        started_at: Millis,
        next_compaction_due_at: Millis,
        hard_purge_due_at: Millis,
    },
}
```

Rules:

- Cleanup phase is encoded only by the enum variant.
- No `thermal` field alongside phase-bearing optional timestamps.
- No `idle_target_budget` stored field.
- No `max_prune_per_tick` stored field.
- No `max_kept_windows` stored field inside cleanup phase.

Derived policy:

```text
soft_clear_keep_windows = runtime.config.max_kept_windows
cooling_target_budget   = min(runtime.config.max_kept_windows, 2)
max_prune_per_tick      = max(runtime.config.max_kept_windows, 1)
```

Meaning:

- cleanup state owns only **when** cleanup should happen and what phase the scheduler is in
- runtime config owns **how aggressively** cleanup should trim

That separation is critical.

### 4.6 Observation state

Normative shape:

```rust
struct PendingObservation {
    demand: ExternalDemand,
    requested_probes: ProbeRequestSet,
}

struct ObservationSnapshot {
    demand: ExternalDemand,
    basis: ObservationBasis,
    probes: ProbeSet,
    cursor_color_probe_generations: Option<CursorColorProbeGenerations>,
    motion: ObservationMotion,
}

struct ObservationBasis {
    observed_at: Millis,
    mode: String,
    cursor_position: Option<CursorPosition>,
    cursor_location: CursorLocation,
    viewport: ViewportSnapshot,
    buffer_revision: Option<u64>,
    cursor_text_context_state: CursorTextContextState,
}
```

Rules:

- `ObservationId` is derived from `demand.seq()` only.
- `cursor_color_probe_witness` is derived; it is never stored.
- Requested probe policy is authoritative only in `PendingObservation` before activation.
- Once active, requestedness is owned by active probe slots.

### 4.7 Runtime state

Normative authoritative runtime shape:

```rust
struct RuntimeState {
    config: RuntimeConfig,
    config_revision: ConfigRevision,
    projection_policy_revision: ProjectionPolicyRevision,
    plugin_state: PluginState,
    animation_phase: AnimationPhase,
    current_corners: [Point; 4],
    target: CursorTarget,
    trail: TrailState,
    velocity_corners: [Point; 4],
    spring_velocity_corners: [Point; 4],
    particles: Vec<Particle>,
    previous_center: Point,
    rng_state: u32,
    transient: TransientRuntimeState,
}
```

Normative cache shape:

```rust
struct RuntimeCache {
    derived_config: DerivedConfigCache,
    scratch: RuntimeScratch,
    aggregated_particle_cells: SharedAggregatedParticleCells,
    aggregated_particle_cells_dirty: bool,
    particle_screen_cells: SharedParticleScreenCells,
    particle_screen_cells_dirty: bool,
}
```

Rules:

- `config` is authoritative.
- `config_revision` is authoritative freshness for config-derived views.
- `DerivedConfigCache` is cache only and carries **no independent source revision field**.
- Scratch buffers and particle artifacts are caches only.
- Cache state must not be part of semantic identity.

### 4.8 Runtime config

Normative rule: choose one canonical frame-timing field.

Preferred internal shape:

```rust
struct RuntimeConfig {
    frame_interval_ms: f64,
    simulation_hz: f64,
    ...
}
```

External compatibility rule:

- API may accept `fps` as input alias.
- API may accept legacy `time_interval` alias.
- Both normalize immediately into `frame_interval_ms`.
- Supplying both in the same patch is invalid and must be rejected.
- `fps` is derived for display only.

If you prefer `fps` as canonical instead, the same rule applies in reverse. The point is: one stored owner, one or more boundary aliases.

### 4.9 Scene and projection state

Normative shape:

```rust
struct SceneState {
    semantics: SemanticState,
    projection: ProjectionState,
}

struct SemanticState {
    revision: SemanticRevision,
    cursor_trail: Option<CursorTrailSemantic>,
}

struct ProjectionState {
    motion_revision: MotionRevision,
    last_motion_fingerprint: Option<u64>,
    retained_projection: Option<ProjectionHandle>,
}
```

Rules:

- `SceneState` is a composite owner of `SemanticState + ProjectionState`.
- `ScenePatchKind` is always derived from `PatchBasis`.
- `RetainedProjection.logical_raster` is the semantic projection payload.
- `RetainedProjection.realization` is a cache/materialization of the logical raster; it must not become independent truth.

### 4.10 Realization state

Normative shape stays close to current code:

```rust
enum RealizationLedger {
    Cleared,
    Consistent { acknowledged: ProjectionHandle },
    Diverged {
        last_consistent: Option<ProjectionHandle>,
        divergence: RealizationDivergence,
    },
}
```

Rules:

- reducer-side belief about shell state lives here and nowhere else in core
- shell caches do not own realization truth
- cleanup or shell failures transition the ledger to `Diverged` or `Cleared`

### 4.11 Shell cache

Normative shell-cache shape stays conceptually close to current code:

```rust
struct ShellCache {
    namespace_id: Option<u32>,
    host_bridge_state: HostBridgeState,
    probe_cache: ProbeCacheState,
    editor_viewport_cache: EditorViewportCache,
    buffer_text_revision_cache: BufferTextRevisionCache,
    buffer_metadata_cache: BufferMetadataCache,
    buffer_perf_policy_cache: BufferEventPolicyCache,
    buffer_perf_telemetry_cache: BufferPerfTelemetryCache,
    real_cursor_visibility_cache: Option<RealCursorVisibility>,
    background_probe_request_scratch: Vec<Object>,
    conceal_regions_scratch: Vec<ConcealRegion>,
}
```

Rules:

- This subtree is non-authoritative.
- It may be cleared without changing reducer semantics.
- If clearing it changes semantic behavior, ownership is wrong.
- `real_cursor_visibility_cache` is only an optimization cache for already-applied shell writes.

### 4.12 Ephemeral copies that are acceptable

These are acceptable only if they remain local and non-authoritative:

- `IngressReadSnapshot`
- `RuntimePreview`
- `RuntimePreviewBaseline`
- effect payloads such as `RequestObservationBaseEffect`

Rule:

- they may copy state for a single operation
- they may not be committed back as alternate owners
- they may not outlive the operation unless explicitly named retained snapshots

---

## 5. Required invariants

These should be enforced with debug assertions and tests.

### 5.1 No dual ownership

For every semantic fact, name exactly one field as owner.

### 5.2 No phase encoded twice

Any state machine phase must be a sum type. Never represent phase as `enum + optional fields that restate the enum`.

### 5.3 No freshness stored in both owner and cache

If `config_revision` is authoritative, caches may key off it but may not store their own competing revision.

### 5.4 No config snapshots inside scheduler state

Scheduled cleanup state may store deadlines, but not copied config knobs like `max_kept_windows`.

### 5.5 Caches are purgeable

Dropping caches may increase CPU or IO, but may not change semantic output for the same sequence of external inputs.

### 5.6 Illegal states are unrepresentable where practical

Reducer-owned types should encode phase legality and coupled lifetimes directly in their shape. Invalid combinations should be impossible, or else rejected before they enter reducer state.

### 5.7 Caches are provably non-semantic

There must be an executable way to strip or purge caches and show that authoritative state transitions and user-visible results are unchanged for the same external input sequence.

### 5.8 Ownership doc must match code

`docs/state_ownership.md` must be rewritten to match the implementation exactly, or generated/validated so drift fails CI.

---

## 6. Migration order

1. Rewrite the ownership doc from current code, not memory.
2. Collapse `fps` and `time_interval` to one canonical internal field.
3. Remove `source_revision` from `DerivedConfigCache`.
4. Re-encode `TimerState` as slot state instead of generation-plus-token duplication.
5. Re-encode `RenderCleanupState` as a true enum and derive budgets from current config.
6. Move caches behind explicit `cache` naming or sub-structs so authoritative vs cache state is visible in the type shape.
7. Tighten reducer-owned type shapes so phase legality and coupled lifetimes are represented directly instead of by sibling flags and `Option` fields.
8. Add semantic projection or cache-strip helpers plus purge tests that prove caches are non-semantic.
9. Make semantic equality and reducer tests compare authoritative state, not cache materialization.

---

## 7. Bottom line

The plugin already has the right big idea: reducer-owned core state and shell-local cache state.

What blocks it from being “Jane Street level” is not architecture churn. It is a handful of precise ownership mistakes:

- one fact stored in two units (`fps` and `time_interval`)
- one freshness value stored in two places (`config_revision` and `source_revision`)
- timer activity storing generation twice
- cleanup phase/policy encoded as overlapping product state instead of a single sum type
- a stale ownership document acting as a second, incorrect source of truth

Fix those, and the rest of the codebase will read much more like a real single-owner state machine instead of a system that still depends on synchronized mirrors.
