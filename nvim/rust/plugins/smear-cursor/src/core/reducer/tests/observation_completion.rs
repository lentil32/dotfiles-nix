use super::*;

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
        previous_request.clone(),
        observation_basis_with_text_context(
            &previous_request,
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
        .with_last_cursor(Some(cursor(9, 9)))
        .with_runtime(runtime)
        .into_ready_with_observation(previous_observation);
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
            &request,
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
        previous_request.clone(),
        observation_basis_with_text_context(
            &previous_request,
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
        .with_last_cursor(Some(cursor(9, 9)))
        .with_runtime(runtime)
        .into_ready_with_observation(previous_observation);
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
            &request,
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
        previous_request.clone(),
        observation_basis_with_text_context(
            &previous_request,
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
        .with_last_cursor(Some(cursor(9, 9)))
        .with_runtime(runtime)
        .into_ready_with_observation(previous_observation);
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
            request.observation_id(),
            Millis::new(101),
            "n".to_string(),
            Some(cursor(10, 3)),
            CursorLocation::new(11, 22, 4, 10),
            ViewportSnapshot::new(CursorRow(40), CursorCol(120)),
        )
        .with_cursor_text_context(Some(text_context(
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
