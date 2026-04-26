fn prune_tab_windows(
    tab_windows: &mut TabWindows,
    namespace_id: NamespaceId,
    max_kept_windows: usize,
) -> usize {
    let keep_budget = effective_keep_budget(tab_windows.cached_budget, max_kept_windows);
    if tab_windows.windows.len() <= keep_budget {
        let _ = remove_invalid_windows(tab_windows, namespace_id);
        return 0;
    }

    let mut pruned_windows = 0_usize;
    let remove_indices = lru_prune_indices(&tab_windows.windows, keep_budget);
    if !remove_indices.is_empty() {
        let _event_ignore = EventIgnoreGuard::set_all();
        for remove_index in remove_indices.iter().copied().rev() {
            if remove_cached_window_at(tab_windows, namespace_id, remove_index)
                == TrackedResourceCloseOutcome::ClosedOrGone
            {
                pruned_windows = pruned_windows.saturating_add(1);
            }
        }
    }

    let invalid_removed = remove_invalid_windows(tab_windows, namespace_id);
    if invalid_removed > 0 {
        pruned_windows = pruned_windows.saturating_add(invalid_removed);
    }

    pruned_windows
}

pub(crate) fn prune_tab(
    tab_windows: &mut TabWindows,
    namespace_id: NamespaceId,
    max_kept_windows: usize,
) -> usize {
    prune_tab_windows(tab_windows, namespace_id, max_kept_windows)
}

fn total_window_count(render_tabs: &std::collections::HashMap<TabHandle, TabWindows>) -> usize {
    render_tabs
        .values()
        .map(|tab_windows| tab_windows.windows.len())
        .sum()
}

fn has_pending_compaction_work(
    render_tabs: &std::collections::HashMap<TabHandle, TabWindows>,
    target_budget: usize,
) -> bool {
    total_window_count(render_tabs) > target_budget
        || render_tabs.values().any(|tab_windows| {
            tab_has_visible_windows(tab_windows) || tab_windows.has_invalid_windows()
        })
}

fn global_compaction_prune_plan(
    render_tabs: &std::collections::HashMap<TabHandle, TabWindows>,
    target_budget: usize,
    max_prune_per_tick: usize,
) -> std::collections::HashMap<TabHandle, Vec<usize>> {
    let total_windows = total_window_count(render_tabs);
    if total_windows <= target_budget || max_prune_per_tick == 0 {
        return std::collections::HashMap::new();
    }

    let prune_goal = total_windows
        .saturating_sub(target_budget)
        .min(max_prune_per_tick);
    let available_candidates = render_tabs
        .values()
        .map(|tab_windows| tab_windows.reusable_window_indices.len())
        .sum::<usize>();
    if available_candidates == 0 {
        return std::collections::HashMap::new();
    }
    if prune_goal >= available_candidates {
        let mut plan = std::collections::HashMap::<TabHandle, Vec<usize>>::new();
        for (tab_handle, tab_windows) in render_tabs {
            if tab_windows.reusable_window_indices.is_empty() {
                continue;
            }
            let mut indices = tab_windows.reusable_window_indices.clone();
            indices.sort_unstable();
            plan.insert(*tab_handle, indices);
        }
        return plan;
    }

    // Keep only the `prune_goal` oldest candidates; a full global sort is unnecessary.
    let mut selected_candidates = std::collections::BinaryHeap::with_capacity(prune_goal);
    for (tab_handle, tab_windows) in render_tabs {
        for index in tab_windows.reusable_window_indices.iter().copied() {
            let Some(cached) = tab_windows.windows.get(index) else {
                continue;
            };
            let Some(epoch) = cached.available_epoch() else {
                continue;
            };
            let candidate = (epoch, *tab_handle, index);

            if selected_candidates.len() < prune_goal {
                selected_candidates.push(candidate);
                continue;
            }

            let Some(current_newest_selected) = selected_candidates.peek().copied() else {
                continue;
            };
            if candidate >= current_newest_selected {
                continue;
            }

            let _ = selected_candidates.pop();
            selected_candidates.push(candidate);
        }
    }

    let mut plan = std::collections::HashMap::<TabHandle, Vec<usize>>::new();
    for (_, tab_handle, index) in selected_candidates.into_iter() {
        plan.entry(tab_handle).or_default().push(index);
    }
    for indices in plan.values_mut() {
        indices.sort_unstable();
    }
    plan
}

