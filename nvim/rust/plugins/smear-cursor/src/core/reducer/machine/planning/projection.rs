use super::render_planning_observation;
use crate::core::realization::project_particle_overlay_cells;
use crate::core::realization::project_render_plan;
use crate::core::runtime_reducer::TargetCellPresentation;
use crate::core::state::ApplyFailureKind;
use crate::core::state::BackgroundProbeBatch;
use crate::core::state::CursorTrailSemantic;
use crate::core::state::ObservationSnapshot;
use crate::core::state::ProjectionHandle;
use crate::core::state::ProjectionPlannerClock;
use crate::core::state::ProjectionReuseKey;
use crate::core::state::ProjectionWitness;
use crate::core::state::RetainedProjection;
use crate::core::state::SceneState;
use crate::core::types::ObservationId;
use crate::core::types::ProjectionPolicyRevision;
use crate::core::types::ProjectorRevision;
use crate::core::types::RenderRevision;
use crate::draw::render_plan;
use crate::draw::render_plan::PlannerState as ProjectionPlannerState;
use crate::position::ViewportBounds;
use crate::types::RenderFrame;
use std::cell::OnceCell;

fn retained_projection_for_reuse(
    scene: &SceneState,
    viewport: ViewportBounds,
    projection_policy_revision: ProjectionPolicyRevision,
) -> Option<&RetainedProjection> {
    let retained_projection = scene.retained_projection()?;
    let witness = retained_projection.witness();
    let reuse_key = retained_projection.reuse_key();
    if witness.viewport() != viewport
        || reuse_key.projection_policy_revision() != projection_policy_revision
    {
        return None;
    }

    Some(retained_projection)
}

fn planner_seed(
    scene: &SceneState,
    projection_policy_revision: ProjectionPolicyRevision,
) -> ProjectionPlannerState {
    // projection snapshots are witness-bound shell inputs, but planner history is part
    // of semantic motion continuity. Reusing planner state across observation/revision drift keeps
    // the ribbon history alive between frames while policy drift still resets the planner.
    scene
        .retained_projection()
        .filter(|retained_projection| {
            retained_projection.reuse_key().projection_policy_revision()
                == projection_policy_revision
        })
        .map_or_else(
            ProjectionPlannerState::default,
            |retained_projection: &RetainedProjection| {
                retained_projection.cached_planner_state().clone()
            },
        )
}

fn planner_clock(planner_state: &ProjectionPlannerState) -> ProjectionPlannerClock {
    ProjectionPlannerClock::new(planner_state.step_index(), planner_state.history_revision())
}

fn frame_advances_planner(frame: &RenderFrame) -> bool {
    !frame.step_samples.is_empty() || frame.planner_idle_steps > 0
}

fn projection_reuse_planner_clock(
    frame: &RenderFrame,
    planner_state: &ProjectionPlannerState,
) -> Option<ProjectionPlannerClock> {
    // planner aging is projection-owned, not semantic-geometry-owned. Advancing frames
    // are only reusable when replayed from the same planner clock; otherwise retained projection
    // reuse would skip latent-field advancement and freeze the tail.
    frame_advances_planner(frame).then(|| planner_clock(planner_state))
}

fn frame_requires_background_probe(frame: &RenderFrame) -> bool {
    !frame.particles_over_text && frame.has_particles()
}

pub(super) struct PreparedProjection<'a> {
    render_revision: RenderRevision,
    observation_id: ObservationId,
    viewport_snapshot: ViewportBounds,
    viewport: ViewportBounds,
    frame: RenderFrame,
    trail_signature: OnceCell<Option<u64>>,
    particle_overlay_signature: OnceCell<Option<u64>>,
    planner_state: ProjectionPlannerState,
    reuse_planner_clock: Option<ProjectionPlannerClock>,
    background_probe: Option<&'a BackgroundProbeBatch>,
}

impl PreparedProjection<'_> {
    fn projection_policy_revision(&self) -> ProjectionPolicyRevision {
        self.frame.projection_policy_revision
    }

    fn trail_signature(&self) -> Option<u64> {
        *self
            .trail_signature
            .get_or_init(|| render_plan::frame_draw_signature(&self.frame))
    }

    fn particle_overlay_signature(&self) -> Option<u64> {
        *self
            .particle_overlay_signature
            .get_or_init(|| render_plan::frame_particle_overlay_signature(&self.frame))
    }

    fn witness(&self) -> ProjectionWitness {
        ProjectionWitness::new(
            self.render_revision,
            self.observation_id,
            self.viewport_snapshot,
            ProjectorRevision::CURRENT,
        )
    }

    fn reuse_key(&self, target_cell_presentation: TargetCellPresentation) -> ProjectionReuseKey {
        ProjectionReuseKey::new(
            self.trail_signature(),
            self.particle_overlay_signature(),
            self.reuse_planner_clock,
            target_cell_presentation,
            self.projection_policy_revision(),
        )
    }
}

