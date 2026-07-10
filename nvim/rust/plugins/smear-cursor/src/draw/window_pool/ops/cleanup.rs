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

fn hide_unused_tab_windows_with(
    host: &impl DrawResourcePort,
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
        let Some(mut buffer) = host.valid_buffer(handles.buffer_id) else {
            let _ = mark_cached_window_invalid(tab_windows, index);
            continue;
        };
        let Some(mut window) = host.valid_window_i32(handles.window_id) else {
            let _ = mark_cached_window_invalid(tab_windows, index);
            continue;
        };

        if crate::draw::clear_namespace_and_hide_floating_window_with(
            host,
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

    tab_windows.debug_assert_tracking_consistent();
    summary
}

fn hide_unused_tab_windows(
    tab_windows: &mut TabWindows,
    namespace_id: NamespaceId,
) -> ReleaseUnusedSummary {
    hide_unused_tab_windows_with(&NeovimHost, tab_windows, namespace_id)
}

pub(crate) fn release_unused_in_tab(
    tab_windows: &mut TabWindows,
    namespace_id: NamespaceId,
) -> ReleaseUnusedSummary {
    let mut summary = hide_unused_tab_windows(tab_windows, namespace_id);
    let invalid_removed = remove_invalid_windows(tab_windows, namespace_id);
    summary.invalid_removed_windows = summary
        .invalid_removed_windows
        .saturating_add(invalid_removed);
    summary
}

pub(crate) fn hide_unused_in_tab_for_cleanup_with(
    host: &impl DrawResourcePort,
    tab_windows: &mut TabWindows,
    namespace_id: NamespaceId,
) -> ReleaseUnusedSummary {
    hide_unused_tab_windows_with(host, tab_windows, namespace_id)
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

pub(crate) fn purge_tab_with_closer(
    tab_windows: &mut TabWindows,
    namespace_id: NamespaceId,
    close_cached_window: &mut impl FnMut(NamespaceId, WindowBufferHandle) -> TrackedResourceCloseOutcome,
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
