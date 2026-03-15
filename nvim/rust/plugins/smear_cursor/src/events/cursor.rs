use super::CURSOR_COLOR_LUAEVAL_EXPR;
use super::logging::trace_lazy;
use crate::lua::{i64_from_object, i64_from_object_ref_with, string_from_object};
use crate::types::Point;
use nvim_oxi::api::opts::OptionOpts;
use nvim_oxi::conversion::FromObject;
use nvim_oxi::{Array, Dictionary, Object, Result, String as NvimString, api};
use nvim_utils::mode::is_cmdline_mode;

type ScreenCell = (i64, i64);
type ScreenPoint = (f64, f64);

pub(super) fn mode_string() -> String {
    api::get_mode().mode.to_string_lossy().into_owned()
}

fn dictionary_i64_field(dict: &Dictionary, context: &str, field: &str) -> Result<Option<i64>> {
    let field_key = NvimString::from(field);
    let Some(value) = dict.get(&field_key) else {
        return Ok(None);
    };

    i64_from_object_ref_with(value, || format!("{context}.{field}")).map(Some)
}

fn screenpos_dictionary(screenpos: Object) -> Result<Dictionary> {
    Dictionary::from_object(screenpos).map_err(|_| {
        nvim_oxi::api::Error::Other("screenpos returned invalid dictionary".into()).into()
    })
}

fn parse_screenpos_cell_from_dict(dict: &Dictionary) -> Result<Option<ScreenCell>> {
    let row = dictionary_i64_field(dict, "screenpos", "row")?;
    let col = dictionary_i64_field(dict, "screenpos", "col")?;

    match (row, col) {
        (None, None) => Ok(None),
        (Some(row), Some(col)) if row > 0 && col > 0 => Ok(Some((row, col))),
        (Some(_), Some(_)) => Ok(None),
        _ => Err(nvim_oxi::api::Error::Other("screenpos missing row/col".into()).into()),
    }
}

fn parse_screenpos_cell(screenpos: Object) -> Result<Option<ScreenCell>> {
    parse_screenpos_cell_from_dict(&screenpos_dictionary(screenpos)?)
}

fn buffer_column_to_col1(column: usize) -> i64 {
    i64::try_from(column.saturating_add(1)).unwrap_or(i64::MAX)
}

fn screen_cell_to_point((row, col): ScreenCell) -> ScreenPoint {
    (row as f64, col as f64)
}

struct BufferCursorRead {
    line: usize,
    column: usize,
    screenpos_summary: String,
    raw_position: Option<ScreenPoint>,
    resolved_position: Option<ScreenPoint>,
}

impl BufferCursorRead {
    fn conceal_adjusted_position(&self) -> Option<ScreenPoint> {
        match (self.raw_position, self.resolved_position) {
            (Some(raw_position), Some(resolved_position)) if raw_position != resolved_position => {
                Some(resolved_position)
            }
            _ => None,
        }
    }

    fn selected_position(&self) -> Option<ScreenPoint> {
        self.resolved_position
    }

