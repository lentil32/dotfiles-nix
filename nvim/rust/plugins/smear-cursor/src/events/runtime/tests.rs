use super::recovery::RuntimeRecoveryAction;
use super::recovery::RuntimeRecoveryPlan;
use super::recovery::start_runtime_recovery_action_log_for_test;
use super::recovery::take_runtime_recovery_action_log_for_test;
use super::*;
use crate::config::BufferPerfMode;
use crate::core::state::BufferPerfClass;
use crate::core::state::CursorColorProbeWitness;
use crate::core::state::CursorColorSample;
use crate::core::state::CursorTextContext;
use crate::core::state::ExternalDemand;
use crate::core::state::ExternalDemandKind;
use crate::core::state::ObservationBasis;
use crate::core::state::ObservationMotion;
use crate::core::state::ObservationSnapshot;
use crate::core::state::ObservedTextRow;
use crate::core::state::PendingObservation;
use crate::core::state::ProbeKind;
use crate::core::state::ProbeRequestSet;
use crate::core::state::ProbeReuse;
use crate::core::state::ProbeState;
use crate::core::types::Generation;
use crate::core::types::IngressSeq;
use crate::core::types::Millis;
use crate::core::types::TimerGeneration;
use crate::core::types::TimerId;
use crate::events::RealCursorVisibility;
use crate::events::cursor::BufferMetadata;
#[cfg(feature = "perf-counters")]
use crate::events::ingress::AutocmdIngress;
use crate::events::policy::BufferEventPolicy;
use crate::events::policy::BufferPerfTelemetry;
use crate::events::timer_protocol::FiredHostTimer;
use crate::events::timer_protocol::HostCallbackId;
use crate::events::timer_protocol::HostTimerId;
use crate::host::NamespaceId;
use crate::host::api;
use crate::position::BufferLine;
use crate::position::CursorObservation;
use crate::position::ObservedCell;
use crate::position::SurfaceId;
use crate::position::ViewportBounds;
use crate::position::WindowSurfaceSnapshot;
use crate::test_support::cursor;
use insta::assert_snapshot;
use nvim_oxi::Object;
use pretty_assertions::assert_eq;

#[path = "tests/telemetry_non_interference.rs"]
mod telemetry_non_interference;
#[path = "tests/timer_retry_linearizability.rs"]
mod timer_retry_linearizability;

fn host_callback_id(value: i64) -> HostCallbackId {
    HostCallbackId::try_new(value).expect("test host callback id must be positive")
}

fn host_timer_id(value: i64) -> HostTimerId {
    HostTimerId::try_new(value).expect("test host timer id must be positive")
}

