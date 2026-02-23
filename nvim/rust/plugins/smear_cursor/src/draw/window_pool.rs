use super::log_draw_error;
use nvim_oxi::Result;
use nvim_oxi::api;
use nvim_oxi::api::opts::{OptionOpts, OptionScope};
use nvim_oxi::api::types::{WindowConfig, WindowRelativeTo, WindowStyle};
use nvim_oxi_utils::handles;
use std::collections::{BinaryHeap, HashMap, HashSet};

pub(crate) const ADAPTIVE_POOL_MIN_BUDGET: usize = 16;
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
    pub(crate) zindex: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CachedWindowPayload {
    character: String,
    hl_group: String,
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
    Created,
    Reused,
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
    pub(crate) fn cached_payload_matches(
        &self,
        window_id: i32,
        character: &str,
        hl_group: &str,
    ) -> bool {
        self.payload_by_window
            .get(&window_id)
            .is_some_and(|payload| payload.character == character && payload.hl_group == hl_group)
    }

    pub(crate) fn cache_payload(&mut self, window_id: i32, character: &str, hl_group: &str) {
        if let Some(payload) = self.payload_by_window.get_mut(&window_id) {
            payload.character.clear();
            payload.character.push_str(character);
            payload.hl_group.clear();
            payload.hl_group.push_str(hl_group);
            return;
        }

        self.payload_by_window.insert(
            window_id,
            CachedWindowPayload {
                character: character.to_string(),
                hl_group: hl_group.to_string(),
            },
        );
    }

    fn clear_payload(&mut self, window_id: i32) {
        self.payload_by_window.remove(&window_id);
    }
}

struct EventIgnoreGuard {
    previous: Option<String>,
}

impl EventIgnoreGuard {
    fn set_all() -> Self {
        let opts = OptionOpts::builder().build();
        let previous = api::get_option_value::<String>("eventignore", &opts).ok();
        if let Err(err) = api::set_option_value("eventignore", "all", &opts) {
            log_draw_error("set eventignore=all", &err);
        }
        Self { previous }
    }
}

