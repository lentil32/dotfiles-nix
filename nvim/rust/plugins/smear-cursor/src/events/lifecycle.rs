//! Plugin lifecycle orchestration for setup, teardown, and toggle boundaries.
//!
//! This module keeps Neovim callbacks, host-bridge state, and runtime state in
//! lockstep so setup failures degrade into an explicitly disabled runtime
//! instead of leaving stale callbacks behind.

use super::AUTOCMD_GROUP_NAME;
use super::cursor::cursor_observation_for_mode_with_probe_policy;
use super::host_bridge::DISPATCH_AUTOCMD_FUNCTION_NAME;
use super::host_bridge::ensure_namespace_id;
use super::host_bridge::installed_host_bridge;
use super::host_bridge::verify_host_bridge;
use super::ingress::registered_autocmd_event_names;
use super::logging::debug;
use super::logging::ensure_hideable_guicursor;
use super::logging::invalidate_real_cursor_visibility;
use super::logging::unhide_real_cursor;
use super::logging::warn;
use super::runtime::apply_core_setup_options;
use super::runtime::diagnostics_report;
use super::runtime::disable_core_runtime;
use super::runtime::namespace_id;
use super::runtime::refresh_editor_viewport_cache;
use super::runtime::reset_transient_event_state;
use super::runtime::set_namespace_id;
use super::runtime::sync_core_runtime_to_current_cursor;
use super::runtime::toggle_core_runtime;
use super::runtime::with_core_read;
use super::surface::current_window_surface_snapshot;
use crate::core::effect::ProbePolicy;
use crate::draw::clear_highlight_cache;
use crate::draw::initialize_runtime_capabilities;
use crate::draw::purge_render_windows;
use crate::host::BufferHandle;
use crate::host::CurrentEditorPort;
use crate::host::LifecyclePort;
use crate::host::NeovimHost;
use crate::lua::i64_from_object;
use crate::lua::parse_optional_with;
use crate::lua::require_with_typed;
use crate::lua::string_from_object;
use crate::lua::string_from_object_typed;
use crate::state::TrackedCursor;
use nvim_oxi::Dictionary;
use nvim_oxi::Object;
use nvim_oxi::Result;
use nvim_oxi::String as NvimString;

#[derive(Debug, Clone, Eq, PartialEq)]
struct RegisteredAutocmdPayload {
    event: String,
    buffer_handle: Option<BufferHandle>,
    match_name: Option<String>,
}

fn jump_to_current_cursor() -> Result<()> {
    jump_to_current_cursor_with(&NeovimHost)
}

