use super::super::event_loop;
use super::super::ingress::AutocmdIngress;
use super::super::policy::BufferPerfTelemetry;
use super::engine::mutate_engine_state;
use super::engine::read_engine_state;
use super::timers::now_ms;
use crate::core::effect::TimerKind;
use crate::core::state::ProbeKind;
use crate::core::state::ProbeReuse;
use crate::core::state::RenderThermalState;
use crate::core::types::Millis;

fn record_buffer_perf_telemetry(
    buffer_handle: Option<i64>,
    update: impl FnOnce(&mut super::super::ShellState, i64),
) {
    let Some(buffer_handle) = buffer_handle else {
        return;
    };
    let _ = mutate_engine_state(|state| update(&mut state.shell, buffer_handle));
}

fn record_probe_extmark_fallback_for_buffer(
    shell: &mut super::super::ShellState,
    buffer_handle: i64,
    kind: ProbeKind,
    observed_at_ms: f64,
) {
    match kind {
        ProbeKind::CursorColor => {
            shell
                .buffer_perf_telemetry_cache
                .record_cursor_color_extmark_fallback(buffer_handle, observed_at_ms);
        }
        ProbeKind::Background => {}
    }
}

pub(crate) fn note_autocmd_event_now() {
    event_loop::note_autocmd_event(now_ms());
}

pub(crate) fn note_observation_request_now() {
    event_loop::note_observation_request(now_ms());
}

pub(crate) fn clear_autocmd_event_timestamp() {
    event_loop::clear_autocmd_event_timestamp();
}

pub(crate) fn clear_observation_request_timestamp() {
    event_loop::clear_observation_request_timestamp();
}

pub(crate) fn record_cursor_callback_duration(buffer_handle: Option<i64>, duration_ms: f64) {
    event_loop::record_cursor_callback_duration(duration_ms);
    record_buffer_perf_telemetry(buffer_handle, |shell, buffer_handle| {
        shell
            .buffer_perf_telemetry_cache
            .record_callback_duration(buffer_handle, duration_ms);
    });
}

pub(crate) fn clear_cursor_callback_duration_estimate() {
    event_loop::clear_cursor_callback_duration_estimate();
}

pub(crate) fn cursor_callback_duration_estimate_ms(buffer_handle: Option<i64>) -> f64 {
    let local_estimate = buffer_handle.and_then(|buffer_handle| {
        read_engine_state(|state| {
            state
                .shell
                .buffer_perf_telemetry_cache
                .telemetry(buffer_handle)
                .map(BufferPerfTelemetry::callback_duration_estimate_ms)
        })
        .ok()
        .flatten()
    });
    local_estimate.unwrap_or_else(event_loop::cursor_callback_duration_estimate_ms)
}

pub(crate) fn record_ingress_received() {
    event_loop::record_ingress_received();
}

pub(crate) fn record_ingress_coalesced() {
    event_loop::record_ingress_coalesced();
}

pub(crate) fn record_ingress_coalesced_count(count: usize) {
    event_loop::record_ingress_coalesced_count(count);
}

pub(crate) fn record_delayed_ingress_pending_update() {
    event_loop::record_delayed_ingress_pending_update();
}

pub(crate) fn record_delayed_ingress_pending_update_count(count: usize) {
    event_loop::record_delayed_ingress_pending_update_count(count);
}

pub(crate) fn record_ingress_dropped() {
    event_loop::record_ingress_dropped();
}

pub(crate) fn record_ingress_applied() {
    event_loop::record_ingress_applied();
}

#[cfg(feature = "perf-counters")]
pub(crate) fn record_cursor_autocmd_fast_path_dropped(ingress: AutocmdIngress) {
    event_loop::record_cursor_autocmd_fast_path_dropped(ingress);
}

#[cfg(not(feature = "perf-counters"))]
pub(crate) fn record_cursor_autocmd_fast_path_dropped(_ingress: AutocmdIngress) {}

#[cfg(feature = "perf-counters")]
pub(crate) fn record_cursor_autocmd_fast_path_continued(ingress: AutocmdIngress) {
    event_loop::record_cursor_autocmd_fast_path_continued(ingress);
}

