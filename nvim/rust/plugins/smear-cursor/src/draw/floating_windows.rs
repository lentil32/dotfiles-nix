//! Common floating window and buffer helpers shared by render and prepaint paths.

use super::resource_close::TrackedResourceCloseOutcome;
use crate::host::DrawResourcePort;
use crate::host::NamespaceId;
use crate::host::NeovimHost;
use crate::host::api;
use crate::host::api::opts::OptionScope;
use crate::host::api::types::WindowConfig;
use crate::host::api::types::WindowRelativeTo;
use crate::host::api::types::WindowStyle;
use nvim_oxi::Result;

pub(crate) struct EventIgnoreGuard<'a, H: DrawResourcePort + ?Sized> {
    previous: Option<String>,
    host: &'a H,
}

impl EventIgnoreGuard<'static, NeovimHost> {
    pub(crate) fn set_all() -> Self {
        static HOST: NeovimHost = NeovimHost;
        Self::set_all_with(&HOST)
    }
}

impl<'a, H> EventIgnoreGuard<'a, H>
where
    H: DrawResourcePort + ?Sized,
{
    pub(crate) fn set_all_with(host: &'a H) -> Self {
        let previous = host.eventignore().ok();
        if let Err(err) = host.set_eventignore("all") {
            super::context::log_draw_error("set eventignore=all", &err);
        }
        Self { previous, host }
    }
}

