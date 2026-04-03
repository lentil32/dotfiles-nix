fn window_from_handle_i32(handle: i32) -> Option<api::Window> {
    if handle <= 0 {
        return None;
    }
    handles::valid_window(i64::from(handle))
}

fn buffer_from_handle_i32(handle: i32) -> Option<api::Buffer> {
    if handle <= 0 {
        return None;
    }
    handles::valid_buffer(i64::from(handle))
}

fn window_from_handle_i32_unchecked(handle: i32) -> api::Window {
    api::Window::from(handle)
}

fn buffer_from_handle_i32_unchecked(handle: i32) -> api::Buffer {
    api::Buffer::from(handle)
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
        false,
        WindowRelativeTo::Editor,
        WindowStyle::Minimal,
    )
}

fn hide_window_config() -> WindowConfig {
    crate::draw::hide_floating_window_config()
}

fn set_existing_window_config(window: &mut api::Window, config: WindowConfig) -> Result<()> {
    crate::draw::set_existing_floating_window_config(window, config)
}

fn initialize_buffer_options(buffer: &api::Buffer) -> Result<()> {
    crate::draw::initialize_floating_buffer_options(
        buffer,
        RENDER_BUFFER_TYPE,
        RENDER_BUFFER_FILETYPE,
    )
}

fn initialize_window_options(window: &api::Window) -> Result<()> {
    crate::draw::initialize_floating_window_options(window, OptionScope::Local)
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