fn handle(value: i64, timer_id: TimerId, generation: u64) -> CoreTimerHandle {
    CoreTimerHandle {
        host_callback_id: host_callback_id(value),
        host_timer_id: host_timer_id(value),
        token: crate::core::types::TimerToken::new(timer_id, TimerGeneration::new(generation)),
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
struct ShellScratchStorageResidency {
    background_probe_request_retained: bool,
    conceal_regions_retained: bool,
}

impl ShellScratchStorageResidency {
    const RETAINED: Self = Self {
        background_probe_request_retained: true,
        conceal_regions_retained: true,
    };

    const RELEASED: Self = Self {
        background_probe_request_retained: false,
        conceal_regions_retained: false,
    };
}

fn shell_scratch_storage_residency() -> RuntimeAccessResult<ShellScratchStorageResidency> {
    read_shell_state(|state| ShellScratchStorageResidency {
        background_probe_request_retained: state.background_probe_request_scratch_capacity() > 0,
        conceal_regions_retained: state.conceal_regions_scratch_capacity() > 0,
    })
}

#[derive(Debug, Clone, PartialEq)]
struct RuntimeRecoverySnapshot {
    core_state: crate::core::state::CoreState,
    real_cursor_visibility: Option<RealCursorVisibility>,
    viewport: Option<EditorViewportSnapshot>,
    outstanding_animation_timer: bool,
    outstanding_ingress_timer: bool,
    pending_timer_retries: usize,
    queued_work_units: usize,
    queued_items: usize,
    drain_scheduled: bool,
    palette_epoch: u64,
}

fn runtime_recovery_snapshot() -> RuntimeAccessResult<RuntimeRecoverySnapshot> {
    let (real_cursor_visibility, viewport) = read_shell_state(|state| {
        (
            state.real_cursor_visibility(),
            state.editor_viewport_cache.cached_for_test(),
        )
    })?;
    let (outstanding_animation_timer, outstanding_ingress_timer, pending_timer_retries) =
        super::timers::mutate_timer_bridge_for_test(|bridge| {
            (
                bridge.has_timer_id(TimerId::Animation),
                bridge.has_timer_id(TimerId::Ingress),
                bridge.pending_retry_len(),
            )
        })?;
    let (queued_work_units, queued_items, drain_scheduled) = with_dispatch_queue(|queue| {
        (
            queue.pending_work_units,
            queue.items.len(),
            queue.drain_scheduled,
        )
    });

    Ok(RuntimeRecoverySnapshot {
        core_state: core_state()?,
        real_cursor_visibility,
        viewport,
        outstanding_animation_timer,
        outstanding_ingress_timer,
        pending_timer_retries,
        queued_work_units,
        queued_items,
        drain_scheduled,
        palette_epoch: crate::draw::palette_epoch_for_test(),
    })
}

fn prime_shell_scratch_storage() {
    let mut background_scratch =
        take_background_probe_request_scratch().expect("background scratch should be available");
    background_scratch.reserve(32);
    background_scratch.push(Object::from(7_i64));
    reclaim_background_probe_request_scratch(background_scratch)
        .expect("background scratch should be reclaimable");

    let mut conceal_scratch =
        take_conceal_regions_scratch().expect("conceal region scratch should be available");
    conceal_scratch.reserve(32);
    conceal_scratch.push(crate::test_support::conceal_region(3, 4, 11, 1));
    reclaim_conceal_regions_scratch(conceal_scratch)
        .expect("conceal region scratch should be reclaimable");
}

#[test]
fn colorscheme_boundary_clears_only_color_dependent_shell_caches() {
    let (metadata, policy, context_key, context) = prime_shell_boundary_state();
    let cleared_color_witness = shell_cache_color_witness(Generation::new(1), Generation::new(1));

    note_cursor_color_colorscheme_change().expect("colorscheme boundary should succeed");

    let (
        real_cursor_visibility,
        viewport,
        cached_metadata,
        cached_policy,
        cached_telemetry,
        text_revision,
        colorscheme_generation,
        cache_generation,
        cached_context,
        cached_color,
    ) = mutate_shell_state(|state| {
        (
            state.real_cursor_visibility(),
            state.editor_viewport_cache.cached_for_test(),
            state
                .buffer_metadata_cache
                .cached_entry_for_test(SHELL_CACHE_TEST_BUFFER_HANDLE),
            state
                .buffer_perf_policy_cache
                .cached_policy(SHELL_CACHE_TEST_BUFFER_HANDLE),
            state
                .buffer_perf_telemetry_cache
                .telemetry(SHELL_CACHE_TEST_BUFFER_HANDLE),
            state
                .buffer_text_revision_cache
                .cached_entry_for_test(SHELL_CACHE_TEST_BUFFER_HANDLE),
            state.probe_cache.colorscheme_generation(),
            state.probe_cache.cursor_color_cache_generation(),
            state.probe_cache.cached_cursor_text_context(&context_key),
            state
                .probe_cache
                .cached_cursor_color_sample(&cleared_color_witness),
        )
    })
    .expect("shell state access should succeed");

    assert_eq!(real_cursor_visibility, None);
    assert_eq!(
        viewport,
        Some(EditorViewportSnapshot::from_dimensions(24, 1, 80))
    );
    assert_eq!(cached_metadata, Some(metadata));
    assert_eq!(cached_policy, Some(policy));
    assert_eq!(
        cached_telemetry.map(BufferPerfTelemetry::callback_duration_estimate_ms),
        Some(SHELL_CACHE_TEST_CALLBACK_DURATION_MS)
    );
    assert_eq!(text_revision, Some(Generation::new(1)));
    assert_eq!(colorscheme_generation, Generation::new(1));
    assert_eq!(cache_generation, Generation::new(1));
    assert_eq!(
        cached_context,
        super::super::probe_cache::CursorTextContextCacheLookup::Hit(Some(context))
    );
    assert_eq!(
        cached_color,
        super::super::probe_cache::CursorColorCacheLookup::Miss
    );
}

#[test]
fn transient_reset_purges_shell_caches_and_timer_bridge_but_preserves_host_bridge_verification() {
    let context_key = prime_shell_boundary_state().2;
    let cleared_color_witness = shell_cache_color_witness(Generation::INITIAL, Generation::INITIAL);
    let animation_handle = handle(11, TimerId::Animation, 3);
    let ingress_handle = handle(21, TimerId::Ingress, 8);

    mutate_shell_state(|state| {
        state.set_namespace_id(NamespaceId::new(/*value*/ 77));
        state.note_host_bridge_verified(super::super::HostBridgeRevision::CURRENT);
    })
    .expect("shell state access should succeed");
    super::timers::mutate_timer_bridge_for_test(|bridge| {
        assert_eq!(bridge.replace_handle(animation_handle), None);
        assert_eq!(bridge.replace_handle(ingress_handle), None);
    })
    .expect("timer bridge access should succeed");

    reset_transient_event_state();

    let (
        namespace_id,
        host_bridge_state,
        real_cursor_visibility,
        viewport,
        outstanding_animation_timer,
        outstanding_ingress_timer,
        cached_metadata,
        cached_policy,
        cached_telemetry,
        text_revision,
        colorscheme_generation,
        cache_generation,
        cached_context,
        cached_color,
    ) = {
        let shell_state = mutate_shell_state(|state| {
            (
                state.namespace_id(),
                state.host_bridge_state(),
                state.real_cursor_visibility(),
                state.editor_viewport_cache.cached_for_test(),
                state
                    .buffer_metadata_cache
                    .cached_entry_for_test(SHELL_CACHE_TEST_BUFFER_HANDLE),
                state
                    .buffer_perf_policy_cache
                    .cached_policy(SHELL_CACHE_TEST_BUFFER_HANDLE),
                state
                    .buffer_perf_telemetry_cache
                    .telemetry(SHELL_CACHE_TEST_BUFFER_HANDLE),
                state
                    .buffer_text_revision_cache
                    .cached_entry_for_test(SHELL_CACHE_TEST_BUFFER_HANDLE),
                state.probe_cache.colorscheme_generation(),
                state.probe_cache.cursor_color_cache_generation(),
                state.probe_cache.cached_cursor_text_context(&context_key),
                state
                    .probe_cache
                    .cached_cursor_color_sample(&cleared_color_witness),
            )
        })
        .expect("shell state access should succeed");
        let timer_bridge_state = super::timers::mutate_timer_bridge_for_test(|bridge| {
            (
                bridge.has_timer_id(TimerId::Animation),
                bridge.has_timer_id(TimerId::Ingress),
            )
        })
        .expect("timer bridge access should succeed");
        (
            shell_state.0,
            shell_state.1,
            shell_state.2,
            shell_state.3,
            timer_bridge_state.0,
            timer_bridge_state.1,
            shell_state.4,
            shell_state.5,
            shell_state.6,
            shell_state.7,
            shell_state.8,
            shell_state.9,
            shell_state.10,
            shell_state.11,
        )
    };

    assert_eq!(namespace_id, Some(NamespaceId::new(/*value*/ 77)));
    assert_eq!(
        host_bridge_state,
        super::super::HostBridgeState::Verified {
            revision: super::super::HostBridgeRevision::CURRENT,
        }
    );
    assert_eq!(real_cursor_visibility, None);
    assert_eq!(viewport, None);
    assert!(!outstanding_animation_timer);
    assert!(!outstanding_ingress_timer);
    assert_eq!(cached_metadata, None);
    assert_eq!(cached_policy, None);
    assert_eq!(cached_telemetry, None);
    assert_eq!(text_revision, None);
    assert_eq!(colorscheme_generation, Generation::INITIAL);
    assert_eq!(cache_generation, Generation::INITIAL);
    assert_eq!(
        cached_context,
        super::super::probe_cache::CursorTextContextCacheLookup::Miss
    );
    assert_eq!(
        cached_color,
        super::super::probe_cache::CursorColorCacheLookup::Miss
    );
}

#[test]
fn reducer_panic_path_applies_runtime_recovery_plan_in_documented_order() {
    reset_transient_event_state();
    start_runtime_recovery_action_log_for_test();

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _: RuntimeAccessResult<()> = with_core_transition(|state| {
            let _ = state.into_primed();
            panic!("forced reducer failure after state mutation");
        });
    }));

    assert!(result.is_err());
    assert_eq!(
        take_runtime_recovery_action_log_for_test(),
        vec![
            RuntimeRecoveryAction::RestoreDefaultLogLevel,
            RuntimeRecoveryAction::EmitPanicRecoveryWarning,
            RuntimeRecoveryAction::RecoverDrawResources,
            RuntimeRecoveryAction::StopRecoveredCoreTimerHandles,
            RuntimeRecoveryAction::ClearRecoveredTimerBridge,
            RuntimeRecoveryAction::ResetDispatchQueue,
            RuntimeRecoveryAction::ResetRecoveredShellState,
            RuntimeRecoveryAction::ClearTelemetryTimestamps,
            RuntimeRecoveryAction::RecoverPaletteEpoch,
            RuntimeRecoveryAction::ResetCoreState,
        ]
    );
}