fn apply_global_compaction_prune_plan(
    render_tabs: &mut std::collections::HashMap<TabHandle, TabWindows>,
    namespace_id: NamespaceId,
    prune_plan: std::collections::HashMap<TabHandle, Vec<usize>>,
) -> usize {
    let mut pruned_windows = 0_usize;
    let mut tab_handles = prune_plan.keys().copied().collect::<Vec<_>>();
    tab_handles.sort_unstable();

    for tab_handle in tab_handles {
        let Some(tab_windows) = render_tabs.get_mut(&tab_handle) else {
            continue;
        };
        let Some(indices) = prune_plan.get(&tab_handle) else {
            continue;
        };
        let mut removed_any = false;
        let _event_ignore = EventIgnoreGuard::set_all();
        for remove_index in indices.iter().copied().rev() {
            if remove_index >= tab_windows.windows.len() {
                continue;
            }
            if remove_cached_window_at(tab_windows, namespace_id, remove_index)
                == TrackedResourceCloseOutcome::ClosedOrGone
            {
                pruned_windows = pruned_windows.saturating_add(1);
            }
            removed_any = true;
        }
        if removed_any {
            tab_windows.debug_assert_tracking_consistent();
        }
    }

    pruned_windows
}

pub(crate) fn compact_tabs_to_budget(
    render_tabs: &mut std::collections::HashMap<TabHandle, TabWindows>,
    namespace_id: NamespaceId,
    target_budget: usize,
    max_prune_per_tick: usize,
) -> CompactRenderWindowsSummary {
    let mut summary = CompactRenderWindowsSummary {
        target_budget,
        ..CompactRenderWindowsSummary::default()
    };
    let mut tab_handles = render_tabs.keys().copied().collect::<Vec<_>>();
    tab_handles.sort_unstable();

    for tab_handle in tab_handles {
        let Some(tab_windows) = render_tabs.get_mut(&tab_handle) else {
            continue;
        };
        summary.closed_visible_windows = summary
            .closed_visible_windows
            .saturating_add(close_shell_visible_tab_windows(tab_windows, namespace_id));
        let invalid_removed = remove_invalid_windows(tab_windows, namespace_id);
        if invalid_removed > 0 {
            summary.invalid_removed_windows = summary
                .invalid_removed_windows
                .saturating_add(invalid_removed);
        }
    }

    let prune_plan = global_compaction_prune_plan(render_tabs, target_budget, max_prune_per_tick);
    summary.pruned_windows =
        apply_global_compaction_prune_plan(render_tabs, namespace_id, prune_plan);
    summary.total_windows_after = total_window_count(render_tabs);
    summary.has_visible_windows_after = render_tabs.values().any(tab_has_visible_windows);
    summary.has_pending_work_after = has_pending_compaction_work(render_tabs, target_budget);
    summary
}

fn shell_visible_close_indices(tab_windows: &TabWindows) -> Vec<usize> {
    tab_windows
        .windows
        .iter()
        .enumerate()
        .filter_map(|(index, cached)| cached.is_shell_visible().then_some(index))
        .collect()
}

fn close_shell_visible_tab_windows(
    tab_windows: &mut TabWindows,
    namespace_id: NamespaceId,
) -> usize {
    let remove_indices = shell_visible_close_indices(tab_windows);
    if remove_indices.is_empty() {
        return 0;
    }

    let _event_ignore = EventIgnoreGuard::set_all();
    let mut closed_windows = 0_usize;
    for remove_index in remove_indices.into_iter().rev() {
        if remove_cached_window_at(tab_windows, namespace_id, remove_index)
            == TrackedResourceCloseOutcome::ClosedOrGone
        {
            closed_windows = closed_windows.saturating_add(1);
        }
    }
    closed_windows
}

