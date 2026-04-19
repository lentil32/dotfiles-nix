I inspected `plugins/smear-cursor`. It is already more disciplined than most plugins because it has an explicit ownership doc in `docs/state_ownership.md`. But it is not Jane Street-level yet, because several facts are still stored in multiple mutable places and then “kept honest” with invariants. That is the exact smell to remove.

The rule I used was simple:

> if the same semantic fact exists in two mutable records and the code needs equality checks or matrix invariants to keep them aligned, that is not SSoT.

## What is redundant or violates SSoT now

### 1. Protocol phase is represented twice

Today `ProtocolState` is a product of:

- `observation: ObservationSlot`
- `workflow: ProtocolWorkflow`

See `core/state/protocol.rs:260-294`.

Then there is a compatibility matrix:

- `workflow_allows_observation_slot(...)` in `protocol.rs:260-279`
- debug invariant in `protocol.rs:458-503`

That means the lifecycle is encoded twice: once by `workflow`, once by `observation slot kind`.

This is a structural SSoT violation. The code is saying “these two fields together mean the real phase”.

**Jane Street fix:** make the phase a single enum whose variants carry exactly the payload allowed in that phase. No slot/workflow matrix.

---

### 2. The active observation request is owned twice

In `ProtocolWorkflow`:

- `ObservingRequest { request, ... }`
- `ObservingActive { request, ... }`

See `core/state/protocol.rs:212-219`.

But once the observation becomes active, `ObservationSnapshot` also owns:

- `request: ObservationRequest`

See `core/state/observation/snapshot.rs:363-370`.

And the code literally checks that the two copies match:

- `protocol.rs:487-490`

So in `ObservingActive`, the same request exists both in workflow state and in the active observation.

**This is a real SSoT bug.**

**Jane Street fix:** in “collecting” phase, own a pending observation request once. In “active” phases, own the active observation once. Workflow should hold a handle/payload, not a second copy of the same request.

---

### 3. Observation identity is duplicated in too many places

`ObservationRequest` stores `observation_id`:

- `snapshot.rs:188-205`

and that id is derived from `demand.seq()`:

- `snapshot.rs:196-199`

`ObservationBasis` also stores `observation_id`:

- `snapshot.rs:217-252`

And the reducer checks that basis id equals request id:

- `core/reducer/machine/observation.rs:278`

So the same identity exists in:

- `ExternalDemand.seq`
- `ObservationRequest.observation_id`
- `ObservationBasis.observation_id`

That is too many owners for one fact.

**Jane Street fix:** keep observation identity once at the observation root. My preference here is: once a demand enters observation workflow, the observation root owns the id, and subrecords do not. If you want maximal normalization, derive it from `demand.seq()` and store it nowhere else.

---

### 4. Probe “requestedness” is duplicated

`ObservationRequest.probes` is a boolean policy:

- `core/state/observation/probe.rs:5-25`

But once an observation is active, probe lifecycle already encodes requestedness:

- `ProbeSlot::Unrequested | Requested(...)` in `probe.rs:179-218`
- `BackgroundProbeState::Unrequested | Collecting | Ready | Failed` in `snapshot.rs:26-43`

And `ObservationSnapshot::debug_assert_invariants()` checks that the request booleans and probe slot states agree:

- `snapshot.rs:423-445`

So after activation, you have two encodings of the same fact:

- “was cursor-color requested?”
- “was background probe requested?”

That is not SSoT.

**Jane Street fix:** keep requested-probe policy only while the observation is pending. Once the observation becomes active, the probe states themselves are the sole owner of requestedness.

---

### 5. Probe request ids are deterministic, but stored everywhere

`ProbeKind::request_id(observation_id)` is a pure function:

- `probe.rs:34-49`

But the result is stored redundantly in:

- `ProbeState::{Pending, Ready, Failed}` in `probe.rs:113-162`
- `BackgroundProbeState::{Collecting, Ready, Failed}` in `snapshot.rs:26-77`

