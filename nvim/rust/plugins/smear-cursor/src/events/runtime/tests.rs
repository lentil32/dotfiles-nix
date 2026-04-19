use super::*;
use crate::config::BufferPerfMode;
use crate::core::types::TimerGeneration;
use crate::core::types::TimerId;
use crate::events::RealCursorVisibility;
use crate::events::cursor::BufferMetadata;
#[cfg(feature = "perf-counters")]
use crate::events::ingress::AutocmdIngress;
use crate::events::policy::BufferEventPolicy;
use crate::events::policy::BufferPerfTelemetry;
use insta::assert_snapshot;
use nvim_oxi::api;
use pretty_assertions::assert_eq;

fn shell_timer_id(value: i64) -> super::super::timers::NvimTimerId {
    super::super::timers::NvimTimerId::try_new(value).expect("test shell timer id must be positive")
}

fn handle(value: i64, timer_id: TimerId, generation: u64) -> CoreTimerHandle {
    CoreTimerHandle {
        shell_timer_id: shell_timer_id(value),
        token: crate::core::types::TimerToken::new(timer_id, TimerGeneration::new(generation)),
    }
}

const TIMER_IDS: [TimerId; 4] = [
    TimerId::Animation,
    TimerId::Ingress,
    TimerId::Recovery,
    TimerId::Cleanup,
];

const fn timer_slot_index(timer_id: TimerId) -> usize {
    match timer_id {
        TimerId::Animation => 0,
        TimerId::Ingress => 1,
        TimerId::Recovery => 2,
        TimerId::Cleanup => 3,
    }
}

fn normalize_handles(mut handles: Vec<CoreTimerHandle>) -> Vec<CoreTimerHandle> {
    handles.sort_by_key(|handle| {
        (
            timer_slot_index(handle.token.id()),
            handle.shell_timer_id.get(),
            handle.token.generation().value(),
        )
    });
    handles
}

#[test]
fn core_timer_handles_smoke_replace_lookup_and_clear_each_slot() {
    let mut handles = CoreTimerHandles::default();
    let animation = handle(11, TimerId::Animation, 3);
    let animation_replaced = handle(12, TimerId::Animation, 4);
    let ingress = handle(21, TimerId::Ingress, 8);

    assert_eq!(handles.replace(animation), None);
    assert!(handles.has_outstanding_timer_id(TimerId::Animation));
    assert_eq!(handles.replace(animation_replaced), Some(animation));
    assert_eq!(
        handles.take_by_shell_timer_id(animation.shell_timer_id),
        None
    );
    assert_eq!(
        handles.take_by_shell_timer_id(animation_replaced.shell_timer_id),
        Some(animation_replaced)
    );
    assert!(!handles.has_outstanding_timer_id(TimerId::Animation));

    assert_eq!(handles.replace(ingress), None);
    assert!(handles.has_outstanding_timer_id(TimerId::Ingress));

    let cleared = normalize_handles(handles.clear_all());
    assert_eq!(cleared, vec![ingress]);

    for timer_id in TIMER_IDS {
        assert!(!handles.has_outstanding_timer_id(timer_id));
    }
}

#[test]
fn core_timer_handles_only_consume_matching_generation_for_persistent_slots() {
    let mut handles = CoreTimerHandles::default();
    let original = handle(11, TimerId::Animation, 3);
    let rearmed = handle(11, TimerId::Animation, 4);

    assert_eq!(handles.replace(original), None);
    assert_eq!(handles.replace(rearmed), Some(original));
    assert!(handles.has_shell_timer_id(rearmed.shell_timer_id));
    assert_eq!(handles.take_fired(rearmed.shell_timer_id, 3), None);
    assert!(handles.has_shell_timer_id(rearmed.shell_timer_id));
    assert_eq!(handles.take_fired(rearmed.shell_timer_id, 4), Some(rearmed));
    assert!(!handles.has_shell_timer_id(rearmed.shell_timer_id));
}

#[test]
fn nested_engine_state_access_returns_reentry_error_and_preserves_state() {
    let nested = mutate_engine_state(|state| {
        state.shell.set_namespace_id(77);
        read_engine_state(|inner| inner.shell.namespace_id())
    });

    assert_eq!(nested, Ok(Err(super::super::EngineAccessError::Reentered)));
    assert_eq!(
        read_engine_state(|state| state.shell.namespace_id()),
        Ok(Some(77))
    );
}

#[test]
fn colorscheme_boundary_clears_real_cursor_visibility_cache() {
    mutate_engine_state(|state| {
        state
            .shell
            .note_real_cursor_visibility(RealCursorVisibility::Hidden);
    })
    .expect("engine state access should succeed");

    note_cursor_color_colorscheme_change().expect("colorscheme boundary should succeed");

    assert_eq!(
        read_engine_state(|state| state.shell.real_cursor_visibility()),
        Ok(None)
    );
}