impl Drop for EventIgnoreGuard {
    fn drop(&mut self) {
        let Some(previous) = self.previous.take() else {
            return;
        };
        let opts = OptionOpts::builder().build();
        if let Err(err) = api::set_option_value("eventignore", previous, &opts) {
            log_draw_error("restore eventignore", &err);
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct AdaptiveBudgetState {
    ewma_demand_milli: u64,
    cached_budget: usize,
}

fn ceil_div_u64(lhs: u64, rhs: u64) -> u64 {
    if rhs == 0 {
        return 0;
    }
    lhs.div_ceil(rhs)
}

fn next_adaptive_budget(previous: AdaptiveBudgetState, frame_demand: usize) -> AdaptiveBudgetState {
    let demand_milli = u64::try_from(frame_demand)
        .unwrap_or(u64::MAX)
        .saturating_mul(ADAPTIVE_POOL_EWMA_SCALE);
    let weighted_prev = previous
        .ewma_demand_milli
        .saturating_mul(ADAPTIVE_POOL_EWMA_PREV_WEIGHT);
    let weighted_new = demand_milli.saturating_mul(ADAPTIVE_POOL_EWMA_NEW_WEIGHT);
    let denominator = ADAPTIVE_POOL_EWMA_PREV_WEIGHT.saturating_add(ADAPTIVE_POOL_EWMA_NEW_WEIGHT);
    let next_ewma = if previous.ewma_demand_milli == 0 {
        demand_milli
    } else {
        weighted_prev
            .saturating_add(weighted_new)
            .saturating_add(denominator.saturating_sub(1))
            / denominator.max(1)
    };
    let ewma_demand =
        usize::try_from(ceil_div_u64(next_ewma, ADAPTIVE_POOL_EWMA_SCALE)).unwrap_or(usize::MAX);
    let target_budget = ewma_demand
        .saturating_add(ADAPTIVE_POOL_BUDGET_MARGIN)
        .clamp(ADAPTIVE_POOL_MIN_BUDGET, ADAPTIVE_POOL_HARD_MAX_BUDGET);
    let next_budget = if target_budget >= previous.cached_budget {
        target_budget
    } else {
        previous
            .cached_budget
            .saturating_sub(ADAPTIVE_POOL_BUDGET_MARGIN)
            .max(target_budget)
            .max(ADAPTIVE_POOL_MIN_BUDGET)
    };

    AdaptiveBudgetState {
        ewma_demand_milli: next_ewma,
        cached_budget: next_budget,
    }
}

fn effective_keep_budget(adaptive_budget: usize, max_kept_windows: usize) -> usize {
    adaptive_budget.min(max_kept_windows)
}

fn lru_prune_indices(windows: &[CachedRenderWindow], keep_count: usize) -> Vec<usize> {
    let available: Vec<(usize, FrameEpoch)> = windows
        .iter()
        .enumerate()
        .filter_map(|(index, cached)| cached.available_epoch().map(|epoch| (index, epoch)))
        .collect();
    if available.len() <= keep_count {
        return Vec::new();
    }

    if keep_count == 0 {
        let mut remove_indices: Vec<usize> =
            available.into_iter().map(|(index, _)| index).collect();
        remove_indices.sort_unstable();
        return remove_indices;
    }

    let mut keep_heap: BinaryHeap<std::cmp::Reverse<(FrameEpoch, usize)>> = BinaryHeap::new();
    for (index, epoch) in available.iter().copied() {
        if keep_heap.len() < keep_count {
            keep_heap.push(std::cmp::Reverse((epoch, index)));
            continue;
        }

        let Some(std::cmp::Reverse((oldest_kept_epoch, oldest_kept_index))) =
            keep_heap.peek().copied()
        else {
            continue;
        };
        if (epoch, index) > (oldest_kept_epoch, oldest_kept_index) {
            keep_heap.pop();
            keep_heap.push(std::cmp::Reverse((epoch, index)));
        }
    }

    let keep_indices: HashSet<usize> = keep_heap
        .into_iter()
        .map(|std::cmp::Reverse((_, index))| index)
        .collect();
    let mut remove_indices: Vec<usize> = available
        .into_iter()
        .filter_map(|(index, _)| (!keep_indices.contains(&index)).then_some(index))
        .collect();
    remove_indices.sort_unstable();
    remove_indices
}

fn window_from_handle_i32(handle: i32) -> Option<api::Window> {
    handles::valid_window(i64::from(handle))
}

fn buffer_from_handle_i32(handle: i32) -> Option<api::Buffer> {
    handles::valid_buffer(i64::from(handle))
}

fn open_window_config(row: i64, col: i64, zindex: u32) -> WindowConfig {
    let mut builder = WindowConfig::builder();
    builder
        .relative(WindowRelativeTo::Editor)
        .row(row as f64 - 1.0)
        .col(col as f64 - 1.0)
        .width(1)
        .height(1)
        .focusable(false)
        .style(WindowStyle::Minimal)
        .noautocmd(true)
        .hide(false)
        .zindex(zindex);
    builder.build()
}

fn reconfigure_window_config(row: i64, col: i64, zindex: u32) -> WindowConfig {
    let mut builder = WindowConfig::builder();
    builder
        .relative(WindowRelativeTo::Editor)
        .row(row as f64 - 1.0)
        .col(col as f64 - 1.0)
        .width(1)
        .height(1)
        .focusable(false)
        .style(WindowStyle::Minimal)
        .hide(false)
        .zindex(zindex);
    builder.build()
}

fn hide_window_config() -> WindowConfig {
    let mut builder = WindowConfig::builder();
    builder.hide(true);
    builder.build()
}

fn initialize_buffer_options(buffer: &api::Buffer) -> Result<()> {
    let opts = OptionOpts::builder().buf(buffer.clone()).build();
    api::set_option_value("buftype", RENDER_BUFFER_TYPE, &opts)?;
    api::set_option_value("filetype", RENDER_BUFFER_FILETYPE, &opts)?;
    api::set_option_value("bufhidden", "wipe", &opts)?;
    api::set_option_value("swapfile", false, &opts)?;
    Ok(())
}

fn initialize_window_options(window: &api::Window) -> Result<()> {
    let opts = OptionOpts::builder()
        .scope(OptionScope::Local)
        .win(window.clone())
        .build();
    api::set_option_value("winhighlight", "NormalFloat:Normal", &opts)?;
    api::set_option_value("winblend", 100_i64, &opts)?;
    Ok(())
}

fn close_cached_window(namespace_id: u32, handles: WindowBufferHandle) {
    if let Some(mut buffer) = buffer_from_handle_i32(handles.buffer_id)
        && let Err(err) = buffer.clear_namespace(namespace_id, 0..)
    {
        log_draw_error("clear cached render namespace", &err);
    }
    if let Some(window) = window_from_handle_i32(handles.window_id)
        && let Err(err) = window.close(true)
    {
        log_draw_error("close cached render window", &err);
    }
}

fn adjust_tracking_after_remove(tab_windows: &mut TabWindows, removed_index: usize) {
    if tab_windows.reuse_scan_index > removed_index {
        tab_windows.reuse_scan_index = tab_windows.reuse_scan_index.saturating_sub(1);
    }

    let mut write = 0;
    for read in 0..tab_windows.in_use_indices.len() {
        let index = tab_windows.in_use_indices[read];
        if index == removed_index {
            continue;
        }
        tab_windows.in_use_indices[write] = if index > removed_index {
            index.saturating_sub(1)
        } else {
            index
        };
        write += 1;
    }
    tab_windows.in_use_indices.truncate(write);

    let mut visible_write = 0;
    for read in 0..tab_windows.visible_available_indices.len() {
        let index = tab_windows.visible_available_indices[read];
        if index == removed_index {
            continue;
        }
        tab_windows.visible_available_indices[visible_write] = if index > removed_index {
            index.saturating_sub(1)
        } else {
            index
        };
        visible_write += 1;
    }
    tab_windows
        .visible_available_indices
        .truncate(visible_write);
}

fn remove_cached_window_at(tab_windows: &mut TabWindows, namespace_id: u32, remove_index: usize) {
    let cached = tab_windows.windows.remove(remove_index);
    adjust_tracking_after_remove(tab_windows, remove_index);
    tab_windows.clear_payload(cached.handles.window_id);
    close_cached_window(namespace_id, cached.handles);
}

fn mark_cached_window_invalid(tab_windows: &mut TabWindows, index: usize) -> bool {
    let Some(window_id) = tab_windows
        .windows
        .get(index)
        .map(|cached| cached.handles.window_id)
    else {
        return false;
    };
    if tab_windows
        .windows
        .get(index)
        .is_some_and(|cached| matches!(cached.lifecycle, CachedWindowLifecycle::Invalid))
    {
        return false;
    }
    tab_windows.clear_payload(window_id);
    let Some(cached) = tab_windows.windows.get_mut(index) else {
        return false;
    };
    cached.mark_invalid();
    true
}

fn remove_invalid_windows(tab_windows: &mut TabWindows, namespace_id: u32) -> usize {
    let mut remove_indices = Vec::new();
    for (index, cached) in tab_windows.windows.iter().enumerate() {
        if matches!(cached.lifecycle, CachedWindowLifecycle::Invalid) {
            remove_indices.push(index);
        }
    }
    if remove_indices.is_empty() {
        return 0;
    }

    let _event_ignore = EventIgnoreGuard::set_all();
    for remove_index in remove_indices.iter().copied().rev() {
        remove_cached_window_at(tab_windows, namespace_id, remove_index);
    }
    remove_indices.len()
}

fn rollover_in_use_windows(tab_windows: &mut TabWindows, previous_epoch: FrameEpoch) {
    let in_use_indices = std::mem::take(&mut tab_windows.in_use_indices);
    let mut visible_available_indices = Vec::with_capacity(in_use_indices.len());
    for index in in_use_indices {
        if let Some(cached) = tab_windows.windows.get_mut(index) {
            let rollover = cached.rollover_to_next_epoch(previous_epoch);
            if matches!(
                rollover,
                EpochRollover::ReleasedForReuse | EpochRollover::RecoveredStaleInUse
            ) {
                visible_available_indices.push(index);
            }
        }
    }
    tab_windows.visible_available_indices = visible_available_indices;
}

fn rebuild_available_window_placement_index(tab_windows: &mut TabWindows) {
    tab_windows.available_windows_by_placement.clear();
    for (index, cached) in tab_windows.windows.iter().enumerate() {
        if !cached.is_available_for_reuse() {
            continue;
        }
        let Some(placement) = cached.placement else {
            continue;
        };
        tab_windows
            .available_windows_by_placement
            .entry(placement)
            .or_default()
            .push(index);
    }
}

fn available_window_index_for_placement(
    tab_windows: &mut TabWindows,
    placement: WindowPlacement,
) -> Option<usize> {
    let candidates = tab_windows
        .available_windows_by_placement
        .get_mut(&placement)?;
    while let Some(index) = candidates.pop() {
        if tab_windows
            .windows
            .get(index)
            .copied()
            .is_some_and(|cached| {
                cached.is_available_for_reuse() && cached.placement == Some(placement)
            })
        {
            return Some(index);
        }
    }
    None
}

enum ReuseAttempt {
    NotCandidate,
    Reused(AcquiredWindow),
    Failed(ReuseFailureReason),
}

fn try_reuse_cached_window_at_index(
    tab_windows: &mut TabWindows,
    index: usize,
    placement: WindowPlacement,
) -> ReuseAttempt {
    let Some(cached) = tab_windows.windows.get(index).copied() else {
        return ReuseAttempt::NotCandidate;
    };
    if !cached.is_available_for_reuse() {
        return ReuseAttempt::NotCandidate;
    }

    let Some(mut window) = window_from_handle_i32(cached.handles.window_id) else {
        let _ = mark_cached_window_invalid(tab_windows, index);
        return ReuseAttempt::Failed(ReuseFailureReason::MissingWindow);
    };

    if cached.needs_reconfigure(placement) {
        let config = reconfigure_window_config(placement.row, placement.col, placement.zindex);
        if window.set_config(&config).is_err() {
            let _ = mark_cached_window_invalid(tab_windows, index);
            return ReuseAttempt::Failed(ReuseFailureReason::ReconfigureFailed);
        }
    }

    let Some(buffer) = buffer_from_handle_i32(cached.handles.buffer_id) else {
        let _ = mark_cached_window_invalid(tab_windows, index);
        return ReuseAttempt::Failed(ReuseFailureReason::MissingBuffer);
    };

    if !tab_windows.windows[index].mark_in_use(tab_windows.current_epoch) {
        return ReuseAttempt::NotCandidate;
    }

    tab_windows.windows[index].set_placement(placement);
    tab_windows.frame_demand = tab_windows.frame_demand.saturating_add(1);
    tab_windows.in_use_indices.push(index);
    ReuseAttempt::Reused(AcquiredWindow {
        window_id: cached.handles.window_id,
        buffer,
        kind: AcquireKind::Reused,
        reuse_failures: ReuseFailureCounters::default(),
    })
}

pub(crate) fn acquire(
    tabs: &mut HashMap<i32, TabWindows>,
    namespace_id: u32,
    tab_handle: i32,
    placement: WindowPlacement,
) -> Result<AcquiredWindow> {
    let tab_windows = tabs.entry(tab_handle).or_default();
    let mut reuse_failures = ReuseFailureCounters::default();

    if let Some(index) = available_window_index_for_placement(tab_windows, placement) {
        match try_reuse_cached_window_at_index(tab_windows, index, placement) {
            ReuseAttempt::Reused(mut acquired) => {
                acquired.reuse_failures = reuse_failures;
                return Ok(acquired);
            }
            ReuseAttempt::Failed(reason) => {
                reuse_failures.record(reason);
                if remove_invalid_windows(tab_windows, namespace_id) > 0 {
                    rebuild_available_window_placement_index(tab_windows);
                }
            }
            ReuseAttempt::NotCandidate => {}
        }
    }

    while tab_windows.reuse_scan_index < tab_windows.windows.len() {
        let index = tab_windows.reuse_scan_index;
        tab_windows.reuse_scan_index = tab_windows.reuse_scan_index.saturating_add(1);

        match try_reuse_cached_window_at_index(tab_windows, index, placement) {
            ReuseAttempt::Reused(mut acquired) => {
                acquired.reuse_failures = reuse_failures;
                return Ok(acquired);
            }
            ReuseAttempt::Failed(reason) => {
                reuse_failures.record(reason);
                if remove_invalid_windows(tab_windows, namespace_id) > 0 {
                    rebuild_available_window_placement_index(tab_windows);
                }
            }
            ReuseAttempt::NotCandidate => {}
        }
    }

    let _event_ignore = EventIgnoreGuard::set_all();
    let buffer = api::create_buf(false, true)?;
    let config = open_window_config(placement.row, placement.col, placement.zindex);
    let window = api::open_win(&buffer, false, &config)?;
    initialize_buffer_options(&buffer)?;
    initialize_window_options(&window)?;

    let handles = WindowBufferHandle {
        window_id: window.handle(),
        buffer_id: buffer.handle(),
    };

    tab_windows.windows.push(CachedRenderWindow::new_in_use(
        handles,
        tab_windows.current_epoch,
        placement,
    ));
    tab_windows
        .in_use_indices
        .push(tab_windows.windows.len().saturating_sub(1));
    tab_windows.frame_demand = tab_windows.frame_demand.saturating_add(1);
    tab_windows.reuse_scan_index = tab_windows.windows.len();

    Ok(AcquiredWindow {
        window_id: handles.window_id,
        buffer,
        kind: AcquireKind::Created,
        reuse_failures,
    })
}

pub(crate) fn begin_frame(tabs: &mut HashMap<i32, TabWindows>) {
    for tab_windows in tabs.values_mut() {
        tab_windows.last_draw_signature = None;
        let next_budget = next_adaptive_budget(
            AdaptiveBudgetState {
                ewma_demand_milli: tab_windows.ewma_demand_milli,
                cached_budget: tab_windows.cached_budget,
            },
            tab_windows.frame_demand,
        );
        tab_windows.ewma_demand_milli = next_budget.ewma_demand_milli;
        tab_windows.cached_budget = next_budget.cached_budget;
        let previous_epoch = tab_windows.current_epoch;
        tab_windows.current_epoch = tab_windows.current_epoch.next();
        tab_windows.frame_demand = 0;
        tab_windows.reuse_scan_index = 0;
        rollover_in_use_windows(tab_windows, previous_epoch);

        debug_assert!(
            tab_windows.in_use_indices.is_empty(),
            "in-use index set must be empty after rollover"
        );
        debug_assert!(
            tab_windows
                .windows
                .iter()
                .all(|cached| cached.is_available_for_reuse()),
            "cached render windows must be available after epoch rollover"
        );

        rebuild_available_window_placement_index(tab_windows);
    }
}

pub(crate) fn prune(
    tabs: &mut HashMap<i32, TabWindows>,
    namespace_id: u32,
    max_kept_windows: usize,
) -> usize {
    let mut pruned_windows = 0_usize;
    for tab_windows in tabs.values_mut() {
        let keep_budget = effective_keep_budget(tab_windows.cached_budget, max_kept_windows);
        if tab_windows.windows.len() <= keep_budget {
            if remove_invalid_windows(tab_windows, namespace_id) > 0 {
                rebuild_available_window_placement_index(tab_windows);
            }
            continue;
        }

        let remove_indices = lru_prune_indices(&tab_windows.windows, keep_budget);
        if !remove_indices.is_empty() {
            let _event_ignore = EventIgnoreGuard::set_all();
            for remove_index in remove_indices.iter().copied().rev() {
                remove_cached_window_at(tab_windows, namespace_id, remove_index);
            }
            pruned_windows = pruned_windows.saturating_add(remove_indices.len());
        }

        let invalid_removed = remove_invalid_windows(tab_windows, namespace_id);
        if invalid_removed > 0 {
            pruned_windows = pruned_windows.saturating_add(invalid_removed);
        }
        if !remove_indices.is_empty() || invalid_removed > 0 {
            rebuild_available_window_placement_index(tab_windows);
        }
    }
    pruned_windows
}

pub(crate) fn release_unused(tabs: &mut HashMap<i32, TabWindows>, namespace_id: u32) {
    let hide_config = hide_window_config();
    for tab_windows in tabs.values_mut() {
        let mut hide_indices = std::mem::take(&mut tab_windows.visible_available_indices);
        if hide_indices.is_empty() {
            continue;
        }
        hide_indices.sort_unstable();
        hide_indices.dedup();

        for index in hide_indices.into_iter().rev() {
            if index >= tab_windows.windows.len() {
                continue;
            }
            if !tab_windows.windows[index].should_hide() {
                continue;
            }

            let handles = tab_windows.windows[index].handles;
            let Some(mut window) = window_from_handle_i32(handles.window_id) else {
                let _ = mark_cached_window_invalid(tab_windows, index);
                continue;
            };

            if window.set_config(&hide_config).is_err() {
                let _ = mark_cached_window_invalid(tab_windows, index);
                continue;
            }

            tab_windows.windows[index].mark_hidden();
        }

        if remove_invalid_windows(tab_windows, namespace_id) > 0 {
            rebuild_available_window_placement_index(tab_windows);
        }
    }
}

pub(crate) fn recover_invalid_window(
    tabs: &mut HashMap<i32, TabWindows>,
    namespace_id: u32,
    tab_handle: i32,
    window_id: i32,
) -> bool {
    let Some(tab_windows) = tabs.get_mut(&tab_handle) else {
        return false;
    };
    let Some(index) = tab_windows
        .windows
        .iter()
        .position(|cached| cached.handles.window_id == window_id)
    else {
        return false;
    };

    if !mark_cached_window_invalid(tab_windows, index) {
        return false;
    }
    let _ = remove_invalid_windows(tab_windows, namespace_id);
    rebuild_available_window_placement_index(tab_windows);
    true
}

fn window_buffer(window: &api::Window) -> Option<api::Buffer> {
    if !window.is_valid() {
        return None;
    }
    match window.get_buf() {
        Ok(buffer) => Some(buffer),
        Err(err) => {
            log_draw_error("window.get_buf", &err);
            None
        }
    }
}

fn buffer_has_render_marker(buffer: &api::Buffer) -> bool {
    let opts = OptionOpts::builder().buf(buffer.clone()).build();
    let Ok(filetype) = api::get_option_value::<String>("filetype", &opts) else {
        return false;
    };
    if filetype != RENDER_BUFFER_FILETYPE {
        return false;
    }

    let Ok(buftype) = api::get_option_value::<String>("buftype", &opts) else {
        return false;
    };
    buftype == RENDER_BUFFER_TYPE
}

pub(crate) fn close_orphan_render_windows(namespace_id: u32) {
    let _event_ignore = EventIgnoreGuard::set_all();
    for window in api::list_wins() {
        let Some(mut buffer) = window_buffer(&window) else {
            continue;
        };
        if !buffer_has_render_marker(&buffer) {
            continue;
        }

        if let Err(err) = buffer.clear_namespace(namespace_id, 0..) {
            log_draw_error("clear orphan render namespace", &err);
        }
        if let Err(err) = window.close(true) {
            log_draw_error("close orphan render window", &err);
        }
    }
}

pub(crate) fn purge(tabs: &mut HashMap<i32, TabWindows>, namespace_id: u32) {
    let _event_ignore = EventIgnoreGuard::set_all();

    for tab_windows in tabs.values() {
        for cached in tab_windows.windows.iter().copied() {
            close_cached_window(namespace_id, cached.handles);
        }
    }

    tabs.clear();
    close_orphan_render_windows(namespace_id);
}

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

#[cfg(test)]
mod tests {
    use super::{
        ADAPTIVE_POOL_HARD_MAX_BUDGET, ADAPTIVE_POOL_MIN_BUDGET, AdaptiveBudgetState,
        CachedRenderWindow, CachedWindowLifecycle, EpochRollover, FrameEpoch, TabWindows,
        WindowBufferHandle, WindowPlacement, adjust_tracking_after_remove,
        available_window_index_for_placement, effective_keep_budget, lru_prune_indices,
        next_adaptive_budget, rebuild_available_window_placement_index, rollover_in_use_windows,
    };

    #[test]
    fn adaptive_budget_has_floor_when_idle() {
        let previous = AdaptiveBudgetState {
            ewma_demand_milli: 0,
            cached_budget: ADAPTIVE_POOL_MIN_BUDGET,
        };

        let next = next_adaptive_budget(previous, 0);
        assert_eq!(next.cached_budget, ADAPTIVE_POOL_MIN_BUDGET);
        assert_eq!(next.ewma_demand_milli, 0);
    }

    #[test]
    fn adaptive_budget_grows_with_demand() {
        let previous = AdaptiveBudgetState {
            ewma_demand_milli: 0,
            cached_budget: ADAPTIVE_POOL_MIN_BUDGET,
        };

        let next = next_adaptive_budget(previous, 120);
        assert_eq!(next.ewma_demand_milli, 120_000);
        assert_eq!(next.cached_budget, 128);
    }

    #[test]
    fn adaptive_budget_shrinks_gradually() {
        let previous = AdaptiveBudgetState {
            ewma_demand_milli: 120_000,
            cached_budget: 120,
        };

        let next = next_adaptive_budget(previous, 0);
        assert_eq!(next.cached_budget, 112);
    }

    #[test]
    fn adaptive_budget_honors_hard_max() {
        let previous = AdaptiveBudgetState {
            ewma_demand_milli: 0,
            cached_budget: ADAPTIVE_POOL_MIN_BUDGET,
        };

        let next = next_adaptive_budget(previous, 10_000);
        assert_eq!(next.cached_budget, ADAPTIVE_POOL_HARD_MAX_BUDGET);
    }

    #[test]
    fn keep_budget_respects_max_kept_windows_cap() {
        assert_eq!(effective_keep_budget(120, 50), 50);
        assert_eq!(effective_keep_budget(32, 50), 32);
        assert_eq!(effective_keep_budget(16, 0), 0);
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
                zindex: 50,
            }),
        }
    }

