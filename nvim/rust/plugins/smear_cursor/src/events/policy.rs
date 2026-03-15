use super::cursor::current_buffer_filetype;
use super::runtime::IngressReadSnapshot;
#[cfg(test)]
use crate::types::ScreenCell;
use nvim_oxi::{Result, api};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BufferEventPolicy {
    Normal,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(super) struct IngressCursorPresentationContext {
    pub(super) enabled: bool,
    pub(super) animating: bool,
    pub(super) mode_allowed: bool,
    pub(super) hide_target_hack: bool,
    pub(super) outside_cmdline: bool,
    pub(super) prepaint_cell: Option<ScreenCell>,
    pub(super) windows_zindex: u32,
}

#[cfg(test)]
impl IngressCursorPresentationContext {
    pub(super) const fn new(
        enabled: bool,
        animating: bool,
        mode_allowed: bool,
        hide_target_hack: bool,
        outside_cmdline: bool,
        prepaint_cell: Option<ScreenCell>,
        windows_zindex: u32,
    ) -> Self {
        Self {
            enabled,
            animating,
            mode_allowed,
            hide_target_hack,
            outside_cmdline,
            prepaint_cell,
            windows_zindex,
        }
    }
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(super) enum IngressCursorPresentationPolicy {
    NoAction,
    HideCursor,
    HideCursorAndPrepaint { cell: ScreenCell, zindex: u32 },
}

impl BufferEventPolicy {
    #[cfg(test)]
    pub(super) fn from_buffer_metadata(
        _buftype: &str,
        _buflisted: bool,
        _line_count: i64,
        _callback_duration_estimate_ms: f64,
    ) -> Self {
        Self::Normal
    }

    pub(super) const fn use_key_fallback(self) -> bool {
        match self {
            Self::Normal => true,
        }
    }

    #[cfg(test)]
    pub(super) fn ingress_cursor_presentation_policy(
        self,
        context: IngressCursorPresentationContext,
    ) -> IngressCursorPresentationPolicy {
        let _ = self;

        // `hide_target_hack` keeps its legacy inverted meaning here.
        // When it is enabled, ingress-time cursor masking stays fully disabled.
        if !context.enabled
            || context.animating
            || !context.mode_allowed
            || context.hide_target_hack
            || !context.outside_cmdline
        {
            return IngressCursorPresentationPolicy::NoAction;
        }

        context
            .prepaint_cell
            .map_or(IngressCursorPresentationPolicy::HideCursor, |cell| {
                IngressCursorPresentationPolicy::HideCursorAndPrepaint {
                    cell,
                    zindex: context.windows_zindex,
                }
            })
    }
}

pub(super) fn current_buffer_event_policy(_buffer: &api::Buffer) -> BufferEventPolicy {
    // Policy variants are currently static; avoid per-callback metadata probes in the hot path.
    BufferEventPolicy::Normal
}

pub(super) fn skip_current_buffer_events(
    snapshot: &IngressReadSnapshot,
    buffer: &api::Buffer,
) -> Result<bool> {
    if !snapshot.has_disabled_filetypes() {
        return Ok(false);
    }

    let filetype = current_buffer_filetype(buffer)?;
    Ok(snapshot.filetype_disabled(&filetype))
}
