fn window_from_handle_i32(handle: i32) -> Option<api::Window> {
    NeovimHost.valid_window_i32(handle)
}

fn buffer_from_handle(handle: BufferHandle) -> Option<api::Buffer> {
    NeovimHost.valid_buffer(handle)
}

fn window_from_handle_i32_unchecked(handle: i32) -> api::Window {
    NeovimHost.window_from_handle_i32_unchecked(handle)
}

fn buffer_from_handle_unchecked(handle: BufferHandle) -> Option<api::Buffer> {
    NeovimHost.buffer_from_handle_unchecked(handle)
}

fn open_hidden_window_config() -> WindowConfig {
    crate::draw::open_hidden_floating_window_config(
        1,
        WindowRelativeTo::Editor,
        WindowStyle::Minimal,
    )
}

fn reconfigure_window_config(row: i64, col: i64, width: u32, zindex: u32) -> WindowConfig {
    crate::draw::reconfigure_floating_window_config(
        crate::draw::FloatingWindowPlacement {
            row,
            col,
            width,
            zindex,
        },
        crate::draw::FloatingWindowVisibility::Visible,
        WindowRelativeTo::Editor,
        WindowStyle::Minimal,
    )
}

fn set_existing_window_config(window: &mut api::Window, config: WindowConfig) -> Result<()> {
    crate::draw::set_existing_floating_window_config_with(&NeovimHost, window, config)
}

fn initialize_buffer_options(buffer: &api::Buffer) -> Result<()> {
    crate::draw::initialize_floating_buffer_options_with(
        &NeovimHost,
        buffer,
        RENDER_BUFFER_TYPE,
        RENDER_BUFFER_FILETYPE,
    )
}

fn initialize_window_options(window: &api::Window) -> Result<()> {
    crate::draw::initialize_floating_window_options_with(&NeovimHost, window, OptionScope::Local)
}

fn close_cached_window(
    namespace_id: NamespaceId,
    handles: WindowBufferHandle,
) -> TrackedResourceCloseOutcome {
    let mut outcome = TrackedResourceCloseOutcome::ClosedOrGone;
    let host = NeovimHost;
    let mut buffer = buffer_from_handle(handles.buffer_id);
    if let Some(existing_buffer) = buffer.as_mut()
        && let Err(err) = host.clear_buffer_namespace(existing_buffer, namespace_id)
    {
        log_draw_error("clear cached render namespace", &err);
    }
    if let Some(window) = window_from_handle_i32(handles.window_id) {
        outcome = outcome.merge(crate::draw::close_floating_window_with(
            &host,
            window,
            "close cached render window",
        ));
    }
    if let Some(buffer) = buffer.take() {
        outcome = outcome.merge(crate::draw::delete_floating_buffer_with(
            &host,
            buffer,
            "delete cached render buffer",
        ));
    }
    outcome
}