#[test]
fn runtime_lane_panic_recovery_plan_is_idempotent_for_runtime_state() {
    reset_transient_event_state();
    crate::draw::recover_palette_to_epoch(/*epoch*/ 41);
    set_core_state(crate::core::state::CoreState::default().into_primed())
        .expect("core state write should succeed");
    mutate_shell_state(|state| {
        state.note_real_cursor_visibility(RealCursorVisibility::Visible);
        state
            .editor_viewport_cache
            .store_for_test(EditorViewportSnapshot::from_dimensions(
                /*lines*/ 24, /*cmdheight*/ 1, /*columns*/ 80,
            ));
    })
    .expect("shell state access should succeed");
    super::timers::mutate_timer_bridge_for_test(|bridge| {
        assert_eq!(
            bridge.replace_handle(handle(
                /*value*/ 11,
                TimerId::Animation,
                /*generation*/ 3,
            )),
            None
        );
        assert_eq!(
            bridge.replace_handle(handle(
                /*value*/ 21,
                TimerId::Ingress,
                /*generation*/ 8,
            )),
            None
        );
        let _ = bridge.stage_retry(FiredHostTimer::new(
            host_callback_id(/*value*/ 31),
            host_timer_id(/*value*/ 32),
        ));
    })
    .expect("timer bridge access should succeed");
    with_dispatch_queue(|queue| {
        queue.pending_work_units = 3;
        queue.drain_scheduled = true;
    });

    let plan = RuntimeRecoveryPlan::runtime_lane_panic(
        super::shell::ShellRecoveryState::default(),
        super::timer_bridge::TimerBridgeRecoveryState {
            core_timer_handles: vec![handle(
                /*value*/ 41,
                TimerId::Animation,
                /*generation*/ 13,
            )],
        },
    );

    plan.apply();
    let once = runtime_recovery_snapshot().expect("recovery snapshot should be readable");
    plan.apply();
    let twice = runtime_recovery_snapshot().expect("recovery snapshot should be readable");

    assert_eq!(twice, once);
}

