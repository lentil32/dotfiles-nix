use super::super::host_bridge::ensure_namespace_id;
use super::super::logging::hide_real_cursor;
use super::super::logging::trace_lazy;
use super::super::logging::unhide_real_cursor;
use super::super::logging::warn;
use super::super::runtime::now_ms;
use super::super::runtime::to_core_millis;
use super::super::trace::realization_plan_summary;
use super::super::trace::render_cleanup_execution_summary;
use super::super::trace::render_side_effects_summary;
use super::super::trace::scene_patch_summary;
mod telemetry;
use self::telemetry::RenderExecutionMetrics;
use self::telemetry::draw_projection_debug_summary;
use crate::core::effect::ApplyRenderCleanupEffect;
use crate::core::effect::IngressCursorPresentationEffect;
use crate::core::effect::RenderCleanupExecution;
use crate::core::event::EffectFailedEvent;
use crate::core::event::Event as CoreEvent;
use crate::core::runtime_reducer::CursorVisibilityEffect;
use crate::core::runtime_reducer::RenderAllocationPolicy;
use crate::core::state::InFlightProposal;
use crate::core::state::ProjectionSnapshot;
use crate::core::state::ProposalExecution;
use crate::core::state::RealizationFailure;
use crate::draw::AllocationPolicy;
use crate::draw::ApplyMetrics;
use crate::draw::ClearPrepaintOverlaysSummary;
use crate::draw::CompactRenderWindowsSummary;
use crate::draw::PurgeRenderWindowsSummary;
use crate::draw::clear_active_render_windows;
use crate::draw::clear_all_prepaint_overlays;
use crate::draw::clear_prepaint_for_current_tab;
use crate::draw::compact_render_windows;
use crate::draw::draw_current;
use crate::draw::editor_bounds;
use crate::draw::prepaint_cursor_cell;
use crate::draw::purge_render_windows;
use crate::draw::redraw;
use nvim_oxi::Result;

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
        IngressCursorPresentationEffect::HideCursorAndPrepaint {
            cell,
            shape,
            zindex,
        } => {
            hide_real_cursor();
            match ensure_namespace_id() {
                Ok(namespace_id) => prepaint_cursor_cell(namespace_id, cell, shape, zindex),
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
            let draw_metrics = draw_current(
                namespace_id,
                draw.palette(),
                projection,
                draw.max_kept_windows(),
                to_draw_allocation_policy(draw.allocation_policy()),
            )?;
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
    use super::ApplyMetrics;
    use super::ApplyRenderActionError;
    use super::RenderCleanupOutcome;
    use super::apply_render_action;
    use super::draw_release_requires_redraw;
    use super::viewport_matches_projection_witness;
    use crate::core::realization::LogicalRaster;
    use crate::core::runtime_reducer::RenderSideEffects;
    use crate::core::state::ApplyFailureKind;
    use crate::core::state::InFlightProposal;
    use crate::core::state::PatchBasis;
    use crate::core::state::ProjectionSnapshot;
    use crate::core::state::ProjectionWitness;
    use crate::core::state::RealizationClear;
    use crate::core::state::RealizationDivergence;
    use crate::core::state::RealizationFailure;
    use crate::core::state::ScenePatch;
    use crate::core::types::CursorCol;
    use crate::core::types::CursorRow;
    use crate::core::types::IngressSeq;
    use crate::core::types::ObservationId;
    use crate::core::types::ProjectorRevision;
    use crate::core::types::ProposalId;
    use crate::core::types::SceneRevision;
    use crate::core::types::ViewportSnapshot;
    use crate::draw::ClearActiveRenderWindowsSummary;
    use crate::draw::ClearPrepaintOverlaysSummary;
    use crate::draw::CompactRenderWindowsSummary;
    use crate::draw::PurgeRenderWindowsSummary;
    use crate::draw::render_plan::CellOp;
    use crate::draw::render_plan::Viewport;
    use crate::test_support::proptest::pure_config;
    use proptest::prelude::*;
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

    fn expected_cleanup_action(
        outcome: RenderCleanupOutcome,
    ) -> crate::core::event::RenderCleanupAppliedAction {
        match outcome {
            RenderCleanupOutcome::SoftClear { .. } => {
                crate::core::event::RenderCleanupAppliedAction::SoftCleared
            }
            RenderCleanupOutcome::CompactToBudget(summary) => {
                crate::core::event::RenderCleanupAppliedAction::CompactedToBudget {
                    converged_to_idle: summary.total_windows_after <= summary.target_budget
                        && !summary.has_visible_windows_after
                        && !summary.has_pending_work_after,
                }
            }
            RenderCleanupOutcome::HardPurge(_) => {
                crate::core::event::RenderCleanupAppliedAction::HardPurged
            }
        }
    }

    fn expected_visual_change(outcome: RenderCleanupOutcome) -> bool {
        match outcome {
            RenderCleanupOutcome::SoftClear { render, prepaint } => {
                (render.had_visible_windows_before_clear
                    && (render.pruned_windows > 0
                        || render.hidden_windows > 0
                        || render.invalid_removed_windows > 0))
                    || (prepaint.had_visible_prepaint_before_clear
                        && prepaint.cleared_prepaint_overlays > 0)
            }
            RenderCleanupOutcome::CompactToBudget(summary) => summary.closed_visible_windows > 0,
            RenderCleanupOutcome::HardPurge(summary) => {
                (summary.had_visible_render_windows_before_purge && summary.purged_windows > 0)
                    || (summary.had_visible_prepaint_before_purge
                        && summary.cleared_prepaint_overlays > 0)
            }
        }
    }

    fn render_cleanup_outcome_strategy() -> impl Strategy<Value = RenderCleanupOutcome> {
        prop_oneof![
            (
                any::<bool>(),
                0_usize..8_usize,
                0_usize..8_usize,
                0_usize..8_usize,
                any::<bool>(),
                0_usize..8_usize,
            )
                .prop_map(
                    |(
                        had_visible_windows_before_clear,
                        pruned_windows,
                        hidden_windows,
                        invalid_removed_windows,
                        had_visible_prepaint_before_clear,
                        cleared_prepaint_overlays,
                    )| {
                        RenderCleanupOutcome::SoftClear {
                            render: ClearActiveRenderWindowsSummary {
                                had_visible_windows_before_clear,
                                pruned_windows,
                                hidden_windows,
                                invalid_removed_windows,
                            },
                            prepaint: ClearPrepaintOverlaysSummary {
                                had_visible_prepaint_before_clear,
                                cleared_prepaint_overlays,
                            },
                        }
                    },
                ),
            (
                0_usize..8_usize,
                0_usize..8_usize,
                0_usize..8_usize,
                0_usize..8_usize,
                any::<bool>(),
                any::<bool>(),
            )
                .prop_map(
                    |(
                        target_budget,
                        total_windows_after,
                        closed_visible_windows,
                        invalid_removed_windows,
                        has_visible_windows_after,
                        has_pending_work_after,
                    )| {
                        RenderCleanupOutcome::CompactToBudget(CompactRenderWindowsSummary {
                            target_budget,
                            total_windows_after,
                            closed_visible_windows,
                            pruned_windows: 0,
                            invalid_removed_windows,
                            has_visible_windows_after,
                            has_pending_work_after,
                        })
                    },
                ),
            (
                any::<bool>(),
                any::<bool>(),
                0_usize..8_usize,
                0_usize..8_usize
            )
                .prop_map(
                    |(
                        had_visible_render_windows_before_purge,
                        had_visible_prepaint_before_purge,
                        purged_windows,
                        cleared_prepaint_overlays,
                    )| {
                        RenderCleanupOutcome::HardPurge(PurgeRenderWindowsSummary {
                            had_visible_render_windows_before_purge,
                            had_visible_prepaint_before_purge,
                            purged_windows,
                            cleared_prepaint_overlays,
                        })
                    },
                ),
        ]
    }

    proptest! {
        #![proptest_config(pure_config())]

        #[test]
        fn prop_viewport_match_depends_only_on_editor_bounds_equality(
            max_row in 0_u16..256_u16,
            max_col in 0_u16..512_u16,
            live_row in -1_i64..258_i64,
            live_col in -1_i64..514_i64,
        ) {
            let projection = projection_snapshot(ViewportSnapshot::new(
                CursorRow(u32::from(max_row)),
                CursorCol(u32::from(max_col)),
            ));
            let live_viewport = Viewport { max_row: live_row, max_col: live_col };

            prop_assert_eq!(
                viewport_matches_projection_witness(&projection, live_viewport),
                live_row == i64::from(max_row) && live_col == i64::from(max_col)
            );
        }

        #[test]
        fn prop_render_cleanup_outcome_matches_apply_outcome_model(
            outcome in render_cleanup_outcome_strategy(),
        ) {
            prop_assert_eq!(outcome.action(), expected_cleanup_action(outcome));
            prop_assert_eq!(outcome.had_visual_change(), expected_visual_change(outcome));
        }
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
                    mode: crate::types::ModeClass::NormalLike,
                    corners: [crate::types::Point { row: 1.0, col: 1.0 }; 4],
                    step_samples: Vec::new().into(),
                    planner_idle_steps: 0,
                    target: crate::types::Point { row: 1.0, col: 1.0 },
                    target_corners: [crate::types::Point { row: 1.0, col: 1.0 }; 4],
                    vertical_bar: false,
                    trail_stroke_id: crate::core::types::StrokeId::new(1),
                    retarget_epoch: 1,
                    particle_count: 0,
                    aggregated_particle_cells: std::sync::Arc::default(),
                    particle_screen_cells: std::sync::Arc::default(),
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
