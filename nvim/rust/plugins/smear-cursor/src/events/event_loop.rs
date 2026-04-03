use crate::core::effect::TimerKind;
use crate::core::state::ProbeKind;
use crate::core::state::ProbeReuse;
use crate::core::state::RenderThermalState;
use crate::core::types::Millis;
use std::cell::RefCell;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct DurationTelemetry {
    pub(super) samples: u64,
    pub(super) total_micros: u64,
    pub(super) max_micros: u64,
}

impl DurationTelemetry {
    fn record_micros(&mut self, duration_micros: u64) {
        self.samples = self.samples.saturating_add(1);
        self.total_micros = self.total_micros.saturating_add(duration_micros);
        self.max_micros = self.max_micros.max(duration_micros);
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct DepthTelemetry {
    pub(super) samples: u64,
    pub(super) total_depth: u64,
    pub(super) max_depth: u64,
}

impl DepthTelemetry {
    fn record_depth(&mut self, depth: usize) {
        let depth = u64::try_from(depth).unwrap_or(u64::MAX);
        self.samples = self.samples.saturating_add(1);
        self.total_depth = self.total_depth.saturating_add(depth);
        self.max_depth = self.max_depth.max(depth);
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct TimerCountTelemetry {
    pub(super) animation: u64,
    pub(super) ingress: u64,
    pub(super) recovery: u64,
    pub(super) cleanup: u64,
}

impl TimerCountTelemetry {
    const fn new() -> Self {
        Self {
            animation: 0,
            ingress: 0,
            recovery: 0,
            cleanup: 0,
        }
    }

    fn record_kind(&mut self, kind: TimerKind) {
        match kind {
            TimerKind::Animation => {
                self.animation = self.animation.saturating_add(1);
            }
            TimerKind::Ingress => {
                self.ingress = self.ingress.saturating_add(1);
            }
            TimerKind::Recovery => {
                self.recovery = self.recovery.saturating_add(1);
            }
            TimerKind::Cleanup => {
                self.cleanup = self.cleanup.saturating_add(1);
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct ThermalDepthTelemetry {
    pub(super) hot: DepthTelemetry,
    pub(super) cooling: DepthTelemetry,
    pub(super) cold: DepthTelemetry,
}

impl ThermalDepthTelemetry {
    const fn new() -> Self {
        Self {
            hot: DepthTelemetry {
                samples: 0,
                total_depth: 0,
                max_depth: 0,
            },
            cooling: DepthTelemetry {
                samples: 0,
                total_depth: 0,
                max_depth: 0,
            },
            cold: DepthTelemetry {
                samples: 0,
                total_depth: 0,
                max_depth: 0,
            },
        }
    }

    fn record_depth(&mut self, thermal: RenderThermalState, depth: usize) {
        match thermal {
            RenderThermalState::Hot => self.hot.record_depth(depth),
            RenderThermalState::Cooling => self.cooling.record_depth(depth),
            RenderThermalState::Cold => self.cold.record_depth(depth),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct ThermalCountTelemetry {
    pub(super) hot: u64,
    pub(super) cooling: u64,
    pub(super) cold: u64,
}

impl ThermalCountTelemetry {
    const fn new() -> Self {
        Self {
            hot: 0,
            cooling: 0,
            cold: 0,
        }
    }

    fn record(&mut self, thermal: RenderThermalState) {
        match thermal {
            RenderThermalState::Hot => {
                self.hot = self.hot.saturating_add(1);
            }
            RenderThermalState::Cooling => {
                self.cooling = self.cooling.saturating_add(1);
            }
            RenderThermalState::Cold => {
                self.cold = self.cold.saturating_add(1);
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct HitMissTelemetry {
    pub(super) hits: u64,
    pub(super) misses: u64,
}

impl HitMissTelemetry {
    const fn new() -> Self {
        Self { hits: 0, misses: 0 }
    }

    fn record_hit(&mut self) {
        self.hits = self.hits.saturating_add(1);
    }

    fn record_miss(&mut self) {
        self.misses = self.misses.saturating_add(1);
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct ProbeReuseTelemetry {
    pub(super) exact: u64,
    pub(super) compatible: u64,
    pub(super) refresh_required: u64,
}

impl ProbeReuseTelemetry {
    const fn new() -> Self {
        Self {
            exact: 0,
            compatible: 0,
            refresh_required: 0,
        }
    }

    fn record(&mut self, reuse: ProbeReuse) {
        match reuse {
            ProbeReuse::Exact => {
                self.exact = self.exact.saturating_add(1);
            }
            ProbeReuse::Compatible => {
                self.compatible = self.compatible.saturating_add(1);
            }
            ProbeReuse::RefreshRequired => {
                self.refresh_required = self.refresh_required.saturating_add(1);
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct ConcealProbeTelemetry {
    pub(super) region_cache: HitMissTelemetry,
    pub(super) screen_cell_cache: HitMissTelemetry,
    pub(super) full_scan_calls: u64,
    pub(super) raw_screenpos_fallback_calls: u64,
}

impl ConcealProbeTelemetry {
    const fn new() -> Self {
        Self {
            region_cache: HitMissTelemetry::new(),
            screen_cell_cache: HitMissTelemetry::new(),
            full_scan_calls: 0,
            raw_screenpos_fallback_calls: 0,
        }
    }

    fn record_region_cache_hit(&mut self) {
        self.region_cache.record_hit();
    }

    fn record_region_cache_miss(&mut self) {
        self.region_cache.record_miss();
    }

    fn record_screen_cell_cache_hit(&mut self) {
        self.screen_cell_cache.record_hit();
    }

    fn record_screen_cell_cache_miss(&mut self) {
        self.screen_cell_cache.record_miss();
    }

    fn record_full_scan(&mut self) {
        self.full_scan_calls = self.full_scan_calls.saturating_add(1);
    }

    fn record_raw_screenpos_fallback(&mut self) {
        self.raw_screenpos_fallback_calls = self.raw_screenpos_fallback_calls.saturating_add(1);
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct PlannerTelemetry {
    pub(super) bucket_maps_scanned: u64,
    pub(super) bucket_cells_scanned: u64,
    pub(super) local_query_envelope_area_cells: u64,
    pub(super) local_query_cells: u64,
    pub(super) compiled_query_cells: u64,
    pub(super) candidate_query_cells: u64,
    pub(super) compiled_cells_emitted: u64,
    pub(super) candidate_cells_built: u64,
    pub(super) reference_compiles: u64,
    pub(super) local_query_compiles: u64,
}

impl PlannerTelemetry {
    const fn new() -> Self {
        Self {
            bucket_maps_scanned: 0,
            bucket_cells_scanned: 0,
            local_query_envelope_area_cells: 0,
            local_query_cells: 0,
            compiled_query_cells: 0,
            candidate_query_cells: 0,
            compiled_cells_emitted: 0,
            candidate_cells_built: 0,
            reference_compiles: 0,
            local_query_compiles: 0,
        }
    }

    fn record_local_query(
        &mut self,
        bucket_maps_scanned: usize,
        bucket_cells_scanned: usize,
        local_query_cells: usize,
    ) {
        let bucket_maps_scanned = u64::try_from(bucket_maps_scanned).unwrap_or(u64::MAX);
        let bucket_cells_scanned = u64::try_from(bucket_cells_scanned).unwrap_or(u64::MAX);
        let local_query_cells = u64::try_from(local_query_cells).unwrap_or(u64::MAX);
        self.bucket_maps_scanned = self.bucket_maps_scanned.saturating_add(bucket_maps_scanned);
        self.bucket_cells_scanned = self
            .bucket_cells_scanned
            .saturating_add(bucket_cells_scanned);
        self.local_query_cells = self.local_query_cells.saturating_add(local_query_cells);
    }

    fn record_local_query_envelope_area_cells(&mut self, area_cells: u64) {
        self.local_query_envelope_area_cells = self
            .local_query_envelope_area_cells
            .saturating_add(area_cells);
    }

    fn record_compiled_query_cells_count(&mut self, count: usize) {
        let count = u64::try_from(count).unwrap_or(u64::MAX);
        self.compiled_query_cells = self.compiled_query_cells.saturating_add(count);
    }

    fn record_candidate_query_cells_count(&mut self, count: usize) {
        let count = u64::try_from(count).unwrap_or(u64::MAX);
        self.candidate_query_cells = self.candidate_query_cells.saturating_add(count);
    }

    fn record_compiled_cells_emitted_count(&mut self, count: usize) {
        let count = u64::try_from(count).unwrap_or(u64::MAX);
        self.compiled_cells_emitted = self.compiled_cells_emitted.saturating_add(count);
    }

    fn record_candidate_cells_built_count(&mut self, count: usize) {
        let count = u64::try_from(count).unwrap_or(u64::MAX);
        self.candidate_cells_built = self.candidate_cells_built.saturating_add(count);
    }

    fn record_reference_compile(&mut self) {
        self.reference_compiles = self.reference_compiles.saturating_add(1);
    }

    fn record_local_query_compile(&mut self) {
        self.local_query_compiles = self.local_query_compiles.saturating_add(1);
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct MillisDurationTelemetry {
    pub(super) samples: u64,
    pub(super) total_ms: u64,
    pub(super) max_ms: u64,
    pub(super) last_ms: u64,
}

impl MillisDurationTelemetry {
    const fn new() -> Self {
        Self {
            samples: 0,
            total_ms: 0,
            max_ms: 0,
            last_ms: 0,
        }
    }

    fn record_duration_ms(&mut self, duration_ms: u64) {
        self.samples = self.samples.saturating_add(1);
        self.total_ms = self.total_ms.saturating_add(duration_ms);
        self.max_ms = self.max_ms.max(duration_ms);
        self.last_ms = duration_ms;
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct RuntimeBehaviorMetrics {
    pub(super) ingress_received: u64,
    pub(super) ingress_coalesced: u64,
    pub(super) ingress_dropped: u64,
    pub(super) ingress_applied: u64,
    pub(super) observation_requests_executed: u64,
    pub(super) degraded_draw_applications: u64,
    pub(super) stale_token_events: u64,
    pub(super) timer_schedule: DurationTelemetry,
    pub(super) timer_fire: DurationTelemetry,
    pub(super) host_timer_rearms_total: u64,
    pub(super) host_timer_rearms_by_kind: TimerCountTelemetry,
    pub(super) delayed_ingress_pending_updates: u64,
    pub(super) scheduled_queue_depth: DepthTelemetry,
    pub(super) scheduled_drain_items: DepthTelemetry,
    pub(super) scheduled_drain_reschedules: u64,
    pub(super) scheduled_queue_depth_by_thermal: ThermalDepthTelemetry,
    pub(super) scheduled_drain_items_by_thermal: ThermalDepthTelemetry,
    pub(super) scheduled_drain_reschedules_by_thermal: ThermalCountTelemetry,
    pub(super) post_burst_convergence: MillisDurationTelemetry,
    pub(super) cursor_color_cache: HitMissTelemetry,
    pub(super) cursor_color_reuse: ProbeReuseTelemetry,
    pub(super) cursor_color_probe: ProbeTelemetry,
    pub(super) background_probe: ProbeTelemetry,
    pub(super) conceal_probe: ConcealProbeTelemetry,
    pub(super) planner: PlannerTelemetry,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct ProbeTelemetry {
    pub(super) duration: DurationTelemetry,
    pub(super) refresh_retries: u64,
    pub(super) refresh_budget_exhausted: u64,
    pub(super) extmark_fallback_calls: u64,
}

impl ProbeTelemetry {
    const fn new() -> Self {
        Self {
            duration: DurationTelemetry {
                samples: 0,
                total_micros: 0,
                max_micros: 0,
            },
            refresh_retries: 0,
            refresh_budget_exhausted: 0,
            extmark_fallback_calls: 0,
        }
    }
}

impl RuntimeBehaviorMetrics {
    const fn new() -> Self {
        Self {
            ingress_received: 0,
            ingress_coalesced: 0,
            ingress_dropped: 0,
            ingress_applied: 0,
            observation_requests_executed: 0,
            degraded_draw_applications: 0,
            stale_token_events: 0,
            timer_schedule: DurationTelemetry {
                samples: 0,
                total_micros: 0,
                max_micros: 0,
            },
            timer_fire: DurationTelemetry {
                samples: 0,
                total_micros: 0,
                max_micros: 0,
            },
            host_timer_rearms_total: 0,
            host_timer_rearms_by_kind: TimerCountTelemetry::new(),
            delayed_ingress_pending_updates: 0,
            scheduled_queue_depth: DepthTelemetry {
                samples: 0,
                total_depth: 0,
                max_depth: 0,
            },
            scheduled_drain_items: DepthTelemetry {
                samples: 0,
                total_depth: 0,
                max_depth: 0,
            },
            scheduled_drain_reschedules: 0,
            scheduled_queue_depth_by_thermal: ThermalDepthTelemetry::new(),
            scheduled_drain_items_by_thermal: ThermalDepthTelemetry::new(),
            scheduled_drain_reschedules_by_thermal: ThermalCountTelemetry::new(),
            post_burst_convergence: MillisDurationTelemetry::new(),
            cursor_color_cache: HitMissTelemetry::new(),
            cursor_color_reuse: ProbeReuseTelemetry::new(),
            cursor_color_probe: ProbeTelemetry::new(),
            background_probe: ProbeTelemetry::new(),
            conceal_probe: ConcealProbeTelemetry::new(),
            planner: PlannerTelemetry::new(),
        }
    }

    fn probe_telemetry_mut(&mut self, kind: ProbeKind) -> &mut ProbeTelemetry {
        match kind {
            ProbeKind::CursorColor => &mut self.cursor_color_probe,
            ProbeKind::Background => &mut self.background_probe,
        }
    }

    fn record_ingress_received(&mut self) {
        self.ingress_received = self.ingress_received.saturating_add(1);
    }

    fn record_ingress_coalesced(&mut self) {
        self.ingress_coalesced = self.ingress_coalesced.saturating_add(1);
    }

    fn record_ingress_coalesced_count(&mut self, count: usize) {
        let count = u64::try_from(count).unwrap_or(u64::MAX);
        self.ingress_coalesced = self.ingress_coalesced.saturating_add(count);
    }

    fn record_ingress_dropped(&mut self) {
        self.ingress_dropped = self.ingress_dropped.saturating_add(1);
    }

    fn record_ingress_applied(&mut self) {
        self.ingress_applied = self.ingress_applied.saturating_add(1);
    }

    fn record_observation_request_executed(&mut self) {
        self.observation_requests_executed = self.observation_requests_executed.saturating_add(1);
    }

    fn record_degraded_draw_application(&mut self) {
        self.degraded_draw_applications = self.degraded_draw_applications.saturating_add(1);
    }

    fn record_stale_token_event(&mut self) {
        self.stale_token_events = self.stale_token_events.saturating_add(1);
    }

    fn record_stale_token_event_count(&mut self, count: usize) {
        let count = u64::try_from(count).unwrap_or(u64::MAX);
        self.stale_token_events = self.stale_token_events.saturating_add(count);
    }

    fn record_timer_schedule_duration(&mut self, duration_micros: u64) {
        self.timer_schedule.record_micros(duration_micros);
    }

    fn record_timer_fire_duration(&mut self, duration_micros: u64) {
        self.timer_fire.record_micros(duration_micros);
    }

    fn record_host_timer_rearm(&mut self, kind: TimerKind) {
        self.host_timer_rearms_total = self.host_timer_rearms_total.saturating_add(1);
        self.host_timer_rearms_by_kind.record_kind(kind);
    }

    fn record_delayed_ingress_pending_update(&mut self) {
        self.delayed_ingress_pending_updates =
            self.delayed_ingress_pending_updates.saturating_add(1);
    }

    fn record_delayed_ingress_pending_update_count(&mut self, count: usize) {
        let count = u64::try_from(count).unwrap_or(u64::MAX);
        self.delayed_ingress_pending_updates =
            self.delayed_ingress_pending_updates.saturating_add(count);
    }

    fn record_scheduled_queue_depth(&mut self, depth: usize) {
        self.scheduled_queue_depth.record_depth(depth);
    }

    fn record_scheduled_queue_depth_for_thermal(
        &mut self,
        thermal: RenderThermalState,
        depth: usize,
    ) {
        self.scheduled_queue_depth_by_thermal
            .record_depth(thermal, depth);
    }

    fn record_scheduled_drain_items(&mut self, drained_items: usize) {
        self.scheduled_drain_items.record_depth(drained_items);
    }

    fn record_scheduled_drain_items_for_thermal(
        &mut self,
        thermal: RenderThermalState,
        drained_items: usize,
    ) {
        self.scheduled_drain_items_by_thermal
            .record_depth(thermal, drained_items);
    }

    fn record_scheduled_drain_reschedule(&mut self) {
        self.scheduled_drain_reschedules = self.scheduled_drain_reschedules.saturating_add(1);
    }

    fn record_scheduled_drain_reschedule_for_thermal(&mut self, thermal: RenderThermalState) {
        self.scheduled_drain_reschedules_by_thermal.record(thermal);
    }

    fn record_post_burst_convergence(&mut self, started_at: Millis, converged_at: Millis) {
        let duration_ms = converged_at.value().saturating_sub(started_at.value());
        self.post_burst_convergence.record_duration_ms(duration_ms);
    }

    fn record_probe_duration(&mut self, kind: ProbeKind, duration_micros: u64) {
        self.probe_telemetry_mut(kind)
            .duration
            .record_micros(duration_micros);
    }

    fn record_probe_refresh_retried(&mut self, kind: ProbeKind) {
        let telemetry = self.probe_telemetry_mut(kind);
        telemetry.refresh_retries = telemetry.refresh_retries.saturating_add(1);
    }

    fn record_probe_refresh_retried_count(&mut self, kind: ProbeKind, count: usize) {
        let count = u64::try_from(count).unwrap_or(u64::MAX);
        let telemetry = self.probe_telemetry_mut(kind);
        telemetry.refresh_retries = telemetry.refresh_retries.saturating_add(count);
    }

    fn record_probe_refresh_budget_exhausted(&mut self, kind: ProbeKind) {
        let telemetry = self.probe_telemetry_mut(kind);
        telemetry.refresh_budget_exhausted = telemetry.refresh_budget_exhausted.saturating_add(1);
    }

    fn record_probe_refresh_budget_exhausted_count(&mut self, kind: ProbeKind, count: usize) {
        let count = u64::try_from(count).unwrap_or(u64::MAX);
        let telemetry = self.probe_telemetry_mut(kind);
        telemetry.refresh_budget_exhausted =
            telemetry.refresh_budget_exhausted.saturating_add(count);
    }

    fn record_probe_extmark_fallback(&mut self, kind: ProbeKind) {
        let telemetry = self.probe_telemetry_mut(kind);
        telemetry.extmark_fallback_calls = telemetry.extmark_fallback_calls.saturating_add(1);
    }

    fn record_cursor_color_cache_hit(&mut self) {
        self.cursor_color_cache.record_hit();
    }

    fn record_cursor_color_cache_miss(&mut self) {
        self.cursor_color_cache.record_miss();
    }

    fn record_cursor_color_reuse(&mut self, reuse: ProbeReuse) {
        self.cursor_color_reuse.record(reuse);
    }

    fn record_conceal_region_cache_hit(&mut self) {
        self.conceal_probe.record_region_cache_hit();
    }

    fn record_conceal_region_cache_miss(&mut self) {
        self.conceal_probe.record_region_cache_miss();
    }

    fn record_conceal_screen_cell_cache_hit(&mut self) {
        self.conceal_probe.record_screen_cell_cache_hit();
    }

    fn record_conceal_screen_cell_cache_miss(&mut self) {
        self.conceal_probe.record_screen_cell_cache_miss();
    }

    fn record_conceal_full_scan(&mut self) {
        self.conceal_probe.record_full_scan();
    }

    fn record_conceal_raw_screenpos_fallback(&mut self) {
        self.conceal_probe.record_raw_screenpos_fallback();
    }

    fn record_planner_local_query(
        &mut self,
        bucket_maps_scanned: usize,
        bucket_cells_scanned: usize,
        local_query_cells: usize,
    ) {
        self.planner.record_local_query(
            bucket_maps_scanned,
            bucket_cells_scanned,
            local_query_cells,
        );
    }

    fn record_planner_local_query_envelope_area_cells(&mut self, area_cells: u64) {
        self.planner
            .record_local_query_envelope_area_cells(area_cells);
    }

    fn record_planner_compiled_query_cells_count(&mut self, count: usize) {
        self.planner.record_compiled_query_cells_count(count);
    }

    fn record_planner_candidate_query_cells_count(&mut self, count: usize) {
        self.planner.record_candidate_query_cells_count(count);
    }

    fn record_planner_compiled_cells_emitted_count(&mut self, count: usize) {
        self.planner.record_compiled_cells_emitted_count(count);
    }

    fn record_planner_candidate_cells_built_count(&mut self, count: usize) {
        self.planner.record_candidate_cells_built_count(count);
    }

    fn record_planner_reference_compile(&mut self) {
        self.planner.record_reference_compile();
    }

    fn record_planner_local_query_compile(&mut self) {
        self.planner.record_local_query_compile();
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct EventLoopDiagnostics {
    pub(super) metrics: RuntimeBehaviorMetrics,
    pub(super) last_autocmd_event_ms: f64,
    pub(super) last_observation_request_ms: f64,
    pub(super) callback_duration_ewma_ms: f64,
}

pub(super) struct EventLoopState {
    last_autocmd_event_ms: f64,
    last_observation_request_ms: f64,
    callback_duration_ewma_ms: f64,
    runtime_metrics: RuntimeBehaviorMetrics,
}

impl EventLoopState {
    const CALLBACK_DURATION_EWMA_ALPHA: f64 = 0.25;

    pub(super) const fn new() -> Self {
        Self {
            last_autocmd_event_ms: 0.0,
            last_observation_request_ms: 0.0,
            callback_duration_ewma_ms: 0.0,
            runtime_metrics: RuntimeBehaviorMetrics::new(),
        }
    }

    pub(super) fn note_autocmd_event(&mut self, now_ms: f64) {
        self.last_autocmd_event_ms = now_ms;
    }

    pub(super) fn clear_autocmd_event_timestamp(&mut self) {
        self.last_autocmd_event_ms = 0.0;
    }

    #[cfg(test)]
    pub(super) fn elapsed_ms_since_last_autocmd_event(&self, now_ms: f64) -> f64 {
        let last = self.last_autocmd_event_ms;
        if last <= 0.0 {
            f64::INFINITY
        } else {
            (now_ms - last).max(0.0)
        }
    }

    pub(super) fn note_observation_request(&mut self, now_ms: f64) {
        self.last_observation_request_ms = now_ms;
    }

    pub(super) fn clear_observation_request_timestamp(&mut self) {
        self.last_observation_request_ms = 0.0;
    }

    pub(super) fn record_cursor_callback_duration(&mut self, duration_ms: f64) {
        if !duration_ms.is_finite() {
            return;
        }

        let observed = duration_ms.max(0.0);
        let previous = self.callback_duration_ewma_ms;
        self.callback_duration_ewma_ms = if previous <= 0.0 {
            observed
        } else {
            previous + Self::CALLBACK_DURATION_EWMA_ALPHA * (observed - previous)
        };
    }

    pub(super) fn clear_cursor_callback_duration_estimate(&mut self) {
        self.callback_duration_ewma_ms = 0.0;
    }

    pub(super) fn cursor_callback_duration_estimate_ms(&self) -> f64 {
        self.callback_duration_ewma_ms.max(0.0)
    }

    pub(super) fn record_ingress_received(&mut self) {
        self.runtime_metrics.record_ingress_received();
    }

    pub(super) fn record_ingress_coalesced(&mut self) {
        self.runtime_metrics.record_ingress_coalesced();
    }

    pub(super) fn record_ingress_coalesced_count(&mut self, count: usize) {
        self.runtime_metrics.record_ingress_coalesced_count(count);
    }

    pub(super) fn record_ingress_dropped(&mut self) {
        self.runtime_metrics.record_ingress_dropped();
    }

    pub(super) fn record_ingress_applied(&mut self) {
        self.runtime_metrics.record_ingress_applied();
    }

    pub(super) fn record_observation_request_executed(&mut self) {
        self.runtime_metrics.record_observation_request_executed();
    }

    pub(super) fn record_degraded_draw_application(&mut self) {
        self.runtime_metrics.record_degraded_draw_application();
    }

    pub(super) fn record_stale_token_event(&mut self) {
        self.runtime_metrics.record_stale_token_event();
    }

    pub(super) fn record_stale_token_event_count(&mut self, count: usize) {
        self.runtime_metrics.record_stale_token_event_count(count);
    }

    pub(super) fn record_timer_schedule_duration(&mut self, duration_micros: u64) {
        self.runtime_metrics
            .record_timer_schedule_duration(duration_micros);
    }

    pub(super) fn record_timer_fire_duration(&mut self, duration_micros: u64) {
        self.runtime_metrics
            .record_timer_fire_duration(duration_micros);
    }

    pub(super) fn record_host_timer_rearm(&mut self, kind: TimerKind) {
        self.runtime_metrics.record_host_timer_rearm(kind);
    }

    pub(super) fn record_delayed_ingress_pending_update(&mut self) {
        self.runtime_metrics.record_delayed_ingress_pending_update();
    }

    pub(super) fn record_delayed_ingress_pending_update_count(&mut self, count: usize) {
        self.runtime_metrics
            .record_delayed_ingress_pending_update_count(count);
    }

    pub(super) fn record_scheduled_queue_depth(&mut self, depth: usize) {
        self.runtime_metrics.record_scheduled_queue_depth(depth);
    }

    pub(super) fn record_scheduled_queue_depth_for_thermal(
        &mut self,
        thermal: RenderThermalState,
        depth: usize,
    ) {
        self.runtime_metrics
            .record_scheduled_queue_depth_for_thermal(thermal, depth);
    }

    pub(super) fn record_scheduled_drain_items(&mut self, drained_items: usize) {
        self.runtime_metrics
            .record_scheduled_drain_items(drained_items);
    }

    pub(super) fn record_scheduled_drain_items_for_thermal(
        &mut self,
        thermal: RenderThermalState,
        drained_items: usize,
    ) {
        self.runtime_metrics
            .record_scheduled_drain_items_for_thermal(thermal, drained_items);
    }

    pub(super) fn record_scheduled_drain_reschedule(&mut self) {
        self.runtime_metrics.record_scheduled_drain_reschedule();
    }

    pub(super) fn record_scheduled_drain_reschedule_for_thermal(
        &mut self,
        thermal: RenderThermalState,
    ) {
        self.runtime_metrics
            .record_scheduled_drain_reschedule_for_thermal(thermal);
    }

    pub(super) fn record_post_burst_convergence(
        &mut self,
        started_at: Millis,
        converged_at: Millis,
    ) {
        self.runtime_metrics
            .record_post_burst_convergence(started_at, converged_at);
    }

    pub(super) fn record_probe_duration(&mut self, kind: ProbeKind, duration_micros: u64) {
        self.runtime_metrics
            .record_probe_duration(kind, duration_micros);
    }

    pub(super) fn record_probe_refresh_retried(&mut self, kind: ProbeKind) {
        self.runtime_metrics.record_probe_refresh_retried(kind);
    }

    pub(super) fn record_probe_refresh_retried_count(&mut self, kind: ProbeKind, count: usize) {
        self.runtime_metrics
            .record_probe_refresh_retried_count(kind, count);
    }

    pub(super) fn record_probe_refresh_budget_exhausted(&mut self, kind: ProbeKind) {
        self.runtime_metrics
            .record_probe_refresh_budget_exhausted(kind);
    }

    pub(super) fn record_probe_refresh_budget_exhausted_count(
        &mut self,
        kind: ProbeKind,
        count: usize,
    ) {
        self.runtime_metrics
            .record_probe_refresh_budget_exhausted_count(kind, count);
    }

    pub(super) fn record_probe_extmark_fallback(&mut self, kind: ProbeKind) {
        self.runtime_metrics.record_probe_extmark_fallback(kind);
    }

    pub(super) fn record_cursor_color_cache_hit(&mut self) {
        self.runtime_metrics.record_cursor_color_cache_hit();
    }

    pub(super) fn record_cursor_color_cache_miss(&mut self) {
        self.runtime_metrics.record_cursor_color_cache_miss();
    }

    pub(super) fn record_cursor_color_reuse(&mut self, reuse: ProbeReuse) {
        self.runtime_metrics.record_cursor_color_reuse(reuse);
    }

    pub(super) fn record_conceal_region_cache_hit(&mut self) {
        self.runtime_metrics.record_conceal_region_cache_hit();
    }

    pub(super) fn record_conceal_region_cache_miss(&mut self) {
        self.runtime_metrics.record_conceal_region_cache_miss();
    }

    pub(super) fn record_conceal_screen_cell_cache_hit(&mut self) {
        self.runtime_metrics.record_conceal_screen_cell_cache_hit();
    }

    pub(super) fn record_conceal_screen_cell_cache_miss(&mut self) {
        self.runtime_metrics.record_conceal_screen_cell_cache_miss();
    }

    pub(super) fn record_conceal_full_scan(&mut self) {
        self.runtime_metrics.record_conceal_full_scan();
    }

    pub(super) fn record_conceal_raw_screenpos_fallback(&mut self) {
        self.runtime_metrics.record_conceal_raw_screenpos_fallback();
    }

    pub(super) fn record_planner_local_query(
        &mut self,
        bucket_maps_scanned: usize,
        bucket_cells_scanned: usize,
        local_query_cells: usize,
    ) {
        self.runtime_metrics.record_planner_local_query(
            bucket_maps_scanned,
            bucket_cells_scanned,
            local_query_cells,
        );
    }

    pub(super) fn record_planner_local_query_envelope_area_cells(&mut self, area_cells: u64) {
        self.runtime_metrics
            .record_planner_local_query_envelope_area_cells(area_cells);
    }

    pub(super) fn record_planner_compiled_query_cells_count(&mut self, count: usize) {
        self.runtime_metrics
            .record_planner_compiled_query_cells_count(count);
    }

    pub(super) fn record_planner_candidate_query_cells_count(&mut self, count: usize) {
        self.runtime_metrics
            .record_planner_candidate_query_cells_count(count);
    }

    pub(super) fn record_planner_compiled_cells_emitted_count(&mut self, count: usize) {
        self.runtime_metrics
            .record_planner_compiled_cells_emitted_count(count);
    }

    pub(super) fn record_planner_candidate_cells_built_count(&mut self, count: usize) {
        self.runtime_metrics
            .record_planner_candidate_cells_built_count(count);
    }

    pub(super) fn record_planner_reference_compile(&mut self) {
        self.runtime_metrics.record_planner_reference_compile();
    }

    pub(super) fn record_planner_local_query_compile(&mut self) {
        self.runtime_metrics.record_planner_local_query_compile();
    }

    pub(super) fn runtime_metrics(&self) -> RuntimeBehaviorMetrics {
        self.runtime_metrics
    }

    pub(super) fn diagnostics_snapshot(&self) -> EventLoopDiagnostics {
        EventLoopDiagnostics {
            metrics: self.runtime_metrics(),
            last_autocmd_event_ms: self.last_autocmd_event_ms,
            last_observation_request_ms: self.last_observation_request_ms,
            callback_duration_ewma_ms: self.callback_duration_ewma_ms.max(0.0),
        }
    }
}

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
    with_event_loop_state(EventLoopState::record_ingress_received);
}

pub(super) fn record_ingress_coalesced() {
    with_event_loop_state(EventLoopState::record_ingress_coalesced);
}

pub(super) fn record_ingress_coalesced_count(count: usize) {
    if count == 0 {
        return;
    }
    with_event_loop_state(|state| state.record_ingress_coalesced_count(count));
}

pub(super) fn record_ingress_dropped() {
    with_event_loop_state(EventLoopState::record_ingress_dropped);
}

pub(super) fn record_ingress_applied() {
    with_event_loop_state(EventLoopState::record_ingress_applied);
}

pub(super) fn record_observation_request_executed() {
    with_event_loop_state(EventLoopState::record_observation_request_executed);
}

pub(super) fn record_degraded_draw_application() {
    with_event_loop_state(EventLoopState::record_degraded_draw_application);
}

pub(super) fn record_stale_token_event() {
    with_event_loop_state(EventLoopState::record_stale_token_event);
}

pub(super) fn record_stale_token_event_count(count: usize) {
    if count == 0 {
        return;
    }
    with_event_loop_state(|state| state.record_stale_token_event_count(count));
}

pub(super) fn record_timer_schedule_duration(duration_micros: u64) {
    with_event_loop_state(|state| state.record_timer_schedule_duration(duration_micros));
}

pub(super) fn record_timer_fire_duration(duration_micros: u64) {
    with_event_loop_state(|state| state.record_timer_fire_duration(duration_micros));
}

pub(super) fn record_host_timer_rearm(kind: TimerKind) {
    with_event_loop_state(|state| state.record_host_timer_rearm(kind));
}

pub(super) fn record_delayed_ingress_pending_update() {
    with_event_loop_state(EventLoopState::record_delayed_ingress_pending_update);
}

pub(super) fn record_delayed_ingress_pending_update_count(count: usize) {
    if count == 0 {
        return;
    }
    with_event_loop_state(|state| state.record_delayed_ingress_pending_update_count(count));
}

pub(super) fn record_scheduled_queue_depth(depth: usize) {
    with_event_loop_state(|state| state.record_scheduled_queue_depth(depth));
}

pub(super) fn record_scheduled_queue_depth_for_thermal(thermal: RenderThermalState, depth: usize) {
    with_event_loop_state(|state| state.record_scheduled_queue_depth_for_thermal(thermal, depth));
}

pub(super) fn record_scheduled_drain_items(drained_items: usize) {
    with_event_loop_state(|state| state.record_scheduled_drain_items(drained_items));
}

pub(super) fn record_scheduled_drain_items_for_thermal(
    thermal: RenderThermalState,
    drained_items: usize,
) {
    with_event_loop_state(|state| {
        state.record_scheduled_drain_items_for_thermal(thermal, drained_items)
    });
}

pub(super) fn record_scheduled_drain_reschedule() {
    with_event_loop_state(EventLoopState::record_scheduled_drain_reschedule);
}

pub(super) fn record_scheduled_drain_reschedule_for_thermal(thermal: RenderThermalState) {
    with_event_loop_state(|state| state.record_scheduled_drain_reschedule_for_thermal(thermal));
}

pub(super) fn record_post_burst_convergence(started_at: Millis, converged_at: Millis) {
    with_event_loop_state(|state| state.record_post_burst_convergence(started_at, converged_at));
}

pub(super) fn record_probe_duration(kind: ProbeKind, duration_micros: u64) {
    with_event_loop_state(|state| state.record_probe_duration(kind, duration_micros));
}

pub(super) fn record_probe_refresh_retried(kind: ProbeKind) {
    with_event_loop_state(|state| state.record_probe_refresh_retried(kind));
}

pub(super) fn record_probe_refresh_retried_count(kind: ProbeKind, count: usize) {
    if count == 0 {
        return;
    }
    with_event_loop_state(|state| state.record_probe_refresh_retried_count(kind, count));
}

pub(super) fn record_probe_refresh_budget_exhausted(kind: ProbeKind) {
    with_event_loop_state(|state| state.record_probe_refresh_budget_exhausted(kind));
}

pub(super) fn record_probe_refresh_budget_exhausted_count(kind: ProbeKind, count: usize) {
    if count == 0 {
        return;
    }
    with_event_loop_state(|state| state.record_probe_refresh_budget_exhausted_count(kind, count));
}

pub(super) fn diagnostics_snapshot() -> EventLoopDiagnostics {
    read_event_loop_state(EventLoopState::diagnostics_snapshot)
        .unwrap_or_else(|| EventLoopState::new().diagnostics_snapshot())
}

pub(super) fn record_probe_extmark_fallback(kind: ProbeKind) {
    with_event_loop_state(|state| state.record_probe_extmark_fallback(kind));
}

pub(super) fn record_cursor_color_cache_hit() {
    with_event_loop_state(EventLoopState::record_cursor_color_cache_hit);
}

pub(super) fn record_cursor_color_cache_miss() {
    with_event_loop_state(EventLoopState::record_cursor_color_cache_miss);
}

pub(super) fn record_cursor_color_reuse(reuse: ProbeReuse) {
    with_event_loop_state(|state| state.record_cursor_color_reuse(reuse));
}

pub(super) fn record_conceal_region_cache_hit() {
    with_event_loop_state(EventLoopState::record_conceal_region_cache_hit);
}

pub(super) fn record_conceal_region_cache_miss() {
    with_event_loop_state(EventLoopState::record_conceal_region_cache_miss);
}

pub(super) fn record_conceal_screen_cell_cache_hit() {
    with_event_loop_state(EventLoopState::record_conceal_screen_cell_cache_hit);
}

pub(super) fn record_conceal_screen_cell_cache_miss() {
    with_event_loop_state(EventLoopState::record_conceal_screen_cell_cache_miss);
}

pub(super) fn record_conceal_full_scan() {
    with_event_loop_state(EventLoopState::record_conceal_full_scan);
}

pub(super) fn record_conceal_raw_screenpos_fallback() {
    with_event_loop_state(EventLoopState::record_conceal_raw_screenpos_fallback);
}

pub(super) fn record_planner_local_query(
    bucket_maps_scanned: usize,
    bucket_cells_scanned: usize,
    local_query_cells: usize,
) {
    if bucket_maps_scanned == 0 && bucket_cells_scanned == 0 && local_query_cells == 0 {
        return;
    }
    with_event_loop_state(|state| {
        state.record_planner_local_query(
            bucket_maps_scanned,
            bucket_cells_scanned,
            local_query_cells,
        )
    });
}

pub(super) fn record_planner_local_query_envelope_area_cells(area_cells: u64) {
    if area_cells == 0 {
        return;
    }
    with_event_loop_state(|state| state.record_planner_local_query_envelope_area_cells(area_cells));
}

pub(super) fn record_planner_compiled_query_cells_count(count: usize) {
    if count == 0 {
        return;
    }
    with_event_loop_state(|state| state.record_planner_compiled_query_cells_count(count));
}

pub(super) fn record_planner_candidate_query_cells_count(count: usize) {
    if count == 0 {
        return;
    }
    with_event_loop_state(|state| state.record_planner_candidate_query_cells_count(count));
}

pub(super) fn record_planner_compiled_cells_emitted_count(count: usize) {
    if count == 0 {
        return;
    }
    with_event_loop_state(|state| state.record_planner_compiled_cells_emitted_count(count));
}

pub(super) fn record_planner_reference_compile() {
    with_event_loop_state(EventLoopState::record_planner_reference_compile);
}

pub(super) fn record_planner_local_query_compile() {
    with_event_loop_state(EventLoopState::record_planner_local_query_compile);
}

pub(super) fn record_planner_candidate_cells_built_count(count: usize) {
    if count == 0 {
        return;
    }
    with_event_loop_state(|state| state.record_planner_candidate_cells_built_count(count));
}

#[cfg(test)]
mod tests {
    use super::EventLoopState;
    use super::diagnostics_snapshot;
    use super::record_conceal_full_scan;
    use super::record_conceal_raw_screenpos_fallback;
    use super::record_conceal_region_cache_hit;
    use super::record_conceal_region_cache_miss;
    use super::record_conceal_screen_cell_cache_hit;
    use super::record_conceal_screen_cell_cache_miss;
    use super::record_cursor_color_cache_hit;
    use super::record_cursor_color_cache_miss;
    use super::record_cursor_color_reuse;
    use super::record_delayed_ingress_pending_update;
    use super::record_host_timer_rearm;
    use super::record_planner_candidate_cells_built_count;
    use super::record_planner_candidate_query_cells_count;
    use super::record_planner_compiled_cells_emitted_count;
    use super::record_planner_compiled_query_cells_count;
    use super::record_planner_local_query;
    use super::record_planner_local_query_compile;
    use super::record_planner_local_query_envelope_area_cells;
    use super::record_planner_reference_compile;
    use super::record_post_burst_convergence;
    use super::record_probe_duration;
    use super::record_probe_extmark_fallback;
    use super::record_probe_refresh_budget_exhausted;
    use super::record_probe_refresh_retried;
    use super::record_scheduled_drain_items;
    use super::record_scheduled_drain_items_for_thermal;
    use super::record_scheduled_drain_reschedule;
    use super::record_scheduled_drain_reschedule_for_thermal;
    use super::record_scheduled_queue_depth;
    use super::record_scheduled_queue_depth_for_thermal;
    use super::record_timer_fire_duration;
    use super::record_timer_schedule_duration;
    use super::with_event_loop_state_for_test;
    use crate::core::effect::TimerKind;
    use crate::core::state::ProbeKind;
    use crate::core::state::ProbeReuse;
    use crate::core::state::RenderThermalState;
    use crate::core::types::Millis;

    fn reset_event_loop_state() {
        with_event_loop_state_for_test(|state| *state = EventLoopState::new());
    }

    #[test]
    fn probe_telemetry_records_duration_and_retry_counts_per_probe_kind() {
        reset_event_loop_state();

        record_probe_duration(ProbeKind::CursorColor, 2_500);
        record_probe_refresh_retried(ProbeKind::CursorColor);
        record_probe_refresh_budget_exhausted(ProbeKind::CursorColor);
        record_probe_extmark_fallback(ProbeKind::CursorColor);
        record_cursor_color_cache_hit();
        record_cursor_color_cache_miss();
        record_cursor_color_reuse(ProbeReuse::Exact);
        record_cursor_color_reuse(ProbeReuse::Compatible);
        record_cursor_color_reuse(ProbeReuse::RefreshRequired);
        record_probe_duration(ProbeKind::Background, 5_000);
        record_probe_refresh_retried(ProbeKind::Background);
        record_conceal_region_cache_hit();
        record_conceal_region_cache_miss();
        record_conceal_screen_cell_cache_hit();
        record_conceal_screen_cell_cache_miss();
        record_conceal_full_scan();
        record_conceal_raw_screenpos_fallback();

        let diagnostics = diagnostics_snapshot();

        assert_eq!(diagnostics.metrics.cursor_color_probe.duration.samples, 1);
        assert_eq!(
            diagnostics.metrics.cursor_color_probe.duration.max_micros,
            2_500
        );
        assert_eq!(diagnostics.metrics.cursor_color_probe.refresh_retries, 1);
        assert_eq!(
            diagnostics
                .metrics
                .cursor_color_probe
                .refresh_budget_exhausted,
            1
        );
        assert_eq!(
            diagnostics
                .metrics
                .cursor_color_probe
                .extmark_fallback_calls,
            1
        );
        assert_eq!(diagnostics.metrics.cursor_color_cache.hits, 1);
        assert_eq!(diagnostics.metrics.cursor_color_cache.misses, 1);
        assert_eq!(diagnostics.metrics.cursor_color_reuse.exact, 1);
        assert_eq!(diagnostics.metrics.cursor_color_reuse.compatible, 1);
        assert_eq!(diagnostics.metrics.cursor_color_reuse.refresh_required, 1);
        assert_eq!(diagnostics.metrics.background_probe.duration.samples, 1);
        assert_eq!(
            diagnostics.metrics.background_probe.duration.max_micros,
            5_000
        );
        assert_eq!(diagnostics.metrics.background_probe.refresh_retries, 1);
        assert_eq!(
            diagnostics
                .metrics
                .background_probe
                .refresh_budget_exhausted,
            0
        );
        assert_eq!(
            diagnostics.metrics.background_probe.extmark_fallback_calls,
            0
        );
        assert_eq!(diagnostics.metrics.conceal_probe.region_cache.hits, 1);
        assert_eq!(diagnostics.metrics.conceal_probe.region_cache.misses, 1);
        assert_eq!(diagnostics.metrics.conceal_probe.screen_cell_cache.hits, 1);
        assert_eq!(
            diagnostics.metrics.conceal_probe.screen_cell_cache.misses,
            1
        );
        assert_eq!(diagnostics.metrics.conceal_probe.full_scan_calls, 1);
        assert_eq!(
            diagnostics
                .metrics
                .conceal_probe
                .raw_screenpos_fallback_calls,
            1
        );
    }

    #[test]
    fn timer_and_queue_telemetry_record_duration_and_depth_samples() {
        reset_event_loop_state();

        record_timer_schedule_duration(400);
        record_timer_schedule_duration(1_600);
        record_timer_fire_duration(750);
        record_scheduled_queue_depth(1);
        record_scheduled_queue_depth(3);
        record_scheduled_drain_items(2);
        record_scheduled_drain_items(5);
        record_scheduled_drain_reschedule();

        let diagnostics = diagnostics_snapshot();

        assert_eq!(diagnostics.metrics.timer_schedule.samples, 2);
        assert_eq!(diagnostics.metrics.timer_schedule.total_micros, 2_000);
        assert_eq!(diagnostics.metrics.timer_schedule.max_micros, 1_600);
        assert_eq!(diagnostics.metrics.timer_fire.samples, 1);
        assert_eq!(diagnostics.metrics.timer_fire.total_micros, 750);
        assert_eq!(diagnostics.metrics.timer_fire.max_micros, 750);
        assert_eq!(diagnostics.metrics.scheduled_queue_depth.samples, 2);
        assert_eq!(diagnostics.metrics.scheduled_queue_depth.total_depth, 4);
        assert_eq!(diagnostics.metrics.scheduled_queue_depth.max_depth, 3);
        assert_eq!(diagnostics.metrics.scheduled_drain_items.samples, 2);
        assert_eq!(diagnostics.metrics.scheduled_drain_items.total_depth, 7);
        assert_eq!(diagnostics.metrics.scheduled_drain_items.max_depth, 5);
        assert_eq!(diagnostics.metrics.scheduled_drain_reschedules, 1);
    }

    #[test]
    fn host_timer_rearm_telemetry_counts_total_and_kind_specific_rearms() {
        reset_event_loop_state();

        record_host_timer_rearm(TimerKind::Ingress);
        record_host_timer_rearm(TimerKind::Ingress);
        record_host_timer_rearm(TimerKind::Cleanup);

        let diagnostics = diagnostics_snapshot();

        assert_eq!(diagnostics.metrics.host_timer_rearms_total, 3);
        assert_eq!(diagnostics.metrics.host_timer_rearms_by_kind.ingress, 2);
        assert_eq!(diagnostics.metrics.host_timer_rearms_by_kind.cleanup, 1);
        assert_eq!(diagnostics.metrics.host_timer_rearms_by_kind.animation, 0);
    }

    #[test]
    fn delayed_ingress_and_convergence_telemetry_record_explicit_rewrite_diagnostics() {
        reset_event_loop_state();

        record_delayed_ingress_pending_update();
        record_delayed_ingress_pending_update();
        record_post_burst_convergence(Millis::new(120), Millis::new(165));
        record_post_burst_convergence(Millis::new(200), Millis::new(280));

        let diagnostics = diagnostics_snapshot();

        assert_eq!(diagnostics.metrics.delayed_ingress_pending_updates, 2);
        assert_eq!(diagnostics.metrics.post_burst_convergence.samples, 2);
        assert_eq!(diagnostics.metrics.post_burst_convergence.last_ms, 80);
        assert_eq!(diagnostics.metrics.post_burst_convergence.max_ms, 80);
        assert_eq!(diagnostics.metrics.post_burst_convergence.total_ms, 125);
    }

    #[test]
    fn thermal_queue_telemetry_tracks_backlog_and_drain_samples_by_cleanup_state() {
        reset_event_loop_state();

        record_scheduled_queue_depth_for_thermal(RenderThermalState::Hot, 7);
        record_scheduled_queue_depth_for_thermal(RenderThermalState::Cooling, 11);
        record_scheduled_drain_items_for_thermal(RenderThermalState::Cooling, 9);
        record_scheduled_drain_reschedule_for_thermal(RenderThermalState::Cooling);
        record_scheduled_drain_items_for_thermal(RenderThermalState::Cold, 1);

        let diagnostics = diagnostics_snapshot();

        assert_eq!(
            diagnostics
                .metrics
                .scheduled_queue_depth_by_thermal
                .hot
                .total_depth,
            7
        );
        assert_eq!(
            diagnostics
                .metrics
                .scheduled_queue_depth_by_thermal
                .cooling
                .total_depth,
            11
        );
        assert_eq!(
            diagnostics
                .metrics
                .scheduled_drain_items_by_thermal
                .cooling
                .total_depth,
            9
        );
        assert_eq!(
            diagnostics
                .metrics
                .scheduled_drain_reschedules_by_thermal
                .cooling,
            1
        );
        assert_eq!(
            diagnostics
                .metrics
                .scheduled_drain_items_by_thermal
                .cold
                .total_depth,
            1
        );
    }

    #[test]
    fn planner_telemetry_tracks_local_queries_and_materialized_cells() {
        reset_event_loop_state();

        record_planner_local_query(3, 19, 7);
        record_planner_local_query(2, 11, 5);
        record_planner_local_query_envelope_area_cells(41);
        record_planner_compiled_query_cells_count(9);
        record_planner_candidate_query_cells_count(5);
        record_planner_reference_compile();
        record_planner_local_query_compile();
        record_planner_compiled_cells_emitted_count(13);
        record_planner_candidate_cells_built_count(17);

        let diagnostics = diagnostics_snapshot();

        assert_eq!(diagnostics.metrics.planner.bucket_maps_scanned, 5);
        assert_eq!(diagnostics.metrics.planner.bucket_cells_scanned, 30);
        assert_eq!(
            diagnostics.metrics.planner.local_query_envelope_area_cells,
            41
        );
        assert_eq!(diagnostics.metrics.planner.local_query_cells, 12);
        assert_eq!(diagnostics.metrics.planner.compiled_query_cells, 9);
        assert_eq!(diagnostics.metrics.planner.candidate_query_cells, 5);
        assert_eq!(diagnostics.metrics.planner.compiled_cells_emitted, 13);
        assert_eq!(diagnostics.metrics.planner.candidate_cells_built, 17);
        assert_eq!(diagnostics.metrics.planner.reference_compiles, 1);
        assert_eq!(diagnostics.metrics.planner.local_query_compiles, 1);
    }

    #[test]
    fn nested_telemetry_update_drops_contended_sample_without_panicking() {
        use super::record_ingress_received;

        reset_event_loop_state();

        with_event_loop_state_for_test(|state| {
            state.note_autocmd_event(42.0);
            record_ingress_received();
        });

        let diagnostics = diagnostics_snapshot();
        assert_eq!(diagnostics.last_autocmd_event_ms, 42.0);
        assert_eq!(diagnostics.metrics.ingress_received, 0);
    }
}
