use super::cursor::{cursor_position_for_mode, mode_string, smear_outside_cmd_row};
use crate::draw::RenderFrame;
use crate::reducer::ScrollShift;
use crate::state::{CursorLocation, CursorSnapshot};
use crate::types::{EPSILON, Point};
use nvim_oxi::api::opts::WinTextHeightOpts;
use nvim_oxi::{Result, api};

fn point_inside_target_bounds(
    point: Point,
    target_min_row: f64,
    target_max_row: f64,
    target_min_col: f64,
    target_max_col: f64,
) -> bool {
    point.row >= target_min_row
        && point.row <= target_max_row
        && point.col >= target_min_col
        && point.col <= target_max_col
}

fn frame_center(corners: &[Point; 4]) -> Point {
    let mut row = 0.0_f64;
    let mut col = 0.0_f64;
    for point in corners {
        row += point.row;
        col += point.col;
    }
    Point {
        row: row / 4.0,
        col: col / 4.0,
    }
}

pub(super) fn frame_reaches_target_cell(frame: &RenderFrame) -> bool {
    let target_min_row = frame.target_corners[0].row;
    let target_max_row = frame.target_corners[2].row;
    let target_min_col = frame.target_corners[0].col;
    let target_max_col = frame.target_corners[2].col;
    let center = frame_center(&frame.corners);
    if point_inside_target_bounds(
        center,
        target_min_row,
        target_max_row,
        target_min_col,
        target_max_col,
    ) {
        return true;
    }

    frame.corners.iter().copied().any(|point| {
        point_inside_target_bounds(
            point,
            target_min_row,
            target_max_row,
            target_min_col,
            target_max_col,
        )
    })
}

fn line_index_1_to_0(row: i64) -> usize {
    let clamped = row.max(1).saturating_sub(1);
    usize::try_from(clamped).unwrap_or_default()
}

fn screen_distance(window: &api::Window, row_start: i64, row_end: i64) -> Result<f64> {
    let mut start = row_start;
    let mut end = row_end;
    let mut reversed = false;
    if start > end {
        std::mem::swap(&mut start, &mut end);
        reversed = true;
    }

    let window_height = i64::from(window.get_height()?);
    let distance = if end.saturating_sub(start) >= window_height {
        window_height.saturating_sub(1)
    } else {
        let opts = WinTextHeightOpts::builder()
            .start_row(line_index_1_to_0(start))
            .end_row(line_index_1_to_0(end))
            .build();
        match window.text_height(&opts) {
            Ok(height) => i64::from(height.all).saturating_sub(1),
            Err(_) => 0,
        }
    };

    if reversed {
        Ok(-(distance as f64))
    } else {
        Ok(distance as f64)
    }
}

pub(super) fn maybe_scroll_shift(
    window: &api::Window,
    scroll_buffer_space: bool,
    current_corners: &[Point; 4],
    previous_location: Option<CursorLocation>,
    current_location: CursorLocation,
) -> Result<Option<ScrollShift>> {
    if !scroll_buffer_space {
        return Ok(None);
    }
    let Some(previous_location) = previous_location else {
        return Ok(None);
    };
    if previous_location.window_handle != current_location.window_handle
        || previous_location.buffer_handle != current_location.buffer_handle
    {
        return Ok(None);
    }
    if !smear_outside_cmd_row(current_corners)? {
        return Ok(None);
    }
    if previous_location.top_row == current_location.top_row
        || previous_location.line == current_location.line
    {
        return Ok(None);
    }

    let shift = screen_distance(window, previous_location.top_row, current_location.top_row)?;
    let (window_row_zero, _) = window.get_position()?;
    let window_height = f64::from(window.get_height()?);
    let min_row = window_row_zero as f64 + 1.0;
    let max_row = min_row + window_height - 1.0;

    Ok(Some(ScrollShift {
        shift,
        min_row,
        max_row,
    }))
}

pub(super) fn current_cursor_snapshot(smear_to_cmd: bool) -> Result<Option<CursorSnapshot>> {
    let mode = mode_string();

    let window = api::get_current_win();
    if !window.is_valid() {
        return Ok(None);
    }

    let Some((row, col)) = cursor_position_for_mode(&window, &mode, smear_to_cmd)? else {
        return Ok(None);
    };

    Ok(Some(CursorSnapshot { mode, row, col }))
}

pub(super) fn snapshots_match(lhs: &CursorSnapshot, rhs: &CursorSnapshot) -> bool {
    lhs.mode == rhs.mode
        && (lhs.row - rhs.row).abs() <= EPSILON
        && (lhs.col - rhs.col).abs() <= EPSILON
}
