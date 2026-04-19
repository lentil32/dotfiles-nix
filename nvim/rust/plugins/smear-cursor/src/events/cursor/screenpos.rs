use super::CursorParseError;
use super::CursorResult;
use super::conceal::observed_cell_for_raw_screenpos;
use super::conceal::resolve_buffer_cursor_position;
use super::cursor_parse_error;
use crate::core::effect::ProbePolicy;
use crate::events::logging::trace_lazy;
use crate::events::runtime::record_conceal_raw_screenpos_fallback;
use crate::lua::i64_from_object_ref_with_typed;
use crate::lua::i64_from_object_typed;
use crate::position::BufferLine;
use crate::position::CursorObservation;
use crate::position::ObservedCell;
use crate::position::RenderPoint;
use crate::position::ScreenCell;
use crate::position::WindowSurfaceSnapshot;
use nvim_oxi::Array;
use nvim_oxi::Dictionary;
use nvim_oxi::Object;
use nvim_oxi::Result;
use nvim_oxi::String as NvimString;
use nvim_oxi::api;
use nvim_oxi::api::opts::OptionOpts;
use nvim_oxi::api::types::ModeStr;
use nvim_oxi::conversion::FromObject;
use nvimrs_nvim_utils::mode::is_cmdline_mode;

pub(crate) fn current_mode() -> ModeStr {
    api::get_mode().mode
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
        (Some(row), Some(col)) => ScreenCell::new(row, col)
            .map(Some)
            .ok_or(CursorParseError::ScreenposInvalidCell { row, col }.into()),
        _ => Err(CursorParseError::ScreenposMissingRowCol.into()),
    }
}

pub(super) fn parse_screenpos_cell(screenpos: Object) -> CursorResult<Option<ScreenCell>> {
    parse_screenpos_cell_from_dict(&screenpos_dictionary(screenpos)?)
}

pub(super) fn buffer_column_to_col1(column: usize) -> i64 {
    i64::try_from(column.saturating_add(1)).unwrap_or(i64::MAX)
}

#[derive(Debug, Clone)]
pub(super) struct BufferCursorRead {
    pub(super) line: usize,
    pub(super) column: usize,
    pub(super) screenpos_summary: String,
    pub(super) raw_observed_cell: Option<ObservedCell>,
    pub(super) resolved_cell: Option<ScreenCell>,
}

impl BufferCursorRead {
    pub(super) fn conceal_adjusted_cell(&self) -> Option<ScreenCell> {
        match (
            self.raw_observed_cell.and_then(ObservedCell::screen_cell),
            self.resolved_cell,
        ) {
            (Some(raw_cell), Some(resolved_cell)) if raw_cell != resolved_cell => {
                Some(resolved_cell)
            }
            _ => None,
        }
    }

    pub(super) fn selected_observed_cell(&self) -> ObservedCell {
        self.resolved_cell
            .map(ObservedCell::Exact)
            .unwrap_or(ObservedCell::Unavailable)
    }

    pub(super) fn selected_observed_cell_for_probe_policy(
        &self,
        probe_policy: ProbePolicy,
    ) -> ObservedCell {
        if probe_policy.uses_raw_screenpos_fallback() {
            self.raw_observed_cell.unwrap_or(ObservedCell::Unavailable)
        } else {
            self.selected_observed_cell()
        }
    }

