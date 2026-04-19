//! Plugin lifecycle orchestration for setup, teardown, and toggle boundaries.
//!
//! This module keeps Neovim callbacks, host-bridge state, and runtime state in
//! lockstep so setup failures degrade into an explicitly disabled runtime
//! instead of leaving stale callbacks behind.

use super::AUTOCMD_GROUP_NAME;
use super::cursor::current_mode;
use super::cursor::cursor_position_for_mode;
use super::handlers::cursor_location_for_core_render;
use super::host_bridge::ensure_namespace_id;
use super::host_bridge::installed_host_bridge;
use super::host_bridge::verify_host_bridge;
use super::ingress::registered_autocmd_event_names;
use super::logging::debug;
use super::logging::ensure_hideable_guicursor;
use super::logging::invalidate_real_cursor_visibility;
use super::logging::set_log_level;
use super::logging::unhide_real_cursor;
use super::logging::warn;
use super::options::apply_runtime_options;
use super::runtime::diagnostics_report;
use super::runtime::mutate_engine_state;
use super::runtime::read_engine_state;
use super::runtime::refresh_editor_viewport_cache;
use super::runtime::reset_transient_event_state;
use crate::draw::clear_highlight_cache;
use crate::draw::initialize_runtime_capabilities;
use crate::draw::purge_render_windows;
use crate::state::CursorShape;
use crate::types::Point;
use nvim_oxi::Dictionary;
use nvim_oxi::Result;
use nvim_oxi::String as NvimString;
use nvim_oxi::api;
use nvim_oxi::api::opts::CreateAugroupOpts;
use nvim_oxi::api::opts::CreateAutocmdOpts;
use nvim_oxi::api::opts::CreateCommandOpts;
use nvim_oxi::api::types::AutocmdCallbackArgs;

fn jump_to_current_cursor() -> Result<()> {
    let namespace_id = ensure_namespace_id()?;
    let mode = current_mode();
    let mode = mode.to_string_lossy();
    let window = api::get_current_win();
    if !window.is_valid() {
        return Ok(());
    }

    let buffer = api::get_current_buf();
    if !buffer.is_valid() {
        return Ok(());
    }

    let smear_to_cmd = read_engine_state(|state| state.core_state().runtime().config.smear_to_cmd)?;
    let Some((row, col)) = cursor_position_for_mode(&window, mode.as_ref(), smear_to_cmd)? else {
        return Ok(());
    };

    let location = cursor_location_for_core_render(Some(&window), Some(&buffer), None, None);

    let hide_target_hack = mutate_engine_state(|state| {
        state.shell.set_namespace_id(namespace_id);
        let runtime = state.core_state_mut().runtime_mut();
        let cursor_shape = CursorShape::new(
            runtime.config.cursor_is_vertical_bar(mode.as_ref()),
            runtime.config.cursor_is_horizontal_bar(mode.as_ref()),
        );
        runtime.sync_to_current_cursor(Point { row, col }, cursor_shape, &location);
        runtime.config.hide_target_hack
    })?;

    reset_transient_event_state();
    let _ = purge_render_windows(namespace_id);
    if !hide_target_hack {
        unhide_real_cursor();
    }

    Ok(())
}

fn clear_autocmd_group() {
    let opts = CreateAugroupOpts::builder().clear(true).build();
    if let Err(err) = api::create_augroup(AUTOCMD_GROUP_NAME, &opts) {
        warn(&format!("clear autocmd group failed: {err}"));
    }
}

fn dispatch_registered_autocmd(event: &str) -> Result<bool> {
    crate::guard_plugin_call("on_autocmd", || super::handlers::on_autocmd_event(event))?;
    Ok(false)
}

fn dispatch_registered_autocmd_callback(args: AutocmdCallbackArgs) -> Result<bool> {
    crate::guard_plugin_call("on_autocmd", || super::handlers::on_autocmd_callback(args))?;
    Ok(false)
}

fn setup_autocmds() -> Result<()> {
    let group = api::create_augroup(
        AUTOCMD_GROUP_NAME,
        &CreateAugroupOpts::builder().clear(true).build(),
    )?;

    for event in registered_autocmd_event_names() {
        let opts = CreateAutocmdOpts::builder()
            .group(group)
            // Keep the bridge in Rust so autocmd wakeups avoid reparsing a Lua command string.
            .callback(dispatch_registered_autocmd_callback)
            .build();
        api::create_autocmd([event], &opts)?;
    }

    Ok(())
}

