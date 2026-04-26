use crate::core::state::DegradedApplyMetrics;
use crate::core::state::ShellProjection;
use crate::draw::ApplyMetrics;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct RenderExecutionMetrics {
    pub(crate) ops_planned: usize,
    pub(crate) ops_applied: usize,
    pub(crate) ops_skipped_capacity: usize,
    pub(crate) windows_created: usize,
    pub(crate) windows_reused: usize,
    pub(crate) reuse_failed_missing_window: usize,
    pub(crate) reuse_failed_reconfigure: usize,
    pub(crate) reuse_failed_missing_buffer: usize,
    pub(crate) windows_pruned: usize,
    pub(crate) windows_hidden: usize,
    pub(crate) windows_invalid_removed: usize,
    pub(crate) windows_recovered: usize,
    pub(crate) pool_total_windows: usize,
    pub(crate) pool_available_windows: usize,
    pub(crate) pool_in_use_windows: usize,
    pub(crate) pool_cached_budget: usize,
    pub(crate) pool_last_frame_demand: usize,
    pub(crate) pool_peak_total_windows: usize,
    pub(crate) pool_peak_frame_demand: usize,
    pub(crate) pool_peak_requested_capacity: usize,
    pub(crate) pool_capacity_cap_hits: usize,
    pub(crate) retained_cleanup_resources: usize,
    pub(crate) had_visual_change: bool,
}

impl RenderExecutionMetrics {
    pub(crate) fn merge_apply_metrics(&mut self, metrics: ApplyMetrics) {
        let ApplyMetrics {
            planned_ops,
            applied_ops,
            skipped_ops_capacity,
            created_windows,
            reused_windows,
            reuse_failed_missing_window,
            reuse_failed_reconfigure,
            reuse_failed_missing_buffer,
            pruned_windows,
            hidden_windows,
            invalid_removed_windows,
            recovered_windows,
            pool_snapshot,
        } = metrics;
        self.ops_planned = self.ops_planned.saturating_add(planned_ops);
        self.ops_applied = self.ops_applied.saturating_add(applied_ops);
        self.ops_skipped_capacity = self
            .ops_skipped_capacity
            .saturating_add(skipped_ops_capacity);
        self.windows_created = self.windows_created.saturating_add(created_windows);
        self.windows_reused = self.windows_reused.saturating_add(reused_windows);
        self.reuse_failed_missing_window = self
            .reuse_failed_missing_window
            .saturating_add(reuse_failed_missing_window);
        self.reuse_failed_reconfigure = self
            .reuse_failed_reconfigure
            .saturating_add(reuse_failed_reconfigure);
        self.reuse_failed_missing_buffer = self
            .reuse_failed_missing_buffer
            .saturating_add(reuse_failed_missing_buffer);
        self.windows_pruned = self.windows_pruned.saturating_add(pruned_windows);
        self.windows_hidden = self.windows_hidden.saturating_add(hidden_windows);
        self.windows_invalid_removed = self
            .windows_invalid_removed
            .saturating_add(invalid_removed_windows);
        self.windows_recovered = self.windows_recovered.saturating_add(recovered_windows);
        self.had_visual_change = self.had_visual_change
            || applied_ops > 0
            || created_windows > 0
            || pruned_windows > 0
            || hidden_windows > 0
            || invalid_removed_windows > 0
            || recovered_windows > 0;
        if let Some(snapshot) = pool_snapshot {
            self.pool_total_windows = snapshot.total_windows;
            self.pool_available_windows = snapshot.available_windows;
            self.pool_in_use_windows = snapshot.in_use_windows;
            self.pool_cached_budget = snapshot.cached_budget;
            self.pool_last_frame_demand = snapshot.last_frame_demand;
            self.pool_peak_total_windows = snapshot.peak_total_windows;
            self.pool_peak_frame_demand = snapshot.peak_frame_demand;
            self.pool_peak_requested_capacity = snapshot.peak_requested_capacity;
            self.pool_capacity_cap_hits = snapshot.capacity_cap_hits;
        }
    }

    pub(crate) fn is_degraded_apply(&self) -> bool {
        // Degraded apply is intentionally broad for baseline telemetry:
        // partial apply, capacity drops, reuse failures, or recovery all count.
        (self.ops_applied < self.ops_planned)
            || self.ops_skipped_capacity > 0
            || self.reuse_failed_missing_window > 0
            || self.reuse_failed_reconfigure > 0
            || self.reuse_failed_missing_buffer > 0
            || self.windows_recovered > 0
    }

    pub(crate) fn degraded_apply_metrics(&self) -> DegradedApplyMetrics {
        DegradedApplyMetrics::new(
            self.ops_planned,
            self.ops_applied,
            self.ops_skipped_capacity,
            self.reuse_failed_missing_window,
            self.reuse_failed_reconfigure,
            self.reuse_failed_missing_buffer,
            self.windows_recovered,
        )
    }

