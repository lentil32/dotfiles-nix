use super::projection::prepare_projection;
use super::projection::project_prepared_frame;
use super::projection::reusable_prepared_projection;
use crate::config::RuntimeConfig;
use crate::core::realization::PaletteSpec;
use crate::core::runtime_reducer::RenderAction;
use crate::core::runtime_reducer::RenderDecision;
use crate::core::state::ApplyFailureKind;
use crate::core::state::CursorTrailSemantic;
use crate::core::state::PatchBasis;
use crate::core::state::PlannedProjectionUpdate;
use crate::core::state::PlannedSceneUpdate;
use crate::core::state::ProjectionHandle;
use crate::core::state::RealizationClear;
use crate::core::state::RealizationDivergence;
use crate::core::state::RealizationDraw;
use crate::core::state::RealizationFailure;
use crate::core::state::RealizationPlan;
use crate::core::state::SceneState;
use crate::core::types::MotionRevision;
use crate::core::types::RenderRevision;
use crate::types::RenderFrame;
use std::collections::hash_map::DefaultHasher;
use std::hash::Hash;
use std::hash::Hasher;

#[cfg(test)]
use std::collections::BTreeSet;

#[cfg(test)]
use crate::core::state::SemanticEntityId;

fn hash_f64(hasher: &mut DefaultHasher, value: f64) {
    hasher.write_u64(value.to_bits());
}

fn render_frame_motion_fingerprint(frame: &RenderFrame) -> u64 {
    let mut hasher = DefaultHasher::new();
    frame.mode.hash(&mut hasher);
    frame.planner_idle_steps.hash(&mut hasher);
    frame.vertical_bar.hash(&mut hasher);
    frame.trail_stroke_id.hash(&mut hasher);
    frame.retarget_epoch.hash(&mut hasher);
    frame.particle_count.hash(&mut hasher);

    hash_f64(&mut hasher, frame.target.row);
    hash_f64(&mut hasher, frame.target.col);

    for corner in &frame.corners {
        hash_f64(&mut hasher, corner.row);
        hash_f64(&mut hasher, corner.col);
    }
    for corner in &frame.target_corners {
        hash_f64(&mut hasher, corner.row);
        hash_f64(&mut hasher, corner.col);
    }
    for sample in frame.step_samples.iter() {
        for corner in &sample.corners {
            hash_f64(&mut hasher, corner.row);
            hash_f64(&mut hasher, corner.col);
        }
        hash_f64(&mut hasher, sample.dt_ms);
    }
    for aggregate in frame.aggregated_particle_cells().iter() {
        aggregate.row().hash(&mut hasher);
        aggregate.col().hash(&mut hasher);
        for sub_row in aggregate.cell() {
            for value in sub_row {
                hash_f64(&mut hasher, *value);
            }
        }
    }

    hasher.finish()
}

fn next_motion_revision(
    current_scene: &SceneState,
    next_motion_fingerprint: Option<u64>,
) -> MotionRevision {
    let current_render_revision = current_scene.render_revision();
    if current_scene.last_motion_fingerprint() != next_motion_fingerprint {
        current_render_revision.motion().next()
    } else {
        current_render_revision.motion()
    }
}

fn next_cursor_trail_for_render_decision(
    current: Option<&CursorTrailSemantic>,
    render_decision: &RenderDecision,
) -> Option<CursorTrailSemantic> {
    match &render_decision.render_action {
        RenderAction::Draw(_frame) => Some(CursorTrailSemantic::new(
            render_decision.render_side_effects.target_cell_presentation,
        )),
        RenderAction::ClearAll => None,
        RenderAction::Noop => current.cloned(),
    }
}

fn semantic_cursor_trail_changed(
    previous: Option<&CursorTrailSemantic>,
    next: Option<&CursorTrailSemantic>,
) -> bool {
    previous != next
}

#[cfg(test)]
fn dirty_entities(
    previous: Option<&CursorTrailSemantic>,
    next: Option<&CursorTrailSemantic>,
) -> BTreeSet<SemanticEntityId> {
    if semantic_cursor_trail_changed(previous, next) {
        BTreeSet::from([SemanticEntityId::CursorTrail])
    } else {
        BTreeSet::new()
    }
}

