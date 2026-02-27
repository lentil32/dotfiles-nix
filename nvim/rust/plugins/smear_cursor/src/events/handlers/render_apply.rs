use super::super::host_bridge::ensure_namespace_id;
use super::super::logging::{hide_real_cursor, trace_lazy, unhide_real_cursor};
use super::super::runtime::{now_ms, to_core_millis};
use super::super::trace::{
    realization_plan_summary, render_cleanup_execution_summary, render_side_effects_summary,
    scene_patch_summary,
};
use crate::core::effect::{
    ApplyRenderCleanupEffect, IngressCursorPresentationEffect, RenderCleanupExecution,
};
use crate::core::realization::{
    ScenePatchRealization, ScenePatchRealizationError, project_scene_patch, realize_logical_raster,
};
use crate::core::runtime_reducer::{
    CursorVisibilityEffect, RenderAllocationPolicy, RenderSideEffects,
};
use crate::core::state::{DegradedApplyMetrics, ProjectionSnapshot, RealizationPlan, ScenePatch};
use crate::draw::{
    AllocationPolicy, ApplyMetrics, PurgeRenderWindowsSummary, clear_active_render_windows,
    clear_prepaint_for_current_tab, draw_current, editor_bounds, prepaint_cursor_block,
    purge_render_windows, redraw,
};
use nvim_oxi::Result;

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct RenderExecutionMetrics {
    ops_planned: usize,
    ops_applied: usize,
    ops_skipped_capacity: usize,
    windows_created: usize,
    windows_reused: usize,
    reuse_failed_missing_window: usize,
    reuse_failed_reconfigure: usize,
    reuse_failed_missing_buffer: usize,
    windows_pruned: usize,
    windows_hidden: usize,
    windows_invalid_removed: usize,
    windows_recovered: usize,
    pool_total_windows: usize,
    pool_available_windows: usize,
    pool_in_use_windows: usize,
    pool_cached_budget: usize,
    pool_last_frame_demand: usize,
    had_visual_change: bool,
}

impl RenderExecutionMetrics {
    fn merge_apply_metrics(&mut self, metrics: ApplyMetrics) {
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
        }
    }

    pub(super) fn is_degraded_apply(&self) -> bool {
        // Degraded apply is intentionally broad for baseline telemetry:
        // partial apply, capacity drops, reuse failures, or recovery all count.
        (self.ops_applied < self.ops_planned)
            || self.ops_skipped_capacity > 0
            || self.reuse_failed_missing_window > 0
            || self.reuse_failed_reconfigure > 0
            || self.reuse_failed_missing_buffer > 0
            || self.windows_recovered > 0
    }

    pub(super) fn degraded_apply_metrics(self) -> DegradedApplyMetrics {
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

    pub(super) fn perf_details(self) -> String {
        format!(
            "ops_planned={} ops_applied={} ops_skipped_capacity={} windows_created={} windows_reused={} reuse_failed_missing_window={} reuse_failed_reconfigure={} reuse_failed_missing_buffer={} windows_pruned={} windows_hidden={} windows_invalid_removed={} windows_recovered={} pool_total_windows={} pool_available_windows={} pool_in_use_windows={} pool_cached_budget={} pool_last_frame_demand={}",
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
            self.pool_last_frame_demand
        )
    }

    pub(super) fn had_visual_change(&self) -> bool {
        self.had_visual_change
    }
}

fn draw_projection_debug_summary(projection: &ProjectionSnapshot) -> String {
    let logical_cells = projection.logical_raster().cells();
    let logical_sample = if logical_cells.is_empty() {
        "none".to_string()
    } else {
        logical_cells
            .iter()
            .take(8)
            .map(|cell| format!("{}:{}@{}", cell.row, cell.col, cell.zindex))
            .collect::<Vec<_>>()
            .join(",")
    };

    let realized = realize_logical_raster(projection.logical_raster());
    let span_sample = if realized.spans().is_empty() {
        "none".to_string()
    } else {
        realized
            .spans()
            .iter()
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
        "logical_cells={} logical_sample=[{}] realized_spans={} span_sample=[{}]",
        logical_cells.len(),
        logical_sample,
        realized.spans().len(),
        span_sample,
    )
}

#[derive(Debug)]
pub(super) enum ApplyRenderActionError {
    Shell(nvim_oxi::Error),
    ViewportDrift,
}

