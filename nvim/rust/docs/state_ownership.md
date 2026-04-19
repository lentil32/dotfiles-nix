# Smear Cursor State Ownership

This document names semantic facts, not raw fields.

Every fact appears once, with one authoritative mutable owner. Helper accessors
that reconstruct a fact from that owner are listed only as derived views, not as
second owners.

Each row is classified as one of:

- `authoritative`: reducer-owned source of truth for a semantic fact
- `cache`: derived state that can be purged and rebuilt from authoritative state
- `snapshot`: retained copy of ingress, prior, or external state used for reuse or
  comparison
- `telemetry`: bookkeeping that describes execution rather than UI truth

## Top-Level Split

Reducer-owned truth lives under `EngineState.core_state`. Shell-facing state lives
under `EngineState.shell` and is limited to purgeable caches, reusable scratch
buffers, telemetry, and retained host capability snapshots. Shell state may
accelerate reads or avoid redundant host calls, but it does not own reducer
semantics.

At the root boundary:

- `CoreState` is the only authoritative top-level reducer subtree.
- `ShellState` is restricted to cache, snapshot, and telemetry roles listed in
  this document; purging it may change cost or reuse, but not semantic output
  for the same external input sequence.

## Non-Owners

These types and accessors intentionally do not own semantic facts:

- `SceneState` and `CoreState::shared_scene()` are composite views over
  `motion + semantics + projection`.
- `CoreState::phase_observation()` and `ProtocolState::phase_observation()` are
  derived views over whichever observation payload the current phase legally owns.
- `PatchBasis::kind()` derives `ScenePatchKind`; the kind is never stored.
- Raw retained projection access stays inside `crate::core`; shell and event
  code cross the boundary through explicit `ProjectionHandle` views instead of
  borrowing `RetainedProjection` directly.
- `ProtocolPhaseKind` and `Lifecycle` are derived from `ProtocolState.phase`.

## Field Role Quick Map

This storage-role index classifies the main reducer and shell roots by field.
It does not assign new owners; it only labels how the stored fields are used.

- `EngineState`: `core_state` is the authoritative reducer root; `shell` is the
  shell-owned cache, snapshot, and telemetry root.
- `ShellState`
  - snapshot: `namespace_id`, `host_bridge_state`, `editor_viewport_cache`,
    `buffer_metadata_cache`, `real_cursor_visibility`
  - cache: `probe_cache`, `background_probe_request_scratch`,
    `conceal_regions_scratch`, `buffer_text_revision_cache`,
    `buffer_perf_policy_cache`
  - telemetry: `buffer_perf_telemetry_cache`
- `RuntimeState`
  - authoritative: `config`, `config_revision`, `projection_policy`,
    `plugin_state`, `animation_phase`, `current_corners`, `target`, `trail`,
    `velocity_corners`, `spring_velocity_corners`, `particles`,
    `previous_center`, `rng_state`, `transient`
  - cache: `derived_config`, `caches`
- `RuntimeCaches`: cache-only grouping for `scratch_buffers` and
  `particle_artifacts`.
- `ProtocolSharedState`: `demand`, `timers`, `recovery_policy`,
  `ingress_policy`, and `render_cleanup` are authoritative.
- `PendingObservation`: `demand` is snapshot; `requested_probes` is
  authoritative.
- `ObservationBasis`: `observed_at`, `mode`, `surface`, `cursor`, `viewport`,
  `buffer_revision`, and `cursor_text_context_state` are authoritative.
- `ObservationSnapshot`: `demand` is snapshot; `basis`, `probes`, and `motion`
  are authoritative; `cursor_color_probe_generations` is a retained snapshot
  witness used to derive cursor-color reuse keys.
- `ProtocolPhase`
  - snapshot: `Collecting.retained`, `Recovering.retained`
  - authoritative: `Collecting.pending`, both `probe_refresh` fields,
    each `active` observation payload, `Planning.proposal_id`,
    `Applying.proposal`
  - cache: `Observing.prepared_plan`
- `CoreStatePayload`: `entropy`, `latest_exact_cursor_cell`, `motion`,
  `semantics`, `projection`, and `realization` are authoritative reducer-owned
  payload.
