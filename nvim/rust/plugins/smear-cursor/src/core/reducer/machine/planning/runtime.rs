use super::super::Transition;
use super::super::support::request_render_plan_effect;
use super::render_planning_observation;
use crate::core::effect::RenderPlanningContext;
use crate::core::runtime_reducer::CursorEventContext;
use crate::core::runtime_reducer::EventSource;
use crate::core::runtime_reducer::MotionTarget;
use crate::core::runtime_reducer::RenderAction;
use crate::core::runtime_reducer::select_event_source;
use crate::core::state::BackgroundProbePlan;
use crate::core::state::CoreState;
use crate::core::state::ObservationSnapshot;
use crate::core::state::PreparedObservationPlan;
use crate::core::types::Millis;
use crate::position::RenderPoint;
use crate::position::ScreenCell;
use crate::state::TrackedCursor;
use crate::types::DEFAULT_RNG_STATE;

struct PlannedRuntimeTransition {
    transition: crate::core::runtime_reducer::CursorTransition,
}

fn deterministic_event_seed(observation: &ObservationSnapshot) -> u32 {
    let observed_at = observation.basis().observed_at().value() as u32;
    let observation_id = observation.observation_id().value() as u32;
    let mixed = observation_id ^ observed_at.rotate_left(13) ^ 0x9E37_79B9;
    if mixed == 0 { DEFAULT_RNG_STATE } else { mixed }
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
    let semantic_event =
        crate::core::state::classify_semantic_event(previous_observation, observation);
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
    viewport: crate::position::ViewportBounds,
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
        animation_schedule,
    } = cursor_transition;

    let Some(planning_observation) = state.observation().map(render_planning_observation) else {
        // Surprising: `plan_ready_state` lost its ready observation before planning.
        return Transition::stay(&state);
    };

    let (state, proposal_id) = state.allocate_proposal_id();
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

#[cfg(test)]
mod tests {
    use super::plan_runtime_transition_for_runtime;
    use super::prepare_observation_plan_with_runtime;
    use crate::core::runtime_reducer::RenderAction;
    use crate::core::state::BufferPerfClass;
    use crate::core::state::ExternalDemand;
    use crate::core::state::ExternalDemandKind;
    use crate::core::state::ObservationMotion;
    use crate::core::state::ObservationSnapshot;
    use crate::core::state::PendingObservation;
    use crate::core::state::ProbeRequestSet;
    use crate::core::types::IngressSeq;
    use crate::core::types::Millis;
    use crate::position::ObservedCell;
    use crate::position::RenderPoint;
    use crate::state::CursorShape;
    use crate::state::RuntimeState;
    use pretty_assertions::assert_eq;

    use super::super::test_support::observation;
    use super::super::test_support::observation_basis;
    use super::super::test_support::screen_cell;
    use super::super::test_support::valid_surface_location;

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

        let (returned_runtime, prepared_plan) = prepare_observation_plan_with_runtime(
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

        let (_returned_runtime, prepared_plan) = prepare_observation_plan_with_runtime(
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

        let planned = plan_runtime_transition_for_runtime(
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
        assert!(planned.transition.should_schedule_next_animation());
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

        let planned = plan_runtime_transition_for_runtime(
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
        assert!(planned.transition.should_schedule_next_animation());
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

        let planned = plan_runtime_transition_for_runtime(
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
        assert!(planned.transition.should_schedule_next_animation());
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

        let planned = plan_runtime_transition_for_runtime(
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
        assert!(planned.transition.should_schedule_next_animation());
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
        let moved_location = crate::state::TrackedCursor::fixture(1, 1, 1, 3)
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

        plan_runtime_transition_for_runtime(
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
            latest_exact_cursor_cell: Option<crate::position::ScreenCell>,
            expected_target_cell: crate::position::ScreenCell,
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

            plan_runtime_transition_for_runtime(
                &mut runtime,
                latest_exact_cursor_cell,
                None,
                &observation,
                Millis::new(16),
            );

            assert_eq!(
                crate::position::ScreenCell::from_rounded_point(runtime.target_position()),
                Some(expected_target_cell),
                "{case_name}"
            );
        }
    }
}
