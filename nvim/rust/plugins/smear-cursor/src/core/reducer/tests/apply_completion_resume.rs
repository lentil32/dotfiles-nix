use super::*;

fn completed_apply_with_pending_mode_change() -> (CoreState, Transition) {
    let (staged, proposal_id) = applying_state_with_realization_plan(
        ready_state_with_observation(cursor(4, 9)),
        noop_realization_plan(),
        true,
        Some(Millis::new(90)),
    );
    let staged = reduce(
        &staged,
        external_demand_event(ExternalDemandKind::ModeChanged, 71, None),
    )
    .next;

    let completed = reduce(
        &staged,
        Event::ApplyReported(ApplyReport::AppliedFully {
            proposal_id,
            observed_at: Millis::new(72),
            visual_change: false,
        }),
    );
    (staged, completed)
}

#[test]
fn apply_completion_clears_the_in_flight_proposal_and_resumes_observing() {
    let (_staged, completed) = completed_apply_with_pending_mode_change();

    pretty_assert_eq!(completed.next.lifecycle(), Lifecycle::Observing);
    pretty_assert_eq!(completed.next.realization(), &RealizationLedger::Cleared);
    assert!(completed.next.pending_proposal().is_none());
}

#[test]
fn apply_completion_requests_the_pending_observation_before_rearming_animation() {
    let (staged, completed) = completed_apply_with_pending_mode_change();

    pretty_assert_eq!(
        completed.effects[0],
        Effect::RequestObservationBase(RequestObservationBaseEffect {
            request: observation_request(1, ExternalDemandKind::ModeChanged, 71),
            context: observation_runtime_context(&staged, ExternalDemandKind::ModeChanged),
        })
    );
}

#[test]
fn apply_completion_rearms_animation_after_requesting_the_pending_observation() {
    let (_staged, completed) = completed_apply_with_pending_mode_change();

    pretty_assert_eq!(completed.effects.len(), 2);
    assert!(matches!(
        completed.effects[1],
        Effect::ScheduleTimer(ScheduleTimerEffect { .. })
    ));
}