Then invariants recompute and compare the expected id:

- `snapshot.rs:449-455`

So request id is not really state. It is a derived name for `(observation_id, probe kind)`.

**Jane Street fix:** do not store `ProbeRequestId` in steady state. Derive it on demand for logging/effects if you still want it. The state should only store probe lifecycle payload.

This same redundancy leaks into event payloads too:

- `core/event.rs:47-75` carries both `observation_id` and `probe_request_id`

Given current semantics, the event variant already tells you the probe kind, so `probe_request_id` is also derivable there.

---

### 6. Cursor target state is split across unrelated fields

Current runtime target truth is fragmented across:

- `TransientRuntimeState.target_position`
- `TransientRuntimeState.retarget_epoch`
- `TransientRuntimeState.trail_stroke_id`
- `TransientRuntimeState.tracked_location`
- `RuntimeState.target_corners`

See:

- `state/machine/types.rs:98-106`
- `state/machine/transitions.rs:82-92`
- `state/machine/lifecycle.rs:357-366`

This means “where the cursor is going” is not one record. It is scattered between transient state and geometry fields.

That is bad state design even if it mostly works.

**Jane Street fix:** one `CursorTarget` aggregate owns all target identity:

- position
- shape
- tracked location
- retarget epoch

and one `TrailState` owns trail identity:

- trail stroke id
- trail origin
- trail elapsed

`target_corners` becomes derived from `position + shape`.

---

### 7. Scene state mirrors runtime render data instead of owning semantics

`CursorTrailProjectionPolicy` is copied from `RenderFrame`:

- `core/state/scene.rs:33-72`

`CursorTrailGeometry` is also copied from `RenderFrame`:

- `scene.rs:75-151`

But `RenderFrame` already contains those facts:

- `types.rs:609-625`

Then `SceneState` also contains `motion: RuntimeState`:

- `scene.rs:571-580`

So the design is:

- runtime owns motion/render truth
- render frame copies it
- scene semantics copies the render frame
- planner rebuilds `PlannerFrame` from the semantic copy

That is too much mirroring.

**Jane Street fix:** scene semantics should own only semantic facts. Motion should own motion facts. Projection input should be built from motion + semantics at planning time, not stored as another persistent copy.

Today, because there is only one semantic entity (`SemanticEntityId::CursorTrail`), I would go further: do not use a generic scene map for this yet.

---

### 8. Projection snapshots are duplicated across cache and realization

Projection cache holds:

- `ProjectionCacheEntry.snapshot`

See `scene.rs:416-445`.

Realization ledger holds:

- `RealizationLedger::Consistent { acknowledged: ProjectionSnapshot }`
- `RealizationLedger::Diverged { last_consistent: Option<ProjectionSnapshot>, ... }`

See `core/state/realization.rs:88-149`.

Conceptually these are different roles:

- “latest reusable projection”
- “trusted shell-applied projection”

So the model distinction is fine.

But the representation is too heavy: both places own a full projection snapshot payload.

**Jane Street fix:** store immutable projection records once and share them by handle/id. Cache and realization should point to the same projection object, not clone it.

---

### 9. Some steady-state fields are purely derived and should not be stored

Three easy examples:

- `ScenePatch.kind` is derived from `basis` but stored separately
  (`scene.rs:525-569`)
- `DirtyEntitySet` is derived from old/new semantics and is effectively telemetry
  (`planning.rs:180-187`, `scene.rs:571-580`)
- `SemanticScene` is a `BTreeMap` even though the only entity is `CursorTrail`
  (`scene.rs:25-30`, `scene.rs:201-219`)

These are not the worst violations, but they are exactly the kind of “genericity first, data model second” residue that prevents the code from feeling sharp.

---

### 10. The ownership doc still blesses double ownership in a few places

`docs/state_ownership.md` is useful, but some lines are not strict enough.

Examples:

- it marks both `ObservationRequest.observation_id` and `ObservationBasis.observation_id` as authoritative (`docs/state_ownership.md:55-58`)
- it marks both `ObservationSlot` and `ProtocolWorkflow::*request` as authoritative (`docs/state_ownership.md:82-89`)

That document should be rewritten so each semantic fact has one authoritative row, not two.

---

## The normalized spec I would write

Below is the state model I would adopt.

The big design decisions are:

1. impossible protocol states are unrepresentable
2. observations are split into `PendingObservation` and `Observation`
3. active observations do not carry separate request/probe-policy mirrors
4. motion owns motion; semantics owns semantics
5. projections are immutable shared records
6. all caches are purgeable and never authoritative

---

## 1. Top-level model

```rust
struct Model {
    generation: Generation,
    entropy: EntropyState,

    protocol: ProtocolState,

    motion: MotionState,
    semantics: SemanticState,
    projection: ProjectionState,
    realization: RealizationState,

    latest_exact_cursor_position: Option<CursorPosition>,
}
```

### Ownership

- `generation` owns lifecycle freshness
- `entropy` owns sequence/proposal allocation
- `protocol` owns ingress queue, timers, retries, and current workflow
- `motion` owns all live cursor simulation state
- `semantics` owns only semantic scene facts
- `projection` owns cached projected outputs and planner reuse state
- `realization` owns shell-trust state
- `latest_exact_cursor_position` owns the last exact fallback anchor

`SceneState` as a mixed bag disappears.

---

## 2. Protocol state

```rust
struct ProtocolShared {
    demand_queue: DemandQueue,
    timers: TimerState,
    recovery_policy: RecoveryPolicyState,
    ingress_policy: IngressPolicyState,
    render_cleanup: RenderCleanupState,
}

struct ProtocolState {
    shared: ProtocolShared,
    phase: ProtocolPhase,
}

enum ProtocolPhase {
    Idle,
    Primed,

    Collecting {
        retained_previous: Option<Box<Observation>>,
        pending: PendingObservation,
        probe_refresh: ProbeRefreshState,
    },

    Observing {
        active: Box<Observation>,
        probe_refresh: ProbeRefreshState,
        prepared_plan: Option<PreparedObservationPlan>,
    },

    Ready {
        active: Box<Observation>,
    },

    Planning {
        active: Box<Observation>,
        proposal_id: ProposalId,
    },

    Applying {
        active: Box<Observation>,
        proposal: Box<InFlightProposal>,
    },

    Recovering {
        retained_previous: Option<Box<Observation>>,
    },
}
```

### Why this is the right shape

This removes both:

- `ObservationSlot`
- `ProtocolWorkflow`

as separate state axes.

There is no longer any need for:

- `workflow_allows_observation_slot(...)`
- workflow/slot matrix assertions
- duplicated `request` fields in workflow variants

Each variant carries exactly the observation payload it is allowed to carry.

---

## 3. Observation state

### Pending observation

```rust
struct PendingObservation {
    demand: ExternalDemand,
    requested_probes: ProbeRequestSet,
}
```

`PendingObservation` exists only before basis collection completes.

### Active observation

```rust
struct Observation {
    demand: ExternalDemand,
    basis: ObservationBasis,
    motion: ObservationMotion,

    cursor_color_probe: CursorColorProbeState,
    background_probe: BackgroundProbeState,
}
```

### Observation identity

```rust
impl PendingObservation {
    fn id(&self) -> ObservationId {
        ObservationId::from_ingress_seq(self.demand.seq())
    }
}

impl Observation {
    fn id(&self) -> ObservationId {
        ObservationId::from_ingress_seq(self.demand.seq())
    }
}
```

There is no `observation_id` field on `ObservationBasis`.
There is no separate `observation_id` field on the request object.
The root observation owns identity.

### Basis

```rust
struct ObservationBasis {
    collected_at: Millis,   // when the basis was actually read
    mode: String,
    cursor_position: Option<CursorPosition>,
    cursor_location: CursorLocation,
    viewport: ViewportSnapshot,
    cursor_color_witness: Option<CursorColorProbeWitness>,
    cursor_text_context: CursorTextContextState,
}
```

