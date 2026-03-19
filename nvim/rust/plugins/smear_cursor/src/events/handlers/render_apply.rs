use super::super::host_bridge::ensure_namespace_id;
use super::super::logging::{hide_real_cursor, trace_lazy, unhide_real_cursor, warn};
use super::super::runtime::{now_ms, to_core_millis};
use super::super::trace::{
    realization_plan_summary, render_cleanup_execution_summary, render_side_effects_summary,
    scene_patch_summary,
};
use crate::core::effect::{
    ApplyRenderCleanupEffect, IngressCursorPresentationEffect, RenderCleanupExecution,
};
use crate::core::event::{EffectFailedEvent, Event as CoreEvent};
use crate::core::realization::realize_logical_raster;
use crate::core::runtime_reducer::{CursorVisibilityEffect, RenderAllocationPolicy};
use crate::core::state::{
    DegradedApplyMetrics, InFlightProposal, ProjectionSnapshot, ProposalExecution,
    RealizationFailure,
};
use crate::draw::{
    AllocationPolicy, ApplyMetrics, ClearPrepaintOverlaysSummary, CompactRenderWindowsSummary,
    PurgeRenderWindowsSummary, clear_active_render_windows, clear_all_prepaint_overlays,
    clear_prepaint_for_current_tab, compact_render_windows, draw_current, editor_bounds,
    prepaint_cursor_block, purge_render_windows, redraw,
};
use nvim_oxi::Result;

