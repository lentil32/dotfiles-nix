use super::AUTOCMD_GROUP_NAME;
use super::cursor::{cursor_position_for_mode, line_value, mode_string};
use super::host_bridge::{
    ensure_namespace_id, installed_host_bridge, set_on_key_listener, verify_host_bridge,
};
use super::ingress::registered_autocmd_event_names;
use super::logging::{debug, ensure_hideable_guicursor, set_log_level, unhide_real_cursor, warn};
use super::options::apply_runtime_options;
use super::runtime::{diagnostics_report, engine_lock, reset_transient_event_state};
use crate::draw::{clear_highlight_cache, purge_render_windows};
use crate::state::{CursorLocation, CursorShape};
use crate::types::Point;
use nvim_oxi::api::opts::{CreateAugroupOpts, CreateAutocmdOpts, CreateCommandOpts};
use nvim_oxi::{Dictionary, Result, String as NvimString, api};

fn jump_to_current_cursor() -> Result<()> {
    let namespace_id = ensure_namespace_id();
    let mode = mode_string();
    let window = api::get_current_win();
    if !window.is_valid() {
        return Ok(());
    }

    let buffer = api::get_current_buf();
    if !buffer.is_valid() {
        return Ok(());
    }

    let smear_to_cmd = {
        let state = engine_lock();
        state.core_state.runtime().config.smear_to_cmd
    };
    let Some((row, col)) = cursor_position_for_mode(&window, &mode, smear_to_cmd)? else {
        return Ok(());
    };

    let location = CursorLocation::new(
        i64::from(window.handle()),
        i64::from(buffer.handle()),
        line_value("w0")?,
        line_value(".")?,
    );

    let hide_target_hack = {
        let mut state = engine_lock();
        state.shell.set_namespace_id(namespace_id);
        let mut runtime = state.core_state.runtime().clone();
        let cursor_shape = CursorShape::new(
            runtime.config.cursor_is_vertical_bar(&mode),
            runtime.config.cursor_is_horizontal_bar(&mode),
        );
        runtime.sync_to_current_cursor(Point { row, col }, cursor_shape, location);
        let hide_target_hack = runtime.config.hide_target_hack;
        let next_core = state.core_state().with_runtime(runtime);
        state.set_core_state(next_core);
        hide_target_hack
    };

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

fn setup_autocmds() -> Result<()> {
    let group = api::create_augroup(
        AUTOCMD_GROUP_NAME,
        &CreateAugroupOpts::builder().clear(true).build(),
    )?;

    for event in registered_autocmd_event_names() {
        let command = format!("lua require('rs_smear_cursor').on_autocmd('{event}')");
        let opts = CreateAutocmdOpts::builder()
            .group(group)
            .command(command)
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
        "lua require('rs_smear_cursor').toggle()",
        &CreateCommandOpts::builder().build(),
    )?;
    api::create_user_command(
        "SmearCursorDiagnostics",
        "lua print(require('rs_smear_cursor').diagnostics())",
        &CreateCommandOpts::builder().build(),
    )?;
    Ok(())
}

pub(crate) fn setup(opts: &Dictionary) -> Result<()> {
    let host_bridge = verify_host_bridge()?;
    let namespace_id = ensure_namespace_id();
    ensure_hideable_guicursor();
    unhide_real_cursor();
    clear_highlight_cache();

    {
        let mut state = engine_lock();
        state.shell.set_namespace_id(namespace_id);
        let mut runtime = state.core_state.runtime().clone();
        runtime.disable();
        let next_core = state.core_state().with_runtime(runtime);
        state.set_core_state(next_core);
    }
    clear_autocmd_group();
    set_on_key_listener(host_bridge, namespace_id, false)?;
    reset_transient_event_state();

    let has_enabled_option = opts.get(&NvimString::from("enabled")).is_some();
    let (enabled, setup_warning) = {
        let mut state = engine_lock();
        state.shell.set_namespace_id(namespace_id);
        let mut runtime = state.core_state.runtime().clone();
        // Surprising: setup errors used to return before teardown, leaving stale callbacks alive.
        // We disable+clear first, then re-enable only after options parse/apply succeeds.
        // Setup defaults to enabled=true when the option is omitted.
        if !has_enabled_option {
            runtime.set_enabled(true);
        }
        let outcome = match apply_runtime_options(&mut runtime, opts) {
            Ok(()) => {
                set_log_level(runtime.config.logging_level);
                runtime.clear_runtime_state();
                (runtime.is_enabled(), None)
            }
            Err(err) => {
                runtime.disable();
                runtime.clear_runtime_state();
                (
                    false,
                    Some(format!(
                        "setup rejected options; smear cursor remains disabled: {err}"
                    )),
                )
            }
        };
        let next_core = state.core_state().with_runtime(runtime);
        state.set_core_state(next_core);
        outcome
    };

    setup_user_command()?;
    if enabled {
        setup_autocmds()?;
        set_on_key_listener(host_bridge, namespace_id, true)?;
    }
    jump_to_current_cursor()?;
    if let Some(message) = setup_warning {
        warn(&message);
    }
    Ok(())
}

pub(crate) fn toggle() -> Result<()> {
    let host_bridge = installed_host_bridge()?;
    let (is_enabled, namespace_id, hide_target_hack) = {
        let mut state = engine_lock();
        let mut runtime = state.core_state.runtime().clone();
        let toggled_enabled = !runtime.is_enabled();
        if toggled_enabled {
            runtime.set_enabled(true);
        } else {
            runtime.disable();
        }
        let outcome = (
            runtime.is_enabled(),
            state.shell.namespace_id(),
            runtime.config.hide_target_hack,
        );
        let next_core = state.core_state().with_runtime(runtime);
        state.set_core_state(next_core);
        outcome
    };

    if let Some(namespace_id) = namespace_id {
        if is_enabled {
            setup_autocmds()?;
            set_on_key_listener(host_bridge, namespace_id, true)?;
            jump_to_current_cursor()?;
        } else {
            clear_autocmd_group();
            set_on_key_listener(host_bridge, namespace_id, false)?;
            reset_transient_event_state();
            let _ = purge_render_windows(namespace_id);
            if !hide_target_hack {
                unhide_real_cursor();
            }
        }
    }

    Ok(())
}

pub(crate) fn diagnostics() -> Result<String> {
    Ok(diagnostics_report())
}
