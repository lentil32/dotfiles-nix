use super::*;
use crate::core::state::ProjectionSnapshot;

// Keep these as curated examples: reducer failure contracts and stale-token
// handling are easier to audit as named scenarios than as generated sequences.

fn staged_noop_apply(position: CursorPosition) -> (CoreState, ProposalId) {
    applying_state_with_realization_plan(
        ready_state_with_observation(position),
        noop_realization_plan(),
        false,
        None,
    )
}

fn seeded_projection_snapshot(observed_at: u64) -> ProjectionSnapshot {
    let (seeded, _proposal_id) =
        planned_state_after_animation_tick(ready_state_with_observation(cursor(9, 9)), observed_at);
    seeded
        .pending_proposal()
        .and_then(|proposal| proposal.patch().basis().target().cloned())
        .expect("seeded acknowledged projection")
}

fn ready_state_with_acknowledged_projection(
    proposal_observed_at: u64,
    apply_observed_at: u64,
) -> (CoreState, ProjectionSnapshot) {
    let (staged, proposal_id) = planned_state_after_animation_tick(
        ready_state_with_observation(cursor(9, 9)),
        proposal_observed_at,
    );
    let acknowledged = staged
        .pending_proposal()
        .and_then(|proposal| proposal.patch().basis().target().cloned())
        .expect("first acknowledged target");
    let ready = reduce(
        &staged,
        Event::ApplyReported(ApplyReport::AppliedFully {
            proposal_id,
            observed_at: Millis::new(apply_observed_at),
            visual_change: true,
        }),
    )
    .next;
    (ready, acknowledged)
}

fn assert_recovering_diverged(
    transition: &Transition,
    last_consistent: Option<ProjectionSnapshot>,
    divergence: RealizationDivergence,
) {
    pretty_assert_eq!(transition.next.lifecycle(), Lifecycle::Recovering);
    pretty_assert_eq!(
        transition.next.realization(),
        &RealizationLedger::Diverged {
            last_consistent,
            divergence,
        }
    );
}

#[test]
fn failed_apply_preserves_last_acknowledged_basis_in_divergence() {
    let acknowledged = seeded_projection_snapshot(75);

    let state = ready_state_with_observation(cursor(4, 9)).with_realization(
        RealizationLedger::Consistent {
            acknowledged: acknowledged.clone(),
        },
    );
    let (staged, proposal_id) =
        applying_state_with_realization_plan(state, noop_realization_plan(), false, None);

    let failed = reduce(
        &staged,
        Event::ApplyReported(ApplyReport::ApplyFailed {
            proposal_id,
            reason: crate::core::state::ApplyFailureKind::ShellError,
            divergence: RealizationDivergence::ShellStateUnknown,
            observed_at: Millis::new(77),
        }),
    );

    assert_recovering_diverged(
        &failed,
        Some(acknowledged),
        RealizationDivergence::ShellStateUnknown,
    );
}

#[test]
fn viewport_drift_apply_failure_enters_recovering() {
    let (staged, proposal_id) = staged_noop_apply(cursor(4, 9));

    let failed = reduce(
        &staged,
        Event::ApplyReported(ApplyReport::ApplyFailed {
            proposal_id,
            reason: crate::core::state::ApplyFailureKind::ViewportDrift,
            divergence: RealizationDivergence::ShellStateUnknown,
            observed_at: Millis::new(78),
        }),
    );

    assert_recovering_diverged(&failed, None, RealizationDivergence::ShellStateUnknown);
}

#[test]
fn stale_apply_report_is_ignored_by_proposal_id() {
    let (staged, proposal_id) = staged_noop_apply(cursor(4, 9));

    let stale = reduce(
        &staged,
        Event::ApplyReported(ApplyReport::AppliedFully {
            proposal_id: proposal_id.next(),
            observed_at: Millis::new(79),
            visual_change: true,
        }),
    );

    pretty_assert_eq!(stale.next, staged);
    pretty_assert_eq!(
        stale.effects,
        vec![Effect::RecordEventLoopMetric(
            EventLoopMetricEffect::StaleToken
        )]
    );
}

