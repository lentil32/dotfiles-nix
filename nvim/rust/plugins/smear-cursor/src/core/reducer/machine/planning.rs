use super::Transition;
use super::support::request_render_plan_effect;
use crate::core::effect::RenderPlanningContext;
use crate::core::effect::RenderPlanningObservation;
use crate::core::realization::PaletteSpec;
use crate::core::realization::project_render_plan;
use crate::core::runtime_reducer::CursorEventContext;
use crate::core::runtime_reducer::EventSource;
use crate::core::runtime_reducer::RenderAction;
use crate::core::runtime_reducer::RenderDecision;
use crate::core::runtime_reducer::TargetCellPresentation;
use crate::core::runtime_reducer::select_event_source;
use crate::core::state::AnimationSchedule;
use crate::core::state::ApplyFailureKind;
use crate::core::state::BackgroundProbeBatch;
use crate::core::state::BackgroundProbePlan;
use crate::core::state::CoreState;
use crate::core::state::CursorTrailGeometry;
use crate::core::state::CursorTrailProjectionPolicy;
use crate::core::state::CursorTrailSemantic;
use crate::core::state::DirtyEntitySet;
use crate::core::state::InFlightProposal;
use crate::core::state::ObservationSnapshot;
use crate::core::state::PatchBasis;
use crate::core::state::PlannedProjectionUpdate;
use crate::core::state::PlannedRender;
use crate::core::state::PlannedSceneUpdate;
use crate::core::state::PreparedObservationPlan;
use crate::core::state::ProjectionCache;
use crate::core::state::ProjectionCacheEntry;
use crate::core::state::ProjectionPlannerClock;
use crate::core::state::ProjectionReuseKey;
use crate::core::state::ProjectionSnapshot;
use crate::core::state::ProjectionWitness;
use crate::core::state::RealizationClear;
use crate::core::state::RealizationDivergence;
use crate::core::state::RealizationDraw;
use crate::core::state::RealizationFailure;
use crate::core::state::RealizationPlan;
use crate::core::state::ScenePatch;
use crate::core::state::SceneState;
use crate::core::state::SemanticEntity;
use crate::core::state::SemanticEntityId;
use crate::core::state::SemanticScene;
use crate::core::state::classify_semantic_event;
use crate::core::types::Millis;
use crate::core::types::ProjectorRevision;
use crate::core::types::ProposalId;
use crate::core::types::SceneRevision;
use crate::core::types::ViewportSnapshot;
use crate::draw::render_plan::PlannerState as ProjectionPlannerState;
use crate::draw::render_plan::Viewport;
use crate::draw::render_plan::{self};
use crate::types::DEFAULT_RNG_STATE;
use crate::types::Point;
use crate::types::RenderFrame;
use std::cell::OnceCell;

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

fn render_planning_observation(observation: &ObservationSnapshot) -> RenderPlanningObservation {
    RenderPlanningObservation::new(
        observation.basis().observation_id(),
        observation.basis().viewport(),
        observation.background_probe().cloned(),
    )
}

fn projection_witness(
    scene_revision: SceneRevision,
    observation: &RenderPlanningObservation,
) -> ProjectionWitness {
    ProjectionWitness::new(
        scene_revision,
        observation.observation_id(),
        observation.viewport(),
        ProjectorRevision::CURRENT,
    )
}

fn projection_entry_for_reuse<'a>(
    scene: &'a SceneState,
    witness: ProjectionWitness,
    policy: &CursorTrailProjectionPolicy,
) -> Option<&'a ProjectionCacheEntry> {
    let entry = scene.projection_entry()?;
    if entry.snapshot().witness().viewport() != witness.viewport()
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
    // projection snapshots are witness-bound shell inputs, but planner history is part
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
    // planner aging is projection-owned, not semantic-geometry-owned. Advancing frames
    // are only reusable when replayed from the same planner clock; otherwise retained projection
    // reuse would skip latent-field advancement and freeze the tail.
    frame_advances_planner(frame).then(|| planner_clock(planner_state))
}

fn next_semantics_for_render_decision(
    current: &SemanticScene,
    render_decision: &RenderDecision,
) -> SemanticScene {
    match &render_decision.render_action {
        RenderAction::Draw(frame) => current.clone().with_entity(SemanticEntity::CursorTrail(
            CursorTrailSemantic::from_render_frame(
                frame.as_ref(),
                render_decision.render_side_effects.target_cell_presentation,
            ),
        )),
        RenderAction::ClearAll => current
            .clone()
            .without_entity(SemanticEntityId::CursorTrail),
        RenderAction::Noop => current.clone(),
    }
}

fn cursor_trail_semantic(scene: &SemanticScene) -> Option<&CursorTrailSemantic> {
    match scene.entity(SemanticEntityId::CursorTrail)? {
        SemanticEntity::CursorTrail(trail) => Some(trail),
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

struct PreparedProjection<'a> {
    witness: ProjectionWitness,
    viewport: Viewport,
    planner_frame: RenderFrame,
    trail_signature: OnceCell<Option<u64>>,
    particle_overlay_signature: OnceCell<Option<u64>>,
    planner_state: ProjectionPlannerState,
    reuse_planner_clock: Option<ProjectionPlannerClock>,
    background_probe: Option<&'a BackgroundProbeBatch>,
}

impl PreparedProjection<'_> {
    fn trail_signature(&self) -> Option<u64> {
        *self
            .trail_signature
            .get_or_init(|| render_plan::frame_draw_signature(&self.planner_frame))
    }

    fn particle_overlay_signature(&self) -> Option<u64> {
        *self
            .particle_overlay_signature
            .get_or_init(|| render_plan::frame_particle_overlay_signature(&self.planner_frame))
    }
}

