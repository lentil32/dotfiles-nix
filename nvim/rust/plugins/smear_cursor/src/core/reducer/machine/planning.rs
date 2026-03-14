use super::Transition;
use super::support::request_render_plan_effect;
use crate::core::realization::{PaletteSpec, project_render_plan};
use crate::core::runtime_reducer::{
    CursorEventContext, EventSource, RenderAction, RenderDecision, TargetCellPresentation,
    reduce_cursor_event, select_event_source,
};
use crate::core::state::{
    ApplyFailureKind, CoreState, CursorTrailGeometry, CursorTrailProjectionPolicy,
    CursorTrailSemantic, DirtyEntitySet, InFlightProposal, ObservationSnapshot, PatchBasis,
    PlannedRender, ProjectionCache, ProjectionCacheEntry, ProjectionPlannerClock,
    ProjectionReuseKey, ProjectionSnapshot, ProjectionWitness, RealizationClear,
    RealizationDivergence, RealizationDraw, RealizationFailure, RealizationLedger, RealizationPlan,
    ScenePatch, ScenePatchKind, SceneState, SemanticEntity, SemanticEntityId, SemanticScene,
};
use crate::core::types::{Millis, ProjectorRevision, ProposalId, SceneRevision, ViewportSnapshot};
use crate::draw::render_plan::{self, PlannerState as ProjectionPlannerState, Viewport};
use crate::types::{DEFAULT_RNG_STATE, Point, RenderFrame};

fn point_from_cursor_position(position: crate::core::types::CursorPosition) -> Point {
    Point {
        row: f64::from(position.row.0),
        col: f64::from(position.col.0),
    }
}

fn deterministic_event_seed(observation: &ObservationSnapshot) -> u32 {
    let request = observation.request();
    let observed_at = observation.basis().observed_at().value() as u32;
    let observation_id = request.observation_id().value() as u32;
    let mixed = observation_id ^ observed_at.rotate_left(13) ^ 0x9E37_79B9;
    if mixed == 0 { DEFAULT_RNG_STATE } else { mixed }
}

fn projection_viewport(viewport: ViewportSnapshot) -> Viewport {
    Viewport {
        max_row: i64::from(viewport.max_row.value()),
        max_col: i64::from(viewport.max_col.value()),
    }
}

fn projection_witness(
    scene_revision: SceneRevision,
    observation: &ObservationSnapshot,
) -> ProjectionWitness {
    ProjectionWitness::new(
        scene_revision,
        observation.request().observation_id(),
        observation.basis().viewport(),
        ProjectorRevision::CURRENT,
    )
}

fn projection_entry_for_witness<'a>(
    scene: &'a SceneState,
    witness: ProjectionWitness,
    policy: &CursorTrailProjectionPolicy,
) -> Option<&'a ProjectionCacheEntry> {
    let entry = scene.projection_entry()?;
    if scene.revision() != witness.scene_revision()
        || entry.snapshot().witness() != witness
        || entry.reuse_key().policy() != policy
    {
        return None;
    }

    Some(entry)
}

