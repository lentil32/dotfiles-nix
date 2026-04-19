# Smear Cursor State Ownership

This document names the mutable owners in `nvimrs-smear-cursor` and labels each
field as one of:

- `authoritative`: the reducer-owned source of truth for a semantic fact
- `cache`: derived data that can be rebuilt from authoritative state
- `snapshot`: a retained copy of external or prior state used for comparisons
- `telemetry`: bookkeeping that describes execution rather than UI truth

If a field is not `authoritative`, this document must say what authoritative
state it is derived from and how it invalidates.

## Runtime State

| Field | Class | Owner / derivation |
| --- | --- | --- |
| `RuntimeState.config` | authoritative | User-configured runtime behavior. |
| `RuntimeState.render_static_config` | cache | Derived from `config`; invalidated by runtime-option patches that mutate `config` and rebuilt with `refresh_render_static_config()`. |
| `RuntimeState.plugin_state` | authoritative | Runtime enable / disable lifecycle. |
| `RuntimeState.animation_phase` | authoritative | Runtime animation lifecycle. Settling variants own only timing; target and tracking remain transient-owned. |
| `RunningPhase.clock` | authoritative | Running-phase animation timing owner. |
| `DrainingPhase.clock` | authoritative | Tail-drain animation timing owner. |
| `MotionClock.last_tick_ms` | telemetry | Last consumed animation tick timestamp for the active motion clock. |
| `MotionClock.next_frame_at_ms` | telemetry | Next scheduled animation deadline for the active motion clock. |
| `MotionClock.simulation_accumulator_ms` | telemetry | Fixed-step accumulated elapsed time for the active motion clock. |
| `RuntimeState.current_corners` | authoritative | Current simulated cursor geometry. |
| `RuntimeState.trail_origin_corners` | authoritative | Tail origin geometry for the live motion model. |
| `RuntimeState.target_corners` | authoritative | Current render target geometry. |
| `RuntimeState.velocity_corners` | authoritative | Live physics velocity state. |
| `RuntimeState.spring_velocity_corners` | authoritative | Live spring velocity state. |
| `RuntimeState.trail_elapsed_ms` | authoritative | Per-corner trail lifetime state. |
| `RuntimeState.particles` | authoritative | Live particle simulation payload. |
| `RuntimeState.preview_particles_source` | snapshot | Borrowed snapshot of the source runtime's particles for preview planning. |
| `RuntimeState.preview_particles_materialized` | telemetry | Preview bookkeeping: whether `particles` diverged from `preview_particles_source`. |
| `RuntimeState.preview_baseline` | snapshot | Baseline runtime snapshot paired with `preview_particles_source`. |
| `RuntimeState.preview_particles_scratch` | cache | Reusable allocation scratch for preview particle materialization; invalidated when `reclaim_preview_particles_scratch()` installs a larger reclaimed buffer. |
| `RuntimeState.aggregated_particle_cells` | cache | Derived from authoritative live particles; invalidated by `invalidate_cached_particle_artifacts()` whenever `particles` changes. |
| `RuntimeState.aggregated_particle_cells_dirty` | telemetry | Cache invalidation flag for `aggregated_particle_cells`. |
| `RuntimeState.particle_screen_cells` | cache | Derived from authoritative live particles; invalidated by `invalidate_cached_particle_artifacts()` whenever `particles` changes. |
| `RuntimeState.particle_screen_cells_dirty` | telemetry | Cache invalidation flag for `particle_screen_cells`. |
| `RuntimeState.previous_center` | authoritative | Prior simulated cursor anchor used by motion stepping. |
| `RuntimeState.rng_state` | authoritative | Reducer-owned RNG state for deterministic particle generation. |
| `TransientRuntimeState.target_position` | authoritative | Cursor target position. This is the owner even while settling. |
| `TransientRuntimeState.retarget_epoch` | authoritative | Monotonic retarget identity for render / projection reuse. |
| `TransientRuntimeState.trail_stroke_id` | authoritative | Current semantic trail instance identity. |
| `TransientRuntimeState.last_mode_was_cmdline` | snapshot | Last observed mode classification from the editor. |
| `TransientRuntimeState.tracked_location` | authoritative | Last trusted cursor location from ingress. This is the owner even while settling. |
| `TransientRuntimeState.color_at_cursor` | authoritative | Last committed cursor color sample applied to runtime rendering. |

## Observation State

| Field | Class | Owner / derivation |
| --- | --- | --- |
| `ObservationRequest.observation_id` | authoritative | Identity for a single observation workflow. |
| `ObservationRequest.demand` | snapshot | Retained ingress demand snapshot that started the observation. |
| `ObservationRequest.probes` | authoritative | Requested probe policy for the observation workflow. |
| `ObservationBasis.observation_id` | authoritative | Observation identity echoed into base data. |
| `ObservationBasis.observed_at` | snapshot | Timestamp captured from ingress collection. |
| `ObservationBasis.mode` | snapshot | Editor mode snapshot. |
| `ObservationBasis.cursor_position` | snapshot | Best cursor position captured for this observation. |
| `ObservationBasis.cursor_location` | snapshot | Cursor location snapshot from ingress. |
| `ObservationBasis.viewport` | snapshot | Viewport snapshot from ingress. |
| `ObservationBasis.cursor_color_witness` | snapshot | Witness used to validate cursor color probe reuse. |
| `ObservationBasis.cursor_text_context_state` | snapshot | Unified cursor text-context snapshot. `BoundaryOnly` retains the reuse witness; `Sampled` owns the sampled rows and derives the boundary from that sample. |
| `ObservationSnapshot.request` | authoritative | Observation workflow owner for request metadata. |
| `ObservationSnapshot.basis` | authoritative | Observation base owner for ingress snapshots. |
| `ObservationSnapshot.probes.cursor_color` | authoritative | Cursor-color probe lifecycle owner. |
| `ObservationSnapshot.background_probe` | authoritative | Unified background-probe lifecycle owner. `Collecting` owns the request id and chunk progress; terminal variants own the sampled batch or failure with their reuse metadata. |
| `ObservationSnapshot.motion` | authoritative | Observation-scoped motion metadata. |

