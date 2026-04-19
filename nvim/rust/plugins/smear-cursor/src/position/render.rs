//! Continuous render-space coordinates derived from discrete screen cells.

use super::validated::ScreenCell;

const DISPLAY_DISTANCE_EPSILON: f64 = 1.0e-9;

/// A continuous row/column coordinate in render space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RenderPoint {
    pub(crate) row: f64,
    pub(crate) col: f64,
}

impl From<ScreenCell> for RenderPoint {
    fn from(cell: ScreenCell) -> Self {
        Self {
            row: cell.row() as f64,
            col: cell.col() as f64,
        }
    }
}

impl RenderPoint {
    pub(crate) const ZERO: Self = Self { row: 0.0, col: 0.0 };

    pub(crate) fn distance_squared(self, other: Self) -> f64 {
        let dy = self.row - other.row;
        let dx = self.col - other.col;
        dy * dy + dx * dx
    }

    pub(crate) fn display_distance_squared(self, other: Self, block_aspect_ratio: f64) -> f64 {
        let dy = (self.row - other.row) * display_metric_row_scale(block_aspect_ratio);
        let dx = self.col - other.col;
        dy * dy + dx * dx
    }

    pub(crate) fn display_distance(self, other: Self, block_aspect_ratio: f64) -> f64 {
        self.display_distance_squared(other, block_aspect_ratio)
            .sqrt()
    }
}

pub(crate) fn display_metric_row_scale(block_aspect_ratio: f64) -> f64 {
    if block_aspect_ratio.is_finite() {
        block_aspect_ratio.abs().max(DISPLAY_DISTANCE_EPSILON)
    } else {
        1.0
    }
}

pub(crate) fn corners_center(corners: &[RenderPoint; 4]) -> RenderPoint {
    let mut row = 0.0_f64;
    let mut col = 0.0_f64;
    for point in corners {
        row += point.row;
        col += point.col;
    }
    RenderPoint {
        row: row / 4.0,
        col: col / 4.0,
    }
}

pub(crate) fn current_visual_cursor_anchor(
    current_corners: &[RenderPoint; 4],
    target_corners: &[RenderPoint; 4],
    target_position: RenderPoint,
) -> RenderPoint {
    let current_center = corners_center(current_corners);
    let target_center = corners_center(target_corners);
    RenderPoint {
        row: target_position.row + (current_center.row - target_center.row),
        col: target_position.col + (current_center.col - target_center.col),
    }
}