fn planner_seed(
    scene: &SceneState,
    policy: &CursorTrailProjectionPolicy,
) -> ProjectionPlannerState {
    // Comment: projection snapshots are witness-bound shell inputs, but planner history is part
    // of semantic motion continuity. Reusing planner state across observation/revision drift keeps
    // the ribbon history alive between frames while policy drift still resets the planner.
    scene
        .projection_entry()
        .filter(|entry| entry.reuse_key().policy() == policy)
        .map_or_else(
            ProjectionPlannerState::default,
            |entry: &ProjectionCacheEntry| entry.planner_state().clone(),
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
    // Comment: planner aging is projection-owned, not semantic-geometry-owned. Advancing frames
    // are only reusable when replayed from the same planner clock; otherwise retained projection
    // reuse would skip latent-field advancement and freeze the tail.
    frame_advances_planner(frame).then(|| planner_clock(planner_state))
}

fn next_semantics_for_render_decision(
    current: &SemanticScene,
    render_decision: &RenderDecision,
) -> (SemanticScene, Option<CursorTrailSemantic>) {
    match &render_decision.render_action {
        RenderAction::Draw(frame) => {
            let trail = CursorTrailSemantic::from_render_frame(
                frame.as_ref(),
                render_decision.render_side_effects.target_cell_presentation,
            );
            (
                current
                    .clone()
                    .with_entity(SemanticEntity::CursorTrail(trail.clone())),
                Some(trail),
            )
        }
        RenderAction::ClearAll => (
            current
                .clone()
                .without_entity(SemanticEntityId::CursorTrail),
            None,
        ),
        RenderAction::Noop => (current.clone(), None),
    }
}

fn dirty_entities(previous: &SemanticScene, next: &SemanticScene) -> DirtyEntitySet {
    let previous_entity = previous.entity(SemanticEntityId::CursorTrail);
    let next_entity = next.entity(SemanticEntityId::CursorTrail);
    if previous_entity == next_entity {
        DirtyEntitySet::default()
    } else {
        DirtyEntitySet::default().insert(SemanticEntityId::CursorTrail)
    }
}

fn frame_requires_background_probe(
    geometry: &CursorTrailGeometry,
    policy: &CursorTrailProjectionPolicy,
) -> bool {
    geometry.requires_background_probe(policy)
}

fn project_draw_frame(
    current_scene: &SceneState,
    scene_revision: SceneRevision,
    observation: &ObservationSnapshot,
    geometry: &CursorTrailGeometry,
    policy: &CursorTrailProjectionPolicy,
    target_cell_presentation: TargetCellPresentation,
) -> Result<ProjectionCacheEntry, ApplyFailureKind> {
    let background_probe = if frame_requires_background_probe(geometry, policy) {
        let Some(background_probe) = observation.background_probe() else {
            // Comment: probe-gated particles may not silently degrade into a
            // "successful" projection. Force an explicit retry path instead.
            return Err(ApplyFailureKind::MissingRequiredProbe);
        };
        Some(background_probe)
    } else {
        None
    };
    let witness = projection_witness(scene_revision, observation);
    let planner_frame = geometry.planner_frame(policy);
    let viewport = projection_viewport(observation.basis().viewport());
    let signature = render_plan::frame_draw_signature(&planner_frame);
    let planner_state = planner_seed(current_scene, policy);
    let reuse_planner_clock = projection_reuse_planner_clock(&planner_frame, &planner_state);
    let mut planner_output = render_plan::render_frame_to_plan_with_signature(
        &planner_frame,
        planner_state,
        viewport,
        signature,
    );
    if matches!(
        target_cell_presentation,
        TargetCellPresentation::OverlayBlockCell
    ) {
        planner_output.plan.target_cell_overlay =
            render_plan::plan_target_cell_overlay(&planner_frame, viewport);
    }

    let logical_raster = project_render_plan(&planner_output.plan, viewport, background_probe);
    let snapshot = ProjectionSnapshot::new(witness, logical_raster);
    Ok(ProjectionCacheEntry::new(
        planner_output.next_state,
        snapshot,
        ProjectionReuseKey::new(
            planner_output.signature,
            reuse_planner_clock,
            target_cell_presentation,
            policy.clone(),
        ),
    ))
}

fn reusable_projection_entry(
    current_scene: &SceneState,
    scene_revision: SceneRevision,
    observation: &ObservationSnapshot,
    geometry: &CursorTrailGeometry,
    policy: &CursorTrailProjectionPolicy,
    target_cell_presentation: TargetCellPresentation,
) -> Option<ProjectionCacheEntry> {
    let next_witness = projection_witness(scene_revision, observation);
    let planner_frame = geometry.planner_frame(policy);
    let signature = render_plan::frame_draw_signature(&planner_frame)?;
    let current_planner_seed = planner_seed(current_scene, policy);
    let current_reuse_planner_clock =
        projection_reuse_planner_clock(&planner_frame, &current_planner_seed);

    if frame_requires_background_probe(geometry, policy) {
        // Comment: the typed probe loop now keeps observation ownership explicit, but background
        // reuse still stays conservative because the retained cache is viewport-wide rather than
        // witness-bound to exact probe cells.
        return None;
    }

    let entry = projection_entry_for_witness(current_scene, next_witness, policy)?;
    let snapshot = entry.snapshot();
    let reuse_key = entry.reuse_key();

    if reuse_key.signature() != Some(signature)
        || reuse_key.planner_clock() != current_reuse_planner_clock
        || reuse_key.target_cell_presentation() != target_cell_presentation
        || reuse_key.policy() != policy
    {
        return None;
    }

    Some(ProjectionCacheEntry::new(
        entry.planner_state().clone(),
        snapshot.clone(),
        reuse_key.clone(),
    ))
}

fn update_scene_from_render_decision(
    state: &CoreState,
    render_decision: &RenderDecision,
) -> (
    SceneState,
    Option<ProjectionSnapshot>,
    Option<ApplyFailureKind>,
) {
    let current_scene = state.scene().clone();
    let (next_semantics, draw_semantic) =
        next_semantics_for_render_decision(current_scene.semantics(), render_decision);
    let dirty = dirty_entities(current_scene.semantics(), &next_semantics);
    let next_revision = if dirty.is_empty() {
        current_scene.revision()
    } else {
        current_scene.revision().next()
    };

    match &render_decision.render_action {
        RenderAction::Draw(frame) => {
            let Some(trail_semantic) = draw_semantic.as_ref() else {
                // Surprising: draw planning lost its semantic trail payload before projection.
                let next_scene = current_scene
                    .with_revision(next_revision)
                    .with_semantics(next_semantics)
                    .with_projection(ProjectionCache::Invalid)
                    .with_dirty(dirty);
                return (next_scene, None, Some(ApplyFailureKind::MissingProjection));
            };
            let policy = CursorTrailProjectionPolicy::from_render_frame(frame);
            if let Some(reused) = state.observation().and_then(|observation| {
                if dirty.is_empty() {
                    reusable_projection_entry(
                        &current_scene,
                        next_revision,
                        observation,
                        trail_semantic.geometry(),
                        &policy,
                        render_decision.render_side_effects.target_cell_presentation,
                    )
                } else {
                    None
                }
            }) {
                let snapshot = reused.snapshot().clone();
                let next_scene = current_scene
                    .with_revision(next_revision)
                    .with_semantics(next_semantics)
                    .with_projection(ProjectionCache::Ready(Box::new(reused)))
                    .with_dirty(DirtyEntitySet::default());
                return (next_scene, Some(snapshot), None);
            }

            let Some(observation) = state.observation() else {
                // Surprising: render planning completed without an active observation basis.
                // Keep semantic truth updated, but do not fabricate a projection.
                let next_scene = current_scene
                    .with_revision(next_revision)
                    .with_semantics(next_semantics)
                    .with_projection(ProjectionCache::Invalid)
                    .with_dirty(dirty);
                return (next_scene, None, Some(ApplyFailureKind::MissingProjection));
            };

            let entry = match project_draw_frame(
                &current_scene,
                next_revision,
                observation,
                trail_semantic.geometry(),
                &policy,
                render_decision.render_side_effects.target_cell_presentation,
            ) {
                Ok(entry) => entry,
                Err(reason) => {
                    let next_scene = current_scene
                        .with_revision(next_revision)
                        .with_semantics(next_semantics)
                        .with_projection(ProjectionCache::Invalid)
                        .with_dirty(dirty);
                    return (next_scene, None, Some(reason));
                }
            };
            let snapshot = entry.snapshot().clone();
            let next_scene = current_scene
                .with_revision(next_revision)
                .with_semantics(next_semantics)
                .with_projection(ProjectionCache::Ready(Box::new(entry)))
                .with_dirty(dirty);
            (next_scene, Some(snapshot), None)
        }
        RenderAction::ClearAll => {
            let next_scene = current_scene
                .with_revision(next_revision)
                .with_semantics(next_semantics)
                .with_projection(ProjectionCache::Invalid)
                .with_dirty(dirty);
            (next_scene, None, None)
        }
        RenderAction::Noop => {
            let projection = state
                .realization()
                .trusted_acknowledged_for_patch()
                .cloned();
            let next_scene = current_scene
                .with_semantics(next_semantics)
                .with_dirty(DirtyEntitySet::default());
            // Comment: the scene projection cache is planner reuse state, not shell authority.
            // A noop proposal may only target the trusted acknowledged render; otherwise cleanup
            // or divergence can leak stale cached draw input into apply as a replace patch.
            (next_scene, projection, None)
        }
    }
}

fn patch_basis(ledger: &RealizationLedger, target: Option<ProjectionSnapshot>) -> PatchBasis {
    PatchBasis::new(ledger.trusted_acknowledged_for_patch().cloned(), target)
}

fn realization_plan_for_render_decision(
    state: &CoreState,
    render_decision: &RenderDecision,
    patch_kind: ScenePatchKind,
    projection: Option<&ProjectionSnapshot>,
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
            let _ = patch_kind;
            // Comment: shell-visible smear occupancy is authoritative for clear intents.
            // Even if projection trust has already degraded to a noop patch basis, a reducer
            // `ClearAll` must still force shell clear work or the last visible trail can survive
            // until unrelated ingress repaints the UI.
            RealizationPlan::Clear(RealizationClear::new(
                state.runtime().config.max_kept_windows,
            ))
        }
        RenderAction::Noop => RealizationPlan::Noop,
    }
}

