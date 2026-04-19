use super::*;

fn queued_cursor_demand(
    state: &CoreState,
    observed_at: u64,
    requested_target: CursorPosition,
) -> CoreState {
    reduce(
        state,
        external_demand_event(
            ExternalDemandKind::ExternalCursor,
            observed_at,
            Some(requested_target),
        ),
    )
    .next
}

fn assert_animation_tick_is_noop(state: CoreState, observed_at: u64) -> CoreState {
    let (armed_state, token) = timer_armed_state(state);
    let expected_state = armed_state
        .clone()
        .with_timers(armed_state.timers().clear_matching(token));

    let transition = reduce(&armed_state, animation_tick_event(token, observed_at));

    pretty_assert_eq!(transition.next, expected_state);
    assert!(transition.effects.is_empty());
    transition.next
}

fn recovery_tick_event(token: TimerToken, observed_at: u64) -> Event {
    Event::TimerFiredWithToken(TimerFiredWithTokenEvent {
        token,
        observed_at: Millis::new(observed_at),
    })
}

#[test]
fn animation_tick_during_observing_request_is_noop_even_with_queued_ingress() {
    let observing_request = observing_state_from_demand(
        &ready_state_with_observation(cursor(7, 8)),
        ExternalDemandKind::ExternalCursor,
        25,
        Some(cursor(7, 9)),
    );
    let request = active_request(&observing_request);
    let interleaved = assert_animation_tick_is_noop(
        queued_cursor_demand(&observing_request, 26, cursor(9, 10)),
        27,
    );

    let completed = collect_observation_base(
        &interleaved,
        &request,
        observation_basis(Some(cursor(7, 9)), 28),
        observation_motion(),
    );

    pretty_assert_eq!(completed.next.lifecycle(), Lifecycle::Planning);
    assert!(matches!(
        completed.effects.as_slice(),
        [Effect::RequestRenderPlan(_)]
    ));
}

#[test]
fn animation_tick_during_observing_active_is_noop_even_with_queued_ingress() {
    let scenario = ObservationScenario::new(cursor_color_probe_ready_state());
    let request = scenario.request;
    let interleaved = assert_animation_tick_is_noop(
        queued_cursor_demand(&scenario.based.next, 27, cursor(9, 10)),
        28,
    );

    let completed = reduce(
        &interleaved,
        cursor_color_probe_report(&request, ProbeReuse::Exact, Some(0x00AB_CDEF)),
    );

    pretty_assert_eq!(completed.next.lifecycle(), Lifecycle::Planning);
    assert!(matches!(
        completed.effects.as_slice(),
        [Effect::RequestRenderPlan(_)]
    ));
}

#[test]
fn animation_tick_during_planning_is_noop_even_with_queued_ingress() {
    let (ready, animation_token) = timer_armed_state(ready_state_with_observation(cursor(9, 9)));
    let planning = reduce(&ready, animation_tick_event(animation_token, 50));
    let [Effect::RequestRenderPlan(payload)] = planning.effects.as_slice() else {
        panic!("expected render plan request after initial animation tick");
    };
    let payload = payload.clone();
    let interleaved =
        assert_animation_tick_is_noop(queued_cursor_demand(&planning.next, 51, cursor(9, 10)), 52);

    let completed = reduce(
        &interleaved,
        Event::RenderPlanComputed(RenderPlanComputedEvent {
            planned_render: Box::new(
                crate::core::reducer::build_planned_render(
                    payload.planning.clone(),
                    payload.proposal_id,
                    &payload.render_decision,
                    payload.animation_schedule,
                )
                .expect("planning state should still accept its in-flight proposal"),
            ),
            observed_at: payload.requested_at,
        }),
    );

    pretty_assert_eq!(completed.next.lifecycle(), Lifecycle::Applying);
    pretty_assert_eq!(
        completed
            .next
            .pending_proposal()
            .map(InFlightProposal::proposal_id),
        Some(payload.proposal_id)
    );
    assert!(matches!(
        completed.effects.as_slice(),
        [Effect::ApplyProposal(_)]
    ));
}

#[test]
fn animation_tick_during_applying_is_noop_even_with_queued_ingress() {
    let (applying, proposal_id) = applying_state_with_realization_plan(
        ready_state_with_observation(cursor(4, 9)),
        noop_realization_plan(),
        false,
        None,
    );
    let interleaved =
        assert_animation_tick_is_noop(queued_cursor_demand(&applying, 90, cursor(5, 10)), 91);

    let completed = reduce(
        &interleaved,
        Event::ApplyReported(ApplyReport::AppliedFully {
            proposal_id,
            observed_at: Millis::new(92),
            visual_change: false,
        }),
    );
    let replacement_request = active_request(&completed.next);

    pretty_assert_eq!(completed.next.lifecycle(), Lifecycle::Observing);
    pretty_assert_eq!(
        replacement_request.demand().requested_target(),
        Some(cursor(5, 10))
    );
    assert!(completed.effects.iter().any(|effect| {
        matches!(
            effect,
            Effect::RequestObservationBase(payload) if payload.request == replacement_request
        )
    }));
}

#[test]
fn animation_tick_during_recovering_is_noop_even_with_queued_ingress() {
    let recovering = queued_cursor_demand(
        &recovering_state_with_observation(cursor(2, 2)),
        120,
        cursor(3, 3),
    );
    let (timers, recovery_token) = recovering.timers().arm(TimerId::Recovery);
    let interleaved = assert_animation_tick_is_noop(recovering.with_timers(timers), 121);

    let resumed = reduce(&interleaved, recovery_tick_event(recovery_token, 122));
    let replacement_request = active_request(&resumed.next);

    pretty_assert_eq!(resumed.next.lifecycle(), Lifecycle::Observing);
    pretty_assert_eq!(
        replacement_request.demand().requested_target(),
        Some(cursor(3, 3))
    );
}