    #[test]
    fn available_window_index_for_placement_returns_matching_available_window() {
        let target = WindowPlacement {
            row: 14,
            col: 22,
            zindex: 300,
        };
        let mut tab_windows = TabWindows {
            windows: vec![
                cached(10, 110, 1),
                CachedRenderWindow::new_in_use(
                    WindowBufferHandle {
                        window_id: 20,
                        buffer_id: 120,
                    },
                    FrameEpoch(9),
                    target,
                ),
                CachedRenderWindow {
                    handles: WindowBufferHandle {
                        window_id: 30,
                        buffer_id: 130,
                    },
                    lifecycle: CachedWindowLifecycle::AvailableVisible {
                        last_used_epoch: FrameEpoch(7),
                    },
                    placement: Some(target),
                },
            ],
            ..TabWindows::default()
        };
        rebuild_available_window_placement_index(&mut tab_windows);

        let selected = available_window_index_for_placement(&mut tab_windows, target);
        assert_eq!(selected, Some(2));

        tab_windows.windows[2].lifecycle = CachedWindowLifecycle::InUse {
            epoch: FrameEpoch(10),
        };
        assert_eq!(
            available_window_index_for_placement(&mut tab_windows, target),
            None
        );
    }

    #[test]
    fn available_window_index_for_placement_returns_none_for_missing_or_unplaced_windows() {
        let target = WindowPlacement {
            row: 4,
            col: 9,
            zindex: 40,
        };
        let mut tab_windows = TabWindows {
            windows: vec![
                CachedRenderWindow {
                    handles: WindowBufferHandle {
                        window_id: 1,
                        buffer_id: 11,
                    },
                    lifecycle: CachedWindowLifecycle::AvailableHidden {
                        last_used_epoch: FrameEpoch(1),
                    },
                    placement: None,
                },
                CachedRenderWindow {
                    handles: WindowBufferHandle {
                        window_id: 2,
                        buffer_id: 12,
                    },
                    lifecycle: CachedWindowLifecycle::AvailableVisible {
                        last_used_epoch: FrameEpoch(2),
                    },
                    placement: Some(WindowPlacement {
                        row: 4,
                        col: 10,
                        zindex: 40,
                    }),
                },
            ],
            ..TabWindows::default()
        };

        assert_eq!(
            available_window_index_for_placement(&mut tab_windows, target),
            None
        );
    }

