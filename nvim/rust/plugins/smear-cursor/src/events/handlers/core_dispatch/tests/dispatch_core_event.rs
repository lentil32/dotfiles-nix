use super::*;
use pretty_assertions::assert_eq;

#[test]
fn stages_observation_request_work_for_external_cursor_ingress() {
    let scope = CoreDispatchTestContext::new();
    scope.set_core_state(ready_state());
    let mut staged_batches = Vec::new();

    dispatch_core_event(external_cursor_demand(21), &mut |effects| {
        staged_batches.push(effects)
    })
    .expect("external ingress dispatch should succeed");

    assert_eq!(staged_batches.len(), 1);
    assert!(
        staged_batches[0]
            .iter()
            .any(|effect| matches!(effect, Effect::RequestObservationBase(_))),
        "expected queued observation request effect"
    );
}

#[test]
fn commits_observing_state_before_shell_work_runs() {
    let scope = CoreDispatchTestContext::new();
    scope.set_core_state(ready_state());

    dispatch_core_event(external_cursor_demand(21), &mut |_| {})
        .expect("external ingress dispatch should succeed");

    let staged_state = current_core_state();
    assert_eq!(staged_state.lifecycle(), Lifecycle::Observing);
    assert!(
        staged_state.pending_observation().is_some(),
        "dispatch should commit reducer state before shell work runs"
    );
    assert!(
        staged_state.observation().is_none(),
        "observation collection must stay deferred until the scheduled shell edge"
    );
}