fn setup_user_command() -> Result<()> {
    if let Err(err) = api::del_user_command("SmearCursorToggle") {
        debug(&format!(
            "delete existing SmearCursorToggle failed (continuing): {err}"
        ));
    }
    if let Err(err) = api::del_user_command("SmearCursorDiagnostics") {
        debug(&format!(
            "delete existing SmearCursorDiagnostics failed (continuing): {err}"
        ));
    }
    api::create_user_command(
        "SmearCursorToggle",
        "lua require('nvimrs_smear_cursor').toggle()",
        &CreateCommandOpts::builder().build(),
    )?;
    api::create_user_command(
        "SmearCursorDiagnostics",
        "lua print(require('nvimrs_smear_cursor').diagnostics())",
        &CreateCommandOpts::builder().build(),
    )?;
    Ok(())
}

/// Applies runtime options and installs the event bridge for the plugin session.
pub(crate) fn setup(opts: &Dictionary) -> Result<()> {
    let _host_bridge = verify_host_bridge()?;
    let namespace_id = ensure_namespace_id()?;
    initialize_runtime_capabilities()?;
    ensure_hideable_guicursor();
    invalidate_real_cursor_visibility();
    unhide_real_cursor();
    clear_highlight_cache();

    mutate_engine_state(|state| {
        state.shell.set_namespace_id(namespace_id);
        state.core_state_mut().runtime_mut().disable();
    })?;
    refresh_editor_viewport_cache()?;
    clear_autocmd_group();
    reset_transient_event_state();

    let has_enabled_option = opts.get(&NvimString::from("enabled")).is_some();
    let (enabled, setup_warning) = mutate_engine_state(|state| {
        state.shell.set_namespace_id(namespace_id);
        let runtime = state.core_state_mut().runtime_mut();
        if !has_enabled_option {
            runtime.set_enabled(true);
        }
        match apply_runtime_options(runtime, opts) {
            Ok(()) => {
                set_log_level(runtime.config.logging_level);
                runtime.clear_runtime_state();
                (runtime.is_enabled(), None)
            }
            Err(err) => {
                runtime.disable();
                (
                    false,
                    Some(format!(
                        "setup rejected options; smear cursor remains disabled: {err}"
                    )),
                )
            }
        }
    })?;

    setup_user_command()?;
    if enabled {
        setup_autocmds()?;
    }
    jump_to_current_cursor()?;
    if let Some(message) = setup_warning {
        warn(&message);
    }
    Ok(())
}

pub(crate) fn toggle() -> Result<()> {
    let _host_bridge = installed_host_bridge()?;
    let (is_enabled, namespace_id, hide_target_hack) = mutate_engine_state(|state| {
        let namespace_id = state.shell.namespace_id();
        let runtime = state.core_state_mut().runtime_mut();
        let toggled_enabled = !runtime.is_enabled();
        if toggled_enabled {
            runtime.set_enabled(true);
        } else {
            runtime.disable();
        }
        (
            runtime.is_enabled(),
            namespace_id,
            runtime.config.hide_target_hack,
        )
    })?;

    if let Some(namespace_id) = namespace_id {
        if is_enabled {
            refresh_editor_viewport_cache()?;
            setup_autocmds()?;
            jump_to_current_cursor()?;
        } else {
            clear_autocmd_group();
            reset_transient_event_state();
            let _ = purge_render_windows(namespace_id);
            if !hide_target_hack {
                unhide_real_cursor();
            }
        }
    }

    Ok(())
}

pub(crate) fn diagnostics() -> String {
    diagnostics_report()
}

pub(crate) fn validation_counters() -> String {
    super::runtime::validation_counters_report()
}

#[cfg(test)]
mod tests {
    use super::dispatch_registered_autocmd;
    use pretty_assertions::assert_eq;

    #[test]
    fn registered_autocmd_callback_keeps_the_handler_installed_after_unknown_events() {
        let should_delete =
            dispatch_registered_autocmd("DefinitelyNotReal").expect("unknown event should no-op");
        assert_eq!(should_delete, false);
    }
}