pub(super) fn prepare_projection<'a>(
    current_scene: &SceneState,
    render_revision: RenderRevision,
    observation: &'a crate::core::effect::RenderPlanningObservation,
    frame: RenderFrame,
) -> Result<PreparedProjection<'a>, ApplyFailureKind> {
    let background_probe = if frame_requires_background_probe(&frame) {
        let Some(background_probe) = observation.background_probe() else {
            // probe-gated particles may not silently degrade into a
            // "successful" projection. Force an explicit retry path instead.
            return Err(ApplyFailureKind::MissingRequiredProbe);
        };
        Some(background_probe)
    } else {
        None
    };
    let projection_policy_revision = frame.projection_policy_revision;
    let planner_state = planner_seed(current_scene, projection_policy_revision);
    let reuse_planner_clock = projection_reuse_planner_clock(&frame, &planner_state);
    Ok(PreparedProjection {
        render_revision,
        observation_id: observation.observation_id(),
        viewport_snapshot: observation.viewport(),
        viewport: observation.viewport(),
        frame,
        trail_signature: OnceCell::new(),
        particle_overlay_signature: OnceCell::new(),
        planner_state,
        reuse_planner_clock,
        background_probe,
    })
}

pub(super) fn project_prepared_frame(
    prepared: PreparedProjection<'_>,
    trail_semantic: &CursorTrailSemantic,
) -> ProjectionHandle {
    let target_cell_presentation = trail_semantic.target_cell_presentation();
    let trail_signature = prepared.trail_signature();
    let witness = prepared.witness();
    let reuse_key = prepared.reuse_key(target_cell_presentation);
    let mut planner_output = render_plan::render_frame_to_plan_with_signature(
        &prepared.frame,
        prepared.planner_state,
        prepared.viewport,
        trail_signature,
    );
    if let TargetCellPresentation::OverlayCursorCell(shape) = target_cell_presentation {
        planner_output.plan.target_cell_overlay =
            render_plan::plan_target_cell_overlay(&prepared.frame, prepared.viewport, shape);
    }

    let logical_raster = project_render_plan(
        &planner_output.plan,
        prepared.viewport,
        prepared.background_probe,
    );
    RetainedProjection::new(
        witness,
        reuse_key,
        planner_output.next_state,
        logical_raster,
    )
    .into_handle()
}

pub(super) fn reusable_prepared_projection(
    current_scene: &SceneState,
    prepared: &PreparedProjection<'_>,
    trail_semantic: &CursorTrailSemantic,
) -> Option<ProjectionHandle> {
    let target_cell_presentation = trail_semantic.target_cell_presentation();
    let projection_policy_revision = prepared.projection_policy_revision();
    let witness = prepared.witness();
    let reuse_key = prepared.reuse_key(target_cell_presentation);
    let retained_projection = retained_projection_for_reuse(
        current_scene,
        witness.viewport(),
        projection_policy_revision,
    )?;
    let retained_witness = retained_projection.witness();
    let retained_reuse_key = retained_projection.reuse_key();

    if retained_reuse_key.planner_clock() != reuse_key.planner_clock()
        || retained_reuse_key.target_cell_presentation() != target_cell_presentation
    {
        return None;
    }
    if retained_reuse_key.trail_signature() != reuse_key.trail_signature() {
        return None;
    }

    let refresh_particle_overlay = retained_reuse_key.particle_overlay_signature()
        != reuse_key.particle_overlay_signature()
        || (prepared.background_probe.is_some()
            && retained_witness.observation_id() != witness.observation_id());
    let retained_projection = if refresh_particle_overlay {
        let particle_cells = project_particle_overlay_cells(
            &prepared.frame,
            prepared.viewport,
            prepared.background_probe,
        );
        crate::events::record_particle_overlay_refresh(particle_cells.len());
        retained_projection.with_replaced_particle_cells(witness, reuse_key, particle_cells)
    } else {
        retained_projection.rebind_snapshot(witness, reuse_key)
    };

    Some(retained_projection.into_handle())
}

