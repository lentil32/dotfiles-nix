use super::super::event_loop;
use super::super::logging::warn;
use super::super::policy::BufferEventPolicy;
use super::clear_autocmd_event_timestamp;
use super::clear_cursor_callback_duration_estimate;
use super::clear_observation_request_timestamp;
use super::engine::mutate_engine_state;
use super::engine::read_engine_state;
use super::engine::reset_core_state;
use super::timers::clear_all_core_timer_handles;
use crate::allocation_counters;
use crate::core::effect::ProbePolicy;
use crate::core::effect::RetainedCursorColorFallback;
use crate::core::state::ExternalDemandKind;
use crate::core::state::ObservationSnapshot;
use crate::core::state::RenderThermalState;
use crate::draw::render_pool_diagnostics;
use nvim_oxi::Result;

#[cfg(not(test))]
use super::ingress_read_snapshot;

pub(crate) fn event_loop_diagnostics() -> event_loop::EventLoopDiagnostics {
    event_loop::diagnostics_snapshot()
}

#[cfg(not(test))]
fn current_buffer_perf_policy() -> Result<Option<BufferEventPolicy>> {
    Ok(ingress_read_snapshot()?.current_buffer_event_policy())
}

#[cfg(test)]
fn current_buffer_perf_policy() -> Result<Option<BufferEventPolicy>> {
    Ok(None)
}

pub(crate) fn diagnostics_report() -> String {
    perf_diagnostics_report()
}

#[cfg(feature = "perf-counters")]
pub(crate) fn validation_counters_report() -> String {
    allocation_counters::with_counting_suspended(|| {
        let loop_diag = event_loop_diagnostics();
        let allocation = allocation_counters::snapshot();
        [
            "smear_cursor_validation".to_string(),
            format!("pss={}", loop_diag.metrics.particle_path.simulation_steps),
            format!(
                "psp={}",
                loop_diag.metrics.particle_path.simulation_particles
            ),
            format!("pac={}", loop_diag.metrics.particle_path.aggregation_calls),
            format!(
                "pap={}",
                loop_diag.metrics.particle_path.aggregation_particles
            ),
            format!("ppi={}", loop_diag.metrics.planning_preview.calls),
            format!(
                "ppp={}",
                loop_diag.metrics.planning_preview.copied_particles
            ),
            format!("prh={}", loop_diag.metrics.planner.projection_reuse.hits),
            format!("prm={}", loop_diag.metrics.planner.projection_reuse.misses),
            format!(
                "pch={}",
                loop_diag.metrics.planner.compiled_field_cache.hits
            ),
            format!(
                "pcm={}",
                loop_diag.metrics.planner.compiled_field_cache.misses
            ),
            format!("pce={}", loop_diag.metrics.planner.compiled_cells_emitted),
            format!("pcb={}", loop_diag.metrics.planner.candidate_cells_built),
            format!(
                "wed={}",
                loop_diag.metrics.cursor_autocmd_fast_path.win_enter.dropped
            ),
            format!(
                "wec={}",
                loop_diag
                    .metrics
                    .cursor_autocmd_fast_path
                    .win_enter
                    .continued
            ),
            format!(
                "wsd={}",
                loop_diag
                    .metrics
                    .cursor_autocmd_fast_path
                    .win_scrolled
                    .dropped
            ),
            format!(
                "wsc={}",
                loop_diag
                    .metrics
                    .cursor_autocmd_fast_path
                    .win_scrolled
                    .continued
            ),
            format!(
                "bed={}",
                loop_diag.metrics.cursor_autocmd_fast_path.buf_enter.dropped
            ),
            format!(
                "bec={}",
                loop_diag
                    .metrics
                    .cursor_autocmd_fast_path
                    .buf_enter
                    .continued
            ),
            format!("por={}", loop_diag.metrics.particle_path.overlay_refreshes),
            format!(
                "poc={}",
                loop_diag.metrics.particle_path.overlay_refresh_cells
            ),
            format!("alc={}", allocation.allocation_ops),
            format!("alb={}", allocation.allocation_bytes),
            format!(
                "bmr={}",
                loop_diag.metrics.validation_reads.buffer_metadata_reads
            ),
            format!(
                "cbtr={}",
                loop_diag
                    .metrics
                    .validation_reads
                    .current_buffer_changedtick_reads
            ),
            format!(
                "ebr={}",
                loop_diag.metrics.validation_reads.editor_bounds_reads
            ),
            format!(
                "crr={}",
                loop_diag.metrics.validation_reads.command_row_reads
            ),
        ]
        .join(" ")
    })
}

