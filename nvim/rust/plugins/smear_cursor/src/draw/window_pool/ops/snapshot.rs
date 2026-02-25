pub(crate) fn last_draw_signature(tabs: &HashMap<i32, TabWindows>, tab_handle: i32) -> Option<u64> {
    tabs.get(&tab_handle)
        .and_then(|tab| tab.last_draw_signature)
}

pub(crate) fn set_last_draw_signature(
    tabs: &mut HashMap<i32, TabWindows>,
    tab_handle: i32,
    signature: Option<u64>,
) {
    if let Some(tab_windows) = tabs.get_mut(&tab_handle) {
        tab_windows.last_draw_signature = signature;
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct TabPoolSnapshot {
    pub(crate) total_windows: usize,
    pub(crate) available_windows: usize,
    pub(crate) in_use_windows: usize,
    pub(crate) cached_budget: usize,
    pub(crate) last_frame_demand: usize,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct GlobalPoolSnapshot {
    pub(crate) total_windows: usize,
    pub(crate) available_windows: usize,
    pub(crate) in_use_windows: usize,
    pub(crate) cached_budget: usize,
    pub(crate) recent_frame_demand: usize,
}

fn tab_pool_snapshot_from_tab(tab_windows: &TabWindows) -> TabPoolSnapshot {
    let mut available_windows = 0_usize;
    let mut in_use_windows = 0_usize;
    for cached in &tab_windows.windows {
        match cached.lifecycle {
            CachedWindowLifecycle::AvailableVisible { .. }
            | CachedWindowLifecycle::AvailableHidden { .. } => {
                available_windows = available_windows.saturating_add(1);
            }
            CachedWindowLifecycle::InUse { .. } => {
                in_use_windows = in_use_windows.saturating_add(1);
            }
            CachedWindowLifecycle::Invalid => {}
        }
    }

    TabPoolSnapshot {
        total_windows: tab_windows.windows.len(),
        available_windows,
        in_use_windows,
        cached_budget: tab_windows.cached_budget,
        last_frame_demand: tab_windows.last_frame_demand,
    }
}

pub(crate) fn tab_pool_snapshot(
    tabs: &HashMap<i32, TabWindows>,
    tab_handle: i32,
) -> Option<TabPoolSnapshot> {
    tabs.get(&tab_handle).map(tab_pool_snapshot_from_tab)
}

pub(crate) fn global_pool_snapshot(tabs: &HashMap<i32, TabWindows>) -> GlobalPoolSnapshot {
    let mut snapshot = GlobalPoolSnapshot::default();
    for tab_windows in tabs.values() {
        let tab_snapshot = tab_pool_snapshot_from_tab(tab_windows);
        snapshot.total_windows = snapshot
            .total_windows
            .saturating_add(tab_snapshot.total_windows);
        snapshot.available_windows = snapshot
            .available_windows
            .saturating_add(tab_snapshot.available_windows);
        snapshot.in_use_windows = snapshot
            .in_use_windows
            .saturating_add(tab_snapshot.in_use_windows);
        snapshot.cached_budget = snapshot
            .cached_budget
            .saturating_add(tab_snapshot.cached_budget);
        snapshot.recent_frame_demand = snapshot
            .recent_frame_demand
            .saturating_add(tab_snapshot.last_frame_demand);
    }
    snapshot
}