## Core Protocol State

| Field | Class | Owner / derivation |
| --- | --- | --- |
| `ProtocolSharedState.demand` | authoritative | Queued ingress demands waiting for protocol service. |
| `ProtocolSharedState.timers` | authoritative | Timer token ownership and generations. |
| `ProtocolSharedState.recovery_policy` | authoritative | Recovery retry policy state. |
| `ProtocolSharedState.ingress_policy` | authoritative | Ingress delay / cursor-autocmd policy state. |
| `ProtocolSharedState.render_cleanup` | authoritative | Deferred render cleanup lifecycle. |
| `ProtocolState.shared` | authoritative | Shared protocol policy state carried across workflow transitions. |
| `ProtocolState.observation` | authoritative | Stable observation slot owner; the workflow matrix constrains whether it may be `Empty`, `Retained`, or `Active`. |
| `ObservationSlot::Retained(observation)` | snapshot | Retained prior observation reused while a new request or recovery workflow is in flight. |
| `ObservationSlot::Active(observation)` | authoritative | Current observation used by observing-active, ready, planning, and applying workflows. |
| `ProtocolState.workflow` | authoritative | Protocol lifecycle owner independent of the observation slot. |
| `ProtocolWorkflow::ObservingRequest.request` | authoritative | Current observation request while base collection is pending. |
| `ProtocolWorkflow::{ObservingRequest,ObservingActive}.probe_refresh` | authoritative | Probe refresh retry lifecycle for the active observation workflow. |
| `ProtocolWorkflow::ObservingActive.request` | authoritative | Current observation request paired with the active observation. |
| `ProtocolWorkflow::ObservingActive.prepared_plan` | cache | Preview-planned runtime transition derived from the active observation and current runtime; invalidated when the active observation changes or the workflow leaves `ObservingActive`. |
| `ProtocolWorkflow::Planning.proposal_id` | authoritative | In-flight proposal identity allocation. |
| `ProtocolWorkflow::Applying.proposal` | authoritative | In-flight realization proposal being applied. |
| `CoreState.generation` | authoritative | Lifecycle generation for cache invalidation and effect freshness. |
| `CoreState.protocol` | authoritative | Protocol lifecycle owner. |
| `CoreStatePayload.entropy` | authoritative | Proposal / ingress sequence allocation. |
| `CoreStatePayload.latest_exact_cursor_position` | authoritative | Fallback cursor anchor updated only from exact cursor observations. |
| `CoreStatePayload.scene` | authoritative | Semantic scene plus runtime motion owner. |
| `CoreStatePayload.realization` | authoritative | Acknowledged shell realization owner. |

## Scene And Realization State

| Field | Class | Owner / derivation |
| --- | --- | --- |
| `SceneState.revision` | authoritative | Semantic-scene revision. |
| `SceneState.semantics` | authoritative | Semantic entity graph for the current frame. |
| `SceneState.motion` | authoritative | Runtime motion state for rendering. |
| `SceneState.projection` | cache | Planner reuse cache derived from scene semantics plus observation / policy witnesses; invalidated when `apply_planned_update()` mutates scene semantics or when the projection witness / reuse key stops matching. |
| `SceneState.dirty` | telemetry | Dirty semantic-entity bookkeeping for incremental planning. |
| `ProjectionCacheEntry.planner_state` | cache | Retained planner reuse state derived from the last projection; invalidated when `reuse_key` no longer matches the current scene / policy inputs or when the cache entry is replaced. |
| `ProjectionCacheEntry.snapshot` | cache | Retained projection snapshot derived from scene semantics; invalidated when `reuse_key` no longer matches or when the cache entry is replaced. |
| `ProjectionCacheEntry.reuse_key` | cache | Key derived from semantic geometry, planner clock, presentation, and projection policy; invalidated by any change to those inputs. |
| `ProjectionSnapshot.witness` | snapshot | Observation / revision witness for a retained projection. |
| `ProjectionSnapshot.logical_raster` | cache | Derived logical raster for the retained projection; invalidated when the projection snapshot is replaced. |
| `ProjectionSnapshot.realization` | cache | Derived from `logical_raster` via `realize_logical_raster`; invalidated when `logical_raster` is replaced. |
| `RealizationLedger::Consistent.acknowledged` | authoritative | Trusted shell realization. |
| `RealizationLedger::Diverged.last_consistent` | snapshot | Last trusted shell realization before divergence. |
| `RealizationLedger::Diverged.divergence` | authoritative | Current divergence reason. |

## Invariant Hooks

The debug-only invariant entrypoints live next to the owning state types:

- `RuntimeState::debug_assert_invariants()`
- `ObservationSnapshot::debug_assert_invariants()`
- `CoreState::debug_assert_invariants()`

Current invariants pin the adopted ownership model:

- settling phase may own timing only, and requires transient tracking ownership
- running and draining phases are the only owners of animation tick bookkeeping
- cursor text context state owns boundary-only and sampled text snapshots in one slot
- background probe state is the single owner of request id, progress, and terminal payload
- protocol workflow and observation-slot combinations must satisfy the central matrix in `ProtocolState`