I would rename current `observed_at` to `collected_at` here, because `ExternalDemand.observed_at` and basis time are different clocks. That removes ambiguity.

### Probe states

```rust
enum CursorColorProbeState {
    Unrequested,
    Pending,
    Ready {
        observed_from: ObservationId,
        reuse: ProbeReuse,
        sample: Option<CursorColorSample>,
    },
    Failed {
        failure: ProbeFailure,
    },
}

enum BackgroundProbeState {
    Unrequested,
    Collecting {
        progress: BackgroundProbeProgress,
    },
    Ready {
        observed_from: ObservationId,
        reuse: ProbeReuse,
        batch: BackgroundProbeBatch,
    },
    Failed {
        failure: ProbeFailure,
    },
}
```

### Rules

- `requested_probes` exists only in `PendingObservation`
- once active, requestedness is encoded by `Unrequested` vs non-`Unrequested`
- no probe state stores `ProbeRequestId`
- `ProbeRequestId` is a pure helper function:
  `probe_request_id(observation_id, kind)`

---

## 4. Motion state

```rust
struct MotionState {
    config: RuntimeConfig,

    enabled: bool,
    animation: AnimationState,

    current_pose: CursorPose,
    target: CursorTarget,
    trail: TrailState,
    dynamics: MotionDynamics,

    particles: ParticleState,

    last_mode_was_cmdline: Option<bool>,
    committed_cursor_color: Option<u32>,

    revision: MotionRevision,
}

struct CursorPose {
    corners: [Point; 4],
    previous_center: Point,
}

struct CursorTarget {
    position: Point,
    shape: CursorShape,
    location: Option<CursorLocation>,
    epoch: u64,
}

struct TrailState {
    id: StrokeId,
    origin_corners: [Point; 4],
    elapsed_ms: [f64; 4],
}

struct MotionDynamics {
    velocity_corners: [Point; 4],
    spring_velocity_corners: [Point; 4],
}

struct ParticleState {
    live: Vec<Particle>,
    rng_state: u32,
}

enum AnimationState {
    Uninitialized,
    Idle,

    Settling {
        stable_since_ms: f64,
        settle_deadline_ms: f64,
    },

    Running {
        clock: MotionClock,
        settle_hold_counter: u32,
    },

    Draining {
        clock: MotionClock,
        remaining_steps: NonZeroU32,
    },
}
```

### What this fixes

Current code spreads target truth across:

- `target_position`
- `target_corners`
- `tracked_location`
- `retarget_epoch`
- `trail_stroke_id`

The normalized model makes ownership explicit:

- `CursorTarget` owns target identity
- `TrailState` owns trail identity
- `target_corners` is derived from `target.position + target.shape`

`trail_stroke_id` should not live in the same miscellaneous transient bag as `tracked_location`.

### Revision rule

`MotionRevision` increments whenever a change can affect projection output:

- current pose changes
- target changes
- trail changes
- particles change
- any motion-visible field used by planning changes

It does **not** increment for pure telemetry/cache changes.

---

## 5. Semantic state

Because the plugin currently has exactly one semantic entity, I would not keep a generic scene map yet.

```rust
struct SemanticState {
    revision: SemanticRevision,
    cursor_trail: Option<CursorTrailSemantic>,
}

struct CursorTrailSemantic {
    target_cell_presentation: TargetCellPresentation,
}
```

### Important point

All of this current state should **not** live in semantics:

- mode
- corners
- step samples
- target point
- target corners
- retarget epoch
- trail stroke id
- particles
- planner policy

Those are motion/projection inputs, not semantic scene ownership.

So these persistent structs should disappear from steady state:

- `CursorTrailGeometry`
- `CursorTrailProjectionPolicy`

Semantics should answer only:

- does a trail entity exist?
- what semantic target-cell presentation should it have?

