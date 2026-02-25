#[test]
fn adaptive_budget_has_floor_when_idle() {
    let previous = AdaptiveBudgetState {
        ewma_demand_milli: 0,
        cached_budget: ADAPTIVE_POOL_MIN_BUDGET,
    };

    let next = next_adaptive_budget(previous, 0);
    assert_eq!(next.cached_budget, ADAPTIVE_POOL_MIN_BUDGET);
    assert_eq!(next.ewma_demand_milli, 0);
}

#[test]
fn adaptive_budget_grows_with_demand() {
    let previous = AdaptiveBudgetState {
        ewma_demand_milli: 0,
        cached_budget: ADAPTIVE_POOL_MIN_BUDGET,
    };

    let next = next_adaptive_budget(previous, 120);
    assert_eq!(next.ewma_demand_milli, 120_000);
    assert_eq!(next.cached_budget, 128);
}

#[test]
fn adaptive_budget_shrinks_gradually() {
    let previous = AdaptiveBudgetState {
        ewma_demand_milli: 120_000,
        cached_budget: 120,
    };

    let next = next_adaptive_budget(previous, 0);
    assert_eq!(next.cached_budget, 112);
}

#[test]
fn adaptive_budget_honors_hard_max() {
    let previous = AdaptiveBudgetState {
        ewma_demand_milli: 0,
        cached_budget: ADAPTIVE_POOL_MIN_BUDGET,
    };

    let next = next_adaptive_budget(previous, 10_000);
    assert_eq!(next.cached_budget, ADAPTIVE_POOL_HARD_MAX_BUDGET);
}

#[test]
fn begin_tab_frame_uses_expected_demand_signal() {
    let mut tab_windows = TabWindows {
        frame_demand: 1,
        ..TabWindows::default()
    };

    begin_tab_frame(&mut tab_windows, 40);

    assert_eq!(tab_windows.last_frame_demand, 40);
    assert_eq!(tab_windows.frame_demand, 0);
    assert_eq!(tab_windows.cached_budget, 48);
}

#[test]
fn keep_budget_respects_max_kept_windows_cap() {
    assert_eq!(effective_keep_budget(120, 50), 50);
    assert_eq!(effective_keep_budget(32, 50), 32);
    assert_eq!(effective_keep_budget(16, 0), 0);
}