impl From<nvim_oxi::Error> for ApplyRenderActionError {
    fn from(error: nvim_oxi::Error) -> Self {
        Self::Shell(error)
    }
}

pub(crate) fn apply_ingress_cursor_presentation_effect(
    effect: IngressCursorPresentationEffect,
) -> Result<()> {
    match effect {
        IngressCursorPresentationEffect::HideCursor => {
            hide_real_cursor();
            Ok(())
        }
        IngressCursorPresentationEffect::HideCursorAndPrepaint { cell, zindex } => {
            hide_real_cursor();
            prepaint_cursor_block(ensure_namespace_id(), cell, zindex)
        }
    }
}

pub(crate) fn execute_redraw_cmdline_effect() {
    flush_shell_redraw("redraw");
}

fn flush_shell_redraw(effect_name: &'static str) {
    // Comment: render apply already executes on Neovim's scheduled shell edge.
    // Re-scheduling redraw from here defers visible float removal to a later
    // event-loop turn, which can leave the last smear tail on screen until
    // unrelated ingress wakes the UI again.
    match redraw() {
        Ok(()) => {
            trace_lazy(|| format!("shell_redraw trigger={effect_name} result=ok"));
        }
        Err(err) => {
            trace_lazy(|| format!("shell_redraw trigger={effect_name} result=err error={err}"));
        }
    }
}

