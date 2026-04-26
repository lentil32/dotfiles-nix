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
- `resource`: effectful host handles or I/O handles that materialize reducer
  output but do not define it
- `queue`: deferred shell-edge work or retry coalescing after reducer output has
  already been chosen
- `context`: access guards or diagnostic configuration around the engine, not
  semantic state

## Top-Level Split

Runtime-owned state now starts at `RuntimeCell`, the event-layer thread-local
root. It owns separate lanes for reducer-semantic state, shell-local state,
host-timer bridging, shell capability caches, draw resources, event-loop
telemetry, palette resources, scheduled dispatch work, and diagnostic
configuration:

- `RuntimeCell.reducer` owns strict access to `ReducerState`, which contains
  only `CoreState`. Event callers enter this lane through core-specific
  ports (`with_core_read`, lifecycle-specific runtime commands such as
  `sync_core_runtime_to_current_cursor`, `apply_core_setup_options`, and
  `toggle_core_runtime`, plus `with_core_transition`) instead of whole-engine
  read or mutation APIs.
- `RuntimeCell.shell` owns strict access to `ShellState`: purgeable caches,
  reusable scratch buffers, host capability snapshots, and shell telemetry.
  Callers cross this lane through cache-specific ports such as
  `with_editor_viewport_cache`, `with_probe_cache`,
  `with_buffer_perf_telemetry_cache`, and `try_record_telemetry`; broad
  `ShellState` access remains the private transaction primitive behind those
  ports.
- `RuntimeCell.timer_bridge` owns strict access to `TimerBridge`: host callback
  id allocation, host timer cancellation witnesses, and timer-dispatch retry
  coalescing. Timer runtime code mutates this lane through `with_timer_bridge`
  and starts or stops Neovim timers through `HostBridgePort`.
- `RuntimeCell.host_capabilities` owns cheap shell capability caches that can be
  probed and updated without entering reducer or shell-state borrows, including
  the cached `nvim__redraw` availability used by draw flushing. Redraw probing,
  flush execution, and fallback `redraw!` command execution cross the host
  boundary through `RedrawCommandPort`.
- `RuntimeCell.dispatch_queue` owns deferred reducer events and effect batches
  that must move to a later shell edge after reducer output has already been
  chosen. Only shell-only work may be coalesced inside this lane; callers enter
  it through `with_dispatch_queue`.
- `RuntimeCell.draw_resources` owns live and reusable Neovim draw resources:
  render-tab window pools and prepaint overlays. Draw callers detach these maps
  before mutation and restore them after shell work completes. Window and buffer
  creation, option writes, namespace clears, extmark writes, and orphan-resource
  scans cross the Neovim boundary through `DrawResourcePort`.
- `RuntimeCell.palette` owns highlight-palette cache state, highlight-group-name
  reuse, and deferred palette refresh coalescing. Palette callers detach this
  state before mutation and restore it after the cache-local transition
  completes. Palette host reads and highlight writes cross the Neovim boundary
  through `HighlightPalettePort`, with foreground/background color selection
  modeled by `HighlightColorField` instead of positional booleans.
- `RuntimeCell.telemetry` owns best-effort access to `EventLoopState`: advisory
  runtime metrics, callback-duration estimates, and last-observed event
  timestamps. Contended writes may be dropped and never drive reducer truth.
- `RuntimeCell.diagnostics` owns typed diagnostic log-level configuration and the
  best-effort log-file handle. Host notification and error output are emitted
  through `HostLoggingPort`.

Runtime recovery is centralized through `RuntimeRecoveryPlan`. Transient resets
and runtime-lane panic recovery both execute named action lists instead of
assembling ad hoc reset sequences at each failure site. Panic recovery runs in
this order: restore diagnostic logging, report the panic, recover draw
resources from the captured namespace witness, stop recovered host timers, reset
the recovered timer bridge and pending retry queue, reset the scheduled dispatch
queue, reset recovered shell state, clear advisory telemetry timestamps, reset
the palette lane to a captured recovery epoch, and finally reset reducer core state.
The palette epoch is captured when the plan is built, so applying the same plan
again converges to the same runtime state.

Shell state may accelerate reads, retain host-resource witnesses, or avoid
redundant host calls, but it does not own reducer semantics.

At the root boundary:

- `CoreState`, behind `RuntimeCell.reducer`, is the only authoritative top-level
  reducer subtree.
- `ShellState`, behind `RuntimeCell.shell`, is restricted to cache, snapshot,
  resource-witness, and telemetry roles listed in this document; purging it may
  change cost or reuse, but not semantic output for the same external input
  sequence.
