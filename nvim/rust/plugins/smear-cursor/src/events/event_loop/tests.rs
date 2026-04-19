use super::EventLoopDiagnostics;
use super::EventLoopState;
use super::RuntimeBehaviorMetrics;
use super::diagnostics_snapshot;
use super::record_compiled_field_cache_hit;
use super::record_compiled_field_cache_miss;
use super::record_conceal_full_scan;
use super::record_conceal_raw_screenpos_fallback;
use super::record_conceal_region_cache_hit;
use super::record_conceal_region_cache_miss;
use super::record_conceal_screen_cell_cache_hit;
use super::record_conceal_screen_cell_cache_miss;
use super::record_cursor_color_cache_hit;
use super::record_cursor_color_cache_miss;
use super::record_cursor_color_reuse;
use super::record_delayed_ingress_pending_update_count;
use super::record_host_timer_rearm;
use super::record_ingress_coalesced_count;
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
use super::record_probe_refresh_budget_exhausted_count;
use super::record_probe_refresh_retried_count;
use super::record_projection_reuse_hit;
use super::record_projection_reuse_miss;
use super::record_scheduled_drain_items;
use super::record_scheduled_drain_items_for_thermal;
use super::record_scheduled_drain_reschedule;
use super::record_scheduled_drain_reschedule_for_thermal;
use super::record_scheduled_queue_depth;
use super::record_scheduled_queue_depth_for_thermal;
use super::record_stale_token_event_count;
use super::record_timer_fire_duration;
use super::record_timer_schedule_duration;
use super::with_event_loop_state_for_test;
use crate::core::effect::TimerKind;
use crate::core::state::ProbeKind;
use crate::core::state::ProbeReuse;
use crate::core::state::RenderThermalState;
use crate::core::types::Millis;
use crate::test_support::proptest::pure_config;
use pretty_assertions::assert_eq;
use proptest::collection::vec;
use proptest::prelude::*;

const PERF_COUNTERS_ENABLED: bool = cfg!(feature = "perf-counters");

fn reset_event_loop_state() {
    with_event_loop_state_for_test(|state| *state = EventLoopState::new());
}

#[derive(Clone, Copy, Debug)]
enum TelemetryOp {
    ProbeDuration {
        kind: ProbeKind,
        duration_micros: u64,
    },
    ProbeRefreshRetriedCount {
        kind: ProbeKind,
        count: usize,
    },
    ProbeRefreshBudgetExhaustedCount {
        kind: ProbeKind,
        count: usize,
    },
    ProbeExtmarkFallback {
        kind: ProbeKind,
    },
    CursorColorCacheHit,
    CursorColorCacheMiss,
    CursorColorReuse {
        reuse: ProbeReuse,
    },
    ConcealRegionCacheHit,
    ConcealRegionCacheMiss,
    ConcealScreenCellCacheHit,
    ConcealScreenCellCacheMiss,
    ConcealFullScan,
    ConcealRawScreenposFallback,
    TimerScheduleDuration {
        duration_micros: u64,
    },
    TimerFireDuration {
        duration_micros: u64,
    },
    ScheduledQueueDepth {
        depth: usize,
    },
    ScheduledDrainItems {
        drained_items: usize,
    },
    ScheduledDrainReschedule,
    ScheduledQueueDepthForThermal {
        thermal: RenderThermalState,
        depth: usize,
    },
    ScheduledDrainItemsForThermal {
        thermal: RenderThermalState,
        drained_items: usize,
    },
    ScheduledDrainRescheduleForThermal {
        thermal: RenderThermalState,
    },
    HostTimerRearm {
        kind: TimerKind,
    },
    DelayedIngressPendingUpdateCount {
        count: usize,
    },
    IngressCoalescedCount {
        count: usize,
    },
    StaleTokenEventCount {
        count: usize,
    },
    PlannerLocalQuery {
        bucket_maps_scanned: usize,
        bucket_cells_scanned: usize,
        local_query_cells: usize,
    },
    PlannerLocalQueryEnvelopeAreaCells {
        area_cells: u64,
    },
    PlannerCompiledQueryCellsCount {
        count: usize,
    },
    PlannerCandidateQueryCellsCount {
        count: usize,
    },
    PlannerCompiledCellsEmittedCount {
        count: usize,
    },
    PlannerCandidateCellsBuiltCount {
        count: usize,
    },
    PlannerReferenceCompile,
    PlannerLocalQueryCompile,
    ProjectionReuseHit,
    ProjectionReuseMiss,
    CompiledFieldCacheHit,
    CompiledFieldCacheMiss,
    PostBurstConvergence {
        started_at_ms: u64,
        converged_at_ms: u64,
    },
}

