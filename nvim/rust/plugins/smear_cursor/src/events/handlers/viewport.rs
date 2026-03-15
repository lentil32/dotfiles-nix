use super::super::cursor::{line_value, smear_outside_cmd_row};
use crate::core::effect::ObservationRuntimeContext;
use crate::core::runtime_reducer::ScrollShift;
use crate::state::CursorLocation;
use nvim_oxi::api::opts::WinTextHeightOpts;
use nvim_oxi::{Result, api};

pub(super) fn cursor_location_for_core_render(
    tracked_location: Option<CursorLocation>,
) -> CursorLocation {
    let window = api::get_current_win();
    let buffer = api::get_current_buf();
    if window.is_valid() && buffer.is_valid() {
        let default_top_row = tracked_location
            .as_ref()
            .map_or(0_i64, |location| location.top_row);
        let default_line = tracked_location
            .as_ref()
            .map_or(0_i64, |location| location.line);
        let top_row = line_value("w0").unwrap_or(default_top_row);
        let line = line_value(".").unwrap_or(default_line);
        return CursorLocation::new(
            i64::from(window.handle()),
            i64::from(buffer.handle()),
            top_row,
            line,
        );
    }

    tracked_location.unwrap_or(CursorLocation::new(0, 0, 0, 0))
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

pub(super) fn maybe_scroll_shift_for_core_event(
    window: &api::Window,
    context: &ObservationRuntimeContext,
    current_location: &CursorLocation,
) -> Result<Option<ScrollShift>> {
    if !context.scroll_buffer_space() {
        return Ok(None);
    }
    let Some(previous_location) = context.tracked_location() else {
        return Ok(None);
    };
    if previous_location.window_handle != current_location.window_handle
        || previous_location.buffer_handle != current_location.buffer_handle
    {
        return Ok(None);
    }
    if !smear_outside_cmd_row(&context.current_corners())? {
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