#[test]
fn runtime_recovery_runs_after_partial_shell_failure() {
    reset_transient_event_state();
    set_core_state(crate::core::state::CoreState::default())
        .expect("core state write should succeed");
    crate::draw::recover_palette_to_epoch(/*epoch*/ 9);

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = mutate_shell_state(|state| {
            state.note_real_cursor_visibility(RealCursorVisibility::Hidden);
            state
                .editor_viewport_cache
                .store_for_test(EditorViewportSnapshot::from_dimensions(
                    /*lines*/ 24, /*cmdheight*/ 1, /*columns*/ 80,
                ));
            panic!("forced shell failure");
        });
    }));

    assert!(result.is_err());
    let snapshot = runtime_recovery_snapshot().expect("recovery snapshot should be readable");
    assert_eq!(
        snapshot,
        RuntimeRecoverySnapshot {
            core_state: crate::core::state::CoreState::default(),
            real_cursor_visibility: None,
            viewport: None,
            outstanding_animation_timer: false,
            outstanding_ingress_timer: false,
            pending_timer_retries: 0,
            queued_work_units: 0,
            queued_items: 0,
            drain_scheduled: false,
            palette_epoch: 10,
        }
    );
}

#[test]
fn transient_reset_drops_shell_scratch_buffer_capacity() {
    prime_shell_scratch_storage();

    reset_transient_event_state();

    let background_scratch =
        take_background_probe_request_scratch().expect("background scratch should be available");
    assert!(background_scratch.is_empty());
    assert_eq!(background_scratch.capacity(), 0);
    reclaim_background_probe_request_scratch(background_scratch)
        .expect("background scratch should be reclaimable");

    let conceal_scratch =
        take_conceal_regions_scratch().expect("conceal region scratch should be available");
    assert!(conceal_scratch.is_empty());
    assert_eq!(conceal_scratch.capacity(), 0);
    reclaim_conceal_regions_scratch(conceal_scratch)
        .expect("conceal region scratch should be reclaimable");
}

