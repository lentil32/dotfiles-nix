use super::*;
use pretty_assertions::assert_eq;

fn setup_refresh_required_retry() -> (
    CoreDispatchTestContext,
    ObservationRequest,
    CoreState,
    CoreEvent,
    RecordingExecutor,
) {
    let scope = CoreDispatchTestContext::new();
    let (request, based_state) = scope.observing_state_after_base_collection();
    let refresh_required = refresh_required_probe_report(&request);
    assert!(queue_stage_batch(vec![Effect::RedrawCmdline]));
    let executor = RecordingExecutor {
        planned_follow_ups: VecDeque::from([vec![refresh_required.clone()]]),
        ..RecordingExecutor::default()
    };
    (scope, request, based_state, refresh_required, executor)
}

#[test]
fn probe_edge_requeues_refresh_required_follow_up_as_a_core_event() {
    let (_scope, _request, _based_state, refresh_required, mut executor) =
        setup_refresh_required_retry();

    let has_more_items = drain_next_edge(&mut executor);

    assert!(
        has_more_items,
        "refresh-required probe follow-up should remain queued for a later edge"
    );
    assert_eq!(
        queued_work_count(),
        1,
        "retry event should be queued explicitly"
    );
    assert!(matches!(
        queued_front_work_item(),
        Some(ScheduledWorkUnit::CoreEvent(event)) if *event == refresh_required
    ));
}

#[test]
fn probe_edge_leaves_the_active_state_unchanged_until_the_retry_event_runs() {
    let (_scope, _request, based_state, _refresh_required, mut executor) =
        setup_refresh_required_retry();

    let _ = drain_next_edge(&mut executor);

    assert_eq!(current_core_state(), based_state);
}

#[test]
fn retry_edge_keeps_the_active_request_authoritative() {
    let (_scope, request, _based_state, _refresh_required, mut executor) =
        setup_refresh_required_retry();

    let _ = drain_next_edge(&mut executor);
    let _ = drain_next_edge(&mut executor);

    let retried_state = current_core_state();
    assert_eq!(retried_state.lifecycle(), Lifecycle::Observing);
    assert_eq!(retried_state.active_observation_request(), Some(&request));
}

#[test]
fn retry_edge_clears_the_mixed_world_observation_before_replay() {
    let (_scope, _request, _based_state, _refresh_required, mut executor) =
        setup_refresh_required_retry();

    let _ = drain_next_edge(&mut executor);
    let _ = drain_next_edge(&mut executor);

    assert!(
        current_core_state().observation().is_none(),
        "refresh-required retry should clear retained observation data before replay"
    );
}

#[test]
fn retry_edge_stages_a_new_observation_base_request_for_a_later_edge() {
    let (_scope, _request, _based_state, _refresh_required, mut executor) =
        setup_refresh_required_retry();

    let _ = drain_next_edge(&mut executor);
    let has_more_items = drain_next_edge(&mut executor);

    assert!(
        has_more_items,
        "retry transition should stage a later observation batch"
    );
    assert_eq!(queued_work_count(), 1);
    assert!(matches!(
        queued_front_work_item(),
        Some(ScheduledWorkUnit::EffectBatch(ref effects))
            if contains_observation_base_request(effects)
    ));
}
