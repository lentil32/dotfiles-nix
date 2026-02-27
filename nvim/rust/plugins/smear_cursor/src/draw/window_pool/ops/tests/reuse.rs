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
    tab_windows.sync_lifecycle_counters();

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

#[test]
fn rollover_populates_available_placement_index_without_rebuild() {
    let placement = WindowPlacement {
        row: 8,
        col: 13,
        width: 1,
        zindex: 40,
    };
    let previous_epoch = FrameEpoch(12);
    let mut tab_windows = TabWindows {
        windows: vec![CachedRenderWindow::new_in_use(
            WindowBufferHandle {
                window_id: 31,
                buffer_id: 41,
            },
            previous_epoch,
            placement,
        )],
        in_use_indices: vec![0],
        ..TabWindows::default()
    };
    tab_windows.sync_lifecycle_counters();

    rollover_in_use_windows(&mut tab_windows, previous_epoch);

    assert_eq!(
        available_window_index_for_placement(&mut tab_windows, placement),
        Some(0)
    );
}

#[test]
fn changing_reuse_placement_drops_stale_key_and_tracks_new_one_next_frame() {
    let old_placement = WindowPlacement {
        row: 10,
        col: 20,
        width: 1,
        zindex: 300,
    };
    let new_placement = WindowPlacement {
        row: 11,
        col: 24,
        width: 1,
        zindex: 300,
    };
    let mut tab_windows = TabWindows {
        windows: vec![CachedRenderWindow {
            handles: WindowBufferHandle {
                window_id: 11,
                buffer_id: 111,
            },
            lifecycle: CachedWindowLifecycle::AvailableVisible {
                last_used_epoch: FrameEpoch(1),
            },
            placement: Some(old_placement),
        }],
        ..TabWindows::default()
    };
    rebuild_available_window_placement_index(&mut tab_windows);
    let previous_lifecycle = tab_windows.windows[0].lifecycle;
    let previous_placement = tab_windows.windows[0].placement;
    assert!(tab_windows.windows[0].mark_in_use(FrameEpoch(2)));
    let next_lifecycle = tab_windows.windows[0].lifecycle;
    tab_windows.track_available_window_transition(
        0,
        previous_lifecycle,
        previous_placement,
        next_lifecycle,
        previous_placement,
    );
    tab_windows.track_window_transition(previous_lifecycle, next_lifecycle);
    tab_windows.windows[0].set_placement(new_placement);
    tab_windows.in_use_indices.push(0);

    assert!(
        !tab_windows
            .available_windows_by_placement
            .contains_key(&old_placement)
    );

    begin_tab_frame(&mut tab_windows, 0);

    assert_eq!(
        available_window_index_for_placement(&mut tab_windows, new_placement),
        Some(0)
    );
    assert!(
        !tab_windows
            .available_windows_by_placement
            .contains_key(&old_placement)
    );
}

#[test]
fn prepared_pool_supports_expected_number_of_reuse_acquires() {
    let placement = WindowPlacement {
        row: 10,
        col: 20,
        width: 1,
        zindex: 300,
    };
    let mut tab_windows = TabWindows {
        windows: vec![
            CachedRenderWindow {
                handles: WindowBufferHandle {
                    window_id: 11,
                    buffer_id: 111,
                },
                lifecycle: CachedWindowLifecycle::AvailableVisible {
                    last_used_epoch: FrameEpoch(1),
                },
                placement: Some(placement),
            },
            CachedRenderWindow {
                handles: WindowBufferHandle {
                    window_id: 12,
                    buffer_id: 112,
                },
                lifecycle: CachedWindowLifecycle::AvailableVisible {
                    last_used_epoch: FrameEpoch(1),
                },
                placement: Some(placement),
            },
            CachedRenderWindow {
                handles: WindowBufferHandle {
                    window_id: 13,
                    buffer_id: 113,
                },
                lifecycle: CachedWindowLifecycle::AvailableVisible {
                    last_used_epoch: FrameEpoch(1),
                },
                placement: Some(placement),
            },
        ],
        ..TabWindows::default()
    };
    rebuild_available_window_placement_index(&mut tab_windows);
    let mut tabs = tabs_with(tab_windows);

    for _ in 0..3 {
        let acquired = acquire(&mut tabs, 1, 1, placement, AllocationPolicy::ReuseOnly)
            .expect("prepared pool must satisfy expected reuse demand");
        assert_eq!(acquired.reuse_failures, ReuseFailureCounters::default());
    }

    let err = acquire(&mut tabs, 1, 1, placement, AllocationPolicy::ReuseOnly)
        .expect_err("fourth acquire should exhaust a three-window prepared pool");
    assert_eq!(
        err,
        AcquireError::Exhausted {
            allocation_policy: AllocationPolicy::ReuseOnly,
        }
    );
}