pub(crate) fn close_shell_visible_tab(
    tab_windows: &mut TabWindows,
    namespace_id: NamespaceId,
) -> usize {
    close_shell_visible_tab_windows(tab_windows, namespace_id)
}

pub(crate) fn remove_window_in_tab(
    tab_windows: &mut TabWindows,
    namespace_id: NamespaceId,
    window_id: i32,
) -> TrackedResourceCloseSummary {
    if !tab_windows
        .windows
        .iter()
        .any(|cached| cached.handles.window_id == window_id)
    {
        return TrackedResourceCloseSummary::default();
    }

    #[cfg(not(test))]
    let _event_ignore = EventIgnoreGuard::set_all();

    let mut summary = TrackedResourceCloseSummary::default();
    while let Some(index) = tab_windows
        .windows
        .iter()
        .position(|cached| cached.handles.window_id == window_id)
    {
        let outcome = remove_cached_window_at(tab_windows, namespace_id, index);
        summary.record(outcome);
        if outcome.should_retain() {
            break;
        }
    }
    if summary.closed_or_gone > 0 {
        tab_windows.debug_assert_tracking_consistent();
    }
    summary
}

fn release_unused_tab_windows(
    tab_windows: &mut TabWindows,
    namespace_id: NamespaceId,
) -> ReleaseUnusedSummary {
    let mut summary = ReleaseUnusedSummary::default();
    let mut hide_indices = tab_windows.take_visible_available_indices_for_hide();
    if hide_indices.is_empty() {
        return summary;
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
        let Some(mut buffer) = buffer_from_handle(handles.buffer_id) else {
            let _ = mark_cached_window_invalid(tab_windows, index);
            continue;
        };
        let Some(mut window) = window_from_handle_i32(handles.window_id) else {
            let _ = mark_cached_window_invalid(tab_windows, index);
            continue;
        };

        if crate::draw::clear_namespace_and_hide_floating_window(
            namespace_id,
            &mut buffer,
            &mut window,
            "clear cached render namespace before hide",
            "hide cached render window",
        )
        .is_err()
        {
            let _ = mark_cached_window_invalid(tab_windows, index);
            continue;
        }

        let previous_lifecycle = tab_windows.windows[index].lifecycle;
        let previous_placement = tab_windows.windows[index].placement;
        tab_windows.windows[index].mark_hidden();
        let next_lifecycle = tab_windows.windows[index].lifecycle;
        let next_placement = tab_windows.windows[index].placement;
        tab_windows.track_window_transition(
            index,
            previous_lifecycle,
            previous_placement,
            next_lifecycle,
            next_placement,
        );
        summary.hidden_windows = summary.hidden_windows.saturating_add(1);
    }

    let invalid_removed = remove_invalid_windows(tab_windows, namespace_id);
    if invalid_removed > 0 {
        summary.invalid_removed_windows = summary
            .invalid_removed_windows
            .saturating_add(invalid_removed);
    }
    tab_windows.debug_assert_tracking_consistent();
    summary
}

pub(crate) fn release_unused_in_tab(
    tab_windows: &mut TabWindows,
    namespace_id: NamespaceId,
) -> ReleaseUnusedSummary {
    release_unused_tab_windows(tab_windows, namespace_id)
}

pub(crate) fn recover_invalid_window_in_tab(
    tab_windows: &mut TabWindows,
    namespace_id: NamespaceId,
    window_id: i32,
) -> bool {
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
    tab_windows.debug_assert_tracking_consistent();
    true
}

fn window_buffer(window: &api::Window) -> Option<api::Buffer> {
    let host = NeovimHost;
    if !host.window_is_valid(window) {
        return None;
    }
    match host.window_buffer(window) {
        Ok(buffer) => Some(buffer),
        Err(err) => {
            log_draw_error("window.get_buf", &err);
            None
        }
    }
}

fn buffer_matches_marker(buffer: &api::Buffer, filetype_marker: &str, buftype_marker: &str) -> bool {
    let host = NeovimHost;
    let Ok(filetype) = host.buffer_string_option(buffer, "filetype") else {
        return false;
    };
    if filetype != filetype_marker {
        return false;
    }

    let Ok(buftype) = host.buffer_string_option(buffer, "buftype") else {
        return false;
    };
    buftype == buftype_marker
}

