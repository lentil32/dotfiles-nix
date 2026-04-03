use crate::animation::corners_for_cursor;
use crate::types::Point;

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct CursorLocation {
    pub(crate) window_handle: i64,
    pub(crate) buffer_handle: i64,
    pub(crate) top_row: i64,
    pub(crate) line: i64,
    pub(crate) left_col: i64,
    pub(crate) text_offset: i64,
    pub(crate) window_row: i64,
    pub(crate) window_col: i64,
    pub(crate) window_width: i64,
    pub(crate) window_height: i64,
}

impl CursorLocation {
    pub(crate) const fn new(
        window_handle: i64,
        buffer_handle: i64,
        top_row: i64,
        line: i64,
    ) -> Self {
        Self {
            window_handle,
            buffer_handle,
            top_row,
            line,
            left_col: 0,
            text_offset: 0,
            window_row: 0,
            window_col: 0,
            window_width: 0,
            window_height: 0,
        }
    }

    pub(crate) fn with_viewport_columns(mut self, left_col: i64, text_offset: i64) -> Self {
        self.left_col = left_col;
        self.text_offset = text_offset;
        self
    }

    pub(crate) fn with_window_origin(mut self, window_row: i64, window_col: i64) -> Self {
        self.window_row = window_row;
        self.window_col = window_col;
        self
    }

    pub(crate) fn with_window_dimensions(mut self, window_width: i64, window_height: i64) -> Self {
        self.window_width = window_width;
        self.window_height = window_height;
        self
    }

    pub(crate) fn window_dimensions_changed(&self, other: &Self) -> bool {
        self.window_width != other.window_width || self.window_height != other.window_height
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct CursorShape {
    vertical_bar: bool,
    horizontal_bar: bool,
}

impl CursorShape {
    pub(crate) const fn new(vertical_bar: bool, horizontal_bar: bool) -> Self {
        Self {
            vertical_bar,
            horizontal_bar,
        }
    }

    pub(super) fn corners(self, position: Point) -> [Point; 4] {
        corners_for_cursor(
            position.row,
            position.col,
            self.vertical_bar,
            self.horizontal_bar,
        )
    }
}