    pub(crate) fn perf_details(&self) -> String {
        format!(
            "ops_planned={} ops_applied={} ops_skipped_capacity={} windows_created={} windows_reused={} reuse_failed_missing_window={} reuse_failed_reconfigure={} reuse_failed_missing_buffer={} windows_pruned={} windows_hidden={} windows_invalid_removed={} windows_recovered={} pool_total_windows={} pool_available_windows={} pool_in_use_windows={} pool_cached_budget={} pool_last_frame_demand={} pool_peak_total={} pool_peak_demand={} pool_peak_requested={} pool_cap_hits={}",
            self.ops_planned,
            self.ops_applied,
            self.ops_skipped_capacity,
            self.windows_created,
            self.windows_reused,
            self.reuse_failed_missing_window,
            self.reuse_failed_reconfigure,
            self.reuse_failed_missing_buffer,
            self.windows_pruned,
            self.windows_hidden,
            self.windows_invalid_removed,
            self.windows_recovered,
            self.pool_total_windows,
            self.pool_available_windows,
            self.pool_in_use_windows,
            self.pool_cached_budget,
            self.pool_last_frame_demand,
            self.pool_peak_total_windows,
            self.pool_peak_frame_demand,
            self.pool_peak_requested_capacity,
            self.pool_capacity_cap_hits
        )
    }

    pub(crate) fn had_visual_change(&self) -> bool {
        self.had_visual_change
    }
}

pub(super) fn draw_projection_debug_summary(projection: ShellProjection<'_>) -> String {
    let mut logical_cell_count = 0;
    let mut logical_sample_cells = Vec::with_capacity(8);
    for cell in projection.logical_raster().iter_cells() {
        logical_cell_count += 1;
        if logical_sample_cells.len() < 8 {
            logical_sample_cells.push(format!("{}:{}@{}", cell.row, cell.col, cell.zindex));
        }
    }
    let logical_sample = if logical_sample_cells.is_empty() {
        "none".to_string()
    } else {
        logical_sample_cells.join(",")
    };

    let realized = projection.realization();
    let span_count = realized.span_count();
    let span_sample = if span_count == 0 {
        "none".to_string()
    } else {
        realized
            .spans()
            .take(8)
            .map(|span| {
                format!(
                    "{}:{}x{}@{}",
                    span.row(),
                    span.col(),
                    span.width(),
                    span.zindex(),
                )
            })
            .collect::<Vec<_>>()
            .join(",")
    };

    format!(
        "logical_cells={logical_cell_count} logical_sample=[{logical_sample}] realized_spans={span_count} span_sample=[{span_sample}]",
    )
}

#[cfg(test)]
mod tests {
    use super::RenderExecutionMetrics;
    use super::draw_projection_debug_summary;
    use crate::core::realization::LogicalRaster;
    use crate::core::state::DegradedApplyMetrics;
    use crate::core::state::ProjectionReuseKey;
    use crate::core::state::ProjectionWitness;
    use crate::core::state::RetainedProjection;
    use crate::core::types::IngressSeq;
    use crate::core::types::ObservationId;
    use crate::core::types::ProjectionPolicyRevision;
    use crate::core::types::ProjectorRevision;
    use crate::core::types::RenderRevision;
    use crate::draw::ApplyMetrics;
    use crate::draw::TabPoolSnapshot;
    use crate::draw::render_plan::CellOp;
    use crate::draw::render_plan::Glyph;
    use crate::draw::render_plan::HighlightLevel;
    use crate::draw::render_plan::HighlightRef;
    use crate::position::ViewportBounds;
    use insta::assert_snapshot;
    use pretty_assertions::assert_eq;
    use std::sync::Arc;

    fn cell(row: i64, col: i64, zindex: u32) -> CellOp {
        CellOp {
            row,
            col,
            zindex,
            glyph: Glyph::BLOCK,
            highlight: HighlightRef::Normal(HighlightLevel::from_raw_clamped(2)),
        }
    }

    fn retained_projection(cells: Vec<CellOp>) -> RetainedProjection {
        RetainedProjection::new(
            ProjectionWitness::new(
                RenderRevision::INITIAL,
                ObservationId::from_ingress_seq(IngressSeq::new(1)),
                ViewportBounds::new(20, 40).expect("positive viewport bounds"),
                ProjectorRevision::CURRENT,
            ),
            ProjectionReuseKey::new(
                None,
                None,
                None,
                crate::core::runtime_reducer::TargetCellPresentation::None,
                ProjectionPolicyRevision::INITIAL,
            ),
            crate::draw::render_plan::PlannerState::default(),
            LogicalRaster::new(None, Arc::from(cells)),
        )
    }

