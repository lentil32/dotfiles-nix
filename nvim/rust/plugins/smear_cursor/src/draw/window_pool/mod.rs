use nvim_oxi::api;
use std::collections::HashMap;

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
    ewma_demand_milli: u64,
    cached_budget: usize,
    pub(crate) last_draw_signature: Option<u64>,
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
            ewma_demand_milli: 0,
            cached_budget: ADAPTIVE_POOL_MIN_BUDGET,
            last_draw_signature: None,
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

    fn clear_payload(&mut self, window_id: i32) {
        self.payload_by_window.remove(&window_id);
    }
}

mod ops;
pub(crate) use ops::*;