fn prepare_projection<'a>(
    current_scene: &SceneState,
    scene_revision: SceneRevision,
    observation: &'a RenderPlanningObservation,
    geometry: &CursorTrailGeometry,
    policy: &CursorTrailProjectionPolicy,
) -> Result<PreparedProjection<'a>, ApplyFailureKind> {
    let background_probe = if frame_requires_background_probe(geometry, policy) {
        let Some(background_probe) = observation.background_probe() else {
            // probe-gated particles may not silently degrade into a
            // "successful" projection. Force an explicit retry path instead.
            return Err(ApplyFailureKind::MissingRequiredProbe);
        };
        Some(background_probe)
    } else {
        None
    };
    let planner_frame = geometry.planner_frame(policy);
    let planner_state = planner_seed(current_scene, policy);
    let reuse_planner_clock = projection_reuse_planner_clock(&planner_frame, &planner_state);
    Ok(PreparedProjection {
        witness: projection_witness(scene_revision, observation),
        viewport: projection_viewport(observation.viewport()),
        planner_frame,
        trail_signature: OnceCell::new(),
        particle_overlay_signature: OnceCell::new(),
        planner_state,
        reuse_planner_clock,
        background_probe,
    })
}

fn project_prepared_frame(
    prepared: PreparedProjection<'_>,
    policy: &CursorTrailProjectionPolicy,
    target_cell_presentation: TargetCellPresentation,
) -> ProjectionCacheEntry {
    let trail_signature = prepared.trail_signature();
    let particle_overlay_signature = prepared.particle_overlay_signature();
    let mut planner_output = render_plan::render_frame_to_plan_with_signature(
        &prepared.planner_frame,
        prepared.planner_state,
        prepared.viewport,
        trail_signature,
    );
    if let TargetCellPresentation::OverlayCursorCell(shape) = target_cell_presentation {
        planner_output.plan.target_cell_overlay = render_plan::plan_target_cell_overlay(
            &prepared.planner_frame,
            prepared.viewport,
            shape,
        );
    }

    let logical_raster = project_render_plan(
        &planner_output.plan,
        prepared.viewport,
        prepared.background_probe,
    );
    let snapshot = ProjectionSnapshot::new(prepared.witness, logical_raster);
    let PreparedProjection {
        reuse_planner_clock,
        ..
    } = prepared;
    ProjectionCacheEntry::new(
        planner_output.next_state,
        snapshot,
        ProjectionReuseKey::new(
            planner_output.signature,
            particle_overlay_signature,
            reuse_planner_clock,
            target_cell_presentation,
            policy.clone(),
        ),
    )
}

#[cfg(test)]
fn project_draw_frame(
    current_scene: &SceneState,
    scene_revision: SceneRevision,
    observation: &ObservationSnapshot,
    geometry: &CursorTrailGeometry,
    policy: &CursorTrailProjectionPolicy,
    target_cell_presentation: TargetCellPresentation,
) -> Result<ProjectionCacheEntry, ApplyFailureKind> {
    let observation = render_planning_observation(observation);
    let prepared = prepare_projection(
        current_scene,
        scene_revision,
        &observation,
        geometry,
        policy,
    )?;
    Ok(project_prepared_frame(
        prepared,
        policy,
        target_cell_presentation,
    ))
}

fn reusable_prepared_projection_entry(
    current_scene: &SceneState,
    prepared: &PreparedProjection<'_>,
    policy: &CursorTrailProjectionPolicy,
    target_cell_presentation: TargetCellPresentation,
) -> Option<ProjectionCacheEntry> {
    let entry = projection_entry_for_reuse(current_scene, prepared.witness, policy)?;
    let reuse_key = entry.reuse_key();

    if reuse_key.planner_clock() != prepared.reuse_planner_clock
        || reuse_key.target_cell_presentation() != target_cell_presentation
        || reuse_key.policy() != policy
    {
        return None;
    }
    if reuse_key.trail_signature() != prepared.trail_signature() {
        return None;
    }

    let refresh_particle_overlay = reuse_key.particle_overlay_signature()
        != prepared.particle_overlay_signature()
        || (prepared.background_probe.is_some()
            && entry.snapshot().witness().observation_id() != prepared.witness.observation_id());
    let logical_raster = if refresh_particle_overlay {
        let particle_cells = project_render_plan(
            &render_plan::particle_overlay_plan(&prepared.planner_frame, prepared.viewport),
            prepared.viewport,
            prepared.background_probe,
        )
        .into_particle_cells();
        crate::events::record_particle_overlay_refresh(particle_cells.len());
        entry
            .snapshot()
            .logical_raster()
            .replace_particle_cells(particle_cells)
    } else {
        entry.snapshot().logical_raster().clone()
    };
    let snapshot = ProjectionSnapshot::new(prepared.witness, logical_raster);

    Some(ProjectionCacheEntry::new(
        entry.planner_state().clone(),
        snapshot,
        ProjectionReuseKey::new(
            prepared.trail_signature(),
            prepared.particle_overlay_signature(),
            prepared.reuse_planner_clock,
            target_cell_presentation,
            policy.clone(),
        ),
    ))
}

#[cfg(test)]
fn reusable_projection_entry(
    current_scene: &SceneState,
    scene_revision: SceneRevision,
    observation: &ObservationSnapshot,
    geometry: &CursorTrailGeometry,
    policy: &CursorTrailProjectionPolicy,
    target_cell_presentation: TargetCellPresentation,
) -> Option<ProjectionCacheEntry> {
    let observation = render_planning_observation(observation);
    let prepared = prepare_projection(
        current_scene,
        scene_revision,
        &observation,
        geometry,
        policy,
    )
    .ok()?;
    reusable_prepared_projection_entry(current_scene, &prepared, policy, target_cell_presentation)
}