#[test]
fn cleanup_cold_storage_release_drops_shell_scratch_buffer_capacity() {
    prime_shell_scratch_storage();

    assert_eq!(
        shell_scratch_storage_residency().expect("runtime access should succeed"),
        ShellScratchStorageResidency::RETAINED
    );

    release_cleanup_cold_shell_storage().expect("cold cleanup storage release should succeed");

    assert_eq!(
        shell_scratch_storage_residency().expect("runtime access should succeed"),
        ShellScratchStorageResidency::RELEASED
    );
}

#[test]
fn perf_diagnostics_report_includes_recovery_fields_within_bridge_budget() {
    super::reset_event_loop_for_test();
    let report = test_perf_diagnostics_report();

    assert!(report.starts_with("smear_cursor "));
    assert!(
        report.len() < 1024,
        "perf diagnostics report exceeded bridge budget: {} bytes",
        report.len()
    );
    assert!(report.len() < 1000);
}

#[test]
fn perf_diagnostics_report_snapshot_renders_stable_field_order() {
    super::reset_event_loop_for_test();
    assert_snapshot!(test_perf_diagnostics_report());
}

#[test]
fn perf_diagnostics_report_uses_phase_owned_cursor_color_for_probe_policy() {
    let previous_core_state = core_state().expect("core state read should succeed");
    super::reset_event_loop_for_test();

    let pending = PendingObservation::new(
        ExternalDemand::new(
            IngressSeq::new(7),
            ExternalDemandKind::ExternalCursor,
            Millis::new(25),
            BufferPerfClass::Full,
        ),
        ProbeRequestSet::only(ProbeKind::CursorColor),
    );
    let basis = ObservationBasis::new(
        Millis::new(26),
        "n".to_string(),
        WindowSurfaceSnapshot::new(
            SurfaceId::new(11, SHELL_CACHE_TEST_BUFFER_HANDLE).expect("positive handles"),
            BufferLine::new(3).expect("positive top buffer line"),
            0,
            0,
            crate::position::ScreenCell::new(1, 1).expect("one-based window origin"),
            ViewportBounds::new(24, 80).expect("positive window size"),
        ),
        CursorObservation::new(
            BufferLine::new(5).expect("positive buffer line"),
            ObservedCell::Exact(cursor(3, 5)),
        ),
        ViewportBounds::new(24, 80).expect("positive viewport bounds"),
    )
    .with_buffer_revision(Some(17));
    let mut observation = ObservationSnapshot::new(pending, basis, ObservationMotion::new(None))
        .with_cursor_color_probe_generations(Some(
            crate::core::state::CursorColorProbeGenerations::new(
                Generation::INITIAL,
                Generation::INITIAL,
            ),
        ));
    assert!(
        observation
            .probes_mut()
            .set_cursor_color_state(ProbeState::ready(
                ProbeReuse::Compatible,
                Some(CursorColorSample::new(0x00AB_CDEF)),
            )),
        "cursor color probe state should accept a sampled fallback",
    );

    set_core_state(
        crate::core::state::CoreState::default()
            .into_primed()
            .with_ready_observation(observation)
            .expect("primed state should accept a ready observation"),
    )
    .expect("core state write should succeed");

    let report = test_perf_diagnostics_report();
    assert_snapshot!(report);

    set_core_state(previous_core_state).expect("core state restore should succeed");
}

