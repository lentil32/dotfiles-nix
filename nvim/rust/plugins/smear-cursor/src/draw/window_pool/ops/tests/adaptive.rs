proptest! {
    #![proptest_config(pure_config())]

    #[test]
    fn prop_next_adaptive_budget_stays_within_supported_bounds(
        previous_ewma_demand_milli in 0_u64..=1_000_000,
        previous_cached_budget in ADAPTIVE_POOL_MIN_BUDGET..=ADAPTIVE_POOL_HARD_MAX_BUDGET,
        frame_demand in 0_usize..=2048,
    ) {
        let next = next_adaptive_budget(
            AdaptiveBudgetState {
                ewma_demand_milli: previous_ewma_demand_milli,
                cached_budget: previous_cached_budget,
            },
            frame_demand,
        );

        prop_assert!(next.cached_budget >= ADAPTIVE_POOL_MIN_BUDGET);
        prop_assert!(next.cached_budget <= ADAPTIVE_POOL_HARD_MAX_BUDGET);
    }

    #[test]
    fn prop_next_adaptive_budget_hits_floor_or_hard_cap_from_fresh_state(
        previous_cached_budget in ADAPTIVE_POOL_MIN_BUDGET..=ADAPTIVE_POOL_HARD_MAX_BUDGET,
        saturated_frame_demand in ADAPTIVE_POOL_HARD_MAX_BUDGET..=4096,
    ) {
        let idle = next_adaptive_budget(
            AdaptiveBudgetState {
                ewma_demand_milli: 0,
                cached_budget: previous_cached_budget,
            },
            0,
        );
        prop_assert_eq!(idle.ewma_demand_milli, 0);
        prop_assert!(idle.cached_budget >= ADAPTIVE_POOL_MIN_BUDGET);
        prop_assert!(idle.cached_budget <= previous_cached_budget);
        if previous_cached_budget == ADAPTIVE_POOL_MIN_BUDGET {
            prop_assert_eq!(idle.cached_budget, ADAPTIVE_POOL_MIN_BUDGET);
        }

        let saturated = next_adaptive_budget(
            AdaptiveBudgetState {
                ewma_demand_milli: 0,
                cached_budget: previous_cached_budget,
            },
            saturated_frame_demand,
        );
        prop_assert_eq!(saturated.cached_budget, ADAPTIVE_POOL_HARD_MAX_BUDGET);
    }

    #[test]
    fn prop_begin_tab_frame_uses_max_demand_signal_and_resets_frame_state(
        current_epoch in any::<u64>(),
        ewma_demand_milli in 0_u64..=1_000_000,
        cached_budget in ADAPTIVE_POOL_MIN_BUDGET..=ADAPTIVE_POOL_HARD_MAX_BUDGET,
        frame_demand in 0_usize..=512,
        expected_demand in 0_usize..=512,
        peak_frame_demand in 0_usize..=512,
    ) {
        let demand_signal = frame_demand.max(expected_demand);
        let expected_budget = next_adaptive_budget(
            AdaptiveBudgetState {
                ewma_demand_milli,
                cached_budget,
            },
            demand_signal,
        );
        let mut tab_windows = TabWindows {
            current_epoch: FrameEpoch(current_epoch),
            frame_demand,
            peak_frame_demand,
            ewma_demand_milli,
            cached_budget,
            ..TabWindows::default()
        };
        let next_epoch = tab_windows.current_epoch.next();

        begin_tab_frame(&mut tab_windows, expected_demand);

        prop_assert_eq!(tab_windows.current_epoch, next_epoch);
        prop_assert_eq!(tab_windows.last_frame_demand, demand_signal);
        prop_assert_eq!(tab_windows.peak_frame_demand, peak_frame_demand.max(demand_signal));
        prop_assert_eq!(tab_windows.frame_demand, 0);
        prop_assert_eq!(tab_windows.ewma_demand_milli, expected_budget.ewma_demand_milli);
        prop_assert_eq!(tab_windows.cached_budget, expected_budget.cached_budget);
    }
}
