#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct BoundedCloseSummary {
    attempted: usize,
    closed_or_gone: usize,
    retained: usize,
}

impl BoundedCloseSummary {
    fn record(&mut self, outcome: TrackedResourceCloseOutcome) {
        self.attempted = self.attempted.saturating_add(1);
        if outcome == TrackedResourceCloseOutcome::ClosedOrGone {
            self.closed_or_gone = self.closed_or_gone.saturating_add(1);
        } else {
            self.retained = self.retained.saturating_add(1);
        }
    }
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

fn has_compaction_close_candidate(
    render_tabs: &std::collections::HashMap<TabHandle, TabWindows>,
    target_budget: usize,
) -> bool {
    render_tabs.values().any(|tab_windows| {
        tab_has_visible_windows(tab_windows) || tab_windows.has_invalid_windows()
    }) || (total_window_count(render_tabs) > target_budget
        && render_tabs
            .values()
            .any(|tab_windows| !tab_windows.reusable_window_indices.is_empty()))
}

fn global_compaction_prune_plan(
    render_tabs: &std::collections::HashMap<TabHandle, TabWindows>,
    target_budget: usize,
    max_teardown_attempts_per_tick: usize,
) -> std::collections::HashMap<TabHandle, Vec<usize>> {
    let total_windows = total_window_count(render_tabs);
    if total_windows <= target_budget || max_teardown_attempts_per_tick == 0 {
        return std::collections::HashMap::new();
    }

    let prune_goal = total_windows
        .saturating_sub(target_budget)
        .min(max_teardown_attempts_per_tick);
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
    for (_, tab_handle, index) in selected_candidates {
        plan.entry(tab_handle).or_default().push(index);
    }
    for indices in plan.values_mut() {
        indices.sort_unstable();
    }
    plan
}

fn apply_global_compaction_prune_plan<C>(
    render_tabs: &mut std::collections::HashMap<TabHandle, TabWindows>,
    namespace_id: NamespaceId,
    prune_plan: std::collections::HashMap<TabHandle, Vec<usize>>,
    close_cached: &mut C,
) -> BoundedCloseSummary
where
    C: FnMut(&mut TabWindows, NamespaceId, usize) -> TrackedResourceCloseOutcome,
{
    let mut summary = BoundedCloseSummary::default();
    let mut tab_handles = prune_plan.keys().copied().collect::<Vec<_>>();
    tab_handles.sort_unstable();

    for tab_handle in tab_handles {
        let Some(tab_windows) = render_tabs.get_mut(&tab_handle) else {
            continue;
        };
        let Some(indices) = prune_plan.get(&tab_handle) else {
            continue;
        };
        let mut attempted_any = false;
        for remove_index in indices.iter().copied().rev() {
            if remove_index >= tab_windows.windows.len() {
                continue;
            }
            let outcome = close_cached(tab_windows, namespace_id, remove_index);
            summary.record(outcome);
            attempted_any = true;
            if outcome.should_retain() {
                break;
            }
        }
        if attempted_any {
            tab_windows.debug_assert_tracking_consistent();
        }
        if summary.retained > 0 {
            break;
        }
    }

    summary
}

fn close_indices_up_to<C>(
    tab_windows: &mut TabWindows,
    namespace_id: NamespaceId,
    remove_indices: impl IntoIterator<Item = usize>,
    close_cached: &mut C,
) -> BoundedCloseSummary
where
    C: FnMut(&mut TabWindows, NamespaceId, usize) -> TrackedResourceCloseOutcome,
{
    let mut remove_indices = remove_indices.into_iter().collect::<Vec<_>>();
    remove_indices.sort_unstable();
    remove_indices.dedup();

    let mut summary = BoundedCloseSummary::default();
    for remove_index in remove_indices.into_iter().rev() {
        if remove_index >= tab_windows.windows.len() {
            continue;
        }
        let outcome = close_cached(tab_windows, namespace_id, remove_index);
        summary.record(outcome);
        if outcome.should_retain() {
            break;
        }
    }
    if summary.attempted > 0 {
        tab_windows.debug_assert_tracking_consistent();
    }
    summary
}

fn close_visible_windows_up_to<C>(
    render_tabs: &mut std::collections::HashMap<TabHandle, TabWindows>,
    namespace_id: NamespaceId,
    max_attempts: usize,
    close_cached: &mut C,
) -> BoundedCloseSummary
where
    C: FnMut(&mut TabWindows, NamespaceId, usize) -> TrackedResourceCloseOutcome,
{
    let mut summary = BoundedCloseSummary::default();
    let mut tab_handles = render_tabs.keys().copied().collect::<Vec<_>>();
    tab_handles.sort_unstable();

    for tab_handle in tab_handles {
        let remaining = max_attempts.saturating_sub(summary.attempted);
        if remaining == 0 {
            break;
        }
        let Some(tab_windows) = render_tabs.get_mut(&tab_handle) else {
            continue;
        };
        let remove_indices = tab_windows
            .windows
            .iter()
            .enumerate()
            .filter_map(|(index, cached)| cached.is_shell_visible().then_some(index))
            .take(remaining)
            .collect::<Vec<_>>();
        let tab_summary =
            close_indices_up_to(tab_windows, namespace_id, remove_indices, close_cached);
        summary.attempted = summary.attempted.saturating_add(tab_summary.attempted);
        summary.closed_or_gone = summary
            .closed_or_gone
            .saturating_add(tab_summary.closed_or_gone);
        summary.retained = summary.retained.saturating_add(tab_summary.retained);
        if tab_summary.retained > 0 {
            break;
        }
    }

    summary
}

fn remove_invalid_windows_up_to<C>(
    render_tabs: &mut std::collections::HashMap<TabHandle, TabWindows>,
    namespace_id: NamespaceId,
    max_attempts: usize,
    close_cached: &mut C,
) -> BoundedCloseSummary
where
    C: FnMut(&mut TabWindows, NamespaceId, usize) -> TrackedResourceCloseOutcome,
{
    let mut summary = BoundedCloseSummary::default();
    let mut tab_handles = render_tabs.keys().copied().collect::<Vec<_>>();
    tab_handles.sort_unstable();

    for tab_handle in tab_handles {
        let remaining = max_attempts.saturating_sub(summary.attempted);
        if remaining == 0 {
            break;
        }
        let Some(tab_windows) = render_tabs.get_mut(&tab_handle) else {
            continue;
        };
        let remove_indices = tab_windows
            .windows
            .iter()
            .enumerate()
            .filter_map(|(index, cached)| {
                matches!(cached.lifecycle, CachedWindowLifecycle::Invalid).then_some(index)
            })
            .take(remaining)
            .collect::<Vec<_>>();
        let tab_summary =
            close_indices_up_to(tab_windows, namespace_id, remove_indices, close_cached);
        summary.attempted = summary.attempted.saturating_add(tab_summary.attempted);
        summary.closed_or_gone = summary
            .closed_or_gone
            .saturating_add(tab_summary.closed_or_gone);
        summary.retained = summary.retained.saturating_add(tab_summary.retained);
        if tab_summary.retained > 0 {
            break;
        }
    }

    summary
}

fn compact_tabs_to_budget_with_closer<C>(
    render_tabs: &mut std::collections::HashMap<TabHandle, TabWindows>,
    namespace_id: NamespaceId,
    target_budget: usize,
    max_teardown_attempts_per_tick: usize,
    close_cached: &mut C,
) -> CompactRenderWindowsSummary
where
    C: FnMut(&mut TabWindows, NamespaceId, usize) -> TrackedResourceCloseOutcome,
{
    let mut summary = CompactRenderWindowsSummary {
        target_budget,
        ..CompactRenderWindowsSummary::default()
    };
    let visible_summary = close_visible_windows_up_to(
        render_tabs,
        namespace_id,
        max_teardown_attempts_per_tick,
        close_cached,
    );
    summary.closed_visible_windows = visible_summary.closed_or_gone;
    let remaining_after_visible = if visible_summary.retained > 0 {
        0
    } else {
        max_teardown_attempts_per_tick.saturating_sub(visible_summary.attempted)
    };

    let invalid_summary = remove_invalid_windows_up_to(
        render_tabs,
        namespace_id,
        remaining_after_visible,
        close_cached,
    );
    summary.invalid_removed_windows = invalid_summary.closed_or_gone;
    // Window teardown can succeed while buffer deletion fails, which aggregates to Retained. Any
    // visible/invalid attempt may therefore have removed a host float and needs a redraw.
    summary.potential_visual_change = visible_summary.attempted > 0 || invalid_summary.attempted > 0;
    let remaining_after_invalid = if invalid_summary.retained > 0 {
        0
    } else {
        remaining_after_visible.saturating_sub(invalid_summary.attempted)
    };

    let prune_plan =
        global_compaction_prune_plan(render_tabs, target_budget, remaining_after_invalid);
    let prune_summary = apply_global_compaction_prune_plan(
        render_tabs,
        namespace_id,
        prune_plan,
        close_cached,
    );
    summary.pruned_windows = prune_summary.closed_or_gone;
    summary.teardown_attempts = visible_summary
        .attempted
        .saturating_add(invalid_summary.attempted)
        .saturating_add(prune_summary.attempted);
    summary.close_stalled = visible_summary.retained > 0
        || invalid_summary.retained > 0
        || prune_summary.retained > 0;

    debug_assert!(
        summary.teardown_attempts <= max_teardown_attempts_per_tick,
        "render compaction exceeded its per-tick resource teardown attempt budget"
    );

    summary.total_windows_after = total_window_count(render_tabs);
    summary.has_visible_windows_after = render_tabs.values().any(tab_has_visible_windows);
    summary.has_pending_work_after = has_pending_compaction_work(render_tabs, target_budget);
    summary
}

pub(crate) fn compact_tabs_to_budget(
    render_tabs: &mut std::collections::HashMap<TabHandle, TabWindows>,
    namespace_id: NamespaceId,
    target_budget: usize,
    max_teardown_attempts_per_tick: usize,
) -> CompactRenderWindowsSummary {
    let should_attempt_close = max_teardown_attempts_per_tick > 0
        && has_compaction_close_candidate(render_tabs, target_budget);
    let _event_ignore = should_attempt_close.then(EventIgnoreGuard::set_all);
    compact_tabs_to_budget_with_closer(
        render_tabs,
        namespace_id,
        target_budget,
        max_teardown_attempts_per_tick,
        &mut remove_cached_window_at,
    )
}
