#[cfg(feature = "perf-counters")]
use super::super::ingress::AutocmdIngress;
use crate::core::state::ProbeKind;
use crate::core::state::ProbeReuse;
use crate::core::state::RenderThermalState;
use crate::core::types::Millis;
use crate::core::types::TimerId;

fn saturating_add_count(total: &mut u64, count: usize) {
    let count = u64::try_from(count).unwrap_or(u64::MAX);
    *total = total.saturating_add(count);
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(in crate::events) struct ValidationReadTelemetry {
    pub(in crate::events) buffer_metadata_reads: u64,
    pub(in crate::events) current_buffer_changedtick_reads: u64,
    pub(in crate::events) editor_bounds_reads: u64,
    pub(in crate::events) command_row_reads: u64,
}

impl ValidationReadTelemetry {
    pub(super) const fn new() -> Self {
        Self {
            buffer_metadata_reads: 0,
            current_buffer_changedtick_reads: 0,
            editor_bounds_reads: 0,
            command_row_reads: 0,
        }
    }
}

#[cfg(feature = "perf-counters")]
impl ValidationReadTelemetry {
    pub(super) fn record_buffer_metadata_read(&mut self) {
        self.buffer_metadata_reads = self.buffer_metadata_reads.saturating_add(1);
    }

    pub(super) fn record_editor_bounds_read(&mut self) {
        self.editor_bounds_reads = self.editor_bounds_reads.saturating_add(1);
    }

    pub(super) fn record_command_row_read(&mut self) {
        self.command_row_reads = self.command_row_reads.saturating_add(1);
    }
}

#[cfg(feature = "perf-counters")]
impl ValidationReadTelemetry {
    pub(super) fn record_current_buffer_changedtick_read(&mut self) {
        self.current_buffer_changedtick_reads =
            self.current_buffer_changedtick_reads.saturating_add(1);
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(in crate::events) struct PlanningPreviewTelemetry {
    pub(in crate::events) calls: u64,
    pub(in crate::events) copied_particles: u64,
}

impl PlanningPreviewTelemetry {
    pub(super) const fn new() -> Self {
        Self {
            calls: 0,
            copied_particles: 0,
        }
    }

    #[cfg(feature = "perf-counters")]
    pub(super) fn record_invocation(&mut self) {
        self.calls = self.calls.saturating_add(1);
    }

    #[cfg(feature = "perf-counters")]
    pub(super) fn record_copied_particles(&mut self, particle_count: usize) {
        saturating_add_count(&mut self.copied_particles, particle_count);
    }

    #[cfg(feature = "perf-counters")]
    pub(super) fn record_copy(&mut self, particle_count: usize) {
        self.record_invocation();
        self.record_copied_particles(particle_count);
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(in crate::events) struct ParticlePathTelemetry {
    pub(in crate::events) simulation_steps: u64,
    pub(in crate::events) simulation_particles: u64,
    pub(in crate::events) aggregation_calls: u64,
    pub(in crate::events) aggregation_particles: u64,
    pub(in crate::events) overlay_refreshes: u64,
    pub(in crate::events) overlay_refresh_cells: u64,
}

impl ParticlePathTelemetry {
    pub(super) const fn new() -> Self {
        Self {
            simulation_steps: 0,
            simulation_particles: 0,
            aggregation_calls: 0,
            aggregation_particles: 0,
            overlay_refreshes: 0,
            overlay_refresh_cells: 0,
        }
    }

    #[cfg(feature = "perf-counters")]
    pub(super) fn record_simulation_step(&mut self, particle_count: usize) {
        self.simulation_steps = self.simulation_steps.saturating_add(1);
        saturating_add_count(&mut self.simulation_particles, particle_count);
    }

    #[cfg(feature = "perf-counters")]
    pub(super) fn record_aggregation(&mut self, particle_count: usize) {
        self.aggregation_calls = self.aggregation_calls.saturating_add(1);
        saturating_add_count(&mut self.aggregation_particles, particle_count);
    }

    #[cfg(feature = "perf-counters")]
    pub(super) fn record_overlay_refresh(&mut self, cell_count: usize) {
        self.overlay_refreshes = self.overlay_refreshes.saturating_add(1);
        saturating_add_count(&mut self.overlay_refresh_cells, cell_count);
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(in crate::events) struct DurationTelemetry {
    pub(in crate::events) samples: u64,
    pub(in crate::events) total_micros: u64,
    pub(in crate::events) max_micros: u64,
}

impl DurationTelemetry {
    pub(super) fn record_micros(&mut self, duration_micros: u64) {
        self.samples = self.samples.saturating_add(1);
        self.total_micros = self.total_micros.saturating_add(duration_micros);
        self.max_micros = self.max_micros.max(duration_micros);
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(in crate::events) struct DepthTelemetry {
    pub(in crate::events) samples: u64,
    pub(in crate::events) total_depth: u64,
    pub(in crate::events) max_depth: u64,
}

impl DepthTelemetry {
    pub(super) fn record_depth(&mut self, depth: usize) {
        let depth = u64::try_from(depth).unwrap_or(u64::MAX);
        self.samples = self.samples.saturating_add(1);
        self.total_depth = self.total_depth.saturating_add(depth);
        self.max_depth = self.max_depth.max(depth);
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(in crate::events) struct TimerCountTelemetry {
    pub(in crate::events) animation: u64,
    pub(in crate::events) ingress: u64,
    pub(in crate::events) recovery: u64,
    pub(in crate::events) cleanup: u64,
}

impl TimerCountTelemetry {
    pub(super) const fn new() -> Self {
        Self {
            animation: 0,
            ingress: 0,
            recovery: 0,
            cleanup: 0,
        }
    }

    pub(super) fn record_timer_id(&mut self, timer_id: TimerId) {
        match timer_id {
            TimerId::Animation => {
                self.animation = self.animation.saturating_add(1);
            }
            TimerId::Ingress => {
                self.ingress = self.ingress.saturating_add(1);
            }
            TimerId::Recovery => {
                self.recovery = self.recovery.saturating_add(1);
            }
            TimerId::Cleanup => {
                self.cleanup = self.cleanup.saturating_add(1);
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(in crate::events) struct ThermalDepthTelemetry {
    pub(in crate::events) hot: DepthTelemetry,
    pub(in crate::events) cooling: DepthTelemetry,
    pub(in crate::events) cold: DepthTelemetry,
}

impl ThermalDepthTelemetry {
    pub(super) const fn new() -> Self {
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

    pub(super) fn record_depth(&mut self, thermal: RenderThermalState, depth: usize) {
        match thermal {
            RenderThermalState::Hot => self.hot.record_depth(depth),
            RenderThermalState::Cooling => self.cooling.record_depth(depth),
            RenderThermalState::Cold => self.cold.record_depth(depth),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(in crate::events) struct ThermalCountTelemetry {
    pub(in crate::events) hot: u64,
    pub(in crate::events) cooling: u64,
    pub(in crate::events) cold: u64,
}

impl ThermalCountTelemetry {
    pub(super) const fn new() -> Self {
        Self {
            hot: 0,
            cooling: 0,
            cold: 0,
        }
    }

    pub(super) fn record(&mut self, thermal: RenderThermalState) {
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
pub(in crate::events) struct HitMissTelemetry {
    pub(in crate::events) hits: u64,
    pub(in crate::events) misses: u64,
}

impl HitMissTelemetry {
    pub(super) const fn new() -> Self {
        Self { hits: 0, misses: 0 }
    }

    pub(super) fn record_hit(&mut self) {
        self.hits = self.hits.saturating_add(1);
    }

    pub(super) fn record_miss(&mut self) {
        self.misses = self.misses.saturating_add(1);
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(in crate::events) struct DropContinueTelemetry {
    pub(in crate::events) dropped: u64,
    pub(in crate::events) continued: u64,
}

impl DropContinueTelemetry {
    pub(super) const fn new() -> Self {
        Self {
            dropped: 0,
            continued: 0,
        }
    }
}

#[cfg(feature = "perf-counters")]
impl DropContinueTelemetry {
    pub(super) fn record_dropped(&mut self) {
        self.dropped = self.dropped.saturating_add(1);
    }

    pub(super) fn record_continued(&mut self) {
        self.continued = self.continued.saturating_add(1);
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(in crate::events) struct CursorAutocmdFastPathTelemetry {
    pub(in crate::events) win_enter: DropContinueTelemetry,
    pub(in crate::events) win_scrolled: DropContinueTelemetry,
    pub(in crate::events) buf_enter: DropContinueTelemetry,
}

impl CursorAutocmdFastPathTelemetry {
    pub(super) const fn new() -> Self {
        Self {
            win_enter: DropContinueTelemetry::new(),
            win_scrolled: DropContinueTelemetry::new(),
            buf_enter: DropContinueTelemetry::new(),
        }
    }
}

#[cfg(feature = "perf-counters")]
impl CursorAutocmdFastPathTelemetry {
    fn ingress_metrics_mut(
        &mut self,
        ingress: AutocmdIngress,
    ) -> Option<&mut DropContinueTelemetry> {
        match ingress {
            AutocmdIngress::WinEnter => Some(&mut self.win_enter),
            AutocmdIngress::WinScrolled => Some(&mut self.win_scrolled),
            AutocmdIngress::BufEnter => Some(&mut self.buf_enter),
            _ => None,
        }
    }

    pub(super) fn record_dropped(&mut self, ingress: AutocmdIngress) {
        if let Some(metrics) = self.ingress_metrics_mut(ingress) {
            metrics.record_dropped();
        }
    }

    pub(super) fn record_continued(&mut self, ingress: AutocmdIngress) {
        if let Some(metrics) = self.ingress_metrics_mut(ingress) {
            metrics.record_continued();
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(in crate::events) struct ProbeReuseTelemetry {
    pub(in crate::events) exact: u64,
    pub(in crate::events) compatible: u64,
    pub(in crate::events) refresh_required: u64,
}

impl ProbeReuseTelemetry {
    pub(super) const fn new() -> Self {
        Self {
            exact: 0,
            compatible: 0,
            refresh_required: 0,
        }
    }

    pub(super) fn record(&mut self, reuse: ProbeReuse) {
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
pub(in crate::events) struct ConcealProbeTelemetry {
    pub(in crate::events) region_cache: HitMissTelemetry,
    pub(in crate::events) screen_cell_cache: HitMissTelemetry,
    pub(in crate::events) full_scan_calls: u64,
    pub(in crate::events) deferred_projection_calls: u64,
}

impl ConcealProbeTelemetry {
    pub(super) const fn new() -> Self {
        Self {
            region_cache: HitMissTelemetry::new(),
            screen_cell_cache: HitMissTelemetry::new(),
            full_scan_calls: 0,
            deferred_projection_calls: 0,
        }
    }

    pub(super) fn record_region_cache_hit(&mut self) {
        self.region_cache.record_hit();
    }

    pub(super) fn record_region_cache_miss(&mut self) {
        self.region_cache.record_miss();
    }

    pub(super) fn record_screen_cell_cache_hit(&mut self) {
        self.screen_cell_cache.record_hit();
    }

    pub(super) fn record_screen_cell_cache_miss(&mut self) {
        self.screen_cell_cache.record_miss();
    }

    pub(super) fn record_full_scan(&mut self) {
        self.full_scan_calls = self.full_scan_calls.saturating_add(1);
    }

    pub(super) fn record_deferred_projection(&mut self) {
        self.deferred_projection_calls = self.deferred_projection_calls.saturating_add(1);
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(in crate::events) struct PlannerTelemetry {
    pub(in crate::events) bucket_maps_scanned: u64,
    pub(in crate::events) bucket_cells_scanned: u64,
    pub(in crate::events) local_query_envelope_area_cells: u64,
    pub(in crate::events) local_query_cells: u64,
    pub(in crate::events) compiled_query_cells: u64,
    pub(in crate::events) candidate_query_cells: u64,
    pub(in crate::events) compiled_cells_emitted: u64,
    pub(in crate::events) candidate_cells_built: u64,
    pub(in crate::events) reference_compiles: u64,
    pub(in crate::events) local_query_compiles: u64,
    pub(in crate::events) projection_reuse: HitMissTelemetry,
    pub(in crate::events) compiled_field_cache: HitMissTelemetry,
}

impl PlannerTelemetry {
    pub(super) const fn new() -> Self {
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
            projection_reuse: HitMissTelemetry::new(),
            compiled_field_cache: HitMissTelemetry::new(),
        }
    }
}

impl PlannerTelemetry {
    pub(super) fn record_local_query(
        &mut self,
        bucket_maps_scanned: usize,
        bucket_cells_scanned: usize,
        local_query_cells: usize,
    ) {
        saturating_add_count(&mut self.bucket_maps_scanned, bucket_maps_scanned);
        saturating_add_count(&mut self.bucket_cells_scanned, bucket_cells_scanned);
        saturating_add_count(&mut self.local_query_cells, local_query_cells);
    }

    pub(super) fn record_local_query_envelope_area_cells(&mut self, area_cells: u64) {
        self.local_query_envelope_area_cells = self
            .local_query_envelope_area_cells
            .saturating_add(area_cells);
    }

    pub(super) fn record_compiled_query_cells_count(&mut self, count: usize) {
        saturating_add_count(&mut self.compiled_query_cells, count);
    }

    pub(super) fn record_candidate_query_cells_count(&mut self, count: usize) {
        saturating_add_count(&mut self.candidate_query_cells, count);
    }

    pub(super) fn record_compiled_cells_emitted_count(&mut self, count: usize) {
        saturating_add_count(&mut self.compiled_cells_emitted, count);
    }

    pub(super) fn record_candidate_cells_built_count(&mut self, count: usize) {
        saturating_add_count(&mut self.candidate_cells_built, count);
    }

    pub(super) fn record_reference_compile(&mut self) {
        self.reference_compiles = self.reference_compiles.saturating_add(1);
    }

    pub(super) fn record_local_query_compile(&mut self) {
        self.local_query_compiles = self.local_query_compiles.saturating_add(1);
    }
}

#[cfg(feature = "perf-counters")]
impl PlannerTelemetry {
    pub(super) fn record_projection_reuse_hit(&mut self) {
        self.projection_reuse.record_hit();
    }

    pub(super) fn record_projection_reuse_miss(&mut self) {
        self.projection_reuse.record_miss();
    }

    pub(super) fn record_compiled_field_cache_hit(&mut self) {
        self.compiled_field_cache.record_hit();
    }

    pub(super) fn record_compiled_field_cache_miss(&mut self) {
        self.compiled_field_cache.record_miss();
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(in crate::events) struct MillisDurationTelemetry {
    pub(in crate::events) samples: u64,
    pub(in crate::events) total_ms: u64,
    pub(in crate::events) max_ms: u64,
    pub(in crate::events) last_ms: u64,
}

impl MillisDurationTelemetry {
    pub(super) const fn new() -> Self {
        Self {
            samples: 0,
            total_ms: 0,
            max_ms: 0,
            last_ms: 0,
        }
    }

    pub(super) fn record_duration_ms(&mut self, duration_ms: u64) {
        self.samples = self.samples.saturating_add(1);
        self.total_ms = self.total_ms.saturating_add(duration_ms);
        self.max_ms = self.max_ms.max(duration_ms);
        self.last_ms = duration_ms;
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(in crate::events) struct ProbeTelemetry {
    pub(in crate::events) duration: DurationTelemetry,
    pub(in crate::events) refresh_retries: u64,
    pub(in crate::events) refresh_budget_exhausted: u64,
    pub(in crate::events) extmark_fallback_calls: u64,
}

impl ProbeTelemetry {
    pub(super) const fn new() -> Self {
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(in crate::events) struct RuntimeBehaviorMetrics {
    pub(in crate::events) ingress_received: u64,
    pub(in crate::events) ingress_coalesced: u64,
    pub(in crate::events) ingress_dropped: u64,
    pub(in crate::events) ingress_applied: u64,
    pub(in crate::events) cursor_autocmd_fast_path: CursorAutocmdFastPathTelemetry,
    pub(in crate::events) observation_requests_executed: u64,
    pub(in crate::events) degraded_draw_applications: u64,
    pub(in crate::events) stale_token_events: u64,
    pub(in crate::events) timer_schedule: DurationTelemetry,
    pub(in crate::events) timer_fire: DurationTelemetry,
    pub(in crate::events) host_timer_rearms_total: u64,
    pub(in crate::events) host_timer_rearms_by_kind: TimerCountTelemetry,
    pub(in crate::events) delayed_ingress_pending_updates: u64,
    pub(in crate::events) scheduled_queue_depth: DepthTelemetry,
    pub(in crate::events) scheduled_drain_items: DepthTelemetry,
    pub(in crate::events) scheduled_drain_reschedules: u64,
    pub(in crate::events) scheduled_queue_depth_by_thermal: ThermalDepthTelemetry,
    pub(in crate::events) scheduled_drain_items_by_thermal: ThermalDepthTelemetry,
    pub(in crate::events) scheduled_drain_reschedules_by_thermal: ThermalCountTelemetry,
    pub(in crate::events) post_burst_convergence: MillisDurationTelemetry,
    pub(in crate::events) cursor_color_cache: HitMissTelemetry,
    pub(in crate::events) cursor_color_reuse: ProbeReuseTelemetry,
    pub(in crate::events) cursor_color_probe: ProbeTelemetry,
    pub(in crate::events) background_probe: ProbeTelemetry,
    pub(in crate::events) conceal_probe: ConcealProbeTelemetry,
    pub(in crate::events) planner: PlannerTelemetry,
    pub(in crate::events) planning_preview: PlanningPreviewTelemetry,
    pub(in crate::events) particle_path: ParticlePathTelemetry,
    pub(in crate::events) validation_reads: ValidationReadTelemetry,
}

impl RuntimeBehaviorMetrics {
    pub(in crate::events) const fn new() -> Self {
        Self {
            ingress_received: 0,
            ingress_coalesced: 0,
            ingress_dropped: 0,
            ingress_applied: 0,
            cursor_autocmd_fast_path: CursorAutocmdFastPathTelemetry::new(),
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
            planning_preview: PlanningPreviewTelemetry::new(),
            particle_path: ParticlePathTelemetry::new(),
            validation_reads: ValidationReadTelemetry::new(),
        }
    }

    fn probe_telemetry_mut(&mut self, kind: ProbeKind) -> &mut ProbeTelemetry {
        match kind {
            ProbeKind::CursorColor => &mut self.cursor_color_probe,
            ProbeKind::Background => &mut self.background_probe,
        }
    }

    pub(in crate::events) fn record_ingress_received(&mut self) {
        self.ingress_received = self.ingress_received.saturating_add(1);
    }

    pub(in crate::events) fn record_ingress_coalesced(&mut self) {
        self.ingress_coalesced = self.ingress_coalesced.saturating_add(1);
    }

    pub(in crate::events) fn record_ingress_coalesced_count(&mut self, count: usize) {
        saturating_add_count(&mut self.ingress_coalesced, count);
    }

    pub(in crate::events) fn record_ingress_dropped(&mut self) {
        self.ingress_dropped = self.ingress_dropped.saturating_add(1);
    }

    pub(in crate::events) fn record_ingress_applied(&mut self) {
        self.ingress_applied = self.ingress_applied.saturating_add(1);
    }
}

#[cfg(feature = "perf-counters")]
impl RuntimeBehaviorMetrics {
    pub(in crate::events) fn record_cursor_autocmd_fast_path_dropped(
        &mut self,
        ingress: AutocmdIngress,
    ) {
        self.cursor_autocmd_fast_path.record_dropped(ingress);
    }

    pub(in crate::events) fn record_cursor_autocmd_fast_path_continued(
        &mut self,
        ingress: AutocmdIngress,
    ) {
        self.cursor_autocmd_fast_path.record_continued(ingress);
    }
}

impl RuntimeBehaviorMetrics {
    pub(in crate::events) fn record_observation_request_executed(&mut self) {
        self.observation_requests_executed = self.observation_requests_executed.saturating_add(1);
    }
}

impl RuntimeBehaviorMetrics {
    pub(in crate::events) fn record_degraded_draw_application(&mut self) {
        self.degraded_draw_applications = self.degraded_draw_applications.saturating_add(1);
    }

    pub(in crate::events) fn record_stale_token_event(&mut self) {
        self.stale_token_events = self.stale_token_events.saturating_add(1);
    }

    pub(in crate::events) fn record_stale_token_event_count(&mut self, count: usize) {
        saturating_add_count(&mut self.stale_token_events, count);
    }

    pub(in crate::events) fn record_timer_schedule_duration(&mut self, duration_micros: u64) {
        self.timer_schedule.record_micros(duration_micros);
    }

    pub(in crate::events) fn record_timer_fire_duration(&mut self, duration_micros: u64) {
        self.timer_fire.record_micros(duration_micros);
    }

    pub(in crate::events) fn record_host_timer_rearm(&mut self, timer_id: TimerId) {
        self.host_timer_rearms_total = self.host_timer_rearms_total.saturating_add(1);
        self.host_timer_rearms_by_kind.record_timer_id(timer_id);
    }

    pub(in crate::events) fn record_delayed_ingress_pending_update(&mut self) {
        self.delayed_ingress_pending_updates =
            self.delayed_ingress_pending_updates.saturating_add(1);
    }

    pub(in crate::events) fn record_delayed_ingress_pending_update_count(&mut self, count: usize) {
        saturating_add_count(&mut self.delayed_ingress_pending_updates, count);
    }

    pub(in crate::events) fn record_scheduled_queue_depth(&mut self, depth: usize) {
        self.scheduled_queue_depth.record_depth(depth);
    }

    pub(in crate::events) fn record_scheduled_queue_depth_for_thermal(
        &mut self,
        thermal: RenderThermalState,
        depth: usize,
    ) {
        self.scheduled_queue_depth_by_thermal
            .record_depth(thermal, depth);
    }

    pub(in crate::events) fn record_scheduled_drain_items(&mut self, drained_items: usize) {
        self.scheduled_drain_items.record_depth(drained_items);
    }

    pub(in crate::events) fn record_scheduled_drain_items_for_thermal(
        &mut self,
        thermal: RenderThermalState,
        drained_items: usize,
    ) {
        self.scheduled_drain_items_by_thermal
            .record_depth(thermal, drained_items);
    }

    pub(in crate::events) fn record_scheduled_drain_reschedule(&mut self) {
        self.scheduled_drain_reschedules = self.scheduled_drain_reschedules.saturating_add(1);
    }

    pub(in crate::events) fn record_scheduled_drain_reschedule_for_thermal(
        &mut self,
        thermal: RenderThermalState,
    ) {
        self.scheduled_drain_reschedules_by_thermal.record(thermal);
    }

    pub(in crate::events) fn record_post_burst_convergence(
        &mut self,
        started_at: Millis,
        converged_at: Millis,
    ) {
        let duration_ms = converged_at.value().saturating_sub(started_at.value());
        self.post_burst_convergence.record_duration_ms(duration_ms);
    }

    pub(in crate::events) fn record_probe_duration(
        &mut self,
        kind: ProbeKind,
        duration_micros: u64,
    ) {
        self.probe_telemetry_mut(kind)
            .duration
            .record_micros(duration_micros);
    }

    pub(in crate::events) fn record_probe_refresh_retried(&mut self, kind: ProbeKind) {
        let telemetry = self.probe_telemetry_mut(kind);
        telemetry.refresh_retries = telemetry.refresh_retries.saturating_add(1);
    }

    pub(in crate::events) fn record_probe_refresh_retried_count(
        &mut self,
        kind: ProbeKind,
        count: usize,
    ) {
        let telemetry = self.probe_telemetry_mut(kind);
        saturating_add_count(&mut telemetry.refresh_retries, count);
    }

    pub(in crate::events) fn record_probe_refresh_budget_exhausted(&mut self, kind: ProbeKind) {
        let telemetry = self.probe_telemetry_mut(kind);
        telemetry.refresh_budget_exhausted = telemetry.refresh_budget_exhausted.saturating_add(1);
    }

    pub(in crate::events) fn record_probe_refresh_budget_exhausted_count(
        &mut self,
        kind: ProbeKind,
        count: usize,
    ) {
        let telemetry = self.probe_telemetry_mut(kind);
        saturating_add_count(&mut telemetry.refresh_budget_exhausted, count);
    }

    pub(in crate::events) fn record_probe_extmark_fallback(&mut self, kind: ProbeKind) {
        let telemetry = self.probe_telemetry_mut(kind);
        telemetry.extmark_fallback_calls = telemetry.extmark_fallback_calls.saturating_add(1);
    }

    pub(in crate::events) fn record_cursor_color_cache_hit(&mut self) {
        self.cursor_color_cache.record_hit();
    }

    pub(in crate::events) fn record_cursor_color_cache_miss(&mut self) {
        self.cursor_color_cache.record_miss();
    }

    pub(in crate::events) fn record_cursor_color_reuse(&mut self, reuse: ProbeReuse) {
        self.cursor_color_reuse.record(reuse);
    }

    pub(in crate::events) fn record_conceal_region_cache_hit(&mut self) {
        self.conceal_probe.record_region_cache_hit();
    }

    pub(in crate::events) fn record_conceal_region_cache_miss(&mut self) {
        self.conceal_probe.record_region_cache_miss();
    }

    pub(in crate::events) fn record_conceal_screen_cell_cache_hit(&mut self) {
        self.conceal_probe.record_screen_cell_cache_hit();
    }

    pub(in crate::events) fn record_conceal_screen_cell_cache_miss(&mut self) {
        self.conceal_probe.record_screen_cell_cache_miss();
    }

    pub(in crate::events) fn record_conceal_full_scan(&mut self) {
        self.conceal_probe.record_full_scan();
    }

    pub(in crate::events) fn record_conceal_deferred_projection(&mut self) {
        self.conceal_probe.record_deferred_projection();
    }

    pub(in crate::events) fn record_planner_local_query(
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

    pub(in crate::events) fn record_planner_local_query_envelope_area_cells(
        &mut self,
        area_cells: u64,
    ) {
        self.planner
            .record_local_query_envelope_area_cells(area_cells);
    }

    pub(in crate::events) fn record_planner_compiled_query_cells_count(&mut self, count: usize) {
        self.planner.record_compiled_query_cells_count(count);
    }

    pub(in crate::events) fn record_planner_candidate_query_cells_count(&mut self, count: usize) {
        self.planner.record_candidate_query_cells_count(count);
    }

    pub(in crate::events) fn record_planner_compiled_cells_emitted_count(&mut self, count: usize) {
        self.planner.record_compiled_cells_emitted_count(count);
    }

    pub(in crate::events) fn record_planner_candidate_cells_built_count(&mut self, count: usize) {
        self.planner.record_candidate_cells_built_count(count);
    }

    pub(in crate::events) fn record_planner_reference_compile(&mut self) {
        self.planner.record_reference_compile();
    }

    pub(in crate::events) fn record_planner_local_query_compile(&mut self) {
        self.planner.record_local_query_compile();
    }
}

#[cfg(feature = "perf-counters")]
impl RuntimeBehaviorMetrics {
    pub(in crate::events) fn record_projection_reuse_hit(&mut self) {
        self.planner.record_projection_reuse_hit();
    }

    pub(in crate::events) fn record_projection_reuse_miss(&mut self) {
        self.planner.record_projection_reuse_miss();
    }

    pub(in crate::events) fn record_compiled_field_cache_hit(&mut self) {
        self.planner.record_compiled_field_cache_hit();
    }

    pub(in crate::events) fn record_compiled_field_cache_miss(&mut self) {
        self.planner.record_compiled_field_cache_miss();
    }
}

impl RuntimeBehaviorMetrics {
    #[cfg(feature = "perf-counters")]
    pub(in crate::events) fn record_planning_preview_invocation(&mut self) {
        self.planning_preview.record_invocation();
    }

    #[cfg(feature = "perf-counters")]
    pub(in crate::events) fn record_planning_preview_copied_particles(
        &mut self,
        particle_count: usize,
    ) {
        self.planning_preview
            .record_copied_particles(particle_count);
    }

    #[cfg(feature = "perf-counters")]
    pub(in crate::events) fn record_planning_preview_copy(&mut self, particle_count: usize) {
        self.planning_preview.record_copy(particle_count);
    }

    #[cfg(feature = "perf-counters")]
    pub(in crate::events) fn record_particle_simulation_step(&mut self, particle_count: usize) {
        self.particle_path.record_simulation_step(particle_count);
    }

    #[cfg(feature = "perf-counters")]
    pub(in crate::events) fn record_particle_aggregation(&mut self, particle_count: usize) {
        self.particle_path.record_aggregation(particle_count);
    }

    #[cfg(feature = "perf-counters")]
    pub(in crate::events) fn record_particle_overlay_refresh(&mut self, cell_count: usize) {
        self.particle_path.record_overlay_refresh(cell_count);
    }
}

#[cfg(feature = "perf-counters")]
impl RuntimeBehaviorMetrics {
    pub(in crate::events) fn record_buffer_metadata_read(&mut self) {
        self.validation_reads.record_buffer_metadata_read();
    }

    pub(in crate::events) fn record_editor_bounds_read(&mut self) {
        self.validation_reads.record_editor_bounds_read();
    }

    pub(in crate::events) fn record_command_row_read(&mut self) {
        self.validation_reads.record_command_row_read();
    }
}

#[cfg(feature = "perf-counters")]
impl RuntimeBehaviorMetrics {
    pub(in crate::events) fn record_current_buffer_changedtick_read(&mut self) {
        self.validation_reads
            .record_current_buffer_changedtick_read();
    }
}