#[derive(Debug, Default)]
struct TelemetryModel {
    metrics: RuntimeBehaviorMetrics,
}

impl TelemetryModel {
    fn apply(&mut self, op: TelemetryOp) {
        match op {
            TelemetryOp::ProbeDuration {
                kind,
                duration_micros,
            } => {
                let probe = self.probe_telemetry_mut(kind);
                probe.duration.samples = probe.duration.samples.saturating_add(1);
                probe.duration.total_micros =
                    probe.duration.total_micros.saturating_add(duration_micros);
                probe.duration.max_micros = probe.duration.max_micros.max(duration_micros);
            }
            TelemetryOp::ProbeRefreshRetriedCount { kind, count } => {
                self.saturating_add_probe_retries(kind, count);
            }
            TelemetryOp::ProbeRefreshBudgetExhaustedCount { kind, count } => {
                self.saturating_add_probe_budget_exhausted(kind, count);
            }
            TelemetryOp::ProbeExtmarkFallback { kind } => {
                let probe = self.probe_telemetry_mut(kind);
                probe.extmark_fallback_calls = probe.extmark_fallback_calls.saturating_add(1);
            }
            TelemetryOp::CursorColorCacheHit => {
                self.metrics.cursor_color_cache.hits =
                    self.metrics.cursor_color_cache.hits.saturating_add(1);
            }
            TelemetryOp::CursorColorCacheMiss => {
                self.metrics.cursor_color_cache.misses =
                    self.metrics.cursor_color_cache.misses.saturating_add(1);
            }
            TelemetryOp::CursorColorReuse { reuse } => match reuse {
                ProbeReuse::Exact => {
                    self.metrics.cursor_color_reuse.exact =
                        self.metrics.cursor_color_reuse.exact.saturating_add(1);
                }
                ProbeReuse::Compatible => {
                    self.metrics.cursor_color_reuse.compatible =
                        self.metrics.cursor_color_reuse.compatible.saturating_add(1);
                }
                ProbeReuse::RefreshRequired => {
                    self.metrics.cursor_color_reuse.refresh_required = self
                        .metrics
                        .cursor_color_reuse
                        .refresh_required
                        .saturating_add(1);
                }
            },
            TelemetryOp::ConcealRegionCacheHit => {
                self.metrics.conceal_probe.region_cache.hits = self
                    .metrics
                    .conceal_probe
                    .region_cache
                    .hits
                    .saturating_add(1);
            }
            TelemetryOp::ConcealRegionCacheMiss => {
                self.metrics.conceal_probe.region_cache.misses = self
                    .metrics
                    .conceal_probe
                    .region_cache
                    .misses
                    .saturating_add(1);
            }
            TelemetryOp::ConcealScreenCellCacheHit => {
                self.metrics.conceal_probe.screen_cell_cache.hits = self
                    .metrics
                    .conceal_probe
                    .screen_cell_cache
                    .hits
                    .saturating_add(1);
            }
            TelemetryOp::ConcealScreenCellCacheMiss => {
                self.metrics.conceal_probe.screen_cell_cache.misses = self
                    .metrics
                    .conceal_probe
                    .screen_cell_cache
                    .misses
                    .saturating_add(1);
            }
            TelemetryOp::ConcealFullScan => {
                self.metrics.conceal_probe.full_scan_calls =
                    self.metrics.conceal_probe.full_scan_calls.saturating_add(1);
            }
            TelemetryOp::ConcealRawScreenposFallback => {
                self.metrics.conceal_probe.raw_screenpos_fallback_calls = self
                    .metrics
                    .conceal_probe
                    .raw_screenpos_fallback_calls
                    .saturating_add(1);
            }
            TelemetryOp::TimerScheduleDuration { duration_micros } => {
                self.record_duration_telemetry(true, duration_micros);
            }
            TelemetryOp::TimerFireDuration { duration_micros } => {
                self.record_duration_telemetry(false, duration_micros);
            }
            TelemetryOp::ScheduledQueueDepth { depth } => {
                record_depth(&mut self.metrics.scheduled_queue_depth, depth);
            }
            TelemetryOp::ScheduledDrainItems { drained_items } => {
                record_depth(&mut self.metrics.scheduled_drain_items, drained_items);
            }
            TelemetryOp::ScheduledDrainReschedule => {
                self.metrics.scheduled_drain_reschedules =
                    self.metrics.scheduled_drain_reschedules.saturating_add(1);
            }
            TelemetryOp::ScheduledQueueDepthForThermal { thermal, depth } => {
                record_thermal_depth(
                    &mut self.metrics.scheduled_queue_depth_by_thermal,
                    thermal,
                    depth,
                );
            }
            TelemetryOp::ScheduledDrainItemsForThermal {
                thermal,
                drained_items,
            } => {
                record_thermal_depth(
                    &mut self.metrics.scheduled_drain_items_by_thermal,
                    thermal,
                    drained_items,
                );
            }
            TelemetryOp::ScheduledDrainRescheduleForThermal { thermal } => {
                record_thermal_count(
                    &mut self.metrics.scheduled_drain_reschedules_by_thermal,
                    thermal,
                );
            }
            TelemetryOp::HostTimerRearm { kind } => {
                self.metrics.host_timer_rearms_total =
                    self.metrics.host_timer_rearms_total.saturating_add(1);
                match kind {
                    TimerKind::Animation => {
                        self.metrics.host_timer_rearms_by_kind.animation = self
                            .metrics
                            .host_timer_rearms_by_kind
                            .animation
                            .saturating_add(1);
                    }
                    TimerKind::Ingress => {
                        self.metrics.host_timer_rearms_by_kind.ingress = self
                            .metrics
                            .host_timer_rearms_by_kind
                            .ingress
                            .saturating_add(1);
                    }
                    TimerKind::Recovery => {
                        self.metrics.host_timer_rearms_by_kind.recovery = self
                            .metrics
                            .host_timer_rearms_by_kind
                            .recovery
                            .saturating_add(1);
                    }
                    TimerKind::Cleanup => {
                        self.metrics.host_timer_rearms_by_kind.cleanup = self
                            .metrics
                            .host_timer_rearms_by_kind
                            .cleanup
                            .saturating_add(1);
                    }
                }
            }
            TelemetryOp::DelayedIngressPendingUpdateCount { count } => {
                saturating_add_u64(
                    &mut self.metrics.delayed_ingress_pending_updates,
                    usize_to_u64(count),
                );
            }
            TelemetryOp::IngressCoalescedCount { count } => {
                saturating_add_u64(&mut self.metrics.ingress_coalesced, usize_to_u64(count));
            }
            TelemetryOp::StaleTokenEventCount { count } => {
                saturating_add_u64(&mut self.metrics.stale_token_events, usize_to_u64(count));
            }
            TelemetryOp::PlannerLocalQuery {
                bucket_maps_scanned,
                bucket_cells_scanned,
                local_query_cells,
            } => {
                saturating_add_u64(
                    &mut self.metrics.planner.bucket_maps_scanned,
                    usize_to_u64(bucket_maps_scanned),
                );
                saturating_add_u64(
                    &mut self.metrics.planner.bucket_cells_scanned,
                    usize_to_u64(bucket_cells_scanned),
                );
                saturating_add_u64(
                    &mut self.metrics.planner.local_query_cells,
                    usize_to_u64(local_query_cells),
                );
            }
            TelemetryOp::PlannerLocalQueryEnvelopeAreaCells { area_cells } => {
                saturating_add_u64(
                    &mut self.metrics.planner.local_query_envelope_area_cells,
                    area_cells,
                );
            }
            TelemetryOp::PlannerCompiledQueryCellsCount { count } => {
                saturating_add_u64(
                    &mut self.metrics.planner.compiled_query_cells,
                    usize_to_u64(count),
                );
            }
            TelemetryOp::PlannerCandidateQueryCellsCount { count } => {
                saturating_add_u64(
                    &mut self.metrics.planner.candidate_query_cells,
                    usize_to_u64(count),
                );
            }
            TelemetryOp::PlannerCompiledCellsEmittedCount { count } => {
                saturating_add_u64(
                    &mut self.metrics.planner.compiled_cells_emitted,
                    usize_to_u64(count),
                );
            }
            TelemetryOp::PlannerCandidateCellsBuiltCount { count } => {
                saturating_add_u64(
                    &mut self.metrics.planner.candidate_cells_built,
                    usize_to_u64(count),
                );
            }
            TelemetryOp::PlannerReferenceCompile => {
                self.metrics.planner.reference_compiles =
                    self.metrics.planner.reference_compiles.saturating_add(1);
            }
            TelemetryOp::PlannerLocalQueryCompile => {
                self.metrics.planner.local_query_compiles =
                    self.metrics.planner.local_query_compiles.saturating_add(1);
            }
            TelemetryOp::ProjectionReuseHit => {
                if PERF_COUNTERS_ENABLED {
                    self.metrics.planner.projection_reuse.hits =
                        self.metrics.planner.projection_reuse.hits.saturating_add(1);
                }
            }
            TelemetryOp::ProjectionReuseMiss => {
                if PERF_COUNTERS_ENABLED {
                    self.metrics.planner.projection_reuse.misses = self
                        .metrics
                        .planner
                        .projection_reuse
                        .misses
                        .saturating_add(1);
                }
            }
            TelemetryOp::CompiledFieldCacheHit => {
                if PERF_COUNTERS_ENABLED {
                    self.metrics.planner.compiled_field_cache.hits = self
                        .metrics
                        .planner
                        .compiled_field_cache
                        .hits
                        .saturating_add(1);
                }
            }
            TelemetryOp::CompiledFieldCacheMiss => {
                if PERF_COUNTERS_ENABLED {
                    self.metrics.planner.compiled_field_cache.misses = self
                        .metrics
                        .planner
                        .compiled_field_cache
                        .misses
                        .saturating_add(1);
                }
            }
            TelemetryOp::PostBurstConvergence {
                started_at_ms,
                converged_at_ms,
            } => {
                let duration_ms = converged_at_ms.saturating_sub(started_at_ms);
                self.metrics.post_burst_convergence.samples = self
                    .metrics
                    .post_burst_convergence
                    .samples
                    .saturating_add(1);
                self.metrics.post_burst_convergence.total_ms = self
                    .metrics
                    .post_burst_convergence
                    .total_ms
                    .saturating_add(duration_ms);
                self.metrics.post_burst_convergence.max_ms =
                    self.metrics.post_burst_convergence.max_ms.max(duration_ms);
                self.metrics.post_burst_convergence.last_ms = duration_ms;
            }
        }
    }