fn plan_runtime_transition(
    state: &CoreState,
    observation: &ObservationSnapshot,
    observed_at: Millis,
) -> (
    crate::state::RuntimeState,
    crate::core::runtime_reducer::CursorTransition,
) {
    let mut runtime = state.runtime().clone();
    runtime.set_color_at_cursor(observation.cursor_color().map(str::to_owned));

    let mode = observation.basis().mode();
    let cursor_location = observation.basis().cursor_location();
    let requested_target = state.last_cursor().map(point_from_cursor_position);
    let observed_target = observation
        .basis()
        .cursor_position()
        .map(point_from_cursor_position);
    let fallback_target = runtime.target_position();
    let source = select_event_source(mode, &runtime, requested_target, cursor_location);
    let event_now_ms = match source {
        EventSource::External => observation.basis().observed_at().value() as f64,
        EventSource::AnimationTick => {
            // Comment: animation ticks can reuse a stable observation snapshot after ingress
            // quiets down. Advancing the reducer with that frozen observation clock stalls tail
            // drain forever, so tick-driven reductions must use the timer event timestamp.
            observed_at.value() as f64
        }
    };
    let event_target = match source {
        EventSource::External => observed_target
            .or(requested_target)
            .unwrap_or(fallback_target),
        EventSource::AnimationTick => requested_target
            .or(observed_target)
            .unwrap_or(fallback_target),
    };

    let transition = reduce_cursor_event(
        &mut runtime,
        mode,
        CursorEventContext {
            row: event_target.row,
            col: event_target.col,
            now_ms: event_now_ms,
            seed: deterministic_event_seed(observation),
            cursor_location,
            scroll_shift: observation.scroll_shift(),
        },
        source,
    );
    (runtime, transition)
}

