#[cfg(test)]
mod tests {
    use super::{
        ADAPTIVE_POOL_HARD_MAX_BUDGET, ADAPTIVE_POOL_MIN_BUDGET, AcquireError, AdaptiveBudgetState,
        AllocationPolicy, CachedRenderWindow, CachedWindowLifecycle, FrameCapacityTarget,
        FrameEpoch, ReuseFailureCounters, TabPoolSnapshot, TabWindows, WindowBufferHandle,
        WindowPlacement, acquire, available_window_index_for_placement, begin_tab_frame,
        frame_capacity_target, global_compaction_prune_plan, has_pending_clear_work,
        lru_prune_indices, next_adaptive_budget, purge_tab_with_closer, rollover_in_use_windows,
        tab_pool_snapshot_from_tab,
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

    include!("tests/adaptive.rs");
    include!("tests/clear_work.rs");
    include!("tests/compaction.rs");
    include!("tests/reuse.rs");
}
