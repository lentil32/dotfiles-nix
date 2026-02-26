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

    if cached.handles.window_id <= 0 {
        let _ = mark_cached_window_invalid(tab_windows, index);
        return ReuseAttempt::Failed(ReuseFailureReason::MissingWindow);
    }
    // Hot path: avoid per-op nvim_win_is_valid probes; config/draw failures trigger recovery.
    let mut window = window_from_handle_i32_unchecked(cached.handles.window_id);

    if cached.needs_reconfigure(placement) {
        let config = reconfigure_window_config(
            placement.row,
            placement.col,
            placement.width,
            placement.zindex,
        );
        if set_existing_window_config(&mut window, config).is_err() {
            let _ = mark_cached_window_invalid(tab_windows, index);
            return ReuseAttempt::Failed(ReuseFailureReason::ReconfigureFailed);
        }
    }

    if cached.handles.buffer_id <= 0 {
        let _ = mark_cached_window_invalid(tab_windows, index);
        return ReuseAttempt::Failed(ReuseFailureReason::MissingBuffer);
    }
    // Hot path: skip nvim_buf_is_valid and rely on set_extmark failure recovery.
    let buffer = buffer_from_handle_i32_unchecked(cached.handles.buffer_id);

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
    _allocation_policy: AllocationPolicy,
) -> Result<Option<AcquiredWindow>> {
    let tab_windows = tabs.entry(tab_handle).or_default();
    let mut reuse_failures = ReuseFailureCounters::default();

    if let Some(index) = available_window_index_for_placement(tab_windows, placement) {
        match try_reuse_cached_window_at_index(tab_windows, index, placement) {
            ReuseAttempt::Reused(mut acquired) => {
                acquired.reuse_failures = reuse_failures;
                return Ok(Some(acquired));
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
                return Ok(Some(acquired));
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

    Ok(None)
}

pub(crate) fn ensure_tab_capacity(
    tabs: &mut HashMap<i32, TabWindows>,
    tab_handle: i32,
    desired_capacity: usize,
    max_kept_windows: usize,
) -> Result<usize> {
    let tab_windows = tabs.entry(tab_handle).or_default();
    let target_capacity = desired_capacity.min(max_kept_windows);
    if tab_windows.windows.len() >= target_capacity {
        return Ok(0);
    }

    let create_count = target_capacity.saturating_sub(tab_windows.windows.len());
    if create_count == 0 {
        return Ok(0);
    }

    let _event_ignore = EventIgnoreGuard::set_all();
    let hidden_config = open_hidden_window_config();
    tab_windows.windows.reserve(create_count);

    for _ in 0..create_count {
        let buffer = api::create_buf(false, true)?;
        let window = api::open_win(&buffer, false, &hidden_config)?;
        initialize_buffer_options(&buffer)?;
        initialize_window_options(&window)?;

        let handles = WindowBufferHandle {
            window_id: window.handle(),
            buffer_id: buffer.handle(),
        };
        tab_windows
            .windows
            .push(CachedRenderWindow::new_available_hidden(
                handles,
                tab_windows.current_epoch,
            ));
    }

    if target_capacity > tab_windows.cached_budget {
        tab_windows.cached_budget = target_capacity;
    }
    rebuild_available_window_placement_index(tab_windows);

    Ok(create_count)
}

fn begin_tab_frame(tab_windows: &mut TabWindows, expected_demand: usize) {
    tab_windows.last_draw_signature = None;
    let demand_signal = tab_windows.frame_demand.max(expected_demand);
    let next_budget = next_adaptive_budget(
        AdaptiveBudgetState {
            ewma_demand_milli: tab_windows.ewma_demand_milli,
            cached_budget: tab_windows.cached_budget,
        },
        demand_signal,
    );
    tab_windows.ewma_demand_milli = next_budget.ewma_demand_milli;
    tab_windows.cached_budget = next_budget.cached_budget;
    let previous_epoch = tab_windows.current_epoch;
    tab_windows.current_epoch = tab_windows.current_epoch.next();
    tab_windows.last_frame_demand = demand_signal;
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

pub(crate) fn begin_frame_for_tab(
    tabs: &mut HashMap<i32, TabWindows>,
    tab_handle: i32,
    expected_demand: usize,
) {
    let tab_windows = tabs.entry(tab_handle).or_default();
    begin_tab_frame(tab_windows, expected_demand);
}

pub(crate) fn begin_frame(tabs: &mut HashMap<i32, TabWindows>) {
    for tab_windows in tabs.values_mut() {
        begin_tab_frame(tab_windows, tab_windows.frame_demand);
    }
}