pub(super) fn plan_ready_state(
    state: CoreState,
    observation: ObservationSnapshot,
    observed_at: Millis,
) -> Transition {
    let (runtime, cursor_transition) = plan_runtime_transition(&state, &observation, observed_at);
    let crate::core::runtime_reducer::CursorTransition {
        render_decision,
        motion_class: _,
        should_schedule_next_animation,
        next_animation_at_ms,
    } = cursor_transition;

    let planning_state = state.with_runtime(runtime);
    let (planning_state, proposal_id) = planning_state.allocate_proposal_id();
    let next_animation_at_ms = next_animation_at_ms.map(Millis::new);
    let Some(next_state) = planning_state.clone().into_planning(proposal_id) else {
        // Surprising: planning reached render-plan staging without a retained observation.
        return Transition::stay(&planning_state);
    };

    Transition::new(
        next_state,
        vec![request_render_plan_effect(
            planning_state,
            observation,
            proposal_id,
            render_decision,
            should_schedule_next_animation,
            next_animation_at_ms,
            observed_at,
        )],
    )
}

pub(crate) fn build_planned_render(
    planning_state: &CoreState,
    proposal_id: ProposalId,
    render_decision: &RenderDecision,
    should_schedule_next_animation: bool,
    next_animation_at_ms: Option<Millis>,
) -> PlannedRender {
    let (next_scene, projection, projection_failure) =
        update_scene_from_render_decision(planning_state, render_decision);
    let basis = patch_basis(planning_state.realization(), projection);
    let patch_kind = ScenePatchKind::from_basis(&basis);
    let realization = realization_plan_for_render_decision(
        planning_state,
        render_decision,
        patch_kind,
        basis.target(),
        projection_failure,
    );
    let patch = ScenePatch::derive(basis);
    let proposal = InFlightProposal::new(
        proposal_id,
        patch,
        realization,
        render_decision.render_cleanup_action,
        render_decision.render_side_effects,
        should_schedule_next_animation,
        next_animation_at_ms,
    );
    PlannedRender::new(next_scene, proposal)
}