#[derive(Clone, Debug, Default)]
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

    pub(super) fn degraded_apply_metrics(&self) -> DegradedApplyMetrics {
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

    pub(super) fn perf_details(&self) -> String {
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

#[derive(Debug, thiserror::Error)]
pub(super) enum ApplyRenderActionError {
    #[error("render shell apply failed: {0}")]
    Shell(#[from] nvim_oxi::Error),
    #[error("render apply viewport drifted")]
    ViewportDrift,
    #[error("failure proposal reached render shell apply")]
    FailureProposalReachedShell(RealizationFailure),
}

pub(crate) fn apply_ingress_cursor_presentation_effect(effect: IngressCursorPresentationEffect) {
    match effect {
        IngressCursorPresentationEffect::HideCursor => {
            hide_real_cursor();
        }
        IngressCursorPresentationEffect::HideCursorAndPrepaint { cell, zindex } => {
            hide_real_cursor();
            match ensure_namespace_id() {
                Ok(namespace_id) => prepaint_cursor_block(namespace_id, cell, zindex),
                Err(err) => warn(&format!(
                    "engine state re-entered while preparing ingress prepaint; skipping overlay: {err}"
                )),
            }
        }
    }
}

pub(crate) fn execute_redraw_cmdline_effect() {
    flush_shell_redraw("redraw");
}

fn flush_shell_redraw(effect_name: &'static str) {
    // render apply already executes on Neovim's scheduled shell edge.
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
    SoftClear {
        render: crate::draw::ClearActiveRenderWindowsSummary,
        prepaint: ClearPrepaintOverlaysSummary,
    },
    CompactToBudget(CompactRenderWindowsSummary),
    HardPurge(PurgeRenderWindowsSummary),
}

impl RenderCleanupOutcome {
    fn action(self) -> crate::core::event::RenderCleanupAppliedAction {
        match self {
            Self::SoftClear { .. } => crate::core::event::RenderCleanupAppliedAction::SoftCleared,
            Self::CompactToBudget(summary) => {
                crate::core::event::RenderCleanupAppliedAction::CompactedToBudget {
                    converged_to_idle: summary.converged_to_idle(),
                }
            }
            Self::HardPurge(_) => crate::core::event::RenderCleanupAppliedAction::HardPurged,
        }
    }

    fn had_visual_change(self) -> bool {
        match self {
            Self::SoftClear { render, prepaint } => {
                render.had_visual_change() || prepaint.had_visual_change()
            }
            Self::CompactToBudget(summary) => summary.had_visual_change(),
            Self::HardPurge(summary) => summary.had_visual_change(),
        }
    }
}

pub(crate) fn execute_core_apply_render_cleanup_effect(
    payload: ApplyRenderCleanupEffect,
) -> Vec<CoreEvent> {
    let observed_at = to_core_millis(now_ms());
    let namespace_id = match ensure_namespace_id() {
        Ok(namespace_id) => namespace_id,
        Err(err) => {
            warn(&format!(
                "engine state re-entered while applying render cleanup; emitting effect failure: {err}"
            ));
            return vec![CoreEvent::EffectFailed(EffectFailedEvent {
                proposal_id: None,
                observed_at,
            })];
        }
    };
    let cleanup_summary = render_cleanup_execution_summary(payload.execution);
    trace_lazy(|| {
        format!(
            "render_cleanup_start observed_at={} execution={} namespace_id={}",
            observed_at.value(),
            cleanup_summary,
            namespace_id,
        )
    });

    let outcome = match payload.execution {
        RenderCleanupExecution::SoftClear { max_kept_windows } => {
            // Surprising: cleanup is a lifecycle edge, not a current-tab affordance. Sweep every
            // tracked prepaint overlay before entering Cooling so stale cursor blocks cannot leak
            // across tab switches after the hot burst ends.
            RenderCleanupOutcome::SoftClear {
                prepaint: clear_all_prepaint_overlays(namespace_id),
                render: clear_active_render_windows(namespace_id, max_kept_windows),
            }
        }
        RenderCleanupExecution::CompactToBudget {
            target_budget,
            max_prune_per_tick,
        } => RenderCleanupOutcome::CompactToBudget(compact_render_windows(
            namespace_id,
            target_budget,
            max_prune_per_tick,
        )),
        RenderCleanupExecution::HardPurge => {
            RenderCleanupOutcome::HardPurge(purge_render_windows(namespace_id))
        }
    };

    if outcome.had_visual_change() {
        // cleanup already runs on the scheduled shell edge, so redraw must
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

    vec![CoreEvent::RenderCleanupApplied(
        crate::core::event::RenderCleanupAppliedEvent {
            observed_at,
            action,
        },
    )]
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

pub(super) fn apply_render_action(
    namespace_id: u32,
    proposal: &InFlightProposal,
) -> std::result::Result<RenderExecutionMetrics, ApplyRenderActionError> {
    let realization = proposal.realization();
    let realization_summary = realization_plan_summary(&realization);
    let patch_summary = scene_patch_summary(proposal.patch());
    let render_side_effects = proposal.side_effects();
    let side_effects_summary = render_side_effects_summary(render_side_effects);
    trace_lazy(|| {
        format!(
            "render_apply_start namespace_id={namespace_id} realization={realization_summary} patch={patch_summary} side_effects=({side_effects_summary})"
        )
    });
    clear_prepaint_for_current_tab(namespace_id);
    let mut metrics = RenderExecutionMetrics::default();
    match proposal.execution() {
        ProposalExecution::Draw {
            target_projection,
            realization: draw,
            ..
        } => {
            let projection = target_projection;
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
        ProposalExecution::Clear {
            realization: clear, ..
        } => {
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
        ProposalExecution::Noop { .. } => {
            metrics.had_visual_change = apply_cursor_visibility_effect(
                render_side_effects.cursor_visibility,
                render_side_effects.allow_real_cursor_updates,
            );
        }
        ProposalExecution::Failure { failure, .. } => {
            return Err(ApplyRenderActionError::FailureProposalReachedShell(
                *failure,
            ));
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
        ApplyMetrics, ApplyRenderActionError, RenderCleanupOutcome, RenderExecutionMetrics,
        apply_render_action, draw_release_requires_redraw, viewport_matches_projection_witness,
    };
    use crate::core::realization::LogicalRaster;
    use crate::core::runtime_reducer::RenderSideEffects;
    use crate::core::state::{
        ApplyFailureKind, InFlightProposal, PatchBasis, ProjectionSnapshot, ProjectionWitness,
        RealizationClear, RealizationDivergence, RealizationFailure, ScenePatch,
    };
    use crate::core::types::{
        CursorCol, CursorRow, IngressSeq, ObservationId, ProjectorRevision, ProposalId,
        SceneRevision, ViewportSnapshot,
    };
    use crate::draw::render_plan::{CellOp, Viewport};
    use crate::draw::{
        ClearActiveRenderWindowsSummary, ClearPrepaintOverlaysSummary, CompactRenderWindowsSummary,
        PurgeRenderWindowsSummary,
    };
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
        let proposal = InFlightProposal::clear(
            ProposalId::new(1),
            patch,
            RealizationClear::new(1),
            crate::core::runtime_reducer::RenderCleanupAction::NoAction,
            RenderSideEffects::default(),
            crate::core::state::AnimationSchedule::Idle,
        )
        .expect("clear proposal should accept a noop patch basis");

        let result = apply_render_action(0, &proposal);

        assert!(result.is_ok());
    }

    #[test]
    fn draw_proposal_uses_target_projection_for_noop_patch_basis() {
        let viewport = ViewportSnapshot::new(CursorRow(20), CursorCol(40));
        let projection = projection_snapshot(viewport);
        let patch = ScenePatch::derive(PatchBasis::new(
            Some(projection.clone()),
            Some(projection.clone()),
        ));
        let proposal = InFlightProposal::draw(
            ProposalId::new(1),
            patch,
            crate::core::state::RealizationDraw::new(
                crate::core::realization::PaletteSpec::from_frame(&crate::types::RenderFrame {
                    mode: "n".to_string(),
                    corners: [crate::types::Point { row: 1.0, col: 1.0 }; 4],
                    step_samples: Vec::new().into(),
                    planner_idle_steps: 0,
                    target: crate::types::Point { row: 1.0, col: 1.0 },
                    target_corners: [crate::types::Point { row: 1.0, col: 1.0 }; 4],
                    vertical_bar: false,
                    trail_stroke_id: crate::core::types::StrokeId::new(1),
                    retarget_epoch: 1,
                    particles: Vec::new().into(),
                    color_at_cursor: None,
                    static_config: std::sync::Arc::new(crate::types::StaticRenderConfig {
                        cursor_color: None,
                        cursor_color_insert_mode: None,
                        normal_bg: None,
                        transparent_bg_fallback_color: String::new(),
                        cterm_cursor_colors: None,
                        cterm_bg: None,
                        hide_target_hack: false,
                        max_kept_windows: 32,
                        never_draw_over_target: false,
                        particle_max_lifetime: 0.0,
                        particle_switch_octant_braille: 0.0,
                        particles_over_text: true,
                        color_levels: 16,
                        gamma: 1.0,
                        block_aspect_ratio: crate::config::DEFAULT_BLOCK_ASPECT_RATIO,
                        tail_duration_ms: 0.0,
                        simulation_hz: 0.0,
                        trail_thickness: 0.0,
                        trail_thickness_x: 0.0,
                        spatial_coherence_weight: 0.0,
                        temporal_stability_weight: 0.0,
                        top_k_per_cell: 1,
                        windows_zindex: 1,
                    }),
                }),
                crate::core::runtime_reducer::RenderAllocationPolicy::ReuseOnly,
                32,
            ),
            crate::core::runtime_reducer::RenderCleanupAction::NoAction,
            RenderSideEffects::default(),
            crate::core::state::AnimationSchedule::Idle,
        )
        .expect("noop draw patch should keep the target projection");

        let Some((draw_projection, _)) = proposal.execution().draw_realization() else {
            panic!("expected draw proposal execution");
        };

        assert_eq!(draw_projection, &projection);
    }

    #[test]
    fn render_cleanup_outcome_skips_redraw_for_noop_soft_clear() {
        let outcome = RenderCleanupOutcome::SoftClear {
            render: ClearActiveRenderWindowsSummary::default(),
            prepaint: ClearPrepaintOverlaysSummary::default(),
        };

        assert!(!outcome.had_visual_change());
    }

    #[test]
    fn render_cleanup_outcome_flushes_redraw_for_visible_soft_clear_prepaint() {
        let outcome = RenderCleanupOutcome::SoftClear {
            render: ClearActiveRenderWindowsSummary::default(),
            prepaint: ClearPrepaintOverlaysSummary {
                had_visible_prepaint_before_clear: true,
                cleared_prepaint_overlays: 1,
            },
        };

        assert!(outcome.had_visual_change());
    }

    #[test]
    fn render_cleanup_outcome_maps_compaction_convergence_to_idle_action() {
        let outcome = RenderCleanupOutcome::CompactToBudget(CompactRenderWindowsSummary {
            target_budget: 2,
            total_windows_before: 4,
            total_windows_after: 2,
            closed_visible_windows: 0,
            pruned_windows: 2,
            invalid_removed_windows: 0,
            has_visible_windows_after: false,
            has_pending_work_after: false,
        });

        assert_eq!(
            outcome.action(),
            crate::core::event::RenderCleanupAppliedAction::CompactedToBudget {
                converged_to_idle: true,
            }
        );
    }

    #[test]
    fn render_cleanup_outcome_keeps_cooling_when_visible_windows_survive_compaction() {
        let outcome = RenderCleanupOutcome::CompactToBudget(CompactRenderWindowsSummary {
            target_budget: 2,
            total_windows_before: 4,
            total_windows_after: 2,
            closed_visible_windows: 0,
            pruned_windows: 2,
            invalid_removed_windows: 0,
            has_visible_windows_after: true,
            has_pending_work_after: false,
        });

        assert_eq!(
            outcome.action(),
            crate::core::event::RenderCleanupAppliedAction::CompactedToBudget {
                converged_to_idle: false,
            }
        );
    }

    #[test]
    fn render_cleanup_outcome_keeps_cooling_when_retained_pool_is_still_oversized() {
        let outcome = RenderCleanupOutcome::CompactToBudget(CompactRenderWindowsSummary {
            target_budget: 2,
            total_windows_before: 5,
            total_windows_after: 3,
            closed_visible_windows: 0,
            pruned_windows: 2,
            invalid_removed_windows: 0,
            has_visible_windows_after: false,
            has_pending_work_after: true,
        });

        assert_eq!(
            outcome.action(),
            crate::core::event::RenderCleanupAppliedAction::CompactedToBudget {
                converged_to_idle: false,
            }
        );
    }

    #[test]
    fn render_cleanup_outcome_flushes_redraw_for_visible_compaction_recovery() {
        let outcome = RenderCleanupOutcome::CompactToBudget(CompactRenderWindowsSummary {
            target_budget: 1,
            total_windows_before: 2,
            total_windows_after: 1,
            closed_visible_windows: 1,
            pruned_windows: 0,
            invalid_removed_windows: 0,
            has_visible_windows_after: false,
            has_pending_work_after: false,
        });

        assert!(outcome.had_visual_change());
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

    #[test]
    fn failure_proposal_returns_typed_error_instead_of_panicking() {
        let failure = RealizationFailure::new(
            ApplyFailureKind::MissingProjection,
            RealizationDivergence::ShellStateUnknown,
        );
        let proposal = InFlightProposal::failure(
            ProposalId::new(1),
            ScenePatch::derive(PatchBasis::new(None, None)),
            failure,
            crate::core::runtime_reducer::RenderCleanupAction::NoAction,
            RenderSideEffects::default(),
            crate::core::state::AnimationSchedule::Idle,
        );

        let result = apply_render_action(0, &proposal);

        assert!(matches!(
            result,
            Err(ApplyRenderActionError::FailureProposalReachedShell(actual))
                if actual == failure
        ));
    }
}
