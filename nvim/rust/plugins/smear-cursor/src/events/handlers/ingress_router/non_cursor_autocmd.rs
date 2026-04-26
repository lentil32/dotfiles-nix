use super::AutocmdDispatchContext;
use super::IngressDispatchOutcome;
use crate::draw::clear_highlight_cache;
use crate::draw::prune_closed_window_resources;
use crate::draw::prune_stale_tab_resources;
use crate::draw::tracked_draw_tab_handles;
use crate::events::ingress::AutocmdIngress;
use crate::events::runtime;
use crate::events::runtime::note_cursor_color_colorscheme_change;
use crate::events::runtime::now_ms;
use crate::events::runtime::refresh_editor_viewport_cache;
use crate::events::runtime::to_core_millis;
use crate::host::BufferHandle;
use crate::host::HostTabSnapshot;
use crate::host::NeovimHost;
use crate::host::TabHandle;
use crate::host::TabPagePort;
use nvim_oxi::Result;
use std::collections::HashSet;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct LiveTabSnapshot {
    pub(super) tab_handle: TabHandle,
    pub(super) tab_number: Option<u32>,
}

impl From<HostTabSnapshot> for LiveTabSnapshot {
    fn from(snapshot: HostTabSnapshot) -> Self {
        Self {
            tab_handle: snapshot.tab_handle,
            tab_number: snapshot.tab_number,
        }
    }
}

pub(super) fn on_colorscheme_ingress() -> Result<IngressDispatchOutcome> {
    clear_highlight_cache();
    note_cursor_color_colorscheme_change()?;
    Ok(IngressDispatchOutcome::Applied)
}

pub(super) fn on_non_cursor_autocmd_ingress(
    ingress: AutocmdIngress,
    context: AutocmdDispatchContext<'_>,
) -> Result<IngressDispatchOutcome> {
    match ingress {
        AutocmdIngress::BufWipeout => {
            if let Some(buffer_handle) = context.buffer_handle {
                invalidate_buffer_local_caches(buffer_handle)?;
            }
            Ok(IngressDispatchOutcome::Dropped)
        }
        AutocmdIngress::OptionSet => {
            handle_option_set_autocmd(context)?;
            Ok(IngressDispatchOutcome::Dropped)
        }
        AutocmdIngress::TabClosed => {
            handle_tab_closed_autocmd()?;
            Ok(IngressDispatchOutcome::Dropped)
        }
        AutocmdIngress::TextChanged | AutocmdIngress::TextChangedInsert => {
            handle_text_mutation_autocmd(context)?;
            Ok(IngressDispatchOutcome::Dropped)
        }
        AutocmdIngress::VimResized => {
            refresh_editor_viewport_cache()?;
            Ok(IngressDispatchOutcome::Dropped)
        }
        AutocmdIngress::WinClosed => {
            handle_win_closed_autocmd(context)?;
            Ok(IngressDispatchOutcome::Dropped)
        }
        AutocmdIngress::CmdlineChanged
        | AutocmdIngress::CursorMoved
        | AutocmdIngress::CursorMovedInsert
        | AutocmdIngress::ModeChanged
        | AutocmdIngress::WinEnter
        | AutocmdIngress::WinScrolled
        | AutocmdIngress::BufEnter
        | AutocmdIngress::ColorScheme => {
            unreachable!("cursor/color autocmd routed through non_cursor_autocmd")
        }
    }
}

fn handle_option_set_autocmd(context: AutocmdDispatchContext<'_>) -> Result<()> {
    if let Some((match_name, buffer_handle)) = context.match_name.zip(context.buffer_handle) {
        invalidate_buffer_metadata_for_option_set(match_name, buffer_handle)?;
        invalidate_conceal_probe_caches_for_option_set(match_name, buffer_handle)?;
    }
    if let Some(match_name) = context.match_name {
        refresh_editor_viewport_for_option_set(match_name)?;
    }
    Ok(())
}

fn handle_text_mutation_autocmd(context: AutocmdDispatchContext<'_>) -> Result<()> {
    if let Some(buffer_handle) = context.buffer_handle {
        advance_buffer_text_revision(buffer_handle)?;
        invalidate_buffer_metadata(buffer_handle)?;
        invalidate_buffer_local_probe_caches(buffer_handle)?;
    }
    Ok(())
}

fn schedule_retained_resource_cleanup_retry(retained_resources: usize) -> Result<()> {
    let Some(event) = super::super::retained_resource_cleanup_retry_event(
        retained_resources,
        to_core_millis(now_ms()),
    ) else {
        return Ok(());
    };

    super::super::dispatch_core_event_with_default_scheduler(event)
}

fn parse_positive_i64(match_name: Option<&str>) -> Option<i64> {
    match_name?.parse::<i64>().ok().filter(|value| *value > 0)
}

pub(super) fn parse_closed_window_id(match_name: Option<&str>) -> Option<i32> {
    let window_id = parse_positive_i64(match_name)?;
    i32::try_from(window_id).ok()
}

fn live_tab_snapshot() -> Vec<LiveTabSnapshot> {
    live_tab_snapshot_with(&NeovimHost)
}