#[cfg(test)]
mod tests {
    use super::{
        dirty_entities, patch_basis, planner_seed, project_draw_frame,
        realization_plan_for_render_decision, reusable_projection_entry,
        update_scene_from_render_decision,
    };
    use crate::core::runtime_reducer::{RenderAction, RenderDecision, TargetCellPresentation};
    use crate::core::state::{
        CoreState, CursorTrailGeometry, CursorTrailProjectionPolicy, CursorTrailSemantic,
        ExternalDemand, ExternalDemandKind, ObservationBasis, ObservationMotion,
        ObservationRequest, ObservationSnapshot, ProbeRequestSet, ProbeSet, ProjectionCache,
        RealizationClear, RealizationDivergence, RealizationLedger, RealizationPlan, ScenePatch,
        ScenePatchKind, SceneState, SemanticEntity, SemanticEntityId, SemanticScene,
    };
    use crate::core::types::{
        CursorCol, CursorPosition, CursorRow, IngressSeq, Millis, ObservationId, SceneRevision,
        StrokeId, ViewportSnapshot,
    };
    use crate::draw::render_plan::PlannerState as ProjectionPlannerState;
    use crate::state::CursorLocation;
    use crate::types::{Point, RenderFrame, RenderStepSample, StaticRenderConfig};
    use std::collections::BTreeSet;
    use std::sync::Arc;

    fn base_frame() -> RenderFrame {
        let corners = [
            Point {
                row: 12.0,
                col: 10.0,
            },
            Point {
                row: 12.0,
                col: 11.0,
            },
            Point {
                row: 13.0,
                col: 11.0,
            },
            Point {
                row: 13.0,
                col: 10.0,
            },
        ];
        RenderFrame {
            mode: "n".to_string(),
            corners,
            step_samples: vec![RenderStepSample::new(corners, 1.0)],
            planner_idle_steps: 0,
            target: Point {
                row: 10.0,
                col: 10.0,
            },
            target_corners: [
                Point {
                    row: 10.0,
                    col: 10.0,
                },
                Point {
                    row: 10.0,
                    col: 11.0,
                },
                Point {
                    row: 11.0,
                    col: 11.0,
                },
                Point {
                    row: 11.0,
                    col: 10.0,
                },
            ],
            vertical_bar: false,
            trail_stroke_id: StrokeId::new(1),
            retarget_epoch: 1,
            particles: Vec::new(),
            color_at_cursor: None,
            static_config: Arc::new(StaticRenderConfig {
                cursor_color: None,
                cursor_color_insert_mode: None,
                normal_bg: None,
                transparent_bg_fallback_color: "#303030".to_string(),
                cterm_cursor_colors: None,
                cterm_bg: None,
                hide_target_hack: true,
                max_kept_windows: 32,
                never_draw_over_target: false,
                particle_max_lifetime: 1.0,
                particle_switch_octant_braille: 0.3,
                particles_over_text: true,
                color_levels: 16,
                gamma: 2.2,
                block_aspect_ratio: crate::config::DEFAULT_BLOCK_ASPECT_RATIO,
                tail_duration_ms: 180.0,
                simulation_hz: 120.0,
                trail_thickness: 1.0,
                trail_thickness_x: 1.0,
                spatial_coherence_weight: 1.0,
                temporal_stability_weight: 0.12,
                top_k_per_cell: 5,
                windows_zindex: 200,
            }),
        }
    }

