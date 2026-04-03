use crate::core::effect::TimerKind;
use crate::core::state::ProbeKind;
use crate::core::state::ProbeReuse;
use crate::core::state::RenderThermalState;
use crate::core::types::Millis;
use std::cell::RefCell;

mod state;
mod telemetry;

pub(super) use state::EventLoopDiagnostics;
pub(super) use state::EventLoopState;
pub(super) use telemetry::RuntimeBehaviorMetrics;

#[cfg(test)]
mod tests;

thread_local! {
    static EVENT_LOOP_STATE: RefCell<EventLoopState> = const { RefCell::new(EventLoopState::new()) };
}

fn with_event_loop_state(mutator: impl FnOnce(&mut EventLoopState)) {
    let _ = EVENT_LOOP_STATE.with(|state| {
        // Event-loop telemetry is advisory. If a nested callback is already sampling it, drop the
        // contended sample instead of panicking the plugin on a RefCell borrow failure.
        let Ok(mut state) = state.try_borrow_mut() else {
            return None;
        };
        mutator(&mut state);
        Some(())
    });
}

fn with_runtime_metrics(mutator: impl FnOnce(&mut RuntimeBehaviorMetrics)) {
    with_event_loop_state(|state| mutator(state.runtime_metrics_mut()));
}

fn with_nonzero_metric_count(
    count: usize,
    mutator: impl FnOnce(&mut RuntimeBehaviorMetrics, usize),
) {
    if count == 0 {
        return;
    }
    with_runtime_metrics(|metrics| mutator(metrics, count));
}

fn with_nonzero_probe_metric_count(
    kind: ProbeKind,
    count: usize,
    mutator: impl FnOnce(&mut RuntimeBehaviorMetrics, ProbeKind, usize),
) {
    if count == 0 {
        return;
    }
    with_runtime_metrics(|metrics| mutator(metrics, kind, count));
}

fn with_nonzero_metric_total(
    area_cells: u64,
    mutator: impl FnOnce(&mut RuntimeBehaviorMetrics, u64),
) {
    if area_cells == 0 {
        return;
    }
    with_runtime_metrics(|metrics| mutator(metrics, area_cells));
}

fn read_event_loop_state<R>(reader: impl FnOnce(&EventLoopState) -> R) -> Option<R> {
    EVENT_LOOP_STATE.with(|state| {
        let Ok(state) = state.try_borrow() else {
            return None;
        };
        Some(reader(&state))
    })
}

#[cfg(test)]
fn with_event_loop_state_for_test<R>(mutator: impl FnOnce(&mut EventLoopState) -> R) -> R {
    EVENT_LOOP_STATE.with(|state| {
        let mut state = state.borrow_mut();
        mutator(&mut state)
    })
}

pub(super) fn note_autocmd_event(now_ms: f64) {
    with_event_loop_state(|state| state.note_autocmd_event(now_ms));
}

pub(super) fn clear_autocmd_event_timestamp() {
    with_event_loop_state(EventLoopState::clear_autocmd_event_timestamp);
}

pub(super) fn note_observation_request(now_ms: f64) {
    with_event_loop_state(|state| state.note_observation_request(now_ms));
}

pub(super) fn clear_observation_request_timestamp() {
    with_event_loop_state(EventLoopState::clear_observation_request_timestamp);
}

pub(super) fn record_cursor_callback_duration(duration_ms: f64) {
    with_event_loop_state(|state| state.record_cursor_callback_duration(duration_ms));
}

pub(super) fn clear_cursor_callback_duration_estimate() {
    with_event_loop_state(EventLoopState::clear_cursor_callback_duration_estimate);
}

pub(super) fn cursor_callback_duration_estimate_ms() -> f64 {
    read_event_loop_state(EventLoopState::cursor_callback_duration_estimate_ms).unwrap_or(0.0)
}

pub(super) fn record_ingress_received() {
    with_runtime_metrics(RuntimeBehaviorMetrics::record_ingress_received);
}

pub(super) fn record_ingress_coalesced() {
    with_runtime_metrics(RuntimeBehaviorMetrics::record_ingress_coalesced);
}

