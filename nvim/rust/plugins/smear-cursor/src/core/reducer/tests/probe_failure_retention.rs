use super::*;

#[test]
fn failed_probe_report_is_retained_without_collapsing_to_missing() {
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
            ingress_observation_surface: None,
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

    let completed = reduce(&based.next, cursor_color_probe_failed(&request));

    let observation = completed
        .next
        .observation()
        .expect("stored observation snapshot");
    pretty_assert_eq!(observation.cursor_color(), None);
    match observation.probes().cursor_color() {
        ProbeSlot::Requested(ProbeState::Failed { failure, .. }) => {
            pretty_assert_eq!(*failure, ProbeFailure::ShellReadFailed)
        }
        other => panic!("expected failed cursor color probe, got {other:?}"),
    }
}