impl<H> Drop for EventIgnoreGuard<'_, H>
where
    H: DrawResourcePort + ?Sized,
{
    fn drop(&mut self) {
        let Some(previous) = self.previous.take() else {
            return;
        };
        if let Err(err) = self.host.set_eventignore(&previous) {
            super::context::log_draw_error("restore eventignore", &err);
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct FloatingWindowPlacement {
    pub(crate) row: i64,
    pub(crate) col: i64,
    pub(crate) width: u32,
    pub(crate) zindex: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FloatingWindowVisibility {
    Visible,
    Hidden,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FloatingWindowAutocmdPolicy {
    SuppressAutocmds,
    AllowAutocmds,
}

fn build_floating_window_config(
    placement: FloatingWindowPlacement,
    relative_to: WindowRelativeTo,
    style: WindowStyle,
    visibility: FloatingWindowVisibility,
    autocmd_policy: FloatingWindowAutocmdPolicy,
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
        .hide(matches!(visibility, FloatingWindowVisibility::Hidden))
        .zindex(placement.zindex);
    if matches!(
        autocmd_policy,
        FloatingWindowAutocmdPolicy::SuppressAutocmds
    ) {
        builder.noautocmd(true);
    }
    builder.build()
}

pub(crate) fn open_floating_window_config(
    placement: FloatingWindowPlacement,
    visibility: FloatingWindowVisibility,
    relative_to: WindowRelativeTo,
    style: WindowStyle,
) -> WindowConfig {
    build_floating_window_config(
        placement,
        relative_to,
        style,
        visibility,
        FloatingWindowAutocmdPolicy::SuppressAutocmds,
    )
}

pub(crate) fn reconfigure_floating_window_config(
    placement: FloatingWindowPlacement,
    visibility: FloatingWindowVisibility,
    relative_to: WindowRelativeTo,
    style: WindowStyle,
) -> WindowConfig {
    build_floating_window_config(
        placement,
        relative_to,
        style,
        visibility,
        FloatingWindowAutocmdPolicy::AllowAutocmds,
    )
}

pub(crate) fn open_hidden_floating_window_config(
    zindex: u32,
    relative_to: WindowRelativeTo,
    style: WindowStyle,
) -> WindowConfig {
    build_floating_window_config(
        FloatingWindowPlacement {
            row: 1,
            col: 1,
            width: 1,
            zindex,
        },
        relative_to,
        style,
        FloatingWindowVisibility::Hidden,
        FloatingWindowAutocmdPolicy::SuppressAutocmds,
    )
}

pub(crate) fn hide_floating_window_config() -> WindowConfig {
    let mut builder = WindowConfig::builder();
    builder.hide(true);
    builder.build()
}

pub(crate) fn set_existing_floating_window_config_with(
    host: &impl DrawResourcePort,
    window: &mut api::Window,
    mut config: WindowConfig,
) -> Result<()> {
    // nvim_win_set_config rejects the `noautocmd` key for existing windows.
    config.noautocmd = None;
    host.set_window_config(window, &config)
}

pub(crate) fn clear_namespace_and_hide_floating_window(
    namespace_id: NamespaceId,
    buffer: &mut api::Buffer,
    window: &mut api::Window,
    clear_context: &'static str,
    hide_context: &'static str,
) -> Result<()> {
    clear_namespace_and_hide_floating_window_with(
        &NeovimHost,
        namespace_id,
        buffer,
        window,
        clear_context,
        hide_context,
    )
}

pub(crate) fn clear_namespace_and_hide_floating_window_with(
    host: &impl DrawResourcePort,
    namespace_id: NamespaceId,
    buffer: &mut api::Buffer,
    window: &mut api::Window,
    clear_context: &'static str,
    hide_context: &'static str,
) -> Result<()> {
    if let Err(err) = host.clear_buffer_namespace(buffer, namespace_id) {
        super::context::log_draw_error(clear_context, &err);
        return Err(err);
    }
    if let Err(err) =
        set_existing_floating_window_config_with(host, window, hide_floating_window_config())
    {
        super::context::log_draw_error(hide_context, &err);
        return Err(err);
    }
    Ok(())
}

pub(crate) fn initialize_floating_buffer_options_with(
    host: &impl DrawResourcePort,
    buffer: &api::Buffer,
    buftype: &str,
    filetype: &str,
) -> Result<()> {
    host.set_buffer_string_option(buffer, "buftype", buftype)?;
    host.set_buffer_string_option(buffer, "filetype", filetype)?;
    host.set_buffer_string_option(buffer, "bufhidden", "wipe")?;
    host.set_buffer_bool_option(buffer, "swapfile", false)?;
    Ok(())
}

pub(crate) fn delete_floating_buffer(
    buffer: api::Buffer,
    context: &str,
) -> TrackedResourceCloseOutcome {
    delete_floating_buffer_with(&NeovimHost, buffer, context)
}

pub(crate) fn delete_floating_buffer_with(
    host: &impl DrawResourcePort,
    buffer: api::Buffer,
    context: &str,
) -> TrackedResourceCloseOutcome {
    if !host.buffer_is_valid(&buffer) {
        return TrackedResourceCloseOutcome::ClosedOrGone;
    }

    let _event_ignore = EventIgnoreGuard::set_all_with(host);
    match host.delete_buffer_force(buffer) {
        Ok(()) => TrackedResourceCloseOutcome::ClosedOrGone,
        Err(err) => {
            super::context::log_draw_error(context, &err);
            TrackedResourceCloseOutcome::Retained
        }
    }
}

pub(crate) fn close_floating_window(
    window: api::Window,
    context: &str,
) -> TrackedResourceCloseOutcome {
    close_floating_window_with(&NeovimHost, window, context)
}

pub(crate) fn close_floating_window_with(
    host: &impl DrawResourcePort,
    window: api::Window,
    context: &str,
) -> TrackedResourceCloseOutcome {
    if !host.window_is_valid(&window) {
        return TrackedResourceCloseOutcome::ClosedOrGone;
    }

    let _event_ignore = EventIgnoreGuard::set_all_with(host);
    match host.close_window_force(window) {
        Ok(()) => TrackedResourceCloseOutcome::ClosedOrGone,
        Err(err) => {
            super::context::log_draw_error(context, &err);
            TrackedResourceCloseOutcome::Retained
        }
    }
}

pub(crate) fn initialize_floating_window_options_with(
    host: &impl DrawResourcePort,
    window: &api::Window,
    scope: OptionScope,
) -> Result<()> {
    host.set_window_string_option(window, scope, "winhighlight", "NormalFloat:Normal")?;
    host.set_window_i64_option(window, scope, "winblend", 100_i64)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::EventIgnoreGuard;
    use crate::host::DrawResourceCall;
    use crate::host::FakeDrawResourcePort;
    use pretty_assertions::assert_eq;

    #[test]
    fn eventignore_guard_suppresses_and_restores_through_draw_resource_port() {
        let host = FakeDrawResourcePort::default();
        host.push_eventignore("old-eventignore");

        {
            let _guard = EventIgnoreGuard::set_all_with(&host);
        }

        assert_eq!(
            host.calls(),
            vec![
                DrawResourceCall::Eventignore,
                DrawResourceCall::SetEventignore {
                    value: "all".to_owned()
                },
                DrawResourceCall::SetEventignore {
                    value: "old-eventignore".to_owned()
                },
            ]
        );
    }
}