#[cfg(feature = "perf-counters")]
#[test]
fn validation_counters_report_renders_particle_counter_summary() {
    super::reset_event_loop_for_test();
    crate::allocation_counters::reset_for_test();
    crate::allocation_counters::set_enabled_for_test(false);
    crate::events::record_particle_simulation_step(5);
    crate::events::record_particle_simulation_step(3);
    crate::events::record_particle_aggregation(7);
    crate::events::record_planning_preview_copy(5);
    crate::events::record_planning_preview_copy(3);
    crate::events::record_projection_reuse_hit();
    crate::events::record_projection_reuse_miss();
    crate::events::record_projection_reuse_miss();
    crate::events::record_compiled_field_cache_hit();
    crate::events::record_compiled_field_cache_hit();
    crate::events::record_compiled_field_cache_miss();
    crate::events::record_planner_compiled_cells_emitted_count(11);
    crate::events::record_planner_compiled_cells_emitted_count(5);
    crate::events::record_planner_candidate_cells_built_count(13);
    crate::events::record_planner_candidate_cells_built_count(2);
    super::super::event_loop::with_event_loop_state_for_test(|state| {
        let metrics = state.runtime_metrics_mut();
        metrics.record_cursor_autocmd_fast_path_dropped(AutocmdIngress::WinEnter);
        metrics.record_cursor_autocmd_fast_path_continued(AutocmdIngress::WinEnter);
        metrics.record_cursor_autocmd_fast_path_dropped(AutocmdIngress::WinScrolled);
        metrics.record_cursor_autocmd_fast_path_continued(AutocmdIngress::BufEnter);
    });
    crate::events::record_particle_overlay_refresh(4);
    super::super::event_loop::with_event_loop_state_for_test(|state| {
        let metrics = state.runtime_metrics_mut();
        metrics.record_buffer_metadata_read();
        metrics.record_current_buffer_changedtick_read();
        metrics.record_current_buffer_changedtick_read();
        metrics.record_editor_bounds_read();
        metrics.record_editor_bounds_read();
        metrics.record_command_row_read();
    });

    let report = test_validation_counters_report();
    assert!(report.starts_with("smear_cursor_validation "));
    assert!(report.contains("pss=2"));
    assert!(report.contains("psp=8"));
    assert!(report.contains("pac=1"));
    assert!(report.contains("pap=7"));
    assert!(report.contains("ppi=2"));
    assert!(report.contains("ppp=8"));
    assert!(report.contains("prh=1"));
    assert!(report.contains("prm=2"));
    assert!(report.contains("pch=2"));
    assert!(report.contains("pcm=1"));
    assert!(report.contains("pce=16"));
    assert!(report.contains("pcb=15"));
    assert!(report.contains("wed=1"));
    assert!(report.contains("wec=1"));
    assert!(report.contains("wsd=1"));
    assert!(report.contains("wsc=0"));
    assert!(report.contains("bed=0"));
    assert!(report.contains("bec=1"));
    assert!(report.contains("por=1"));
    assert!(report.contains("poc=4"));
    assert!(report.contains("alc=0"));
    assert!(report.contains("alb=0"));
    assert!(report.contains("bmr=1"));
    assert!(report.contains("cbtr=2"));
    assert!(report.contains("ebr=2"));
    assert!(report.contains("crr=1"));
}

