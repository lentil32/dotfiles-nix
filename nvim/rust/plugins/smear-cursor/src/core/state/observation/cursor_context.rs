use crate::core::types::Generation;
use crate::host::BufferHandle;
use crate::position::ScreenCell;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct CursorTextContextBoundary {
    buffer_handle: BufferHandle,
    changedtick: u64,
}

impl CursorTextContextBoundary {
    pub(crate) fn new(buffer_handle: impl Into<BufferHandle>, changedtick: u64) -> Self {
        let buffer_handle = buffer_handle.into();
        Self {
            buffer_handle,
            changedtick,
        }
    }

    pub(crate) fn matches(self, buffer_handle: impl Into<BufferHandle>, changedtick: u64) -> bool {
        let buffer_handle = buffer_handle.into();
        self.buffer_handle == buffer_handle && self.changedtick == changedtick
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) enum CursorTextContextState {
    #[default]
    Unavailable,
    BoundaryOnly(CursorTextContextBoundary),
    Sampled(CursorTextContext),
}

impl CursorTextContextState {
    pub(crate) fn from_parts(
        sampled: Option<CursorTextContext>,
        boundary: Option<CursorTextContextBoundary>,
    ) -> Self {
        if let Some(sampled) = sampled {
            Self::Sampled(sampled)
        } else if let Some(boundary) = boundary {
            Self::BoundaryOnly(boundary)
        } else {
            Self::Unavailable
        }
    }

    pub(crate) fn boundary(&self) -> Option<CursorTextContextBoundary> {
        match self {
            Self::Unavailable => None,
            Self::BoundaryOnly(boundary) => Some(*boundary),
            Self::Sampled(context) => Some(CursorTextContextBoundary::new(
                context.buffer_handle(),
                context.changedtick(),
            )),
        }
    }

    pub(crate) fn sampled(&self) -> Option<&CursorTextContext> {
        match self {
            Self::Sampled(context) => Some(context),
            Self::Unavailable | Self::BoundaryOnly(_) => None,
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub(crate) struct CursorColorProbeWitness {
    window_handle: i64,
    buffer_handle: BufferHandle,
    changedtick: u64,
    mode: String,
    cursor_position: Option<ScreenCell>,
    colorscheme_generation: Generation,
    cache_generation: Generation,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct CursorColorProbeGenerations {
    colorscheme_generation: Generation,
    cache_generation: Generation,
}

impl CursorColorProbeGenerations {
    pub(crate) const fn new(
        colorscheme_generation: Generation,
        cache_generation: Generation,
    ) -> Self {
        Self {
            colorscheme_generation,
            cache_generation,
        }
    }

    pub(crate) const fn colorscheme_generation(self) -> Generation {
        self.colorscheme_generation
    }

    pub(crate) const fn cache_generation(self) -> Generation {
        self.cache_generation
    }
}

impl CursorColorProbeWitness {
    pub(crate) fn new(
        window_handle: i64,
        buffer_handle: impl Into<BufferHandle>,
        changedtick: u64,
        mode: String,
        cursor_position: Option<ScreenCell>,
        colorscheme_generation: Generation,
        cache_generation: Generation,
    ) -> Self {
        Self {
            window_handle,
            buffer_handle: buffer_handle.into(),
            changedtick,
            mode,
            cursor_position,
            colorscheme_generation,
            cache_generation,
        }
    }

    pub(crate) const fn window_handle(&self) -> i64 {
        self.window_handle
    }

    pub(crate) const fn buffer_handle(&self) -> BufferHandle {
        self.buffer_handle
    }

    pub(crate) const fn changedtick(&self) -> u64 {
        self.changedtick
    }

    pub(crate) fn mode(&self) -> &str {
        &self.mode
    }

    pub(crate) const fn cursor_position(&self) -> Option<ScreenCell> {
        self.cursor_position
    }

    pub(crate) const fn colorscheme_generation(&self) -> Generation {
        self.colorscheme_generation
    }

    pub(crate) const fn cache_generation(&self) -> Generation {
        self.cache_generation
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct ObservedTextRow {
    text: String,
}

impl ObservedTextRow {
    pub(crate) fn new(text: String) -> Self {
        Self { text }
    }

    pub(crate) fn text(&self) -> &str {
        &self.text
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct CursorTextContext {
    buffer_handle: BufferHandle,
    changedtick: u64,
    cursor_line: i64,
    nearby_rows: Arc<[ObservedTextRow]>,
    tracked_nearby_rows: Option<Arc<[ObservedTextRow]>>,
}

impl CursorTextContext {
    #[cfg(test)]
    pub(crate) fn new(
        buffer_handle: impl Into<BufferHandle>,
        changedtick: u64,
        cursor_line: i64,
        nearby_rows: Vec<ObservedTextRow>,
        tracked_nearby_rows: Option<Vec<ObservedTextRow>>,
    ) -> Self {
        Self::from_shared(
            buffer_handle,
            changedtick,
            cursor_line,
            nearby_rows.into(),
            tracked_nearby_rows.map(Into::into),
        )
    }

    pub(crate) fn from_shared(
        buffer_handle: impl Into<BufferHandle>,
        changedtick: u64,
        cursor_line: i64,
        nearby_rows: Arc<[ObservedTextRow]>,
        tracked_nearby_rows: Option<Arc<[ObservedTextRow]>>,
    ) -> Self {
        Self {
            buffer_handle: buffer_handle.into(),
            changedtick,
            cursor_line,
            nearby_rows,
            tracked_nearby_rows,
        }
    }

    pub(crate) const fn buffer_handle(&self) -> BufferHandle {
        self.buffer_handle
    }

    pub(crate) const fn changedtick(&self) -> u64 {
        self.changedtick
    }

    pub(crate) const fn cursor_line(&self) -> i64 {
        self.cursor_line
    }

    pub(crate) fn nearby_rows(&self) -> &[ObservedTextRow] {
        self.nearby_rows.as_ref()
    }

    pub(crate) fn tracked_nearby_rows(&self) -> Option<&[ObservedTextRow]> {
        self.tracked_nearby_rows.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::CursorTextContext;
    use super::CursorTextContextBoundary;
    use super::CursorTextContextState;
    use super::ObservedTextRow;
    use pretty_assertions::assert_eq;

    fn sample_context() -> CursorTextContext {
        CursorTextContext::new(
            22,
            14,
            7,
            vec![
                ObservedTextRow::new("before".to_string()),
                ObservedTextRow::new("cursor".to_string()),
                ObservedTextRow::new("after".to_string()),
            ],
            None,
        )
    }

    #[test]
    fn cursor_text_context_state_accessors_follow_the_single_owned_variant() {
        let boundary = CursorTextContextBoundary::new(22, 14);
        let sampled = sample_context();

        assert_eq!(
            CursorTextContextState::from_parts(None, None),
            CursorTextContextState::Unavailable
        );
        assert_eq!(
            CursorTextContextState::from_parts(None, Some(boundary)),
            CursorTextContextState::BoundaryOnly(boundary)
        );

        let sampled_state =
            CursorTextContextState::from_parts(Some(sampled.clone()), Some(boundary));
        assert_eq!(
            sampled_state.boundary(),
            Some(CursorTextContextBoundary::new(
                sampled.buffer_handle(),
                sampled.changedtick(),
            ))
        );
        assert_eq!(sampled_state.sampled(), Some(&sampled));
    }
}