#[cfg(test)]
pub(super) fn project_draw_frame(
    current_scene: &SceneState,
    render_revision: RenderRevision,
    observation: &ObservationSnapshot,
    frame: &RenderFrame,
    target_cell_presentation: TargetCellPresentation,
) -> Result<ProjectionHandle, ApplyFailureKind> {
    let observation = render_planning_observation(observation);
    let prepared = prepare_projection(current_scene, render_revision, &observation, frame.clone())?;
    let trail_semantic = CursorTrailSemantic::new(target_cell_presentation);
    Ok(project_prepared_frame(prepared, &trail_semantic))
}

#[cfg(test)]
pub(super) fn reusable_projection(
    current_scene: &SceneState,
    render_revision: RenderRevision,
    observation: &ObservationSnapshot,
    frame: &RenderFrame,
    target_cell_presentation: TargetCellPresentation,
) -> Option<ProjectionHandle> {
    let observation = render_planning_observation(observation);
    let prepared =
        prepare_projection(current_scene, render_revision, &observation, frame.clone()).ok()?;
    let trail_semantic = CursorTrailSemantic::new(target_cell_presentation);
    reusable_prepared_projection(current_scene, &prepared, &trail_semantic)
}

#[cfg(test)]
mod tests {
    use super::planner_seed;
    use super::project_draw_frame;
    use super::reusable_projection;
    use crate::core::runtime_reducer::TargetCellPresentation;
    use crate::core::state::SceneState;
    use crate::core::types::MotionRevision;
    use crate::core::types::RenderRevision;
    use crate::core::types::SemanticRevision;
    use crate::draw::render_plan::PlannerState as ProjectionPlannerState;
    use crate::position::RenderPoint;
    use crate::types::Particle;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;
    use std::sync::Arc;

    use super::super::test_support::base_frame;
    use super::super::test_support::frame_with_background_probe_requirement;
    use super::super::test_support::frame_with_particle_overlay_drift;
    use super::super::test_support::frame_with_policy_drift;
    use super::super::test_support::observation;
    use super::super::test_support::observation_for_projection;
    use super::super::test_support::projection_policy_revision;
    use super::super::test_support::render_revision;
    use super::super::test_support::reuse_mutation_axis_strategy;
    use super::super::test_support::target_cell_presentation_strategy;
    use crate::test_support::proptest::pure_config;

