use super::AutocmdDispatchContext;
use crate::events::ingress::TeardownAutocmdIngress;
use crate::events::runtime;
use crate::events::runtime::close_tab_number;
use crate::host::BufferHandle;
use crate::host::TabHandle;
use nvim_oxi::Result;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(super) enum DeferredTeardownEffect {
    ClosedTab { tab_handle: TabHandle },
    ClosedWindow { window_id: i32 },
}

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub(super) struct TeardownDispatch {
    deferred_effect: Option<DeferredTeardownEffect>,
}

impl TeardownDispatch {
    const fn new(deferred_effect: Option<DeferredTeardownEffect>) -> Self {
        Self { deferred_effect }
    }

    pub(super) const fn deferred_effect(self) -> Option<DeferredTeardownEffect> {
        self.deferred_effect
    }
}

pub(super) fn on_teardown_autocmd_ingress(
    ingress: TeardownAutocmdIngress,
    context: AutocmdDispatchContext<'_>,
) -> Result<TeardownDispatch> {
    match ingress {
        TeardownAutocmdIngress::BufWipeout => handle_buf_wipeout_autocmd(context),
        TeardownAutocmdIngress::TabClosed => handle_tab_closed_autocmd(context),
        TeardownAutocmdIngress::WinClosed => Ok(handle_win_closed_autocmd(context)),
    }
}

fn parse_positive_i64(match_name: Option<&str>) -> Option<i64> {
    match_name?.parse::<i64>().ok().filter(|value| *value > 0)
}

pub(super) fn parse_closed_window_id(match_name: Option<&str>) -> Option<i32> {
    let window_id = parse_positive_i64(match_name)?;
    i32::try_from(window_id).ok()
}

pub(super) fn parse_closed_tab_number(file_name: Option<&str>) -> Option<u32> {
    let tab_number = parse_positive_i64(file_name)?;
    u32::try_from(tab_number).ok()
}

fn handle_buf_wipeout_autocmd(context: AutocmdDispatchContext<'_>) -> Result<TeardownDispatch> {
    if let Some(buffer_handle) = context.buffer_handle {
        invalidate_buffer_local_caches(buffer_handle)?;
    }
    Ok(TeardownDispatch::default())
}

fn handle_tab_closed_autocmd(context: AutocmdDispatchContext<'_>) -> Result<TeardownDispatch> {
    let Some(closed_tab_number) = parse_closed_tab_number(context.file_name) else {
        return Ok(TeardownDispatch::default());
    };

    let Some(tab_handle) = close_tab_number(closed_tab_number).map_err(nvim_oxi::Error::from)?
    else {
        return Ok(TeardownDispatch::default());
    };

    Ok(TeardownDispatch::new(Some(
        DeferredTeardownEffect::ClosedTab { tab_handle },
    )))
}

fn handle_win_closed_autocmd(context: AutocmdDispatchContext<'_>) -> TeardownDispatch {
    let Some(window_id) = parse_closed_window_id(context.match_name) else {
        return TeardownDispatch::default();
    };

    TeardownDispatch::new(Some(DeferredTeardownEffect::ClosedWindow { window_id }))
}

pub(super) fn invalidate_buffer_local_caches(buffer_handle: impl Into<BufferHandle>) -> Result<()> {
    let buffer_handle = buffer_handle.into();
    runtime::invalidate_buffer_local_caches(buffer_handle).map_err(nvim_oxi::Error::from)?;
    Ok(())
}
