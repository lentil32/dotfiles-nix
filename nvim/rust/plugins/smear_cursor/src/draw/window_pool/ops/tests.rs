#[cfg(test)]
mod tests {
    use super::{
        ADAPTIVE_POOL_HARD_MAX_BUDGET, ADAPTIVE_POOL_MIN_BUDGET, AdaptiveBudgetState,
        CachedRenderWindow, CachedWindowLifecycle, EpochRollover, FrameEpoch, TabWindows,
        WindowBufferHandle, WindowPlacement, adjust_tracking_after_remove,
        available_window_index_for_placement, begin_tab_frame, effective_keep_budget,
        has_pending_clear_work, has_visible_windows, lru_prune_indices, next_adaptive_budget,
        rebuild_available_window_placement_index, rollover_in_use_windows,
    };
    use std::collections::HashMap;

    fn tabs_with(tab_windows: TabWindows) -> HashMap<i32, TabWindows> {
        HashMap::from([(1_i32, tab_windows)])
    }

    fn cached(window_id: i32, buffer_id: i32, last_used_epoch: u64) -> CachedRenderWindow {
        CachedRenderWindow {
            handles: WindowBufferHandle {
                window_id,
                buffer_id,
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
    include!("tests/reuse.rs");
}
