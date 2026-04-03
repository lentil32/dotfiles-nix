use crate::config::RuntimeConfig;
use crate::core::runtime_reducer::MIN_RENDER_CLEANUP_DELAY_MS;
use crate::core::runtime_reducer::MIN_RENDER_HARD_PURGE_DELAY_MS;
use crate::core::runtime_reducer::RENDER_HARD_PURGE_DELAY_MULTIPLIER;
use crate::core::runtime_reducer::render_cleanup_delay_ms;
use crate::core::runtime_reducer::render_hard_cleanup_delay_ms;
use crate::test_support::proptest::pure_config;
use proptest::prelude::*;

proptest! {
    #![proptest_config(pure_config())]

    #[test]
    fn prop_render_cleanup_delays_are_floored_and_monotone(
        base_time_interval in 0.0_f64..320.0_f64,
        base_delay_event_to_smear in 0.0_f64..160.0_f64,
        extra_time_interval in 0.0_f64..320.0_f64,
        extra_delay_event_to_smear in 0.0_f64..160.0_f64,
    ) {
        let base = RuntimeConfig {
            time_interval: base_time_interval,
            delay_event_to_smear: base_delay_event_to_smear,
            ..RuntimeConfig::default()
        };
        let larger = RuntimeConfig {
            time_interval: base_time_interval + extra_time_interval,
            delay_event_to_smear: base_delay_event_to_smear + extra_delay_event_to_smear,
            ..RuntimeConfig::default()
        };

        let base_soft = render_cleanup_delay_ms(&base);
        let base_hard = render_hard_cleanup_delay_ms(&base);
        let larger_soft = render_cleanup_delay_ms(&larger);
        let larger_hard = render_hard_cleanup_delay_ms(&larger);

        prop_assert!(base_soft >= MIN_RENDER_CLEANUP_DELAY_MS);
        prop_assert!(base_hard >= MIN_RENDER_HARD_PURGE_DELAY_MS);
        prop_assert_eq!(
            base_hard,
            (base_soft * RENDER_HARD_PURGE_DELAY_MULTIPLIER).max(MIN_RENDER_HARD_PURGE_DELAY_MS)
        );
        prop_assert!(larger_soft >= base_soft);
        prop_assert!(larger_hard >= base_hard);
        prop_assert_eq!(
            larger_hard,
            (larger_soft * RENDER_HARD_PURGE_DELAY_MULTIPLIER)
                .max(MIN_RENDER_HARD_PURGE_DELAY_MS)
        );
    }
}
