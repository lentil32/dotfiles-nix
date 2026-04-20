use super::AutocmdDispatchContext;
use super::IngressDispatchOutcome;
use crate::draw::clear_highlight_cache;
use crate::events::ingress::AutocmdIngress;
use crate::events::runtime::mutate_engine_state;
use crate::events::runtime::note_cursor_color_colorscheme_change;
use crate::events::runtime::refresh_editor_viewport_cache;
use nvim_oxi::Result;

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
        AutocmdIngress::TextChanged | AutocmdIngress::TextChangedInsert => {
            handle_text_mutation_autocmd(context)?;
            Ok(IngressDispatchOutcome::Dropped)
        }
        AutocmdIngress::VimResized => {
            refresh_editor_viewport_cache()?;
            Ok(IngressDispatchOutcome::Dropped)
        }
        AutocmdIngress::Unknown => Ok(IngressDispatchOutcome::Dropped),
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

pub(super) fn should_invalidate_buffer_metadata_for_option(option_name: &str) -> bool {
    matches!(option_name, "filetype" | "buftype" | "buflisted")
}

pub(super) fn should_refresh_editor_viewport_for_option(option_name: &str) -> bool {
    matches!(option_name, "cmdheight" | "lines" | "columns")
}

pub(super) fn should_invalidate_conceal_probe_cache_for_option(option_name: &str) -> bool {
    matches!(option_name, "conceallevel" | "concealcursor")
}

pub(super) fn invalidate_buffer_metadata(buffer_handle: i64) -> Result<()> {
    mutate_engine_state(|state| {
        state.shell.invalidate_buffer_metadata(buffer_handle);
    })
    .map_err(nvim_oxi::Error::from)?;
    Ok(())
}

fn invalidate_buffer_metadata_for_option_set(match_name: &str, buffer_handle: i64) -> Result<()> {
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

pub(super) fn invalidate_buffer_local_probe_caches(buffer_handle: i64) -> Result<()> {
    mutate_engine_state(|state| {
        state
            .shell
            .invalidate_buffer_local_probe_caches(buffer_handle);
    })
    .map_err(nvim_oxi::Error::from)?;
    Ok(())
}

pub(super) fn advance_buffer_text_revision(buffer_handle: i64) -> Result<()> {
    mutate_engine_state(|state| {
        state
            .shell
            .buffer_text_revision_cache
            .advance(buffer_handle);
    })
    .map_err(nvim_oxi::Error::from)?;
    Ok(())
}

pub(super) fn invalidate_conceal_probe_caches(buffer_handle: i64) -> Result<()> {
    mutate_engine_state(|state| {
        state.shell.invalidate_conceal_probe_caches(buffer_handle);
    })
    .map_err(nvim_oxi::Error::from)?;
    Ok(())
}

fn invalidate_conceal_probe_caches_for_option_set(
    match_name: &str,
    buffer_handle: i64,
) -> Result<()> {
    if !should_invalidate_conceal_probe_cache_for_option(match_name) {
        return Ok(());
    }

    invalidate_conceal_probe_caches(buffer_handle)
}

pub(super) fn invalidate_buffer_local_caches(buffer_handle: i64) -> Result<()> {
    mutate_engine_state(|state| {
        state.shell.invalidate_buffer_local_caches(buffer_handle);
    })
    .map_err(nvim_oxi::Error::from)?;
    Ok(())
}
