use super::Transition;
use super::support::request_render_plan_effect;
use crate::config::RuntimeConfig;
use crate::core::effect::RenderPlanningContext;
use crate::core::effect::RenderPlanningObservation;
use crate::core::realization::PaletteSpec;
use crate::core::realization::project_particle_overlay_cells;
use crate::core::realization::project_render_plan;
use crate::core::runtime_reducer::CursorEventContext;
use crate::core::runtime_reducer::EventSource;
use crate::core::runtime_reducer::MotionTarget;
use crate::core::runtime_reducer::RenderAction;
use crate::core::runtime_reducer::RenderDecision;
use crate::core::runtime_reducer::TargetCellPresentation;
use crate::core::runtime_reducer::select_event_source;
use crate::core::state::AnimationSchedule;
use crate::core::state::ApplyFailureKind;
use crate::core::state::BackgroundProbeBatch;
use crate::core::state::BackgroundProbePlan;
use crate::core::state::CoreState;
use crate::core::state::CursorTrailSemantic;
use crate::core::state::InFlightProposal;
use crate::core::state::ObservationSnapshot;
use crate::core::state::PatchBasis;
use crate::core::state::PlannedProjectionUpdate;
use crate::core::state::PlannedRender;
use crate::core::state::PlannedSceneUpdate;
use crate::core::state::PreparedObservationPlan;
use crate::core::state::ProjectionHandle;
use crate::core::state::ProjectionPlannerClock;
use crate::core::state::ProjectionReuseKey;
use crate::core::state::ProjectionWitness;
use crate::core::state::RealizationClear;
use crate::core::state::RealizationDivergence;
use crate::core::state::RealizationDraw;
use crate::core::state::RealizationFailure;
use crate::core::state::RealizationPlan;
use crate::core::state::RetainedProjection;
use crate::core::state::ScenePatch;
use crate::core::state::SceneState;
use crate::core::state::classify_semantic_event;
use crate::core::types::Millis;
use crate::core::types::MotionRevision;
use crate::core::types::ObservationId;
use crate::core::types::ProjectionPolicyRevision;
use crate::core::types::ProjectorRevision;
use crate::core::types::ProposalId;
use crate::core::types::RenderRevision;
use crate::draw::render_plan::PlannerState as ProjectionPlannerState;
use crate::draw::render_plan::{self};
use crate::position::RenderPoint;
use crate::position::ScreenCell;
use crate::position::ViewportBounds;
use crate::state::TrackedCursor;
use crate::types::DEFAULT_RNG_STATE;
use crate::types::RenderFrame;
use std::cell::OnceCell;
#[cfg(test)]
use std::collections::BTreeSet;
use std::collections::hash_map::DefaultHasher;
use std::hash::Hash;
use std::hash::Hasher;

#[cfg(test)]
use crate::core::state::SemanticEntityId;

struct PlannedRuntimeTransition {
    transition: crate::core::runtime_reducer::CursorTransition,
}

fn deterministic_event_seed(observation: &ObservationSnapshot) -> u32 {
    let observed_at = observation.basis().observed_at().value() as u32;
    let observation_id = observation.observation_id().value() as u32;
    let mixed = observation_id ^ observed_at.rotate_left(13) ^ 0x9E37_79B9;
    if mixed == 0 { DEFAULT_RNG_STATE } else { mixed }
}

fn render_planning_observation(observation: &ObservationSnapshot) -> RenderPlanningObservation {
    RenderPlanningObservation::new(
        observation.observation_id(),
        observation.basis().viewport(),
        observation.probes().background().batch().cloned(),
    )
}

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

fn frame_requires_background_probe(frame: &RenderFrame) -> bool {
    !frame.particles_over_text && frame.has_particles()
}

struct PreparedProjection<'a> {
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

fn prepare_projection<'a>(
    current_scene: &SceneState,
    render_revision: RenderRevision,
    observation: &'a RenderPlanningObservation,
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