    fn selected_source(&self) -> &'static str {
        match (self.raw_position, self.resolved_position) {
            (Some(raw_position), Some(resolved_position)) if raw_position != resolved_position => {
                "screenpos_conceal_adjusted"
            }
            (Some(_), Some(_)) => "screenpos",
            _ => "none",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ConcealRegion {
    start_col1: i64,
    end_col1: i64,
    match_id: i64,
    replacement_width: i64,
}

fn screenpos_field_summary(dict: &Dictionary, field: &str) -> String {
    match dictionary_i64_field(dict, "screenpos", field) {
        Ok(Some(value)) => value.to_string(),
        Ok(None) => "none".to_string(),
        Err(_) => "invalid".to_string(),
    }
}

fn screenpos_summary(dict: &Dictionary) -> String {
    format!(
        "row={} col={} endcol={} curscol={}",
        screenpos_field_summary(dict, "row"),
        screenpos_field_summary(dict, "col"),
        screenpos_field_summary(dict, "endcol"),
        screenpos_field_summary(dict, "curscol"),
    )
}

fn trace_screen_cursor_read(window: &api::Window, buffer_read: &BufferCursorRead) {
    trace_lazy(|| {
        let buffer_summary = buffer_read.raw_position.map_or_else(
            || "none".to_string(),
            |(row, col)| format!("{row:.1}:{col:.1}"),
        );
        let conceal_adjusted_summary = buffer_read.conceal_adjusted_position().map_or_else(
            || "none".to_string(),
            |(row, col)| format!("{row:.1}:{col:.1}"),
        );
        let selected = buffer_read.selected_position();
        let selected_source = buffer_read.selected_source();
        let selected_summary = selected.map_or_else(
            || "none".to_string(),
            |(row, col)| format!("{row:.1}:{col:.1}"),
        );

        format!(
            "cursor_read win={} cursor=line:{} byte_col0={} screenpos_arg_col={} screenpos=({}) buffer_parsed={} conceal_adjusted={} selected={} selected_source={}",
            window.handle(),
            buffer_read.line,
            buffer_read.column,
            buffer_read.column.saturating_add(1),
            buffer_read.screenpos_summary,
            buffer_summary,
            conceal_adjusted_summary,
            selected_summary,
            selected_source,
        )
    });
}

fn replacement_display_width(replacement: &str) -> Result<i64> {
    if replacement.is_empty() {
        return Ok(0);
    }

    let args = Array::from_iter([Object::from(replacement)]);
    let width = api::call_function("strdisplaywidth", args)?;
    i64_from_object("strdisplaywidth", width)
}

fn parse_synconcealed(value: Object) -> Result<Option<(String, i64)>> {
    let [concealed, replacement, match_id]: [Object; 3] = Vec::<Object>::from_object(value)
        .map_err(|_| nvim_oxi::api::Error::Other("synconcealed returned invalid list".into()))?
        .try_into()
        .map_err(|_| {
            nvim_oxi::api::Error::Other("synconcealed returned unexpected list length".into())
        })?;

    let concealed = i64_from_object("synconcealed[0]", concealed)?;
    if concealed == 0 {
        return Ok(None);
    }

    let replacement = string_from_object("synconcealed[1]", replacement)?;
    let match_id = i64_from_object("synconcealed[2]", match_id)?;
    Ok(Some((replacement, match_id)))
}

fn concealed_regions_before_cursor(line: usize, column: usize) -> Result<Vec<ConcealRegion>> {
    let mut regions: Vec<ConcealRegion> = Vec::new();
    let line = i64::try_from(line).unwrap_or(i64::MAX);
    let max_col1 = i64::try_from(column).unwrap_or(i64::MAX);
    for col1 in 1..=max_col1 {
        let args = Array::from_iter([Object::from(line), Object::from(col1)]);
        let concealed = parse_synconcealed(api::call_function("synconcealed", args)?)?;
        let Some((replacement, match_id)) = concealed else {
            continue;
        };

        if let Some(last) = regions.last_mut()
            && last.match_id == match_id
            && last.end_col1.saturating_add(1) == col1
        {
            last.end_col1 = col1;
            continue;
        }

        regions.push(ConcealRegion {
            start_col1: col1,
            end_col1: col1,
            match_id,
            replacement_width: replacement_display_width(&replacement)?,
        });
    }
    Ok(regions)
}

fn screenpos_for_buffer_column(window: &api::Window, line: usize, col1: i64) -> Result<Object> {
    let args = Array::from_iter([
        Object::from(window.handle()),
        Object::from(i64::try_from(line).unwrap_or(i64::MAX)),
        Object::from(col1),
    ]);
    Ok(api::call_function("screenpos", args)?)
}

fn apply_conceal_delta(raw_cell: ScreenCell, conceal_delta: i64) -> ScreenPoint {
    (
        raw_cell.0 as f64,
        raw_cell.1.saturating_sub(conceal_delta).max(1) as f64,
    )
}

fn resolve_buffer_cursor_position(
    window: &api::Window,
    line: usize,
    column: usize,
    raw_cell: ScreenCell,
) -> Result<ScreenPoint> {
    if column == 0 {
        return Ok(screen_cell_to_point(raw_cell));
    }

    let current_col1 = buffer_column_to_col1(column);
    let mut conceal_delta = 0_i64;
    for region in concealed_regions_before_cursor(line, column)? {
        let start = parse_screenpos_cell(screenpos_for_buffer_column(
            window,
            line,
            region.start_col1,
        )?)?;
        let next_col1 = region.end_col1.saturating_add(1);
        let end = if next_col1 == current_col1 {
            Some(raw_cell)
        } else {
            parse_screenpos_cell(screenpos_for_buffer_column(window, line, next_col1)?)?
        };

        let (Some((start_row, start_col)), Some((end_row, end_col))) = (start, end) else {
            continue;
        };
        if start_row != end_row {
            // Comment: this conceal correction is proven for same-row drift. If a concealed region
            // crosses a soft-wrap boundary, keep the shell-authoritative raw screenpos until we
            // model wrapped line offsets explicitly.
            return Ok(screen_cell_to_point(raw_cell));
        }

        let raw_width = end_col.saturating_sub(start_col);
        conceal_delta =
            conceal_delta.saturating_add(raw_width.saturating_sub(region.replacement_width).max(0));
    }

    Ok(apply_conceal_delta(raw_cell, conceal_delta))
}

fn buffer_screen_cursor_position(window: &api::Window) -> Result<BufferCursorRead> {
    let (line, column) = window.get_cursor()?;
    let screenpos = screenpos_for_buffer_column(window, line, buffer_column_to_col1(column))?;
    let screenpos = screenpos_dictionary(screenpos)?;
    // Comment: `screenpos.curscol` points at the cursor landing column, which is the end of a
    // Tab expansion. The smear target needs the first screen cell that renders the buffer
    // character under the cursor, which is `screenpos.col`.
    let raw_cell = parse_screenpos_cell_from_dict(&screenpos)?;
    let raw_position = raw_cell.map(screen_cell_to_point);
    let resolved_position = raw_cell
        .map(|raw_cell| resolve_buffer_cursor_position(window, line, column, raw_cell))
        .transpose()?;
    Ok(BufferCursorRead {
        line,
        column,
        screenpos_summary: screenpos_summary(&screenpos),
        raw_position,
        resolved_position,
    })
}

fn screen_cursor_position(window: &api::Window) -> Result<Option<ScreenPoint>> {
    let buffer_read = buffer_screen_cursor_position(window)?;
    // Comment: `screenpos()` is the stable callback-safe base here. The `gg` trace showed
    // `screenrow()`/`screencol()` reporting stale or command-line cells on scheduled edges, so we
    // keep the timing-sensitive live probe out of production selection and correct conceal drift
    // from the stable `screenpos()` sample instead.
    trace_screen_cursor_read(window, &buffer_read);
    Ok(buffer_read.selected_position())
}

fn command_row_from_dimensions(lines: i64, cmdheight: i64) -> i64 {
    let visible_cmdheight = cmdheight.max(1);
    lines.saturating_sub(visible_cmdheight).saturating_add(1)
}

fn command_type_string() -> Result<String> {
    let value = api::call_function("getcmdtype", Array::new())?;
    string_from_object("getcmdtype", value)
}

fn should_use_real_cmdline_cursor(cmdtype: &str) -> bool {
    !cmdtype.is_empty()
}

fn cmdline_cursor_position(window: &api::Window) -> Result<Option<(f64, f64)>> {
    let cmdtype = command_type_string()?;
    if !should_use_real_cmdline_cursor(&cmdtype) {
        // Comment: showcmd and normal-mode prefix keys can transiently report `mode=c` while the
        // rendered cursor is still in the buffer. Falling back to the buffer cursor avoids
        // animating bottom-row showcmd columns for motions like `gg`.
        return Ok(buffer_screen_cursor_position(window)?.selected_position());
    }

    let screen_col_value = api::call_function("getcmdscreenpos", Array::new())?;
    let screen_col = i64_from_object("getcmdscreenpos", screen_col_value)?;
    if screen_col <= 0 {
        return Ok(None);
    }

    Ok(Some((command_row()?, screen_col as f64)))
}

pub(super) fn cursor_position_for_mode(
    window: &api::Window,
    mode: &str,
    smear_to_cmd: bool,
) -> Result<Option<(f64, f64)>> {
    if is_cmdline_mode(mode) {
        if !smear_to_cmd {
            return Ok(None);
        }
        return cmdline_cursor_position(window);
    }
    screen_cursor_position(window)
}

fn current_buffer_option_string(buffer: &api::Buffer, option_name: &str) -> Result<String> {
    let opts = OptionOpts::builder().buf(buffer.clone()).build();
    let value: String = api::get_option_value(option_name, &opts)?;
    Ok(value)
}

pub(super) fn current_buffer_filetype(buffer: &api::Buffer) -> Result<String> {
    current_buffer_option_string(buffer, "filetype")
}

fn cursor_color_at_current_position() -> Result<Option<String>> {
    let args = Array::from_iter([Object::from(CURSOR_COLOR_LUAEVAL_EXPR)]);
    let value: Object = api::call_function("luaeval", args)?;
    if value.is_nil() {
        return Ok(None);
    }
    Ok(Some(string_from_object("cursor_color_luaeval", value)?))
}

pub(super) fn sampled_cursor_color_at_current_position() -> Result<Option<String>> {
    cursor_color_at_current_position()
}

pub(super) fn line_value(key: &str) -> Result<i64> {
    let args = Array::from_iter([Object::from(key)]);
    let value = api::call_function("line", args)?;
    i64_from_object("line", value)
}

fn command_row() -> Result<f64> {
    let opts = nvim_oxi::api::opts::OptionOpts::builder().build();
    let lines: i64 = api::get_option_value("lines", &opts)?;
    let cmdheight: i64 = api::get_option_value("cmdheight", &opts)?;
    Ok(command_row_from_dimensions(lines, cmdheight) as f64)
}

pub(super) fn smear_outside_cmd_row(corners: &[Point; 4]) -> Result<bool> {
    let cmd_row = command_row()?;
    Ok(corners.iter().any(|point| point.row < cmd_row))
}

#[cfg(test)]
mod tests {
    use super::{
        BufferCursorRead, apply_conceal_delta, command_row_from_dimensions, parse_screenpos_cell,
        screen_cell_to_point, should_use_real_cmdline_cursor,
    };
    use nvim_oxi::{Dictionary, Object};

    fn screenpos_object(row: i64, col: i64, endcol: i64, curscol: i64) -> Object {
        let mut dict = Dictionary::new();
        dict.insert("row", Object::from(row));
        dict.insert("col", Object::from(col));
        dict.insert("endcol", Object::from(endcol));
        dict.insert("curscol", Object::from(curscol));
        Object::from(dict)
    }

    #[test]
    fn parse_screenpos_cell_uses_first_screen_column() {
        let position =
            parse_screenpos_cell(screenpos_object(4, 3, 8, 8)).expect("screenpos should parse");

        assert_eq!(position, Some((4, 3)));
    }

    #[test]
    fn parse_screenpos_cell_returns_none_for_hidden_or_invalid_positions() {
        let hidden = parse_screenpos_cell(screenpos_object(0, 0, 0, 0))
            .expect("hidden screenpos should parse");
        let invalid = parse_screenpos_cell(Object::from(Dictionary::new()))
            .expect("empty screenpos dictionary should map to none");

        assert_eq!(hidden, None);
        assert_eq!(invalid, None);
    }

    #[test]
    fn screen_cell_to_point_maps_screen_cells_to_points() {
        assert_eq!(screen_cell_to_point((16, 33)), (16.0, 33.0));
    }

    #[test]
    fn apply_conceal_delta_moves_cursor_left_without_changing_row() {
        let adjusted = apply_conceal_delta((2, 38), 5);

        assert_eq!(adjusted, (2.0, 33.0));
    }

    #[test]
    fn buffer_cursor_read_prefers_conceal_adjusted_position() {
        let read = BufferCursorRead {
            line: 2,
            column: 37,
            screenpos_summary: "row=2 col=38 endcol=38 curscol=38".to_string(),
            raw_position: Some((2.0, 38.0)),
            resolved_position: Some((2.0, 33.0)),
        };

        assert_eq!(read.selected_position(), Some((2.0, 33.0)));
        assert_eq!(read.conceal_adjusted_position(), Some((2.0, 33.0)));
        assert_eq!(read.selected_source(), "screenpos_conceal_adjusted");
    }

    #[test]
    fn buffer_cursor_read_reports_screenpos_source_when_unadjusted() {
        let read = BufferCursorRead {
            line: 2,
            column: 37,
            screenpos_summary: "row=2 col=38 endcol=38 curscol=38".to_string(),
            raw_position: Some((2.0, 38.0)),
            resolved_position: Some((2.0, 38.0)),
        };

        assert_eq!(read.selected_position(), Some((2.0, 38.0)));
        assert_eq!(read.conceal_adjusted_position(), None);
        assert_eq!(read.selected_source(), "screenpos");
    }

    #[test]
    fn command_row_from_dimensions_treats_cmdheight_zero_as_visible_bottom_row() {
        assert_eq!(command_row_from_dimensions(24, 0), 24);
        assert_eq!(command_row_from_dimensions(24, 2), 23);
    }

    #[test]
    fn empty_command_type_uses_buffer_cursor_fallback() {
        assert!(!should_use_real_cmdline_cursor(""));
        assert!(should_use_real_cmdline_cursor(":"));
    }
}