    fn observation(seq: u64) -> ObservationSnapshot {
        let request = ObservationRequest::new(
            ExternalDemand::new(
                IngressSeq::new(seq),
                ExternalDemandKind::ExternalCursor,
                Millis::new(seq),
                None,
            ),
            ProbeRequestSet::default(),
        );
        let basis = ObservationBasis::new(
            ObservationId::from_ingress_seq(IngressSeq::new(seq)),
            Millis::new(seq),
            "n".to_string(),
            Some(CursorPosition {
                row: CursorRow(10),
                col: CursorCol(10),
            }),
            CursorLocation::new(1, 1, 1, 1),
            ViewportSnapshot::new(CursorRow(40), CursorCol(120)),
        );
        ObservationSnapshot::new(
            request,
            basis,
            ProbeSet::default(),
            ObservationMotion::default(),
        )
    }

    #[test]
    fn dirty_entities_track_target_cell_presentation_explicitly() {
        let frame = base_frame();
        let previous = SemanticScene::default().with_entity(SemanticEntity::CursorTrail(
            CursorTrailSemantic::from_render_frame(&frame, TargetCellPresentation::None),
        ));
        let next = SemanticScene::default().with_entity(SemanticEntity::CursorTrail(
            CursorTrailSemantic::from_render_frame(
                &frame,
                TargetCellPresentation::OverlayBlockCell,
            ),
        ));

        assert_eq!(
            dirty_entities(&previous, &next).entities(),
            &BTreeSet::from([SemanticEntityId::CursorTrail])
        );
    }

    #[test]
    fn dirty_entities_ignore_palette_only_drift() {
        let previous_frame = base_frame();
        let mut next_frame = previous_frame.clone();
        next_frame.color_at_cursor = Some("#abcdef".to_string());

        let previous = SemanticScene::default().with_entity(SemanticEntity::CursorTrail(
            CursorTrailSemantic::from_render_frame(&previous_frame, TargetCellPresentation::None),
        ));
        let next = SemanticScene::default().with_entity(SemanticEntity::CursorTrail(
            CursorTrailSemantic::from_render_frame(&next_frame, TargetCellPresentation::None),
        ));

        assert!(dirty_entities(&previous, &next).is_empty());
    }

    #[test]
    fn reusable_projection_entry_rejects_target_cell_presentation_mismatch() {
        let frame = base_frame();
        let geometry = CursorTrailGeometry::from_render_frame(&frame);
        let policy = CursorTrailProjectionPolicy::from_render_frame(&frame);
        let cached_observation = observation(1);
        let cached = project_draw_frame(
            &SceneState::default(),
            SceneRevision::INITIAL,
            &cached_observation,
            &geometry,
            &policy,
            TargetCellPresentation::None,
        )
        .expect("projection without probe-gated particles");
        let current_scene = SceneState::default()
            .with_projection(ProjectionCache::Ready(Box::new(cached)))
            .with_semantics(
                SemanticScene::default().with_entity(SemanticEntity::CursorTrail(
                    CursorTrailSemantic::from_render_frame(&frame, TargetCellPresentation::None),
                )),
            );
        let next_observation = observation(2);

        assert!(
            reusable_projection_entry(
                &current_scene,
                SceneRevision::INITIAL,
                &next_observation,
                &geometry,
                &policy,
                TargetCellPresentation::OverlayBlockCell,
            )
            .is_none()
        );
    }

    #[test]
    fn reusable_projection_entry_rejects_observation_witness_drift() {
        let frame = base_frame();
        let geometry = CursorTrailGeometry::from_render_frame(&frame);
        let policy = CursorTrailProjectionPolicy::from_render_frame(&frame);
        let cached_observation = observation(1);
        let cached = project_draw_frame(
            &SceneState::default(),
            SceneRevision::INITIAL,
            &cached_observation,
            &geometry,
            &policy,
            TargetCellPresentation::OverlayBlockCell,
        )
        .expect("projection without probe-gated particles");
        let current_scene =
            SceneState::default().with_projection(ProjectionCache::Ready(Box::new(cached)));
        let next_observation = observation(2);

        assert!(
            reusable_projection_entry(
                &current_scene,
                SceneRevision::INITIAL,
                &next_observation,
                &geometry,
                &policy,
                TargetCellPresentation::OverlayBlockCell,
            )
            .is_none()
        );
    }

