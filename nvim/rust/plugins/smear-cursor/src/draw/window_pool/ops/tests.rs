#[cfg(test)]
mod tests {
    use super::{
        ADAPTIVE_POOL_HARD_MAX_BUDGET, ADAPTIVE_POOL_MIN_BUDGET, AcquireError, AdaptiveBudgetState,
        AllocationPolicy, CachedRenderWindow, CachedWindowLifecycle, CompactRenderWindowsSummary,
        FrameCapacityTarget, FrameEpoch, ReuseFailureCounters, TabPoolSnapshot, TabWindows,
        WindowBufferHandle, WindowPlacement, acquire, available_window_index_for_placement,
        begin_tab_frame, effective_keep_budget, frame_capacity_target,
        global_compaction_prune_plan, has_pending_clear_work, has_visible_windows,
        lru_prune_indices, next_adaptive_budget, purge_tab_with_closer, remove_window_in_tab,
        rollover_in_use_windows, shell_visible_close_indices, tab_pool_snapshot_from_tab,
    };
    use crate::host::BufferHandle;
    use crate::host::NamespaceId;
    use crate::host::TabHandle;
    use crate::draw::TrackedResourceCloseOutcome;
    use crate::draw::TrackedResourceCloseSummary;
    use crate::test_support::proptest::pure_config;
    use pretty_assertions::assert_eq;
    use proptest::collection::vec;
    use proptest::prelude::*;
    use std::collections::HashMap;

    fn tab_handle(value: i32) -> TabHandle {
        TabHandle::from_raw_for_test(value)
    }

    fn tabs_with(mut tab_windows: TabWindows) -> HashMap<TabHandle, TabWindows> {
        tab_windows.seed_tracking_from_windows_for_test();
        HashMap::from([(tab_handle(1), tab_windows)])
    }

    fn cached(window_id: i32, buffer_id: i32, last_used_epoch: u64) -> CachedRenderWindow {
        CachedRenderWindow {
            handles: WindowBufferHandle {
                window_id,
                buffer_id: BufferHandle::from(buffer_id),
            },
            lifecycle: CachedWindowLifecycle::AvailableHidden {
                last_used_epoch: FrameEpoch(last_used_epoch),
            },
            placement: Some(WindowPlacement {
                row: 0,
                col: 0,
                width: 1,
                zindex: 50,
            }),
        }
    }

    include!("tests/adaptive.rs");
    include!("tests/clear_work.rs");
    include!("tests/compaction.rs");
    include!("tests/reuse.rs");
}
