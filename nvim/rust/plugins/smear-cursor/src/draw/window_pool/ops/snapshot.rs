#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct TabPoolSnapshot {
    pub(crate) total_windows: usize,
    pub(crate) available_windows: usize,
    pub(crate) in_use_windows: usize,
    pub(crate) cached_budget: usize,
    pub(crate) last_frame_demand: usize,
    pub(crate) peak_total_windows: usize,
    pub(crate) peak_frame_demand: usize,
    pub(crate) peak_requested_capacity: usize,
    pub(crate) capacity_cap_hits: usize,
}

pub(crate) fn tab_pool_snapshot_from_tab(tab_windows: &TabWindows) -> TabPoolSnapshot {
    TabPoolSnapshot {
        total_windows: tab_windows.windows.len(),
        available_windows: tab_windows.available_window_count(),
        in_use_windows: tab_windows.in_use_window_count(),
        cached_budget: tab_windows.cached_budget,
        last_frame_demand: tab_windows.last_frame_demand,
        peak_total_windows: tab_windows.peak_total_windows,
        peak_frame_demand: tab_windows.peak_frame_demand,
        peak_requested_capacity: tab_windows.peak_requested_capacity,
        capacity_cap_hits: tab_windows.capacity_cap_hits,
    }
}

pub(crate) fn tab_in_use_window_count_from_tab(tab_windows: &TabWindows) -> usize {
    tab_windows.in_use_window_count()
}

pub(crate) fn tab_visible_window_count_from_tab(tab_windows: &TabWindows) -> usize {
    tab_windows.visible_window_count()
}

#[cfg(test)]
mod snapshot_tests {
    use super::{
        CachedRenderWindow, CachedWindowLifecycle, FrameEpoch, TabPoolSnapshot, TabWindows,
        WindowBufferHandle, WindowPlacement, tab_pool_snapshot_from_tab,
    };
    use crate::host::BufferHandle;
    use crate::test_support::proptest::pure_config;
    use proptest::collection::vec;
    use proptest::prelude::*;

    #[derive(Clone, Copy, Debug)]
    enum WindowLifecycleSpec {
        AvailableVisible { last_used_epoch: u8 },
        AvailableHidden { last_used_epoch: u8 },
        InUse { epoch: u8 },
        Invalid,
    }

    impl WindowLifecycleSpec {
        fn is_available(self) -> bool {
            matches!(
                self,
                Self::AvailableVisible { .. } | Self::AvailableHidden { .. }
            )
        }

        fn is_in_use(self) -> bool {
            matches!(self, Self::InUse { .. })
        }
    }

    fn window_lifecycle_spec() -> BoxedStrategy<WindowLifecycleSpec> {
        prop_oneof![
            any::<u8>().prop_map(|last_used_epoch| WindowLifecycleSpec::AvailableVisible {
                last_used_epoch,
            }),
            any::<u8>().prop_map(|last_used_epoch| WindowLifecycleSpec::AvailableHidden {
                last_used_epoch,
            }),
            any::<u8>().prop_map(|epoch| WindowLifecycleSpec::InUse { epoch }),
            Just(WindowLifecycleSpec::Invalid),
        ]
        .boxed()
    }

    fn placement(index: usize) -> WindowPlacement {
        let index = i64::try_from(index).unwrap_or(i64::MAX);
        WindowPlacement {
            row: index,
            col: index.saturating_mul(3),
            width: 1,
            zindex: 40,
        }
    }