fn buffer_has_smear_marker(buffer: &api::Buffer) -> bool {
    buffer_matches_marker(buffer, RENDER_BUFFER_FILETYPE, RENDER_BUFFER_TYPE)
        || buffer_matches_marker(
            buffer,
            crate::draw::PREPAINT_BUFFER_FILETYPE,
            crate::draw::PREPAINT_BUFFER_TYPE,
        )
}

fn close_orphan_smear_buffer_with(
    host: &impl DrawResourcePort,
    namespace_id: NamespaceId,
    mut buffer: api::Buffer,
) -> TrackedResourceCloseOutcome {
    if let Err(err) = host.clear_buffer_namespace(&mut buffer, namespace_id) {
        log_draw_error("clear orphan smear namespace", &err);
    }
    crate::draw::delete_floating_buffer_with(host, buffer, "delete orphan smear buffer")
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct OrphanSmearResourceCloseSummary {
    pub(crate) closed_windows: usize,
    pub(crate) retained_resources: usize,
}

impl OrphanSmearResourceCloseSummary {
    fn record_retention(&mut self, outcome: TrackedResourceCloseOutcome) {
        if outcome.should_retain() {
            self.retained_resources = self.retained_resources.saturating_add(1);
        }
    }
}

pub(crate) fn close_orphan_smear_resources(
    namespace_id: NamespaceId,
) -> OrphanSmearResourceCloseSummary {
    let host = NeovimHost;
    let _event_ignore = EventIgnoreGuard::set_all_with(&host);
    let mut summary = OrphanSmearResourceCloseSummary::default();
    let mut window_buffer_handles = HashSet::new();
    for window in host.list_windows() {
        let Some(buffer) = window_buffer(&window) else {
            continue;
        };
        if !buffer_has_smear_marker(&buffer) {
            continue;
        }

        window_buffer_handles.insert(BufferHandle::from_buffer(&buffer));
        let window_outcome =
            crate::draw::close_floating_window_with(&host, window, "close orphan smear window");
        if matches!(window_outcome, TrackedResourceCloseOutcome::ClosedOrGone) {
            summary.closed_windows = summary.closed_windows.saturating_add(1);
        }
        let buffer_outcome = close_orphan_smear_buffer_with(&host, namespace_id, buffer);
        summary.record_retention(window_outcome.merge(buffer_outcome));
    }

    for buffer in host.list_buffers() {
        if !host.buffer_is_valid(&buffer) {
            continue;
        }
        if window_buffer_handles.contains(&BufferHandle::from_buffer(&buffer)) {
            continue;
        }
        if !buffer_has_smear_marker(&buffer) {
            continue;
        }

        summary.record_retention(close_orphan_smear_buffer_with(&host, namespace_id, buffer));
    }
    summary
}

pub(crate) fn purge_tab_with_closer(
    tab_windows: &mut TabWindows,
    namespace_id: NamespaceId,
    close_cached_window: &mut impl FnMut(
        NamespaceId,
        WindowBufferHandle,
    ) -> TrackedResourceCloseOutcome,
) -> TrackedResourceCloseSummary {
    #[cfg(not(test))]
    let _event_ignore = EventIgnoreGuard::set_all();

    let drained_tab_windows = std::mem::take(tab_windows);
    let mut summary = TrackedResourceCloseSummary::default();
    for mut cached in drained_tab_windows.windows {
        let outcome = close_cached_window(namespace_id, cached.handles);
        summary.record(outcome);
        if outcome.should_retain() {
            cached.mark_invalid();
            tab_windows.push_cached_window(cached);
        }
    }

    summary
}

pub(crate) fn purge_tab(
    tab_windows: &mut TabWindows,
    namespace_id: NamespaceId,
) -> TrackedResourceCloseSummary {
    let mut close_tracked_window = close_cached_window;
    purge_tab_with_closer(tab_windows, namespace_id, &mut close_tracked_window)
}
