//! Plugin lifecycle orchestration for setup, teardown, and toggle boundaries.
//!
//! This module keeps Neovim callbacks, host-bridge state, and runtime state in
//! lockstep so setup failures degrade into an explicitly disabled runtime
//! instead of leaving stale callbacks behind.

use super::AUTOCMD_GROUP_NAME;
use super::cursor::current_mode;
use super::cursor::cursor_observation_for_mode_with_probe_policy;
use super::host_bridge::DISPATCH_AUTOCMD_FUNCTION_NAME;
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
use super::surface::current_window_surface_snapshot;
use crate::core::effect::ProbePolicy;
use crate::draw::clear_highlight_cache;
use crate::draw::initialize_runtime_capabilities;
use crate::draw::purge_render_windows;
use crate::lua::i64_from_object;
use crate::lua::parse_optional_with;
use crate::lua::require_with_typed;
use crate::lua::string_from_object;
use crate::lua::string_from_object_typed;
use crate::state::CursorShape;
use crate::state::TrackedCursor;
use nvim_oxi::Dictionary;
use nvim_oxi::Object;
use nvim_oxi::Result;
use nvim_oxi::String as NvimString;
use nvim_oxi::api;
use nvim_oxi::api::opts::CreateAugroupOpts;
use nvim_oxi::api::opts::CreateAutocmdOpts;
use nvim_oxi::api::opts::CreateCommandOpts;

#[derive(Debug, Clone, Eq, PartialEq)]
struct RegisteredAutocmdPayload {
    event: String,
    buffer_handle: Option<i64>,
    match_name: Option<String>,
}

fn jump_to_current_cursor() -> Result<()> {
    let namespace_id = ensure_namespace_id()?;
    let mode = current_mode();
    let mode = mode.to_string_lossy();
    let window = api::get_current_win();
    if !window.is_valid() {
        return Ok(());
    }

    if !api::get_current_buf().is_valid() {
        return Ok(());
    }

    let smear_to_cmd = read_engine_state(|state| state.core_state().runtime().config.smear_to_cmd)?;
    let surface_snapshot = current_window_surface_snapshot(&window).ok();
    let observation = cursor_observation_for_mode_with_probe_policy(
        &window,
        mode.as_ref(),
        smear_to_cmd,
        ProbePolicy::exact(),
        surface_snapshot.as_ref(),
    )?;
    let Some(position) = observation
        .screen_cell()
        .map(crate::position::RenderPoint::from)
    else {
        return Ok(());
    };

    let Some(surface_snapshot) = surface_snapshot else {
        warn("current surface snapshot unavailable during jump-to-current-cursor");
        return Ok(());
    };
    let tracked_cursor = TrackedCursor::new(surface_snapshot, observation.buffer_line());

    let hide_target_hack = mutate_engine_state(|state| {
        state.shell.set_namespace_id(namespace_id);
        let runtime = state.core_state_mut().runtime_mut();
        let cursor_shape =
            CursorShape::from_cell_shape(runtime.config.cursor_cell_shape(mode.as_ref()));
        runtime.sync_to_current_cursor(position, cursor_shape, &tracked_cursor);
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

fn raw_payload_field(payload: &Dictionary, key: &str) -> Option<Object> {
    payload.get(&NvimString::from(key)).cloned()
}

fn parse_optional_buffer_handle(raw: Option<Object>) -> Result<Option<i64>> {
    Ok(parse_optional_with(raw, "buffer", i64_from_object)?
        .and_then(|buffer_handle| (buffer_handle > 0).then_some(buffer_handle)))
}

fn parse_optional_match_name(raw: Option<Object>) -> Result<Option<String>> {
    Ok(parse_optional_with(raw, "match", string_from_object)?
        .and_then(|match_name| (!match_name.is_empty()).then_some(match_name)))
}

fn parse_registered_autocmd_payload(payload: &Dictionary) -> Result<RegisteredAutocmdPayload> {
    let event = require_with_typed(
        raw_payload_field(payload, "event"),
        "event",
        string_from_object_typed,
    )
    .map_err(|source| crate::lua::to_nvim_error(&source))?;
    if event.is_empty() {
        return Err(crate::lua::invalid_key("event", "non-empty string"));
    }

    Ok(RegisteredAutocmdPayload {
        event,
        buffer_handle: parse_optional_buffer_handle(raw_payload_field(payload, "buffer"))?,
        match_name: parse_optional_match_name(raw_payload_field(payload, "match"))?,
    })
}

fn autocmd_dispatch_command(event: &str) -> String {
    format!(
        "call {DISPATCH_AUTOCMD_FUNCTION_NAME}('{event}', str2nr(expand('<abuf>')), expand('<amatch>'))",
    )
}

pub(crate) fn on_autocmd_payload_event(payload: &Dictionary) -> Result<()> {
    let payload = parse_registered_autocmd_payload(payload)?;
    super::handlers::on_autocmd_payload_event(
        &payload.event,
        payload.buffer_handle,
        payload.match_name.as_deref(),
    )
}

fn setup_autocmds() -> Result<()> {
    let group = api::create_augroup(
        AUTOCMD_GROUP_NAME,
        &CreateAugroupOpts::builder().clear(true).build(),
    )?;

    for event in registered_autocmd_event_names() {
        let opts = CreateAutocmdOpts::builder()
            .group(group)
            // Use the host bridge command path so failed registrations cannot leak
            // callback-backed Lua registry refs.
            .command(autocmd_dispatch_command(event))
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
    use super::DISPATCH_AUTOCMD_FUNCTION_NAME;
    use super::RegisteredAutocmdPayload;
    use super::autocmd_dispatch_command;
    use super::parse_registered_autocmd_payload;
    use crate::events::handlers::on_autocmd_event;
    use nvim_oxi::Dictionary;
    use nvim_oxi::Object;
    use pretty_assertions::assert_eq;

    #[test]
    fn unknown_registered_autocmd_is_a_noop() {
        on_autocmd_event("DefinitelyNotReal").expect("unknown event should no-op");
    }

    #[test]
    fn autocmd_dispatch_command_routes_through_the_host_bridge() {
        assert_eq!(
            autocmd_dispatch_command("OptionSet"),
            format!(
                "call {DISPATCH_AUTOCMD_FUNCTION_NAME}('OptionSet', str2nr(expand('<abuf>')), expand('<amatch>'))",
            )
        );
    }

    #[test]
    fn parse_registered_autocmd_payload_normalizes_empty_context_fields() {
        let mut payload = Dictionary::new();
        payload.insert("event", "OptionSet");
        payload.insert("buffer", 0);
        payload.insert("match", "");

        assert_eq!(
            parse_registered_autocmd_payload(&payload).expect("payload should parse"),
            RegisteredAutocmdPayload {
                event: "OptionSet".to_string(),
                buffer_handle: None,
                match_name: None,
            }
        );
    }

    #[test]
    fn parse_registered_autocmd_payload_requires_a_non_empty_event() {
        let mut payload = Dictionary::new();
        payload.insert("event", Object::from(""));

        assert!(parse_registered_autocmd_payload(&payload).is_err());
    }
}
