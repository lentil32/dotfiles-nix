use super::*;
use crate::core::state::IngressPolicyState;
use crate::core::state::TimerState;
use crate::test_support::proptest::pure_config;
use crate::test_support::proptest::timer_id;
use proptest::collection::vec;
use proptest::prelude::*;

fn primed_state_with_shared_policy(
    armed_timers: &[TimerId],
    retry_attempt: u8,
    last_cursor_autocmd_at: u64,
    pending_delay_until: u64,
) -> (
    CoreState,
    TimerState,
    RecoveryPolicyState,
    IngressPolicyState,
) {
    let timers = armed_timers
        .iter()
        .copied()
        .fold(CoreState::default().timers(), |timers, timer_id| {
            timers.arm(timer_id).0
        });
    let recovery_policy = RecoveryPolicyState::default().with_retry_attempt(retry_attempt);
    let ingress_policy = IngressPolicyState::default()
        .note_cursor_autocmd(Millis::new(last_cursor_autocmd_at))
        .note_pending_delay_until(Millis::new(pending_delay_until));
    let primed = CoreState::default()
        .with_timers(timers)
        .with_recovery_policy(recovery_policy)
        .with_ingress_policy(ingress_policy)
        .into_primed();

    (primed, timers, recovery_policy, ingress_policy)
}

fn shared_protocol_snapshot(
    state: &CoreState,
) -> (TimerState, RecoveryPolicyState, IngressPolicyState) {
    (
        state.timers(),
        state.recovery_policy(),
        state.ingress_policy(),
    )
}

proptest! {
    #![proptest_config(pure_config())]

    #[test]
    fn prop_lifecycle_constructors_preserve_shared_protocol_state(
        armed_timers in vec(timer_id(), 0..=8),
        retry_attempt in any::<u8>(),
        last_cursor_autocmd_at in any::<u16>(),
        pending_delay_until in any::<u16>(),
        request_seq in 1_u64..=64_u64,
        observed_at in 0_u64..=500_u64,
        cursor_row in 1_u32..=40_u32,
        cursor_col in 1_u32..=120_u32,
    ) {
        let (primed, expected_timers, recovery_policy, ingress_policy) =
            primed_state_with_shared_policy(
                &armed_timers,
                retry_attempt,
                u64::from(last_cursor_autocmd_at),
                u64::from(pending_delay_until),
            );
        let request = observation_request(
            request_seq,
            ExternalDemandKind::ExternalCursor,
            observed_at,
        );
        let observation = ObservationSnapshot::new(
            request.clone(),
            observation_basis(&request, Some(cursor(cursor_row, cursor_col)), observed_at),
            observation_motion(),
        );
        let observing = primed
            .clone()
            .with_demand_queue(DemandQueue::default())
            .enter_observing_request(request);
        let ready = observing
            .clone()
            .with_active_observation(Some(observation.clone()))
            .expect("observation staging should succeed")
            .enter_ready(observation);
        let recovering = ready.clone().enter_recovering();
        let expected = (expected_timers, recovery_policy, ingress_policy);

        prop_assert_eq!(shared_protocol_snapshot(&ready), expected);
        let (applying, proposal_id) =
            applying_state_with_realization_plan(ready, noop_realization_plan(), false, None);
        let (cleared, _) = applying
            .clear_pending_for(proposal_id)
            .expect("proposal should clear back to ready");

        prop_assert_eq!(shared_protocol_snapshot(&primed), expected);
        prop_assert_eq!(shared_protocol_snapshot(&observing), expected);
        prop_assert_eq!(shared_protocol_snapshot(&recovering), expected);
        prop_assert_eq!(shared_protocol_snapshot(&cleared), expected);
    }
}
