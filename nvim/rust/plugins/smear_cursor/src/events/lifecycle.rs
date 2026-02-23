use super::AUTOCMD_GROUP_NAME;
use super::cursor::{cursor_position_for_mode, line_value, mode_string};
use super::handlers;
use super::logging::{debug, ensure_hideable_guicursor, set_log_level, unhide_real_cursor};
use super::options::apply_runtime_options;
use super::runtime::{reset_transient_event_state, state_lock};
use super::timers::{clear_autocmd_group, ensure_namespace_id, set_on_key_listener};
use crate::draw::{clear_all_namespaces, clear_highlight_cache};
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
        let state = state_lock();
        state.config.smear_to_cmd
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

    let (hide_target_hack, unhide_cursor) = {
        let mut state = state_lock();
        state.set_namespace_id(namespace_id);

        let cursor_shape = CursorShape::new(
            state.config.cursor_is_vertical_bar(&mode),
            state.config.cursor_is_horizontal_bar(&mode),
        );
        let unhide_cursor =
            state.sync_to_current_cursor(Point { row, col }, cursor_shape, location);

        (state.config.hide_target_hack, unhide_cursor)
    };

    reset_transient_event_state();
    clear_all_namespaces(namespace_id);
    if unhide_cursor && !hide_target_hack {
        unhide_real_cursor();
    }

    Ok(())
}

fn setup_autocmds() -> Result<()> {
    let group = api::create_augroup(
        AUTOCMD_GROUP_NAME,
        &CreateAugroupOpts::builder().clear(true).build(),
    )?;

    let move_opts = CreateAutocmdOpts::builder()
        .group(group)
        .callback(handlers::on_cursor_event)
        .build();
    api::create_autocmd(
        [
            "CmdlineChanged",
            "CursorMoved",
            "CursorMovedI",
            "ModeChanged",
            "WinScrolled",
        ],
        &move_opts,
    )?;

    let buf_enter_opts = CreateAutocmdOpts::builder()
        .group(group)
        .callback(handlers::on_buf_enter)
        .build();
    api::create_autocmd(["BufEnter"], &buf_enter_opts)?;

    let colorscheme_opts = CreateAutocmdOpts::builder()
        .group(group)
        .callback(handlers::on_colorscheme)
        .build();
    api::create_autocmd(["ColorScheme"], &colorscheme_opts)?;

    Ok(())
}

fn setup_user_command() -> Result<()> {
    if let Err(err) = api::del_user_command("SmearCursorToggle") {
        debug(&format!(
            "delete existing SmearCursorToggle failed (continuing): {err}"
        ));
    }
    api::create_user_command(
        "SmearCursorToggle",
        "lua require('rs_smear_cursor').toggle()",
        &CreateCommandOpts::builder().build(),
    )?;
    Ok(())
}

pub(crate) fn setup(opts: Dictionary) -> Result<()> {
    let namespace_id = ensure_namespace_id();
    ensure_hideable_guicursor();
    unhide_real_cursor();
    clear_highlight_cache();
    let has_enabled_option = opts.get(&NvimString::from("enabled")).is_some();
    let enabled = {
        let mut state = state_lock();
        state.set_namespace_id(namespace_id);
        // Setup defaults to enabled=true when the option is omitted.
        if !has_enabled_option {
            state.set_enabled(true);
        }
        apply_runtime_options(&mut state, &opts)?;
        set_log_level(state.config.logging_level);
        state.clear_runtime_state();
        state.is_enabled()
    };
    reset_transient_event_state();

    setup_user_command()?;
    clear_autocmd_group();
    set_on_key_listener(namespace_id, false)?;
    if enabled {
        setup_autocmds()?;
        set_on_key_listener(namespace_id, true)?;
    }
    jump_to_current_cursor()?;
    Ok(())
}

pub(crate) fn toggle() -> Result<()> {
    let (is_enabled, namespace_id, hide_target_hack) = {
        let mut state = state_lock();
        let toggled_enabled = !state.is_enabled();
        if toggled_enabled {
            state.set_enabled(true);
        } else {
            state.disable();
        }
        (
            state.is_enabled(),
            state.namespace_id(),
            state.config.hide_target_hack,
        )
    };

    if let Some(namespace_id) = namespace_id {
        if is_enabled {
            setup_autocmds()?;
            set_on_key_listener(namespace_id, true)?;
            jump_to_current_cursor()?;
        } else {
            clear_autocmd_group();
            set_on_key_listener(namespace_id, false)?;
            reset_transient_event_state();
            clear_all_namespaces(namespace_id);
            if !hide_target_hack {
                unhide_real_cursor();
            }
        }
    }

    Ok(())
}
