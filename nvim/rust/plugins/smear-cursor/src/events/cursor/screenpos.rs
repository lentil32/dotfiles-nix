use super::CursorParseError;
use super::CursorResult;
use super::conceal::ExactCursorProjection;
use super::conceal::ExactProjectionSource;
use super::conceal::RawScreenposProjection;
use super::conceal::observed_cell_for_raw_screenpos;
use super::conceal::resolve_buffer_cursor_position;
use super::cursor_parse_error;
use crate::core::effect::ProbePolicy;
use crate::events::logging::trace_lazy;
use crate::events::runtime::record_conceal_deferred_projection;
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
    observed_cell: ObservedCell,
    diagnostics: BufferCursorReadDiagnostics,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum ProjectionSource {
    Screenpos,
    ConcealExact,
    ConcealCached,
}

impl ProjectionSource {
    const fn diagnostic_name(self) -> &'static str {
        match self {
            Self::Screenpos => "screenpos_projection",
            Self::ConcealExact => "conceal_exact_projection",
            Self::ConcealCached => "conceal_cached_projection",
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
struct BufferCursorReadDiagnostics {
    raw_screenpos_cell: Option<ScreenCell>,
    projection_source: Option<ProjectionSource>,
}

impl BufferCursorReadDiagnostics {
    const fn raw_screenpos_cell(self) -> Option<ScreenCell> {
        self.raw_screenpos_cell
    }

    fn projected_adjustment_cell(self, observed_cell: ObservedCell) -> Option<ScreenCell> {
        match (self.raw_screenpos_cell, observed_cell.screen_cell()) {
            (Some(raw_cell), Some(projected_cell)) if raw_cell != projected_cell => {
                Some(projected_cell)
            }
            _ => None,
        }
    }

    const fn projection_source(self) -> &'static str {
        match self.projection_source {
            Some(source) => source.diagnostic_name(),
            None => "none",
        }
    }
}

impl BufferCursorRead {
    pub(super) const fn observed_cell(&self) -> ObservedCell {
        self.observed_cell
    }

    pub(super) fn projected_adjustment_cell(&self) -> Option<ScreenCell> {
        self.diagnostics
            .projected_adjustment_cell(self.observed_cell)
    }

    pub(super) const fn projection_source(&self) -> &'static str {
        self.diagnostics.projection_source()
    }

    pub(super) const fn raw_screenpos_cell(&self) -> Option<ScreenCell> {
        self.diagnostics.raw_screenpos_cell()
    }

    pub(super) const fn uses_deferred_projection(&self) -> bool {
        self.observed_cell.requires_exact_refresh()
    }
}

const fn projection_source_for_exact_projection(
    projection: ExactCursorProjection,
) -> ProjectionSource {
    match projection.source {
        ExactProjectionSource::Screenpos => ProjectionSource::Screenpos,
        ExactProjectionSource::Conceal => ProjectionSource::ConcealExact,
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
        let raw_summary = buffer_read.raw_screenpos_cell().map_or_else(
            || "none".to_string(),
            |cell| format!("{}:{}", cell.row(), cell.col()),
        );
        let projected_adjustment_summary = buffer_read.projected_adjustment_cell().map_or_else(
            || "none".to_string(),
            |cell| format!("{}:{}", cell.row(), cell.col()),
        );
        let projected = buffer_read.observed_cell();
        let projected_summary = projected.screen_cell().map_or_else(
            || "none".to_string(),
            |cell| format!("{}:{}", cell.row(), cell.col()),
        );

        format!(
            "cursor_read win={} cursor=line:{} byte_col0={} screenpos_arg_col={} screenpos=({}) raw_probe={} projected={} projected_adjustment={} projection_source={} projection_freshness={} probe_policy={}",
            window.handle(),
            buffer_read.line,
            buffer_read.column,
            buffer_read.column.saturating_add(1),
            buffer_read.screenpos_summary,
            raw_summary,
            projected_summary,
            projected_adjustment_summary,
            buffer_read.projection_source(),
            observed_cell_freshness_name(projected),
            probe_policy.diagnostic_name(),
        )
    });
}

