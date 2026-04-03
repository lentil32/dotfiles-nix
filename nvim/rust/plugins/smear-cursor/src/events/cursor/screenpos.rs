use super::CursorParseError;
use super::CursorResult;
use super::ScreenCell;
use super::ScreenPoint;
use super::conceal::cursor_position_sync_for_raw_screenpos;
use super::conceal::resolve_buffer_cursor_position;
use super::cursor_parse_error;
use crate::core::effect::ProbePolicy;
use crate::core::effect::ProbeQuality;
use crate::core::state::CursorPositionSync;
use crate::events::logging::trace_lazy;
use crate::events::runtime::record_conceal_raw_screenpos_fallback;
use crate::lua::i64_from_object_ref_with_typed;
use crate::lua::i64_from_object_typed;
use crate::types::Point;
use nvim_oxi::Array;
use nvim_oxi::Dictionary;
use nvim_oxi::Object;
use nvim_oxi::Result;
use nvim_oxi::String as NvimString;
use nvim_oxi::api;
use nvim_oxi::api::opts::OptionOpts;
use nvim_oxi::conversion::FromObject;
use nvimrs_nvim_utils::mode::is_cmdline_mode;

pub(crate) fn mode_string() -> String {
    api::get_mode().mode.to_string_lossy().into_owned()
}

fn dictionary_i64_field(
    dict: &Dictionary,
    context: &str,
    field: &str,
) -> CursorResult<Option<i64>> {
    let field_key = NvimString::from(field);
    let Some(value) = dict.get(&field_key) else {
        return Ok(None);
    };

    i64_from_object_ref_with_typed(value, || format!("{context}.{field}"))
        .map(Some)
        .map_err(|source| cursor_parse_error(format!("{context}.{field}"), source))
}

pub(super) fn required_dictionary_i64_field(
    dict: &Dictionary,
    context: &'static str,
    field: &'static str,
) -> CursorResult<i64> {
    dictionary_i64_field(dict, context, field)?
        .ok_or(CursorParseError::DictionaryMissingField { context, field }.into())
}

fn screenpos_dictionary(screenpos: Object) -> CursorResult<Dictionary> {
    Dictionary::from_object(screenpos).map_err(|_| CursorParseError::ScreenposDictionary.into())
}

pub(super) fn parse_screenpos_cell_from_dict(
    dict: &Dictionary,
) -> CursorResult<Option<ScreenCell>> {
    let row = dictionary_i64_field(dict, "screenpos", "row")?;
    let col = dictionary_i64_field(dict, "screenpos", "col")?;

    match (row, col) {
        (None, None) => Ok(None),
        (Some(row), Some(col)) if row > 0 && col > 0 => Ok(Some((row, col))),
        (Some(_), Some(_)) => Ok(None),
        _ => Err(CursorParseError::ScreenposMissingRowCol.into()),
    }
}

pub(super) fn parse_screenpos_cell(screenpos: Object) -> CursorResult<Option<ScreenCell>> {
    parse_screenpos_cell_from_dict(&screenpos_dictionary(screenpos)?)
}

pub(super) fn buffer_column_to_col1(column: usize) -> i64 {
    i64::try_from(column.saturating_add(1)).unwrap_or(i64::MAX)
}