#[test]
fn transient_reset_clears_real_cursor_visibility_cache() {
    mutate_engine_state(|state| {
        state
            .shell
            .note_real_cursor_visibility(RealCursorVisibility::Visible);
    })
    .expect("engine state access should succeed");

    reset_transient_event_state();

    assert_eq!(
        read_engine_state(|state| state.shell.real_cursor_visibility()),
        Ok(None)
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
    assert!(report.contains("perf_class="));
    assert!(report.contains("perf_mode="));
    assert!(report.contains("perf_effective_mode="));
    assert!(report.contains("buffer_line_count="));
    assert!(report.contains("callback_ewma_ms="));
    assert!(report.contains("probe_policy="));
    assert!(report.contains("perf_reason_bits="));
    assert!(report.contains("planner_bms="));
    assert!(report.contains("planner_bcs="));
    assert!(report.contains("planner_lqea="));
    assert!(report.contains("planner_local_query_cells="));
    assert!(report.contains("planner_compq="));
    assert!(report.contains("planner_candq="));
    assert!(report.contains("planner_compiled_cells_emitted="));
    assert!(report.contains("planner_candidate_cells_built="));
    assert!(report.contains("planner_rc="));
    assert!(report.contains("planner_lqc="));
    assert!(report.contains("cursor_color_extmark_fallback_calls="));
    assert!(report.contains("cursor_color_cache_hit="));
    assert!(report.contains("cursor_color_cache_miss="));
    assert!(report.contains("cursor_color_reuse_exact="));
    assert!(report.contains("cursor_color_reuse_compatible="));
    assert!(report.contains("cursor_color_reuse_refresh_required="));
    assert!(report.contains("conceal_region_cache_hit="));
    assert!(report.contains("conceal_region_cache_miss="));
    assert!(report.contains("conceal_screen_cell_cache_hit="));
    assert!(report.contains("conceal_screen_cell_cache_miss="));
    assert!(report.contains("conceal_full_scan_calls="));
    assert!(report.contains("conceal_raw_screenpos_fallback_calls="));
    assert!(report.contains("perf_reasons="));
    assert!(report.contains("cleanup_thermal="));
    assert!(report.contains("pool_total_windows="));
    assert!(report.contains("pool_cached_budget="));
    assert!(report.contains("pool_peak_requested="));
    assert!(report.contains("pool_cap_hits="));
    assert!(report.contains("max_kept_windows="));
    assert!(report.contains("queue_total_backlog="));
    assert!(report.contains("delayed_ingress_pending_updates="));
    assert!(report.contains("post_burst_convergence_last_ms="));
    assert!(report.contains("host_timer_rearms_ingress="));
    assert!(report.contains("scheduled_drain_reschedules_cooling="));
    assert!(report.len() < 1000);
}

#[test]
fn perf_diagnostics_report_snapshot_renders_stable_field_order() {
    super::reset_event_loop_for_test();
    assert_snapshot!(test_perf_diagnostics_report());
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
    super::telemetry::record_cursor_autocmd_fast_path_dropped(AutocmdIngress::WinEnter);
    super::telemetry::record_cursor_autocmd_fast_path_continued(AutocmdIngress::WinEnter);
    super::telemetry::record_cursor_autocmd_fast_path_dropped(AutocmdIngress::WinScrolled);
    super::telemetry::record_cursor_autocmd_fast_path_continued(AutocmdIngress::BufEnter);
    crate::events::record_particle_overlay_refresh(4);
    crate::events::runtime::record_buffer_metadata_read();
    crate::events::runtime::record_current_buffer_changedtick_read();
    crate::events::runtime::record_current_buffer_changedtick_read();
    super::telemetry::record_editor_bounds_read();
    super::telemetry::record_editor_bounds_read();
    super::telemetry::record_command_row_read();

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

#[cfg(not(feature = "perf-counters"))]
#[test]
fn investigation_counters_are_noops_without_the_perf_counter_feature() {
    use crate::events::ingress::AutocmdIngress;

    super::reset_event_loop_for_test();

    crate::events::record_projection_reuse_hit();
    crate::events::record_projection_reuse_miss();
    crate::events::record_compiled_field_cache_hit();
    crate::events::record_compiled_field_cache_miss();
    super::telemetry::record_cursor_autocmd_fast_path_dropped(AutocmdIngress::WinEnter);
    super::telemetry::record_cursor_autocmd_fast_path_continued(AutocmdIngress::BufEnter);

    let diagnostics = super::diagnostics::event_loop_diagnostics();
    assert_eq!(
        diagnostics.metrics,
        super::super::event_loop::RuntimeBehaviorMetrics::new()
    );
}

fn reset_buffer_event_policy_state() {
    mutate_engine_state(|state| {
        state.shell.buffer_metadata_cache.clear();
        state.shell.buffer_perf_policy_cache.clear();
        state.shell.buffer_perf_telemetry_cache.clear();
    })
    .expect("engine state access should succeed");
}

#[test]
fn callback_duration_telemetry_uses_the_supplied_buffer_handle() {
    const TARGET_BUFFER_HANDLE: i64 = 41;
    const OTHER_BUFFER_HANDLE: i64 = 77;

    super::reset_event_loop_for_test();
    clear_cursor_callback_duration_estimate();
    mutate_engine_state(|state| {
        state.shell.buffer_perf_telemetry_cache.clear();
    })
    .expect("engine state access should succeed");

    record_cursor_callback_duration(Some(TARGET_BUFFER_HANDLE), 12.0);

    let telemetry = read_engine_state(|state| {
        (
            state
                .shell
                .buffer_perf_telemetry_cache
                .telemetry(TARGET_BUFFER_HANDLE),
            state
                .shell
                .buffer_perf_telemetry_cache
                .telemetry(OTHER_BUFFER_HANDLE),
        )
    })
    .expect("engine state access should succeed");

    assert_eq!(
        telemetry
            .0
            .map(BufferPerfTelemetry::callback_duration_estimate_ms),
        Some(12.0)
    );
    assert_eq!(telemetry.1, None);
    assert_eq!(
        cursor_callback_duration_estimate_ms(Some(TARGET_BUFFER_HANDLE)),
        12.0
    );
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
    mutate_engine_state(|state| {
        state.shell.buffer_perf_telemetry_cache.clear();
        state
            .shell
            .buffer_perf_telemetry_cache
            .record_callback_duration(i64::from(TARGET_BUFFER_HANDLE), 12.0);
        state
            .shell
            .buffer_perf_telemetry_cache
            .record_callback_duration(OTHER_BUFFER_HANDLE, 5.0);
    })
    .expect("engine state access should succeed");

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
        current_corners: [crate::types::Point { row: 1.0, col: 1.0 }; 4],
        target_corners: [crate::types::Point { row: 1.0, col: 1.0 }; 4],
        target_position: crate::types::Point { row: 1.0, col: 1.0 },
        tracked_location: None,
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

fn record_pressure_samples_in_engine(
    buffer_handle: i64,
    extmark_count: u8,
    conceal_scan_count: u8,
    conceal_raw_count: u8,
    observed_at_ms: f64,
) {
    mutate_engine_state(|state| {
        for _ in 0..extmark_count {
            state
                .shell
                .buffer_perf_telemetry_cache
                .record_cursor_color_extmark_fallback(buffer_handle, observed_at_ms);
        }
        for _ in 0..conceal_scan_count {
            state
                .shell
                .buffer_perf_telemetry_cache
                .record_conceal_full_scan(buffer_handle, observed_at_ms);
        }
        for _ in 0..conceal_raw_count {
            state
                .shell
                .buffer_perf_telemetry_cache
                .record_conceal_raw_screenpos_fallback(buffer_handle, observed_at_ms);
        }
    })
    .expect("engine state access should succeed");
}

#[test]
fn runtime_policy_resolution_smoke_uses_engine_telemetry_and_caches_the_result() {
    const BUFFER_HANDLE: i64 = 41;

    reset_buffer_event_policy_state();

    let snapshot = snapshot_for_perf_mode(BufferPerfMode::Auto);
    let metadata = listed_buffer_metadata(1);
    record_pressure_samples_in_engine(BUFFER_HANDLE, 2, 0, 0, 1_000.0);

    let first_policy = resolve_policy_for_test(&snapshot, BUFFER_HANDLE, &metadata, 1_000.0);

    assert!(first_policy.observed_reason_bits() != 0);
    assert_eq!(
        read_engine_state(|state| {
            state
                .shell
                .buffer_perf_policy_cache
                .cached_policy(BUFFER_HANDLE)
        })
        .expect("engine state access should succeed"),
        Some(first_policy)
    );

    record_pressure_samples_in_engine(BUFFER_HANDLE, 0, 2, 0, 1_500.0);
    let second_policy = resolve_policy_for_test(&snapshot, BUFFER_HANDLE, &metadata, 1_500.0);

    assert!(second_policy.observed_reason_bits() != 0);
    assert!(
        second_policy.observed_reason_bits() & first_policy.observed_reason_bits()
            == first_policy.observed_reason_bits()
    );
    assert_eq!(
        read_engine_state(|state| {
            state
                .shell
                .buffer_perf_policy_cache
                .cached_policy(BUFFER_HANDLE)
        })
        .expect("engine state access should succeed"),
        Some(second_policy)
    );
}
