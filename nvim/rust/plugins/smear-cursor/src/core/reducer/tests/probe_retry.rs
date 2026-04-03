use super::*;

#[test]
fn refresh_required_probe_report_retries_base_observation() {
    let mut runtime = ready_state().runtime().clone();
    runtime.config.cursor_color = Some("none".to_string());
    let ready = ready_state().with_runtime(runtime);
    let observing = reduce(
        &ready,
        Event::ExternalDemandQueued(ExternalDemandQueuedEvent {
            kind: ExternalDemandKind::ExternalCursor,
            observed_at: Millis::new(25),
            requested_target: None,
            buffer_perf_class: BufferPerfClass::Full,
            ingress_cursor_presentation: None,
        }),
    )
    .next;
    let request = observing
        .active_observation_request()
        .cloned()
        .expect("active observation");
    let based = reduce(
        &observing,
        Event::ObservationBaseCollected(ObservationBaseCollectedEvent {
            request: request.clone(),
            basis: observation_basis(&request, Some(cursor(7, 8)), 26),
            motion: observation_motion(),
        }),
    );

    let retried = reduce(
        &based.next,
        cursor_color_probe_report(&request, ProbeReuse::RefreshRequired, None),
    );

    pretty_assert_eq!(retried.next.lifecycle(), Lifecycle::Observing);
    pretty_assert_eq!(
        retried.effects,
        vec![
            Effect::RecordEventLoopMetric(EventLoopMetricEffect::ProbeRefreshRetried(
                ProbeKind::CursorColor,
            )),
            Effect::RequestObservationBase(RequestObservationBaseEffect {
                request,
                context: observation_runtime_context(
                    &based.next,
                    ExternalDemandKind::ExternalCursor
                ),
            })
        ]
    );
    assert!(retried.next.observation().is_none());
    pretty_assert_eq!(
        retried
            .next
            .probe_refresh_state()
            .expect("probe refresh state while observing")
            .retry_count(ProbeKind::CursorColor),
        1
    );
}
