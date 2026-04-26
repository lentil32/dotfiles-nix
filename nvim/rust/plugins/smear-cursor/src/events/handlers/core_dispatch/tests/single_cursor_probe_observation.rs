use super::*;
use pretty_assertions::assert_eq;

fn setup_cursor_probe_ingress() -> (CoreDispatchTestContext, PendingObservation) {
    let scope = CoreDispatchTestContext::new();
    scope.set_core_state(ready_state_with_cursor_color_probe());
    let request = scope.dispatch_external_cursor_ingress_to_queue(25);
    (scope, request)
}

#[test]
fn ingress_dispatch_queues_one_observation_base_batch() {
    let (_scope, _request) = setup_cursor_probe_ingress();
    let after_ingress = current_core_state();

    assert_eq!(after_ingress.lifecycle(), Lifecycle::Observing);
    assert!(after_ingress.observation().is_none());
    assert!(after_ingress.pending_proposal().is_none());
    assert_eq!(
        queued_work_count(),
        1,
        "ingress should queue one effect batch"
    );
    assert!(matches!(
        queued_front_work_item(),
        Some(ScheduledWorkUnit::OrderedEffectBatch(ref effects))
            if contains_observation_base_request(effects)
    ));
}