    fn cached_window(index: usize, lifecycle: WindowLifecycleSpec) -> CachedRenderWindow {
        let offset = i32::try_from(index).unwrap_or(i32::MAX);
        let handles = WindowBufferHandle {
            window_id: 11_i32.saturating_add(offset),
            buffer_id: BufferHandle::from(21_i32.saturating_add(offset)),
        };

        match lifecycle {
            WindowLifecycleSpec::AvailableVisible { last_used_epoch } => CachedRenderWindow {
                handles,
                lifecycle: CachedWindowLifecycle::AvailableVisible {
                    last_used_epoch: FrameEpoch(u64::from(last_used_epoch)),
                },
                placement: Some(placement(index)),
            },
            WindowLifecycleSpec::AvailableHidden { last_used_epoch } => CachedRenderWindow {
                handles,
                lifecycle: CachedWindowLifecycle::AvailableHidden {
                    last_used_epoch: FrameEpoch(u64::from(last_used_epoch)),
                },
                placement: Some(placement(index)),
            },
            WindowLifecycleSpec::InUse { epoch } => CachedRenderWindow {
                handles,
                lifecycle: CachedWindowLifecycle::InUse {
                    epoch: FrameEpoch(u64::from(epoch)),
                },
                placement: Some(placement(index)),
            },
            WindowLifecycleSpec::Invalid => CachedRenderWindow {
                handles,
                lifecycle: CachedWindowLifecycle::Invalid,
                placement: None,
            },
        }
    }

    fn expected_snapshot(
        lifecycles: &[WindowLifecycleSpec],
        cached_budget: usize,
        last_frame_demand: usize,
        peak_frame_demand: usize,
        peak_requested_capacity: usize,
        peak_total_windows: usize,
        capacity_cap_hits: usize,
    ) -> TabPoolSnapshot {
        TabPoolSnapshot {
            total_windows: lifecycles.len(),
            available_windows: lifecycles
                .iter()
                .copied()
                .filter(|lifecycle| lifecycle.is_available())
                .count(),
            in_use_windows: lifecycles
                .iter()
                .copied()
                .filter(|lifecycle| lifecycle.is_in_use())
                .count(),
            cached_budget,
            last_frame_demand,
            peak_total_windows: peak_total_windows.max(lifecycles.len()),
            peak_frame_demand,
            peak_requested_capacity,
            capacity_cap_hits,
        }
    }

    fn build_tab_windows(
        lifecycles: &[WindowLifecycleSpec],
        cached_budget: usize,
        last_frame_demand: usize,
        peak_frame_demand: usize,
        peak_requested_capacity: usize,
        peak_total_windows: usize,
        capacity_cap_hits: usize,
    ) -> TabWindows {
        let windows = lifecycles
            .iter()
            .copied()
            .enumerate()
            .map(|(index, lifecycle)| cached_window(index, lifecycle))
            .collect();
        let mut tab_windows = TabWindows {
            windows,
            cached_budget,
            last_frame_demand,
            peak_frame_demand,
            peak_requested_capacity,
            peak_total_windows,
            capacity_cap_hits,
            ..TabWindows::default()
        };
        tab_windows.seed_tracking_from_windows_for_test();
        tab_windows
    }

    proptest! {
        #![proptest_config(pure_config())]

        #[test]
        fn prop_tab_pool_snapshot_matches_authoritative_lifecycles_and_counters(
            lifecycles in vec(window_lifecycle_spec(), 0..16),
            cached_budget in 0_usize..=64,
            last_frame_demand in 0_usize..=64,
            peak_frame_demand in 0_usize..=128,
            peak_requested_capacity in 0_usize..=128,
            peak_total_windows in 0_usize..=128,
            capacity_cap_hits in 0_usize..=32,
        ) {
            let expected = expected_snapshot(
                &lifecycles,
                cached_budget,
                last_frame_demand,
                peak_frame_demand,
                peak_requested_capacity,
                peak_total_windows,
                capacity_cap_hits,
            );
            let tab_windows = build_tab_windows(
                &lifecycles,
                cached_budget,
                last_frame_demand,
                peak_frame_demand,
                peak_requested_capacity,
                peak_total_windows,
                capacity_cap_hits,
            );

            prop_assert_eq!(tab_pool_snapshot_from_tab(&tab_windows), expected);

        }
    }
}