pub(super) fn update_scene_from_render_decision_with_context(
    current_scene: &SceneState,
    observation: Option<&crate::core::effect::RenderPlanningObservation>,
    trusted_projection: Option<&ProjectionHandle>,
    render_decision: &RenderDecision,
) -> (
    PlannedSceneUpdate,
    Option<ProjectionHandle>,
    Option<ApplyFailureKind>,
) {
    let current_render_revision = current_scene.render_revision();
    let next_cursor_trail =
        next_cursor_trail_for_render_decision(current_scene.cursor_trail(), render_decision);
    let semantics_changed =
        semantic_cursor_trail_changed(current_scene.cursor_trail(), next_cursor_trail.as_ref());
    let next_semantic_revision = if semantics_changed {
        current_render_revision.semantics().next()
    } else {
        current_render_revision.semantics()
    };

    match &render_decision.render_action {
        RenderAction::Draw(frame) => {
            let next_motion_fingerprint = Some(render_frame_motion_fingerprint(frame.as_ref()));
            let next_motion_revision = next_motion_revision(current_scene, next_motion_fingerprint);
            let next_render_revision =
                RenderRevision::new(next_motion_revision, next_semantic_revision);

            let Some(trail_semantic) = next_cursor_trail.as_ref() else {
                // Surprising: draw planning lost its semantic trail payload before projection.
                let next_scene = PlannedSceneUpdate::new(
                    next_semantic_revision,
                    next_motion_revision,
                    next_motion_fingerprint,
                    next_cursor_trail,
                    PlannedProjectionUpdate::Replace(None),
                );
                return (next_scene, None, Some(ApplyFailureKind::MissingProjection));
            };
            let Some(observation) = observation else {
                // Surprising: render planning completed without an active observation basis.
                // Keep semantic truth updated, but do not fabricate a projection.
                let next_scene = PlannedSceneUpdate::new(
                    next_semantic_revision,
                    next_motion_revision,
                    next_motion_fingerprint,
                    next_cursor_trail,
                    PlannedProjectionUpdate::Replace(None),
                );
                return (next_scene, None, Some(ApplyFailureKind::MissingProjection));
            };

            let prepared = match prepare_projection(
                current_scene,
                next_render_revision,
                observation,
                frame.as_ref().clone(),
            ) {
                Ok(prepared) => prepared,
                Err(reason) => {
                    let next_scene = PlannedSceneUpdate::new(
                        next_semantic_revision,
                        next_motion_revision,
                        next_motion_fingerprint,
                        next_cursor_trail,
                        PlannedProjectionUpdate::Replace(None),
                    );
                    return (next_scene, None, Some(reason));
                }
            };

            if let Some(reused) =
                reusable_prepared_projection(current_scene, &prepared, trail_semantic)
            {
                crate::events::record_projection_reuse_hit();
                let retained_projection = reused.clone();
                let next_scene = PlannedSceneUpdate::new(
                    next_semantic_revision,
                    next_motion_revision,
                    next_motion_fingerprint,
                    next_cursor_trail,
                    PlannedProjectionUpdate::Replace(Some(reused)),
                );
                return (next_scene, Some(retained_projection), None);
            }
            crate::events::record_projection_reuse_miss();

            let retained_projection = project_prepared_frame(prepared, trail_semantic);
            let next_scene = PlannedSceneUpdate::new(
                next_semantic_revision,
                next_motion_revision,
                next_motion_fingerprint,
                next_cursor_trail,
                PlannedProjectionUpdate::Replace(Some(retained_projection.clone())),
            );
            (next_scene, Some(retained_projection), None)
        }
        RenderAction::ClearAll => {
            let next_motion_revision = next_motion_revision(current_scene, None);
            let next_scene = PlannedSceneUpdate::new(
                next_semantic_revision,
                next_motion_revision,
                None,
                next_cursor_trail,
                PlannedProjectionUpdate::Replace(None),
            );
            (next_scene, None, None)
        }
        RenderAction::Noop => {
            let projection = trusted_projection.cloned();
            let next_scene = PlannedSceneUpdate::new(
                current_render_revision.semantics(),
                current_render_revision.motion(),
                current_scene.last_motion_fingerprint(),
                next_cursor_trail,
                PlannedProjectionUpdate::Keep,
            );
            // the retained scene projection is planner reuse state, not shell authority.
            // A noop proposal may only target the trusted acknowledged render; otherwise cleanup
            // or divergence can leak stale cached draw input into apply as a replace patch.
            (next_scene, projection, None)
        }
    }
}