fn jump_to_current_cursor_with(host: &impl CurrentEditorPort) -> Result<()> {
    let namespace_id = ensure_namespace_id()?;
    let mode = host.current_mode();
    let window = host.current_window();
    if !host.window_is_valid(&window) {
        return Ok(());
    }

    let buffer = host.current_buffer();
    if !host.buffer_is_valid(&buffer) {
        return Ok(());
    }

    let smear_to_cmd = with_core_read(|state| state.runtime().config.smear_to_cmd)?;
    let surface_snapshot = current_window_surface_snapshot(&window).ok();
    let observation = cursor_observation_for_mode_with_probe_policy(
        &window,
        &mode,
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

    set_namespace_id(namespace_id)?;
    let hide_target_hack = sync_core_runtime_to_current_cursor(position, &mode, &tracked_cursor)?;

    reset_transient_event_state();
    let _ = purge_render_windows(namespace_id);
    if !hide_target_hack {
        unhide_real_cursor();
    }

    Ok(())
}

fn clear_autocmd_group() {
    clear_autocmd_group_with(&NeovimHost);
}

fn clear_autocmd_group_with(host: &impl LifecyclePort) {
    if let Err(err) = host.clear_autocmd_group(AUTOCMD_GROUP_NAME) {
        warn(&format!("clear autocmd group failed: {err}"));
    }
}

fn raw_payload_field(payload: &Dictionary, key: &str) -> Option<Object> {
    payload.get(&NvimString::from(key)).cloned()
}

fn parse_optional_buffer_handle(raw: Option<Object>) -> Result<Option<BufferHandle>> {
    Ok(parse_optional_with(raw, "buffer", i64_from_object)?.and_then(BufferHandle::new))
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
    setup_autocmds_with(&NeovimHost)
}

fn setup_autocmds_with(host: &impl LifecyclePort) -> Result<()> {
    let group = host.create_autocmd_group(AUTOCMD_GROUP_NAME)?;

    for event in registered_autocmd_event_names() {
        // Use the host bridge command path so failed registrations cannot leak
        // callback-backed Lua registry refs.
        let command = autocmd_dispatch_command(event);
        host.create_autocmd_dispatch(group, event, &command)?;
    }

    Ok(())
}

fn setup_user_command() -> Result<()> {
    setup_user_command_with(&NeovimHost)
}

fn setup_user_command_with(host: &impl LifecyclePort) -> Result<()> {
    if let Err(err) = host.delete_user_command("SmearCursorToggle") {
        debug(&format!(
            "delete existing SmearCursorToggle failed (continuing): {err}"
        ));
    }
    if let Err(err) = host.delete_user_command("SmearCursorDiagnostics") {
        debug(&format!(
            "delete existing SmearCursorDiagnostics failed (continuing): {err}"
        ));
    }
    host.create_string_user_command(
        "SmearCursorToggle",
        "lua require('nvimrs_smear_cursor').toggle()",
    )?;
    host.create_string_user_command(
        "SmearCursorDiagnostics",
        "lua print(require('nvimrs_smear_cursor').diagnostics())",
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

    set_namespace_id(namespace_id)?;
    disable_core_runtime()?;
    refresh_editor_viewport_cache()?;
    clear_autocmd_group();
    reset_transient_event_state();

    set_namespace_id(namespace_id)?;
    let setup = apply_core_setup_options(opts)?;

    setup_user_command()?;
    if setup.enabled {
        setup_autocmds()?;
    }
    jump_to_current_cursor()?;
    if let Some(message) = setup.warning {
        warn(&message);
    }
    Ok(())
}

pub(crate) fn toggle() -> Result<()> {
    let _host_bridge = installed_host_bridge()?;
    let namespace_id = namespace_id()?;
    let toggle = toggle_core_runtime()?;

    if let Some(namespace_id) = namespace_id {
        if toggle.is_enabled {
            refresh_editor_viewport_cache()?;
            setup_autocmds()?;
            jump_to_current_cursor()?;
        } else {
            clear_autocmd_group();
            reset_transient_event_state();
            let _ = purge_render_windows(namespace_id);
            if !toggle.hide_target_hack {
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
    use super::clear_autocmd_group_with;
    use super::parse_registered_autocmd_payload;
    use super::setup_autocmds_with;
    use super::setup_user_command_with;
    use crate::events::handlers::on_autocmd_event;
    use crate::events::ingress::registered_autocmd_event_names;
    use crate::host::FakeLifecyclePort;
    use crate::host::LifecycleCall;
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
    fn clear_autocmd_group_routes_through_lifecycle_port() {
        let host = FakeLifecyclePort::default();

        clear_autocmd_group_with(&host);

        assert_eq!(
            host.calls(),
            vec![LifecycleCall::ClearAutocmdGroup {
                group_name: "RsSmearCursor".to_string(),
            }]
        );
    }

    #[test]
    fn setup_autocmds_routes_registrations_through_lifecycle_port() {
        let host = FakeLifecyclePort::default();
        host.set_group_id(29);

        setup_autocmds_with(&host).expect("fake lifecycle port should accept registrations");

        let calls = host.calls();
        assert!(matches!(
            calls.first(),
            Some(LifecycleCall::CreateAutocmdGroup { group_name }) if group_name == "RsSmearCursor"
        ));
        let registered = calls
            .iter()
            .filter_map(|call| match call {
                LifecycleCall::CreateAutocmdDispatch { event, command, .. } => {
                    Some((event.as_str(), command.as_str()))
                }
                LifecycleCall::CreateAutocmdGroup { .. }
                | LifecycleCall::ClearAutocmdGroup { .. }
                | LifecycleCall::DeleteUserCommand { .. }
                | LifecycleCall::CreateStringUserCommand { .. } => None,
            })
            .collect::<Vec<_>>();
        let expected = registered_autocmd_event_names()
            .map(|event| (event, autocmd_dispatch_command(event)))
            .collect::<Vec<_>>();
        assert_eq!(
            registered,
            expected
                .iter()
                .map(|(event, command)| (*event, command.as_str()))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn setup_user_commands_routes_through_lifecycle_port() {
        let host = FakeLifecyclePort::default();

        setup_user_command_with(&host).expect("fake lifecycle port should accept user commands");

        assert_eq!(
            host.calls(),
            vec![
                LifecycleCall::DeleteUserCommand {
                    name: "SmearCursorToggle".to_string(),
                },
                LifecycleCall::DeleteUserCommand {
                    name: "SmearCursorDiagnostics".to_string(),
                },
                LifecycleCall::CreateStringUserCommand {
                    name: "SmearCursorToggle".to_string(),
                    command: "lua require('nvimrs_smear_cursor').toggle()".to_string(),
                },
                LifecycleCall::CreateStringUserCommand {
                    name: "SmearCursorDiagnostics".to_string(),
                    command: "lua print(require('nvimrs_smear_cursor').diagnostics())".to_string(),
                },
            ]
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
