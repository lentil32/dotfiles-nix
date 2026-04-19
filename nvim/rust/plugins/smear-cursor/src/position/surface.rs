//! Surface snapshots that retain the host window facts attached to an observation.

use super::validated::BufferLine;
use super::validated::ScreenCell;
use super::validated::SurfaceId;
use super::validated::ViewportBounds;

/// The retained window surface facts captured at observation time.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct WindowSurfaceSnapshot {
    id: SurfaceId,
    top_buffer_line: BufferLine,
    left_col0: u32,
    text_offset0: u32,
    window_origin: ScreenCell,
    window_size: ViewportBounds,
}

impl WindowSurfaceSnapshot {
    pub(crate) const fn new(
        id: SurfaceId,
        top_buffer_line: BufferLine,
        left_col0: u32,
        text_offset0: u32,
        window_origin: ScreenCell,
        window_size: ViewportBounds,
    ) -> Self {
        Self {
            id,
            top_buffer_line,
            left_col0,
            text_offset0,
            window_origin,
            window_size,
        }
    }

    pub(crate) const fn id(self) -> SurfaceId {
        self.id
    }

    pub(crate) const fn top_buffer_line(self) -> BufferLine {
        self.top_buffer_line
    }

    pub(crate) const fn left_col0(self) -> u32 {
        self.left_col0
    }

    pub(crate) const fn text_offset0(self) -> u32 {
        self.text_offset0
    }

    pub(crate) const fn window_origin(self) -> ScreenCell {
        self.window_origin
    }

    pub(crate) const fn window_size(self) -> ViewportBounds {
        self.window_size
    }
}