#[test]
fn degraded_apply_enters_recovering_and_schedules_recovery_timer() {
    let (staged, proposal_id) = staged_noop_apply(cursor(4, 9));
    let divergence =
        RealizationDivergence::ApplyMetrics(DegradedApplyMetrics::new(8, 5, 2, 1, 0, 0, 1));

    let transition = reduce(
        &staged,
        Event::ApplyReported(ApplyReport::AppliedDegraded {
            proposal_id,
            divergence,
            observed_at: Millis::new(81),
            visual_change: true,
        }),
    );

    assert_recovering_diverged(&transition, None, divergence);
    assert!(matches!(
        transition.effects.as_slice(),
        [Effect::ScheduleTimer(ScheduleTimerEffect { .. })]
    ));
}

#[test]
fn degraded_apply_keeps_last_acknowledged_projection() {
    let (ready, acknowledged) = ready_state_with_acknowledged_projection(82, 83);

    let (staged, second_proposal_id) =
        applying_state_with_realization_plan(ready, noop_realization_plan(), false, None);

    let divergence =
        RealizationDivergence::ApplyMetrics(DegradedApplyMetrics::new(10, 7, 1, 1, 0, 0, 0));
    let degraded = reduce(
        &staged,
        Event::ApplyReported(ApplyReport::AppliedDegraded {
            proposal_id: second_proposal_id,
            divergence,
            observed_at: Millis::new(85),
            visual_change: false,
        }),
    );

    assert_recovering_diverged(&degraded, Some(acknowledged), divergence);
}

#[test]
fn effect_failure_is_ignored_before_initialize() {
    let state = CoreState::default();

    let transition = reduce(
        &state,
        Event::EffectFailed(EffectFailedEvent {
            proposal_id: None,
            observed_at: Millis::new(99),
        }),
    );

    pretty_assert_eq!(transition.next, state);
    assert!(transition.effects.is_empty());
}

#[test]
fn effect_failure_for_pending_proposal_preserves_acknowledged_basis_in_divergence() {
    let (ready, acknowledged) = ready_state_with_acknowledged_projection(82, 83);

    let (staged, proposal_id) =
        applying_state_with_realization_plan(ready, noop_realization_plan(), false, None);

    let failed = reduce(
        &staged,
        Event::EffectFailed(EffectFailedEvent {
            proposal_id: Some(proposal_id),
            observed_at: Millis::new(85),
        }),
    );

    assert_recovering_diverged(
        &failed,
        Some(acknowledged),
        RealizationDivergence::ShellStateUnknown,
    );
    assert!(matches!(
        failed.effects.as_slice(),
        [Effect::ScheduleTimer(ScheduleTimerEffect { .. })]
    ));
}

#[test]
fn stale_effect_failure_is_ignored_by_proposal_id() {
    let (staged, proposal_id) = staged_noop_apply(cursor(4, 7));
    let stale_proposal_id = proposal_id.next();

    let stale = reduce(
        &staged,
        Event::EffectFailed(EffectFailedEvent {
            proposal_id: Some(stale_proposal_id),
            observed_at: Millis::new(86),
        }),
    );

    pretty_assert_eq!(stale.next, staged);
    pretty_assert_eq!(
        stale.effects,
        vec![Effect::RecordEventLoopMetric(
            EventLoopMetricEffect::StaleToken
        )]
    );
}

#[test]
fn stale_timer_token_is_ignored_without_mutating_state() {
    let state = recovering_state_with_observation(cursor(2, 2));
    let (timers, stale_token) = state.timers().arm(TimerId::Recovery);
    let state = state.with_timers(timers);
    let (timers, _fresh_token) = state.timers().arm(TimerId::Recovery);
    let state = state.with_timers(timers);

    let transition = reduce(
        &state,
        Event::TimerLostWithToken(TimerLostWithTokenEvent {
            token: stale_token,
            observed_at: Millis::new(120),
        }),
    );

    pretty_assert_eq!(transition.next, state);
    pretty_assert_eq!(
        transition.effects,
        vec![Effect::RecordEventLoopMetric(
            EventLoopMetricEffect::StaleToken
        )]
    );
}
