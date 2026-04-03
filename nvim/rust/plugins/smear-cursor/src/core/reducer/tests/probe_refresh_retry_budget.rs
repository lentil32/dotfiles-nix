use super::*;

fn exhausted_refresh_transition() -> (ObservationRequest, Transition) {
    let scenario = ObservationScenario::new(cursor_color_probe_ready_state());
    let queued_newer = reduce(
        &scenario.based.next,
        external_demand_event(ExternalDemandKind::ExternalCursor, 27, Some(cursor(9, 10))),
    )
    .next;

    let retry_once = reduce(
        &queued_newer,
        cursor_color_probe_report(&scenario.request, ProbeReuse::RefreshRequired, None),
    );
    let retry_once_based = collect_observation_base(
        &retry_once.next,
        &scenario.request,
        observation_basis(&scenario.request, Some(cursor(7, 9)), 28),
        observation_motion(),
    );
    let retry_twice = reduce(
        &retry_once_based.next,
        cursor_color_probe_report(&scenario.request, ProbeReuse::RefreshRequired, None),
    );
    let retry_twice_based = collect_observation_base(
        &retry_twice.next,
        &scenario.request,
        observation_basis(&scenario.request, Some(cursor(7, 10)), 29),
        observation_motion(),
    );
    let exhausted = reduce(
        &retry_twice_based.next,
        cursor_color_probe_report(&scenario.request, ProbeReuse::RefreshRequired, None),
    );
    (scenario.request, exhausted)
}

#[test]
fn refresh_budget_exhaustion_promotes_the_newer_ingress_request() {
    let (request, exhausted) = exhausted_refresh_transition();

    let replacement_request = exhausted
        .next
        .active_observation_request()
        .cloned()
        .expect("newer ingress should take over after retry budget exhaustion");
    assert_ne!(replacement_request, request);
    pretty_assert_eq!(
        replacement_request.demand().requested_target(),
        Some(cursor(9, 10))
    );
    pretty_assert_eq!(exhausted.next.lifecycle(), Lifecycle::Observing);
}

#[test]
fn refresh_budget_exhaustion_requests_a_new_base_for_the_replacement_ingress() {
    let (_request, exhausted) = exhausted_refresh_transition();
    let replacement_request = exhausted
        .next
        .active_observation_request()
        .cloned()
        .expect("replacement request after retry exhaustion");

    pretty_assert_eq!(
        exhausted.effects,
        vec![
            Effect::RecordEventLoopMetric(EventLoopMetricEffect::ProbeRefreshBudgetExhausted(
                ProbeKind::CursorColor
            ),),
            Effect::RequestObservationBase(RequestObservationBaseEffect {
                request: replacement_request,
                context: observation_runtime_context(
                    &exhausted.next,
                    ExternalDemandKind::ExternalCursor,
                ),
            }),
        ]
    );
}

#[test]
fn refresh_budget_exhaustion_clears_the_stale_observation_snapshot() {
    let (_request, exhausted) = exhausted_refresh_transition();

    assert!(exhausted.next.observation().is_none());
}