    #[test]
    fn merge_apply_metrics_accumulates_telemetry_and_marks_visual_change() {
        let mut metrics = RenderExecutionMetrics::default();
        metrics.merge_apply_metrics(ApplyMetrics {
            planned_ops: 2,
            applied_ops: 1,
            skipped_ops_capacity: 3,
            created_windows: 4,
            reused_windows: 5,
            reuse_failed_missing_window: 6,
            reuse_failed_reconfigure: 7,
            reuse_failed_missing_buffer: 8,
            pruned_windows: 9,
            hidden_windows: 10,
            invalid_removed_windows: 11,
            recovered_windows: 12,
            pool_snapshot: None,
        });

        let expected = RenderExecutionMetrics {
            ops_planned: 2,
            ops_applied: 1,
            ops_skipped_capacity: 3,
            windows_created: 4,
            windows_reused: 5,
            reuse_failed_missing_window: 6,
            reuse_failed_reconfigure: 7,
            reuse_failed_missing_buffer: 8,
            windows_pruned: 9,
            windows_hidden: 10,
            windows_invalid_removed: 11,
            windows_recovered: 12,
            pool_total_windows: 0,
            pool_available_windows: 0,
            pool_in_use_windows: 0,
            pool_cached_budget: 0,
            pool_last_frame_demand: 0,
            pool_peak_total_windows: 0,
            pool_peak_frame_demand: 0,
            pool_peak_requested_capacity: 0,
            pool_capacity_cap_hits: 0,
            retained_cleanup_resources: 0,
            had_visual_change: true,
        };

        assert_eq!(metrics, expected);
    }

    #[test]
    fn merge_apply_metrics_reads_pool_peak_telemetry_from_snapshot() {
        let mut metrics = RenderExecutionMetrics::default();
        metrics.merge_apply_metrics(ApplyMetrics {
            pool_snapshot: Some(TabPoolSnapshot {
                total_windows: 4,
                available_windows: 3,
                in_use_windows: 1,
                cached_budget: 32,
                last_frame_demand: 8,
                peak_total_windows: 14,
                peak_frame_demand: 11,
                peak_requested_capacity: 17,
                capacity_cap_hits: 2,
            }),
            ..ApplyMetrics::default()
        });

        assert_eq!(
            metrics,
            RenderExecutionMetrics {
                pool_total_windows: 4,
                pool_available_windows: 3,
                pool_in_use_windows: 1,
                pool_cached_budget: 32,
                pool_last_frame_demand: 8,
                pool_peak_total_windows: 14,
                pool_peak_frame_demand: 11,
                pool_peak_requested_capacity: 17,
                pool_capacity_cap_hits: 2,
                ..RenderExecutionMetrics::default()
            }
        );
    }

    #[test]
    fn degraded_apply_metrics_preserves_the_baseline_counters() {
        let mut metrics = RenderExecutionMetrics::default();
        metrics.merge_apply_metrics(ApplyMetrics {
            planned_ops: 3,
            applied_ops: 3,
            skipped_ops_capacity: 0,
            created_windows: 0,
            reused_windows: 0,
            reuse_failed_missing_window: 1,
            reuse_failed_reconfigure: 2,
            reuse_failed_missing_buffer: 3,
            pruned_windows: 0,
            hidden_windows: 0,
            invalid_removed_windows: 0,
            recovered_windows: 4,
            pool_snapshot: None,
        });

        assert!(metrics.is_degraded_apply());
        assert_eq!(
            metrics.degraded_apply_metrics(),
            DegradedApplyMetrics::new(3, 3, 0, 1, 2, 3, 4)
        );
    }

    #[test]
    fn perf_details_snapshot_renders_stable_field_order() {
        let mut metrics = RenderExecutionMetrics::default();
        metrics.merge_apply_metrics(ApplyMetrics {
            planned_ops: 2,
            applied_ops: 1,
            skipped_ops_capacity: 3,
            created_windows: 4,
            reused_windows: 5,
            reuse_failed_missing_window: 6,
            reuse_failed_reconfigure: 7,
            reuse_failed_missing_buffer: 8,
            pruned_windows: 9,
            hidden_windows: 10,
            invalid_removed_windows: 11,
            recovered_windows: 12,
            pool_snapshot: None,
        });

        assert_snapshot!(&metrics.perf_details());
    }

    #[test]
    fn projection_debug_summary_snapshot_renders_empty_raster() {
        let snapshot = retained_projection(Vec::new());

        assert_snapshot!(&draw_projection_debug_summary(snapshot.shell_projection()));
    }

    #[test]
    fn projection_debug_summary_snapshot_renders_small_raster() {
        let snapshot = retained_projection(vec![cell(3, 7, 11), cell(3, 8, 11)]);

        assert_snapshot!(&draw_projection_debug_summary(snapshot.shell_projection()));
    }
}