    #[test]
    fn rollover_releases_in_use_window_from_previous_epoch() {
        let handles = WindowBufferHandle {
            window_id: 10,
            buffer_id: 11,
        };
        let mut cached = CachedRenderWindow::new_in_use(
            handles,
            FrameEpoch(9),
            WindowPlacement {
                row: 4,
                col: 8,
                zindex: 100,
            },
        );
        assert_eq!(
            cached.rollover_to_next_epoch(FrameEpoch(9)),
            EpochRollover::ReleasedForReuse
        );
        assert_eq!(cached.available_epoch(), Some(FrameEpoch(9)));
        assert!(cached.is_available_for_reuse());
        assert!(cached.should_hide());
        cached.mark_hidden();
        assert!(!cached.should_hide());
    }

    #[test]
    fn rollover_recovers_stale_in_use_window() {
        let handles = WindowBufferHandle {
            window_id: 20,
            buffer_id: 21,
        };
        let mut cached = CachedRenderWindow::new_in_use(
            handles,
            FrameEpoch(3),
            WindowPlacement {
                row: 7,
                col: 3,
                zindex: 100,
            },
        );
        assert_eq!(
            cached.rollover_to_next_epoch(FrameEpoch(5)),
            EpochRollover::RecoveredStaleInUse
        );
        assert_eq!(cached.available_epoch(), Some(FrameEpoch(3)));
        assert!(cached.is_available_for_reuse());
        assert!(cached.should_hide());
        cached.mark_hidden();
        assert!(!cached.should_hide());
    }