#[cfg(not(feature = "perf-counters"))]
pub(crate) fn record_cursor_autocmd_fast_path_continued(_ingress: AutocmdIngress) {}

pub(crate) fn record_observation_request_executed() {
    event_loop::record_observation_request_executed();
}

pub(crate) fn record_degraded_draw_application() {
    event_loop::record_degraded_draw_application();
}

pub(crate) fn record_stale_token_event() {
    event_loop::record_stale_token_event();
}

pub(crate) fn record_stale_token_event_count(count: usize) {
    event_loop::record_stale_token_event_count(count);
}

pub(crate) fn record_timer_schedule_duration(duration_micros: u64) {
    event_loop::record_timer_schedule_duration(duration_micros);
}

pub(crate) fn record_timer_fire_duration(duration_micros: u64) {
    event_loop::record_timer_fire_duration(duration_micros);
}

pub(crate) fn record_host_timer_rearm(kind: TimerKind) {
    event_loop::record_host_timer_rearm(kind);
}

pub(crate) fn record_post_burst_convergence(started_at: Millis, converged_at: Millis) {
    event_loop::record_post_burst_convergence(started_at, converged_at);
}

pub(crate) fn record_scheduled_queue_depth(depth: usize) {
    event_loop::record_scheduled_queue_depth(depth);
}

pub(crate) fn record_scheduled_queue_depth_for_thermal(thermal: RenderThermalState, depth: usize) {
    event_loop::record_scheduled_queue_depth_for_thermal(thermal, depth);
}

pub(crate) fn record_scheduled_drain_items(drained_items: usize) {
    event_loop::record_scheduled_drain_items(drained_items);
}

pub(crate) fn record_scheduled_drain_items_for_thermal(
    thermal: RenderThermalState,
    drained_items: usize,
) {
    event_loop::record_scheduled_drain_items_for_thermal(thermal, drained_items);
}

pub(crate) fn record_scheduled_drain_reschedule() {
    event_loop::record_scheduled_drain_reschedule();
}

pub(crate) fn record_scheduled_drain_reschedule_for_thermal(thermal: RenderThermalState) {
    event_loop::record_scheduled_drain_reschedule_for_thermal(thermal);
}

pub(crate) fn record_probe_duration(kind: ProbeKind, duration_micros: u64) {
    event_loop::record_probe_duration(kind, duration_micros);
}

pub(crate) fn record_probe_refresh_retried(kind: ProbeKind) {
    event_loop::record_probe_refresh_retried(kind);
}

pub(crate) fn record_probe_refresh_retried_count(kind: ProbeKind, count: usize) {
    event_loop::record_probe_refresh_retried_count(kind, count);
}

pub(crate) fn record_probe_refresh_budget_exhausted(kind: ProbeKind) {
    event_loop::record_probe_refresh_budget_exhausted(kind);
}

pub(crate) fn record_probe_refresh_budget_exhausted_count(kind: ProbeKind, count: usize) {
    event_loop::record_probe_refresh_budget_exhausted_count(kind, count);
}

pub(crate) fn record_probe_extmark_fallback(buffer_handle: i64, kind: ProbeKind) {
    event_loop::record_probe_extmark_fallback(kind);
    let observed_at_ms = now_ms();
    record_buffer_perf_telemetry(Some(buffer_handle), |shell, buffer_handle| {
        record_probe_extmark_fallback_for_buffer(shell, buffer_handle, kind, observed_at_ms);
    });
}

pub(crate) fn record_cursor_color_cache_hit() {
    event_loop::record_cursor_color_cache_hit();
}

pub(crate) fn record_cursor_color_cache_miss() {
    event_loop::record_cursor_color_cache_miss();
}

pub(crate) fn record_cursor_color_probe_reuse(reuse: ProbeReuse) {
    event_loop::record_cursor_color_reuse(reuse);
}

pub(crate) fn record_conceal_region_cache_hit() {
    event_loop::record_conceal_region_cache_hit();
}