Everything else comes from motion and policy snapshots when projection is built.

### Revision rule

`SemanticRevision` increments only when semantic truth changes:

- trail appears/disappears
- `target_cell_presentation` changes

Not when motion animates.

---

## 6. Projection state

```rust
type ProjectionHandle = Arc<ProjectionRecord>;

struct ProjectionState {
    current_target: Option<ProjectionHandle>,
    reusable_planner: Option<ReusablePlannerState>,
}

struct ProjectionRecord {
    id: ProjectionId,
    witness: ProjectionWitness,
    logical_raster: Arc<LogicalRaster>,
    realization: Arc<RealizationProjection>, // cache derived from logical_raster
}

struct ProjectionWitness {
    render_revision: RenderRevision,
    observation_id: ObservationId,
    viewport: ViewportSnapshot,
    projector_revision: ProjectorRevision,
}

struct RenderRevision {
    motion: MotionRevision,
    semantics: SemanticRevision,
}

struct ReusablePlannerState {
    projection: ProjectionHandle,
    planner_state: ProjectionPlannerState,
    key: ProjectionReuseKey,
}

struct ProjectionReuseKey {
    trail_signature: Option<u64>,
    particle_overlay_signature: Option<u64>,
    planner_clock: Option<ProjectionPlannerClock>,
    target_cell_presentation: TargetCellPresentation,
    projection_policy_revision: ProjectionPolicyRevision,
    viewport: ViewportSnapshot,
}
```

### Why this shape is better

- cache and realization can share `ProjectionHandle`
- planner reuse state points at a projection; it does not own a second snapshot copy
- witness identity is a pair of revisions plus observation/viewport, not “scene revision that secretly includes motion because scene owns motion”

### Build rule

Projection input is **ephemeral**, not persistent state.

It is built from:

- `MotionState`
- `SemanticState`
- active `Observation`
- immutable projection policy snapshot

That means `RenderFrame`/`PlannerFrame` can exist as transient values, but they are not stored back into scene state.

---

## 7. Realization state

```rust
enum RealizationState {
    Cleared,
    Consistent {
        acknowledged: ProjectionHandle,
    },
    Diverged {
        last_consistent: Option<ProjectionHandle>,
        divergence: RealizationDivergence,
    },
}
```

### Key rule

`RealizationState` never owns a full second `ProjectionSnapshot`.
It owns a shared handle to an immutable projection record.

That preserves the conceptual distinction:

- projection cache = reusable target
- realization = trusted shell state

without duplicating payload.

---

## 8. Config snapshots and policy normalization

Right now the same configuration subset is represented in too many shapes:

- `RuntimeConfig`
- `StaticRenderConfig`
- `PlannerRenderConfig`
- `CursorTrailProjectionPolicy`

The normalized model should have exactly two derived immutable snapshots:

```rust
struct RenderPolicies {
    projection: Arc<ProjectionPolicySnapshot>,
    palette: Arc<PalettePolicySnapshot>,
}
```

### Projection policy snapshot owns only fields that affect logical raster

Examples:

- hide target hack
- max kept windows
- draw-over-target policy
- particle overlay policy
- tail duration / simulation Hz
- thickness
- planner weights
- z-index

### Palette policy snapshot owns only fields that affect highlight/palette realization

Examples:

- cursor color strings
- insert-mode color override
- background colors
- cterm colors
- gamma
- color levels
- transparent fallback

### Rule

- projection reuse keys use `projection_policy_revision`, not a cloned policy payload
- realization uses `palette` snapshot, not raw config copies
- `RenderFrame` borrows these snapshots by handle or revision

---

## 9. Derived caches that are allowed

These are fine to keep, but they must be explicitly marked “rebuildable” and never authoritative:

- prepared preview motion / prepared observation plan
- particle aggregation caches
- particle screen-cell caches
- render step sample scratch
- preview baseline/source scratch buffers
- `ProjectionRecord.realization`
- immutable config snapshots
- any digests/signatures used only for reuse keys

