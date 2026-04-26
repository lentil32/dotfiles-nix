use super::*;
use pretty_assertions::assert_eq;

fn setup_multi_probe_ingress() -> (
    CoreDispatchTestContext,
    PendingObservation,
    ObservationBasis,
) {
    let scope = CoreDispatchTestContext::new();
    scope.set_core_state(ready_state_with_cursor_and_background_probes());
    let request = scope.dispatch_external_cursor_ingress_to_queue(25);
    let basis = observation_basis(Some(cursor(7, 8)), 26);
    (scope, request, basis)
}

fn setup_after_observation_base_edge() -> (
    CoreDispatchTestContext,
    PendingObservation,
    ObservationBasis,
    RecordingExecutor,
) {
    let (scope, request, basis) = setup_multi_probe_ingress();
    let mut executor = RecordingExecutor::default();
    executor
        .planned_follow_ups
        .push_back(vec![observation_base_collected(&request, basis.clone())]);
    let _ = drain_next_edge(&mut executor);
    install_background_probe_plan(&basis);
    (scope, request, basis, executor)
}

#[test]
fn observation_base_edge_queues_only_the_cursor_color_probe() {
    let (_scope, _request, _basis, executor) = setup_after_observation_base_edge();

    assert!(matches!(
        executor.executed_effects.as_slice(),
        [Effect::RequestObservationBase(_), Effect::ScheduleTimer(_)]
    ));
    assert_eq!(
        queued_work_count(),
        1,
        "only one probe batch should be queued"
    );
    assert!(matches!(
        queued_front_work_item(),
        Some(ScheduledWorkUnit::OrderedEffectBatch(ref effects))
            if only_cursor_color_probe_request(effects)
    ));
}