#[cfg(feature = "perf-counters")]
#[test]
fn validation_counters_report_snapshot_renders_stable_field_order() {
    super::reset_event_loop_for_test();
    crate::allocation_counters::reset_for_test();
    crate::allocation_counters::set_enabled_for_test(false);
    assert_snapshot!(test_validation_counters_report());
}

#[cfg(not(feature = "perf-counters"))]
#[test]
fn validation_counters_report_marks_the_feature_as_disabled() {
    assert_eq!(
        test_validation_counters_report(),
        "smear_cursor_validation unavailable=feature_disabled"
    );
}

const SHELL_CACHE_TEST_BUFFER_HANDLE: i64 = 91;
const SHELL_CACHE_TEST_CALLBACK_DURATION_MS: f64 = 12.0;

fn shell_cache_context_key() -> super::super::probe_cache::CursorTextContextCacheKey {
    super::super::probe_cache::CursorTextContextCacheKey::new(
        SHELL_CACHE_TEST_BUFFER_HANDLE,
        3,
        7,
        Some(5),
    )
}

fn shell_cache_context() -> CursorTextContext {
    CursorTextContext::new(
        SHELL_CACHE_TEST_BUFFER_HANDLE,
        3,
        7,
        vec![
            ObservedTextRow::new("before".to_string()),
            ObservedTextRow::new("cursor".to_string()),
            ObservedTextRow::new("after".to_string()),
        ],
        Some(vec![ObservedTextRow::new("tracked".to_string())]),
    )
}

fn shell_cache_color_witness(
    colorscheme_generation: Generation,
    cache_generation: Generation,
) -> CursorColorProbeWitness {
    CursorColorProbeWitness::new(
        11,
        SHELL_CACHE_TEST_BUFFER_HANDLE,
        3,
        "n".to_string(),
        None,
        colorscheme_generation,
        cache_generation,
    )
}

fn prime_shell_boundary_state() -> (
    BufferMetadata,
    BufferEventPolicy,
    super::super::probe_cache::CursorTextContextCacheKey,
    CursorTextContext,
) {
    super::reset_event_loop_for_test();
    reset_transient_event_state();

    let snapshot = snapshot_for_perf_mode(BufferPerfMode::Auto);
    let metadata = listed_buffer_metadata(42);
    let context_key = shell_cache_context_key();
    let context = shell_cache_context();
    let color_witness = shell_cache_color_witness(Generation::INITIAL, Generation::INITIAL);

    mutate_shell_state(|state| {
        state.buffer_perf_telemetry_cache.record_callback_duration(
            SHELL_CACHE_TEST_BUFFER_HANDLE,
            SHELL_CACHE_TEST_CALLBACK_DURATION_MS,
        );
    })
    .expect("shell state access should succeed");

    let policy = resolve_policy_for_test(
        &snapshot,
        SHELL_CACHE_TEST_BUFFER_HANDLE,
        &metadata,
        1_000.0,
    );

    mutate_shell_state(|state| {
        state
            .editor_viewport_cache
            .store_for_test(EditorViewportSnapshot::from_dimensions(24, 1, 80));
        state
            .buffer_metadata_cache
            .store_for_test(SHELL_CACHE_TEST_BUFFER_HANDLE, metadata.clone());
        state
            .buffer_text_revision_cache
            .advance(SHELL_CACHE_TEST_BUFFER_HANDLE);
        state
            .probe_cache
            .store_cursor_text_context(context_key.clone(), Some(context.clone()));
        state
            .probe_cache
            .store_cursor_color_sample(color_witness, Some(CursorColorSample::new(0x00AB_CDEF)));
        state.note_real_cursor_visibility(RealCursorVisibility::Hidden);
    })
    .expect("shell state access should succeed");

    (metadata, policy, context_key, context)
}