- `CoreState`: `generation`, `protocol`, and `payload` are authoritative.
- `ProjectionState`: `motion_revision` and `last_motion_fingerprint` are
  authoritative; `cache` is cache.
- `ProjectionReuseCache`: `retained_projection` is cache.
- `RetainedProjection`: `witness` is snapshot; `reuse_key`,
  `cached_planner_state`, `cached_logical_raster`, and `cached_realization` are
  cache.
- `RealizationLedger`
  - authoritative: `Consistent.acknowledged`, `Diverged.divergence`
  - snapshot: `Diverged.last_consistent`
- `SceneState`: non-owner composite view over authoritative `semantics` and
  `projection` handles.

## Core And Protocol Facts

| Fact | Class | Owner / derivation |
| --- | --- | --- |
| Lifecycle freshness | authoritative | `CoreState.generation` owns reducer freshness for cache invalidation and effect staleness. |
| Queued ingress demands | authoritative | `ProtocolSharedState.demand` owns `ExternalDemand`s waiting for protocol service. |
| Timer generations and tokens | authoritative | `ProtocolSharedState.timers` owns timer-slot generations and armed/disarmed lifecycle state. `TimerToken`s are derived views of the currently armed slots, not a second stored owner. |
| Recovery retry policy | authoritative | `ProtocolSharedState.recovery_policy` owns retry counters and backoff state. |
| Ingress throttling and autocmd policy | authoritative | `ProtocolSharedState.ingress_policy` owns delay and cursor-autocmd policy. |
| Deferred render cleanup lifecycle | authoritative | `ProtocolSharedState.render_cleanup` owns cleanup thermal phase and deadlines only through `RenderCleanupState::{Hot, Cooling, Cold}`. Retention budgets are derived from the current runtime config instead of being copied into scheduler state. |
| Protocol workflow phase | authoritative | `ProtocolState.phase` is the only workflow owner. There is no separate workflow/slot matrix. |
| Protocol-attached observation storage | authoritative | The active `ProtocolPhase` variant owns exactly one phase-legal observation payload: `Collecting.pending`, `Collecting.retained`, `Observing.active`, `Ready.active`, `Planning.active`, `Applying.active`, or `Recovering.retained`. |
| Probe-refresh retry state | authoritative | `ProtocolPhase::{Collecting, Observing}.probe_refresh` owns per-observation probe refresh retries while the protocol remains on the observation path. |
| Prepared observation preview | cache | `ProtocolPhase::Observing.prepared_plan` caches the preview runtime transition derived from the active observation and current runtime. It is invalidated when the observation changes or the phase leaves `Observing`. |
| Planning proposal identity | authoritative | `ProtocolPhase::Planning.proposal_id` owns the in-flight proposal id allocation. |
| Applying proposal payload | authoritative | `ProtocolPhase::Applying.proposal` owns the in-flight realization proposal. |
| Proposal and ingress sequence allocation | authoritative | `CoreStatePayload.entropy` owns proposal id and ingress sequence allocation. |
| Latest exact cursor fallback anchor | authoritative | `CoreStatePayload.latest_exact_cursor_cell` owns the last exact cursor cell reused when a later observation lacks one. |

## Observation Facts

