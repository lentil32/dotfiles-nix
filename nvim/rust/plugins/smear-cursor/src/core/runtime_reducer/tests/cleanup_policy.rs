use super::*;
use crate::test_support::proptest::pure_config;
use pretty_assertions::assert_eq;

fn nonempty_cleanup_policy_input() -> impl Strategy<Value = CleanupPolicyInput> {
    (
        1_u16..=2_000,
        1_u16..=4_000,
        1_usize..=128,
        0_usize..=512,
        1_usize..=256,
        0_u16..=128,
    )
        .prop_map(
            |(
                soft_cleanup_delay_ms,
                hard_cleanup_delay_ms,
                pool_total_windows,
                recent_frame_demand,
                max_kept_windows,
                callback_duration_estimate_ms,
            )| CleanupPolicyInput {
                idle_ms: 0,
                soft_cleanup_delay_ms: u64::from(soft_cleanup_delay_ms),
                hard_cleanup_delay_ms: u64::from(hard_cleanup_delay_ms),
                pool_total_windows,
                recent_frame_demand,
                max_kept_windows,
                callback_duration_estimate_ms: f64::from(callback_duration_estimate_ms),
            },
        )
}

#[test]
fn empty_pool_stays_warm_without_rearming_cleanup() {
    let input = CleanupPolicyInput {
        idle_ms: 10_000,
        soft_cleanup_delay_ms: 200,
        hard_cleanup_delay_ms: 3_000,
        pool_total_windows: 0,
        recent_frame_demand: 0,
        max_kept_windows: 64,
        callback_duration_estimate_ms: 10.0,
    };

    assert_eq!(decide_cleanup_directive(input), CleanupDirective::KeepWarm);
    assert_eq!(next_cleanup_check_delay_ms(input), None);
}

proptest! {
    #![proptest_config(pure_config())]

    #[test]
    fn prop_cleanup_directive_partitions_idle_phases(
        input in nonempty_cleanup_policy_input(),
    ) {
        let keep_warm_until_ms = keep_warm_until_ms(input);
        let keep_warm_input = CleanupPolicyInput {
            idle_ms: keep_warm_until_ms - 1,
            ..input
        };
        prop_assert_eq!(
            decide_cleanup_directive(keep_warm_input),
            CleanupDirective::KeepWarm
        );

        let hard_purge_input = CleanupPolicyInput {
            idle_ms: input.hard_cleanup_delay_ms.max(keep_warm_until_ms),
            ..input
        };
        prop_assert_eq!(
            decide_cleanup_directive(hard_purge_input),
            CleanupDirective::HardPurge
        );

        if keep_warm_until_ms < input.hard_cleanup_delay_ms {
            let soft_clear_input = CleanupPolicyInput {
                idle_ms: keep_warm_until_ms,
                ..input
            };
            prop_assert_eq!(
                decide_cleanup_directive(soft_clear_input),
                CleanupDirective::SoftClear {
                    max_kept_windows: input.max_kept_windows,
                }
            );
        } else {
            let pre_hard_input = CleanupPolicyInput {
                idle_ms: input.hard_cleanup_delay_ms - 1,
                ..input
            };
            prop_assert_eq!(
                decide_cleanup_directive(pre_hard_input),
                CleanupDirective::KeepWarm
            );
        }
    }

    #[test]
    fn prop_pending_work_only_delays_cleanup(
        soft_cleanup_delay_ms in 1_u16..=2_000,
        hard_cleanup_delay_ms in 1_u16..=4_000,
        pool_total_windows in 1_usize..=128,
        max_kept_windows in 1_usize..=256,
        idle_ms in 0_u16..=6_000,
        base_recent_frame_demand in 0_usize..=512,
        extra_recent_frame_demand in 0_usize..=512,
        base_callback_duration_estimate_ms in 0_u16..=128,
        extra_callback_duration_estimate_ms in 0_u16..=128,
    ) {
        let low_penalty = CleanupPolicyInput {
            idle_ms: u64::from(idle_ms),
            soft_cleanup_delay_ms: u64::from(soft_cleanup_delay_ms),
            hard_cleanup_delay_ms: u64::from(hard_cleanup_delay_ms),
            pool_total_windows,
            recent_frame_demand: base_recent_frame_demand,
            max_kept_windows,
            callback_duration_estimate_ms: f64::from(base_callback_duration_estimate_ms),
        };
        let high_penalty = CleanupPolicyInput {
            recent_frame_demand: base_recent_frame_demand.saturating_add(extra_recent_frame_demand),
            callback_duration_estimate_ms: f64::from(
                base_callback_duration_estimate_ms
                    .saturating_add(extra_callback_duration_estimate_ms),
            ),
            ..low_penalty
        };

        prop_assert!(keep_warm_until_ms(high_penalty) >= keep_warm_until_ms(low_penalty));

        let low_rank = match decide_cleanup_directive(low_penalty) {
            CleanupDirective::KeepWarm => 0_u8,
            CleanupDirective::SoftClear { .. } => 1,
            CleanupDirective::HardPurge => 2,
        };
        let high_rank = match decide_cleanup_directive(high_penalty) {
            CleanupDirective::KeepWarm => 0_u8,
            CleanupDirective::SoftClear { .. } => 1,
            CleanupDirective::HardPurge => 2,
        };
        prop_assert!(high_rank <= low_rank);
    }

    #[test]
    fn prop_cleanup_rearm_delay_matches_current_phase(
        input in nonempty_cleanup_policy_input(),
        idle_ms in 0_u16..=6_000,
    ) {
        let input = CleanupPolicyInput {
            idle_ms: u64::from(idle_ms),
            ..input
        };
        let keep_warm_until_ms = keep_warm_until_ms(input);

        match decide_cleanup_directive(input) {
            CleanupDirective::KeepWarm => {
                prop_assert_eq!(
                    next_cleanup_check_delay_ms(input),
                    Some(keep_warm_until_ms - input.idle_ms)
                );
            }
            CleanupDirective::SoftClear { max_kept_windows } => {
                prop_assert_eq!(max_kept_windows, input.max_kept_windows);
                prop_assert_eq!(
                    next_cleanup_check_delay_ms(input),
                    Some(input.hard_cleanup_delay_ms - input.idle_ms)
                );
            }
            CleanupDirective::HardPurge => {
                prop_assert_eq!(next_cleanup_check_delay_ms(input), None);
            }
        }
    }
}