#[test]
fn frame_capacity_target_keeps_one_reuse_only_spare() {
    assert_eq!(
        frame_capacity_target(0, 3, 16, AllocationPolicy::ReuseOnly),
        4
    );
    assert_eq!(
        frame_capacity_target(2, 3, 16, AllocationPolicy::ReuseOnly),
        6
    );
}

#[test]
fn frame_capacity_target_stays_exact_for_bootstrap_and_empty_frames() {
    assert_eq!(
        frame_capacity_target(0, 3, 16, AllocationPolicy::BootstrapIfPoolEmpty),
        3
    );
    assert_eq!(
        frame_capacity_target(0, 0, 16, AllocationPolicy::ReuseOnly),
        0
    );
}

#[test]
fn frame_capacity_target_respects_max_kept_windows() {
    assert_eq!(
        frame_capacity_target(3, 3, 6, AllocationPolicy::ReuseOnly),
        6
    );
    assert_eq!(
        frame_capacity_target(3, 3, 5, AllocationPolicy::BootstrapIfPoolEmpty),
        5
    );
}

#[test]
fn warm_spare_does_not_change_matching_reuse_order_for_same_span_plan() {
    let placement = WindowPlacement {
        row: 10,
        col: 20,
        width: 1,
        zindex: 300,
    };
    let mut tab_windows = TabWindows {
        windows: vec![
            CachedRenderWindow {
                handles: WindowBufferHandle {
                    window_id: 11,
                    buffer_id: 111,
                },
                lifecycle: CachedWindowLifecycle::AvailableVisible {
                    last_used_epoch: FrameEpoch(1),
                },
                placement: Some(placement),
            },
            CachedRenderWindow {
                handles: WindowBufferHandle {
                    window_id: 12,
                    buffer_id: 112,
                },
                lifecycle: CachedWindowLifecycle::AvailableVisible {
                    last_used_epoch: FrameEpoch(1),
                },
                placement: Some(placement),
            },
            CachedRenderWindow {
                handles: WindowBufferHandle {
                    window_id: 13,
                    buffer_id: 113,
                },
                lifecycle: CachedWindowLifecycle::AvailableVisible {
                    last_used_epoch: FrameEpoch(1),
                },
                placement: Some(placement),
            },
            CachedRenderWindow {
                handles: WindowBufferHandle {
                    window_id: 14,
                    buffer_id: 114,
                },
                lifecycle: CachedWindowLifecycle::AvailableHidden {
                    last_used_epoch: FrameEpoch(1),
                },
                placement: None,
            },
        ],
        ..TabWindows::default()
    };
    rebuild_available_window_placement_index(&mut tab_windows);
    let mut tabs = tabs_with(tab_windows);

    let acquired_window_ids: Vec<i32> = (0..3)
        .map(|_| {
            acquire(&mut tabs, 1, 1, placement, AllocationPolicy::ReuseOnly)
                .expect("prepared pool must satisfy expected reuse demand")
                .window_id
        })
        .collect();

    assert_eq!(acquired_window_ids, vec![13, 12, 11]);
}
