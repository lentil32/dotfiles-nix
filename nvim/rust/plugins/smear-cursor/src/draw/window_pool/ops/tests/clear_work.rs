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
        buffer_id: 101_i32.saturating_add(offset),
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

        prop_assert_eq!(has_visible_windows(&tabs), expected_has_visible);
        prop_assert_eq!(has_pending_clear_work(&tabs, max_kept_windows), expected_pending);
    }

    #[test]
    fn prop_shell_visible_close_indices_follow_authoritative_lifecycles(
        lifecycles in vec(window_lifecycle_spec(), 0..16),
    ) {
        let tab_windows = tab_windows_from_specs(&lifecycles, ADAPTIVE_POOL_MIN_BUDGET);
        let expected_indices = lifecycles
            .iter()
            .copied()
            .enumerate()
            .filter_map(|(index, lifecycle)| lifecycle.is_shell_visible().then_some(index))
            .collect::<Vec<_>>();

        prop_assert_eq!(shell_visible_close_indices(&tab_windows), expected_indices);
    }
}