#[test]
fn conceal_region_scratch_reuses_the_largest_returned_buffer() {
    let mut scratch =
        take_conceal_regions_scratch().expect("conceal region scratch should be available");
    let scratch_capacity = scratch.capacity().max(32);
    scratch.reserve(scratch_capacity.saturating_sub(scratch.len()));
    scratch.push(crate::test_support::conceal_region(3, 4, 11, 1));
    let scratch_ptr = scratch.as_ptr();
    let scratch_capacity = scratch.capacity();

    reclaim_conceal_regions_scratch(scratch).expect("conceal region scratch should be reclaimable");

    let scratch =
        take_conceal_regions_scratch().expect("conceal region scratch should be available");
    assert_eq!(scratch.capacity(), scratch_capacity);
    assert_eq!(scratch.as_ptr(), scratch_ptr);
    assert!(scratch.is_empty());

    reclaim_conceal_regions_scratch(scratch).expect("conceal region scratch should be reclaimable");
}

#[test]
fn ingress_snapshot_capture_with_current_buffer_uses_the_supplied_handle_for_callback_telemetry() {
    const TARGET_BUFFER_HANDLE: i32 = 41;
    const OTHER_BUFFER_HANDLE: i64 = 77;

    let previous_core_state = core_state().expect("core state read should succeed");
    super::reset_event_loop_for_test();
    clear_cursor_callback_duration_estimate();
    record_cursor_callback_duration(None, 3.0);

    let mut runtime = crate::state::RuntimeState::default();
    runtime.set_enabled(false);
    set_core_state(crate::core::state::CoreState::default().with_runtime(runtime))
        .expect("core state write should succeed");
    mutate_shell_state(|state| {
        state.buffer_perf_telemetry_cache.clear();
        state
            .buffer_perf_telemetry_cache
            .record_callback_duration(i64::from(TARGET_BUFFER_HANDLE), 12.0);
        state
            .buffer_perf_telemetry_cache
            .record_callback_duration(OTHER_BUFFER_HANDLE, 5.0);
    })
    .expect("shell state access should succeed");

    let buffer = api::Buffer::from(TARGET_BUFFER_HANDLE);
    let snapshot = IngressReadSnapshot::capture_with_current_buffer(Some(&buffer))
        .expect("ingress snapshot capture should succeed");

    assert_eq!(snapshot.callback_duration_estimate_ms(), 12.0);
    assert_eq!(snapshot.current_buffer_event_policy(), None);

    set_core_state(previous_core_state).expect("core state restore should succeed");
}

fn snapshot_for_perf_mode(perf_mode: BufferPerfMode) -> IngressReadSnapshot {
    IngressReadSnapshot::new_for_test(IngressReadSnapshotTestInput {
        enabled: true,
        needs_initialize: false,
        current_corners: [crate::position::RenderPoint { row: 1.0, col: 1.0 }; 4],
        target_corners: [crate::position::RenderPoint { row: 1.0, col: 1.0 }; 4],
        target_position: crate::position::RenderPoint { row: 1.0, col: 1.0 },
        tracked_cursor: None,
        mode_flags: [true, true, true, true],
        buffer_perf_mode: perf_mode,
        callback_duration_estimate_ms: 0.0,
        current_buffer_perf_class: None,
        filetypes_disabled: Vec::new(),
    })
}

fn listed_buffer_metadata(line_count: usize) -> BufferMetadata {
    BufferMetadata::new_for_test("lua", "", true, line_count)
}

fn resolve_policy_for_test(
    snapshot: &IngressReadSnapshot,
    buffer_handle: i64,
    metadata: &BufferMetadata,
    observed_at_ms: f64,
) -> BufferEventPolicy {
    resolve_buffer_event_policy_for_metadata(snapshot, buffer_handle, metadata, observed_at_ms)
        .expect("runtime policy resolution should succeed")
}