fn draw_release_requires_redraw(metrics: &ApplyMetrics) -> bool {
    metrics.hidden_windows > 0 || metrics.invalid_removed_windows > 0
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RenderCleanupOutcome {
    SoftClear(crate::draw::ClearActiveRenderWindowsSummary),
    HardPurge(PurgeRenderWindowsSummary),
}

impl RenderCleanupOutcome {
    fn action(self) -> crate::core::event::RenderCleanupAppliedAction {
        match self {
            Self::SoftClear(_) => crate::core::event::RenderCleanupAppliedAction::SoftCleared,
            Self::HardPurge(_) => crate::core::event::RenderCleanupAppliedAction::HardPurged,
        }
    }

    fn had_visual_change(self) -> bool {
        match self {
            Self::SoftClear(summary) => summary.had_visual_change(),
            Self::HardPurge(summary) => summary.had_visual_change(),
        }
    }
}

pub(crate) fn execute_core_apply_render_cleanup_effect(
    payload: ApplyRenderCleanupEffect,
) -> Result<Vec<crate::core::event::Event>> {
    let observed_at = to_core_millis(now_ms());
    let namespace_id = ensure_namespace_id();
    let cleanup_summary = render_cleanup_execution_summary(payload.execution);
    trace_lazy(|| {
        format!(
            "render_cleanup_start observed_at={} execution={} namespace_id={}",
            observed_at.value(),
            cleanup_summary,
            namespace_id,
        )
    });
    clear_prepaint_for_current_tab(namespace_id);

    let outcome = match payload.execution {
        RenderCleanupExecution::SoftClear { max_kept_windows } => RenderCleanupOutcome::SoftClear(
            clear_active_render_windows(namespace_id, max_kept_windows),
        ),
        RenderCleanupExecution::HardPurge => {
            RenderCleanupOutcome::HardPurge(purge_render_windows(namespace_id))
        }
    };

    if outcome.had_visual_change() {
        // Comment: cleanup already runs on the scheduled shell edge, so redraw must
        // flush inline here. Scheduling it again defers the final disappearance to
        // a later loop turn.
        flush_shell_redraw("render_cleanup_redraw");
    }

    let action = outcome.action();
    trace_lazy(|| {
        format!(
            "render_cleanup_result observed_at={} execution={} action={:?} visual_change={}",
            observed_at.value(),
            cleanup_summary,
            action,
            outcome.had_visual_change(),
        )
    });

    Ok(vec![crate::core::event::Event::RenderCleanupApplied(
        crate::core::event::RenderCleanupAppliedEvent {
            observed_at,
            action,
        },
    )])
}

fn to_draw_allocation_policy(effect: RenderAllocationPolicy) -> AllocationPolicy {
    match effect {
        RenderAllocationPolicy::ReuseOnly => AllocationPolicy::ReuseOnly,
        RenderAllocationPolicy::BootstrapIfPoolEmpty => AllocationPolicy::BootstrapIfPoolEmpty,
    }
}

fn apply_cursor_visibility_effect(
    effect: CursorVisibilityEffect,
    allow_real_cursor_updates: bool,
) -> bool {
    if !allow_real_cursor_updates {
        return false;
    }

    match effect {
        CursorVisibilityEffect::Keep => false,
        CursorVisibilityEffect::Hide => {
            hide_real_cursor();
            true
        }
        CursorVisibilityEffect::Show => {
            unhide_real_cursor();
            true
        }
    }
}

fn viewport_matches_projection_witness(
    projection: &ProjectionSnapshot,
    live_viewport: crate::draw::render_plan::Viewport,
) -> bool {
    let expected_viewport = projection.witness().viewport();
    live_viewport.max_row == i64::from(expected_viewport.max_row.value())
        && live_viewport.max_col == i64::from(expected_viewport.max_col.value())
}

fn live_viewport_matches_projection(projection: &ProjectionSnapshot) -> Result<bool> {
    let live_viewport = editor_bounds()?;
    Ok(viewport_matches_projection_witness(
        projection,
        live_viewport,
    ))
}

fn draw_realization_target_projection(
    patch: &ScenePatch,
) -> std::result::Result<&ProjectionSnapshot, ApplyRenderActionError> {
    match project_scene_patch(patch) {
        Ok(ScenePatchRealization::Draw(projection)) => Ok(projection),
        Ok(ScenePatchRealization::Noop) => {
            // Comment: reducer `Draw` is a shell-authoritative frame boundary, not merely a
            // projection diff. If the target projection equals the acknowledged one, the patch
            // kind collapses to `Noop`, but shell apply must still consume the target snapshot so
            // pooled smear windows can roll epochs and release/hide the previous frame.
            patch.basis().target().ok_or_else(|| {
                ApplyRenderActionError::Shell(nvim_oxi::Error::Api(nvim_oxi::api::Error::Other(
                    "draw realization reached shell apply without target projection".into(),
                )))
            })
        }
        Ok(ScenePatchRealization::Clear) => Err(ApplyRenderActionError::Shell(
            nvim_oxi::Error::Api(nvim_oxi::api::Error::Other(
                "draw realization reached shell apply with clear patch".into(),
            )),
        )),
        Err(ScenePatchRealizationError::MissingTargetProjection) => {
            Err(ApplyRenderActionError::Shell(nvim_oxi::Error::Api(
                nvim_oxi::api::Error::Other("replace patch missing target projection".into()),
            )))
        }
    }
}

pub(super) fn apply_render_action(
    namespace_id: u32,
    patch: &ScenePatch,
    realization: RealizationPlan,
    render_side_effects: RenderSideEffects,
) -> std::result::Result<RenderExecutionMetrics, ApplyRenderActionError> {
    let realization_summary = realization_plan_summary(&realization);
    let patch_summary = scene_patch_summary(patch);
    let side_effects_summary = render_side_effects_summary(render_side_effects);
    trace_lazy(|| {
        format!(
            "render_apply_start namespace_id={namespace_id} realization={realization_summary} patch={patch_summary} side_effects=({side_effects_summary})"
        )
    });
    clear_prepaint_for_current_tab(namespace_id);
    let mut metrics = RenderExecutionMetrics::default();
    match realization {
        RealizationPlan::Draw(draw) => {
            let projection = draw_realization_target_projection(patch)?;
            trace_lazy(|| {
                format!(
                    "render_projection_debug namespace_id={} {}",
                    namespace_id,
                    draw_projection_debug_summary(projection),
                )
            });
            if !live_viewport_matches_projection(projection)? {
                return Err(ApplyRenderActionError::ViewportDrift);
            }
            let draw_result = draw_current(
                namespace_id,
                draw.palette(),
                projection,
                draw.max_kept_windows(),
                to_draw_allocation_policy(draw.allocation_policy()),
            )?;
            let draw_metrics = draw_result.metrics;
            let draw_release_redraw = draw_release_requires_redraw(&draw_metrics);
            metrics.merge_apply_metrics(draw_metrics);
            if draw_release_redraw && !render_side_effects.redraw_after_draw_if_cmdline {
                // Surprising: terminal drain can end via an empty draw projection that hides
                // the last smear windows through release/reuse cleanup rather than an explicit
                // clear realization. Flush that hide immediately instead of waiting for later input.
                flush_shell_redraw("draw_release_redraw");
            }
            metrics.had_visual_change = metrics.had_visual_change
                || apply_cursor_visibility_effect(
                    render_side_effects.cursor_visibility,
                    render_side_effects.allow_real_cursor_updates,
                );
        }
        RealizationPlan::Clear(clear) => {
            match project_scene_patch(patch) {
                Ok(ScenePatchRealization::Clear | ScenePatchRealization::Noop) => {}
                Ok(ScenePatchRealization::Draw(_)) => {
                    return Err(ApplyRenderActionError::Shell(nvim_oxi::Error::Api(
                        nvim_oxi::api::Error::Other(
                            "clear realization reached shell apply with draw patch".into(),
                        ),
                    )));
                }
                Err(ScenePatchRealizationError::MissingTargetProjection) => {
                    return Err(ApplyRenderActionError::Shell(nvim_oxi::Error::Api(
                        nvim_oxi::api::Error::Other(
                            "replace patch missing target projection".into(),
                        ),
                    )));
                }
            }
            let clear_summary = clear_active_render_windows(namespace_id, clear.max_kept_windows());
            metrics.windows_pruned = metrics
                .windows_pruned
                .saturating_add(clear_summary.pruned_windows);
            metrics.windows_hidden = metrics
                .windows_hidden
                .saturating_add(clear_summary.hidden_windows);
            metrics.windows_invalid_removed = metrics
                .windows_invalid_removed
                .saturating_add(clear_summary.invalid_removed_windows);
            metrics.had_visual_change = metrics.had_visual_change
                || clear_summary.had_visual_change()
                || apply_cursor_visibility_effect(
                    render_side_effects.cursor_visibility,
                    render_side_effects.allow_real_cursor_updates,
                );
        }
        RealizationPlan::Noop => {
            match project_scene_patch(patch) {
                Ok(ScenePatchRealization::Noop) => {}
                Ok(ScenePatchRealization::Draw(_)) => {
                    return Err(ApplyRenderActionError::Shell(nvim_oxi::Error::Api(
                        nvim_oxi::api::Error::Other(
                            "noop realization reached shell apply with draw patch".into(),
                        ),
                    )));
                }
                Ok(ScenePatchRealization::Clear) => {
                    return Err(ApplyRenderActionError::Shell(nvim_oxi::Error::Api(
                        nvim_oxi::api::Error::Other(
                            "noop realization reached shell apply with clear patch".into(),
                        ),
                    )));
                }
                Err(ScenePatchRealizationError::MissingTargetProjection) => {
                    return Err(ApplyRenderActionError::Shell(nvim_oxi::Error::Api(
                        nvim_oxi::api::Error::Other(
                            "replace patch missing target projection".into(),
                        ),
                    )));
                }
            }
            metrics.had_visual_change = apply_cursor_visibility_effect(
                render_side_effects.cursor_visibility,
                render_side_effects.allow_real_cursor_updates,
            );
        }
        RealizationPlan::Failure(_) => {
            // Comment: the proposal executor converts typed failure plans into
            // `ApplyReported::ApplyFailed` before calling shell apply.
            return Err(ApplyRenderActionError::Shell(nvim_oxi::Error::Api(
                nvim_oxi::api::Error::Other("typed realization failure reached shell apply".into()),
            )));
        }
    }

    trace_lazy(|| {
        format!(
            "render_apply_result namespace_id={} realization={} patch={} visual_change={} metrics={}",
            namespace_id,
            realization_summary,
            patch_summary,
            metrics.had_visual_change(),
            metrics.perf_details(),
        )
    });
    Ok(metrics)
}

#[cfg(test)]
mod tests {
    use super::{
        ApplyMetrics, RenderCleanupOutcome, RenderExecutionMetrics, apply_render_action,
        draw_realization_target_projection, draw_release_requires_redraw,
        viewport_matches_projection_witness,
    };
    use crate::core::realization::LogicalRaster;
    use crate::core::runtime_reducer::RenderSideEffects;
    use crate::core::state::{
        PatchBasis, ProjectionSnapshot, ProjectionWitness, RealizationClear, RealizationPlan,
        ScenePatch,
    };
    use crate::core::types::{
        CursorCol, CursorRow, IngressSeq, ObservationId, ProjectorRevision, SceneRevision,
        ViewportSnapshot,
    };
    use crate::draw::render_plan::{CellOp, Viewport};
    use crate::draw::{ClearActiveRenderWindowsSummary, PurgeRenderWindowsSummary};
    use std::sync::Arc;

    fn projection_snapshot(viewport: ViewportSnapshot) -> ProjectionSnapshot {
        ProjectionSnapshot::new(
            ProjectionWitness::new(
                SceneRevision::INITIAL,
                ObservationId::from_ingress_seq(IngressSeq::new(1)),
                viewport,
                ProjectorRevision::CURRENT,
            ),
            LogicalRaster::new(None, Arc::from(Vec::<CellOp>::new())),
        )
    }

    #[test]
    fn viewport_match_accepts_identical_editor_bounds() {
        let projection = projection_snapshot(ViewportSnapshot::new(CursorRow(20), CursorCol(40)));
        let live_viewport = Viewport {
            max_row: 20,
            max_col: 40,
        };

        assert!(viewport_matches_projection_witness(
            &projection,
            live_viewport,
        ));
    }

    #[test]
    fn viewport_match_rejects_drifted_editor_bounds() {
        let projection = projection_snapshot(ViewportSnapshot::new(CursorRow(20), CursorCol(40)));
        let live_viewport = Viewport {
            max_row: 21,
            max_col: 40,
        };

        assert!(!viewport_matches_projection_witness(
            &projection,
            live_viewport,
        ));
    }

    #[test]
    fn draw_release_requires_redraw_when_apply_hides_windows() {
        let metrics = ApplyMetrics {
            hidden_windows: 1,
            ..ApplyMetrics::default()
        };

        assert!(draw_release_requires_redraw(&metrics));
    }

    #[test]
    fn hidden_window_release_counts_as_visual_change() {
        let mut metrics = RenderExecutionMetrics::default();
        metrics.merge_apply_metrics(ApplyMetrics {
            hidden_windows: 1,
            invalid_removed_windows: 1,
            ..ApplyMetrics::default()
        });

        assert!(metrics.had_visual_change());
        assert_eq!(metrics.windows_hidden, 1);
        assert_eq!(metrics.windows_invalid_removed, 1);
    }

    #[test]
    fn reused_payload_frames_are_not_degraded_when_all_spans_are_satisfied() {
        let mut metrics = RenderExecutionMetrics::default();
        metrics.merge_apply_metrics(ApplyMetrics {
            planned_ops: 2,
            applied_ops: 2,
            reused_windows: 2,
            ..ApplyMetrics::default()
        });

        assert!(!metrics.is_degraded_apply());
    }

    #[test]
    fn clear_realization_accepts_noop_patch_basis() {
        let patch = ScenePatch::derive(PatchBasis::new(None, None));

        let result = apply_render_action(
            0,
            &patch,
            RealizationPlan::Clear(RealizationClear::new(1)),
            RenderSideEffects::default(),
        );

        assert!(result.is_ok());
    }

    #[test]
    fn draw_realization_uses_target_projection_for_noop_patch_basis() {
        let viewport = ViewportSnapshot::new(CursorRow(20), CursorCol(40));
        let projection = projection_snapshot(viewport);
        let patch = ScenePatch::derive(PatchBasis::new(
            Some(projection.clone()),
            Some(projection.clone()),
        ));

        let draw_projection =
            draw_realization_target_projection(&patch).expect("noop draw should keep target");

        assert_eq!(draw_projection, &projection);
    }

    #[test]
    fn render_cleanup_outcome_skips_redraw_for_noop_soft_clear() {
        let outcome = RenderCleanupOutcome::SoftClear(ClearActiveRenderWindowsSummary::default());

        assert!(!outcome.had_visual_change());
    }

    #[test]
    fn render_cleanup_outcome_flushes_redraw_for_visible_hard_purge() {
        let outcome = RenderCleanupOutcome::HardPurge(PurgeRenderWindowsSummary {
            had_visible_render_windows_before_purge: true,
            had_visible_prepaint_before_purge: false,
            purged_windows: 1,
            cleared_prepaint_overlays: 0,
        });

        assert!(outcome.had_visual_change());
    }
}