| Fact | Class | Owner / derivation |
| --- | --- | --- |
| Observation identity | authoritative | Identity is derived only from the observation root demand sequence: `PendingObservation.demand.seq()` while collecting and `ObservationSnapshot.demand.seq()` once active. `ObservationId` accessors compute from that root; neither the snapshot nor active probe lifecycle state stores a mirrored current-observation id. |
| Pending ingress demand | snapshot | `PendingObservation.demand` retains the ingress request while basis collection is in flight. |
| Pending requested probe policy | authoritative | `PendingObservation.requested_probes` is the only owner of probe policy before activation. `ObservationSnapshot::new()` consumes it to initialize active probe lifecycle state. |
| Active ingress demand | snapshot | `ObservationSnapshot.demand` retains the ingress request that produced the active observation. |
| Active observation basis | authoritative | `ObservationSnapshot.basis` owns `observed_at`, `mode`, `surface`, `cursor`, `viewport`, `buffer_revision`, and `cursor_text_context_state`. |
| Observation-scoped motion metadata | authoritative | `ObservationSnapshot.motion` owns scroll-shift metadata for the active observation. |
| Cursor-color probe generation witness | snapshot | `ObservationSnapshot.cursor_color_probe_generations` retains the shell-side cursor-color probe generations needed to derive reuse-safe cursor-color witnesses without turning them into a second semantic owner. |
| Cursor-color probe requestedness and lifecycle | authoritative | `ObservationSnapshot.probes.cursor_color` is the sole active-state owner. `ProbeSlot::Unrequested` vs `Requested(...)` carries requestedness, and `ProbeState` carries reuse, failure, and sample payload. |
| Background probe requestedness and lifecycle | authoritative | `ObservationSnapshot.probes.background` is the sole active-state owner. `Unrequested`, `Collecting`, `Ready`, and `Failed` cover requestedness, progress, reuse, and terminal payload. |

## Runtime Motion Facts

| Fact | Class | Owner / derivation |
| --- | --- | --- |
| Runtime configuration | authoritative | `RuntimeState.config` owns user-configured motion behavior, including render-cleanup retention policy such as `max_kept_windows`. Frame timing is stored canonically as `time_interval`, which is also the only accepted runtime option key for that fact. |
| Runtime config freshness | authoritative | `RuntimeState.config_revision` is the only freshness owner for config-derived runtime views. `DerivedConfigCache` intentionally stores no mirror revision. |
| Projection policy freshness | authoritative | `RuntimeState.projection_policy.revision()` owns freshness for projection-relevant runtime policy. `commit_runtime_config_update()` advances it only when `DerivedConfigCache::matches_projection_policy()` detects a projection-semantic policy change, so planner reuse keys and render frames do not drift on unrelated config churn. |
| Derived runtime render policy cache | cache | `RuntimeState.derived_config` caches policy slices derived from `config`. `RuntimeState::static_render_config()` reconstructs the shell-facing static render config view from that cache. |
| Plugin enable lifecycle | authoritative | `RuntimeState.plugin_state` owns enabled vs disabled plugin state. |
| Animation lifecycle | authoritative | `RuntimeState.animation_phase` owns uninitialized, idle, settling, running, and draining state. |
| Animation tick bookkeeping | authoritative | `RunningPhase.clock` and `DrainingPhase.clock` own `last_tick_ms`, `next_frame_at_ms`, and `simulation_accumulator_ms` while those phases are active. |
| Current simulated cursor geometry | authoritative | `RuntimeState.current_corners` owns the live simulated cursor corners. |
| Cursor target identity | authoritative | `RuntimeState.target` owns target `position`, discrete `cell`, `shape`, `retarget_surface`, `tracked_cursor`, and `retarget_epoch`. `CursorTarget::retarget_key()` derives the reviewable equality surface, and target corners are derived on demand by `CursorTarget::corners()` instead of being stored separately. |
| Trail identity | authoritative | `RuntimeState.trail` owns `stroke_id`, `origin_corners`, and `elapsed_ms`. |
| Velocity state | authoritative | `RuntimeState.velocity_corners` and `RuntimeState.spring_velocity_corners` own live physics velocity state. |
| Live particle simulation | authoritative | `RuntimeState.particles` owns live particle state. |
| Planning preview runtime snapshot | snapshot | `RuntimePreview::baseline` and `RuntimePreview::baseline_particles` retain the authoritative runtime baseline and borrowed source particles for a single preview session. |
| Reusable scratch allocations | cache | `RuntimeState.caches.scratch_buffers.{preview_particles, render_step_samples, particle_aggregation}` are reusable allocations only. |
| Derived particle artifacts | cache | `RuntimeState.caches.particle_artifacts.cached.{aggregated_particle_cells, particle_screen_cells}` derive from authoritative live particles. `None` means the artifacts are not materialized yet for the current runtime inputs, not that a cache-owned freshness bit changed. |
| Previous visual anchor | authoritative | `RuntimeState.previous_center` owns the previous simulated cursor anchor used by motion stepping. |
| Reducer-owned RNG state | authoritative | `RuntimeState.rng_state` owns deterministic particle generation state. |
| Last observed cmdline-mode classification | snapshot | `RuntimeState.transient.last_observed_mode` retains the last ingress mode classification witness. |
| Committed cursor color for runtime rendering | authoritative | `RuntimeState.transient.color_at_cursor` owns the last cursor-color sample committed into runtime rendering state. |

