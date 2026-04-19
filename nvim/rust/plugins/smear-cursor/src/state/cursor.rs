use crate::animation::corners_for_cursor;
use crate::position::BufferLine;
use crate::position::RenderPoint;
use crate::position::WindowSurfaceSnapshot;
use crate::types::CursorCellShape;

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct TrackedCursor {
    surface: WindowSurfaceSnapshot,
    buffer_line: BufferLine,
}

impl TrackedCursor {
    pub(crate) const fn new(surface: WindowSurfaceSnapshot, buffer_line: BufferLine) -> Self {
        Self {
            surface,
            buffer_line,
        }
    }

    pub(crate) const fn surface(&self) -> WindowSurfaceSnapshot {
        self.surface
    }

    pub(crate) const fn buffer_line(&self) -> BufferLine {
        self.buffer_line
    }

    pub(crate) const fn window_handle(&self) -> i64 {
        self.surface.id().window_handle()
    }

    pub(crate) const fn buffer_handle(&self) -> i64 {
        self.surface.id().buffer_handle()
    }

    pub(crate) fn same_window_and_buffer(&self, other: &Self) -> bool {
        self.surface.id() == other.surface.id()
    }

    pub(crate) fn window_dimensions_changed(&self, other: &Self) -> bool {
        self.surface.window_size() != other.surface.window_size()
    }

    #[cfg(test)]
    pub(crate) fn fixture(window_handle: i64, buffer_handle: i64, top_row: i64, line: i64) -> Self {
        use crate::position::ScreenCell;
        use crate::position::SurfaceId;
        use crate::position::ViewportBounds;

        Self::new(
            WindowSurfaceSnapshot::new(
                SurfaceId::new(window_handle, buffer_handle).expect("positive handles"),
                BufferLine::new(top_row).expect("positive top buffer line"),
                0,
                0,
                ScreenCell::new(1, 1).expect("one-based window origin"),
                ViewportBounds::new(1, 1).expect("positive window size"),
            ),
            BufferLine::new(line).expect("positive cursor buffer line"),
        )
    }

    #[cfg(test)]
    pub(crate) fn with_viewport_columns(self, left_col: i64, text_offset: i64) -> Self {
        Self::new(
            WindowSurfaceSnapshot::new(
                self.surface.id(),
                self.surface.top_buffer_line(),
                u32::try_from(left_col).expect("non-negative left column"),
                u32::try_from(text_offset).expect("non-negative text offset"),
                self.surface.window_origin(),
                self.surface.window_size(),
            ),
            self.buffer_line,
        )
    }

    #[cfg(test)]
    pub(crate) fn with_window_origin(self, window_row: i64, window_col: i64) -> Self {
        use crate::position::ScreenCell;

        Self::new(
            WindowSurfaceSnapshot::new(
                self.surface.id(),
                self.surface.top_buffer_line(),
                self.surface.left_col0(),
                self.surface.text_offset0(),
                ScreenCell::new(window_row, window_col).expect("one-based window origin"),
                self.surface.window_size(),
            ),
            self.buffer_line,
        )
    }

    #[cfg(test)]
    pub(crate) fn with_window_dimensions(self, window_width: i64, window_height: i64) -> Self {
        use crate::position::ViewportBounds;

        Self::new(
            WindowSurfaceSnapshot::new(
                self.surface.id(),
                self.surface.top_buffer_line(),
                self.surface.left_col0(),
                self.surface.text_offset0(),
                self.surface.window_origin(),
                ViewportBounds::new(window_height, window_width).expect("positive window size"),
            ),
            self.buffer_line,
        )
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct CursorShape {
    cell_shape: CursorCellShape,
}

impl CursorShape {
    pub(crate) const fn block() -> Self {
        Self::from_cell_shape(CursorCellShape::Block)
    }

    pub(crate) const fn vertical_bar() -> Self {
        Self::from_cell_shape(CursorCellShape::VerticalBar)
    }

    pub(crate) const fn horizontal_bar() -> Self {
        Self::from_cell_shape(CursorCellShape::HorizontalBar)
    }

    pub(crate) const fn from_cell_shape(cell_shape: CursorCellShape) -> Self {
        Self { cell_shape }
    }

    pub(crate) const fn is_vertical_bar(self) -> bool {
        matches!(self.cell_shape, CursorCellShape::VerticalBar)
    }

    pub(crate) const fn is_horizontal_bar(self) -> bool {
        matches!(self.cell_shape, CursorCellShape::HorizontalBar)
    }

    pub(super) fn corners(self, position: RenderPoint) -> [RenderPoint; 4] {
        match self.cell_shape {
            CursorCellShape::Block => corners_for_cursor(position.row, position.col, false, false),
            CursorCellShape::VerticalBar => {
                corners_for_cursor(position.row, position.col, true, false)
            }
            CursorCellShape::HorizontalBar => {
                corners_for_cursor(position.row, position.col, false, true)
            }
        }
    }
}

impl From<CursorCellShape> for CursorShape {
    fn from(cell_shape: CursorCellShape) -> Self {
        Self::from_cell_shape(cell_shape)
    }
}

#[cfg(test)]
mod tests {
    use super::TrackedCursor;
    use crate::position::BufferLine;
    use crate::position::ScreenCell;
    use crate::position::SurfaceId;
    use crate::position::ViewportBounds;
    use crate::position::WindowSurfaceSnapshot;
    use pretty_assertions::assert_eq;

    fn surface_snapshot(
        top_buffer_line: i64,
        left_col0: u32,
        text_offset0: u32,
        window_row: i64,
        window_col: i64,
        window_height: i64,
        window_width: i64,
    ) -> WindowSurfaceSnapshot {
        WindowSurfaceSnapshot::new(
            SurfaceId::new(11, 17).expect("positive handles"),
            BufferLine::new(top_buffer_line).expect("positive top buffer line"),
            left_col0,
            text_offset0,
            ScreenCell::new(window_row, window_col).expect("one-based window origin"),
            ViewportBounds::new(window_height, window_width).expect("positive window size"),
        )
    }

    #[test]
    fn tracked_cursor_retains_surface_and_buffer_line() {
        let surface = surface_snapshot(23, 5, 2, 7, 13, 24, 80);
        let tracked = TrackedCursor::new(
            surface,
            BufferLine::new(29).expect("positive cursor buffer line"),
        );

        assert_eq!(tracked.surface(), surface);
        assert_eq!(
            tracked.buffer_line(),
            BufferLine::new(29).expect("positive cursor buffer line")
        );
        assert_eq!(tracked.window_handle(), 11);
        assert_eq!(tracked.buffer_handle(), 17);
    }

    #[test]
    fn tracked_cursor_detects_window_dimension_changes_from_surface_snapshots() {
        let tracked = TrackedCursor::new(
            surface_snapshot(23, 5, 2, 7, 13, 24, 80),
            BufferLine::new(29).expect("positive cursor buffer line"),
        );
        let resized = TrackedCursor::new(
            surface_snapshot(23, 5, 2, 7, 13, 24, 100),
            BufferLine::new(29).expect("positive cursor buffer line"),
        );
        assert_eq!(tracked.window_dimensions_changed(&resized), true);
        assert_eq!(tracked.same_window_and_buffer(&resized), true);
    }
}