const fn observed_cell_freshness_name(observed_cell: ObservedCell) -> &'static str {
    match observed_cell {
        ObservedCell::Unavailable => "unavailable",
        ObservedCell::Exact(_) => "exact",
        ObservedCell::Deferred(_) => "deferred",
    }
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
    let (observed_cell, projection_source) = match raw_cell {
        Some(raw_cell) if probe_policy.allows_deferred_cursor_projection() => {
            match observed_cell_for_raw_screenpos(
                window,
                line,
                column,
                mode,
                raw_cell,
                conceal_surface,
            )? {
                RawScreenposProjection::Projected {
                    observed_cell,
                    used_cached_conceal,
                } => (
                    observed_cell,
                    Some(if used_cached_conceal {
                        ProjectionSource::ConcealCached
                    } else {
                        ProjectionSource::Screenpos
                    }),
                ),
                RawScreenposProjection::NeedsExactProjection => {
                    let exact_projection = resolve_buffer_cursor_position(
                        window,
                        line,
                        column,
                        mode,
                        raw_cell,
                        conceal_surface,
                    )?;
                    (
                        exact_projection.observed_cell,
                        Some(projection_source_for_exact_projection(exact_projection)),
                    )
                }
            }
        }
        Some(raw_cell) => {
            let exact_projection = resolve_buffer_cursor_position(
                window,
                line,
                column,
                mode,
                raw_cell,
                conceal_surface,
            )?;
            (
                exact_projection.observed_cell,
                Some(projection_source_for_exact_projection(exact_projection)),
            )
        }
        None => (ObservedCell::Unavailable, None),
    };
    Ok(BufferCursorRead {
        line,
        column,
        screenpos_summary: screenpos_summary(&screenpos),
        observed_cell,
        diagnostics: BufferCursorReadDiagnostics {
            raw_screenpos_cell: raw_cell,
            projection_source,
        },
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
    // keep the timing-sensitive live probe out of production selection. The
    // event-layer reader owns raw host details such as conceal correction and
    // cache reuse, then returns a `CursorObservation` through the shared
    // display-space contract. Fast motion now differs only in whether cached or
    // deferred projection is allowed before falling back to the exact
    // projector; reducer-owned cursor truth is always constructed from the
    // projected observation returned by the reader.
    trace_screen_cursor_read(window, &buffer_read, probe_policy);
    if buffer_read.uses_deferred_projection() {
        record_conceal_deferred_projection(i64::from(window.get_buf()?.handle()));
    }
    let buffer_line = BufferLine::new(i64::try_from(buffer_read.line).unwrap_or(i64::MAX)).ok_or(
        CursorParseError::InvalidBufferLine {
            line: buffer_read.line,
        },
    )?;
    Ok(CursorObservation::new(
        buffer_line,
        buffer_read.observed_cell(),
    ))
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
    use super::BufferCursorReadDiagnostics;
    use super::ProjectionSource;
    use super::conceal_surface_snapshot;
    use super::parse_screenpos_cell;
    use super::should_use_real_cmdline_cursor;
    use crate::position::BufferLine;
    use crate::position::ObservedCell;
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
    fn fast_motion_reads_keep_the_projected_deferred_cell_across_conceal_adjustment() {
        let raw_cell = screen_cell(7, 18);
        let projected_cell = screen_cell(7, 15);
        let read = BufferCursorRead {
            line: 7,
            column: 17,
            screenpos_summary: "row=7 col=18 endcol=18 curscol=18".to_string(),
            observed_cell: ObservedCell::Deferred(projected_cell),
            diagnostics: BufferCursorReadDiagnostics {
                raw_screenpos_cell: Some(raw_cell),
                projection_source: Some(ProjectionSource::ConcealCached),
            },
        };

        assert_eq!(read.observed_cell(), ObservedCell::Deferred(projected_cell),);
        assert_eq!(read.projected_adjustment_cell(), Some(projected_cell));
        assert_eq!(read.projection_source(), "conceal_cached_projection");
        assert!(read.uses_deferred_projection());
    }

    #[test]
    fn fast_motion_reads_keep_same_cell_deferred_when_cached_conceal_confirms_no_drift() {
        let raw_cell = screen_cell(9, 24);
        let read = BufferCursorRead {
            line: 9,
            column: 23,
            screenpos_summary: "row=9 col=24 endcol=24 curscol=24".to_string(),
            observed_cell: ObservedCell::Deferred(raw_cell),
            diagnostics: BufferCursorReadDiagnostics {
                raw_screenpos_cell: Some(raw_cell),
                projection_source: Some(ProjectionSource::ConcealCached),
            },
        };

        assert_eq!(read.observed_cell(), ObservedCell::Deferred(raw_cell));
        assert_eq!(read.projected_adjustment_cell(), None);
        assert_eq!(read.projection_source(), "conceal_cached_projection");
        assert!(read.uses_deferred_projection());
    }

    #[test]
    fn exact_conceal_unavailable_projection_still_reports_the_exact_projection_source() {
        let raw_cell = screen_cell(11, 31);
        let read = BufferCursorRead {
            line: 11,
            column: 30,
            screenpos_summary: "row=11 col=31 endcol=31 curscol=31".to_string(),
            observed_cell: ObservedCell::Unavailable,
            diagnostics: BufferCursorReadDiagnostics {
                raw_screenpos_cell: Some(raw_cell),
                projection_source: Some(ProjectionSource::ConcealExact),
            },
        };

        assert_eq!(read.observed_cell(), ObservedCell::Unavailable);
        assert_eq!(read.projected_adjustment_cell(), None);
        assert_eq!(read.projection_source(), "conceal_exact_projection");
        assert_eq!(read.raw_screenpos_cell(), Some(raw_cell));
        assert!(!read.uses_deferred_projection());
    }
}
