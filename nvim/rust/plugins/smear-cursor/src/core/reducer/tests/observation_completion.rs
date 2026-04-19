use super::*;
use crate::types::Particle;
use crate::types::StepOutput;

#[test]
fn idle_apply_completion_requests_boundary_refresh_for_conceal_deferred_cursor_position() {
    let (applying, proposal_id) = applying_state_with_realization_plan(
        conceal_deferred_cursor_ready_state(|_runtime| {}),
        noop_realization_plan(),
        false,
        None,
    );

    let transition = reduce(
        &applying,
        Event::ApplyReported(ApplyReport::AppliedFully {
            proposal_id,
            observed_at: Millis::new(101),
            visual_change: false,
        }),
    );

    let request = active_request(&transition.next);
    pretty_assert_eq!(transition.next.lifecycle(), Lifecycle::Observing);
    pretty_assert_eq!(request.demand().kind(), ExternalDemandKind::BoundaryRefresh);
    pretty_assert_eq!(
        request.demand().buffer_perf_class(),
        BufferPerfClass::FastMotion
    );
    assert!(transition.effects.iter().any(|effect| {
        matches!(
            effect,
            Effect::RequestObservationBase(payload)
                if payload.request == request
                    && payload.context.buffer_perf_class() == BufferPerfClass::FastMotion
                    && payload.context.probe_policy().quality() == ProbeQuality::Exact
        )
    }));
}

#[test]
fn observation_completion_with_text_mutation_requests_clear_all_render_plan() {
    let previous_request = observation_request(9, ExternalDemandKind::ExternalCursor, 90);
    let previous_observation = ObservationSnapshot::new(
        previous_request,
        observation_basis_with_text_context(
            Some(cursor(9, 9)),
            91,
            9,
            10,
            &["before", "alpha", "after"],
            None,
        ),
        observation_motion(),
    );
    let mut runtime = ready_state().runtime().clone();
    runtime.initialize_cursor(
        Point { row: 9.0, col: 9.0 },
        CursorShape::new(false, false),
        7,
        &CursorLocation::new(11, 22, 3, 9),
    );
    let ready = ready_state()
        .with_latest_exact_cursor_position(Some(cursor(9, 9)))
        .with_runtime(runtime)
        .with_ready_observation(previous_observation)
        .expect("primed state should accept a retained ready observation");
    let observing = reduce(
        &ready,
        external_demand_event(ExternalDemandKind::ExternalCursor, 100, None),
    )
    .next;
    let request = active_request(&observing);

    let transition = collect_observation_base(
        &observing,
        &request,
        observation_basis_with_text_context(
            Some(cursor(9, 10)),
            101,
            9,
            11,
            &["before", "alphab", "after"],
            Some(&["before", "alphab", "after"]),
        ),
        observation_motion(),
    );

    let [Effect::RequestRenderPlan(payload)] = transition.effects.as_slice() else {
        panic!("expected render plan request after text mutation observation");
    };
    assert!(matches!(
        payload.render_decision.render_action,
        RenderAction::ClearAll
    ));
}

#[test]
fn observation_completion_with_motion_only_requests_draw_render_plan() {
    let previous_request = observation_request(9, ExternalDemandKind::ExternalCursor, 90);
    let previous_observation = ObservationSnapshot::new(
        previous_request,
        observation_basis_with_text_context(
            Some(cursor(9, 9)),
            91,
            9,
            10,
            &["before", "alpha", "after"],
            None,
        ),
        observation_motion(),
    );
    let mut runtime = ready_state().runtime().clone();
    runtime.initialize_cursor(
        Point { row: 9.0, col: 9.0 },
        CursorShape::new(false, false),
        7,
        &CursorLocation::new(11, 22, 3, 9),
    );
    let ready = ready_state()
        .with_latest_exact_cursor_position(Some(cursor(9, 9)))
        .with_runtime(runtime)
        .with_ready_observation(previous_observation)
        .expect("primed state should accept a retained ready observation");
    let observing = reduce(
        &ready,
        external_demand_event(ExternalDemandKind::ExternalCursor, 100, None),
    )
    .next;
    let request = active_request(&observing);

    let transition = collect_observation_base(
        &observing,
        &request,
        observation_basis_with_text_context(
            Some(cursor(10, 9)),
            101,
            10,
            10,
            &["alpha", "after", "tail"],
            Some(&["before", "alpha", "after"]),
        ),
        observation_motion(),
    );

    let [Effect::RequestRenderPlan(payload)] = transition.effects.as_slice() else {
        panic!("expected render plan request after motion-only observation");
    };
    assert!(matches!(
        payload.render_decision.render_action,
        RenderAction::Draw(_)
    ));
}

