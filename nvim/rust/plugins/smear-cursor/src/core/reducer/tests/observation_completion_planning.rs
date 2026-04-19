use super::*;

fn completed_mode_change_observation_with_cursor_queued() -> Transition {
    let ready = ready_state();
    let observing = observing_state_from_demand(&ready, ExternalDemandKind::ModeChanged, 30);
    let observing_with_cursor_queued = reduce(
        &observing,
        external_demand_event(ExternalDemandKind::ExternalCursor, 31),
    )
    .next;
    let request = active_request(&observing_with_cursor_queued);

    collect_observation_base(
        &observing_with_cursor_queued,
        &request,
        observation_basis(Some(cursor(7, 8)), 32),
        observation_motion(),
    )
}

#[test]
fn observation_completion_enters_planning_and_requests_render_plan() {
    let completed = completed_mode_change_observation_with_cursor_queued();

    pretty_assert_eq!(completed.next.lifecycle(), Lifecycle::Planning);
    pretty_assert_eq!(completed.effects.len(), 1);
    match &completed.effects[0] {
        Effect::RequestRenderPlan(payload) => {
            pretty_assert_eq!(payload.requested_at, Millis::new(32));
            pretty_assert_eq!(
                Some(payload.proposal_id),
                completed.next.pending_plan_proposal_id()
            );
        }
        other => panic!("expected first render plan request, got {other:?}"),
    }
}

#[test]
fn observation_completion_keeps_the_newer_cursor_demand_queued_until_planning_finishes() {
    let completed = completed_mode_change_observation_with_cursor_queued();

    assert!(completed.next.demand_queue().latest_cursor().is_some());
}
