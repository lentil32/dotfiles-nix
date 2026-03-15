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
    available: usize,
    in_use: usize,
    visible: usize,
}

impl WindowLifecycleCounters {
    fn single(lifecycle: CachedWindowLifecycle) -> Self {
        match lifecycle {
            CachedWindowLifecycle::AvailableVisible { .. } => Self {
                available: 1,
                in_use: 0,
                visible: 1,
            },
            CachedWindowLifecycle::AvailableHidden { .. } => Self {
                available: 1,
                in_use: 0,
                visible: 0,
            },
            CachedWindowLifecycle::InUse { .. } => Self {
                available: 0,
                in_use: 1,
                visible: 1,
            },
            CachedWindowLifecycle::Invalid => Self::default(),
        }
    }

    fn add_assign(&mut self, counters: Self) {
        self.available = self.available.saturating_add(counters.available);
        self.in_use = self.in_use.saturating_add(counters.in_use);
        self.visible = self.visible.saturating_add(counters.visible);
    }

    fn sub_assign(&mut self, counters: Self) {
        // If this ever saturates, the derived pool counters drifted from lifecycle truth.
        self.available = self.available.saturating_sub(counters.available);
        self.in_use = self.in_use.saturating_sub(counters.in_use);
        self.visible = self.visible.saturating_sub(counters.visible);
    }
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
    reuse_scan_index: usize,
    in_use_indices: Vec<usize>,
    visible_available_indices: Vec<usize>,
    windows: Vec<CachedRenderWindow>,
    payload_by_window: HashMap<i32, CachedWindowPayload>,
    available_windows_by_placement: HashMap<WindowPlacement, Vec<usize>>,
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
            reuse_scan_index: 0,
            in_use_indices: Vec::new(),
            visible_available_indices: Vec::new(),
            windows: Vec::new(),
            payload_by_window: HashMap::new(),
            available_windows_by_placement: HashMap::new(),
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
        let index = self.windows.len();
        self.windows.push(CachedRenderWindow {
            handles,
            lifecycle,
            placement: Some(placement),
        });
        self.visible_available_indices.push(index);
        self.track_available_window_index_insert(index, Some(placement), lifecycle);
        self.track_window_insert(lifecycle);
    }

    fn clear_payload(&mut self, window_id: i32) {
        self.payload_by_window.remove(&window_id);
    }

    fn track_available_window_index_insert(
        &mut self,
        index: usize,
        placement: Option<WindowPlacement>,
        lifecycle: CachedWindowLifecycle,
    ) {
        if !matches!(
            lifecycle,
            CachedWindowLifecycle::AvailableVisible { .. }
                | CachedWindowLifecycle::AvailableHidden { .. }
        ) {
            return;
        }
        let Some(placement) = placement else {
            return;
        };
        self.available_windows_by_placement
            .entry(placement)
            .or_default()
            .push(index);
    }

    fn track_available_window_index_remove(
        &mut self,
        index: usize,
        placement: Option<WindowPlacement>,
        lifecycle: CachedWindowLifecycle,
    ) {
        if !matches!(
            lifecycle,
            CachedWindowLifecycle::AvailableVisible { .. }
                | CachedWindowLifecycle::AvailableHidden { .. }
        ) {
            return;
        }
        let Some(placement) = placement else {
            return;
        };

        let mut remove_key = false;
        if let Some(indices) = self.available_windows_by_placement.get_mut(&placement) {
            if let Some(position) = indices.iter().position(|candidate| *candidate == index) {
                indices.swap_remove(position);
            }
            remove_key = indices.is_empty();
        }
        if remove_key {
            self.available_windows_by_placement.remove(&placement);
        }
    }

    fn track_available_window_transition(
        &mut self,
        index: usize,
        before_lifecycle: CachedWindowLifecycle,
        before_placement: Option<WindowPlacement>,
        after_lifecycle: CachedWindowLifecycle,
        after_placement: Option<WindowPlacement>,
    ) {
        self.track_available_window_index_remove(index, before_placement, before_lifecycle);
        self.track_available_window_index_insert(index, after_placement, after_lifecycle);
    }

    fn sync_lifecycle_counters(&mut self) {
        let mut counters = WindowLifecycleCounters::default();
        for cached in &self.windows {
            counters.add_assign(WindowLifecycleCounters::single(cached.lifecycle));
        }
        self.lifecycle_counters = counters;
    }

    fn sync_available_window_placement_index(&mut self) {
        let mut next_index = HashMap::new();
        for (index, cached) in self.windows.iter().enumerate() {
            if !matches!(
                cached.lifecycle,
                CachedWindowLifecycle::AvailableVisible { .. }
                    | CachedWindowLifecycle::AvailableHidden { .. }
            ) {
                continue;
            }
            let Some(placement) = cached.placement else {
                continue;
            };
            next_index
                .entry(placement)
                .or_insert_with(Vec::new)
                .push(index);
        }
        self.available_windows_by_placement = next_index;
    }

    fn track_window_insert(&mut self, lifecycle: CachedWindowLifecycle) {
        self.lifecycle_counters
            .add_assign(WindowLifecycleCounters::single(lifecycle));
    }

    fn track_window_remove(&mut self, lifecycle: CachedWindowLifecycle) {
        self.lifecycle_counters
            .sub_assign(WindowLifecycleCounters::single(lifecycle));
    }

    fn track_window_transition(
        &mut self,
        before: CachedWindowLifecycle,
        after: CachedWindowLifecycle,
    ) {
        if before == after {
            return;
        }
        self.track_window_remove(before);
        self.track_window_insert(after);
    }

    fn available_window_count(&self) -> usize {
        // cleanup/capacity decisions must follow authoritative window lifecycle truth.
        // The tracked counters are an optimization only; if they ever drift, rendering must still
        // recover by scanning the retained windows instead of suppressing clear work.
        self.windows
            .iter()
            .filter(|cached| {
                matches!(
                    cached.lifecycle,
                    CachedWindowLifecycle::AvailableVisible { .. }
                        | CachedWindowLifecycle::AvailableHidden { .. }
                )
            })
            .count()
    }

    fn in_use_window_count(&self) -> usize {
        self.windows
            .iter()
            .filter(|cached| matches!(cached.lifecycle, CachedWindowLifecycle::InUse { .. }))
            .count()
    }

    fn visible_window_count(&self) -> usize {
        self.windows
            .iter()
            .filter(|cached| {
                matches!(
                    cached.lifecycle,
                    CachedWindowLifecycle::AvailableVisible { .. }
                        | CachedWindowLifecycle::InUse { .. }
                )
            })
            .count()
    }

    fn usable_window_count(&self) -> usize {
        self.available_window_count()
            .saturating_add(self.in_use_window_count())
    }

    fn has_invalid_windows(&self) -> bool {
        self.windows
            .iter()
            .any(|cached| matches!(cached.lifecycle, CachedWindowLifecycle::Invalid))
    }
}

mod ops;
pub(crate) use ops::*;