#[test]
fn observation_completion_with_scroll_and_text_mutation_still_requests_clear_all_render_plan() {
    let previous_request = observation_request(9, ExternalDemandKind::ExternalCursor, 90);
    let previous_observation = ObservationSnapshot::new(
        previous_request,
        observation_basis_with_text_context(
            Some(cursor(9, 9)),
            91,
            9,
            10,
            &["before", "alpha", "after"],
            None,
        ),
        observation_motion(),
    );
    let mut runtime = ready_state().runtime().clone();
    runtime.initialize_cursor(
        Point { row: 9.0, col: 9.0 },
        CursorShape::new(false, false),
        7,
        &CursorLocation::new(11, 22, 1, 9),
    );
    let ready = ready_state()
        .with_latest_exact_cursor_position(Some(cursor(9, 9)))
        .with_runtime(runtime)
        .with_ready_observation(previous_observation)
        .expect("primed state should accept a retained ready observation");
    let observing = reduce(
        &ready,
        external_demand_event(ExternalDemandKind::ExternalCursor, 100, None),
    )
    .next;
    let request = active_request(&observing);

    let transition = collect_observation_base(
        &observing,
        &request,
        ObservationBasis::new(
            Millis::new(101),
            "n".to_string(),
            Some(cursor(10, 3)),
            CursorLocation::new(11, 22, 4, 10),
            ViewportSnapshot::new(CursorRow(40), CursorCol(120)),
        )
        .with_cursor_text_context_state(CursorTextContextState::Sampled(text_context(
            11,
            10,
            &["alpha pasted", "block", "tail"],
            Some(&["before", "alpha pasted", "block"]),
        ))),
        observation_motion(),
    );

    let [Effect::RequestRenderPlan(payload)] = transition.effects.as_slice() else {
        panic!("expected render plan request after scroll + text mutation observation");
    };
    assert!(matches!(
        payload.render_decision.render_action,
        RenderAction::ClearAll
    ));
}

#[test]
fn observation_completion_moves_runtime_particles_into_render_planning() {
    let mut runtime = ready_state().runtime().clone();
    runtime.config.particles_enabled = false;
    runtime.initialize_cursor(
        Point { row: 9.0, col: 9.0 },
        CursorShape::new(false, false),
        7,
        &CursorLocation::new(11, 22, 3, 9),
    );
    runtime.apply_step_output(StepOutput {
        current_corners: runtime.current_corners(),
        velocity_corners: runtime.velocity_corners(),
        spring_velocity_corners: runtime.spring_velocity_corners(),
        trail_elapsed_ms: runtime.trail_elapsed_ms(),
        particles: vec![Particle {
            position: Point {
                row: 9.0,
                col: 10.0,
            },
            velocity: Point {
                row: 0.5,
                col: 0.25,
            },
            lifetime: 120.0,
        }],
        previous_center: runtime.previous_center(),
        index_head: 0,
        index_tail: 0,
        rng_state: runtime.rng_state(),
    });
    let particles_ptr = runtime.particles().as_ptr();

    let ready = ready_state().with_runtime(runtime);
    let observing = crate::core::reducer::reduce_owned(
        ready,
        external_demand_event(ExternalDemandKind::ExternalCursor, 100, None),
    )
    .next;
    let request = active_request(&observing);

    let transition = crate::core::reducer::reduce_owned(
        observing,
        Event::ObservationBaseCollected(ObservationBaseCollectedEvent {
            observation_id: request.observation_id(),
            basis: ObservationBasis::new(
                Millis::new(101),
                "n".to_string(),
                Some(cursor(40, 20)),
                CursorLocation::new(99, 22, 3, 40),
                ViewportSnapshot::new(CursorRow(40), CursorCol(120)),
            ),
            cursor_color_probe_generations: None,
            motion: observation_motion(),
        }),
    );

    let [Effect::RequestRenderPlan(_payload)] = transition.effects.as_slice() else {
        panic!("expected render plan request after observation completion");
    };
    pretty_assert_eq!(transition.next.runtime().particles().len(), 1);
    assert_eq!(
        transition.next.runtime().particles().as_ptr(),
        particles_ptr
    );
}

#[test]
fn conceal_deferred_observation_completion_preserves_latest_exact_cursor_anchor() {
    let ready = ready_state_with_observation(cursor(9, 9));
    let observing = reduce(
        &ready,
        external_demand_event(ExternalDemandKind::ExternalCursor, 100, None),
    )
    .next;
    let request = active_request(&observing);

    let transition = collect_observation_base(
        &observing,
        &request,
        observation_basis(Some(cursor(12, 13)), 101),
        observation_motion().with_cursor_position_sync(CursorPositionSync::ConcealDeferred),
    );

    pretty_assert_eq!(
        transition.next.latest_exact_cursor_position(),
        Some(cursor(9, 9))
    );
    let Some(observation) = transition.next.observation() else {
        panic!("expected active observation after base collection");
    };
    pretty_assert_eq!(observation.basis().cursor_position(), Some(cursor(12, 13)));
}