- `TimerBridge`, behind `RuntimeCell.timer_bridge`, owns host-side timer
  callback allocation, cancellation witnesses, and retry single-flight state.
  It does not own reducer timer liveness or generations.
- Shell-edge callsites use explicit boundary enums for reducer-wave probe
  dispatch, viewport capture, and cursor-color extmark fallback instead of
  positional booleans.
- Ingress cursor prepaint requests cross the shell/reducer boundary with
  `IngressCursorModeAdmission` and `IngressCursorCommandLineLocation`, and draw
  floating-window helpers use `FloatingWindowVisibility` instead of hidden
  positional booleans.
- Runtime-level holders are restricted to the resource, cache, queue,
  telemetry, and context roles listed in
  [External Shell Holders](#external-shell-holders). They may change shell cost,
  host-resource reuse, diagnostic counters, or callback coalescing, but they
  must not make reducer lifecycle decisions.

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
- `AnimationSchedule::{Idle, DefaultDelay, Deadline}` is the single transition
  and proposal representation for animation-timer intent. Boolean scheduling
  flags and optional deadlines are derived from it only for tests or diagnostics.

## Field Role Quick Map

This storage-role index classifies the main reducer and shell roots by field.
It does not assign new owners; it only labels how the stored fields are used.

- `RuntimeCell`
  - context: `reducer`, `shell`, `timer_bridge`, `diagnostics.log_level`
  - cache: `host_capabilities.flush_redraw_capability`, `palette`
  - resource: `draw_resources`, `diagnostics.log_file_handle`
  - telemetry: `telemetry`
- `ReducerState`: `core_state` is the authoritative reducer root.
- `ShellState`
  - snapshot: `namespace_id`, `host_bridge_state`, `editor_viewport_cache`,
    `buffer_metadata_cache`, `real_cursor_visibility`
  - cache: `probe_cache`, `background_probe_request_scratch`,
    `conceal_regions_scratch`, `buffer_text_revision_cache`,
    `buffer_perf_policy_cache`
  - telemetry: `buffer_perf_telemetry_cache`
- `TimerBridge`
  - resource: `handles`, `next_host_callback_id`
  - queue: `pending_retries`
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
  `cursor` is projected display-space cursor truth; raw host probe details such
  as `screenpos()`, conceal facts, and cached deltas stay in event-layer
  readers and diagnostics.
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
- External thread-local shell holders
  - context: `RUNTIME_CELL` stores the reducer-state lane, shell-state lane,
    timer-bridge lane, host-capability lane, draw-resource lane, palette lane,
    scheduled dispatch-queue lane, event-loop telemetry lane, and log-level
    diagnostics lane; the reducer-state lane guards reducer state, the
    shell-state lane guards shell cache/resource witnesses, the timer-bridge
    lane guards host-timer witnesses and retry coalescing, the host-capability
    lane guards cheap host capability caches, the draw-resource lane guards live
    draw resource handles, the palette lane guards highlight cache resources,
    the dispatch-queue lane guards deferred reducer events and effect batches,
    telemetry owns advisory metrics, and diagnostics owns log verbosity plus the
    best-effort diagnostic file handle.

## Core And Protocol Facts

| Fact | Class | Owner / derivation |
| --- | --- | --- |
| Lifecycle freshness | authoritative | `CoreState.generation` owns reducer freshness for cache invalidation and effect staleness. |
| Queued ingress demands | authoritative | `ProtocolSharedState.demand` owns at most one pending `ExternalDemand` per `ExternalDemandKind`; same-kind ingress coalesces in place while dequeue order is still derived from the occupied demand sequences. |
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
| Animation scheduling intent | authoritative | `AnimationSchedule` owns whether the next animation timer is idle, uses the default delay, or targets a concrete deadline. `CursorTransition.animation_schedule`, `RequestRenderPlanEffect.animation_schedule`, and `InFlightProposal.animation_schedule` carry that same enum across reducer boundaries instead of splitting the fact into a boolean plus optional deadline. |
| Proposal and ingress sequence allocation | authoritative | `CoreStatePayload.entropy` owns proposal id and ingress sequence allocation. |
| Latest exact cursor fallback anchor | authoritative | `CoreStatePayload.latest_exact_cursor_cell` owns the last exact cursor cell reused when a later observation lacks one. |

## Observation Facts

| Fact | Class | Owner / derivation |
| --- | --- | --- |
| Observation identity | authoritative | Identity is derived only from the observation root demand sequence: `PendingObservation.demand.seq()` while collecting and `ObservationSnapshot.demand.seq()` once active. `ObservationId` accessors compute from that root; neither the snapshot nor active probe lifecycle state stores a mirrored current-observation id. |
| Pending ingress demand | snapshot | `PendingObservation.demand` retains the ingress request while basis collection is in flight. |
| Pending requested probe policy | authoritative | `PendingObservation.requested_probes` is the only owner of probe policy before activation. `ObservationSnapshot::new()` consumes it to initialize active probe lifecycle state. The policy chooses freshness, reuse, and fallback cost only; it does not choose between raw and projected cursor coordinate systems. |
| Active ingress demand | snapshot | `ObservationSnapshot.demand` retains the ingress request that produced the active observation. |
| Active observation basis | authoritative | `ObservationSnapshot.basis` owns `observed_at`, `mode`, `surface`, `cursor`, `viewport`, `buffer_revision`, and `cursor_text_context_state`. `ObservationBasis.cursor` is the sole reducer-owned owner of projected display-space cursor truth; raw host probe details remain event-layer parsing and diagnostic state. |
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
| Animation tick bookkeeping | authoritative | `RunningPhase.clock` and `DrainingPhase.clock` own `last_tick_ms`, `next_frame_at_ms`, and `simulation_accumulator_ms` while those phases are active. `RuntimeState::take_animation_clock_sample()` classifies oversized or invalid wall-clock gaps as motion-clock discontinuities, and the allowable catch-up budget is derived from `RuntimeState.config` instead of being copied into the clock. |
| Current simulated cursor geometry | authoritative | `RuntimeState.current_corners` owns the live simulated cursor corners. |
| Cursor target identity | authoritative | `RuntimeState.target` owns target `position`, `shape`, `tracked_cursor`, and `retarget_epoch`. `CursorTarget::retarget_key()` derives the reviewable equality surface, including the discrete target cell and retarget surface, so those derived facts are not stored separately. Target corners are derived on demand by `CursorTarget::corners()`. |
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
| Render namespace handle | snapshot | `ShellState.namespace_id` retains the typed `NamespaceId` draw namespace handle created by `ensure_namespace_id()`. It is a shell capability witness, not a realization or reducer truth owner, and transient cache resets intentionally preserve it. The raw Neovim integer is unwrapped only at host API calls. |
| Buffer handle witness | snapshot | `BufferHandle` wraps Neovim buffer ids retained in `SurfaceId`, cursor-color and cursor-text-context witnesses, shell buffer-local caches, telemetry, and draw resource handles. Host reads convert `api::Buffer::handle()` at the boundary; code unwraps the raw integer only when calling Neovim APIs or parsing raw host payloads. |
| Current editor host snapshot | snapshot | Current mode, current window, current buffer, and current-handle validity checks used by lifecycle and ingress observation code cross through `CurrentEditorPort`. `FakeCurrentEditorPort` covers current-editor capture paths without live Neovim mode, current-window, current-buffer, or validity reads. |
| Window surface host snapshot | snapshot | `WindowSurfaceSnapshot` is parsed by `src/events/surface.rs` from `getwininfo` and window-buffer host reads that cross through `WindowSurfacePort`. `FakeWindowSurfacePort` covers parsing and scroll-distance behavior without live Neovim `getwininfo`, window-buffer, or text-height reads. Once accepted into `ObservationBasis`, the surface becomes reducer-owned observation truth; raw host dictionaries stay event-layer input. |
| Cursor read host snapshot | snapshot | Cursor observation reads for window cursor position, `screenpos()`, command-line cursor position, conceal probes, and cursor text-context rows cross through `CursorReadPort`. `FakeCursorReadPort` covers cursor projection, conceal, and text-context paths without live Neovim cursor, `screenpos()`, `synconcealed()`, `strdisplaywidth()`, `getcmd*`, or buffer-line reads. |
| Tab handle witness | snapshot | `TabHandle` wraps Neovim tabpage ids used to key render-tab window pools, prepaint overlays, and TabClosed cleanup. Host reads convert `api::TabPage::handle()` at the boundary; draw-resource maps and stale-tab pruning compare typed witnesses instead of raw integers. |
| Verified host bridge capability | snapshot | `ShellState.host_bridge_state` retains whether the current host bridge revision has been verified by `verify_host_bridge()`. It is an external capability witness, not a semantic owner. |
| Outstanding host timer ids | resource | `RuntimeCell.timer_bridge` retains the currently armed Neovim host timer ids keyed by reducer `TimerId` inside `TimerBridge`. The reducer token remains authoritative for timer liveness and generation; host timer ids are cancellation witnesses only. `schedule_core_timer_effect()`, `dispatch_core_timer_fired()`, `reset_core_timer_bridge()`, and panic recovery are the ownership transitions. |
| Shell probe reuse state | cache | `ShellState.probe_cache` owns purgeable cursor-color, cursor-text-context, conceal-region, conceal-delta, and conceal-screen-cell reuse keyed by external witnesses such as `CursorColorProbeWitness`, buffer-local text revisions, and window state. `note_cursor_color_observation_boundary()`, `note_cursor_color_colorscheme_change()`, `note_conceal_read_boundary()`, `invalidate_buffer_local_probe_caches()`, and `reset_transient_caches()` are its invalidation and purge paths. |
| Probe request and conceal scratch buffers | cache | `ShellState.background_probe_request_scratch` and `ShellState.conceal_regions_scratch` retain reusable allocations for effect construction only. `reset_transient_caches()` drops the retained allocations, while `reclaim_background_probe_request_scratch()` and `reclaim_conceal_regions_scratch()` only recycle them between operations. |
| Editor viewport witness | snapshot | `ShellState.editor_viewport_cache` retains the last live `EditorViewportSnapshot` read through `EditorViewportPort`. `EditorViewportSnapshot` is also the canonical shell-side owner of command-row math and `ViewportBounds` projection through `command_row()` and `bounds()`. `refresh_editor_viewport_cache()`, `invalidate_editor_viewport_cache()`, and `reset_transient_caches()` refresh or purge the witness without changing semantic state. Tests use `FakeEditorViewportPort` to exercise the shell cache without live Neovim option reads. |
| Buffer metadata witness | snapshot | `ShellState.buffer_metadata_cache` retains `BufferMetadata` read from Neovim so policy and probe code can reuse it without rereading host options on every ingress. Cache misses cross the host boundary through `BufferMetadataPort`, with `FakeBufferMetadataPort` covering cache behavior without live Neovim buffer option or line-count reads. `invalidate_buffer_metadata()`, `invalidate_buffer_local_caches()`, and `reset_transient_caches()` clear it. |
| Buffer text revision cache | cache | `ShellState.buffer_text_revision_cache` retains shell-local generations used to partition buffer-local probe reuse and invalidation. `invalidate_buffer_local_caches()` and `reset_transient_caches()` clear it; it never becomes reducer truth. |
| Buffer performance policy cache | cache | `ShellState.buffer_perf_policy_cache` caches `BufferEventPolicy` derived from the current ingress snapshot, `BufferMetadata`, and buffer-local telemetry. It is invalidated per buffer or by `reset_transient_caches()` and does not own `BufferPerfClass` independently of those inputs. |
| Buffer performance telemetry | telemetry | `ShellState.buffer_perf_telemetry_cache` records callback EWMA and probe-pressure signals used to explain or derive future buffer performance policy. It does not own the selected `BufferPerfClass`, and `invalidate_buffer_local_caches()` plus `reset_transient_caches()` purge it. |
| Real cursor visibility witness | snapshot | `ShellState.real_cursor_visibility` retains the last shell cursor visibility applied so host calls can skip redundant hide/show work. `note_cursor_color_colorscheme_change()`, `invalidate_real_cursor_visibility()`, and `reset_transient_caches()` clear it. |

## External Shell Holders

These thread-local holders live outside the reducer lane by design. They are not
exceptions to single-source-of-truth ownership because they do not own reducer
facts, phase transitions, requested probe state, runtime motion state, or
realization trust. They exist at effectful boundaries where borrowing reducer
state would either re-enter reducer execution during shell work or make
best-effort diagnostics capable of perturbing semantic execution.

| Holder | Class | Owner / boundary |
| --- | --- | --- |
| Runtime cell | context | `RUNTIME_CELL` is the runtime-layer thread-local root for runtime lanes. Its current lanes are strict reducer-state access, strict shell-state access, strict timer-bridge access, host-capability caches, draw resources, palette resources, strict dispatch-queue access, best-effort event-loop telemetry, and diagnostics; future shell-resource and cache migrations attach here instead of adding new roots. |
| Reducer state lane | context | `RuntimeCell.reducer` owns access to the single `ReducerState` through `ReducerStateSlot::{Ready, InUse}`. This is an exclusive-borrow guard around reducer state, not a second semantic owner. `take_reducer_state()` and `restore_reducer_state()` are private slot transitions behind `with_core_read`, lifecycle-specific runtime commands, and `with_core_transition`. |
| Shell state lane | context | `RuntimeCell.shell` owns access to the single `ShellState` through `ShellStateSlot::{Ready, InUse}`. This is an exclusive-borrow guard around shell caches, scratch buffers, host capability snapshots, and shell telemetry. `take_shell_state()` and `restore_shell_state()` are the only state-slot transitions. |
| Timer bridge lane | resource / queue | `RuntimeCell.timer_bridge` owns access to `TimerBridge` through `TimerBridgeSlot::{Ready, InUse}`. `TimerBridge` owns host callback id allocation, host timer cancellation witnesses, and the single-flight retry set for fired host timer callbacks that must be rescheduled after timer-bridge re-entry. Reducer timer liveness and generations remain in `ProtocolSharedState.timers`; bridge retries only decide whether a duplicate scheduled callback is redundant. |
| Host-capability lane | cache | `RuntimeCell.host_capabilities` owns shell capability caches that need no reducer or shell-state borrow. `FlushRedrawCapability` caches whether the current host exposes `nvim__redraw`, is refreshed by `refresh_redraw_capability()`, and is downgraded after an API failure. It chooses between equivalent shell flush paths and does not own visual state. |
| Draw-resource lane | resource | `RuntimeCell.draw_resources` owns live and reusable Neovim draw resources: render-tab window pools and prepaint overlays. The reducer owns desired realization state; draw resources own shell handles used to materialize, reuse, and clean that desire. `take_draw_render_tabs()`, `restore_draw_render_tabs()`, `take_draw_prepaint_by_tab()`, `restore_draw_prepaint_by_tab()`, and their draw-facing wrappers are the detach-mutate-restore ownership boundaries. Draw resource creation, option writes, namespace clears, extmark writes, and orphan-resource scans cross the host boundary through `DrawResourcePort`. |
| Highlight palette lane | cache | `RuntimeCell.palette` owns applied highlight-palette cache state, highlight-group-name reuse, and the single-flight deferred palette refresh slot. Palette inputs come from `PaletteSpec` and runtime config; palette lane state only avoids redundant host highlight writes and coalesces palette churn. `with_runtime_palette_lane()`, `clear_highlight_cache()`, `ensure_highlight_palette_for_spec()`, and deferred refresh draining are its mutation boundaries. |
| Scheduled dispatch-queue lane | queue | `RuntimeCell.dispatch_queue` owns shell-edge backlog for deferred reducer events and effect batches after the reducer has emitted them. It may coalesce adjacent shell-only metric and redraw work, but reducer-significant work remains ordered as queued `CoreEvent` or ordered effect batches. `ScheduledEffectQueueState::{stage_batch, stage_core_event, pop_work_unit, reset}`, `with_dispatch_queue()`, scheduled drain, and reset-after-failure are its ownership boundaries. |
| Event-loop telemetry lane | telemetry | `RuntimeCell.telemetry` owns advisory runtime metrics, EWMA callback duration, and last-observed event timestamps. Recording can be dropped under a nested borrow, so this state is intentionally non-semantic and must not gate reducer transitions. Diagnostics read it through `event_loop_diagnostics()`. |
| Diagnostics lane | context / resource | `RuntimeCell.diagnostics` owns the `LogLevel` verbosity threshold and the best-effort buffered diagnostics sink selected by `SMEAR_CURSOR_LOG_FILE`. It can change which messages are emitted or persisted, but host notification and error output go through `HostLoggingPort` and cannot change reducer events, effects, or state transitions. Nested logging may drop a file line instead of panicking. |

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
- `apply_runtime_options()` validates and normalizes `color_levels` before
  `RuntimeState.config` is mutated, so palette quantization stays bounded at
  the config boundary instead of inflating draw-time highlight state.
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

The cache-free equality surfaces used by runtime and projection tests live next
to the owning types:

- `RuntimeState::semantic_view()` compares authoritative runtime state while
  ignoring purgeable scratch buffers and rebuildable particle/config caches.
- `ProjectionHandle::semantic_view()` compares retained projection witness plus
  logical raster while ignoring reuse-key and cached realization drift.

Current invariants pin the ownership model:

- inactive runtime phases must not retain animation tick bookkeeping
- settling requires tracked-cursor ownership and an ordered settling window
- runtime target equality keys must stay derived from the retained target position, shape, and tracked cursor; discrete target cell and retarget surface are not stored owners
- ready background probe batches must match the observation basis viewport
- retained projection shell materialization must match the retained logical raster
- protocol phase variants recurse only into the phase-legal observation owner
- exact observation samples must refresh `latest_exact_cursor_cell`, while deferred and unavailable samples preserve the retained exact-anchor cache
- observation-owned cursor cells stay in projected display space; raw host quirks such as conceal and `screenpos()` remain event-layer concerns
- requested probe policy may allow deferred projection, but it may not switch reducer-owned cursor truth out of projected display space
