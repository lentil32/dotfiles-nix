use super::super::cursor::line_value;
use super::super::cursor::smear_outside_cmd_row;
use crate::core::effect::ObservationRuntimeContext;
use crate::core::runtime_reducer::ScrollShift;
use crate::lua::i64_from_object_ref_with_typed;
use crate::state::CursorLocation;
use nvim_oxi::Array;
use nvim_oxi::Dictionary;
use nvim_oxi::Object;
use nvim_oxi::Result;
use nvim_oxi::String as NvimString;
use nvim_oxi::api;
use nvim_oxi::api::opts::WinTextHeightOpts;
use nvim_oxi::conversion::FromObject;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
struct SurfaceTranslationDelta {
    vertical_rows: Option<(i64, i64)>,
    horizontal_cols: i64,
    window_row_delta: i64,
    window_col_delta: i64,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
struct WindowSurfaceMetrics {
    left_col: i64,
    text_offset: i64,
    window_row: i64,
    window_col: i64,
    window_width: i64,
    window_height: i64,
}

fn dictionary_i64_field(dict: &Dictionary, field: &str) -> Option<i64> {
    let field_key = NvimString::from(field);
    let value = dict.get(&field_key)?;
    i64_from_object_ref_with_typed(value, || format!("getwininfo.{field}")).ok()
}

fn current_window_surface_metrics(window: &api::Window) -> Option<WindowSurfaceMetrics> {
    let args = Array::from_iter([Object::from(window.handle())]);
    let [entry]: [Object; 1] =
        Vec::<Object>::from_object(api::call_function("getwininfo", args).ok()?)
            .ok()?
            .try_into()
            .ok()?;
    let dict = Dictionary::from_object(entry).ok()?;
    Some(WindowSurfaceMetrics {
        left_col: dictionary_i64_field(&dict, "leftcol")?,
        text_offset: dictionary_i64_field(&dict, "textoff")?,
        window_row: dictionary_i64_field(&dict, "winrow")?,
        window_col: dictionary_i64_field(&dict, "wincol")?,
        window_width: dictionary_i64_field(&dict, "width")?,
        window_height: dictionary_i64_field(&dict, "height")?,
    })
}

fn cursor_location_from_live_window_state(
    window: &api::Window,
    buffer: &api::Buffer,
    top_row: i64,
    line: i64,
    metrics: WindowSurfaceMetrics,
) -> CursorLocation {
    CursorLocation::new(
        i64::from(window.handle()),
        i64::from(buffer.handle()),
        top_row,
        line,
    )
    .with_viewport_columns(metrics.left_col, metrics.text_offset)
    .with_window_origin(metrics.window_row, metrics.window_col)
    .with_window_dimensions(metrics.window_width, metrics.window_height)
}

pub(crate) fn cursor_location_for_ingress_fast_path() -> Option<CursorLocation> {
    let window = api::get_current_win();
    let buffer = api::get_current_buf();
    if !window.is_valid() || !buffer.is_valid() {
        return None;
    }

    let top_row = line_value("w0").ok()?;
    let line = line_value(".").ok()?;
    let metrics = current_window_surface_metrics(&window)?;
    Some(cursor_location_from_live_window_state(
        &window, &buffer, top_row, line, metrics,
    ))
}

pub(crate) fn cursor_location_for_core_render(
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
        let (
            default_left_col,
            default_text_offset,
            default_window_row,
            default_window_col,
            default_window_width,
            default_window_height,
        ) = tracked_location.as_ref().map_or(
            (0_i64, 0_i64, 0_i64, 0_i64, 0_i64, 0_i64),
            |location| {
                (
                    location.left_col,
                    location.text_offset,
                    location.window_row,
                    location.window_col,
                    location.window_width,
                    location.window_height,
                )
            },
        );
        let top_row = line_value("w0").unwrap_or(default_top_row);
        let line = line_value(".").unwrap_or(default_line);
        let metrics = current_window_surface_metrics(&window).unwrap_or(WindowSurfaceMetrics {
            left_col: default_left_col,
            text_offset: default_text_offset,
            window_row: default_window_row,
            window_col: default_window_col,
            window_width: default_window_width,
            window_height: default_window_height,
        });
        return cursor_location_from_live_window_state(&window, &buffer, top_row, line, metrics);
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
        window
            .text_height(&opts)
            .map_or(0, |height| i64::from(height.all).saturating_sub(1))
    };

    if reversed {
        Ok(-(distance as f64))
    } else {
        Ok(distance as f64)
    }
}

