use super::*;
use pretty_assertions::assert_eq;

fn setup_refresh_required_retry() -> (CoreDispatchTestContext, CoreEvent, RecordingExecutor) {
    let scope = CoreDispatchTestContext::new();
    let (request, _based_state) = scope.observing_state_after_base_collection();
    let refresh_required = refresh_required_probe_report(&request);
    assert!(queue_stage_batch(vec![Effect::RedrawCmdline]));
    let executor = RecordingExecutor {
        planned_follow_ups: VecDeque::from([vec![refresh_required.clone()]]),
        ..RecordingExecutor::default()
    };
    (scope, refresh_required, executor)
}

#[test]
fn probe_edge_requeues_refresh_required_follow_up_as_a_core_event() {
    let (_scope, refresh_required, mut executor) = setup_refresh_required_retry();

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