    pub(super) fn selected_source(&self) -> &'static str {
        match (
            self.raw_observed_cell.and_then(ObservedCell::screen_cell),
            self.resolved_cell,
        ) {
            (Some(raw_cell), Some(resolved_cell)) if raw_cell != resolved_cell => {
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

        if self.raw_observed_cell.is_some() {
            "screenpos_fast_path"
        } else {
            "none"
        }
    }

    pub(super) fn uses_raw_screenpos_fallback(&self, probe_policy: ProbePolicy) -> bool {
        probe_policy.uses_raw_screenpos_fallback() && self.raw_observed_cell.is_some()
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
        let raw_summary = buffer_read
            .raw_observed_cell
            .and_then(ObservedCell::screen_cell)
            .map_or_else(
                || "none".to_string(),
                |cell| format!("{}:{}", cell.row(), cell.col()),
            );
        let conceal_adjusted_summary = buffer_read.conceal_adjusted_cell().map_or_else(
            || "none".to_string(),
            |cell| format!("{}:{}", cell.row(), cell.col()),
        );
        let selected = buffer_read.selected_observed_cell_for_probe_policy(probe_policy);
        let selected_source = buffer_read.selected_source_for_probe_policy(probe_policy);
        let selected_summary = selected.screen_cell().map_or_else(
            || "none".to_string(),
            |cell| format!("{}:{}", cell.row(), cell.col()),
        );

        format!(
            "cursor_read win={} cursor=line:{} byte_col0={} screenpos_arg_col={} screenpos=({}) buffer_parsed={} conceal_adjusted={} selected={} selected_source={} probe_policy={}",
            window.handle(),
            buffer_read.line,
            buffer_read.column,
            buffer_read.column.saturating_add(1),
            buffer_read.screenpos_summary,
            raw_summary,
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
    surface_snapshot: Option<&WindowSurfaceSnapshot>,
) -> CursorResult<BufferCursorRead> {
    let (line, column) = window.get_cursor()?;
    let screenpos = screenpos_for_buffer_column(window, line, buffer_column_to_col1(column))?;
    let screenpos = screenpos_dictionary(screenpos)?;
    // `screenpos.curscol` points at the cursor landing column, which is the end of a Tab
    // expansion. The smear target needs the first screen cell that renders the buffer character
    // under the cursor, which is `screenpos.col`.
    let raw_cell = parse_screenpos_cell_from_dict(&screenpos)?;
    let conceal_surface = conceal_surface_snapshot(raw_cell, surface_snapshot);
    let raw_observed_cell = match raw_cell {
        Some(raw_cell) if probe_policy.uses_raw_screenpos_fallback() => Some(
            observed_cell_for_raw_screenpos(window, line, column, mode, raw_cell, conceal_surface)?,
        ),
        Some(raw_cell) => Some(ObservedCell::Exact(raw_cell)),
        None => None,
    };
    let resolved_cell = match raw_cell {
        Some(_raw_cell) if probe_policy.uses_raw_screenpos_fallback() => {
            raw_observed_cell.and_then(ObservedCell::screen_cell)
        }
        Some(raw_cell) => Some(resolve_buffer_cursor_position(
            window,
            line,
            column,
            mode,
            raw_cell,
            conceal_surface,
        )?),
        None => None,
    };
    Ok(BufferCursorRead {
        line,
        column,
        screenpos_summary: screenpos_summary(&screenpos),
        raw_observed_cell,
        resolved_cell,
    })
}

fn conceal_surface_snapshot(
    raw_cell: Option<ScreenCell>,
    surface_snapshot: Option<&WindowSurfaceSnapshot>,
) -> Option<WindowSurfaceSnapshot> {
    raw_cell
        .zip(surface_snapshot)
        .map(|(_raw_cell, surface_snapshot)| *surface_snapshot)
}

fn buffer_line_for_window_cursor(window: &api::Window) -> CursorResult<BufferLine> {
    let (line, _) = window.get_cursor()?;
    BufferLine::new(i64::try_from(line).unwrap_or(i64::MAX))
        .ok_or(CursorParseError::InvalidBufferLine { line }.into())
}

fn screen_cursor_observation(
    window: &api::Window,
    mode: &str,
    probe_policy: ProbePolicy,
    surface_snapshot: Option<&WindowSurfaceSnapshot>,
) -> CursorResult<CursorObservation> {
    let buffer_read = buffer_screen_cursor_position(window, mode, probe_policy, surface_snapshot)?;
    // `screenpos()` is the stable callback-safe base here. The `gg` trace showed
    // `screenrow()`/`screencol()` reporting stale or command-line cells on scheduled edges, so we
    // keep the timing-sensitive live probe out of production selection. Fast motion can stay on
    // the raw `screenpos()` sample, while exact edges pay for conceal correction to re-sync.
    trace_screen_cursor_read(window, &buffer_read, probe_policy);
    if buffer_read.uses_raw_screenpos_fallback(probe_policy) {
        record_conceal_raw_screenpos_fallback(i64::from(window.get_buf()?.handle()));
    }
    let buffer_line = BufferLine::new(i64::try_from(buffer_read.line).unwrap_or(i64::MAX)).ok_or(
        CursorParseError::InvalidBufferLine {
            line: buffer_read.line,
        },
    )?;
    let observed_cell = buffer_read.selected_observed_cell_for_probe_policy(probe_policy);
    Ok(CursorObservation::new(buffer_line, observed_cell))
}

fn command_type_string() -> CursorResult<String> {
    let value = api::call_function("getcmdtype", Array::new())?;
    crate::lua::string_from_object_typed("getcmdtype", value)
        .map_err(|source| cursor_parse_error("getcmdtype", source))
}

pub(super) fn should_use_real_cmdline_cursor(cmdtype: &str) -> bool {
    !cmdtype.is_empty()
}

fn cmdline_cursor_observation(
    window: &api::Window,
    probe_policy: ProbePolicy,
    surface_snapshot: Option<&WindowSurfaceSnapshot>,
) -> CursorResult<CursorObservation> {
    let buffer_line = buffer_line_for_window_cursor(window)?;
    let cmdtype = command_type_string()?;
    if !should_use_real_cmdline_cursor(&cmdtype) {
        // showcmd and normal-mode prefix keys can transiently report `mode=c` while the rendered
        // cursor is still in the buffer. Falling back to the buffer cursor avoids animating
        // bottom-row showcmd columns for motions like `gg`, while preserving any deferred
        // conceal correction that still needs an exact settle-time pass.
        return screen_cursor_observation(window, "n", probe_policy, surface_snapshot);
    }

    let screen_col_value = api::call_function("getcmdscreenpos", Array::new())?;
    let screen_col = i64_from_object_typed("getcmdscreenpos", screen_col_value)
        .map_err(|source| cursor_parse_error("getcmdscreenpos", source))?;
    if screen_col <= 0 {
        return Ok(CursorObservation::new(
            buffer_line,
            ObservedCell::Unavailable,
        ));
    }

    Ok(CursorObservation::new(
        buffer_line,
        ScreenCell::new(command_row()?, screen_col)
            .map(ObservedCell::Exact)
            .unwrap_or(ObservedCell::Unavailable),
    ))
}

pub(in crate::events) fn cursor_observation_for_mode_with_probe_policy_typed(
    window: &api::Window,
    mode: &str,
    smear_to_cmd: bool,
    probe_policy: ProbePolicy,
    surface_snapshot: Option<&WindowSurfaceSnapshot>,
) -> CursorResult<CursorObservation> {
    if is_cmdline_mode(mode) {
        if !smear_to_cmd {
            return buffer_line_for_window_cursor(window)
                .map(|buffer_line| CursorObservation::new(buffer_line, ObservedCell::Unavailable));
        }
        return cmdline_cursor_observation(window, probe_policy, surface_snapshot);
    }
    screen_cursor_observation(window, mode, probe_policy, surface_snapshot)
}

pub(crate) fn cursor_observation_for_mode_with_probe_policy(
    window: &api::Window,
    mode: &str,
    smear_to_cmd: bool,
    probe_policy: ProbePolicy,
    surface_snapshot: Option<&WindowSurfaceSnapshot>,
) -> Result<CursorObservation> {
    cursor_observation_for_mode_with_probe_policy_typed(
        window,
        mode,
        smear_to_cmd,
        probe_policy,
        surface_snapshot,
    )
    .map_err(nvim_oxi::Error::from)
}

fn command_row() -> Result<i64> {
    let viewport = crate::events::runtime::editor_viewport_for_command_row()?;
    Ok(viewport.command_row())
}

pub(crate) fn smear_outside_cmd_row(corners: &[RenderPoint; 4]) -> Result<bool> {
    let cmd_row = command_row()? as f64;
    Ok(corners.iter().any(|point| point.row < cmd_row))
}

#[cfg(test)]
mod tests {
    use super::BufferCursorRead;
    use super::conceal_surface_snapshot;
    use super::parse_screenpos_cell;
    use super::should_use_real_cmdline_cursor;
    use crate::core::effect::ProbePolicy;
    use crate::core::effect::ProbeQuality;
    use crate::position::BufferLine;
    use crate::position::ObservedCell;
    use crate::position::RenderPoint;
    use crate::position::ScreenCell;
    use crate::position::SurfaceId;
    use crate::position::ViewportBounds;
    use crate::position::WindowSurfaceSnapshot;
    use crate::test_support::proptest::pure_config;
    use nvim_oxi::Dictionary;
    use nvim_oxi::Object;
    use pretty_assertions::assert_eq;
    use proptest::collection::vec;
    use proptest::prelude::*;

    fn screenpos_object(
        row: Option<i64>,
        col: Option<i64>,
        endcol: Option<i64>,
        curscol: Option<i64>,
    ) -> Object {
        let mut dict = Dictionary::new();
        if let Some(row) = row {
            dict.insert("row", Object::from(row));
        }
        if let Some(col) = col {
            dict.insert("col", Object::from(col));
        }
        if let Some(endcol) = endcol {
            dict.insert("endcol", Object::from(endcol));
        }
        if let Some(curscol) = curscol {
            dict.insert("curscol", Object::from(curscol));
        }
        Object::from(dict)
    }

    fn cmdtype_strategy() -> BoxedStrategy<String> {
        vec(
            prop_oneof![Just(':'), Just('/'), Just('a'), Just('0'), Just(' '),],
            0..=4,
        )
        .prop_map(|chars| chars.into_iter().collect())
        .boxed()
    }

    #[derive(Clone, Copy, Debug)]
    enum BufferReadCase {
        Hidden,
        Unadjusted,
        ConcealAdjusted,
    }

    #[derive(Clone, Copy, Debug)]
    enum RawObservedCellKind {
        Exact,
        Deferred,
    }

    fn buffer_read_case_strategy() -> BoxedStrategy<BufferReadCase> {
        prop_oneof![
            Just(BufferReadCase::Hidden),
            Just(BufferReadCase::Unadjusted),
            Just(BufferReadCase::ConcealAdjusted),
        ]
        .boxed()
    }

    fn raw_observed_cell_kind_strategy() -> BoxedStrategy<RawObservedCellKind> {
        prop_oneof![
            Just(RawObservedCellKind::Exact),
            Just(RawObservedCellKind::Deferred),
        ]
        .boxed()
    }

    fn screen_cell(row: i64, col: i64) -> ScreenCell {
        ScreenCell::new(row, col).expect("one-based screen cell")
    }

    fn surface_snapshot() -> WindowSurfaceSnapshot {
        WindowSurfaceSnapshot::new(
            SurfaceId::new(11, 17).expect("positive surface handles"),
            BufferLine::new(23).expect("positive buffer line"),
            5,
            2,
            ScreenCell::new(7, 13).expect("one-based origin"),
            ViewportBounds::new(24, 80).expect("positive viewport"),
        )
    }

    proptest! {
        #![proptest_config(pure_config())]

        #[test]
        fn prop_parse_screenpos_cell_rejects_invalid_present_coordinates_and_missing_pairs(
            row in proptest::option::of(-4_i64..=8),
            col in proptest::option::of(-4_i64..=8),
        ) {
            let result = parse_screenpos_cell(screenpos_object(row, col, None, None));

            match (row, col) {
                (None, None) => prop_assert_eq!(result?, None),
                (Some(row), Some(col)) if row > 0 && col > 0 => {
                    prop_assert_eq!(result?, Some(ScreenCell::new(row, col).expect("one-based cell")));
                }
                (Some(_), Some(_)) => prop_assert!(result.is_err()),
                (Some(_), None) | (None, Some(_)) => prop_assert!(result.is_err()),
            }
        }

        #[test]
        fn prop_buffer_cursor_read_exact_policy_prefers_resolved_positions_and_sources(
            case in buffer_read_case_strategy(),
            raw_row in 1_i64..256,
            raw_col in 1_i64..256,
            resolved_row in 1_i64..256,
            resolved_col in 1_i64..256,
            raw_observed_cell_kind in raw_observed_cell_kind_strategy(),
        ) {
            let raw_cell = match case {
                BufferReadCase::Hidden => None,
                BufferReadCase::Unadjusted | BufferReadCase::ConcealAdjusted => Some(screen_cell(raw_row, raw_col)),
            };
            let resolved_cell = match case {
                BufferReadCase::Hidden => None,
                BufferReadCase::Unadjusted => raw_cell,
                BufferReadCase::ConcealAdjusted => Some(screen_cell(
                    resolved_row,
                    if raw_row == resolved_row && raw_col == resolved_col {
                        resolved_col.saturating_add(1)
                    } else {
                        resolved_col
                    },
                )),
            };
            let expected_adjusted = match case {
                BufferReadCase::ConcealAdjusted => Some(
                    resolved_cell.expect("conceal-adjusted reads keep a resolved position"),
                ),
                BufferReadCase::Hidden | BufferReadCase::Unadjusted => None,
            };
            let expected_source = match case {
                BufferReadCase::Hidden => "none",
                BufferReadCase::Unadjusted => "screenpos",
                BufferReadCase::ConcealAdjusted => "screenpos_conceal_adjusted",
            };
            let raw_observed_cell = raw_cell.map(|cell| match raw_observed_cell_kind {
                RawObservedCellKind::Exact => ObservedCell::Exact(cell),
                RawObservedCellKind::Deferred => ObservedCell::Deferred(cell),
            });
            let read = BufferCursorRead {
                line: 2,
                column: 37,
                screenpos_summary: "row=2 col=38 endcol=38 curscol=38".to_string(),
                raw_observed_cell,
                resolved_cell,
            };
            let exact_policy = ProbePolicy::new(ProbeQuality::Exact);
            let expected_observed_cell =
                resolved_cell.map_or(ObservedCell::Unavailable, ObservedCell::Exact);

            prop_assert_eq!(
                read.selected_observed_cell_for_probe_policy(exact_policy),
                expected_observed_cell,
            );
            prop_assert_eq!(
                read.selected_source_for_probe_policy(exact_policy),
                expected_source,
            );
            prop_assert!(!read.uses_raw_screenpos_fallback(exact_policy));

            prop_assert_eq!(read.selected_observed_cell(), expected_observed_cell);
            prop_assert_eq!(
                read.selected_observed_cell_for_probe_policy(exact_policy)
                    .screen_cell()
                    .map(RenderPoint::from),
                resolved_cell.map(RenderPoint::from),
            );
            prop_assert_eq!(read.conceal_adjusted_cell(), expected_adjusted);
            prop_assert_eq!(read.selected_source(), expected_source);
        }

        #[test]
        fn prop_buffer_cursor_read_fast_motion_keeps_raw_screenpos_only_when_available(
            case in buffer_read_case_strategy(),
            raw_row in 1_i64..256,
            raw_col in 1_i64..256,
            resolved_row in 1_i64..256,
            resolved_col in 1_i64..256,
            raw_observed_cell_kind in raw_observed_cell_kind_strategy(),
        ) {
            let raw_cell = match case {
                BufferReadCase::Hidden => None,
                BufferReadCase::Unadjusted | BufferReadCase::ConcealAdjusted => Some(screen_cell(raw_row, raw_col)),
            };
            let resolved_cell = match case {
                BufferReadCase::Hidden => None,
                BufferReadCase::Unadjusted => raw_cell,
                BufferReadCase::ConcealAdjusted => Some(screen_cell(
                    resolved_row,
                    if raw_row == resolved_row && raw_col == resolved_col {
                        resolved_col.saturating_add(1)
                    } else {
                        resolved_col
                    },
                )),
            };
            let raw_observed_cell = raw_cell.map(|cell| match raw_observed_cell_kind {
                RawObservedCellKind::Exact => ObservedCell::Exact(cell),
                RawObservedCellKind::Deferred => ObservedCell::Deferred(cell),
            });
            let read = BufferCursorRead {
                line: 2,
                column: 37,
                screenpos_summary: "row=2 col=38 endcol=38 curscol=38".to_string(),
                raw_observed_cell,
                resolved_cell,
            };
            let fast_motion_policy = ProbePolicy::new(ProbeQuality::FastMotion);
            let expected_observed_cell = raw_observed_cell.unwrap_or(ObservedCell::Unavailable);

            prop_assert_eq!(
                read.selected_observed_cell_for_probe_policy(fast_motion_policy),
                expected_observed_cell,
            );
            prop_assert_eq!(
                read.selected_source_for_probe_policy(fast_motion_policy),
                if raw_observed_cell.is_some() {
                    "screenpos_fast_path"
                } else {
                    "none"
                },
            );
            prop_assert_eq!(
                read.selected_observed_cell_for_probe_policy(fast_motion_policy)
                    .screen_cell()
                    .map(RenderPoint::from),
                expected_observed_cell.screen_cell().map(RenderPoint::from),
            );
            prop_assert_eq!(
                read.uses_raw_screenpos_fallback(fast_motion_policy),
                raw_observed_cell.is_some(),
            );
        }

        #[test]
        fn prop_should_use_real_cmdline_cursor_depends_only_on_cmdtype_emptiness(
            cmdtype in cmdtype_strategy(),
        ) {
            prop_assert_eq!(should_use_real_cmdline_cursor(&cmdtype), !cmdtype.is_empty());
        }
    }

    #[test]
    fn parse_screenpos_cell_rejects_zero_row_instead_of_treating_it_as_hidden() {
        let result = parse_screenpos_cell(screenpos_object(Some(0), Some(3), None, None));

        assert!(result.is_err());
    }

    #[test]
    fn conceal_surface_snapshot_requires_both_a_raw_cell_and_surface_snapshot() {
        let surface = surface_snapshot();
        let expected = Some(surface);

        assert_eq!(
            conceal_surface_snapshot(Some(screen_cell(4, 9)), Some(&surface)),
            expected,
        );
        assert_eq!(conceal_surface_snapshot(None, Some(&surface)), None);
        assert_eq!(
            conceal_surface_snapshot(Some(screen_cell(4, 9)), None),
            None,
        );
    }

    #[test]
    fn fast_motion_policy_preserves_a_deferred_raw_cell_across_conceal_adjustment() {
        let raw_cell = screen_cell(7, 18);
        let resolved_cell = screen_cell(7, 15);
        let read = BufferCursorRead {
            line: 7,
            column: 17,
            screenpos_summary: "row=7 col=18 endcol=18 curscol=18".to_string(),
            raw_observed_cell: Some(ObservedCell::Deferred(raw_cell)),
            resolved_cell: Some(resolved_cell),
        };
        let exact_policy = ProbePolicy::new(ProbeQuality::Exact);
        let fast_motion_policy = ProbePolicy::new(ProbeQuality::FastMotion);

        assert_eq!(
            read.selected_observed_cell_for_probe_policy(exact_policy),
            ObservedCell::Exact(resolved_cell),
        );
        assert_eq!(
            read.selected_source_for_probe_policy(exact_policy),
            "screenpos_conceal_adjusted",
        );
        assert_eq!(
            read.selected_observed_cell_for_probe_policy(fast_motion_policy),
            ObservedCell::Deferred(raw_cell),
        );
        assert_eq!(
            read.selected_source_for_probe_policy(fast_motion_policy),
            "screenpos_fast_path",
        );
    }
}
