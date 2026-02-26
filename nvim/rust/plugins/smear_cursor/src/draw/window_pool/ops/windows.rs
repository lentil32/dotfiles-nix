fn window_from_handle_i32(handle: i32) -> Option<api::Window> {
    handles::valid_window(i64::from(handle))
}

fn buffer_from_handle_i32(handle: i32) -> Option<api::Buffer> {
    handles::valid_buffer(i64::from(handle))
}

fn window_from_handle_i32_unchecked(handle: i32) -> api::Window {
    api::Window::from(handle)
}

fn buffer_from_handle_i32_unchecked(handle: i32) -> api::Buffer {
    api::Buffer::from(handle)
}

fn open_hidden_window_config() -> WindowConfig {
    let mut builder = WindowConfig::builder();
    builder
        .relative(WindowRelativeTo::Editor)
        .row(0.0)
        .col(0.0)
        .width(1)
        .height(1)
        .focusable(false)
        .style(WindowStyle::Minimal)
        .noautocmd(true)
        .hide(true)
        .zindex(1);
    builder.build()
}

fn reconfigure_window_config(row: i64, col: i64, width: u32, zindex: u32) -> WindowConfig {
    let mut builder = WindowConfig::builder();
    builder
        .relative(WindowRelativeTo::Editor)
        .row(row as f64 - 1.0)
        .col(col as f64 - 1.0)
        .width(width.max(1))
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

fn set_existing_window_config(window: &mut api::Window, mut config: WindowConfig) -> Result<()> {
    // nvim_win_set_config rejects the `noautocmd` key for existing windows.
    config.noautocmd = None;
    window.set_config(&config)?;
    Ok(())
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