## Semantics, Projection, And Realization Facts

| Fact | Class | Owner / derivation |
| --- | --- | --- |
| Semantic scene revision | authoritative | `SemanticState.revision` owns semantic-scene freshness. |
| Cursor-trail semantic entity | authoritative | `SemanticState.cursor_trail` owns the optional cursor-trail semantic entity. `CursorTrailSemantic` owns the projection-relevant geometry and target-cell presentation for that entity. |
| Projection reuse cache | cache | `ProjectionState.cache` owns the purgeable retained projection reuse state. |
| Retained projection witness | snapshot | `RetainedProjection.witness` binds a retained projection to scene revision, observation id, viewport, and projector revision. |
| Projection reuse key | cache | `RetainedProjection.reuse_key` is derived from semantic geometry, planner clock, target-cell presentation, and projection policy. It is invalidated when any of those inputs changes. |
| Retained planner reuse state | cache | `RetainedProjection.cached_planner_state` retains the planner state paired with the cached projection snapshot. |
| Retained logical raster | cache | `RetainedProjection.cached_logical_raster` is the immutable retained projection output. |
| Cached shell realization of the retained projection | cache | `RetainedProjection.cached_realization` is derived from `cached_logical_raster` and cached alongside it. |
| Projection sharing handle | cache | `ProjectionHandle` shares one immutable `RetainedProjection` between projection cache and realization ledger instead of cloning snapshot payloads. Cross-module consumers project it through explicit views such as `semantic_view()` or `shell_projection()` rather than generic deref access. |
| Trusted shell-acknowledged projection | authoritative | `RealizationLedger::Consistent.acknowledged` owns the shell-trusted projection handle. |
| Last trusted projection before divergence | snapshot | `RealizationLedger::Diverged.last_consistent` retains the most recent trusted projection handle after divergence. |
| Current divergence reason | authoritative | `RealizationLedger::Diverged.divergence` owns the current reason the shell is no longer trusted to match reducer state. |

## Shell Boundary Facts

| Fact | Class | Owner / derivation |
| --- | --- | --- |
| Render namespace handle | snapshot | `ShellState.namespace_id` retains the draw namespace handle created by `ensure_namespace_id()`. It is a shell capability witness, not a realization or reducer truth owner, and transient cache resets intentionally preserve it. |
| Verified host bridge capability | snapshot | `ShellState.host_bridge_state` retains whether the current host bridge revision has been verified by `verify_host_bridge()`. It is an external capability witness, not a semantic owner. |
| Shell probe reuse state | cache | `ShellState.probe_cache` owns purgeable cursor-color, cursor-text-context, conceal-region, conceal-delta, and conceal-screen-cell reuse keyed by external witnesses such as `CursorColorProbeWitness`, buffer-local text revisions, and window state. `note_cursor_color_observation_boundary()`, `note_cursor_color_colorscheme_change()`, `note_conceal_read_boundary()`, `invalidate_buffer_local_probe_caches()`, and `reset_transient_caches()` are its invalidation and purge paths. |
| Probe request and conceal scratch buffers | cache | `ShellState.background_probe_request_scratch` and `ShellState.conceal_regions_scratch` retain reusable allocations for effect construction only. `reset_transient_caches()` drops the retained allocations, while `reclaim_background_probe_request_scratch()` and `reclaim_conceal_regions_scratch()` only recycle them between operations. |
| Editor viewport witness | snapshot | `ShellState.editor_viewport_cache` retains the last live `EditorViewportSnapshot` read from Neovim. `EditorViewportSnapshot` is also the canonical shell-side owner of command-row math and `ViewportBounds` projection through `command_row()` and `bounds()`. `refresh_editor_viewport_cache()`, `invalidate_editor_viewport_cache()`, and `reset_transient_caches()` refresh or purge the witness without changing semantic state. |
| Buffer metadata witness | snapshot | `ShellState.buffer_metadata_cache` retains `BufferMetadata` read from Neovim so policy and probe code can reuse it without rereading host options on every ingress. `invalidate_buffer_metadata()`, `invalidate_buffer_local_caches()`, and `reset_transient_caches()` clear it. |
| Buffer text revision cache | cache | `ShellState.buffer_text_revision_cache` retains shell-local generations used to partition buffer-local probe reuse and invalidation. `invalidate_buffer_local_caches()` and `reset_transient_caches()` clear it; it never becomes reducer truth. |
| Buffer performance policy cache | cache | `ShellState.buffer_perf_policy_cache` caches `BufferEventPolicy` derived from the current ingress snapshot, `BufferMetadata`, and buffer-local telemetry. It is invalidated per buffer or by `reset_transient_caches()` and does not own `BufferPerfClass` independently of those inputs. |
| Buffer performance telemetry | telemetry | `ShellState.buffer_perf_telemetry_cache` records callback EWMA and probe-pressure signals used to explain or derive future buffer performance policy. It does not own the selected `BufferPerfClass`, and `invalidate_buffer_local_caches()` plus `reset_transient_caches()` purge it. |
| Real cursor visibility witness | snapshot | `ShellState.real_cursor_visibility` retains the last shell cursor visibility applied so host calls can skip redundant hide/show work. `note_cursor_color_colorscheme_change()`, `invalidate_real_cursor_visibility()`, and `reset_transient_caches()` clear it. |