    #[test]
    fn reusable_projection_entry_preserves_matching_target_cell_presentation_for_exact_witness() {
        let mut frame = base_frame();
        frame.step_samples.clear();
        let geometry = CursorTrailGeometry::from_render_frame(&frame);
        let policy = CursorTrailProjectionPolicy::from_render_frame(&frame);
        let cached_observation = observation(1);
        let cached = project_draw_frame(
            &SceneState::default(),
            SceneRevision::INITIAL,
            &cached_observation,
            &geometry,
            &policy,
            TargetCellPresentation::OverlayBlockCell,
        )
        .expect("projection without probe-gated particles");
        let current_scene =
            SceneState::default().with_projection(ProjectionCache::Ready(Box::new(cached)));

        let reused = reusable_projection_entry(
            &current_scene,
            SceneRevision::INITIAL,
            &cached_observation,
            &geometry,
            &policy,
            TargetCellPresentation::OverlayBlockCell,
        )
        .expect("matching presentation should remain reusable");

        assert_eq!(
            reused.reuse_key().target_cell_presentation(),
            TargetCellPresentation::OverlayBlockCell
        );
        assert_eq!(
            reused.snapshot().witness().observation_id(),
            cached_observation.request().observation_id()
        );
    }

    #[test]
    fn reusable_projection_entry_rejects_advancing_frame_after_planner_clock_advance() {
        let mut frame = base_frame();
        frame.step_samples.clear();
        frame.planner_idle_steps = 1;
        let geometry = CursorTrailGeometry::from_render_frame(&frame);
        let policy = CursorTrailProjectionPolicy::from_render_frame(&frame);
        let cached_observation = observation(1);
        let cached = project_draw_frame(
            &SceneState::default(),
            SceneRevision::INITIAL,
            &cached_observation,
            &geometry,
            &policy,
            TargetCellPresentation::None,
        )
        .expect("projection without probe-gated particles");
        let current_scene =
            SceneState::default().with_projection(ProjectionCache::Ready(Box::new(cached)));

        assert!(
            reusable_projection_entry(
                &current_scene,
                SceneRevision::INITIAL,
                &cached_observation,
                &geometry,
                &policy,
                TargetCellPresentation::None,
            )
            .is_none()
        );
    }

    #[test]
    fn planner_seed_survives_projection_witness_drift() {
        let frame = base_frame();
        let geometry = CursorTrailGeometry::from_render_frame(&frame);
        let policy = CursorTrailProjectionPolicy::from_render_frame(&frame);
        let cached_observation = observation(1);
        let cached = project_draw_frame(
            &SceneState::default(),
            SceneRevision::INITIAL,
            &cached_observation,
            &geometry,
            &policy,
            TargetCellPresentation::None,
        )
        .expect("projection without probe-gated particles");
        let cached_planner_state = cached.planner_state().clone();
        assert_ne!(cached_planner_state, ProjectionPlannerState::default());

        let current_scene =
            SceneState::default().with_projection(ProjectionCache::Ready(Box::new(cached)));

        assert_eq!(planner_seed(&current_scene, &policy), cached_planner_state);
    }

    #[test]
    fn project_draw_frame_advances_planner_history_across_observations() {
        let frame = base_frame();
        let geometry = CursorTrailGeometry::from_render_frame(&frame);
        let policy = CursorTrailProjectionPolicy::from_render_frame(&frame);
        let first_observation = observation(1);
        let first = project_draw_frame(
            &SceneState::default(),
            SceneRevision::INITIAL,
            &first_observation,
            &geometry,
            &policy,
            TargetCellPresentation::None,
        )
        .expect("first projection without probe-gated particles");
        let second = project_draw_frame(
            &SceneState::default().with_projection(ProjectionCache::Ready(Box::new(first.clone()))),
            SceneRevision::INITIAL.next(),
            &observation(2),
            &geometry,
            &policy,
            TargetCellPresentation::None,
        )
        .expect("second projection should reuse planner history");

        assert_ne!(first.planner_state(), second.planner_state());
    }

