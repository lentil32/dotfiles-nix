use crate::animation::corners_for_cursor;
use crate::types::Point;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct CursorLocation {
    pub(crate) window_handle: i64,
    pub(crate) buffer_handle: i64,
    pub(crate) top_row: i64,
    pub(crate) line: i64,
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
        }
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