pub(super) fn patch_basis(
    acknowledged_projection: Option<ProjectionHandle>,
    target: Option<ProjectionHandle>,
) -> PatchBasis {
    PatchBasis::new(acknowledged_projection, target)
}

pub(super) fn realization_plan_for_render_decision(
    config: &RuntimeConfig,
    render_decision: &RenderDecision,
    projection: Option<&ProjectionHandle>,
    projection_failure: Option<ApplyFailureKind>,
) -> RealizationPlan {
    match &render_decision.render_action {
        RenderAction::Draw(frame) => {
            if let Some(reason) = projection_failure {
                return RealizationPlan::Failure(RealizationFailure::new(
                    reason,
                    RealizationDivergence::ShellStateUnknown,
                ));
            }
            if projection.is_none() {
                return RealizationPlan::Failure(RealizationFailure::new(
                    ApplyFailureKind::MissingProjection,
                    RealizationDivergence::ShellStateUnknown,
                ));
            }

            RealizationPlan::Draw(RealizationDraw::new(
                PaletteSpec::from_frame(frame),
                render_decision.render_allocation_policy,
                frame.max_kept_windows,
            ))
        }
        RenderAction::ClearAll => {
            // shell-visible smear occupancy is authoritative for clear intents.
            // Even if projection trust has already degraded to a noop patch basis, a reducer
            // `ClearAll` must still force shell clear work or the last visible trail can survive
            // until unrelated ingress repaints the UI.
            RealizationPlan::Clear(RealizationClear::new(config.max_kept_windows))
        }
        RenderAction::Noop => RealizationPlan::Noop,
    }
}

#[cfg(test)]
mod tests {
    use super::dirty_entities;
    use super::patch_basis;
    use super::realization_plan_for_render_decision;
    use super::update_scene_from_render_decision_with_context;
    use crate::config::RuntimeConfig;
    use crate::core::runtime_reducer::RenderAction;
    use crate::core::runtime_reducer::RenderDecision;
    use crate::core::runtime_reducer::TargetCellPresentation;
    use crate::core::state::CursorTrailSemantic;
    use crate::core::state::RealizationClear;
    use crate::core::state::RealizationDivergence;
    use crate::core::state::RealizationLedger;
    use crate::core::state::RealizationPlan;
    use crate::core::state::ScenePatch;
    use crate::core::state::ScenePatchKind;
    use crate::core::state::SceneState;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;

    use super::super::projection::project_draw_frame;
    use super::super::test_support::alternate_target_cell_presentation;
    use super::super::test_support::base_frame;
    use super::super::test_support::dirty_mutation_axis_strategy;
    use super::super::test_support::observation;
    use super::super::test_support::target_cell_presentation_strategy;
    use crate::core::types::RenderRevision;
    use crate::test_support::proptest::pure_config;

