use nvim_oxi::api;
use std::collections::HashMap;
use thiserror::Error;

pub(crate) const ADAPTIVE_POOL_MIN_BUDGET: usize = 32;
pub(crate) const ADAPTIVE_POOL_HARD_MAX_BUDGET: usize = 256;
const ADAPTIVE_POOL_BUDGET_MARGIN: usize = 8;
const ADAPTIVE_POOL_EWMA_SCALE: u64 = 1000;
const ADAPTIVE_POOL_EWMA_PREV_WEIGHT: u64 = 7;
const ADAPTIVE_POOL_EWMA_NEW_WEIGHT: u64 = 3;

const RENDER_BUFFER_FILETYPE: &str = "smear-cursor";
const RENDER_BUFFER_TYPE: &str = "nofile";

#[derive(Clone, Copy, Debug)]
pub(crate) struct WindowBufferHandle {
    pub(crate) window_id: i32,
    pub(crate) buffer_id: i32,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct WindowPlacement {
    pub(crate) row: i64,
    pub(crate) col: i64,
    pub(crate) width: u32,
    pub(crate) zindex: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CachedWindowPayload {
    hash: u64,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct FrameEpoch(u64);

impl FrameEpoch {
    const ZERO: Self = Self(0);

    fn next(self) -> Self {
        Self(self.0.wrapping_add(1))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CachedWindowLifecycle {
    AvailableVisible { last_used_epoch: FrameEpoch },
    AvailableHidden { last_used_epoch: FrameEpoch },
    InUse { epoch: FrameEpoch },
    Invalid,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct WindowLifecycleCounters {
    total: usize,
    available: usize,
    reusable: usize,
    in_use: usize,
    visible: usize,
    invalid: usize,
}

impl WindowLifecycleCounters {
    fn single(lifecycle: CachedWindowLifecycle) -> Self {
        match lifecycle {
            CachedWindowLifecycle::AvailableVisible { .. } => Self {
                total: 1,
                available: 1,
                reusable: 1,
                in_use: 0,
                visible: 1,
                invalid: 0,
            },
            CachedWindowLifecycle::AvailableHidden { .. } => Self {
                total: 1,
                available: 1,
                reusable: 1,
                in_use: 0,
                visible: 0,
                invalid: 0,
            },
            CachedWindowLifecycle::InUse { .. } => Self {
                total: 1,
                available: 0,
                reusable: 0,
                in_use: 1,
                visible: 1,
                invalid: 0,
            },
            CachedWindowLifecycle::Invalid => Self {
                total: 1,
                available: 0,
                reusable: 0,
                in_use: 0,
                visible: 0,
                invalid: 1,
            },
        }
    }

    fn add_assign(&mut self, counters: Self) {
        self.total = self.total.saturating_add(counters.total);
        self.available = self.available.saturating_add(counters.available);
        self.reusable = self.reusable.saturating_add(counters.reusable);
        self.in_use = self.in_use.saturating_add(counters.in_use);
        self.visible = self.visible.saturating_add(counters.visible);
        self.invalid = self.invalid.saturating_add(counters.invalid);
    }

    fn sub_assign(&mut self, counters: Self) {
        // If this ever saturates, the maintained counters drifted from lifecycle truth.
        self.total = self.total.saturating_sub(counters.total);
        self.available = self.available.saturating_sub(counters.available);
        self.reusable = self.reusable.saturating_sub(counters.reusable);
        self.in_use = self.in_use.saturating_sub(counters.in_use);
        self.visible = self.visible.saturating_sub(counters.visible);
        self.invalid = self.invalid.saturating_sub(counters.invalid);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PlacementTrackingSlot {
    placement: WindowPlacement,
    slot: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EpochRollover {
    AvailableUnchanged,
    ReleasedForReuse,
    RecoveredStaleInUse,
    InvalidUnchanged,
}

#[derive(Clone, Copy, Debug)]
struct CachedRenderWindow {
    handles: WindowBufferHandle,
    lifecycle: CachedWindowLifecycle,
    placement: Option<WindowPlacement>,
}

impl CachedRenderWindow {
    #[cfg(test)]
    fn new_in_use(
        handles: WindowBufferHandle,
        epoch: FrameEpoch,
        placement: WindowPlacement,
    ) -> Self {
        Self {
            handles,
            lifecycle: CachedWindowLifecycle::InUse { epoch },
            placement: Some(placement),
        }
    }

    fn new_available_hidden(handles: WindowBufferHandle, last_used_epoch: FrameEpoch) -> Self {
        Self {
            handles,
            lifecycle: CachedWindowLifecycle::AvailableHidden { last_used_epoch },
            placement: None,
        }
    }

    fn is_available_for_reuse(self) -> bool {
        matches!(
            self.lifecycle,
            CachedWindowLifecycle::AvailableVisible { .. }
                | CachedWindowLifecycle::AvailableHidden { .. }
        )
    }

    fn should_hide(self) -> bool {
        matches!(
            self.lifecycle,
            CachedWindowLifecycle::AvailableVisible { .. }
        )
    }

    fn is_shell_visible(self) -> bool {
        matches!(
            self.lifecycle,
            CachedWindowLifecycle::AvailableVisible { .. } | CachedWindowLifecycle::InUse { .. }
        )
    }

    fn mark_hidden(&mut self) {
        if let CachedWindowLifecycle::AvailableVisible { last_used_epoch } = self.lifecycle {
            self.lifecycle = CachedWindowLifecycle::AvailableHidden { last_used_epoch };
        }
    }

    fn needs_reconfigure(self, placement: WindowPlacement) -> bool {
        let is_visible = match self.lifecycle {
            CachedWindowLifecycle::AvailableVisible { .. } => true,
            CachedWindowLifecycle::AvailableHidden { .. } => false,
            CachedWindowLifecycle::InUse { .. } => true,
            CachedWindowLifecycle::Invalid => false,
        };
        !is_visible || self.placement != Some(placement)
    }

    fn set_placement(&mut self, placement: WindowPlacement) {
        self.placement = Some(placement);
    }

    fn mark_in_use(&mut self, epoch: FrameEpoch) -> bool {
        match self.lifecycle {
            CachedWindowLifecycle::AvailableVisible { .. }
            | CachedWindowLifecycle::AvailableHidden { .. } => {
                self.lifecycle = CachedWindowLifecycle::InUse { epoch };
                true
            }
            CachedWindowLifecycle::InUse { .. } | CachedWindowLifecycle::Invalid => false,
        }
    }

    fn mark_invalid(&mut self) {
        self.lifecycle = CachedWindowLifecycle::Invalid;
        self.placement = None;
    }

    fn rollover_to_next_epoch(&mut self, previous_epoch: FrameEpoch) -> EpochRollover {
        match self.lifecycle {
            CachedWindowLifecycle::AvailableVisible { .. }
            | CachedWindowLifecycle::AvailableHidden { .. } => EpochRollover::AvailableUnchanged,
            CachedWindowLifecycle::InUse { epoch } if epoch == previous_epoch => {
                self.lifecycle = CachedWindowLifecycle::AvailableVisible {
                    last_used_epoch: epoch,
                };
                EpochRollover::ReleasedForReuse
            }
            CachedWindowLifecycle::InUse { epoch } => {
                self.lifecycle = CachedWindowLifecycle::AvailableVisible {
                    last_used_epoch: epoch,
                };
                EpochRollover::RecoveredStaleInUse
            }
            CachedWindowLifecycle::Invalid => EpochRollover::InvalidUnchanged,
        }
    }

    fn available_epoch(self) -> Option<FrameEpoch> {
        match self.lifecycle {
            CachedWindowLifecycle::AvailableVisible { last_used_epoch }
            | CachedWindowLifecycle::AvailableHidden { last_used_epoch } => Some(last_used_epoch),
            CachedWindowLifecycle::InUse { .. } | CachedWindowLifecycle::Invalid => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AcquireKind {
    Reused,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AllocationPolicy {
    ReuseOnly,
    BootstrapIfPoolEmpty,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Error)]
pub(crate) enum AcquireError {
    #[error(
        "render window pool exhausted under {policy:?} allocation policy",
        policy = .allocation_policy
    )]
    Exhausted { allocation_policy: AllocationPolicy },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReuseFailureReason {
    MissingWindow,
    ReconfigureFailed,
    MissingBuffer,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct ReuseFailureCounters {
    pub(crate) missing_window: usize,
    pub(crate) reconfigure_failed: usize,
    pub(crate) missing_buffer: usize,
}

impl ReuseFailureCounters {
    fn record(&mut self, reason: ReuseFailureReason) {
        match reason {
            ReuseFailureReason::MissingWindow => {
                self.missing_window = self.missing_window.saturating_add(1);
            }
            ReuseFailureReason::ReconfigureFailed => {
                self.reconfigure_failed = self.reconfigure_failed.saturating_add(1);
            }
            ReuseFailureReason::MissingBuffer => {
                self.missing_buffer = self.missing_buffer.saturating_add(1);
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct ReleaseUnusedSummary {
    pub(crate) hidden_windows: usize,
    pub(crate) invalid_removed_windows: usize,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct CompactRenderWindowsSummary {
    pub(crate) target_budget: usize,
    pub(crate) total_windows_before: usize,
    pub(crate) total_windows_after: usize,
    pub(crate) closed_visible_windows: usize,
    pub(crate) pruned_windows: usize,
    pub(crate) invalid_removed_windows: usize,
    pub(crate) has_visible_windows_after: bool,
    pub(crate) has_pending_work_after: bool,
}

impl CompactRenderWindowsSummary {
    pub(crate) fn converged_to_idle(self) -> bool {
        self.total_windows_after <= self.target_budget
            && !self.has_visible_windows_after
            && !self.has_pending_work_after
    }

    pub(crate) fn had_visual_change(self) -> bool {
        self.closed_visible_windows > 0
    }
}

#[derive(Debug)]
pub(crate) struct AcquiredWindow {
    pub(crate) window_id: i32,
    pub(crate) buffer: api::Buffer,
    pub(crate) kind: AcquireKind,
    pub(crate) reuse_failures: ReuseFailureCounters,
}

#[derive(Debug)]
pub(crate) struct TabWindows {
    current_epoch: FrameEpoch,
    frame_demand: usize,
    last_frame_demand: usize,
    peak_frame_demand: usize,
    peak_requested_capacity: usize,
    peak_total_windows: usize,
    capacity_cap_hits: usize,
    in_use_indices: Vec<usize>,
    in_use_slots: Vec<Option<usize>>,
    visible_available_indices: Vec<usize>,
    visible_available_slots: Vec<Option<usize>>,
    reusable_window_indices: Vec<usize>,
    reusable_window_slots: Vec<Option<usize>>,
    windows: Vec<CachedRenderWindow>,
    payload_by_window: HashMap<i32, CachedWindowPayload>,
    available_windows_by_placement: HashMap<WindowPlacement, Vec<usize>>,
    available_window_placement_slots: Vec<Option<PlacementTrackingSlot>>,
    lifecycle_counters: WindowLifecycleCounters,
    ewma_demand_milli: u64,
    cached_budget: usize,
}

impl Default for TabWindows {
    fn default() -> Self {
        Self {
            current_epoch: FrameEpoch::ZERO,
            frame_demand: 0,
            last_frame_demand: 0,
            peak_frame_demand: 0,
            peak_requested_capacity: 0,
            peak_total_windows: 0,
            capacity_cap_hits: 0,
            in_use_indices: Vec::new(),
            in_use_slots: Vec::new(),
            visible_available_indices: Vec::new(),
            visible_available_slots: Vec::new(),
            reusable_window_indices: Vec::new(),
            reusable_window_slots: Vec::new(),
            windows: Vec::new(),
            payload_by_window: HashMap::new(),
            available_windows_by_placement: HashMap::new(),
            available_window_placement_slots: Vec::new(),
            lifecycle_counters: WindowLifecycleCounters::default(),
            ewma_demand_milli: 0,
            cached_budget: ADAPTIVE_POOL_MIN_BUDGET,
        }
    }
}

impl TabWindows {
    pub(crate) fn cached_payload_matches(&self, window_id: i32, payload_hash: u64) -> bool {
        self.payload_by_window
            .get(&window_id)
            .is_some_and(|payload| payload.hash == payload_hash)
    }

    pub(crate) fn cache_payload(&mut self, window_id: i32, payload_hash: u64) {
        if let Some(payload) = self.payload_by_window.get_mut(&window_id) {
            payload.hash = payload_hash;
            return;
        }

        self.payload_by_window
            .insert(window_id, CachedWindowPayload { hash: payload_hash });
    }

    #[cfg(test)]
    pub(crate) fn push_test_visible_window(
        &mut self,
        handles: WindowBufferHandle,
        placement: WindowPlacement,
        last_used_epoch: u64,
    ) {
        let lifecycle = CachedWindowLifecycle::AvailableVisible {
            last_used_epoch: FrameEpoch(last_used_epoch),
        };
        self.push_cached_window(CachedRenderWindow {
            handles,
            lifecycle,
            placement: Some(placement),
        });
    }

    fn clear_payload(&mut self, window_id: i32) {
        self.payload_by_window.remove(&window_id);
    }

    fn push_cached_window(&mut self, cached: CachedRenderWindow) {
        let index = self.windows.len();
        self.windows.push(cached);
        self.note_peak_total_windows();
        self.in_use_slots.push(None);
        self.visible_available_slots.push(None);
        self.reusable_window_slots.push(None);
        self.available_window_placement_slots.push(None);
        self.track_window_insert(index, cached.lifecycle, cached.placement);
        self.debug_assert_tracking_consistent();
    }

    fn track_index_insert(indices: &mut Vec<usize>, slots: &mut [Option<usize>], index: usize) {
        if slots.get(index).and_then(|slot| *slot).is_some() {
            return;
        }
        let slot = indices.len();
        indices.push(index);
        slots[index] = Some(slot);
    }

    fn track_index_remove(indices: &mut Vec<usize>, slots: &mut [Option<usize>], index: usize) {
        let Some(slot) = slots.get_mut(index).and_then(Option::take) else {
            return;
        };
        indices.swap_remove(slot);
        if slot < indices.len() {
            let moved_index = indices[slot];
            slots[moved_index] = Some(slot);
        }
    }

    fn register_in_use_index(&mut self, index: usize) {
        Self::track_index_insert(&mut self.in_use_indices, &mut self.in_use_slots, index);
    }

    fn unregister_in_use_index(&mut self, index: usize) {
        Self::track_index_remove(&mut self.in_use_indices, &mut self.in_use_slots, index);
    }

    fn register_visible_available_index(&mut self, index: usize) {
        Self::track_index_insert(
            &mut self.visible_available_indices,
            &mut self.visible_available_slots,
            index,
        );
    }

    fn unregister_visible_available_index(&mut self, index: usize) {
        Self::track_index_remove(
            &mut self.visible_available_indices,
            &mut self.visible_available_slots,
            index,
        );
    }

    fn take_visible_available_indices_for_hide(&mut self) -> Vec<usize> {
        let indices = std::mem::take(&mut self.visible_available_indices);
        self.visible_available_slots.fill(None);
        indices
    }

    fn register_reusable_index(&mut self, index: usize) {
        Self::track_index_insert(
            &mut self.reusable_window_indices,
            &mut self.reusable_window_slots,
            index,
        );
    }

    fn unregister_reusable_index(&mut self, index: usize) {
        Self::track_index_remove(
            &mut self.reusable_window_indices,
            &mut self.reusable_window_slots,
            index,
        );
    }

    fn register_available_placement_index(
        &mut self,
        index: usize,
        placement: Option<WindowPlacement>,
    ) {
        let Some(placement) = placement else {
            return;
        };
        if self
            .available_window_placement_slots
            .get(index)
            .and_then(|slot| *slot)
            .is_some()
        {
            return;
        }

        let indices = self
            .available_windows_by_placement
            .entry(placement)
            .or_default();
        let slot = indices.len();
        indices.push(index);
        self.available_window_placement_slots[index] =
            Some(PlacementTrackingSlot { placement, slot });
    }

    fn unregister_available_placement_index(&mut self, index: usize) {
        let Some(entry) = self
            .available_window_placement_slots
            .get_mut(index)
            .and_then(Option::take)
        else {
            return;
        };

        let mut remove_key = false;
        if let Some(indices) = self
            .available_windows_by_placement
            .get_mut(&entry.placement)
        {
            indices.swap_remove(entry.slot);
            if entry.slot < indices.len() {
                let moved_index = indices[entry.slot];
                if let Some(slot) = self.available_window_placement_slots[moved_index].as_mut() {
                    slot.slot = entry.slot;
                }
            }
            remove_key = indices.is_empty();
        }
        if remove_key {
            self.available_windows_by_placement.remove(&entry.placement);
        }
    }

    fn register_window_tracking(
        &mut self,
        index: usize,
        lifecycle: CachedWindowLifecycle,
        placement: Option<WindowPlacement>,
    ) {
        self.lifecycle_counters
            .add_assign(WindowLifecycleCounters::single(lifecycle));
        match lifecycle {
            CachedWindowLifecycle::AvailableVisible { .. } => {
                self.register_reusable_index(index);
                self.register_visible_available_index(index);
                self.register_available_placement_index(index, placement);
            }
            CachedWindowLifecycle::AvailableHidden { .. } => {
                self.register_reusable_index(index);
                self.register_available_placement_index(index, placement);
            }
            CachedWindowLifecycle::InUse { .. } => {
                self.register_in_use_index(index);
            }
            CachedWindowLifecycle::Invalid => {}
        }
    }

    fn unregister_window_tracking(
        &mut self,
        index: usize,
        lifecycle: CachedWindowLifecycle,
        placement: Option<WindowPlacement>,
    ) {
        match lifecycle {
            CachedWindowLifecycle::AvailableVisible { .. } => {
                self.unregister_available_placement_index(index);
                self.unregister_visible_available_index(index);
                self.unregister_reusable_index(index);
            }
            CachedWindowLifecycle::AvailableHidden { .. } => {
                self.unregister_available_placement_index(index);
                self.unregister_reusable_index(index);
            }
            CachedWindowLifecycle::InUse { .. } => {
                self.unregister_in_use_index(index);
            }
            CachedWindowLifecycle::Invalid => {
                let _ = placement;
            }
        }
        self.lifecycle_counters
            .sub_assign(WindowLifecycleCounters::single(lifecycle));
    }

    fn track_window_insert(
        &mut self,
        index: usize,
        lifecycle: CachedWindowLifecycle,
        placement: Option<WindowPlacement>,
    ) {
        self.register_window_tracking(index, lifecycle, placement);
    }

    fn track_window_remove(
        &mut self,
        index: usize,
        lifecycle: CachedWindowLifecycle,
        placement: Option<WindowPlacement>,
    ) {
        self.unregister_window_tracking(index, lifecycle, placement);
    }

    fn track_window_transition(
        &mut self,
        index: usize,
        before_lifecycle: CachedWindowLifecycle,
        before_placement: Option<WindowPlacement>,
        after_lifecycle: CachedWindowLifecycle,
        after_placement: Option<WindowPlacement>,
    ) {
        if before_lifecycle == after_lifecycle && before_placement == after_placement {
            return;
        }
        self.unregister_window_tracking(index, before_lifecycle, before_placement);
        self.register_window_tracking(index, after_lifecycle, after_placement);
    }

    fn retarget_window_index_after_swap_remove(&mut self, from: usize, to: usize) {
        // The `windows` vector is the source of truth, but these side indexes are all keyed by
        // vector position. Retarget the moved tail entry before `swap_remove` mutates the slots.
        if let Some(slot) = self.in_use_slots.get(from).and_then(|slot| *slot) {
            self.in_use_indices[slot] = to;
        }
        if let Some(slot) = self
            .visible_available_slots
            .get(from)
            .and_then(|slot| *slot)
        {
            self.visible_available_indices[slot] = to;
        }
        if let Some(slot) = self.reusable_window_slots.get(from).and_then(|slot| *slot) {
            self.reusable_window_indices[slot] = to;
        }
        if let Some(slot) = self
            .available_window_placement_slots
            .get(from)
            .and_then(|slot| *slot)
            && let Some(indices) = self.available_windows_by_placement.get_mut(&slot.placement)
        {
            indices[slot.slot] = to;
        }
    }

    fn swap_remove_window(&mut self, index: usize) -> Option<CachedRenderWindow> {
        let removed = self.windows.get(index).copied()?;
        let last_index = self.windows.len().saturating_sub(1);
        self.track_window_remove(index, removed.lifecycle, removed.placement);
        self.clear_payload(removed.handles.window_id);

        if index != last_index {
            self.retarget_window_index_after_swap_remove(last_index, index);
        }

        let removed = self.windows.swap_remove(index);
        self.in_use_slots.swap_remove(index);
        self.visible_available_slots.swap_remove(index);
        self.reusable_window_slots.swap_remove(index);
        self.available_window_placement_slots.swap_remove(index);
        self.debug_assert_tracking_consistent();
        Some(removed)
    }

    #[cfg(test)]
    fn seed_tracking_from_windows_for_test(&mut self) {
        // Test fixtures can start from hand-built `windows` vectors, but the live pool must never
        // fall back to scan-and-rebuild bookkeeping after the exact-tracking rewrite.
        self.in_use_indices.clear();
        self.in_use_slots = vec![None; self.windows.len()];
        self.visible_available_indices.clear();
        self.visible_available_slots = vec![None; self.windows.len()];
        self.reusable_window_indices.clear();
        self.reusable_window_slots = vec![None; self.windows.len()];
        self.available_windows_by_placement.clear();
        self.available_window_placement_slots = vec![None; self.windows.len()];
        self.lifecycle_counters = WindowLifecycleCounters::default();

        for index in 0..self.windows.len() {
            let cached = self.windows[index];
            self.track_window_insert(index, cached.lifecycle, cached.placement);
        }
        self.note_peak_total_windows();
        self.debug_assert_tracking_consistent();
    }

    #[cfg(any(test, debug_assertions))]
    fn tracking_snapshot_from_windows(&self) -> WindowTrackingSnapshot {
        let mut snapshot = WindowTrackingSnapshot {
            lifecycle_counters: WindowLifecycleCounters::default(),
            ..WindowTrackingSnapshot::default()
        };

        for (index, cached) in self.windows.iter().copied().enumerate() {
            snapshot
                .lifecycle_counters
                .add_assign(WindowLifecycleCounters::single(cached.lifecycle));
            match cached.lifecycle {
                CachedWindowLifecycle::AvailableVisible { .. } => {
                    snapshot.visible_available_indices.push(index);
                    snapshot.reusable_window_indices.push(index);
                    if let Some(placement) = cached.placement {
                        snapshot
                            .available_windows_by_placement
                            .entry(placement)
                            .or_default()
                            .push(index);
                    }
                }
                CachedWindowLifecycle::AvailableHidden { .. } => {
                    snapshot.reusable_window_indices.push(index);
                    if let Some(placement) = cached.placement {
                        snapshot
                            .available_windows_by_placement
                            .entry(placement)
                            .or_default()
                            .push(index);
                    }
                }
                CachedWindowLifecycle::InUse { .. } => {
                    snapshot.in_use_indices.push(index);
                }
                CachedWindowLifecycle::Invalid => {}
            }
        }

        snapshot.normalize();
        snapshot
    }

    #[cfg(any(test, debug_assertions))]
    fn tracking_snapshot_from_bookkeeping(&self) -> WindowTrackingSnapshot {
        let mut snapshot = WindowTrackingSnapshot {
            lifecycle_counters: self.lifecycle_counters,
            in_use_indices: self.in_use_indices.clone(),
            visible_available_indices: self.visible_available_indices.clone(),
            reusable_window_indices: self.reusable_window_indices.clone(),
            available_windows_by_placement: self.available_windows_by_placement.clone(),
        };
        snapshot.normalize();
        snapshot
    }

    fn debug_assert_tracking_consistent(&self) {
        #[cfg(any(test, debug_assertions))]
        {
            debug_assert_eq!(
                self.tracking_snapshot_from_bookkeeping(),
                self.tracking_snapshot_from_windows(),
                "render window bookkeeping drifted from lifecycle truth"
            );
        }
    }

    #[cfg(test)]
    fn assert_tracking_consistent(&self) {
        assert_eq!(
            self.tracking_snapshot_from_bookkeeping(),
            self.tracking_snapshot_from_windows(),
            "render window bookkeeping drifted from lifecycle truth"
        );
    }

    fn reusable_window_index(&self) -> Option<usize> {
        self.reusable_window_indices.last().copied()
    }

    fn placement_window_index(&self, placement: WindowPlacement) -> Option<usize> {
        self.available_windows_by_placement
            .get(&placement)
            .and_then(|indices| indices.last().copied())
    }

    fn available_window_count(&self) -> usize {
        self.lifecycle_counters.available
    }

    fn in_use_window_count(&self) -> usize {
        self.lifecycle_counters.in_use
    }

    fn visible_window_count(&self) -> usize {
        self.lifecycle_counters.visible
    }

    fn usable_window_count(&self) -> usize {
        self.lifecycle_counters
            .reusable
            .saturating_add(self.lifecycle_counters.in_use)
    }

    fn has_invalid_windows(&self) -> bool {
        self.lifecycle_counters.invalid > 0
    }

    fn note_frame_demand(&mut self, demand_signal: usize) {
        self.peak_frame_demand = self.peak_frame_demand.max(demand_signal);
    }

    fn note_capacity_target(&mut self, target: FrameCapacityTarget) {
        self.peak_requested_capacity = self.peak_requested_capacity.max(target.requested_capacity);
        if target.is_clamped_by_cap() {
            self.capacity_cap_hits = self.capacity_cap_hits.saturating_add(1);
        }
    }

    fn note_peak_total_windows(&mut self) {
        self.peak_total_windows = self.peak_total_windows.max(self.windows.len());
    }
}

#[cfg(any(test, debug_assertions))]
#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct WindowTrackingSnapshot {
    lifecycle_counters: WindowLifecycleCounters,
    in_use_indices: Vec<usize>,
    visible_available_indices: Vec<usize>,
    reusable_window_indices: Vec<usize>,
    available_windows_by_placement: HashMap<WindowPlacement, Vec<usize>>,
}

#[cfg(any(test, debug_assertions))]
impl WindowTrackingSnapshot {
    fn normalize(&mut self) {
        self.in_use_indices.sort_unstable();
        self.visible_available_indices.sort_unstable();
        self.reusable_window_indices.sort_unstable();
        for indices in self.available_windows_by_placement.values_mut() {
            indices.sort_unstable();
        }
    }
}

mod ops;
pub(crate) use ops::*;