pub(crate) fn record_conceal_region_cache_miss() {
    event_loop::record_conceal_region_cache_miss();
}

pub(crate) fn record_conceal_screen_cell_cache_hit() {
    event_loop::record_conceal_screen_cell_cache_hit();
}

pub(crate) fn record_conceal_screen_cell_cache_miss() {
    event_loop::record_conceal_screen_cell_cache_miss();
}

pub(crate) fn record_conceal_full_scan(buffer_handle: i64) {
    event_loop::record_conceal_full_scan();
    record_buffer_perf_telemetry(Some(buffer_handle), |shell, buffer_handle| {
        shell
            .buffer_perf_telemetry_cache
            .record_conceal_full_scan(buffer_handle, now_ms());
    });
}

pub(crate) fn record_conceal_raw_screenpos_fallback(buffer_handle: i64) {
    event_loop::record_conceal_raw_screenpos_fallback();
    record_buffer_perf_telemetry(Some(buffer_handle), |shell, buffer_handle| {
        shell
            .buffer_perf_telemetry_cache
            .record_conceal_raw_screenpos_fallback(buffer_handle, now_ms());
    });
}

pub(crate) fn record_planner_local_query(
    bucket_maps_scanned: usize,
    bucket_cells_scanned: usize,
    local_query_cells: usize,
) {
    event_loop::record_planner_local_query(
        bucket_maps_scanned,
        bucket_cells_scanned,
        local_query_cells,
    );
}

pub(crate) fn record_planner_local_query_envelope_area_cells(area_cells: u64) {
    event_loop::record_planner_local_query_envelope_area_cells(area_cells);
}

pub(crate) fn record_planner_compiled_query_cells_count(count: usize) {
    event_loop::record_planner_compiled_query_cells_count(count);
}

pub(crate) fn record_planner_candidate_query_cells_count(count: usize) {
    event_loop::record_planner_candidate_query_cells_count(count);
}

pub(crate) fn record_planner_compiled_cells_emitted_count(count: usize) {
    event_loop::record_planner_compiled_cells_emitted_count(count);
}

pub(crate) fn record_planner_reference_compile() {
    event_loop::record_planner_reference_compile();
}

pub(crate) fn record_planner_local_query_compile() {
    event_loop::record_planner_local_query_compile();
}

pub(crate) fn record_planner_candidate_cells_built_count(count: usize) {
    event_loop::record_planner_candidate_cells_built_count(count);
}

#[cfg(feature = "perf-counters")]
pub(crate) fn record_projection_reuse_hit() {
    event_loop::record_projection_reuse_hit();
}

#[cfg(not(feature = "perf-counters"))]
pub(crate) fn record_projection_reuse_hit() {}

#[cfg(feature = "perf-counters")]
pub(crate) fn record_projection_reuse_miss() {
    event_loop::record_projection_reuse_miss();
}

#[cfg(not(feature = "perf-counters"))]
pub(crate) fn record_projection_reuse_miss() {}

#[cfg(feature = "perf-counters")]
pub(crate) fn record_compiled_field_cache_hit() {
    event_loop::record_compiled_field_cache_hit();
}

#[cfg(not(feature = "perf-counters"))]
pub(crate) fn record_compiled_field_cache_hit() {}

#[cfg(feature = "perf-counters")]
pub(crate) fn record_compiled_field_cache_miss() {
    event_loop::record_compiled_field_cache_miss();
}

#[cfg(not(feature = "perf-counters"))]
pub(crate) fn record_compiled_field_cache_miss() {}

#[cfg(feature = "perf-counters")]
pub(crate) fn record_planning_preview_invocation() {
    event_loop::record_planning_preview_invocation();
}

#[cfg(not(feature = "perf-counters"))]
pub(crate) fn record_planning_preview_invocation() {}

#[cfg(feature = "perf-counters")]
pub(crate) fn record_planning_preview_copied_particles(particle_count: usize) {
    event_loop::record_planning_preview_copied_particles(particle_count);
}

#[cfg(not(feature = "perf-counters"))]
pub(crate) fn record_planning_preview_copied_particles(_particle_count: usize) {}