    #[test]
    fn planner_seed_rejects_projection_policy_drift() {
        let frame = base_frame();
        let geometry = CursorTrailGeometry::from_render_frame(&frame);
        let cached_policy = CursorTrailProjectionPolicy::from_render_frame(&frame);
        let cached_observation = observation(1);
        let cached = project_draw_frame(
            &SceneState::default(),
            SceneRevision::INITIAL,
            &cached_observation,
            &geometry,
            &cached_policy,
            TargetCellPresentation::None,
        )
        .expect("projection without probe-gated particles");
        let current_scene =
            SceneState::default().with_projection(ProjectionCache::Ready(Box::new(cached)));
        let mut drifted_frame = frame;
        let mut drifted_static_config = (*drifted_frame.static_config).clone();
        drifted_static_config.color_levels = drifted_static_config.color_levels.saturating_add(1);
        drifted_frame.static_config = Arc::new(drifted_static_config);
        let drifted_policy = CursorTrailProjectionPolicy::from_render_frame(&drifted_frame);

        assert_ne!(cached_policy, drifted_policy);
        assert_eq!(
            planner_seed(&current_scene, &drifted_policy),
            ProjectionPlannerState::default()
        );
        assert!(
            reusable_projection_entry(
                &current_scene,
                SceneRevision::INITIAL,
                &cached_observation,
                &geometry,
                &drifted_policy,
                TargetCellPresentation::None,
            )
            .is_none()
        );
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

        assert_eq!(
            realization_plan_for_render_decision(
                &CoreState::default(),
                &decision,
                ScenePatchKind::Noop,
                None,
                None,
            ),
            RealizationPlan::Clear(RealizationClear::new(
                CoreState::default().runtime().config.max_kept_windows,
            ))
        );
    }

    #[test]
    fn noop_render_action_uses_trusted_acknowledged_target_not_stale_scene_projection() {
        let frame = base_frame();
        let geometry = CursorTrailGeometry::from_render_frame(&frame);
        let policy = CursorTrailProjectionPolicy::from_render_frame(&frame);
        let cached_observation = observation(1);
        let cached = project_draw_frame(
            &SceneState::default(),
            SceneRevision::INITIAL,
            &cached_observation,
            &geometry,
            &policy,
            TargetCellPresentation::None,
        )
        .expect("projection without probe-gated particles");
        let scene = SceneState::default()
            .with_projection(ProjectionCache::Ready(Box::new(cached.clone())))
            .with_semantics(
                SemanticScene::default().with_entity(SemanticEntity::CursorTrail(
                    CursorTrailSemantic::from_render_frame(&frame, TargetCellPresentation::None),
                )),
            );
        let state = CoreState::default()
            .initialize()
            .with_scene(scene)
            .with_realization(RealizationLedger::Diverged {
                last_consistent: Some(cached.snapshot().clone()),
                divergence: RealizationDivergence::ShellStateUnknown,
            });
        let decision = RenderDecision {
            render_action: RenderAction::Noop,
            render_cleanup_action: crate::core::runtime_reducer::RenderCleanupAction::NoAction,
            render_allocation_policy:
                crate::core::runtime_reducer::RenderAllocationPolicy::ReuseOnly,
            render_side_effects: crate::core::runtime_reducer::RenderSideEffects::default(),
        };

        let (next_scene, projection, projection_failure) =
            update_scene_from_render_decision(&state, &decision);
        let patch = ScenePatch::derive(patch_basis(state.realization(), projection.clone()));

        assert!(next_scene.projection_entry().is_some());
        assert!(projection_failure.is_none());
        assert_eq!(projection, None);
        assert_eq!(patch.kind(), ScenePatchKind::Noop);
        assert_eq!(
            realization_plan_for_render_decision(
                &state,
                &decision,
                patch.kind(),
                projection.as_ref(),
                projection_failure,
            ),
            RealizationPlan::Noop
        );
    }
}
