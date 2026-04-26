use crate::events::ingress::AutocmdIngress;
use crate::events::ingress::Ingress;
use crate::events::ingress::parse_autocmd_ingress;
use crate::events::runtime::record_ingress_applied;
use crate::events::runtime::record_ingress_coalesced;
use crate::events::runtime::record_ingress_dropped;
use crate::events::runtime::record_ingress_received;
use crate::host::BufferHandle;
use nvim_oxi::Result;

mod cursor_autocmd;
mod non_cursor_autocmd;
#[cfg(test)]
mod tests;

#[cfg(test)]
use self::cursor_autocmd::CursorAutocmdFastPathSnapshot;
#[cfg(test)]
use self::cursor_autocmd::CursorAutocmdPreflight;
#[cfg(test)]
use self::cursor_autocmd::build_cursor_autocmd_events;
#[cfg(test)]
use self::cursor_autocmd::cursor_autocmd_preflight;
#[cfg(test)]
use self::cursor_autocmd::demand_kind_for_autocmd;
#[cfg(test)]
use self::cursor_autocmd::should_coalesce_window_follow_up_autocmd;
#[cfg(test)]
use self::cursor_autocmd::should_drop_unchanged_cursor_autocmd;
#[cfg(test)]
use self::non_cursor_autocmd::LiveTabSnapshot;
#[cfg(test)]
use self::non_cursor_autocmd::advance_buffer_text_revision;
#[cfg(test)]
use self::non_cursor_autocmd::invalidate_buffer_local_caches;
#[cfg(test)]
use self::non_cursor_autocmd::invalidate_buffer_metadata;
#[cfg(test)]
use self::non_cursor_autocmd::parse_closed_window_id;
#[cfg(test)]
use self::non_cursor_autocmd::should_invalidate_buffer_metadata_for_option;
#[cfg(test)]
use self::non_cursor_autocmd::should_invalidate_conceal_probe_cache_for_option;
#[cfg(test)]
use self::non_cursor_autocmd::should_refresh_editor_viewport_for_option;
#[cfg(test)]
use self::non_cursor_autocmd::stale_tracked_tab_handles;
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum IngressDispatchOutcome {
    Applied,
    Coalesced,
    Dropped,
}

#[derive(Debug, Default, Clone, Copy, Eq, PartialEq)]
struct AutocmdDispatchContext<'a> {
    buffer_handle: Option<BufferHandle>,
    match_name: Option<&'a str>,
}

fn on_autocmd_ingress(
    ingress: AutocmdIngress,
    context: AutocmdDispatchContext<'_>,
) -> Result<IngressDispatchOutcome> {
    match ingress {
        AutocmdIngress::ColorScheme => non_cursor_autocmd::on_colorscheme_ingress(),
        AutocmdIngress::BufWipeout
        | AutocmdIngress::OptionSet
        | AutocmdIngress::TabClosed
        | AutocmdIngress::TextChanged
        | AutocmdIngress::TextChangedInsert
        | AutocmdIngress::VimResized
        | AutocmdIngress::WinClosed => {
            non_cursor_autocmd::on_non_cursor_autocmd_ingress(ingress, context)
        }
        AutocmdIngress::CmdlineChanged
        | AutocmdIngress::CursorMoved
        | AutocmdIngress::CursorMovedInsert
        | AutocmdIngress::ModeChanged
        | AutocmdIngress::WinEnter
        | AutocmdIngress::WinScrolled
        | AutocmdIngress::BufEnter => cursor_autocmd::on_cursor_event_core_for_autocmd(ingress),
    }
}

fn dispatch_ingress(ingress: Ingress, context: AutocmdDispatchContext<'_>) -> Result<()> {
    record_ingress_received();
    let outcome = match ingress {
        Ingress::Autocmd(autocmd_ingress) => on_autocmd_ingress(autocmd_ingress, context),
    };
    match outcome {
        Ok(dispatch_outcome) => {
            match dispatch_outcome {
                IngressDispatchOutcome::Applied => record_ingress_applied(),
                IngressDispatchOutcome::Coalesced => {
                    record_ingress_applied();
                    record_ingress_coalesced();
                }
                IngressDispatchOutcome::Dropped => record_ingress_dropped(),
            }
            Ok(())
        }
        Err(err) => {
            record_ingress_dropped();
            Err(err)
        }
    }
}

pub(crate) fn on_autocmd_event(event: &str) -> Result<()> {
    let Some(ingress) = parse_autocmd_ingress(event) else {
        return Err(crate::lua::invalid_key("event", "registered autocmd event"));
    };
    dispatch_ingress(Ingress::Autocmd(ingress), AutocmdDispatchContext::default())
}

pub(in crate::events) fn on_autocmd_payload_event(
    event: &str,
    buffer_handle: Option<BufferHandle>,
    match_name: Option<&str>,
) -> Result<()> {
    let Some(ingress) = parse_autocmd_ingress(event) else {
        return Err(crate::lua::invalid_key("event", "registered autocmd event"));
    };
    dispatch_ingress(
        Ingress::Autocmd(ingress),
        AutocmdDispatchContext {
            buffer_handle,
            match_name,
        },
    )
}