    fn probe_telemetry_mut(&mut self, kind: ProbeKind) -> &mut super::telemetry::ProbeTelemetry {
        match kind {
            ProbeKind::CursorColor => &mut self.metrics.cursor_color_probe,
            ProbeKind::Background => &mut self.metrics.background_probe,
        }
    }

    fn saturating_add_probe_retries(&mut self, kind: ProbeKind, count: usize) {
        let probe = self.probe_telemetry_mut(kind);
        saturating_add_u64(&mut probe.refresh_retries, usize_to_u64(count));
    }

    fn saturating_add_probe_budget_exhausted(&mut self, kind: ProbeKind, count: usize) {
        let probe = self.probe_telemetry_mut(kind);
        saturating_add_u64(&mut probe.refresh_budget_exhausted, usize_to_u64(count));
    }

    fn record_duration_telemetry(&mut self, schedule: bool, duration_micros: u64) {
        let duration = if schedule {
            &mut self.metrics.timer_schedule
        } else {
            &mut self.metrics.timer_fire
        };
        duration.samples = duration.samples.saturating_add(1);
        duration.total_micros = duration.total_micros.saturating_add(duration_micros);
        duration.max_micros = duration.max_micros.max(duration_micros);
    }
}

fn saturating_add_u64(total: &mut u64, delta: u64) {
    *total = total.saturating_add(delta);
}

