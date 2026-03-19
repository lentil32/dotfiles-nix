fn remove_cached_window_at(tab_windows: &mut TabWindows, namespace_id: u32, remove_index: usize) {
    let Some(cached) = tab_windows.swap_remove_window(remove_index) else {
        return;
    };
    close_cached_window(namespace_id, cached.handles);
}

fn mark_cached_window_invalid(tab_windows: &mut TabWindows, index: usize) -> bool {
    let Some((window_id, previous_lifecycle, previous_placement)) = tab_windows
        .windows
        .get(index)
        .map(|cached| (cached.handles.window_id, cached.lifecycle, cached.placement))
    else {
        return false;
    };
    if matches!(previous_lifecycle, CachedWindowLifecycle::Invalid) {
        return false;
    }
    tab_windows.clear_payload(window_id);
    let Some((next_lifecycle, next_placement)) = tab_windows.windows.get_mut(index).map(|cached| {
        cached.mark_invalid();
        (cached.lifecycle, cached.placement)
    }) else {
        return false;
    };
    tab_windows.track_window_transition(
        index,
        previous_lifecycle,
        previous_placement,
        next_lifecycle,
        next_placement,
    );
    tab_windows.debug_assert_tracking_consistent();
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
    tab_windows.in_use_slots.fill(None);
    for index in in_use_indices {
        let Some((
            previous_lifecycle,
            previous_placement,
            rollover,
            next_lifecycle,
            next_placement,
        )) = tab_windows.windows.get_mut(index).map(|cached| {
            let previous_lifecycle = cached.lifecycle;
            let previous_placement = cached.placement;
            let rollover = cached.rollover_to_next_epoch(previous_epoch);
            (
                previous_lifecycle,
                previous_placement,
                rollover,
                cached.lifecycle,
                cached.placement,
            )
        })
        else {
            continue;
        };
        if matches!(
            rollover,
            EpochRollover::ReleasedForReuse | EpochRollover::RecoveredStaleInUse
        ) {
            tab_windows.track_window_transition(
                index,
                previous_lifecycle,
                previous_placement,
                next_lifecycle,
                next_placement,
            );
        }
    }
    tab_windows.debug_assert_tracking_consistent();
}

fn available_window_index_for_placement(
    tab_windows: &TabWindows,
    placement: WindowPlacement,
) -> Option<usize> {
    tab_windows.placement_window_index(placement)
}

fn reusable_window_index(tab_windows: &TabWindows) -> Option<usize> {
    tab_windows.reusable_window_index()
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

    // Reserve the slot before host calls so one re-entrant edge cannot hand the same reusable
    // window to two placements.
    let previous_lifecycle = tab_windows.windows[index].lifecycle;
    let previous_placement = tab_windows.windows[index].placement;
    if !tab_windows.windows[index].mark_in_use(tab_windows.current_epoch) {
        return ReuseAttempt::NotCandidate;
    }
    tab_windows.windows[index].set_placement(placement);
    let next_lifecycle = tab_windows.windows[index].lifecycle;
    let next_placement = tab_windows.windows[index].placement;
    tab_windows.track_window_transition(
        index,
        previous_lifecycle,
        previous_placement,
        next_lifecycle,
        next_placement,
    );
    tab_windows.frame_demand = tab_windows.frame_demand.saturating_add(1);
    tab_windows.debug_assert_tracking_consistent();

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
    ReuseAttempt::Reused(AcquiredWindow {
        window_id: cached.handles.window_id,
        buffer,
        kind: AcquireKind::Reused,
        reuse_failures: ReuseFailureCounters::default(),
    })
}

const REUSE_ONLY_WARM_SPARE_WINDOWS: usize = 1;

pub(crate) fn frame_capacity_target(
    in_use_windows: usize,
    planned_windows: usize,
    max_kept_windows: usize,
    allocation_policy: AllocationPolicy,
) -> usize {
    let required_capacity = in_use_windows.saturating_add(planned_windows);
    if required_capacity == 0 {
        return 0;
    }

    let warm_spare = match allocation_policy {
        AllocationPolicy::ReuseOnly => REUSE_ONLY_WARM_SPARE_WINDOWS,
        AllocationPolicy::BootstrapIfPoolEmpty => 0,
    };

    // Surprising: this peak-frame cap intentionally stays distinct from the adaptive retained
    // budget hard max. One burst can need more simultaneous windows than we want to keep warm
    // once cleanup converges back to idle.
    required_capacity
        .saturating_add(warm_spare)
        .min(max_kept_windows)
}

#[cfg(test)]
pub(crate) fn acquire(
    tabs: &mut HashMap<i32, TabWindows>,
    namespace_id: u32,
    tab_handle: i32,
    placement: WindowPlacement,
    allocation_policy: AllocationPolicy,
) -> std::result::Result<AcquiredWindow, AcquireError> {
    let tab_windows = tabs.entry(tab_handle).or_default();
    acquire_in_tab(tab_windows, namespace_id, placement, allocation_policy)
}

pub(crate) fn acquire_in_tab(
    tab_windows: &mut TabWindows,
    namespace_id: u32,
    placement: WindowPlacement,
    allocation_policy: AllocationPolicy,
) -> std::result::Result<AcquiredWindow, AcquireError> {
    let mut reuse_failures = ReuseFailureCounters::default();

    if let Some(index) = available_window_index_for_placement(tab_windows, placement) {
        match try_reuse_cached_window_at_index(tab_windows, index, placement) {
            ReuseAttempt::Reused(mut acquired) => {
                acquired.reuse_failures = reuse_failures;
                return Ok(acquired);
            }
            ReuseAttempt::Failed(reason) => {
                reuse_failures.record(reason);
                let _ = remove_invalid_windows(tab_windows, namespace_id);
            }
            ReuseAttempt::NotCandidate => {}
        }
    }

    while let Some(index) = reusable_window_index(tab_windows) {
        match try_reuse_cached_window_at_index(tab_windows, index, placement) {
            ReuseAttempt::Reused(mut acquired) => {
                acquired.reuse_failures = reuse_failures;
                return Ok(acquired);
            }
            ReuseAttempt::Failed(reason) => {
                reuse_failures.record(reason);
                let _ = remove_invalid_windows(tab_windows, namespace_id);
            }
            ReuseAttempt::NotCandidate => break,
        }
    }

    Err(AcquireError::Exhausted { allocation_policy })
}

pub(crate) fn ensure_capacity_in_tab(
    tab_windows: &mut TabWindows,
    namespace_id: u32,
    desired_capacity: usize,
    max_kept_windows: usize,
) -> Result<usize> {
    let _ = remove_invalid_windows(tab_windows, namespace_id);

    let target_capacity = desired_capacity.min(max_kept_windows);
    let usable_window_count = tab_windows.usable_window_count();
    if usable_window_count >= target_capacity {
        return Ok(0);
    }

    let create_count = target_capacity.saturating_sub(usable_window_count);
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
        let cached = CachedRenderWindow::new_available_hidden(handles, tab_windows.current_epoch);
        tab_windows.push_cached_window(cached);
    }

    if target_capacity > tab_windows.cached_budget {
        tab_windows.cached_budget = target_capacity;
    }

    Ok(create_count)
}

fn begin_tab_frame(tab_windows: &mut TabWindows, expected_demand: usize) {
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
}

pub(crate) fn begin_apply_frame(tab_windows: &mut TabWindows, expected_demand: usize) {
    begin_tab_frame(tab_windows, expected_demand);
}

pub(crate) fn begin_cleanup_frame(tab_windows: &mut TabWindows) {
    begin_tab_frame(tab_windows, tab_windows.frame_demand);
}