    #[test]
    fn lru_prune_indices_empty_when_budget_sufficient() {
        let windows = vec![cached(1, 10, 7), cached(2, 20, 8)];
        assert!(lru_prune_indices(&windows, 2).is_empty());
        assert!(lru_prune_indices(&windows, 3).is_empty());
    }

    #[test]
    fn lru_prune_indices_removes_oldest_epochs_deterministically() {
        let windows = vec![
            cached(1, 10, 9),
            CachedRenderWindow::new_in_use(
                WindowBufferHandle {
                    window_id: 90,
                    buffer_id: 99,
                },
                FrameEpoch(9),
                WindowPlacement {
                    row: 3,
                    col: 9,
                    zindex: 100,
                },
            ),
            cached(2, 20, 1),
            cached(3, 30, 4),
            cached(4, 40, 1),
            cached(5, 50, 7),
        ];

        assert_eq!(lru_prune_indices(&windows, 3), vec![2, 4]);
    }

    #[test]
    fn cached_window_needs_reconfigure_only_when_placement_or_visibility_changes() {
        let placement = WindowPlacement {
            row: 10,
            col: 20,
            zindex: 30,
        };
        let mut cached = CachedRenderWindow {
            handles: WindowBufferHandle {
                window_id: 77,
                buffer_id: 88,
            },
            lifecycle: CachedWindowLifecycle::AvailableVisible {
                last_used_epoch: FrameEpoch(5),
            },
            placement: Some(placement),
        };

        assert!(!cached.needs_reconfigure(placement));
        assert!(cached.needs_reconfigure(WindowPlacement {
            row: 10,
            col: 21,
            zindex: 30,
        }));

        cached.mark_hidden();
        assert!(cached.needs_reconfigure(placement));
    }