fn update_scene_from_render_decision_with_context(
    current_scene: &SceneState,
    observation: Option<&RenderPlanningObservation>,
    trusted_projection: Option<&ProjectionSnapshot>,
    render_decision: &RenderDecision,
) -> (
    PlannedSceneUpdate,
    Option<ProjectionSnapshot>,
    Option<ApplyFailureKind>,
) {
    let next_semantics =
        next_semantics_for_render_decision(current_scene.semantics(), render_decision);
    let dirty = dirty_entities(current_scene.semantics(), &next_semantics);
    let next_revision = if dirty.is_empty() {
        current_scene.revision()
    } else {
        current_scene.revision().next()
    };

    match &render_decision.render_action {
        RenderAction::Draw(frame) => {
            let Some(trail_semantic) = cursor_trail_semantic(&next_semantics) else {
                // Surprising: draw planning lost its semantic trail payload before projection.
                let next_scene = PlannedSceneUpdate::new(
                    next_revision,
                    next_semantics,
                    PlannedProjectionUpdate::Replace(ProjectionCache::Invalid),
                    dirty,
                );
                return (next_scene, None, Some(ApplyFailureKind::MissingProjection));
            };
            let policy = CursorTrailProjectionPolicy::from_render_frame(frame);
            let Some(observation) = observation else {
                // Surprising: render planning completed without an active observation basis.
                // Keep semantic truth updated, but do not fabricate a projection.
                let next_scene = PlannedSceneUpdate::new(
                    next_revision,
                    next_semantics,
                    PlannedProjectionUpdate::Replace(ProjectionCache::Invalid),
                    dirty,
                );
                return (next_scene, None, Some(ApplyFailureKind::MissingProjection));
            };

            let prepared = match prepare_projection(
                current_scene,
                next_revision,
                observation,
                trail_semantic.geometry(),
                &policy,
            ) {
                Ok(prepared) => prepared,
                Err(reason) => {
                    let next_scene = PlannedSceneUpdate::new(
                        next_revision,
                        next_semantics,
                        PlannedProjectionUpdate::Replace(ProjectionCache::Invalid),
                        dirty,
                    );
                    return (next_scene, None, Some(reason));
                }
            };

            if let Some(reused) = reusable_prepared_projection_entry(
                current_scene,
                &prepared,
                &policy,
                render_decision.render_side_effects.target_cell_presentation,
            ) {
                let snapshot = reused.snapshot().clone();
                let next_scene = PlannedSceneUpdate::new(
                    next_revision,
                    next_semantics,
                    PlannedProjectionUpdate::Replace(ProjectionCache::Ready(Box::new(reused))),
                    if dirty.is_empty() {
                        DirtyEntitySet::default()
                    } else {
                        dirty
                    },
                );
                return (next_scene, Some(snapshot), None);
            }

            let entry = project_prepared_frame(
                prepared,
                &policy,
                render_decision.render_side_effects.target_cell_presentation,
            );
            let snapshot = entry.snapshot().clone();
            let next_scene = PlannedSceneUpdate::new(
                next_revision,
                next_semantics,
                PlannedProjectionUpdate::Replace(ProjectionCache::Ready(Box::new(entry))),
                dirty,
            );
            (next_scene, Some(snapshot), None)
        }
        RenderAction::ClearAll => {
            let next_scene = PlannedSceneUpdate::new(
                next_revision,
                next_semantics,
                PlannedProjectionUpdate::Replace(ProjectionCache::Invalid),
                dirty,
            );
            (next_scene, None, None)
        }
        RenderAction::Noop => {
            let projection = trusted_projection.cloned();
            let next_scene = PlannedSceneUpdate::new(
                current_scene.revision(),
                next_semantics,
                PlannedProjectionUpdate::Keep,
                DirtyEntitySet::default(),
            );
            // the scene projection cache is planner reuse state, not shell authority.
            // A noop proposal may only target the trusted acknowledged render; otherwise cleanup
            // or divergence can leak stale cached draw input into apply as a replace patch.
            (next_scene, projection, None)
        }
    }
}

fn patch_basis(
    acknowledged_projection: Option<ProjectionSnapshot>,
    target: Option<ProjectionSnapshot>,
) -> PatchBasis {
    PatchBasis::new(acknowledged_projection, target)
}

fn realization_plan_for_render_decision(
    clear_all_max_kept_windows: usize,
    render_decision: &RenderDecision,
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
            // shell-visible smear occupancy is authoritative for clear intents.
            // Even if projection trust has already degraded to a noop patch basis, a reducer
            // `ClearAll` must still force shell clear work or the last visible trail can survive
            // until unrelated ingress repaints the UI.
            RealizationPlan::Clear(RealizationClear::new(clear_all_max_kept_windows))
        }
        RenderAction::Noop => RealizationPlan::Noop,
    }
}

