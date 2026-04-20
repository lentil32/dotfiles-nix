//! Common floating window and buffer helpers shared by render and prepaint paths.

use nvim_oxi::Result;
use nvim_oxi::api;
use nvim_oxi::api::opts::BufDeleteOpts;
use nvim_oxi::api::opts::OptionOpts;
use nvim_oxi::api::opts::OptionScope;
use nvim_oxi::api::types::WindowConfig;
use nvim_oxi::api::types::WindowRelativeTo;
use nvim_oxi::api::types::WindowStyle;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct FloatingWindowPlacement {
    pub(crate) row: i64,
    pub(crate) col: i64,
    pub(crate) width: u32,
    pub(crate) zindex: u32,
}

fn build_floating_window_config(
    placement: FloatingWindowPlacement,
    relative_to: WindowRelativeTo,
    style: WindowStyle,
    hidden: bool,
    include_noautocmd: bool,
) -> WindowConfig {
    let mut builder = WindowConfig::builder();
    builder
        .relative(relative_to)
        .row(placement.row as f64 - 1.0)
        .col(placement.col as f64 - 1.0)
        .width(placement.width.max(1))
        .height(1)
        .focusable(false)
        .style(style)
        .hide(hidden)
        .zindex(placement.zindex);
    if include_noautocmd {
        builder.noautocmd(true);
    }
    builder.build()
}

pub(crate) fn open_floating_window_config(
    placement: FloatingWindowPlacement,
    hidden: bool,
    relative_to: WindowRelativeTo,
    style: WindowStyle,
) -> WindowConfig {
    build_floating_window_config(placement, relative_to, style, hidden, true)
}

pub(crate) fn reconfigure_floating_window_config(
    placement: FloatingWindowPlacement,
    hidden: bool,
    relative_to: WindowRelativeTo,
    style: WindowStyle,
) -> WindowConfig {
    build_floating_window_config(placement, relative_to, style, hidden, false)
}

pub(crate) fn open_hidden_floating_window_config(
    zindex: u32,
    relative_to: WindowRelativeTo,
    style: WindowStyle,
) -> WindowConfig {
    let mut builder = WindowConfig::builder();
    builder
        .relative(relative_to)
        .row(0.0)
        .col(0.0)
        .width(1)
        .height(1)
        .focusable(false)
        .style(style)
        .noautocmd(true)
        .hide(true)
        .zindex(zindex);
    builder.build()
}

pub(crate) fn hide_floating_window_config() -> WindowConfig {
    let mut builder = WindowConfig::builder();
    builder.hide(true);
    builder.build()
}

pub(crate) fn set_existing_floating_window_config(
    window: &mut api::Window,
    mut config: WindowConfig,
) -> Result<()> {
    // nvim_win_set_config rejects the `noautocmd` key for existing windows.
    config.noautocmd = None;
    window.set_config(&config)?;
    Ok(())
}

pub(crate) fn clear_namespace_and_hide_floating_window(
    namespace_id: u32,
    buffer: &mut api::Buffer,
    window: &mut api::Window,
    clear_context: &'static str,
    hide_context: &'static str,
) -> Result<()> {
    if let Err(err) = buffer.clear_namespace(namespace_id, 0..) {
        super::context::log_draw_error(clear_context, &err);
        return Err(nvim_oxi::Error::Api(err));
    }
    if let Err(err) = set_existing_floating_window_config(window, hide_floating_window_config()) {
        super::context::log_draw_error(hide_context, &err);
        return Err(err);
    }
    Ok(())
}

pub(crate) fn initialize_floating_buffer_options(
    buffer: &api::Buffer,
    buftype: &str,
    filetype: &str,
) -> Result<()> {
    let opts = OptionOpts::builder().buf(buffer.clone()).build();
    api::set_option_value("buftype", buftype, &opts)?;
    api::set_option_value("filetype", filetype, &opts)?;
    api::set_option_value("bufhidden", "wipe", &opts)?;
    api::set_option_value("swapfile", false, &opts)?;
    Ok(())
}

pub(crate) fn delete_floating_buffer(buffer: api::Buffer, context: &str) {
    if !buffer.is_valid() {
        return;
    }

    let opts = BufDeleteOpts::builder().force(true).build();
    if let Err(err) = buffer.delete(&opts) {
        super::context::log_draw_error(context, &err);
    }
}

pub(crate) fn initialize_floating_window_options(
    window: &api::Window,
    scope: OptionScope,
) -> Result<()> {
    let opts = OptionOpts::builder()
        .scope(scope)
        .win(window.clone())
        .build();
    api::set_option_value("winhighlight", "NormalFloat:Normal", &opts)?;
    api::set_option_value("winblend", 100_i64, &opts)?;
    Ok(())
}