#[cfg(not(feature = "perf-counters"))]
pub(crate) fn validation_counters_report() -> String {
    "smear_cursor_validation unavailable=feature_disabled".to_string()
}

fn compact_float_value(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        format!("{value:.1}")
    }
}

pub(crate) fn perf_diagnostics_report() -> String {
    allocation_counters::with_counting_suspended(|| {
        let loop_diag = event_loop_diagnostics();
        let buffer_perf_policy = current_buffer_perf_policy().ok().flatten();
        match read_engine_state(|state| {
            let core = state.core_state();
            let runtime = core.runtime();
            let cleanup = core.render_cleanup();
            let ingress_policy = core.ingress_policy();
            let demand_queue = core.demand_queue();
            let pool = render_pool_diagnostics();
            let _delayed_ingress_due_at = ingress_policy.pending_delay_until();
            let queue_cursor_pending = demand_queue.latest_cursor().is_some();
            let queue_ordered_backlog = demand_queue.ordered().len();
            let queue_total_backlog =
                queue_ordered_backlog.saturating_add(usize::from(queue_cursor_pending));
            let post_burst_convergence_last_ms =
                if loop_diag.metrics.post_burst_convergence.samples > 0 {
                    Some(loop_diag.metrics.post_burst_convergence.last_ms)
                } else {
                    None
                };
            let has_phase_owned_cursor_color = core
                .phase_observation()
                .and_then(ObservationSnapshot::cursor_color)
                .is_some();
            let retained_cursor_color_fallback = if has_phase_owned_cursor_color {
                RetainedCursorColorFallback::CompatibleSample
            } else {
                RetainedCursorColorFallback::Unavailable
            };
            let probe_policy = core
                .active_demand()
                .map(|demand| {
                    ProbePolicy::for_demand(
                        demand.kind(),
                        demand.buffer_perf_class(),
                        retained_cursor_color_fallback,
                    )
                })
                .or_else(|| {
                    buffer_perf_policy.map(|policy| {
                        ProbePolicy::for_demand(
                            ExternalDemandKind::ExternalCursor,
                            policy.perf_class(),
                            retained_cursor_color_fallback,
                        )
                    })
                });
            let configured_perf_mode = runtime.config.buffer_perf_mode;

            // Surprising: the Lua-visible `diagnostics()` payload truncates around 1 KiB through
            // the plugin bridge, so perf automation uses this compact reducer-owned subset
            // instead.
            [
                "smear_cursor".to_string(),
                format!(
                    "perf_class={}",
                    buffer_perf_policy.map_or("na", BufferEventPolicy::diagnostic_class_name)
                ),
                format!("perf_mode={}", configured_perf_mode.option_name()),
                format!(
                    "perf_effective_mode={}",
                    buffer_perf_policy.map_or("na", |policy| {
                        policy.diagnostic_effective_mode_name(configured_perf_mode)
                    })
                ),
                format!(
                    "buffer_line_count={}",
                    buffer_perf_policy.map_or(0, BufferEventPolicy::line_count)
                ),
                format!(
                    "callback_ewma_ms={}",
                    compact_float_value(buffer_perf_policy.map_or(
                        loop_diag.callback_duration_ewma_ms,
                        BufferEventPolicy::callback_duration_estimate_ms,
                    ))
                ),
                format!(
                    "probe_policy={}",
                    probe_policy.map_or("na", ProbePolicy::diagnostic_name)
                ),
                format!(
                    "perf_reason_bits={}",
                    buffer_perf_policy.map_or(0, BufferEventPolicy::observed_reason_bits)
                ),
                format!(
                    "planner_bms={}",
                    loop_diag.metrics.planner.bucket_maps_scanned
                ),
                format!(
                    "planner_bcs={}",
                    loop_diag.metrics.planner.bucket_cells_scanned
                ),
                format!(
                    "planner_lqea={}",
                    loop_diag.metrics.planner.local_query_envelope_area_cells
                ),
                format!(
                    "planner_local_query_cells={}",
                    loop_diag.metrics.planner.local_query_cells
                ),
                format!(
                    "planner_compq={}",
                    loop_diag.metrics.planner.compiled_query_cells
                ),
                format!(
                    "planner_candq={}",
                    loop_diag.metrics.planner.candidate_query_cells
                ),
                format!(
                    "planner_compiled_cells_emitted={}",
                    loop_diag.metrics.planner.compiled_cells_emitted
                ),
                format!(
                    "planner_candidate_cells_built={}",
                    loop_diag.metrics.planner.candidate_cells_built
                ),
                // Keep these keys abbreviated so the reducer payload stays below
                // the ~1 KiB bridge budget used by the perf harness.
                format!(
                    "planner_rc={}",
                    loop_diag.metrics.planner.reference_compiles
                ),
                format!(
                    "planner_lqc={}",
                    loop_diag.metrics.planner.local_query_compiles
                ),
                format!(
                    "cursor_color_extmark_fallback_calls={}",
                    loop_diag.metrics.cursor_color_probe.extmark_fallback_calls
                ),
                format!(
                    "cursor_color_cache_hit={}",
                    loop_diag.metrics.cursor_color_cache.hits
                ),
                format!(
                    "cursor_color_cache_miss={}",
                    loop_diag.metrics.cursor_color_cache.misses
                ),
                format!(
                    "cursor_color_reuse_exact={}",
                    loop_diag.metrics.cursor_color_reuse.exact
                ),
                format!(
                    "cursor_color_reuse_compatible={}",
                    loop_diag.metrics.cursor_color_reuse.compatible
                ),
                format!(
                    "cursor_color_reuse_refresh_required={}",
                    loop_diag.metrics.cursor_color_reuse.refresh_required
                ),
                format!(
                    "conceal_region_cache_hit={}",
                    loop_diag.metrics.conceal_probe.region_cache.hits
                ),
                format!(
                    "conceal_region_cache_miss={}",
                    loop_diag.metrics.conceal_probe.region_cache.misses
                ),
                format!(
                    "conceal_screen_cell_cache_hit={}",
                    loop_diag.metrics.conceal_probe.screen_cell_cache.hits
                ),
                format!(
                    "conceal_screen_cell_cache_miss={}",
                    loop_diag.metrics.conceal_probe.screen_cell_cache.misses
                ),
                format!(
                    "conceal_full_scan_calls={}",
                    loop_diag.metrics.conceal_probe.full_scan_calls
                ),
                format!(
                    "conceal_raw_screenpos_fallback_calls={}",
                    loop_diag.metrics.conceal_probe.raw_screenpos_fallback_calls
                ),
                format!(
                    "perf_reasons={}",
                    buffer_perf_policy.map_or_else(
                        || "na".to_string(),
                        BufferEventPolicy::diagnostic_observed_reason_summary,
                    )
                ),
                format!(
                    "cleanup_thermal={}",
                    cleanup_thermal_name(cleanup.thermal())
                ),
                format!("pool_total_windows={}", pool.total_windows),
                format!("pool_cached_budget={}", pool.cached_budget),
                format!("pool_peak_requested={}", pool.peak_requested_capacity),
                format!("pool_cap_hits={}", pool.capacity_cap_hits),
                format!("max_kept_windows={}", runtime.config.max_kept_windows),
                format!(
                    "delayed_ingress_pending_updates={}",
                    loop_diag.metrics.delayed_ingress_pending_updates
                ),
                format!("queue_total_backlog={queue_total_backlog}"),
                format!(
                    "post_burst_convergence_last_ms={}",
                    optional_u64_value(post_burst_convergence_last_ms)
                ),
                format!(
                    "host_timer_rearms_ingress={}",
                    loop_diag.metrics.host_timer_rearms_by_kind.ingress
                ),
                format!(
                    "scheduled_drain_reschedules_cooling={}",
                    loop_diag
                        .metrics
                        .scheduled_drain_reschedules_by_thermal
                        .cooling
                ),
            ]
            .join(" ")
        }) {
            Ok(report) => report,
            Err(err) => format!("smear_cursor error={err}"),
        }
    })
}

fn cleanup_thermal_name(thermal: RenderThermalState) -> &'static str {
    match thermal {
        RenderThermalState::Hot => "hot",
        RenderThermalState::Cooling => "cooling",
        RenderThermalState::Cold => "cold",
    }
}

fn optional_u64_value(value: Option<u64>) -> String {
    value.map_or_else(|| "none".to_string(), |value| value.to_string())
}

fn reset_transient_event_state_with_policy() {
    clear_all_core_timer_handles();
    super::super::handlers::reset_scheduled_effect_queue();
    if let Err(err) = mutate_engine_state(|state| {
        state.shell.reset_transient_caches();
    }) {
        warn(&format!(
            "engine state re-entered during transient reset; skipping shell cache reset: {err}"
        ));
    }
    clear_autocmd_event_timestamp();
    clear_observation_request_timestamp();
    clear_cursor_callback_duration_estimate();
    reset_core_state();
}

pub(crate) fn reset_transient_event_state() {
    reset_transient_event_state_with_policy();
}
