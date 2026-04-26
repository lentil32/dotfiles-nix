#[derive(Clone, Copy, Debug)]
enum WindowLifecycleSpec {
    AvailableVisible { last_used_epoch: u8 },
    AvailableHidden { last_used_epoch: u8 },
    InUse { epoch: u8 },
    Invalid,
}

impl WindowLifecycleSpec {
    fn is_shell_visible(self) -> bool {
        matches!(self, Self::AvailableVisible { .. } | Self::InUse { .. })
    }

    fn is_invalid(self) -> bool {
        matches!(self, Self::Invalid)
    }
}

fn window_lifecycle_spec() -> BoxedStrategy<WindowLifecycleSpec> {
    prop_oneof![
        any::<u8>()
            .prop_map(|last_used_epoch| WindowLifecycleSpec::AvailableVisible { last_used_epoch }),
        any::<u8>()
            .prop_map(|last_used_epoch| WindowLifecycleSpec::AvailableHidden { last_used_epoch }),
        any::<u8>().prop_map(|epoch| WindowLifecycleSpec::InUse { epoch }),
        Just(WindowLifecycleSpec::Invalid),
    ]
    .boxed()
}

fn test_placement(index: usize) -> WindowPlacement {
    let index = i64::try_from(index).unwrap_or(i64::MAX);
    WindowPlacement {
        row: index,
        col: index.saturating_mul(2),
        width: 1,
        zindex: 80,
    }
}

fn cached_window(index: usize, lifecycle: WindowLifecycleSpec) -> CachedRenderWindow {
    let offset = i32::try_from(index).unwrap_or(i32::MAX);
    let handles = WindowBufferHandle {
        window_id: 1_i32.saturating_add(offset),
        buffer_id: BufferHandle::from(101_i32.saturating_add(offset)),
    };

    match lifecycle {
        WindowLifecycleSpec::AvailableVisible { last_used_epoch } => CachedRenderWindow {
            handles,
            lifecycle: CachedWindowLifecycle::AvailableVisible {
                last_used_epoch: FrameEpoch(u64::from(last_used_epoch)),
            },
            placement: Some(test_placement(index)),
        },
        WindowLifecycleSpec::AvailableHidden { last_used_epoch } => CachedRenderWindow {
            handles,
            lifecycle: CachedWindowLifecycle::AvailableHidden {
                last_used_epoch: FrameEpoch(u64::from(last_used_epoch)),
            },
            placement: Some(test_placement(index)),
        },
        WindowLifecycleSpec::InUse { epoch } => CachedRenderWindow {
            handles,
            lifecycle: CachedWindowLifecycle::InUse {
                epoch: FrameEpoch(u64::from(epoch)),
            },
            placement: Some(test_placement(index)),
        },
        WindowLifecycleSpec::Invalid => CachedRenderWindow {
            handles,
            lifecycle: CachedWindowLifecycle::Invalid,
            placement: None,
        },
    }
}

fn tab_windows_from_specs(specs: &[WindowLifecycleSpec], cached_budget: usize) -> TabWindows {
    let windows = specs
        .iter()
        .copied()
        .enumerate()
        .map(|(index, lifecycle)| cached_window(index, lifecycle))
        .collect();
    TabWindows {
        windows,
        cached_budget,
        ..TabWindows::default()
    }
}

#[test]
fn purge_tab_retains_failed_window_close_as_invalid_retry_state() {
    let retained_handles = WindowBufferHandle {
        window_id: 11,
        buffer_id: BufferHandle::from_raw_for_test(/*value*/ 111),
    };
    let released_handles = WindowBufferHandle {
        window_id: 12,
        buffer_id: BufferHandle::from_raw_for_test(/*value*/ 112),
    };
    let mut tab_windows = TabWindows::default();
    tab_windows.push_test_visible_window(retained_handles, test_placement(0), 1);
    tab_windows.push_test_visible_window(released_handles, test_placement(1), 2);
    tab_windows.cache_payload(retained_handles.window_id, 111);
    tab_windows.cache_payload(released_handles.window_id, 222);

    let mut close_cached_window = |_: NamespaceId, handles: WindowBufferHandle| {
        if handles == retained_handles {
            TrackedResourceCloseOutcome::Retained
        } else {
            TrackedResourceCloseOutcome::ClosedOrGone
        }
    };

    let summary = purge_tab_with_closer(
        &mut tab_windows,
        NamespaceId::new(/*value*/ 99),
        &mut close_cached_window,
    );

    assert_eq!(
        summary,
        TrackedResourceCloseSummary {
            closed_or_gone: 1,
            retained: 1,
        }
    );
    assert_eq!(
        tab_pool_snapshot_from_tab(&tab_windows),
        TabPoolSnapshot {
            total_windows: 1,
            available_windows: 0,
            in_use_windows: 0,
            cached_budget: ADAPTIVE_POOL_MIN_BUDGET,
            last_frame_demand: 0,
            peak_total_windows: 1,
            peak_frame_demand: 0,
            peak_requested_capacity: 0,
            capacity_cap_hits: 0,
        }
    );
    assert!(!tab_windows.cached_payload_matches(retained_handles.window_id, 111));
    assert!(!tab_windows.cached_payload_matches(released_handles.window_id, 222));
    tab_windows.assert_tracking_consistent();
}

proptest! {
    #![proptest_config(pure_config())]

    #[test]
    fn prop_clear_work_detection_matches_visibility_budget_and_invalidity(
        lifecycles in vec(window_lifecycle_spec(), 0..16),
        cached_budget in 0_usize..=64,
        max_kept_windows in 0_usize..=64,
    ) {
        let tabs = tabs_with(tab_windows_from_specs(&lifecycles, cached_budget));
        let expected_has_visible = lifecycles
            .iter()
            .copied()
            .any(WindowLifecycleSpec::is_shell_visible);
        let expected_pending = expected_has_visible
            || lifecycles.len() > cached_budget.min(max_kept_windows)
            || lifecycles.iter().copied().any(WindowLifecycleSpec::is_invalid);

        prop_assert_eq!(has_pending_clear_work(&tabs, max_kept_windows), expected_pending);
    }
}