pub(super) fn screen_cell_to_point((row, col): ScreenCell) -> ScreenPoint {
    (row as f64, col as f64)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CursorPositionRead {
    position: Option<ScreenPoint>,
    sync: CursorPositionSync,
}

impl CursorPositionRead {
    const fn new(position: Option<ScreenPoint>, sync: CursorPositionSync) -> Self {
        Self { position, sync }
    }

    pub(crate) const fn position(self) -> Option<ScreenPoint> {
        self.position
    }

    pub(crate) const fn sync(self) -> CursorPositionSync {
        self.sync
    }
}

#[derive(Debug, Clone)]
pub(super) struct BufferCursorRead {
    pub(super) line: usize,
    pub(super) column: usize,
    pub(super) screenpos_summary: String,
    pub(super) raw_position: Option<ScreenPoint>,
    pub(super) resolved_position: Option<ScreenPoint>,
    pub(super) raw_position_sync: CursorPositionSync,
}

impl BufferCursorRead {
    pub(super) fn conceal_adjusted_position(&self) -> Option<ScreenPoint> {
        match (self.raw_position, self.resolved_position) {
            (Some(raw_position), Some(resolved_position)) if raw_position != resolved_position => {
                Some(resolved_position)
            }
            _ => None,
        }
    }

    pub(super) fn selected_position(&self) -> Option<ScreenPoint> {
        self.resolved_position
    }

    pub(super) fn selected_position_for_probe_policy(
        &self,
        probe_policy: ProbePolicy,
    ) -> Option<ScreenPoint> {
        if probe_policy.uses_raw_screenpos_fallback() {
            self.raw_position.or(self.resolved_position)
        } else {
            self.selected_position()
        }
    }

    pub(super) fn position_sync_for_probe_policy(
        &self,
        probe_policy: ProbePolicy,
    ) -> CursorPositionSync {
        if self.uses_raw_screenpos_fallback(probe_policy) {
            self.raw_position_sync
        } else {
            CursorPositionSync::Exact
        }
    }

    pub(super) fn selected_read_for_probe_policy(
        &self,
        probe_policy: ProbePolicy,
    ) -> CursorPositionRead {
        CursorPositionRead::new(
            self.selected_position_for_probe_policy(probe_policy),
            self.position_sync_for_probe_policy(probe_policy),
        )
    }

    pub(super) fn selected_source(&self) -> &'static str {
        match (self.raw_position, self.resolved_position) {
            (Some(raw_position), Some(resolved_position)) if raw_position != resolved_position => {
                "screenpos_conceal_adjusted"
            }
            (Some(_), Some(_)) => "screenpos",
            _ => "none",
        }
    }

    pub(super) fn selected_source_for_probe_policy(
        &self,
        probe_policy: ProbePolicy,
    ) -> &'static str {
        if !probe_policy.uses_raw_screenpos_fallback() {
            return self.selected_source();
        }

        if self.raw_position.is_some() {
            "screenpos_fast_path"
        } else {
            "none"
        }
    }

    pub(super) fn uses_raw_screenpos_fallback(&self, probe_policy: ProbePolicy) -> bool {
        probe_policy.uses_raw_screenpos_fallback() && self.raw_position.is_some()
    }
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

fn trace_screen_cursor_read(
    window: &api::Window,
    buffer_read: &BufferCursorRead,
    probe_policy: ProbePolicy,
) {
    trace_lazy(|| {
        let buffer_summary = buffer_read.raw_position.map_or_else(
            || "none".to_string(),
            |(row, col)| format!("{row:.1}:{col:.1}"),
        );
        let conceal_adjusted_summary = buffer_read.conceal_adjusted_position().map_or_else(
            || "none".to_string(),
            |(row, col)| format!("{row:.1}:{col:.1}"),
        );
        let selected = buffer_read.selected_position_for_probe_policy(probe_policy);
        let selected_source = buffer_read.selected_source_for_probe_policy(probe_policy);
        let selected_summary = selected.map_or_else(
            || "none".to_string(),
            |(row, col)| format!("{row:.1}:{col:.1}"),
        );

        format!(
            "cursor_read win={} cursor=line:{} byte_col0={} screenpos_arg_col={} screenpos=({}) buffer_parsed={} conceal_adjusted={} selected={} selected_source={} probe_policy={}",
            window.handle(),
            buffer_read.line,
            buffer_read.column,
            buffer_read.column.saturating_add(1),
            buffer_read.screenpos_summary,
            buffer_summary,
            conceal_adjusted_summary,
            selected_summary,
            selected_source,
            probe_policy.diagnostic_name(),
        )
    });
}

