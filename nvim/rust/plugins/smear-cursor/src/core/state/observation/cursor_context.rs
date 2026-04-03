use crate::core::types::CursorPosition;
use crate::core::types::Generation;
use std::sync::Arc;

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct CursorColorProbeWitness {
    window_handle: i64,
    buffer_handle: i64,
    changedtick: u64,
    mode: String,
    cursor_position: Option<CursorPosition>,
    colorscheme_generation: Generation,
    cache_generation: Generation,
}

impl CursorColorProbeWitness {
    pub(crate) fn new(
        window_handle: i64,
        buffer_handle: i64,
        changedtick: u64,
        mode: String,
        cursor_position: Option<CursorPosition>,
        colorscheme_generation: Generation,
        cache_generation: Generation,
    ) -> Self {
        Self {
            window_handle,
            buffer_handle,
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

    pub(crate) const fn buffer_handle(&self) -> i64 {
        self.buffer_handle
    }

    pub(crate) const fn changedtick(&self) -> u64 {
        self.changedtick
    }

    pub(crate) fn mode(&self) -> &str {
        &self.mode
    }

    pub(crate) const fn cursor_position(&self) -> Option<CursorPosition> {
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
    buffer_handle: i64,
    changedtick: u64,
    cursor_line: i64,
    nearby_rows: Arc<[ObservedTextRow]>,
    tracked_nearby_rows: Option<Arc<[ObservedTextRow]>>,
}

impl CursorTextContext {
    #[cfg(test)]
    pub(crate) fn new(
        buffer_handle: i64,
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
        buffer_handle: i64,
        changedtick: u64,
        cursor_line: i64,
        nearby_rows: Arc<[ObservedTextRow]>,
        tracked_nearby_rows: Option<Arc<[ObservedTextRow]>>,
    ) -> Self {
        Self {
            buffer_handle,
            changedtick,
            cursor_line,
            nearby_rows,
            tracked_nearby_rows,
        }
    }

    pub(crate) const fn buffer_handle(&self) -> i64 {
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
