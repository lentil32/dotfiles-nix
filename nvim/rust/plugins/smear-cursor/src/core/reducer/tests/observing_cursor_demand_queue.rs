use super::*;

#[test]
fn first_cursor_demand_enters_observing_and_requests_observation_base() {
    let ready = ready_state();

    let first = reduce(
        &ready,
        external_demand_event(ExternalDemandKind::ExternalCursor, 20),
    );

    pretty_assert_eq!(first.next.lifecycle(), Lifecycle::Observing);
    pretty_assert_eq!(
        first.effects,
        with_cleanup_invalidation(
            &first.next,
            20,
            vec![Effect::RequestObservationBase(
                RequestObservationBaseEffect {
                    request: observation_request(1, ExternalDemandKind::ExternalCursor, 20),
                    context: observation_runtime_context(
                        &ready,
                        ExternalDemandKind::ExternalCursor,
                    ),
                }
            )]
        )
    );
}

#[test]
fn second_cursor_demand_records_ingress_coalesced_without_restarting_observation() {
    let ready = ready_state();
    let observing = observing_state_from_demand(&ready, ExternalDemandKind::ExternalCursor, 20);

    let second = reduce(
        &observing,
        external_demand_event(ExternalDemandKind::ExternalCursor, 21),
    );

    pretty_assert_eq!(second.next.lifecycle(), Lifecycle::Observing);
    pretty_assert_eq!(
        second.effects,
        vec![Effect::RecordEventLoopMetric(
            EventLoopMetricEffect::IngressCoalesced,
        )]
    );
}

#[test]
fn newest_queued_cursor_replaces_the_older_pending_cursor_demand() {
    let ready = ready_state();
    let observing = observing_state_from_demand(&ready, ExternalDemandKind::ExternalCursor, 20);
    let coalesced = reduce(
        &observing,
        external_demand_event(ExternalDemandKind::ExternalCursor, 21),
    );

    let third = reduce(
        &coalesced.next,
        external_demand_event(ExternalDemandKind::ExternalCursor, 22),
    );

    let queued_cursor = third
        .next
        .demand_queue()
        .latest_cursor()
        .expect("queued cursor demand");
    pretty_assert_eq!(
        queued_cursor,
        &crate::core::state::QueuedDemand::ready(ExternalDemand::new(
            IngressSeq::new(3),
            ExternalDemandKind::ExternalCursor,
            Millis::new(22),
            BufferPerfClass::Full,
        ))
    );
}