pub(super) fn current_window_option<T>(window: &api::Window, option_name: &str) -> Result<T>
where
    T: FromObject,
{
    let opts = OptionOpts::builder().win(window.clone()).build();
    Ok(api::get_option_value(option_name, &opts)?)
}

pub(super) fn screenpos_for_buffer_column(
    window: &api::Window,
    line: usize,
    col1: i64,
) -> Result<Object> {
    let args = Array::from_iter([
        Object::from(window.handle()),
        Object::from(i64::try_from(line).unwrap_or(i64::MAX)),
        Object::from(col1),
    ]);
    Ok(api::call_function("screenpos", args)?)
}

fn buffer_screen_cursor_position(
    window: &api::Window,
    mode: &str,
    probe_policy: ProbePolicy,
) -> CursorResult<BufferCursorRead> {
    let (line, column) = window.get_cursor()?;
    let screenpos = screenpos_for_buffer_column(window, line, buffer_column_to_col1(column))?;
    let screenpos = screenpos_dictionary(screenpos)?;
    // `screenpos.curscol` points at the cursor landing column, which is the end of a Tab
    // expansion. The smear target needs the first screen cell that renders the buffer character
    // under the cursor, which is `screenpos.col`.
    let raw_cell = parse_screenpos_cell_from_dict(&screenpos)?;
    let raw_position = raw_cell.map(screen_cell_to_point);
    let (resolved_position, raw_position_sync) = match raw_cell {
        Some(raw_cell) if probe_policy.uses_raw_screenpos_fallback() => (
            raw_position,
            cursor_position_sync_for_raw_screenpos(window, line, column, mode, raw_cell)?,
        ),
        Some(raw_cell) => (
            Some(resolve_buffer_cursor_position(
                window, line, column, mode, raw_cell,
            )?),
            CursorPositionSync::Exact,
        ),
        None => (None, CursorPositionSync::Exact),
    };
    Ok(BufferCursorRead {
        line,
        column,
        screenpos_summary: screenpos_summary(&screenpos),
        raw_position,
        resolved_position,
        raw_position_sync,
    })
}

fn screen_cursor_position(
    window: &api::Window,
    mode: &str,
    probe_policy: ProbePolicy,
) -> CursorResult<CursorPositionRead> {
    let buffer_read = buffer_screen_cursor_position(window, mode, probe_policy)?;
    // `screenpos()` is the stable callback-safe base here. The `gg` trace showed
    // `screenrow()`/`screencol()` reporting stale or command-line cells on scheduled edges, so we
    // keep the timing-sensitive live probe out of production selection. Fast motion can stay on
    // the raw `screenpos()` sample, while exact edges pay for conceal correction to re-sync.
    trace_screen_cursor_read(window, &buffer_read, probe_policy);
    if buffer_read.uses_raw_screenpos_fallback(probe_policy) {
        record_conceal_raw_screenpos_fallback();
    }
    Ok(buffer_read.selected_read_for_probe_policy(probe_policy))
}

pub(super) fn command_row_from_dimensions(lines: i64, cmdheight: i64) -> i64 {
    let visible_cmdheight = cmdheight.max(1);
    lines.saturating_sub(visible_cmdheight).saturating_add(1)
}

fn command_type_string() -> CursorResult<String> {
    let value = api::call_function("getcmdtype", Array::new())?;
    crate::lua::string_from_object_typed("getcmdtype", value)
        .map_err(|source| cursor_parse_error("getcmdtype", source))
}

pub(super) fn should_use_real_cmdline_cursor(cmdtype: &str) -> bool {
    !cmdtype.is_empty()
}