fn plan_runtime_transition_for_runtime(
    runtime: &mut crate::state::RuntimeState,
    last_cursor: Option<crate::core::types::CursorPosition>,
    previous_observation: Option<&ObservationSnapshot>,
    observation: &ObservationSnapshot,
    observed_at: Millis,
) -> crate::core::runtime_reducer::CursorTransition {
    runtime.set_color_at_cursor(observation.cursor_color());

    let mode = observation.basis().mode();
    let cursor_location = observation.basis().cursor_location();
    let requested_target = last_cursor.map(point_from_cursor_position);
    let observed_target = observation
        .basis()
        .cursor_position()
        .map(point_from_cursor_position);
    let fallback_target = runtime.target_position();
    let semantic_event = classify_semantic_event(previous_observation, observation);
    let source = select_event_source(
        mode,
        runtime,
        semantic_event,
        requested_target,
        &cursor_location,
    );
    let event_now_ms = match source {
        EventSource::External => observation.basis().observed_at().value() as f64,
        EventSource::AnimationTick => {
            // animation ticks can reuse a stable observation snapshot after ingress
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

    let event = CursorEventContext {
        row: event_target.row,
        col: event_target.col,
        now_ms: event_now_ms,
        seed: deterministic_event_seed(observation),
        cursor_location,
        scroll_shift: observation.scroll_shift(),
        semantic_event,
    };
    crate::core::runtime_reducer::reduce_cursor_event_for_perf_class(
        runtime,
        mode,
        &event,
        source,
        observation.request().demand().buffer_perf_class(),
    )
}

fn prepare_observation_plan_with_runtime(
    mut runtime: crate::state::RuntimeState,
    last_cursor: Option<crate::core::types::CursorPosition>,
    previous_observation: Option<&ObservationSnapshot>,
    observation: &ObservationSnapshot,
    observed_at: Millis,
) -> (crate::state::RuntimeState, PreparedObservationPlan) {
    let retained_motion = runtime.prepared_motion();
    let transition = plan_runtime_transition_for_runtime(
        &mut runtime,
        last_cursor,
        previous_observation,
        observation,
        observed_at,
    );
    let prepared_plan = PreparedObservationPlan::new(runtime.prepared_motion(), transition);
    runtime.apply_prepared_motion(retained_motion);
    (runtime, prepared_plan)
}

pub(super) fn prepare_observation_plan(
    mut state: CoreState,
    previous_observation: Option<&ObservationSnapshot>,
    observation: &ObservationSnapshot,
    observed_at: Millis,
) -> (CoreState, PreparedObservationPlan) {
    let (runtime, prepared_plan) = prepare_observation_plan_with_runtime(
        state.take_runtime(),
        state.last_cursor(),
        previous_observation,
        observation,
        observed_at,
    );
    (state.with_runtime(runtime), prepared_plan)
}

pub(super) fn background_probe_plan(
    prepared_plan: &PreparedObservationPlan,
    viewport: ViewportSnapshot,
) -> Option<BackgroundProbePlan> {
    match &prepared_plan.transition().render_decision.render_action {
        RenderAction::Draw(frame) => Some(BackgroundProbePlan::from_render_frame(
            frame.as_ref(),
            viewport,
        )),
        RenderAction::ClearAll | RenderAction::Noop => None,
    }
}

fn plan_ready_state_with_prepared(
    mut state: CoreState,
    observed_at: Millis,
    prepared_plan: PreparedObservationPlan,
) -> Transition {
    let (prepared_motion, cursor_transition) = prepared_plan.into_parts();
    state.runtime_mut().apply_prepared_motion(prepared_motion);
    plan_ready_state_with_transition(state, observed_at, cursor_transition)
}

fn plan_ready_state_with_transition(
    state: CoreState,
    observed_at: Millis,
    cursor_transition: crate::core::runtime_reducer::CursorTransition,
) -> Transition {
    let crate::core::runtime_reducer::CursorTransition {
        render_decision,
        motion_class: _,
        should_schedule_next_animation,
        next_animation_at_ms,
    } = cursor_transition;

    let Some(observation_id) = state
        .observation()
        .map(|observation| observation.basis().observation_id())
    else {
        // Surprising: `plan_ready_state` lost its ready observation before planning.
        return Transition::stay(&state);
    };

    let (state, proposal_id) = state.allocate_proposal_id();
    let animation_schedule = AnimationSchedule::from_parts(
        should_schedule_next_animation,
        next_animation_at_ms.map(Millis::new),
    );
    if !matches!(
        state.protocol(),
        crate::core::state::ProtocolState::Ready { .. }
    ) {
        // Surprising: `plan_ready_state` was invoked outside the ready lifecycle boundary.
        return Transition::stay(&state);
    }
    let planning = RenderPlanningContext::new(
        state.shared_scene(),
        state.observation().map(render_planning_observation),
        state
            .realization()
            .trusted_acknowledged_for_patch()
            .cloned(),
        state.runtime().config.max_kept_windows,
    );
    let Some(next_state) = state.into_planning(proposal_id) else {
        unreachable!("ready lifecycle checked above")
    };

    Transition::new(
        next_state,
        vec![request_render_plan_effect(
            planning,
            observation_id,
            proposal_id,
            render_decision,
            animation_schedule,
            observed_at,
        )],
    )
}

pub(super) fn plan_ready_state(
    mut state: CoreState,
    previous_observation: Option<&ObservationSnapshot>,
    observed_at: Millis,
) -> Transition {
    let last_cursor = state.last_cursor();
    let cursor_transition = {
        let Some((runtime, observation)) = state.runtime_mut_with_observation() else {
            return Transition::stay_owned(state);
        };
        plan_runtime_transition_for_runtime(
            runtime,
            last_cursor,
            previous_observation,
            observation,
            observed_at,
        )
    };
    plan_ready_state_with_transition(state, observed_at, cursor_transition)
}

pub(super) fn plan_ready_state_with_observation_plan(
    state: CoreState,
    observed_at: Millis,
    prepared_plan: PreparedObservationPlan,
) -> Transition {
    plan_ready_state_with_prepared(state, observed_at, prepared_plan)
}

fn build_in_flight_proposal(
    proposal_id: ProposalId,
    patch: ScenePatch,
    realization: RealizationPlan,
    render_cleanup_action: crate::core::runtime_reducer::RenderCleanupAction,
    render_side_effects: crate::core::runtime_reducer::RenderSideEffects,
    animation_schedule: AnimationSchedule,
) -> Result<InFlightProposal, crate::core::state::ProposalShapeError> {
    match realization {
        RealizationPlan::Draw(draw) => InFlightProposal::draw(
            proposal_id,
            patch,
            draw,
            render_cleanup_action,
            render_side_effects,
            animation_schedule,
        ),
        RealizationPlan::Clear(clear) => InFlightProposal::clear(
            proposal_id,
            patch,
            clear,
            render_cleanup_action,
            render_side_effects,
            animation_schedule,
        ),
        RealizationPlan::Noop => InFlightProposal::noop(
            proposal_id,
            patch,
            render_cleanup_action,
            render_side_effects,
            animation_schedule,
        ),
        RealizationPlan::Failure(failure) => Ok(InFlightProposal::failure(
            proposal_id,
            patch,
            failure,
            render_cleanup_action,
            render_side_effects,
            animation_schedule,
        )),
    }
}

pub(crate) fn build_planned_render(
    planning: RenderPlanningContext,
    proposal_id: ProposalId,
    render_decision: &RenderDecision,
    animation_schedule: AnimationSchedule,
) -> Result<PlannedRender, crate::core::state::ProposalShapeError> {
    let (current_scene, observation, acknowledged_projection, clear_all_max_kept_windows) =
        planning.into_parts();
    let (scene_update, projection, projection_failure) =
        update_scene_from_render_decision_with_context(
            current_scene.as_ref(),
            observation.as_ref(),
            acknowledged_projection.as_ref(),
            render_decision,
        );
    let basis = patch_basis(acknowledged_projection, projection);
    let realization = realization_plan_for_render_decision(
        clear_all_max_kept_windows,
        render_decision,
        basis.target(),
        projection_failure,
    );
    let patch = ScenePatch::derive(basis);
    build_in_flight_proposal(
        proposal_id,
        patch,
        realization,
        render_decision.render_cleanup_action,
        render_decision.render_side_effects,
        animation_schedule,
    )
    .map(|proposal| PlannedRender::new(scene_update, proposal))
}

#[cfg(test)]
mod tests {
    use super::AnimationSchedule;
    use super::PaletteSpec;
    use super::build_in_flight_proposal;
    use super::dirty_entities;
    use super::patch_basis;
    use super::planner_seed;
    use super::project_draw_frame;
    use super::realization_plan_for_render_decision;
    use super::reusable_projection_entry;
    use super::update_scene_from_render_decision_with_context;
    use crate::core::runtime_reducer::RenderAction;
    use crate::core::runtime_reducer::RenderDecision;
    use crate::core::runtime_reducer::TargetCellPresentation;
    use crate::core::state::BackgroundProbeChunkMask;
    use crate::core::state::BackgroundProbePlan;
    use crate::core::state::BackgroundProbeUpdate;
    use crate::core::state::BufferPerfClass;
    use crate::core::state::CoreState;
    use crate::core::state::CursorTrailGeometry;
    use crate::core::state::CursorTrailProjectionPolicy;
    use crate::core::state::CursorTrailSemantic;
    use crate::core::state::ExternalDemand;
    use crate::core::state::ExternalDemandKind;
    use crate::core::state::ObservationBasis;
    use crate::core::state::ObservationMotion;
    use crate::core::state::ObservationRequest;
    use crate::core::state::ObservationSnapshot;
    use crate::core::state::PatchBasis;
    use crate::core::state::ProbeKind;
    use crate::core::state::ProbeRequestSet;
    use crate::core::state::ProbeReuse;
    use crate::core::state::ProbeState;
    use crate::core::state::ProjectionCache;
    use crate::core::state::RealizationClear;
    use crate::core::state::RealizationDivergence;
    use crate::core::state::RealizationDraw;
    use crate::core::state::RealizationLedger;
    use crate::core::state::RealizationPlan;
    use crate::core::state::ScenePatch;
    use crate::core::state::ScenePatchKind;
    use crate::core::state::SceneState;
    use crate::core::state::SemanticEntity;
    use crate::core::state::SemanticEntityId;
    use crate::core::state::SemanticScene;
    use crate::core::types::CursorCol;
    use crate::core::types::CursorPosition;
    use crate::core::types::CursorRow;
    use crate::core::types::IngressSeq;
    use crate::core::types::Millis;
    use crate::core::types::ObservationId;
    use crate::core::types::ProposalId;
    use crate::core::types::SceneRevision;
    use crate::core::types::StrokeId;
    use crate::core::types::ViewportSnapshot;
    use crate::draw::render_plan::PlannerState as ProjectionPlannerState;
    use crate::state::CursorLocation;
    use crate::test_support::proptest::pure_config;
    use crate::types::CursorCellShape;
    use crate::types::Particle;
    use crate::types::Point;
    use crate::types::RenderFrame;
    use crate::types::RenderStepSample;
    use crate::types::ScreenCell;
    use crate::types::StaticRenderConfig;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;
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
            step_samples: vec![RenderStepSample::new(corners, 1.0)].into(),
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
            particle_count: 0,
            aggregated_particle_cells: Arc::default(),
            particle_screen_cells: Arc::default(),
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
                BufferPerfClass::Full,
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
        ObservationSnapshot::new(request, basis, ObservationMotion::default())
    }

    fn observation_with_background_probe(
        seq: u64,
        allowed_cells: &[ScreenCell],
    ) -> ObservationSnapshot {
        let request = ObservationRequest::new(
            ExternalDemand::new(
                IngressSeq::new(seq),
                ExternalDemandKind::ExternalCursor,
                Millis::new(seq),
                None,
                BufferPerfClass::Full,
            ),
            ProbeRequestSet::new(false, true),
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
        let snapshot =
            ObservationSnapshot::new(request.clone(), basis, ObservationMotion::default())
                .with_background_probe_plan(BackgroundProbePlan::from_cells(
                    allowed_cells.to_vec(),
                ));
        let progress = snapshot
            .background_progress()
            .expect("background probe plan should remain pending until sampled");
        let chunk = progress
            .next_chunk()
            .expect("single-cell sparse probe should emit one chunk");
        let allowed_mask = BackgroundProbeChunkMask::from_allowed_mask(&vec![true; chunk.len()]);
        let BackgroundProbeUpdate::Complete(batch) = progress
            .apply_chunk(&chunk, &allowed_mask)
            .expect("sparse chunk should materialize a ready background batch")
        else {
            panic!("single chunk sparse probe should complete immediately");
        };
        snapshot
            .with_background_probe(ProbeState::ready(
                ProbeKind::Background.request_id(request.observation_id()),
                request.observation_id(),
                ProbeReuse::Exact,
                batch,
            ))
            .expect("requested background probe should accept the sampled batch")
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum DirtyMutationAxis {
        None,
        PaletteOnly,
        Presentation,
        Geometry,
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum ReuseMutationAxis {
        Exact,
        ObservationWitness,
        SceneRevision,
        ParticleOverlay,
        Presentation,
        Policy,
    }

    fn dirty_mutation_axis_strategy() -> BoxedStrategy<DirtyMutationAxis> {
        prop_oneof![
            Just(DirtyMutationAxis::None),
            Just(DirtyMutationAxis::PaletteOnly),
            Just(DirtyMutationAxis::Presentation),
            Just(DirtyMutationAxis::Geometry),
        ]
        .boxed()
    }

    fn reuse_mutation_axis_strategy() -> BoxedStrategy<ReuseMutationAxis> {
        prop_oneof![
            Just(ReuseMutationAxis::Exact),
            Just(ReuseMutationAxis::ObservationWitness),
            Just(ReuseMutationAxis::SceneRevision),
            Just(ReuseMutationAxis::ParticleOverlay),
            Just(ReuseMutationAxis::Presentation),
            Just(ReuseMutationAxis::Policy),
        ]
        .boxed()
    }

    fn target_cell_presentation_strategy() -> BoxedStrategy<TargetCellPresentation> {
        prop_oneof![
            Just(TargetCellPresentation::None),
            Just(TargetCellPresentation::OverlayCursorCell(
                CursorCellShape::Block
            )),
            Just(TargetCellPresentation::OverlayCursorCell(
                CursorCellShape::VerticalBar,
            )),
            Just(TargetCellPresentation::OverlayCursorCell(
                CursorCellShape::HorizontalBar,
            )),
        ]
        .boxed()
    }

    fn alternate_target_cell_presentation(
        target_cell_presentation: TargetCellPresentation,
    ) -> TargetCellPresentation {
        match target_cell_presentation {
            TargetCellPresentation::None => {
                TargetCellPresentation::OverlayCursorCell(CursorCellShape::Block)
            }
            TargetCellPresentation::OverlayCursorCell(CursorCellShape::Block) => {
                TargetCellPresentation::None
            }
            TargetCellPresentation::OverlayCursorCell(CursorCellShape::VerticalBar) => {
                TargetCellPresentation::OverlayCursorCell(CursorCellShape::HorizontalBar)
            }
            TargetCellPresentation::OverlayCursorCell(CursorCellShape::HorizontalBar) => {
                TargetCellPresentation::OverlayCursorCell(CursorCellShape::VerticalBar)
            }
        }
    }

    fn frame_with_background_probe_requirement(mut frame: RenderFrame) -> RenderFrame {
        let mut static_config = (*frame.static_config).clone();
        static_config.particles_over_text = false;
        frame.static_config = Arc::new(static_config);
        frame.set_particles(std::sync::Arc::new(vec![Particle {
            position: Point {
                row: 16.2,
                col: 18.4,
            },
            velocity: Point::ZERO,
            lifetime: 0.75,
        }]));
        frame
    }

    fn observation_for_projection(
        seq: u64,
        requires_background_probe: bool,
    ) -> ObservationSnapshot {
        if requires_background_probe {
            observation_with_background_probe(
                seq,
                &[ScreenCell::new(16, 18).expect("particle cell should be visible")],
            )
        } else {
            observation(seq)
        }
    }

    fn frame_with_policy_drift(mut frame: RenderFrame) -> RenderFrame {
        let mut drifted_static_config = (*frame.static_config).clone();
        drifted_static_config.color_levels = drifted_static_config.color_levels.saturating_add(1);
        frame.static_config = Arc::new(drifted_static_config);
        frame
    }

    fn frame_with_particle_overlay_drift(mut frame: RenderFrame) -> RenderFrame {
        frame.set_particles(Arc::new(vec![Particle {
            position: Point {
                row: 10.75,
                col: 11.25,
            },
            velocity: Point::ZERO,
            lifetime: 0.8,
        }]));
        frame
    }

    proptest! {
        #![proptest_config(pure_config())]

        #[test]
        fn prop_dirty_entities_track_semantic_cursor_trail_identity(
            mutation_axis in dirty_mutation_axis_strategy(),
            initial_presentation in target_cell_presentation_strategy(),
            color_at_cursor in any::<u32>(),
        ) {
            let previous_frame = base_frame();
            let mut next_frame = previous_frame.clone();
            let next_presentation = match mutation_axis {
                DirtyMutationAxis::None => initial_presentation,
                DirtyMutationAxis::PaletteOnly => {
                    next_frame.color_at_cursor = Some(color_at_cursor);
                    initial_presentation
                }
                DirtyMutationAxis::Presentation => {
                    alternate_target_cell_presentation(initial_presentation)
                }
                DirtyMutationAxis::Geometry => {
                    next_frame.retarget_epoch = next_frame.retarget_epoch.saturating_add(1);
                    initial_presentation
                }
            };

            let previous = SemanticScene::default().with_entity(SemanticEntity::CursorTrail(
                CursorTrailSemantic::from_render_frame(&previous_frame, initial_presentation),
            ));
            let next = SemanticScene::default().with_entity(SemanticEntity::CursorTrail(
                CursorTrailSemantic::from_render_frame(&next_frame, next_presentation),
            ));
            let dirty = dirty_entities(&previous, &next);
            let expected_dirty = previous.entity(SemanticEntityId::CursorTrail)
                != next.entity(SemanticEntityId::CursorTrail);

            prop_assert_eq!(dirty.is_empty(), !expected_dirty);
            if expected_dirty {
                prop_assert_eq!(
                    dirty.entities(),
                    &BTreeSet::from([SemanticEntityId::CursorTrail])
                );
            }

            match mutation_axis {
                DirtyMutationAxis::PaletteOnly => prop_assert!(dirty.is_empty()),
                DirtyMutationAxis::Presentation | DirtyMutationAxis::Geometry => {
                    prop_assert!(!dirty.is_empty())
                }
                DirtyMutationAxis::None => prop_assert!(dirty.is_empty()),
            }
        }

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

            let geometry = CursorTrailGeometry::from_render_frame(&frame);
            let cached_policy = CursorTrailProjectionPolicy::from_render_frame(&frame);
            let cached_observation =
                observation_for_projection(observation_seq, requires_background_probe);
            let cached = project_draw_frame(
                &SceneState::default(),
                SceneRevision::INITIAL,
                &cached_observation,
                &geometry,
                &cached_policy,
                target_cell_presentation,
            )
            .expect("cached projection fixture should be valid");
            let cached_planner_state = cached.planner_state().clone();
            let current_scene =
                SceneState::default().with_projection(ProjectionCache::Ready(Box::new(cached)));
            let query_frame = match mutation_axis {
                ReuseMutationAxis::ParticleOverlay => frame_with_particle_overlay_drift(frame.clone()),
                ReuseMutationAxis::Exact
                | ReuseMutationAxis::ObservationWitness
                | ReuseMutationAxis::SceneRevision
                | ReuseMutationAxis::Presentation
                | ReuseMutationAxis::Policy => frame.clone(),
            };
            let query_geometry = CursorTrailGeometry::from_render_frame(&query_frame);

            let query_observation = match mutation_axis {
                ReuseMutationAxis::ObservationWitness => {
                    observation_for_projection(observation_seq.saturating_add(1), requires_background_probe)
                }
                ReuseMutationAxis::Exact
                | ReuseMutationAxis::SceneRevision
                | ReuseMutationAxis::ParticleOverlay
                | ReuseMutationAxis::Presentation
                | ReuseMutationAxis::Policy => cached_observation,
            };
            let scene_revision = match mutation_axis {
                ReuseMutationAxis::SceneRevision => SceneRevision::INITIAL.next(),
                ReuseMutationAxis::Exact
                | ReuseMutationAxis::ObservationWitness
                | ReuseMutationAxis::ParticleOverlay
                | ReuseMutationAxis::Presentation
                | ReuseMutationAxis::Policy => SceneRevision::INITIAL,
            };
            let query_presentation = match mutation_axis {
                ReuseMutationAxis::Presentation => {
                    alternate_target_cell_presentation(target_cell_presentation)
                }
                ReuseMutationAxis::Exact
                | ReuseMutationAxis::ObservationWitness
                | ReuseMutationAxis::SceneRevision
                | ReuseMutationAxis::ParticleOverlay
                | ReuseMutationAxis::Policy => target_cell_presentation,
            };
            let query_policy = match mutation_axis {
                ReuseMutationAxis::Policy => {
                    CursorTrailProjectionPolicy::from_render_frame(&frame_with_policy_drift(frame))
                }
                ReuseMutationAxis::Exact
                | ReuseMutationAxis::ObservationWitness
                | ReuseMutationAxis::SceneRevision
                | ReuseMutationAxis::ParticleOverlay
                | ReuseMutationAxis::Presentation => cached_policy,
            };

            let reused = reusable_projection_entry(
                &current_scene,
                scene_revision,
                &query_observation,
                &query_geometry,
                &query_policy,
                query_presentation,
            );
            let expected_seed = match mutation_axis {
                ReuseMutationAxis::Policy => ProjectionPlannerState::default(),
                ReuseMutationAxis::Exact
                | ReuseMutationAxis::ObservationWitness
                | ReuseMutationAxis::SceneRevision
                | ReuseMutationAxis::ParticleOverlay
                | ReuseMutationAxis::Presentation => cached_planner_state,
            };
            let expected_reuse = matches!(
                mutation_axis,
                ReuseMutationAxis::Exact
                    | ReuseMutationAxis::ObservationWitness
                    | ReuseMutationAxis::SceneRevision
                    | ReuseMutationAxis::ParticleOverlay
            ) && !advances_planner;

            prop_assert_eq!(planner_seed(&current_scene, &query_policy), expected_seed);
            prop_assert_eq!(reused.is_some(), expected_reuse);

            if let Some(reused) = reused {
                prop_assert_eq!(
                    reused.reuse_key().target_cell_presentation(),
                    target_cell_presentation
                );
                prop_assert_eq!(
                    reused.snapshot().witness().observation_id(),
                    query_observation.request().observation_id(),
                );
            }
        }

        #[test]
        fn prop_project_draw_frame_advances_planner_history_across_observations(
            observation_seq in 1_u64..=(u16::MAX as u64 - 1),
            target_cell_presentation in target_cell_presentation_strategy(),
        ) {
            let frame = base_frame();
            let geometry = CursorTrailGeometry::from_render_frame(&frame);
            let policy = CursorTrailProjectionPolicy::from_render_frame(&frame);
            let first_observation = observation(observation_seq);
            let first = project_draw_frame(
                &SceneState::default(),
                SceneRevision::INITIAL,
                &first_observation,
                &geometry,
                &policy,
                target_cell_presentation,
            )
            .expect("first projection should succeed");
            let current_scene =
                SceneState::default().with_projection(ProjectionCache::Ready(Box::new(first.clone())));
            let second = project_draw_frame(
                &current_scene,
                SceneRevision::INITIAL.next(),
                &observation(observation_seq.saturating_add(1)),
                &geometry,
                &policy,
                target_cell_presentation,
            )
            .expect("second projection should succeed");

            prop_assert_eq!(planner_seed(&current_scene, &policy), first.planner_state().clone());
            prop_assert_ne!(first.planner_state(), second.planner_state());
        }

    }

    #[test]
    fn reusable_projection_entry_refreshes_particle_overlay_without_replanning_the_trail() {
        let mut cached_frame = base_frame();
        cached_frame.step_samples = Vec::new().into();
        cached_frame.planner_idle_steps = 0;
        cached_frame.set_particles(Arc::new(vec![Particle {
            position: Point {
                row: 10.25,
                col: 10.25,
            },
            velocity: Point::ZERO,
            lifetime: 1.0,
        }]));
        let mut query_frame = cached_frame.clone();
        query_frame.set_particles(Arc::new(vec![Particle {
            position: Point {
                row: 10.75,
                col: 11.25,
            },
            velocity: Point::ZERO,
            lifetime: 0.8,
        }]));

        let cached_geometry = CursorTrailGeometry::from_render_frame(&cached_frame);
        let query_geometry = CursorTrailGeometry::from_render_frame(&query_frame);
        let policy = CursorTrailProjectionPolicy::from_render_frame(&cached_frame);
        let cached_observation = observation(1);
        let query_observation = observation(2);
        let cached = project_draw_frame(
            &SceneState::default(),
            SceneRevision::INITIAL,
            &cached_observation,
            &cached_geometry,
            &policy,
            TargetCellPresentation::None,
        )
        .expect("cached particle projection should succeed");
        let current_scene =
            SceneState::default().with_projection(ProjectionCache::Ready(Box::new(cached)));
        let reused = reusable_projection_entry(
            &current_scene,
            SceneRevision::INITIAL.next(),
            &query_observation,
            &query_geometry,
            &policy,
            TargetCellPresentation::None,
        )
        .expect("particle-only drift should still reuse the cached trail");
        let fully_reprojected = project_draw_frame(
            &current_scene,
            SceneRevision::INITIAL.next(),
            &query_observation,
            &query_geometry,
            &policy,
            TargetCellPresentation::None,
        )
        .expect("full particle reprojection should succeed");

        assert_eq!(reused.snapshot(), fully_reprojected.snapshot());
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
    fn clear_action_without_acknowledged_projection_still_uses_clear_realization() {
        let decision = RenderDecision {
            render_action: RenderAction::ClearAll,
            render_cleanup_action: crate::core::runtime_reducer::RenderCleanupAction::NoAction,
            render_allocation_policy:
                crate::core::runtime_reducer::RenderAllocationPolicy::ReuseOnly,
            render_side_effects: crate::core::runtime_reducer::RenderSideEffects::default(),
        };

        let clear_all_max_kept_windows = CoreState::default().runtime().config.max_kept_windows;

        assert_eq!(
            realization_plan_for_render_decision(clear_all_max_kept_windows, &decision, None, None,),
            RealizationPlan::Clear(RealizationClear::new(clear_all_max_kept_windows))
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
        let realization = RealizationLedger::Diverged {
            last_consistent: Some(cached.snapshot().clone()),
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
        let clear_all_max_kept_windows = CoreState::default().runtime().config.max_kept_windows;

        assert!(next_scene.projection_entry().is_some());
        assert!(projection_failure.is_none());
        assert_eq!(projection, None);
        assert_eq!(patch.kind(), ScenePatchKind::Noop);
        assert_eq!(
            realization_plan_for_render_decision(
                clear_all_max_kept_windows,
                &decision,
                projection.as_ref(),
                projection_failure,
            ),
            RealizationPlan::Noop
        );
    }

    #[test]
    fn invalid_proposal_shape_returns_typed_error_instead_of_panicking() {
        let patch = ScenePatch::derive(PatchBasis::new(None, None));

        let result = build_in_flight_proposal(
            ProposalId::new(11),
            patch,
            RealizationPlan::Draw(RealizationDraw::new(
                PaletteSpec::from_frame(&base_frame()),
                crate::core::runtime_reducer::RenderAllocationPolicy::ReuseOnly,
                32,
            )),
            crate::core::runtime_reducer::RenderCleanupAction::NoAction,
            crate::core::runtime_reducer::RenderSideEffects::default(),
            AnimationSchedule::Idle,
        );

        assert_eq!(
            result,
            Err(crate::core::state::ProposalShapeError::DrawMissingTargetProjection)
        );
    }
}