pub(super) fn record_ingress_coalesced_count(count: usize) {
    with_nonzero_metric_count(
        count,
        RuntimeBehaviorMetrics::record_ingress_coalesced_count,
    );
}

pub(super) fn record_ingress_dropped() {
    with_runtime_metrics(RuntimeBehaviorMetrics::record_ingress_dropped);
}

pub(super) fn record_ingress_applied() {
    with_runtime_metrics(RuntimeBehaviorMetrics::record_ingress_applied);
}

pub(super) fn record_observation_request_executed() {
    with_runtime_metrics(RuntimeBehaviorMetrics::record_observation_request_executed);
}

pub(super) fn record_degraded_draw_application() {
    with_runtime_metrics(RuntimeBehaviorMetrics::record_degraded_draw_application);
}

pub(super) fn record_stale_token_event() {
    with_runtime_metrics(RuntimeBehaviorMetrics::record_stale_token_event);
}

pub(super) fn record_stale_token_event_count(count: usize) {
    with_nonzero_metric_count(
        count,
        RuntimeBehaviorMetrics::record_stale_token_event_count,
    );
}

pub(super) fn record_timer_schedule_duration(duration_micros: u64) {
    with_runtime_metrics(|metrics| metrics.record_timer_schedule_duration(duration_micros));
}

pub(super) fn record_timer_fire_duration(duration_micros: u64) {
    with_runtime_metrics(|metrics| metrics.record_timer_fire_duration(duration_micros));
}

pub(super) fn record_host_timer_rearm(kind: TimerKind) {
    with_runtime_metrics(|metrics| metrics.record_host_timer_rearm(kind));
}

pub(super) fn record_delayed_ingress_pending_update() {
    with_runtime_metrics(RuntimeBehaviorMetrics::record_delayed_ingress_pending_update);
}

pub(super) fn record_delayed_ingress_pending_update_count(count: usize) {
    with_nonzero_metric_count(
        count,
        RuntimeBehaviorMetrics::record_delayed_ingress_pending_update_count,
    );
}

pub(super) fn record_scheduled_queue_depth(depth: usize) {
    with_runtime_metrics(|metrics| metrics.record_scheduled_queue_depth(depth));
}

pub(super) fn record_scheduled_queue_depth_for_thermal(thermal: RenderThermalState, depth: usize) {
    with_runtime_metrics(|metrics| {
        metrics.record_scheduled_queue_depth_for_thermal(thermal, depth)
    });
}

pub(super) fn record_scheduled_drain_items(drained_items: usize) {
    with_runtime_metrics(|metrics| metrics.record_scheduled_drain_items(drained_items));
}

pub(super) fn record_scheduled_drain_items_for_thermal(
    thermal: RenderThermalState,
    drained_items: usize,
) {
    with_runtime_metrics(|metrics| {
        metrics.record_scheduled_drain_items_for_thermal(thermal, drained_items)
    });
}

pub(super) fn record_scheduled_drain_reschedule() {
    with_runtime_metrics(RuntimeBehaviorMetrics::record_scheduled_drain_reschedule);
}

pub(super) fn record_scheduled_drain_reschedule_for_thermal(thermal: RenderThermalState) {
    with_runtime_metrics(|metrics| metrics.record_scheduled_drain_reschedule_for_thermal(thermal));
}

pub(super) fn record_post_burst_convergence(started_at: Millis, converged_at: Millis) {
    with_runtime_metrics(|metrics| metrics.record_post_burst_convergence(started_at, converged_at));
}

pub(super) fn record_probe_duration(kind: ProbeKind, duration_micros: u64) {
    with_runtime_metrics(|metrics| metrics.record_probe_duration(kind, duration_micros));
}

pub(super) fn record_probe_refresh_retried(kind: ProbeKind) {
    with_runtime_metrics(|metrics| metrics.record_probe_refresh_retried(kind));
}

pub(super) fn record_probe_refresh_retried_count(kind: ProbeKind, count: usize) {
    with_nonzero_probe_metric_count(
        kind,
        count,
        RuntimeBehaviorMetrics::record_probe_refresh_retried_count,
    );
}

pub(super) fn record_probe_refresh_budget_exhausted(kind: ProbeKind) {
    with_runtime_metrics(|metrics| metrics.record_probe_refresh_budget_exhausted(kind));
}

