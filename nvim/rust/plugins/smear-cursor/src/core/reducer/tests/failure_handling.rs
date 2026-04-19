use super::*;
use crate::core::event::RenderPlanFailedEvent;
use crate::core::state::ProjectionHandle;
use crate::core::state::RetainedProjection;

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

fn seeded_retained_projection(observed_at: u64) -> RetainedProjection {
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
) -> (CoreState, ProjectionHandle) {
    let (staged, proposal_id) = planned_state_after_animation_tick(
        ready_state_with_observation(cursor(9, 9)),
        proposal_observed_at,
    );
    let acknowledged = staged
        .pending_proposal()
        .and_then(|proposal| proposal.patch().basis().target_handle().cloned())
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
    last_consistent: Option<ProjectionHandle>,
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

fn assert_recovery_retry_state(
    transition: &Transition,
    expected_retry_attempt: u8,
    expected_delay_ms: u64,
    requested_at: u64,
) {
    let recovery_token = transition
        .next
        .timers()
        .active_token(TimerId::Recovery)
        .expect("recovery timer should be armed");
    pretty_assert_eq!(
        transition.next.recovery_policy(),
        RecoveryPolicyState::default().with_retry_attempt(expected_retry_attempt)
    );
    pretty_assert_eq!(
        transition.effects,
        vec![Effect::ScheduleTimer(ScheduleTimerEffect {
            token: recovery_token,
            delay: DelayBudgetMs::try_new(expected_delay_ms)
                .expect("recovery delay budget should stay positive"),
            requested_at: Millis::new(requested_at),
        })]
    );
}

#[test]
fn failed_apply_preserves_last_acknowledged_basis_in_divergence() {
    let acknowledged = seeded_retained_projection(75);

    let state = ready_state_with_observation(cursor(4, 9)).with_realization(
        RealizationLedger::Consistent {
            acknowledged: acknowledged.clone().into_handle(),
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
        Some(acknowledged.into()),
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
    assert_recovery_retry_state(&transition, 1, 16, 81);
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
    assert_recovery_retry_state(&failed, 1, 16, 85);
}

#[test]
fn effect_failure_during_collecting_requeues_the_interrupted_demand() {
    let observing = reduce(
        &ready_state(),
        external_demand_event(ExternalDemandKind::ExternalCursor, 25, None),
    )
    .next;
    let request = observing
        .pending_observation()
        .cloned()
        .expect("collecting phase should own the pending observation");

    let failed = reduce(
        &observing,
        Event::EffectFailed(EffectFailedEvent {
            proposal_id: None,
            observed_at: Millis::new(26),
        }),
    );

    pretty_assert_eq!(failed.next.lifecycle(), Lifecycle::Recovering);
    pretty_assert_eq!(
        failed.next.demand_queue().latest_cursor(),
        Some(&crate::core::state::QueuedDemand::ready(
            request.demand().clone()
        ))
    );
    assert_recovery_retry_state(&failed, 1, 16, 26);

    let recovery_token = failed
        .next
        .timers()
        .active_token(TimerId::Recovery)
        .expect("recovery timer should be armed");
    let resumed = reduce(
        &failed.next,
        Event::TimerFiredWithToken(TimerFiredWithTokenEvent {
            token: recovery_token,
            observed_at: Millis::new(42),
        }),
    );

    pretty_assert_eq!(resumed.next.lifecycle(), Lifecycle::Observing);
    pretty_assert_eq!(resumed.next.pending_observation(), Some(&request));
    assert!(matches!(
        resumed.effects.as_slice(),
        [Effect::RequestObservationBase(payload)] if payload.request == request
    ));
}

#[test]
fn effect_failure_during_observing_retries_from_the_root_demand() {
    let mut runtime = ready_state().runtime().clone();
    runtime.config.cursor_color = Some("none".to_string());
    let ready = ready_state().with_runtime(runtime);
    let observing = reduce(
        &ready,
        external_demand_event(ExternalDemandKind::ExternalCursor, 25, None),
    )
    .next;
    let request = observing
        .pending_observation()
        .cloned()
        .expect("collecting phase should own the pending observation");
    let based = reduce(
        &observing,
        Event::ObservationBaseCollected(ObservationBaseCollectedEvent {
            observation_id: request.observation_id(),
            basis: observation_basis(Some(cursor(7, 8)), 26),
            cursor_color_probe_generations: Some(cursor_color_probe_generations()),
            motion: observation_motion(),
        }),
    );

    let failed = reduce(
        &based.next,
        Event::EffectFailed(EffectFailedEvent {
            proposal_id: None,
            observed_at: Millis::new(27),
        }),
    );

    pretty_assert_eq!(failed.next.lifecycle(), Lifecycle::Recovering);
    assert!(failed.next.phase_observation().is_none());
    pretty_assert_eq!(
        failed.next.demand_queue().latest_cursor(),
        Some(&crate::core::state::QueuedDemand::ready(
            request.demand().clone()
        ))
    );
    assert_recovery_retry_state(&failed, 1, 16, 27);

    let recovery_token = failed
        .next
        .timers()
        .active_token(TimerId::Recovery)
        .expect("recovery timer should be armed");
    let resumed = reduce(
        &failed.next,
        Event::TimerFiredWithToken(TimerFiredWithTokenEvent {
            token: recovery_token,
            observed_at: Millis::new(43),
        }),
    );

    pretty_assert_eq!(resumed.next.lifecycle(), Lifecycle::Observing);
    pretty_assert_eq!(resumed.next.pending_observation(), Some(&request));
    assert!(matches!(
        resumed.effects.as_slice(),
        [Effect::RequestObservationBase(payload)] if payload.request == request
    ));
}

#[test]
fn render_plan_failure_increments_retry_attempt_and_clamps_recovery_backoff() {
    let ready = ready_state_with_observation(cursor(4, 9))
        .with_recovery_policy(RecoveryPolicyState::default().with_retry_attempt(4));
    let (armed_state, animation_token) = timer_armed_state(ready);
    let planning = reduce(&armed_state, animation_tick_event(animation_token, 88));
    let proposal_id = match planning.effects.as_slice() {
        [Effect::RequestRenderPlan(payload)] => payload.proposal_id,
        effects => panic!("expected one render-plan request effect, got {effects:?}"),
    };

    let failed = reduce(
        &planning.next,
        Event::RenderPlanFailed(RenderPlanFailedEvent {
            proposal_id,
            observed_at: Millis::new(89),
        }),
    );

    pretty_assert_eq!(failed.next.lifecycle(), Lifecycle::Recovering);
    assert_recovery_retry_state(&failed, 5, 256, 89);
}

#[test]
fn recovery_timer_replays_retained_observation_and_resets_retry_attempt() {
    let state = recovering_state_with_observation(cursor(2, 2))
        .with_recovery_policy(RecoveryPolicyState::default().with_retry_attempt(3));
    let (timers, recovery_token) = state.timers().arm(TimerId::Recovery);
    let state = state.with_timers(timers);

    let transition = reduce(
        &state,
        Event::TimerFiredWithToken(TimerFiredWithTokenEvent {
            token: recovery_token,
            observed_at: Millis::new(121),
        }),
    );

    pretty_assert_eq!(
        transition.next.recovery_policy(),
        RecoveryPolicyState::default()
    );
    pretty_assert_eq!(transition.next.lifecycle(), Lifecycle::Planning);
    assert!(transition.next.phase_observation().is_some());
    assert!(matches!(
        transition.effects.as_slice(),
        [Effect::RequestRenderPlan(_)]
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