fn project_prepared_frame(
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

#[cfg(test)]
fn project_draw_frame(
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

fn reusable_prepared_projection(
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
fn reusable_projection(
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

fn update_scene_from_render_decision_with_context(
    current_scene: &SceneState,
    observation: Option<&RenderPlanningObservation>,
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

fn patch_basis(
    acknowledged_projection: Option<ProjectionHandle>,
    target: Option<ProjectionHandle>,
) -> PatchBasis {
    PatchBasis::new(acknowledged_projection, target)
}

fn realization_plan_for_render_decision(
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

fn plan_runtime_transition_for_runtime(
    runtime: &mut crate::state::RuntimeState,
    latest_exact_cursor_cell: Option<ScreenCell>,
    previous_observation: Option<&ObservationSnapshot>,
    observation: &ObservationSnapshot,
    observed_at: Millis,
) -> PlannedRuntimeTransition {
    runtime.set_color_at_cursor(observation.cursor_color());

    let mode = observation.basis().mode();
    let surface = observation.basis().surface();
    let cursor = observation.basis().cursor();
    let tracked_cursor = TrackedCursor::new(surface, cursor.buffer_line());
    let motion_target = match observation.basis().cursor().cell() {
        crate::position::ObservedCell::Exact(cell) => MotionTarget::Available(cell),
        crate::position::ObservedCell::Deferred(cell) => MotionTarget::Available(cell),
        crate::position::ObservedCell::Unavailable => {
            latest_exact_cursor_cell.map_or(MotionTarget::Unavailable, MotionTarget::Available)
        }
    };
    let fallback_target = runtime.target_position();
    // The exact cursor slot is a boundary-refresh anchor and fallback, not the primary motion
    // target. When an observation carries a newer cursor sample whose exactness is deferred
    // (for example while conceal correction is still pending), motion must follow that observed
    // position or repeated hjkl ingress appears frozen at the stale exact anchor.
    let semantic_event = classify_semantic_event(previous_observation, observation);
    let source = select_event_source(
        mode,
        runtime,
        semantic_event,
        motion_target,
        &tracked_cursor,
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
    let event_target = match motion_target {
        MotionTarget::Available(target_cell) => RenderPoint::from(target_cell),
        MotionTarget::Unavailable => fallback_target,
    };

    let event = CursorEventContext {
        row: event_target.row,
        col: event_target.col,
        now_ms: event_now_ms,
        seed: deterministic_event_seed(observation),
        tracked_cursor,
        scroll_shift: observation.scroll_shift(),
        semantic_event,
    };
    let transition = crate::core::runtime_reducer::reduce_cursor_event_for_perf_class(
        runtime,
        mode,
        &event,
        source,
        observation.demand().buffer_perf_class(),
    );
    PlannedRuntimeTransition { transition }
}

fn prepare_observation_plan_with_runtime(
    mut runtime: crate::state::RuntimeState,
    latest_exact_cursor_cell: Option<ScreenCell>,
    previous_observation: Option<&ObservationSnapshot>,
    observation: &ObservationSnapshot,
    observed_at: Millis,
) -> (crate::state::RuntimeState, PreparedObservationPlan) {
    let mut preview_runtime = crate::state::RuntimePreview::new(&mut runtime);
    let planned_transition = plan_runtime_transition_for_runtime(
        preview_runtime.runtime_mut(),
        latest_exact_cursor_cell,
        previous_observation,
        observation,
        observed_at,
    );
    let prepared_plan = if !preview_runtime.runtime_changed_since_preview() {
        let preview_particles = preview_runtime.into_preview_particle_storage();
        runtime.reclaim_preview_particles_scratch(preview_particles);
        PreparedObservationPlan::unchanged(planned_transition.transition)
    } else {
        PreparedObservationPlan::new(
            preview_runtime.into_prepared_motion(),
            planned_transition.transition,
        )
    };
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
        state.latest_exact_cursor_cell(),
        previous_observation,
        observation,
        observed_at,
    );
    (state.with_runtime(runtime), prepared_plan)
}

pub(super) fn background_probe_plan(
    prepared_plan: &PreparedObservationPlan,
    viewport: ViewportBounds,
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
    let cursor_transition = prepared_plan.apply_to_runtime_and_take_transition(state.runtime_mut());
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

    let Some(planning_observation) = state.observation().map(render_planning_observation) else {
        // Surprising: `plan_ready_state` lost its ready observation before planning.
        return Transition::stay(&state);
    };

    let (state, proposal_id) = state.allocate_proposal_id();
    let animation_schedule = AnimationSchedule::from_parts(
        should_schedule_next_animation,
        next_animation_at_ms.map(Millis::new),
    );
    if state.lifecycle() != crate::core::types::Lifecycle::Ready {
        // Surprising: `plan_ready_state` was invoked outside the ready lifecycle boundary.
        return Transition::stay(&state);
    }
    let planning = RenderPlanningContext::new(
        state.shared_scene(),
        Some(planning_observation),
        state
            .realization()
            .trusted_acknowledged_for_patch()
            .cloned(),
        std::sync::Arc::new(state.runtime().config.clone()),
    );
    let Some(next_state) = state.enter_planning(proposal_id) else {
        unreachable!("ready lifecycle checked above")
    };

    Transition::new(
        next_state,
        vec![request_render_plan_effect(
            planning,
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
    let latest_exact_cursor_cell = state.latest_exact_cursor_cell();
    let planned_transition = {
        let Some((runtime, observation)) = state.runtime_mut_with_observation() else {
            return Transition::stay_owned(state);
        };
        plan_runtime_transition_for_runtime(
            runtime,
            latest_exact_cursor_cell,
            previous_observation,
            observation,
            observed_at,
        )
    };
    plan_ready_state_with_transition(state, observed_at, planned_transition.transition)
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
    let (current_scene, observation, acknowledged_projection, config) = planning.into_parts();
    let (scene_update, projection, projection_failure) =
        update_scene_from_render_decision_with_context(
            &current_scene,
            observation.as_ref(),
            acknowledged_projection.as_ref(),
            render_decision,
        );
    let realization = realization_plan_for_render_decision(
        config.as_ref(),
        render_decision,
        projection.as_ref(),
        projection_failure,
    );
    let basis = patch_basis(acknowledged_projection, projection);
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
    use super::build_planned_render;
    use super::dirty_entities;
    use super::patch_basis;
    use super::planner_seed;
    use super::project_draw_frame;
    use super::realization_plan_for_render_decision;
    use super::reusable_projection;
    use super::update_scene_from_render_decision_with_context;
    use crate::config::RuntimeConfig;
    use crate::core::effect::RenderPlanningContext;
    use crate::core::runtime_reducer::RenderAction;
    use crate::core::runtime_reducer::RenderDecision;
    use crate::core::runtime_reducer::TargetCellPresentation;
    use crate::core::state::BackgroundProbeChunkMask;
    use crate::core::state::BackgroundProbePlan;
    use crate::core::state::BufferPerfClass;
    use crate::core::state::CursorTrailSemantic;
    use crate::core::state::ExternalDemand;
    use crate::core::state::ExternalDemandKind;
    use crate::core::state::ObservationBasis;
    use crate::core::state::ObservationMotion;
    use crate::core::state::ObservationSnapshot;
    use crate::core::state::PatchBasis;
    use crate::core::state::PendingObservation;
    use crate::core::state::ProbeKind;
    use crate::core::state::ProbeRequestSet;
    use crate::core::state::RealizationClear;
    use crate::core::state::RealizationDivergence;
    use crate::core::state::RealizationDraw;
    use crate::core::state::RealizationLedger;
    use crate::core::state::RealizationPlan;
    use crate::core::state::ScenePatch;
    use crate::core::state::ScenePatchKind;
    use crate::core::state::SceneState;
    use crate::core::state::SemanticEntityId;
    use crate::core::types::IngressSeq;
    use crate::core::types::Millis;
    use crate::core::types::MotionRevision;
    use crate::core::types::ProjectionPolicyRevision;
    use crate::core::types::ProposalId;
    use crate::core::types::RenderRevision;
    use crate::core::types::SemanticRevision;
    use crate::core::types::StrokeId;
    use crate::draw::render_plan::PlannerState as ProjectionPlannerState;
    use crate::position::CursorObservation;
    use crate::position::ObservedCell;
    use crate::position::RenderPoint;
    use crate::position::ScreenCell;
    use crate::position::ViewportBounds;
    use crate::state::CursorShape;
    use crate::state::RuntimeState;
    use crate::state::TrackedCursor;
    use crate::test_support::proptest::pure_config;
    use crate::types::CursorCellShape;
    use crate::types::ModeClass;
    use crate::types::Particle;
    use crate::types::RenderFrame;
    use crate::types::RenderStepSample;
    use crate::types::StaticRenderConfig;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;
    use std::collections::BTreeSet;
    use std::sync::Arc;

    fn screen_cell(row: i64, col: i64) -> ScreenCell {
        ScreenCell::new(row, col).expect("positive cursor position")
    }

    fn viewport_bounds(max_row: i64, max_col: i64) -> ViewportBounds {
        ViewportBounds::new(max_row, max_col).expect("positive viewport bounds")
    }

    fn observation_basis(
        seq: u64,
        observed_cell: ObservedCell,
        location: TrackedCursor,
    ) -> ObservationBasis {
        ObservationBasis::new(
            Millis::new(seq),
            "n".to_string(),
            location.surface(),
            CursorObservation::new(location.buffer_line(), observed_cell),
            viewport_bounds(40, 120),
        )
    }

    fn valid_surface_location() -> TrackedCursor {
        TrackedCursor::fixture(1, 1, 1, 1)
            .with_window_origin(1, 1)
            .with_window_dimensions(120, 40)
    }

    fn base_frame() -> RenderFrame {
        let corners = [
            RenderPoint {
                row: 12.0,
                col: 10.0,
            },
            RenderPoint {
                row: 12.0,
                col: 11.0,
            },
            RenderPoint {
                row: 13.0,
                col: 11.0,
            },
            RenderPoint {
                row: 13.0,
                col: 10.0,
            },
        ];
        RenderFrame {
            mode: ModeClass::NormalLike,
            corners,
            step_samples: vec![RenderStepSample::new(corners, 1.0)].into(),
            planner_idle_steps: 0,
            target: RenderPoint {
                row: 10.0,
                col: 10.0,
            },
            target_corners: [
                RenderPoint {
                    row: 10.0,
                    col: 10.0,
                },
                RenderPoint {
                    row: 10.0,
                    col: 11.0,
                },
                RenderPoint {
                    row: 11.0,
                    col: 11.0,
                },
                RenderPoint {
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
            projection_policy_revision: ProjectionPolicyRevision::INITIAL,
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

    fn render_revision(
        motion_revision: MotionRevision,
        semantic_revision: SemanticRevision,
    ) -> RenderRevision {
        RenderRevision::new(motion_revision, semantic_revision)
    }

    fn projection_policy_revision(frame: &RenderFrame) -> ProjectionPolicyRevision {
        frame.projection_policy_revision
    }

    fn observation(seq: u64) -> ObservationSnapshot {
        let request = PendingObservation::new(
            ExternalDemand::new(
                IngressSeq::new(seq),
                ExternalDemandKind::ExternalCursor,
                Millis::new(seq),
                BufferPerfClass::Full,
            ),
            ProbeRequestSet::default(),
        );
        let basis = observation_basis(
            seq,
            ObservedCell::Exact(screen_cell(10, 10)),
            valid_surface_location(),
        );
        ObservationSnapshot::new(request, basis, ObservationMotion::default())
    }

    #[test]
    fn prepare_observation_plan_reclaims_preview_scratch_when_runtime_is_unchanged() {
        let mut runtime = RuntimeState::default();
        runtime.initialize_cursor(
            RenderPoint {
                row: 10.0,
                col: 10.0,
            },
            CursorShape::block(),
            7,
            &valid_surface_location(),
        );
        runtime.record_observed_mode(/*current_is_cmdline*/ false);
        runtime.reclaim_preview_particles_scratch(Vec::with_capacity(8));

        let expected_scratch_capacity = runtime.preview_particles_scratch_capacity();
        let expected_scratch_ptr = runtime.preview_particles_scratch_ptr();

        let (returned_runtime, prepared_plan) = super::prepare_observation_plan_with_runtime(
            runtime,
            None,
            None,
            &observation(1),
            Millis::new(1),
        );

        assert_eq!(prepared_plan.prepared_particles_capacity(), 0);
        assert!(!prepared_plan.retains_preview_motion());
        assert_eq!(
            returned_runtime.preview_particles_scratch_capacity(),
            expected_scratch_capacity
        );
        assert_eq!(
            returned_runtime.preview_particles_scratch_ptr(),
            expected_scratch_ptr
        );
    }

    #[test]
    fn prepare_observation_plan_keeps_preview_motion_when_runtime_changes() {
        let mut runtime = RuntimeState::default();
        runtime.reclaim_preview_particles_scratch(Vec::with_capacity(8));

        let (_returned_runtime, prepared_plan) = super::prepare_observation_plan_with_runtime(
            runtime,
            None,
            None,
            &observation(1),
            Millis::new(1),
        );

        assert!(prepared_plan.retains_preview_motion());
    }

    #[test]
    fn exact_observation_retargets_to_the_new_exact_cell() {
        let mut runtime = RuntimeState::default();
        runtime.config.delay_event_to_smear = 24.0;
        runtime.initialize_cursor(
            RenderPoint {
                row: 10.0,
                col: 10.0,
            },
            CursorShape::block(),
            7,
            &valid_surface_location(),
        );
        runtime.record_observed_mode(/*current_is_cmdline*/ false);

        let request = PendingObservation::new(
            ExternalDemand::new(
                IngressSeq::new(2),
                ExternalDemandKind::ExternalCursor,
                Millis::new(2),
                BufferPerfClass::Full,
            ),
            ProbeRequestSet::default(),
        );
        let observation = ObservationSnapshot::new(
            request,
            observation_basis(
                2,
                ObservedCell::Exact(screen_cell(9, 10)),
                valid_surface_location(),
            ),
            ObservationMotion::default(),
        );

        let planned = super::plan_runtime_transition_for_runtime(
            &mut runtime,
            Some(screen_cell(10, 10)),
            None,
            &observation,
            Millis::new(16),
        );

        assert_eq!(
            runtime.target_position(),
            RenderPoint {
                row: 9.0,
                col: 10.0
            }
        );
        assert!(runtime.is_settling() || runtime.is_animating());
        assert!(planned.transition.should_schedule_next_animation);
    }

    #[test]
    fn conceal_deferred_motion_prefers_observed_cursor_over_stale_exact_anchor() {
        let mut runtime = RuntimeState::default();
        runtime.config.delay_event_to_smear = 24.0;
        runtime.initialize_cursor(
            RenderPoint {
                row: 10.0,
                col: 10.0,
            },
            CursorShape::block(),
            7,
            &valid_surface_location(),
        );
        runtime.record_observed_mode(/*current_is_cmdline*/ false);

        let request = PendingObservation::new(
            ExternalDemand::new(
                IngressSeq::new(2),
                ExternalDemandKind::ExternalCursor,
                Millis::new(2),
                BufferPerfClass::Full,
            ),
            ProbeRequestSet::default(),
        );
        let observation = ObservationSnapshot::new(
            request,
            observation_basis(
                2,
                ObservedCell::Deferred(screen_cell(9, 10)),
                valid_surface_location(),
            ),
            ObservationMotion::default(),
        );

        let planned = super::plan_runtime_transition_for_runtime(
            &mut runtime,
            Some(screen_cell(10, 10)),
            None,
            &observation,
            Millis::new(16),
        );

        assert_eq!(
            runtime.target_position(),
            RenderPoint {
                row: 9.0,
                col: 10.0
            }
        );
        assert!(runtime.is_settling() || runtime.is_animating());
        assert!(planned.transition.should_schedule_next_animation);
    }

    #[test]
    fn conceal_deferred_retarget_while_animating_uses_observed_cursor_over_stale_exact_anchor() {
        let mut runtime = RuntimeState::default();
        let shape = CursorShape::block();
        let location = valid_surface_location();
        runtime.initialize_cursor(
            RenderPoint {
                row: 10.0,
                col: 10.0,
            },
            shape,
            7,
            &location,
        );
        runtime.record_observed_mode(/*current_is_cmdline*/ false);
        runtime.retarget_preserving_current_pose(
            crate::state::RuntimeTargetSnapshot::preserving_tracking(
                RenderPoint {
                    row: 9.0,
                    col: 10.0,
                },
                shape,
                runtime.tracked_cursor_ref(),
            ),
        );
        runtime.start_animation_towards_target();
        runtime.record_animation_tick(100.0);

        let request = PendingObservation::new(
            ExternalDemand::new(
                IngressSeq::new(3),
                ExternalDemandKind::ExternalCursor,
                Millis::new(3),
                BufferPerfClass::Full,
            ),
            ProbeRequestSet::default(),
        );
        let observation = ObservationSnapshot::new(
            request,
            observation_basis(3, ObservedCell::Deferred(screen_cell(8, 10)), location),
            ObservationMotion::default(),
        );

        let planned = super::plan_runtime_transition_for_runtime(
            &mut runtime,
            Some(screen_cell(9, 10)),
            None,
            &observation,
            Millis::new(116),
        );

        assert_eq!(
            runtime.target_position(),
            RenderPoint {
                row: 8.0,
                col: 10.0
            }
        );
        assert!(runtime.is_animating());
        assert!(matches!(
            planned.transition.render_decision.render_action,
            RenderAction::Draw(_)
        ));
        assert!(planned.transition.should_schedule_next_animation);
    }

    #[test]
    fn unavailable_observation_falls_back_to_latest_exact_anchor() {
        let mut runtime = RuntimeState::default();
        runtime.config.delay_event_to_smear = 24.0;
        runtime.initialize_cursor(
            RenderPoint {
                row: 10.0,
                col: 10.0,
            },
            CursorShape::block(),
            7,
            &valid_surface_location(),
        );
        runtime.record_observed_mode(/*current_is_cmdline*/ false);

        let request = PendingObservation::new(
            ExternalDemand::new(
                IngressSeq::new(4),
                ExternalDemandKind::ExternalCursor,
                Millis::new(4),
                BufferPerfClass::Full,
            ),
            ProbeRequestSet::default(),
        );
        let observation = ObservationSnapshot::new(
            request,
            observation_basis(4, ObservedCell::Unavailable, valid_surface_location()),
            ObservationMotion::default(),
        );

        let planned = super::plan_runtime_transition_for_runtime(
            &mut runtime,
            Some(screen_cell(8, 10)),
            None,
            &observation,
            Millis::new(16),
        );

        assert_eq!(
            runtime.target_position(),
            RenderPoint {
                row: 8.0,
                col: 10.0
            }
        );
        assert!(runtime.is_settling() || runtime.is_animating());
        assert!(planned.transition.should_schedule_next_animation);
    }

    #[test]
    fn unavailable_observation_without_anchor_refreshes_runtime_tracking() {
        let mut runtime = RuntimeState::default();
        runtime.config.delay_event_to_smear = 24.0;
        runtime.initialize_cursor(
            RenderPoint {
                row: 10.0,
                col: 10.0,
            },
            CursorShape::block(),
            7,
            &valid_surface_location(),
        );
        runtime.record_observed_mode(/*current_is_cmdline*/ false);
        let baseline_epoch = runtime.retarget_epoch();
        let moved_location = TrackedCursor::fixture(1, 1, 1, 3)
            .with_window_origin(1, 1)
            .with_window_dimensions(120, 40);

        let request = PendingObservation::new(
            ExternalDemand::new(
                IngressSeq::new(6),
                ExternalDemandKind::ExternalCursor,
                Millis::new(6),
                BufferPerfClass::Full,
            ),
            ProbeRequestSet::default(),
        );
        let observation = ObservationSnapshot::new(
            request,
            observation_basis(6, ObservedCell::Unavailable, moved_location.clone()),
            ObservationMotion::default(),
        );

        super::plan_runtime_transition_for_runtime(
            &mut runtime,
            None,
            None,
            &observation,
            Millis::new(16),
        );

        assert_eq!(
            runtime.target_position(),
            RenderPoint {
                row: 10.0,
                col: 10.0
            }
        );
        assert_eq!(runtime.tracked_cursor(), Some(moved_location));
        assert_eq!(runtime.retarget_epoch(), baseline_epoch);
    }

    #[test]
    fn observed_cell_variants_preserve_runtime_target_selection_semantics() {
        #[derive(Clone, Copy)]
        struct Case {
            observed_cell: ObservedCell,
            latest_exact_cursor_cell: Option<ScreenCell>,
            expected_target_cell: ScreenCell,
        }

        let cases = [
            (
                "exact_observation_uses_the_new_exact_cell",
                Case {
                    observed_cell: ObservedCell::Exact(screen_cell(9, 10)),
                    latest_exact_cursor_cell: Some(screen_cell(10, 10)),
                    expected_target_cell: screen_cell(9, 10),
                },
            ),
            (
                "deferred_observation_uses_the_new_deferred_cell",
                Case {
                    observed_cell: ObservedCell::Deferred(screen_cell(8, 10)),
                    latest_exact_cursor_cell: Some(screen_cell(10, 10)),
                    expected_target_cell: screen_cell(8, 10),
                },
            ),
            (
                "unavailable_observation_uses_the_latest_exact_anchor",
                Case {
                    observed_cell: ObservedCell::Unavailable,
                    latest_exact_cursor_cell: Some(screen_cell(7, 10)),
                    expected_target_cell: screen_cell(7, 10),
                },
            ),
            (
                "unavailable_observation_without_anchor_keeps_the_runtime_target",
                Case {
                    observed_cell: ObservedCell::Unavailable,
                    latest_exact_cursor_cell: None,
                    expected_target_cell: screen_cell(10, 10),
                },
            ),
        ];

        for (
            case_name,
            Case {
                observed_cell,
                latest_exact_cursor_cell,
                expected_target_cell,
            },
        ) in cases
        {
            let mut runtime = RuntimeState::default();
            runtime.config.delay_event_to_smear = 24.0;
            runtime.initialize_cursor(
                RenderPoint {
                    row: 10.0,
                    col: 10.0,
                },
                CursorShape::block(),
                7,
                &valid_surface_location(),
            );
            runtime.record_observed_mode(/*current_is_cmdline*/ false);

            let request = PendingObservation::new(
                ExternalDemand::new(
                    IngressSeq::new(5),
                    ExternalDemandKind::ExternalCursor,
                    Millis::new(5),
                    BufferPerfClass::Full,
                ),
                ProbeRequestSet::default(),
            );
            let observation = ObservationSnapshot::new(
                request,
                observation_basis(5, observed_cell, valid_surface_location()),
                ObservationMotion::default(),
            );

            super::plan_runtime_transition_for_runtime(
                &mut runtime,
                latest_exact_cursor_cell,
                None,
                &observation,
                Millis::new(16),
            );

            assert_eq!(
                ScreenCell::from_rounded_point(runtime.target_position()),
                Some(expected_target_cell),
                "{case_name}"
            );
        }
    }

    fn observation_with_background_probe(
        seq: u64,
        allowed_cells: &[ScreenCell],
    ) -> ObservationSnapshot {
        let request = PendingObservation::new(
            ExternalDemand::new(
                IngressSeq::new(seq),
                ExternalDemandKind::ExternalCursor,
                Millis::new(seq),
                BufferPerfClass::Full,
            ),
            ProbeRequestSet::only(ProbeKind::Background),
        );
        let basis = observation_basis(
            seq,
            ObservedCell::Exact(screen_cell(10, 10)),
            valid_surface_location(),
        );
        let mut snapshot = ObservationSnapshot::new(request, basis, ObservationMotion::default());
        *snapshot.probes_mut().background_mut() =
            crate::core::state::BackgroundProbeState::from_plan(BackgroundProbePlan::from_cells(
                allowed_cells.to_vec(),
            ));
        let chunk = snapshot
            .probes()
            .background()
            .next_chunk()
            .expect("single-cell sparse probe should emit one chunk");
        let allowed_mask = BackgroundProbeChunkMask::from_allowed_mask(&vec![true; chunk.len()]);
        let viewport = snapshot.basis().viewport();
        assert!(
            snapshot
                .probes_mut()
                .background_mut()
                .apply_chunk(viewport, &chunk, &allowed_mask,),
            "single chunk sparse probe should complete immediately",
        );
        snapshot
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
        MotionRevision,
        SemanticRevision,
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
            Just(ReuseMutationAxis::MotionRevision),
            Just(ReuseMutationAxis::SemanticRevision),
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
            position: RenderPoint {
                row: 16.2,
                col: 18.4,
            },
            velocity: RenderPoint::ZERO,
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
        frame.projection_policy_revision = frame.projection_policy_revision.next();
        frame
    }

    fn frame_with_particle_overlay_drift(mut frame: RenderFrame) -> RenderFrame {
        frame.set_particles(Arc::new(vec![Particle {
            position: RenderPoint {
                row: 10.75,
                col: 11.25,
            },
            velocity: RenderPoint::ZERO,
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
            _color_at_cursor in any::<u32>(),
        ) {
            let next_presentation = match mutation_axis {
                DirtyMutationAxis::None => initial_presentation,
                DirtyMutationAxis::PaletteOnly | DirtyMutationAxis::Geometry => {
                    initial_presentation
                }
                DirtyMutationAxis::Presentation => {
                    alternate_target_cell_presentation(initial_presentation)
                }
            };

            let previous = Some(CursorTrailSemantic::new(initial_presentation));
            let next = Some(CursorTrailSemantic::new(next_presentation));
            let dirty = dirty_entities(previous.as_ref(), next.as_ref());
            let expected_dirty = previous != next;

            prop_assert_eq!(dirty.is_empty(), !expected_dirty);
            if expected_dirty {
                prop_assert_eq!(&dirty, &BTreeSet::from([SemanticEntityId::CursorTrail]));
            }

            match mutation_axis {
                DirtyMutationAxis::PaletteOnly => prop_assert!(dirty.is_empty()),
                DirtyMutationAxis::Presentation => prop_assert!(!dirty.is_empty()),
                DirtyMutationAxis::Geometry => prop_assert!(dirty.is_empty()),
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
                ReuseMutationAxis::ParticleOverlay => frame_with_particle_overlay_drift(frame.clone()),
                ReuseMutationAxis::Exact
                | ReuseMutationAxis::ObservationWitness
                | ReuseMutationAxis::MotionRevision
                | ReuseMutationAxis::SemanticRevision
                | ReuseMutationAxis::Presentation
                | ReuseMutationAxis::Policy => frame.clone(),
            };
            let query_observation = match mutation_axis {
                ReuseMutationAxis::ObservationWitness => {
                    observation_for_projection(observation_seq.saturating_add(1), requires_background_probe)
                }
                ReuseMutationAxis::Exact
                | ReuseMutationAxis::MotionRevision
                | ReuseMutationAxis::SemanticRevision
                | ReuseMutationAxis::ParticleOverlay
                | ReuseMutationAxis::Presentation
                | ReuseMutationAxis::Policy => cached_observation,
            };
            let render_revision = match mutation_axis {
                ReuseMutationAxis::MotionRevision => {
                    render_revision(MotionRevision::INITIAL.next(), SemanticRevision::INITIAL)
                }
                ReuseMutationAxis::SemanticRevision => {
                    render_revision(MotionRevision::INITIAL, SemanticRevision::INITIAL.next())
                }
                ReuseMutationAxis::Exact
                | ReuseMutationAxis::ObservationWitness
                | ReuseMutationAxis::ParticleOverlay
                | ReuseMutationAxis::Presentation
                | ReuseMutationAxis::Policy => RenderRevision::INITIAL,
            };
            let query_presentation = match mutation_axis {
                ReuseMutationAxis::Presentation => {
                    alternate_target_cell_presentation(target_cell_presentation)
                }
                ReuseMutationAxis::Exact
                | ReuseMutationAxis::ObservationWitness
                | ReuseMutationAxis::MotionRevision
                | ReuseMutationAxis::SemanticRevision
                | ReuseMutationAxis::ParticleOverlay
                | ReuseMutationAxis::Policy => target_cell_presentation,
            };
            let query_policy_revision = match mutation_axis {
                ReuseMutationAxis::Policy => {
                    projection_policy_revision(&frame_with_policy_drift(frame))
                }
                ReuseMutationAxis::Exact
                | ReuseMutationAxis::ObservationWitness
                | ReuseMutationAxis::MotionRevision
                | ReuseMutationAxis::SemanticRevision
                | ReuseMutationAxis::ParticleOverlay
                | ReuseMutationAxis::Presentation => cached_policy_revision,
            };

            let mut query_frame = query_frame;
            if mutation_axis == ReuseMutationAxis::Policy {
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
                ReuseMutationAxis::Policy => ProjectionPlannerState::default(),
                ReuseMutationAxis::Exact
                | ReuseMutationAxis::ObservationWitness
                | ReuseMutationAxis::MotionRevision
                | ReuseMutationAxis::SemanticRevision
                | ReuseMutationAxis::ParticleOverlay
                | ReuseMutationAxis::Presentation => cached_planner_state,
            };
            let expected_reuse = matches!(
                mutation_axis,
                ReuseMutationAxis::Exact
                    | ReuseMutationAxis::ObservationWitness
                    | ReuseMutationAxis::MotionRevision
                    | ReuseMutationAxis::SemanticRevision
                    | ReuseMutationAxis::ParticleOverlay
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
            let current_scene =
                SceneState::default().with_retained_projection(first.clone());
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
            realization_plan_for_render_decision(&config, &decision, None, None,),
            RealizationPlan::Clear(RealizationClear::new(config.max_kept_windows))
        );
    }

    #[test]
    fn build_planned_render_uses_captured_config_for_clear_budget() {
        let decision = RenderDecision {
            render_action: RenderAction::ClearAll,
            render_cleanup_action: crate::core::runtime_reducer::RenderCleanupAction::NoAction,
            render_allocation_policy:
                crate::core::runtime_reducer::RenderAllocationPolicy::ReuseOnly,
            render_side_effects: crate::core::runtime_reducer::RenderSideEffects::default(),
        };
        let config = RuntimeConfig {
            max_kept_windows: 13,
            ..RuntimeConfig::default()
        };

        let planned_render = build_planned_render(
            RenderPlanningContext::new(SceneState::default(), None, None, Arc::new(config.clone())),
            ProposalId::new(41),
            &decision,
            AnimationSchedule::Idle,
        )
        .expect("clear planning should preserve proposal shape invariants");

        assert_eq!(
            planned_render.proposal().realization(),
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
