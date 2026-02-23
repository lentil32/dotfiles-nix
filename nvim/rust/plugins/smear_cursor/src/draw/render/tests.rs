use crate::draw::{
    ADAPTIVE_POOL_BUDGET_MARGIN, ADAPTIVE_POOL_EWMA_SCALE, ADAPTIVE_POOL_HARD_MAX_BUDGET,
    ADAPTIVE_POOL_MIN_BUDGET, AdaptiveBudgetState, CachedRenderWindow, CachedWindowLifecycle,
    EpochRollover, FrameEpoch, TabWindows, WindowBufferHandle, WindowPlacement,
    adjust_tracking_after_remove, effective_keep_budget, lru_prune_indices, next_adaptive_budget,
    rollover_in_use_windows,
};

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
    assert_eq!(next.ewma_demand_milli, 120_u64 * ADAPTIVE_POOL_EWMA_SCALE);
    assert_eq!(next.cached_budget, 120 + ADAPTIVE_POOL_BUDGET_MARGIN);
}

#[test]
fn adaptive_budget_shrinks_gradually() {
    let previous = AdaptiveBudgetState {
        ewma_demand_milli: 120_u64 * ADAPTIVE_POOL_EWMA_SCALE,
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
fn keep_budget_respects_max_kept_windows_cap() {
    assert_eq!(effective_keep_budget(120, 50), 50);
    assert_eq!(effective_keep_budget(32, 50), 32);
    assert_eq!(effective_keep_budget(16, 0), 0);
}

fn cached(window_id: i32, buffer_id: i32, last_used_epoch: u64) -> CachedRenderWindow {
    CachedRenderWindow {
        handles: WindowBufferHandle {
            window_id,
            buffer_id,
        },
        lifecycle: CachedWindowLifecycle::Available {
            last_used_epoch: FrameEpoch(last_used_epoch),
            visible: false,
        },
        placement: Some(WindowPlacement {
            row: 0,
            col: 0,
            zindex: 50,
        }),
    }
}

#[test]
fn rollover_releases_in_use_window_from_previous_epoch() {
    let handles = WindowBufferHandle {
        window_id: 10,
        buffer_id: 11,
    };
    let mut cached = CachedRenderWindow::new_in_use(
        handles,
        FrameEpoch(9),
        WindowPlacement {
            row: 4,
            col: 8,
            zindex: 100,
        },
    );
    assert_eq!(
        cached.rollover_to_next_epoch(FrameEpoch(9)),
        EpochRollover::ReleasedForReuse
    );
    assert_eq!(cached.available_epoch(), Some(FrameEpoch(9)));
    assert!(cached.is_available_for_reuse());
    assert!(cached.should_hide());
    cached.mark_hidden();
    assert!(!cached.should_hide());
}

#[test]
fn rollover_recovers_stale_in_use_window() {
    let handles = WindowBufferHandle {
        window_id: 20,
        buffer_id: 21,
    };
    let mut cached = CachedRenderWindow::new_in_use(
        handles,
        FrameEpoch(3),
        WindowPlacement {
            row: 7,
            col: 3,
            zindex: 100,
        },
    );
    assert_eq!(
        cached.rollover_to_next_epoch(FrameEpoch(5)),
        EpochRollover::RecoveredStaleInUse
    );
    assert_eq!(cached.available_epoch(), Some(FrameEpoch(3)));
    assert!(cached.is_available_for_reuse());
    assert!(cached.should_hide());
    cached.mark_hidden();
    assert!(!cached.should_hide());
}

#[test]
fn lru_prune_indices_empty_when_budget_sufficient() {
    let windows = vec![cached(1, 10, 7), cached(2, 20, 8)];
    assert!(lru_prune_indices(&windows, 2).is_empty());
    assert!(lru_prune_indices(&windows, 3).is_empty());
}

#[test]
fn lru_prune_indices_removes_oldest_epochs_deterministically() {
    let windows = vec![
        cached(1, 10, 9),
        CachedRenderWindow::new_in_use(
            WindowBufferHandle {
                window_id: 90,
                buffer_id: 99,
            },
            FrameEpoch(9),
            WindowPlacement {
                row: 3,
                col: 9,
                zindex: 100,
            },
        ),
        cached(2, 20, 1),
        cached(3, 30, 4),
        cached(4, 40, 1),
        cached(5, 50, 7),
    ];

    // Keep three newest reusable epochs; in-use windows are excluded from LRU pruning.
    assert_eq!(lru_prune_indices(&windows, 3), vec![2, 4]);
}

#[test]
fn cached_window_needs_reconfigure_only_when_placement_or_visibility_changes() {
    let placement = WindowPlacement {
        row: 10,
        col: 20,
        zindex: 30,
    };
    let mut cached = CachedRenderWindow {
        handles: WindowBufferHandle {
            window_id: 77,
            buffer_id: 88,
        },
        lifecycle: CachedWindowLifecycle::Available {
            last_used_epoch: FrameEpoch(5),
            visible: true,
        },
        placement: Some(placement),
    };

    assert!(!cached.needs_reconfigure(placement));
    assert!(cached.needs_reconfigure(WindowPlacement {
        row: 10,
        col: 21,
        zindex: 30,
    }));

    cached.mark_hidden();
    assert!(cached.needs_reconfigure(placement));
}

#[test]
fn tab_windows_payload_cache_matches_and_clears() {
    let mut tab_windows = TabWindows::default();
    assert!(!tab_windows.cached_payload_matches(101, "█", "SmearCursor1"));

    tab_windows.cache_payload(101, "█", "SmearCursor1");
    assert!(tab_windows.cached_payload_matches(101, "█", "SmearCursor1"));
    assert!(!tab_windows.cached_payload_matches(101, "▌", "SmearCursor1"));
    assert!(!tab_windows.cached_payload_matches(101, "█", "SmearCursor2"));

    tab_windows.cache_payload(101, "▌", "SmearCursor2");
    assert!(tab_windows.cached_payload_matches(101, "▌", "SmearCursor2"));
    assert!(!tab_windows.cached_payload_matches(101, "█", "SmearCursor1"));

    tab_windows.clear_payload(101);
    assert!(!tab_windows.cached_payload_matches(101, "▌", "SmearCursor2"));
}

#[test]
fn adjust_tracking_after_remove_reindexes_in_use_and_scan_position() {
    let mut tab_windows = TabWindows {
        reuse_scan_index: 4,
        in_use_indices: vec![0, 2, 4],
        visible_available_indices: vec![1, 3, 4],
        windows: vec![
            cached(1, 101, 1),
            cached(2, 102, 2),
            cached(3, 103, 3),
            cached(4, 104, 4),
            cached(5, 105, 5),
        ],
        ..TabWindows::default()
    };

    adjust_tracking_after_remove(&mut tab_windows, 1);

    assert_eq!(tab_windows.reuse_scan_index, 3);
    assert_eq!(tab_windows.in_use_indices, vec![0, 1, 3]);
    assert_eq!(tab_windows.visible_available_indices, vec![2, 3]);
}

#[test]
fn rollover_in_use_windows_releases_tracked_windows() {
    let previous_epoch = FrameEpoch(12);
    let mut tab_windows = TabWindows {
        windows: vec![
            CachedRenderWindow::new_in_use(
                WindowBufferHandle {
                    window_id: 31,
                    buffer_id: 41,
                },
                previous_epoch,
                WindowPlacement {
                    row: 1,
                    col: 1,
                    zindex: 40,
                },
            ),
            CachedRenderWindow::new_in_use(
                WindowBufferHandle {
                    window_id: 32,
                    buffer_id: 42,
                },
                previous_epoch,
                WindowPlacement {
                    row: 1,
                    col: 2,
                    zindex: 40,
                },
            ),
        ],
        in_use_indices: vec![0, 1, 99],
        ..TabWindows::default()
    };

    rollover_in_use_windows(&mut tab_windows, previous_epoch);

    assert!(tab_windows.in_use_indices.is_empty());
    assert_eq!(tab_windows.visible_available_indices, vec![0, 1]);
    assert!(
        tab_windows
            .windows
            .iter()
            .all(|cached| cached.is_available_for_reuse() && cached.should_hide())
    );
}