### Hard rule for caches

If you delete every cache after every reducer step, externally visible behavior must remain unchanged.

Only performance may change.

---

## 10. Boundary event/effect spec

I would also normalize messages so they reference state by id, not by mirrored payload.

### Observation base collected

Current event mirrors the request object:

```rust
struct ObservationBaseCollectedEvent {
    request: ObservationRequest,
    basis: ObservationBasis,
    motion: ObservationMotion,
}
```

I would change it to:

```rust
struct ObservationBaseCollectedEvent {
    observation_id: ObservationId,
    basis: ObservationBasis,
    motion: ObservationMotion,
}
```

Reducer rule: look up the current `PendingObservation` by `observation_id`. If it is not current, ignore the event.

### Probe reported

Current events carry both `observation_id` and `probe_request_id`.

I would change them to:

```rust
enum ProbeReportedEvent {
    CursorColorReady {
        observation_id: ObservationId,
        reuse: ProbeReuse,
        sample: Option<CursorColorSample>,
    },
    CursorColorFailed {
        observation_id: ObservationId,
        failure: ProbeFailure,
    },
    BackgroundReady {
        observation_id: ObservationId,
        reuse: ProbeReuse,
        batch: BackgroundProbeBatch,
    },
    BackgroundChunkReady {
        observation_id: ObservationId,
        chunk: BackgroundProbeChunk,
        allowed_mask: BackgroundProbeChunkMask,
    },
    BackgroundFailed {
        observation_id: ObservationId,
        failure: ProbeFailure,
    },
}
```

If you still want `ProbeRequestId` for logs, compute it from `(observation_id, kind)` at the edge.

---

## 11. Invariants for the rewritten design

These should be written down as the canonical contract.

1. Every semantic fact has exactly one owner.
2. `ProtocolPhase` is the only owner of workflow phase.
3. `PendingObservation` is the only owner of requested probe policy.
4. `Observation` is the only owner of active observation basis and probe lifecycle.
5. Observation id exists only at the observation root.
6. Probe requestedness is encoded only by probe state once active.
7. `ProbeRequestId` is derivable and never stored in steady state.
8. `CursorTarget` is the only owner of target identity.
9. `TrailState` is the only owner of trail identity.
10. Semantics never store motion geometry.
11. Projection records are immutable and shared by handle.
12. `ScenePatchKind` is derived, never stored.
13. Dirty sets are transient diagnostics, not steady state.
14. All caches are purgeable without changing behavior.

---

## 12. The minimum migration order

If I were doing this surgically, I would do it in this order:

1. Replace `ProtocolWorkflow + ObservationSlot` with `ProtocolPhase`.
2. Split `ObservationRequest` into `PendingObservation`, and make `Observation` own only active data.
3. Remove `ObservationBasis.observation_id`.
4. Remove active `requested_probes` duplication; use probe states as requestedness.
5. Remove stored `ProbeRequestId` from steady state.
6. Introduce `CursorTarget` and `TrailState`; derive `target_corners`.
7. Split `SceneState` into `motion`, `semantics`, `projection`, `realization`.
8. Replace generic `SemanticScene` map with `Option<CursorTrailSemantic>`.
9. Replace duplicated projection snapshots with shared handles.
10. Delete `ScenePatch.kind` and persisted `DirtyEntitySet`.
11. Rewrite `docs/state_ownership.md` so every fact has one authoritative row.

## Bottom line

The two biggest flaws are:

- `ProtocolWorkflow` and `ObservationSlot` jointly encode the same phase space
- active observations carry mirrored request/probe identity that the code has to keep in sync with assertions

If you fix those, the rest of the cleanup becomes straightforward.

The spec above gives you a normalized model where:

- protocol phase is singular
- observation identity is singular
- probe identity is derived
- motion and semantics stop mirroring each other
- projection and realization share immutable handles instead of cloned snapshots

That would feel materially sharper and much closer to “Jane Street level.”