## Invariant Hooks

The debug-only invariant entrypoints live next to the owning state types:

- `RuntimeState::debug_assert_invariants()`
- `ObservationSnapshot::debug_assert_invariants()`
- `ProjectionState::debug_assert_invariants()`
- `RealizationLedger::debug_assert_invariants()`
- `ProtocolState::debug_assert_invariants()`
- `CoreState::debug_assert_invariants()`

## Named Enforcement Points

Important weak forms are normalized or rejected at one boundary each:

- `apply_runtime_options()` validates and normalizes `time_interval` before
  `RuntimeState.config` is mutated, so frame timing has one accepted boundary
  key and one retained owner.
- `RenderCleanupState::scheduled()` clamps cleanup delays before the scheduler
  stores them in `ProtocolSharedState.render_cleanup`.
- `TimerState::{arm, active_token, clear_matching}` keep timer generation and
  armed/disarmed ownership in one reducer slot per timer id; stale tokens are
  rejected instead of becoming a second owner.
- `CoreState::{enter_observing_request, activate_observation,
  replace_active_observation_with_pending, enter_ready, complete_active_observation,
  restore_retained_observation_to_ready, enter_planning, enter_applying,
  take_pending_proposal, restore_retained_observation}` are the protocol
  construction boundaries that reject cross-phase observation or proposal
  payload injection instead of persisting an invalid workflow shape.

## Semantic Comparison Surface

The cache-free equality surfaces used by runtime and reducer tests live next to
the owning types:

- `RuntimeState::semantic_view()` compares authoritative runtime state while
  ignoring purgeable scratch buffers and rebuildable particle/config caches.
- `ProjectionHandle::semantic_view()` compares retained projection witness plus
  logical raster while ignoring reuse-key and cached realization drift.
- `InFlightProposal::semantic_view()` compares authoritative proposal payload
  through semantic patch-basis views rather than cached projection internals.
- `CoreState::semantic_view()` compares authoritative reducer state across
  protocol, runtime, scene, and realization owners while ignoring runtime
  scratch buffers, projection reuse caches, and cached shell materialization.

Current invariants pin the ownership model:

- inactive runtime phases must not retain animation tick bookkeeping
- settling requires tracked-cursor ownership and an ordered settling window
- runtime target cell and retarget-surface facts must stay derived from the retained target position and tracked cursor
- ready background probe batches must match the observation basis viewport
- retained projection shell materialization must match the retained logical raster
- protocol phase variants recurse only into the phase-legal observation owner
- exact observation samples must refresh `latest_exact_cursor_cell`, while deferred and unavailable samples preserve the retained exact-anchor cache
