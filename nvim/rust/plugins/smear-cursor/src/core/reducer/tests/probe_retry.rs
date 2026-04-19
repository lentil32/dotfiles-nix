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
            buffer_perf_class: BufferPerfClass::Full,
            ingress_cursor_presentation: None,
            ingress_observation_surface: None,
        }),
    )
    .next;
    let request = observing
        .pending_observation()
        .cloned()
        .expect("active observation");
    let based = reduce(
        &observing,
        Event::ObservationBaseCollected(ObservationBaseCollectedEvent {
            observation_id: request.observation_id(),
            basis: observation_basis(Some(cursor(7, 8)), 26),
            cursor_color_probe_generations: Some(cursor_color_probe_generations()),
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

#[test]
fn refresh_required_probe_report_recomputes_retry_probe_policy_from_active_demand() {
    let request = PendingObservation::new(
        ExternalDemand::new(
            IngressSeq::new(9),
            ExternalDemandKind::ExternalCursor,
            Millis::new(25),
            BufferPerfClass::Full,
        ),
        ProbeRequestSet::none()
            .with_requested(ProbeKind::CursorColor)
            .with_requested(ProbeKind::Background),
    );
    let observing = dual_probe_ready_state()
        .enter_observing_request(request.clone())
        .expect("ready state should stage a collecting observation request")
        .with_active_observation(
            ObservationSnapshot::new(
                request.clone(),
                observation_basis(Some(cursor(7, 8)), 26),
                observation_motion(),
            )
            .with_cursor_color_probe_generations(Some(cursor_color_probe_generations())),
        )
        .expect("observation should stay active");

    let retried = reduce(
        &observing,
        cursor_color_probe_report(&request, ProbeReuse::RefreshRequired, None),
    );

    pretty_assert_eq!(retried.next.lifecycle(), Lifecycle::Observing);
    pretty_assert_eq!(retried.next.pending_observation(), Some(&request));
    pretty_assert_eq!(
        retried.effects,
        vec![
            Effect::RecordEventLoopMetric(EventLoopMetricEffect::ProbeRefreshRetried(
                ProbeKind::CursorColor,
            )),
            Effect::RequestObservationBase(RequestObservationBaseEffect {
                request,
                context: observation_runtime_context(
                    &retried.next,
                    ExternalDemandKind::ExternalCursor
                ),
            })
        ]
    );
    assert!(retried.next.observation().is_none());
}

#[test]
fn background_refresh_retry_retains_the_completed_observation_for_retry_context() {
    let scenario = ObservationScenario::with_background_plan(
        dual_probe_ready_state(),
        vec![ScreenCell::new(7, 8).expect("background probe cell")],
    );
    let request = scenario.request.clone();
    let after_cursor = reduce(
        &scenario.based.next,
        cursor_color_probe_report(&request, ProbeReuse::Exact, Some(0x00AB_CDEF)),
    );
    let expected_retained = after_cursor
        .next
        .phase_observation()
        .cloned()
        .expect("cursor-color completion should keep the observation active");

    let retried = reduce(
        &after_cursor.next,
        background_probe_report(
            &request,
            scenario.basis.viewport(),
            &[(7, 8)],
            ProbeReuse::RefreshRequired,
        ),
    );

    pretty_assert_eq!(retried.next.lifecycle(), Lifecycle::Observing);
    pretty_assert_eq!(retried.next.pending_observation(), Some(&request));
    pretty_assert_eq!(
        retried.next.retained_observation(),
        Some(&expected_retained)
    );
    pretty_assert_eq!(retried.next.phase_observation(), Some(&expected_retained));
    pretty_assert_eq!(
        retried.effects,
        vec![
            Effect::RecordEventLoopMetric(EventLoopMetricEffect::ProbeRefreshRetried(
                ProbeKind::Background,
            )),
            Effect::RequestObservationBase(RequestObservationBaseEffect {
                request,
                context: observation_runtime_context(
                    &after_cursor.next,
                    ExternalDemandKind::ExternalCursor
                ),
            })
        ]
    );
    assert!(retried.next.observation().is_none());
}