    proptest! {
        #![proptest_config(pure_config())]

        #[test]
        fn prop_dirty_entities_track_semantic_cursor_trail_identity(
            mutation_axis in dirty_mutation_axis_strategy(),
            initial_presentation in target_cell_presentation_strategy(),
            _color_at_cursor in any::<u32>(),
        ) {
            let next_presentation = match mutation_axis {
                super::super::test_support::DirtyMutationAxis::None => initial_presentation,
                super::super::test_support::DirtyMutationAxis::PaletteOnly
                | super::super::test_support::DirtyMutationAxis::Geometry => {
                    initial_presentation
                }
                super::super::test_support::DirtyMutationAxis::Presentation => {
                    alternate_target_cell_presentation(initial_presentation)
                }
            };

            let previous = Some(CursorTrailSemantic::new(initial_presentation));
            let next = Some(CursorTrailSemantic::new(next_presentation));
            let dirty = dirty_entities(previous.as_ref(), next.as_ref());
            let expected_dirty = previous != next;

            prop_assert_eq!(dirty.is_empty(), !expected_dirty);
            if expected_dirty {
                prop_assert_eq!(
                    &dirty,
                    &std::collections::BTreeSet::from([
                        crate::core::state::SemanticEntityId::CursorTrail,
                    ]),
                );
            }

            match mutation_axis {
                super::super::test_support::DirtyMutationAxis::PaletteOnly => {
                    prop_assert!(dirty.is_empty())
                }
                super::super::test_support::DirtyMutationAxis::Presentation => {
                    prop_assert!(!dirty.is_empty())
                }
                super::super::test_support::DirtyMutationAxis::Geometry => {
                    prop_assert!(dirty.is_empty())
                }
                super::super::test_support::DirtyMutationAxis::None => {
                    prop_assert!(dirty.is_empty())
                }
            }
        }
    }

    #[test]
    fn clear_action_without_acknowledged_projection_still_uses_clear_realization() {
        let decision = RenderDecision {
            render_action: RenderAction::ClearAll,
            render_cleanup_action: crate::core::runtime_reducer::RenderCleanupAction::NoAction,
            render_allocation_policy:
                crate::core::runtime_reducer::RenderAllocationPolicy::ReuseOnly,
            render_side_effects: crate::core::runtime_reducer::RenderSideEffects::default(),
        };

        let config = RuntimeConfig {
            max_kept_windows: 21,
            ..RuntimeConfig::default()
        };

        assert_eq!(
            realization_plan_for_render_decision(&config, &decision, None, None),
            RealizationPlan::Clear(RealizationClear::new(config.max_kept_windows))
        );
    }

    #[test]
    fn noop_render_action_uses_trusted_acknowledged_target_not_stale_scene_projection() {
        let frame = base_frame();
        let cached_observation = observation(1);
        let cached = project_draw_frame(
            &SceneState::default(),
            RenderRevision::INITIAL,
            &cached_observation,
            &frame,
            TargetCellPresentation::None,
        )
        .expect("projection without probe-gated particles");
        let scene = SceneState::default()
            .with_retained_projection(cached.clone())
            .with_cursor_trail(CursorTrailSemantic::new(TargetCellPresentation::None));
        let realization = RealizationLedger::Diverged {
            last_consistent: Some(cached),
            divergence: RealizationDivergence::ShellStateUnknown,
        };
        let decision = RenderDecision {
            render_action: RenderAction::Noop,
            render_cleanup_action: crate::core::runtime_reducer::RenderCleanupAction::NoAction,
            render_allocation_policy:
                crate::core::runtime_reducer::RenderAllocationPolicy::ReuseOnly,
            render_side_effects: crate::core::runtime_reducer::RenderSideEffects::default(),
        };

        let (scene_update, projection, projection_failure) =
            update_scene_from_render_decision_with_context(
                &scene,
                None,
                realization.trusted_acknowledged_for_patch(),
                &decision,
            );
        let mut next_scene = scene;
        next_scene.apply_planned_update(scene_update);
        let patch = ScenePatch::derive(patch_basis(
            realization.trusted_acknowledged_for_patch().cloned(),
            projection.clone(),
        ));
        let config = RuntimeConfig::default();

        assert!(next_scene.retained_projection().is_some());
        assert!(projection_failure.is_none());
        assert_eq!(projection, None);
        assert_eq!(patch.kind(), ScenePatchKind::Noop);
        assert_eq!(
            realization_plan_for_render_decision(
                &config,
                &decision,
                projection.as_ref(),
                projection_failure,
            ),
            RealizationPlan::Noop
        );
    }
}