fn usize_to_u64(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

fn record_depth(depth_telemetry: &mut super::telemetry::DepthTelemetry, depth: usize) {
    let depth = usize_to_u64(depth);
    depth_telemetry.samples = depth_telemetry.samples.saturating_add(1);
    depth_telemetry.total_depth = depth_telemetry.total_depth.saturating_add(depth);
    depth_telemetry.max_depth = depth_telemetry.max_depth.max(depth);
}

fn record_thermal_depth(
    telemetry: &mut super::telemetry::ThermalDepthTelemetry,
    thermal: RenderThermalState,
    depth: usize,
) {
    match thermal {
        RenderThermalState::Hot => record_depth(&mut telemetry.hot, depth),
        RenderThermalState::Cooling => record_depth(&mut telemetry.cooling, depth),
        RenderThermalState::Cold => record_depth(&mut telemetry.cold, depth),
    }
}

fn record_thermal_count(
    telemetry: &mut super::telemetry::ThermalCountTelemetry,
    thermal: RenderThermalState,
) {
    match thermal {
        RenderThermalState::Hot => {
            telemetry.hot = telemetry.hot.saturating_add(1);
        }
        RenderThermalState::Cooling => {
            telemetry.cooling = telemetry.cooling.saturating_add(1);
        }
        RenderThermalState::Cold => {
            telemetry.cold = telemetry.cold.saturating_add(1);
        }
    }
}

fn probe_kind_strategy() -> impl Strategy<Value = ProbeKind> {
    prop_oneof![Just(ProbeKind::CursorColor), Just(ProbeKind::Background)]
}

fn probe_reuse_strategy() -> impl Strategy<Value = ProbeReuse> {
    prop_oneof![
        Just(ProbeReuse::Exact),
        Just(ProbeReuse::Compatible),
        Just(ProbeReuse::RefreshRequired),
    ]
}

fn thermal_state_strategy() -> impl Strategy<Value = RenderThermalState> {
    prop_oneof![
        Just(RenderThermalState::Hot),
        Just(RenderThermalState::Cooling),
        Just(RenderThermalState::Cold),
    ]
}

fn timer_kind_strategy() -> impl Strategy<Value = TimerKind> {
    prop_oneof![
        Just(TimerKind::Animation),
        Just(TimerKind::Ingress),
        Just(TimerKind::Recovery),
        Just(TimerKind::Cleanup),
    ]
}

fn telemetry_op_strategy() -> impl Strategy<Value = TelemetryOp> {
    prop_oneof![
        (probe_kind_strategy(), 0_u64..50_000_u64).prop_map(|(kind, duration_micros)| {
            TelemetryOp::ProbeDuration {
                kind,
                duration_micros,
            }
        }),
        (probe_kind_strategy(), 0_usize..8_usize)
            .prop_map(|(kind, count)| { TelemetryOp::ProbeRefreshRetriedCount { kind, count } }),
        (probe_kind_strategy(), 0_usize..8_usize).prop_map(|(kind, count)| {
            TelemetryOp::ProbeRefreshBudgetExhaustedCount { kind, count }
        }),
        probe_kind_strategy().prop_map(|kind| TelemetryOp::ProbeExtmarkFallback { kind }),
        Just(TelemetryOp::CursorColorCacheHit),
        Just(TelemetryOp::CursorColorCacheMiss),
        probe_reuse_strategy().prop_map(|reuse| TelemetryOp::CursorColorReuse { reuse }),
        Just(TelemetryOp::ConcealRegionCacheHit),
        Just(TelemetryOp::ConcealRegionCacheMiss),
        Just(TelemetryOp::ConcealScreenCellCacheHit),
        Just(TelemetryOp::ConcealScreenCellCacheMiss),
        Just(TelemetryOp::ConcealFullScan),
        Just(TelemetryOp::ConcealRawScreenposFallback),
        (0_u64..50_000_u64)
            .prop_map(|duration_micros| TelemetryOp::TimerScheduleDuration { duration_micros }),
        (0_u64..50_000_u64)
            .prop_map(|duration_micros| TelemetryOp::TimerFireDuration { duration_micros }),
        (0_usize..32_usize).prop_map(|depth| TelemetryOp::ScheduledQueueDepth { depth }),
        (0_usize..32_usize)
            .prop_map(|drained_items| TelemetryOp::ScheduledDrainItems { drained_items }),
        Just(TelemetryOp::ScheduledDrainReschedule),
        (thermal_state_strategy(), 0_usize..32_usize).prop_map(|(thermal, depth)| {
            TelemetryOp::ScheduledQueueDepthForThermal { thermal, depth }
        }),
        (thermal_state_strategy(), 0_usize..32_usize).prop_map(|(thermal, drained_items)| {
            TelemetryOp::ScheduledDrainItemsForThermal {
                thermal,
                drained_items,
            }
        }),
        thermal_state_strategy()
            .prop_map(|thermal| { TelemetryOp::ScheduledDrainRescheduleForThermal { thermal } }),
        timer_kind_strategy().prop_map(|kind| TelemetryOp::HostTimerRearm { kind }),
        (0_usize..8_usize)
            .prop_map(|count| TelemetryOp::DelayedIngressPendingUpdateCount { count }),
        (0_usize..8_usize).prop_map(|count| TelemetryOp::IngressCoalescedCount { count }),
        (0_usize..8_usize).prop_map(|count| TelemetryOp::StaleTokenEventCount { count }),
        (0_usize..8_usize, 0_usize..32_usize, 0_usize..16_usize).prop_map(
            |(bucket_maps_scanned, bucket_cells_scanned, local_query_cells)| {
                TelemetryOp::PlannerLocalQuery {
                    bucket_maps_scanned,
                    bucket_cells_scanned,
                    local_query_cells,
                }
            }
        ),
        (0_u64..128_u64)
            .prop_map(|area_cells| TelemetryOp::PlannerLocalQueryEnvelopeAreaCells { area_cells }),
        (0_usize..16_usize).prop_map(|count| TelemetryOp::PlannerCompiledQueryCellsCount { count }),
        (0_usize..16_usize)
            .prop_map(|count| TelemetryOp::PlannerCandidateQueryCellsCount { count }),
        (0_usize..16_usize)
            .prop_map(|count| TelemetryOp::PlannerCompiledCellsEmittedCount { count }),
        (0_usize..16_usize)
            .prop_map(|count| TelemetryOp::PlannerCandidateCellsBuiltCount { count }),
        Just(TelemetryOp::PlannerReferenceCompile),
        Just(TelemetryOp::PlannerLocalQueryCompile),
        Just(TelemetryOp::ProjectionReuseHit),
        Just(TelemetryOp::ProjectionReuseMiss),
        Just(TelemetryOp::CompiledFieldCacheHit),
        Just(TelemetryOp::CompiledFieldCacheMiss),
        (0_u64..512_u64, 0_u64..512_u64).prop_map(|(started_at_ms, extra_ms)| {
            TelemetryOp::PostBurstConvergence {
                started_at_ms,
                converged_at_ms: started_at_ms.saturating_add(extra_ms),
            }
        }),
    ]
}

fn apply_telemetry_op(op: TelemetryOp) {
    match op {
        TelemetryOp::ProbeDuration {
            kind,
            duration_micros,
        } => record_probe_duration(kind, duration_micros),
        TelemetryOp::ProbeRefreshRetriedCount { kind, count } => {
            record_probe_refresh_retried_count(kind, count);
        }
        TelemetryOp::ProbeRefreshBudgetExhaustedCount { kind, count } => {
            record_probe_refresh_budget_exhausted_count(kind, count);
        }
        TelemetryOp::ProbeExtmarkFallback { kind } => record_probe_extmark_fallback(kind),
        TelemetryOp::CursorColorCacheHit => record_cursor_color_cache_hit(),
        TelemetryOp::CursorColorCacheMiss => record_cursor_color_cache_miss(),
        TelemetryOp::CursorColorReuse { reuse } => record_cursor_color_reuse(reuse),
        TelemetryOp::ConcealRegionCacheHit => record_conceal_region_cache_hit(),
        TelemetryOp::ConcealRegionCacheMiss => record_conceal_region_cache_miss(),
        TelemetryOp::ConcealScreenCellCacheHit => record_conceal_screen_cell_cache_hit(),
        TelemetryOp::ConcealScreenCellCacheMiss => record_conceal_screen_cell_cache_miss(),
        TelemetryOp::ConcealFullScan => record_conceal_full_scan(),
        TelemetryOp::ConcealRawScreenposFallback => record_conceal_raw_screenpos_fallback(),
        TelemetryOp::TimerScheduleDuration { duration_micros } => {
            record_timer_schedule_duration(duration_micros);
        }
        TelemetryOp::TimerFireDuration { duration_micros } => {
            record_timer_fire_duration(duration_micros);
        }
        TelemetryOp::ScheduledQueueDepth { depth } => record_scheduled_queue_depth(depth),
        TelemetryOp::ScheduledDrainItems { drained_items } => {
            record_scheduled_drain_items(drained_items);
        }
        TelemetryOp::ScheduledDrainReschedule => record_scheduled_drain_reschedule(),
        TelemetryOp::ScheduledQueueDepthForThermal { thermal, depth } => {
            record_scheduled_queue_depth_for_thermal(thermal, depth);
        }
        TelemetryOp::ScheduledDrainItemsForThermal {
            thermal,
            drained_items,
        } => record_scheduled_drain_items_for_thermal(thermal, drained_items),
        TelemetryOp::ScheduledDrainRescheduleForThermal { thermal } => {
            record_scheduled_drain_reschedule_for_thermal(thermal);
        }
        TelemetryOp::HostTimerRearm { kind } => record_host_timer_rearm(kind),
        TelemetryOp::DelayedIngressPendingUpdateCount { count } => {
            record_delayed_ingress_pending_update_count(count);
        }
        TelemetryOp::IngressCoalescedCount { count } => record_ingress_coalesced_count(count),
        TelemetryOp::StaleTokenEventCount { count } => record_stale_token_event_count(count),
        TelemetryOp::PlannerLocalQuery {
            bucket_maps_scanned,
            bucket_cells_scanned,
            local_query_cells,
        } => {
            record_planner_local_query(bucket_maps_scanned, bucket_cells_scanned, local_query_cells)
        }
        TelemetryOp::PlannerLocalQueryEnvelopeAreaCells { area_cells } => {
            record_planner_local_query_envelope_area_cells(area_cells);
        }
        TelemetryOp::PlannerCompiledQueryCellsCount { count } => {
            record_planner_compiled_query_cells_count(count);
        }
        TelemetryOp::PlannerCandidateQueryCellsCount { count } => {
            record_planner_candidate_query_cells_count(count);
        }
        TelemetryOp::PlannerCompiledCellsEmittedCount { count } => {
            record_planner_compiled_cells_emitted_count(count);
        }
        TelemetryOp::PlannerCandidateCellsBuiltCount { count } => {
            record_planner_candidate_cells_built_count(count);
        }
        TelemetryOp::PlannerReferenceCompile => record_planner_reference_compile(),
        TelemetryOp::PlannerLocalQueryCompile => record_planner_local_query_compile(),
        TelemetryOp::ProjectionReuseHit => record_projection_reuse_hit(),
        TelemetryOp::ProjectionReuseMiss => record_projection_reuse_miss(),
        TelemetryOp::CompiledFieldCacheHit => record_compiled_field_cache_hit(),
        TelemetryOp::CompiledFieldCacheMiss => record_compiled_field_cache_miss(),
        TelemetryOp::PostBurstConvergence {
            started_at_ms,
            converged_at_ms,
        } => {
            record_post_burst_convergence(Millis::new(started_at_ms), Millis::new(converged_at_ms))
        }
    }
}

fn assert_diagnostics_match_model(diagnostics: EventLoopDiagnostics, model: &TelemetryModel) {
    assert_eq!(diagnostics.metrics, model.metrics);
    assert_eq!(diagnostics.last_autocmd_event_ms, 0.0);
    assert_eq!(diagnostics.last_observation_request_ms, 0.0);
    assert_eq!(diagnostics.callback_duration_ewma_ms, 0.0);
}

proptest! {
    #![proptest_config(pure_config())]

    #[test]
    fn prop_event_loop_telemetry_accumulates_generated_sequences(
        operations in vec(telemetry_op_strategy(), 1..=128),
    ) {
        reset_event_loop_state();
        let mut model = TelemetryModel::default();

        for operation in operations {
            apply_telemetry_op(operation);
            model.apply(operation);
        }

        assert_diagnostics_match_model(diagnostics_snapshot(), &model);
    }
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