fn surface_translation_delta(
    previous_location: &CursorLocation,
    current_location: &CursorLocation,
) -> Option<SurfaceTranslationDelta> {
    if previous_location.window_handle != current_location.window_handle
        || previous_location.buffer_handle != current_location.buffer_handle
    {
        return None;
    }
    if previous_location.window_dimensions_changed(current_location) {
        return None;
    }

    let vertical_rows = (previous_location.top_row != current_location.top_row)
        .then_some((previous_location.top_row, current_location.top_row));
    let horizontal_cols = (current_location.left_col - previous_location.left_col)
        - (current_location.text_offset - previous_location.text_offset);
    let window_row_delta = current_location.window_row - previous_location.window_row;
    let window_col_delta = current_location.window_col - previous_location.window_col;
    if vertical_rows.is_none()
        && horizontal_cols == 0
        && window_row_delta == 0
        && window_col_delta == 0
    {
        return None;
    }

    Some(SurfaceTranslationDelta {
        vertical_rows,
        horizontal_cols,
        window_row_delta,
        window_col_delta,
    })
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
    if !smear_outside_cmd_row(&context.current_corners())? {
        return Ok(None);
    }
    let Some(delta) = surface_translation_delta(&previous_location, current_location) else {
        return Ok(None);
    };

    let viewport_row_shift = match delta.vertical_rows {
        Some((previous_top_row, current_top_row)) => {
            screen_distance(window, previous_top_row, current_top_row)?
        }
        None => 0.0,
    };
    let row_shift = viewport_row_shift - delta.window_row_delta as f64;
    let col_shift = delta.horizontal_cols as f64 - delta.window_col_delta as f64;
    if row_shift == 0.0 && col_shift == 0.0 {
        return Ok(None);
    }
    let window_row = if current_location.window_row > 0 {
        current_location.window_row as f64
    } else {
        let (window_row_zero, _) = window.get_position()?;
        window_row_zero as f64 + 1.0
    };
    let window_height = f64::from(window.get_height()?);
    let min_row = window_row;
    let max_row = min_row + window_height - 1.0;

    Ok(Some(ScrollShift {
        row_shift,
        col_shift,
        min_row,
        max_row,
    }))
}

#[cfg(test)]
mod tests {
    use super::SurfaceTranslationDelta;
    use super::surface_translation_delta;
    use crate::state::CursorLocation;
    use pretty_assertions::assert_eq;

    fn assert_surface_translation_case(
        label: &str,
        previous: CursorLocation,
        current: CursorLocation,
        expected: SurfaceTranslationDelta,
    ) {
        assert_eq!(
            surface_translation_delta(&previous, &current),
            Some(expected),
            "{label}"
        );
    }

    #[test]
    fn surface_translation_delta_detects_surface_motion_cases() {
        for (label, previous, current, expected) in [
            (
                "same-line vertical scroll",
                CursorLocation::new(10, 20, 4, 12).with_viewport_columns(3, 1),
                CursorLocation::new(10, 20, 6, 12).with_viewport_columns(3, 1),
                SurfaceTranslationDelta {
                    vertical_rows: Some((4, 6)),
                    horizontal_cols: 0,
                    window_row_delta: 0,
                    window_col_delta: 0,
                },
            ),
            (
                "same-line horizontal scroll",
                CursorLocation::new(10, 20, 4, 12).with_viewport_columns(2, 1),
                CursorLocation::new(10, 20, 4, 12).with_viewport_columns(5, 1),
                SurfaceTranslationDelta {
                    vertical_rows: None,
                    horizontal_cols: 3,
                    window_row_delta: 0,
                    window_col_delta: 0,
                },
            ),
            (
                "text offset change",
                CursorLocation::new(10, 20, 4, 12).with_viewport_columns(5, 2),
                CursorLocation::new(10, 20, 4, 12).with_viewport_columns(5, 4),
                SurfaceTranslationDelta {
                    vertical_rows: None,
                    horizontal_cols: -2,
                    window_row_delta: 0,
                    window_col_delta: 0,
                },
            ),
            (
                "window origin shift without viewport motion",
                CursorLocation::new(10, 20, 4, 12)
                    .with_viewport_columns(2, 1)
                    .with_window_origin(3, 4),
                CursorLocation::new(10, 20, 4, 12)
                    .with_viewport_columns(2, 1)
                    .with_window_origin(5, 7),
                SurfaceTranslationDelta {
                    vertical_rows: None,
                    horizontal_cols: 0,
                    window_row_delta: 2,
                    window_col_delta: 3,
                },
            ),
        ] {
            assert_surface_translation_case(label, previous, current, expected);
        }
    }

    #[test]
    fn surface_translation_delta_ignores_window_dimension_changes() {
        let previous = CursorLocation::new(10, 20, 4, 12)
            .with_viewport_columns(2, 1)
            .with_window_origin(3, 4)
            .with_window_dimensions(80, 20);
        let current = CursorLocation::new(10, 20, 4, 12)
            .with_viewport_columns(2, 1)
            .with_window_origin(3, 4)
            .with_window_dimensions(72, 20);

        assert_eq!(surface_translation_delta(&previous, &current), None);
    }

    #[test]
    fn surface_translation_delta_ignores_cursor_motion_without_surface_motion() {
        let previous = CursorLocation::new(10, 20, 4, 12).with_viewport_columns(2, 1);
        let current = CursorLocation::new(10, 20, 4, 13).with_viewport_columns(2, 1);

        assert_eq!(surface_translation_delta(&previous, &current), None);
    }
}