fn cmdline_cursor_position(
    window: &api::Window,
    probe_policy: ProbePolicy,
) -> CursorResult<CursorPositionRead> {
    let cmdtype = command_type_string()?;
    if !should_use_real_cmdline_cursor(&cmdtype) {
        // showcmd and normal-mode prefix keys can transiently report `mode=c` while the rendered
        // cursor is still in the buffer. Falling back to the buffer cursor avoids animating
        // bottom-row showcmd columns for motions like `gg`, while preserving any deferred
        // conceal correction that still needs an exact settle-time pass.
        return Ok(buffer_screen_cursor_position(window, "n", probe_policy)?
            .selected_read_for_probe_policy(probe_policy));
    }

    let screen_col_value = api::call_function("getcmdscreenpos", Array::new())?;
    let screen_col = i64_from_object_typed("getcmdscreenpos", screen_col_value)
        .map_err(|source| cursor_parse_error("getcmdscreenpos", source))?;
    if screen_col <= 0 {
        return Ok(CursorPositionRead::new(None, CursorPositionSync::Exact));
    }

    Ok(CursorPositionRead::new(
        Some((command_row()?, screen_col as f64)),
        CursorPositionSync::Exact,
    ))
}

pub(crate) fn cursor_position_for_mode(
    window: &api::Window,
    mode: &str,
    smear_to_cmd: bool,
) -> Result<Option<(f64, f64)>> {
    cursor_position_read_for_mode_with_probe_policy(
        window,
        mode,
        smear_to_cmd,
        ProbePolicy::new(ProbeQuality::Exact),
    )
    .map(CursorPositionRead::position)
}

pub(crate) fn cursor_position_read_for_mode_with_probe_policy(
    window: &api::Window,
    mode: &str,
    smear_to_cmd: bool,
    probe_policy: ProbePolicy,
) -> Result<CursorPositionRead> {
    if is_cmdline_mode(mode) {
        if !smear_to_cmd {
            return Ok(CursorPositionRead::new(None, CursorPositionSync::Exact));
        }
        return cmdline_cursor_position(window, probe_policy).map_err(nvim_oxi::Error::from);
    }
    screen_cursor_position(window, mode, probe_policy).map_err(nvim_oxi::Error::from)
}

pub(crate) fn line_value(key: &str) -> Result<i64> {
    let args = Array::from_iter([Object::from(key)]);
    let value = api::call_function("line", args)?;
    i64_from_object_typed("line", value)
        .map_err(|source| nvim_oxi::Error::from(cursor_parse_error(format!("line({key})"), source)))
}

fn command_row() -> Result<f64> {
    let opts = nvim_oxi::api::opts::OptionOpts::builder().build();
    let lines: i64 = api::get_option_value("lines", &opts)?;
    let cmdheight: i64 = api::get_option_value("cmdheight", &opts)?;
    Ok(command_row_from_dimensions(lines, cmdheight) as f64)
}

pub(crate) fn smear_outside_cmd_row(corners: &[Point; 4]) -> Result<bool> {
    let cmd_row = command_row()?;
    Ok(corners.iter().any(|point| point.row < cmd_row))
}

#[cfg(test)]
mod tests {
    use super::BufferCursorRead;
    use super::command_row_from_dimensions;
    use super::parse_screenpos_cell;
    use super::screen_cell_to_point;
    use super::should_use_real_cmdline_cursor;
    use crate::core::effect::CursorColorFallbackMode;
    use crate::core::effect::CursorColorReuseMode;
    use crate::core::effect::CursorPositionProbeMode;
    use crate::core::effect::ProbePolicy;
    use crate::core::effect::ProbeQuality;
    use crate::core::state::CursorPositionSync;
    use nvim_oxi::Dictionary;
    use nvim_oxi::Object;
    use pretty_assertions::assert_eq;

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
    fn buffer_cursor_read_prefers_conceal_adjusted_position() {
        let read = BufferCursorRead {
            line: 2,
            column: 37,
            screenpos_summary: "row=2 col=38 endcol=38 curscol=38".to_string(),
            raw_position: Some((2.0, 38.0)),
            resolved_position: Some((2.0, 33.0)),
            raw_position_sync: CursorPositionSync::Exact,
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
            raw_position_sync: CursorPositionSync::Exact,
        };

        assert_eq!(read.selected_position(), Some((2.0, 38.0)));
        assert_eq!(read.conceal_adjusted_position(), None);
        assert_eq!(read.selected_source(), "screenpos");
        assert_eq!(
            read.position_sync_for_probe_policy(ProbePolicy::new(ProbeQuality::FastMotion)),
            CursorPositionSync::Exact,
        );
    }