    proptest! {
        #![proptest_config(pure_config())]

        #[test]
        fn prop_projection_reuse_and_planner_seed_follow_presentation_policy_and_clock_boundaries(
            mutation_axis in reuse_mutation_axis_strategy(),
            advances_planner in any::<bool>(),
            requires_background_probe in any::<bool>(),
            target_cell_presentation in target_cell_presentation_strategy(),
            observation_seq in 1_u64..=(u16::MAX as u64 - 1),
        ) {
            let mut frame = base_frame();
            if !advances_planner {
                frame.step_samples = Vec::new().into();
                frame.planner_idle_steps = 0;
            }
            if requires_background_probe {
                frame = frame_with_background_probe_requirement(frame);
            }

            let cached_policy_revision = projection_policy_revision(&frame);
            let cached_observation =
                observation_for_projection(observation_seq, requires_background_probe);
            let cached = project_draw_frame(
                &SceneState::default(),
                RenderRevision::INITIAL,
                &cached_observation,
                &frame,
                target_cell_presentation,
            )
            .expect("cached projection fixture should be valid");
            let cached_planner_state = cached.cached_planner_state().clone();
            let current_scene = SceneState::default().with_retained_projection(cached);
            let query_frame = match mutation_axis {
                super::super::test_support::ReuseMutationAxis::ParticleOverlay => {
                    frame_with_particle_overlay_drift(frame.clone())
                }
                super::super::test_support::ReuseMutationAxis::Exact
                | super::super::test_support::ReuseMutationAxis::ObservationWitness
                | super::super::test_support::ReuseMutationAxis::MotionRevision
                | super::super::test_support::ReuseMutationAxis::SemanticRevision
                | super::super::test_support::ReuseMutationAxis::Presentation
                | super::super::test_support::ReuseMutationAxis::Policy => frame.clone(),
            };
            let query_observation = match mutation_axis {
                super::super::test_support::ReuseMutationAxis::ObservationWitness => {
                    observation_for_projection(
                        observation_seq.saturating_add(1),
                        requires_background_probe,
                    )
                }
                super::super::test_support::ReuseMutationAxis::Exact
                | super::super::test_support::ReuseMutationAxis::MotionRevision
                | super::super::test_support::ReuseMutationAxis::SemanticRevision
                | super::super::test_support::ReuseMutationAxis::ParticleOverlay
                | super::super::test_support::ReuseMutationAxis::Presentation
                | super::super::test_support::ReuseMutationAxis::Policy => cached_observation,
            };
            let render_revision = match mutation_axis {
                super::super::test_support::ReuseMutationAxis::MotionRevision => {
                    render_revision(MotionRevision::INITIAL.next(), SemanticRevision::INITIAL)
                }
                super::super::test_support::ReuseMutationAxis::SemanticRevision => {
                    render_revision(MotionRevision::INITIAL, SemanticRevision::INITIAL.next())
                }
                super::super::test_support::ReuseMutationAxis::Exact
                | super::super::test_support::ReuseMutationAxis::ObservationWitness
                | super::super::test_support::ReuseMutationAxis::ParticleOverlay
                | super::super::test_support::ReuseMutationAxis::Presentation
                | super::super::test_support::ReuseMutationAxis::Policy => RenderRevision::INITIAL,
            };
            let query_presentation = match mutation_axis {
                super::super::test_support::ReuseMutationAxis::Presentation => {
                    super::super::test_support::alternate_target_cell_presentation(
                        target_cell_presentation,
                    )
                }
                super::super::test_support::ReuseMutationAxis::Exact
                | super::super::test_support::ReuseMutationAxis::ObservationWitness
                | super::super::test_support::ReuseMutationAxis::MotionRevision
                | super::super::test_support::ReuseMutationAxis::SemanticRevision
                | super::super::test_support::ReuseMutationAxis::ParticleOverlay
                | super::super::test_support::ReuseMutationAxis::Policy => {
                    target_cell_presentation
                }
            };
            let query_policy_revision = match mutation_axis {
                super::super::test_support::ReuseMutationAxis::Policy => {
                    projection_policy_revision(&frame_with_policy_drift(frame))
                }
                super::super::test_support::ReuseMutationAxis::Exact
                | super::super::test_support::ReuseMutationAxis::ObservationWitness
                | super::super::test_support::ReuseMutationAxis::MotionRevision
                | super::super::test_support::ReuseMutationAxis::SemanticRevision
                | super::super::test_support::ReuseMutationAxis::ParticleOverlay
                | super::super::test_support::ReuseMutationAxis::Presentation => {
                    cached_policy_revision
                }
            };

            let mut query_frame = query_frame;
            if mutation_axis == super::super::test_support::ReuseMutationAxis::Policy {
                query_frame.projection_policy_revision = query_policy_revision;
            }
            let reused = reusable_projection(
                &current_scene,
                render_revision,
                &query_observation,
                &query_frame,
                query_presentation,
            );
            let expected_seed = match mutation_axis {
                super::super::test_support::ReuseMutationAxis::Policy => {
                    ProjectionPlannerState::default()
                }
                super::super::test_support::ReuseMutationAxis::Exact
                | super::super::test_support::ReuseMutationAxis::ObservationWitness
                | super::super::test_support::ReuseMutationAxis::MotionRevision
                | super::super::test_support::ReuseMutationAxis::SemanticRevision
                | super::super::test_support::ReuseMutationAxis::ParticleOverlay
                | super::super::test_support::ReuseMutationAxis::Presentation => {
                    cached_planner_state
                }
            };
            let expected_reuse = matches!(
                mutation_axis,
                super::super::test_support::ReuseMutationAxis::Exact
                    | super::super::test_support::ReuseMutationAxis::ObservationWitness
                    | super::super::test_support::ReuseMutationAxis::MotionRevision
                    | super::super::test_support::ReuseMutationAxis::SemanticRevision
                    | super::super::test_support::ReuseMutationAxis::ParticleOverlay
            ) && !advances_planner;

            prop_assert_eq!(
                planner_seed(&current_scene, query_policy_revision),
                expected_seed
            );
            prop_assert_eq!(reused.is_some(), expected_reuse);

            if let Some(reused) = reused {
                prop_assert_eq!(
                    reused.reuse_key().target_cell_presentation(),
                    target_cell_presentation
                );
                prop_assert_eq!(
                    reused.witness().observation_id(),
                    query_observation.observation_id(),
                );
            }
        }

        #[test]
        fn prop_project_draw_frame_advances_planner_history_across_observations(
            observation_seq in 1_u64..=(u16::MAX as u64 - 1),
            target_cell_presentation in target_cell_presentation_strategy(),
        ) {
            let frame = base_frame();
            let policy_revision = projection_policy_revision(&frame);
            let first_observation = observation(observation_seq);
            let first = project_draw_frame(
                &SceneState::default(),
                RenderRevision::INITIAL,
                &first_observation,
                &frame,
                target_cell_presentation,
            )
            .expect("first projection should succeed");
            let current_scene = SceneState::default().with_retained_projection(first.clone());
            let second = project_draw_frame(
                &current_scene,
                render_revision(MotionRevision::INITIAL.next(), SemanticRevision::INITIAL),
                &observation(observation_seq.saturating_add(1)),
                &frame,
                target_cell_presentation,
            )
            .expect("second projection should succeed");

            prop_assert_eq!(
                planner_seed(&current_scene, policy_revision),
                first.cached_planner_state().clone()
            );
            prop_assert_ne!(first.cached_planner_state(), second.cached_planner_state());
        }
    }