#[cfg(feature = "perf-counters")]
pub(crate) fn record_planning_preview_copy(particle_count: usize) {
    event_loop::record_planning_preview_copy(particle_count);
}

#[cfg(feature = "perf-counters")]
pub(crate) fn record_particle_simulation_step(particle_count: usize) {
    event_loop::record_particle_simulation_step(particle_count);
}

#[cfg(not(feature = "perf-counters"))]
pub(crate) fn record_particle_simulation_step(_particle_count: usize) {}

#[cfg(feature = "perf-counters")]
pub(crate) fn record_particle_aggregation(particle_count: usize) {
    event_loop::record_particle_aggregation(particle_count);
}

#[cfg(not(feature = "perf-counters"))]
pub(crate) fn record_particle_aggregation(_particle_count: usize) {}

#[cfg(feature = "perf-counters")]
pub(crate) fn record_particle_overlay_refresh(cell_count: usize) {
    event_loop::record_particle_overlay_refresh(cell_count);
}

#[cfg(not(feature = "perf-counters"))]
pub(crate) fn record_particle_overlay_refresh(_cell_count: usize) {}

#[cfg(feature = "perf-counters")]
pub(crate) fn record_buffer_metadata_read() {
    event_loop::record_buffer_metadata_read();
}

#[cfg(not(feature = "perf-counters"))]
pub(crate) fn record_buffer_metadata_read() {}

#[cfg(feature = "perf-counters")]
#[allow(dead_code)]
pub(crate) fn record_current_buffer_changedtick_read() {
    event_loop::record_current_buffer_changedtick_read();
}

#[cfg(not(feature = "perf-counters"))]
#[allow(dead_code)]
pub(crate) fn record_current_buffer_changedtick_read() {}

#[cfg(feature = "perf-counters")]
pub(crate) fn record_editor_bounds_read() {
    event_loop::record_editor_bounds_read();
}

#[cfg(not(feature = "perf-counters"))]
pub(crate) fn record_editor_bounds_read() {}

#[cfg(feature = "perf-counters")]
pub(crate) fn record_command_row_read() {
    event_loop::record_command_row_read();
}

#[cfg(not(feature = "perf-counters"))]
pub(crate) fn record_command_row_read() {}

#[cfg(test)]
mod tests {
    use super::record_probe_extmark_fallback_for_buffer;
    use crate::core::state::ProbeKind;
    use crate::events::ShellState;
    use crate::test_support::proptest::pure_config;
    use proptest::prelude::*;

    fn probe_kind() -> BoxedStrategy<ProbeKind> {
        prop_oneof![Just(ProbeKind::CursorColor), Just(ProbeKind::Background),].boxed()
    }

    proptest! {
        #![proptest_config(pure_config())]

        #[test]
        fn prop_probe_extmark_fallback_updates_only_cursor_color_buffer_pressure(
            buffer_handle in any::<i16>(),
            observed_at_ms in -5_000.0_f64..20_000.0_f64,
            kind in probe_kind(),
        ) {
            let buffer_handle = i64::from(buffer_handle);
            let mut shell = ShellState::default();

            record_probe_extmark_fallback_for_buffer(
                &mut shell,
                buffer_handle,
                kind,
                observed_at_ms,
            );

            match kind {
                ProbeKind::Background => {
                    prop_assert_eq!(shell.buffer_perf_telemetry_cache.telemetry(buffer_handle), None);
                }
                ProbeKind::CursorColor => {
                    let signals = shell
                        .buffer_perf_telemetry_cache
                        .telemetry(buffer_handle)
                        .expect("cursor-color fallback should create buffer telemetry")
                        .signals_at(observed_at_ms);
                    prop_assert_eq!(signals.cursor_color_extmark_fallback_pressure(), 1.0);
                    prop_assert_eq!(signals.conceal_full_scan_pressure(), 0.0);
                    prop_assert_eq!(signals.conceal_raw_screenpos_fallback_pressure(), 0.0);
                }
            }
        }
    }
}