    #[test]
    fn tab_windows_payload_cache_matches_and_clears() {
        let mut tab_windows = TabWindows::default();
        assert!(!tab_windows.cached_payload_matches(101, "█", "SmearCursor1"));

        tab_windows.cache_payload(101, "█", "SmearCursor1");
        assert!(tab_windows.cached_payload_matches(101, "█", "SmearCursor1"));
        assert!(!tab_windows.cached_payload_matches(101, "▌", "SmearCursor1"));
        assert!(!tab_windows.cached_payload_matches(101, "█", "SmearCursor2"));

        tab_windows.cache_payload(101, "▌", "SmearCursor2");
        assert!(tab_windows.cached_payload_matches(101, "▌", "SmearCursor2"));
        assert!(!tab_windows.cached_payload_matches(101, "█", "SmearCursor1"));

        tab_windows.clear_payload(101);
        assert!(!tab_windows.cached_payload_matches(101, "▌", "SmearCursor2"));
    }

    #[test]
    fn adjust_tracking_after_remove_reindexes_in_use_and_scan_position() {
        let mut tab_windows = TabWindows {
            reuse_scan_index: 4,
            in_use_indices: vec![0, 2, 4],
            visible_available_indices: vec![1, 3, 4],
            windows: vec![
                cached(1, 101, 1),
                cached(2, 102, 2),
                cached(3, 103, 3),
                cached(4, 104, 4),
                cached(5, 105, 5),
            ],
            ..TabWindows::default()
        };

        adjust_tracking_after_remove(&mut tab_windows, 1);

        assert_eq!(tab_windows.reuse_scan_index, 3);
        assert_eq!(tab_windows.in_use_indices, vec![0, 1, 3]);
        assert_eq!(tab_windows.visible_available_indices, vec![2, 3]);
    }

    #[test]
    fn rollover_in_use_windows_releases_tracked_windows() {
        let previous_epoch = FrameEpoch(12);
        let mut tab_windows = TabWindows {
            windows: vec![
                CachedRenderWindow::new_in_use(
                    WindowBufferHandle {
                        window_id: 31,
                        buffer_id: 41,
                    },
                    previous_epoch,
                    WindowPlacement {
                        row: 1,
                        col: 1,
                        zindex: 40,
                    },
                ),
                CachedRenderWindow::new_in_use(
                    WindowBufferHandle {
                        window_id: 32,
                        buffer_id: 42,
                    },
                    previous_epoch,
                    WindowPlacement {
                        row: 1,
                        col: 2,
                        zindex: 40,
                    },
                ),
            ],
            in_use_indices: vec![0, 1, 99],
            ..TabWindows::default()
        };

        rollover_in_use_windows(&mut tab_windows, previous_epoch);

        assert!(tab_windows.in_use_indices.is_empty());
        assert_eq!(tab_windows.visible_available_indices, vec![0, 1]);
        assert!(
            tab_windows
                .windows
                .iter()
                .all(|cached| cached.is_available_for_reuse() && cached.should_hide())
        );
    }
}
