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

#[cfg(test)]
pub(crate) fn tab_pool_snapshot(
    tabs: &HashMap<i32, TabWindows>,
    tab_handle: i32,
) -> Option<TabPoolSnapshot> {
    tabs.get(&tab_handle).map(tab_pool_snapshot_from_tab)
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
        CachedRenderWindow, CachedWindowLifecycle, FrameEpoch, TabWindows, WindowBufferHandle,
        WindowPlacement, tab_pool_snapshot,
    };
    use std::collections::HashMap;

    #[test]
    fn tab_pool_snapshot_reads_authoritative_window_lifecycles() {
        let placement = Some(WindowPlacement {
            row: 2,
            col: 4,
            width: 1,
            zindex: 40,
        });
        let mut tab_windows = TabWindows {
            windows: vec![
                CachedRenderWindow {
                    handles: WindowBufferHandle {
                        window_id: 11,
                        buffer_id: 21,
                    },
                    lifecycle: CachedWindowLifecycle::AvailableHidden {
                        last_used_epoch: FrameEpoch(1),
                    },
                    placement,
                },
                CachedRenderWindow {
                    handles: WindowBufferHandle {
                        window_id: 12,
                        buffer_id: 22,
                    },
                    lifecycle: CachedWindowLifecycle::AvailableVisible {
                        last_used_epoch: FrameEpoch(2),
                    },
                    placement,
                },
                CachedRenderWindow {
                    handles: WindowBufferHandle {
                        window_id: 13,
                        buffer_id: 23,
                    },
                    lifecycle: CachedWindowLifecycle::InUse {
                        epoch: FrameEpoch(3),
                    },
                    placement,
                },
            ],
            ..TabWindows::default()
        };
        tab_windows.seed_tracking_from_windows_for_test();
        let tabs = HashMap::from([(9_i32, tab_windows)]);

        let snapshot = tab_pool_snapshot(&tabs, 9).expect("tab snapshot should exist");

        assert_eq!(snapshot.total_windows, 3);
        assert_eq!(snapshot.available_windows, 2);
        assert_eq!(snapshot.in_use_windows, 1);
        assert_eq!(snapshot.peak_total_windows, 3);
    }

    #[test]
    fn tab_pool_snapshot_reads_maintained_counters_and_peak_telemetry() {
        let placement = Some(WindowPlacement {
            row: 2,
            col: 4,
            width: 1,
            zindex: 40,
        });
        let mut tab_windows = TabWindows {
            windows: vec![CachedRenderWindow {
                handles: WindowBufferHandle {
                    window_id: 12,
                    buffer_id: 22,
                },
                lifecycle: CachedWindowLifecycle::InUse {
                    epoch: FrameEpoch(2),
                },
                placement,
            }],
            last_frame_demand: 13,
            peak_frame_demand: 21,
            peak_requested_capacity: 34,
            peak_total_windows: 55,
            capacity_cap_hits: 2,
            ..TabWindows::default()
        };
        tab_windows.seed_tracking_from_windows_for_test();
        let tabs = HashMap::from([(9_i32, tab_windows)]);

        let snapshot = tab_pool_snapshot(&tabs, 9).expect("tab snapshot should exist");

        assert_eq!(snapshot.total_windows, 1);
        assert_eq!(snapshot.available_windows, 0);
        assert_eq!(snapshot.in_use_windows, 1);
        assert_eq!(snapshot.last_frame_demand, 13);
        assert_eq!(snapshot.peak_frame_demand, 21);
        assert_eq!(snapshot.peak_requested_capacity, 34);
        assert_eq!(snapshot.peak_total_windows, 55);
        assert_eq!(snapshot.capacity_cap_hits, 2);
    }
}