pub(super) fn record_probe_refresh_budget_exhausted_count(kind: ProbeKind, count: usize) {
    with_nonzero_probe_metric_count(
        kind,
        count,
        RuntimeBehaviorMetrics::record_probe_refresh_budget_exhausted_count,
    );
}

pub(super) fn diagnostics_snapshot() -> EventLoopDiagnostics {
    read_event_loop_state(EventLoopState::diagnostics_snapshot)
        .unwrap_or_else(|| EventLoopState::new().diagnostics_snapshot())
}

pub(super) fn record_probe_extmark_fallback(kind: ProbeKind) {
    with_runtime_metrics(|metrics| metrics.record_probe_extmark_fallback(kind));
}

pub(super) fn record_cursor_color_cache_hit() {
    with_runtime_metrics(RuntimeBehaviorMetrics::record_cursor_color_cache_hit);
}

pub(super) fn record_cursor_color_cache_miss() {
    with_runtime_metrics(RuntimeBehaviorMetrics::record_cursor_color_cache_miss);
}

pub(super) fn record_cursor_color_reuse(reuse: ProbeReuse) {
    with_runtime_metrics(|metrics| metrics.record_cursor_color_reuse(reuse));
}

pub(super) fn record_conceal_region_cache_hit() {
    with_runtime_metrics(RuntimeBehaviorMetrics::record_conceal_region_cache_hit);
}

pub(super) fn record_conceal_region_cache_miss() {
    with_runtime_metrics(RuntimeBehaviorMetrics::record_conceal_region_cache_miss);
}

pub(super) fn record_conceal_screen_cell_cache_hit() {
    with_runtime_metrics(RuntimeBehaviorMetrics::record_conceal_screen_cell_cache_hit);
}

pub(super) fn record_conceal_screen_cell_cache_miss() {
    with_runtime_metrics(RuntimeBehaviorMetrics::record_conceal_screen_cell_cache_miss);
}

pub(super) fn record_conceal_full_scan() {
    with_runtime_metrics(RuntimeBehaviorMetrics::record_conceal_full_scan);
}

pub(super) fn record_conceal_raw_screenpos_fallback() {
    with_runtime_metrics(RuntimeBehaviorMetrics::record_conceal_raw_screenpos_fallback);
}

pub(super) fn record_planner_local_query(
    bucket_maps_scanned: usize,
    bucket_cells_scanned: usize,
    local_query_cells: usize,
) {
    if bucket_maps_scanned == 0 && bucket_cells_scanned == 0 && local_query_cells == 0 {
        return;
    }
    with_runtime_metrics(|metrics| {
        metrics.record_planner_local_query(
            bucket_maps_scanned,
            bucket_cells_scanned,
            local_query_cells,
        )
    });
}

pub(super) fn record_planner_local_query_envelope_area_cells(area_cells: u64) {
    with_nonzero_metric_total(
        area_cells,
        RuntimeBehaviorMetrics::record_planner_local_query_envelope_area_cells,
    );
}

pub(super) fn record_planner_compiled_query_cells_count(count: usize) {
    with_nonzero_metric_count(
        count,
        RuntimeBehaviorMetrics::record_planner_compiled_query_cells_count,
    );
}

pub(super) fn record_planner_candidate_query_cells_count(count: usize) {
    with_nonzero_metric_count(
        count,
        RuntimeBehaviorMetrics::record_planner_candidate_query_cells_count,
    );
}

pub(super) fn record_planner_compiled_cells_emitted_count(count: usize) {
    with_nonzero_metric_count(
        count,
        RuntimeBehaviorMetrics::record_planner_compiled_cells_emitted_count,
    );
}

pub(super) fn record_planner_reference_compile() {
    with_runtime_metrics(RuntimeBehaviorMetrics::record_planner_reference_compile);
}

pub(super) fn record_planner_local_query_compile() {
    with_runtime_metrics(RuntimeBehaviorMetrics::record_planner_local_query_compile);
}

pub(super) fn record_planner_candidate_cells_built_count(count: usize) {
    with_nonzero_metric_count(
        count,
        RuntimeBehaviorMetrics::record_planner_candidate_cells_built_count,
    );
}
