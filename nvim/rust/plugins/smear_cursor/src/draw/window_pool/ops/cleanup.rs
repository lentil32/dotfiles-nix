fn prune_tab_windows(
    tab_windows: &mut TabWindows,
    namespace_id: u32,
    max_kept_windows: usize,
) -> usize {
    let keep_budget = effective_keep_budget(tab_windows.cached_budget, max_kept_windows);
    if tab_windows.windows.len() <= keep_budget {
        if remove_invalid_windows(tab_windows, namespace_id) > 0 {
            rebuild_available_window_placement_index(tab_windows);
        }
        return 0;
    }

    let mut pruned_windows = 0_usize;
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

    pruned_windows
}

pub(crate) fn prune(
    tabs: &mut HashMap<i32, TabWindows>,
    namespace_id: u32,
    max_kept_windows: usize,
) -> usize {
    let mut pruned_windows = 0_usize;
    for tab_windows in tabs.values_mut() {
        pruned_windows = pruned_windows.saturating_add(prune_tab_windows(
            tab_windows,
            namespace_id,
            max_kept_windows,
        ));
    }
    pruned_windows
}

fn release_unused_tab_windows(
    tab_windows: &mut TabWindows,
    namespace_id: u32,
) -> ReleaseUnusedSummary {
    let hide_config = hide_window_config();
    let mut summary = ReleaseUnusedSummary::default();
    let mut hide_indices = std::mem::take(&mut tab_windows.visible_available_indices);
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
        let Some(mut window) = window_from_handle_i32(handles.window_id) else {
            let _ = mark_cached_window_invalid(tab_windows, index);
            continue;
        };

        if set_existing_window_config(&mut window, hide_config.clone()).is_err() {
            let _ = mark_cached_window_invalid(tab_windows, index);
            continue;
        }

        tab_windows.windows[index].mark_hidden();
        summary.hidden_windows = summary.hidden_windows.saturating_add(1);
    }

    let invalid_removed = remove_invalid_windows(tab_windows, namespace_id);
    if invalid_removed > 0 {
        summary.invalid_removed_windows = summary
            .invalid_removed_windows
            .saturating_add(invalid_removed);
        rebuild_available_window_placement_index(tab_windows);
    }
    summary
}

pub(crate) fn release_unused_tab(
    tabs: &mut HashMap<i32, TabWindows>,
    namespace_id: u32,
    tab_handle: i32,
) -> ReleaseUnusedSummary {
    let Some(tab_windows) = tabs.get_mut(&tab_handle) else {
        return ReleaseUnusedSummary::default();
    };
    release_unused_tab_windows(tab_windows, namespace_id)
}

pub(crate) fn release_unused(
    tabs: &mut HashMap<i32, TabWindows>,
    namespace_id: u32,
) -> ReleaseUnusedSummary {
    let mut summary = ReleaseUnusedSummary::default();
    for tab_windows in tabs.values_mut() {
        let tab_summary = release_unused_tab_windows(tab_windows, namespace_id);
        summary.hidden_windows = summary
            .hidden_windows
            .saturating_add(tab_summary.hidden_windows);
        summary.invalid_removed_windows = summary
            .invalid_removed_windows
            .saturating_add(tab_summary.invalid_removed_windows);
    }

    summary
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