pub(super) fn live_tab_snapshot_with(host: &impl TabPagePort) -> Vec<LiveTabSnapshot> {
    host.live_tab_snapshot()
        .into_iter()
        .map(LiveTabSnapshot::from)
        .collect()
}

pub(super) fn stale_tracked_tab_handles(
    tracked_tab_handles: impl IntoIterator<Item = TabHandle>,
    live_tabs: &[LiveTabSnapshot],
) -> Vec<TabHandle> {
    let live_tab_handles = live_tabs
        .iter()
        .map(|tab| tab.tab_handle)
        .collect::<HashSet<_>>();
    let mut stale_tab_handles = tracked_tab_handles
        .into_iter()
        .filter(|tab_handle| !live_tab_handles.contains(tab_handle))
        .collect::<Vec<_>>();
    stale_tab_handles.sort_unstable();
    stale_tab_handles.dedup();
    stale_tab_handles
}

fn handle_tab_closed_autocmd() -> Result<()> {
    let live_tabs = live_tab_snapshot();
    let stale_tab_handles = stale_tracked_tab_handles(tracked_draw_tab_handles(), &live_tabs);
    if stale_tab_handles.is_empty() {
        return Ok(());
    }

    let namespace_id =
        super::super::super::host_bridge::ensure_namespace_id().map_err(nvim_oxi::Error::from)?;
    let summary = prune_stale_tab_resources(namespace_id, &stale_tab_handles);
    schedule_retained_resource_cleanup_retry(summary.retained_resources())
}

fn handle_win_closed_autocmd(context: AutocmdDispatchContext<'_>) -> Result<()> {
    let Some(window_id) = parse_closed_window_id(context.match_name) else {
        return Ok(());
    };

    let namespace_id =
        super::super::super::host_bridge::ensure_namespace_id().map_err(nvim_oxi::Error::from)?;
    let summary = prune_closed_window_resources(namespace_id, window_id);
    schedule_retained_resource_cleanup_retry(summary.retained_resources())
}

pub(super) fn should_invalidate_buffer_metadata_for_option(option_name: &str) -> bool {
    matches!(option_name, "filetype" | "buftype" | "buflisted")
}

pub(super) fn should_refresh_editor_viewport_for_option(option_name: &str) -> bool {
    matches!(option_name, "cmdheight" | "lines" | "columns")
}

pub(super) fn should_invalidate_conceal_probe_cache_for_option(option_name: &str) -> bool {
    matches!(option_name, "conceallevel" | "concealcursor")
}

pub(super) fn invalidate_buffer_metadata(buffer_handle: impl Into<BufferHandle>) -> Result<()> {
    let buffer_handle = buffer_handle.into();
    runtime::invalidate_buffer_metadata(buffer_handle).map_err(nvim_oxi::Error::from)?;
    Ok(())
}

fn invalidate_buffer_metadata_for_option_set(
    match_name: &str,
    buffer_handle: impl Into<BufferHandle>,
) -> Result<()> {
    let buffer_handle = buffer_handle.into();
    if !should_invalidate_buffer_metadata_for_option(match_name) {
        return Ok(());
    }

    invalidate_buffer_metadata(buffer_handle)
}

fn refresh_editor_viewport_for_option_set(match_name: &str) -> Result<()> {
    if !should_refresh_editor_viewport_for_option(match_name) {
        return Ok(());
    }

    refresh_editor_viewport_cache()
}

pub(super) fn invalidate_buffer_local_probe_caches(
    buffer_handle: impl Into<BufferHandle>,
) -> Result<()> {
    let buffer_handle = buffer_handle.into();
    runtime::invalidate_buffer_local_probe_caches(buffer_handle).map_err(nvim_oxi::Error::from)?;
    Ok(())
}

pub(super) fn advance_buffer_text_revision(buffer_handle: impl Into<BufferHandle>) -> Result<()> {
    let buffer_handle = buffer_handle.into();
    runtime::advance_buffer_text_revision(buffer_handle).map_err(nvim_oxi::Error::from)?;
    Ok(())
}

pub(super) fn invalidate_conceal_probe_caches(
    buffer_handle: impl Into<BufferHandle>,
) -> Result<()> {
    let buffer_handle = buffer_handle.into();
    runtime::invalidate_conceal_probe_caches(buffer_handle).map_err(nvim_oxi::Error::from)?;
    Ok(())
}

fn invalidate_conceal_probe_caches_for_option_set(
    match_name: &str,
    buffer_handle: impl Into<BufferHandle>,
) -> Result<()> {
    let buffer_handle = buffer_handle.into();
    if !should_invalidate_conceal_probe_cache_for_option(match_name) {
        return Ok(());
    }

    invalidate_conceal_probe_caches(buffer_handle)
}

pub(super) fn invalidate_buffer_local_caches(buffer_handle: impl Into<BufferHandle>) -> Result<()> {
    let buffer_handle = buffer_handle.into();
    runtime::invalidate_buffer_local_caches(buffer_handle).map_err(nvim_oxi::Error::from)?;
    Ok(())
}
