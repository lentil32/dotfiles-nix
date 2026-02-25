#[test]
fn available_window_index_for_placement_returns_matching_available_window() {
    let target = WindowPlacement {
        row: 14,
        col: 22,
        width: 1,
        zindex: 300,
    };
    let mut tab_windows = TabWindows {
        windows: vec![
            cached(10, 110, 1),
            CachedRenderWindow::new_in_use(
                WindowBufferHandle {
                    window_id: 20,
                    buffer_id: 120,
                },
                FrameEpoch(9),
                target,
            ),
            CachedRenderWindow {
                handles: WindowBufferHandle {
                    window_id: 30,
                    buffer_id: 130,
                },
                lifecycle: CachedWindowLifecycle::AvailableVisible {
                    last_used_epoch: FrameEpoch(7),
                },
                placement: Some(target),
            },
        ],
        ..TabWindows::default()
    };
    rebuild_available_window_placement_index(&mut tab_windows);

    let selected = available_window_index_for_placement(&mut tab_windows, target);
    assert_eq!(selected, Some(2));

    tab_windows.windows[2].lifecycle = CachedWindowLifecycle::InUse {
        epoch: FrameEpoch(10),
    };
    assert_eq!(
        available_window_index_for_placement(&mut tab_windows, target),
        None
    );
}

#[test]
fn available_window_index_for_placement_returns_none_for_missing_or_unplaced_windows() {
    let target = WindowPlacement {
        row: 4,
        col: 9,
        width: 1,
        zindex: 40,
    };
    let mut tab_windows = TabWindows {
        windows: vec![
            CachedRenderWindow {
                handles: WindowBufferHandle {
                    window_id: 1,
                    buffer_id: 11,
                },
                lifecycle: CachedWindowLifecycle::AvailableHidden {
                    last_used_epoch: FrameEpoch(1),
                },
                placement: None,
            },
            CachedRenderWindow {
                handles: WindowBufferHandle {
                    window_id: 2,
                    buffer_id: 12,
                },
                lifecycle: CachedWindowLifecycle::AvailableVisible {
                    last_used_epoch: FrameEpoch(2),
                },
                placement: Some(WindowPlacement {
                    row: 4,
                    col: 10,
                    width: 1,
                    zindex: 40,
                }),
            },
        ],
        ..TabWindows::default()
    };

    assert_eq!(
        available_window_index_for_placement(&mut tab_windows, target),
        None
    );
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
            width: 1,
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
            width: 1,
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
                width: 1,
                zindex: 100,
            },
        ),
        cached(2, 20, 1),
        cached(3, 30, 4),
        cached(4, 40, 1),
        cached(5, 50, 7),
    ];

    assert_eq!(lru_prune_indices(&windows, 3), vec![2, 4]);
}

#[test]
fn cached_window_needs_reconfigure_only_when_placement_or_visibility_changes() {
    let placement = WindowPlacement {
        row: 10,
        col: 20,
        width: 1,
        zindex: 30,
    };
    let mut cached = CachedRenderWindow {
        handles: WindowBufferHandle {
            window_id: 77,
            buffer_id: 88,
        },
        lifecycle: CachedWindowLifecycle::AvailableVisible {
            last_used_epoch: FrameEpoch(5),
        },
        placement: Some(placement),
    };

    assert!(!cached.needs_reconfigure(placement));
    assert!(cached.needs_reconfigure(WindowPlacement {
        row: 10,
        col: 21,
        width: 1,
        zindex: 30,
    }));

    cached.mark_hidden();
    assert!(cached.needs_reconfigure(placement));
}

#[test]
fn tab_windows_payload_cache_matches_and_clears() {
    let mut tab_windows = TabWindows::default();
    assert!(!tab_windows.cached_payload_matches(101, 111));

    tab_windows.cache_payload(101, 111);
    assert!(tab_windows.cached_payload_matches(101, 111));
    assert!(!tab_windows.cached_payload_matches(101, 222));

    tab_windows.cache_payload(101, 222);
    assert!(tab_windows.cached_payload_matches(101, 222));
    assert!(!tab_windows.cached_payload_matches(101, 111));

    tab_windows.clear_payload(101);
    assert!(!tab_windows.cached_payload_matches(101, 222));
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
                    width: 1,
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
                    width: 1,
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