    #[test]
    fn buffer_cursor_read_keeps_raw_screenpos_for_fast_motion_probe_quality() {
        let read = BufferCursorRead {
            line: 2,
            column: 37,
            screenpos_summary: "row=2 col=38 endcol=38 curscol=38".to_string(),
            raw_position: Some((2.0, 38.0)),
            resolved_position: Some((2.0, 33.0)),
            raw_position_sync: CursorPositionSync::ConcealDeferred,
        };

        assert_eq!(
            read.selected_position_for_probe_policy(ProbePolicy::new(ProbeQuality::FastMotion)),
            Some((2.0, 38.0)),
        );
        assert_eq!(
            read.selected_source_for_probe_policy(ProbePolicy::new(ProbeQuality::FastMotion)),
            "screenpos_fast_path",
        );
        assert_eq!(
            read.position_sync_for_probe_policy(ProbePolicy::new(ProbeQuality::FastMotion)),
            CursorPositionSync::ConcealDeferred,
        );
        assert!(read.uses_raw_screenpos_fallback(ProbePolicy::new(ProbeQuality::FastMotion)));
        assert!(!read.uses_raw_screenpos_fallback(ProbePolicy::new(ProbeQuality::Exact)));
    }

    #[test]
    fn buffer_cursor_read_keeps_conceal_adjustment_when_only_cursor_color_reuse_is_compatible() {
        let read = BufferCursorRead {
            line: 2,
            column: 37,
            screenpos_summary: "row=2 col=38 endcol=38 curscol=38".to_string(),
            raw_position: Some((2.0, 38.0)),
            resolved_position: Some((2.0, 33.0)),
            raw_position_sync: CursorPositionSync::ConcealDeferred,
        };
        let exact_compatible_policy = ProbePolicy::from_modes(
            CursorPositionProbeMode::Exact,
            CursorColorReuseMode::CompatibleWithinLine,
            CursorColorFallbackMode::SyntaxThenExtmarks,
        );

        assert_eq!(
            read.selected_position_for_probe_policy(exact_compatible_policy),
            Some((2.0, 33.0)),
        );
        assert_eq!(
            read.selected_source_for_probe_policy(exact_compatible_policy),
            "screenpos_conceal_adjusted",
        );
        assert_eq!(
            read.position_sync_for_probe_policy(exact_compatible_policy),
            CursorPositionSync::Exact,
        );
    }

    #[test]
    fn buffer_cursor_read_can_defer_fast_motion_without_resolved_conceal_adjustment() {
        let read = BufferCursorRead {
            line: 2,
            column: 37,
            screenpos_summary: "row=2 col=38 endcol=38 curscol=38".to_string(),
            raw_position: Some((2.0, 38.0)),
            resolved_position: Some((2.0, 38.0)),
            raw_position_sync: CursorPositionSync::ConcealDeferred,
        };

        assert_eq!(
            read.selected_position_for_probe_policy(ProbePolicy::new(ProbeQuality::FastMotion)),
            Some((2.0, 38.0)),
        );
        assert_eq!(
            read.position_sync_for_probe_policy(ProbePolicy::new(ProbeQuality::FastMotion)),
            CursorPositionSync::ConcealDeferred,
        );
        assert_eq!(read.conceal_adjusted_position(), None);
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