    #[test]
    fn reusable_projection_refreshes_particle_overlay_without_replanning_the_trail() {
        let mut cached_frame = base_frame();
        cached_frame.step_samples = Vec::new().into();
        cached_frame.planner_idle_steps = 0;
        cached_frame.set_particles(Arc::new(vec![Particle {
            position: RenderPoint {
                row: 10.25,
                col: 10.25,
            },
            velocity: RenderPoint::ZERO,
            lifetime: 1.0,
        }]));
        let mut query_frame = cached_frame.clone();
        query_frame.set_particles(Arc::new(vec![Particle {
            position: RenderPoint {
                row: 10.75,
                col: 11.25,
            },
            velocity: RenderPoint::ZERO,
            lifetime: 0.8,
        }]));

        let cached_observation = observation(1);
        let query_observation = observation(2);
        let cached = project_draw_frame(
            &SceneState::default(),
            RenderRevision::INITIAL,
            &cached_observation,
            &cached_frame,
            TargetCellPresentation::None,
        )
        .expect("cached particle projection should succeed");
        let current_scene = SceneState::default().with_retained_projection(cached);
        let reused = reusable_projection(
            &current_scene,
            render_revision(MotionRevision::INITIAL.next(), SemanticRevision::INITIAL),
            &query_observation,
            &query_frame,
            TargetCellPresentation::None,
        )
        .expect("particle-only drift should still reuse the cached trail");
        let fully_reprojected = project_draw_frame(
            &current_scene,
            render_revision(MotionRevision::INITIAL.next(), SemanticRevision::INITIAL),
            &query_observation,
            &query_frame,
            TargetCellPresentation::None,
        )
        .expect("full particle reprojection should succeed");

        assert_eq!(reused.semantic_view(), fully_reprojected.semantic_view());
        assert_eq!(
            reused.cached_realization().static_spans().as_ptr(),
            current_scene
                .retained_projection()
                .expect("cached retained projection")
                .cached_realization()
                .static_spans()
                .as_ptr(),
        );
        assert_eq!(
            reused.reuse_key().trail_signature(),
            fully_reprojected.reuse_key().trail_signature()
        );
        assert_eq!(
            reused.reuse_key().particle_overlay_signature(),
            fully_reprojected.reuse_key().particle_overlay_signature()
        );
    }

    #[test]
    fn stripped_projection_reuse_cache_preserves_projection_output_for_same_frame() {
        let mut cached_frame = base_frame();
        cached_frame.step_samples = Vec::new().into();
        cached_frame.planner_idle_steps = 0;
        let cached_observation = observation(1);
        let cached = project_draw_frame(
            &SceneState::default(),
            RenderRevision::INITIAL,
            &cached_observation,
            &cached_frame,
            TargetCellPresentation::None,
        )
        .expect("cached projection fixture should be valid");
        let warm_scene = SceneState::default().with_retained_projection(cached);
        let query_observation = observation(2);
        let warm_projection = reusable_projection(
            &warm_scene,
            render_revision(MotionRevision::INITIAL.next(), SemanticRevision::INITIAL),
            &query_observation,
            &cached_frame,
            TargetCellPresentation::None,
        )
        .expect("warm projection cache should be reusable");
        let stripped_projection = project_draw_frame(
            &SceneState::default(),
            render_revision(MotionRevision::INITIAL.next(), SemanticRevision::INITIAL),
            &query_observation,
            &cached_frame,
            TargetCellPresentation::None,
        )
        .expect("projection recompute should succeed without reuse cache");

        assert_eq!(
            warm_projection.semantic_view(),
            stripped_projection.semantic_view()
        );
        assert_eq!(
            warm_projection.shell_projection(),
            stripped_projection.shell_projection()
        );
    }
}
