//! Canonical window-surface host reads for observation-time position facts.

use crate::lua::LuaParseError;
use crate::lua::i64_from_object_ref_with_typed;
use crate::position::BufferLine;
use crate::position::ScreenCell;
use crate::position::SurfaceId;
use crate::position::ViewportBounds;
use crate::position::WindowSurfaceSnapshot;
use nvim_oxi::Array;
use nvim_oxi::Dictionary;
use nvim_oxi::Object;
use nvim_oxi::String as NvimString;
use nvim_oxi::api;
use nvim_oxi::conversion::FromObject;

type WindowSurfaceReadResult<T> = std::result::Result<T, WindowSurfaceReadError>;

#[derive(Debug, thiserror::Error)]
pub(crate) enum WindowSurfaceReadError {
    #[error(transparent)]
    Shell(#[from] nvim_oxi::Error),
    #[error("getwininfo returned invalid list")]
    GetwininfoInvalidList,
    #[error("getwininfo returned unexpected list length")]
    GetwininfoUnexpectedLen,
    #[error("getwininfo returned invalid dictionary")]
    GetwininfoDictionary,
    #[error("getwininfo missing {field}")]
    MissingField { field: &'static str },
    #[error("{context} parse failed: {source}")]
    Value {
        context: String,
        #[source]
        source: LuaParseError,
    },
    #[error(
        "getwininfo returned invalid positive handles winid={window_handle} bufnr={buffer_handle}"
    )]
    InvalidSurfaceId {
        window_handle: i64,
        buffer_handle: i64,
    },
    #[error("getwininfo returned invalid one-based topline={topline}")]
    InvalidTopline { topline: i64 },
    #[error("getwininfo returned invalid non-negative {field}={value}")]
    InvalidNonNegativeField { field: &'static str, value: i64 },
    #[error("getwininfo returned invalid one-based window origin row={row} col={col}")]
    InvalidWindowOrigin { row: i64, col: i64 },
    #[error("getwininfo returned invalid positive window size width={width} height={height}")]
    InvalidWindowSize { width: i64, height: i64 },
}

impl From<nvim_oxi::api::Error> for WindowSurfaceReadError {
    fn from(error: nvim_oxi::api::Error) -> Self {
        Self::Shell(error.into())
    }
}

fn dictionary_i64_field(
    dict: &Dictionary,
    field: &'static str,
) -> WindowSurfaceReadResult<Option<i64>> {
    let field_key = NvimString::from(field);
    let Some(value) = dict.get(&field_key) else {
        return Ok(None);
    };

    i64_from_object_ref_with_typed(value, || format!("getwininfo.{field}"))
        .map(Some)
        .map_err(|source| WindowSurfaceReadError::Value {
            context: format!("getwininfo.{field}"),
            source,
        })
}

fn required_dictionary_i64_field(
    dict: &Dictionary,
    field: &'static str,
) -> WindowSurfaceReadResult<i64> {
    dictionary_i64_field(dict, field)?.ok_or(WindowSurfaceReadError::MissingField { field })
}

fn non_negative_u32(value: i64, field: &'static str) -> WindowSurfaceReadResult<u32> {
    u32::try_from(value)
        .map_err(|_| WindowSurfaceReadError::InvalidNonNegativeField { field, value })
}

fn getwininfo_dict(window: &api::Window) -> WindowSurfaceReadResult<Dictionary> {
    let args = Array::from_iter([Object::from(window.handle())]);
    let [entry]: [Object; 1] = Vec::<Object>::from_object(api::call_function("getwininfo", args)?)
        .map_err(|_| WindowSurfaceReadError::GetwininfoInvalidList)?
        .try_into()
        .map_err(|_| WindowSurfaceReadError::GetwininfoUnexpectedLen)?;
    Dictionary::from_object(entry).map_err(|_| WindowSurfaceReadError::GetwininfoDictionary)
}

fn parse_window_surface_snapshot(
    window_handle: i64,
    buffer_handle: i64,
    dict: &Dictionary,
) -> WindowSurfaceReadResult<WindowSurfaceSnapshot> {
    let top_buffer_line = required_dictionary_i64_field(dict, "topline")?;
    let left_col0 = required_dictionary_i64_field(dict, "leftcol")?;
    let text_offset0 = required_dictionary_i64_field(dict, "textoff")?;
    let window_row = required_dictionary_i64_field(dict, "winrow")?;
    let window_col = required_dictionary_i64_field(dict, "wincol")?;
    let window_width = required_dictionary_i64_field(dict, "width")?;
    let window_height = required_dictionary_i64_field(dict, "height")?;

    let id = SurfaceId::new(window_handle, buffer_handle).ok_or(
        WindowSurfaceReadError::InvalidSurfaceId {
            window_handle,
            buffer_handle,
        },
    )?;
    let top_buffer_line =
        BufferLine::new(top_buffer_line).ok_or(WindowSurfaceReadError::InvalidTopline {
            topline: top_buffer_line,
        })?;
    let window_origin = ScreenCell::new(window_row, window_col).ok_or(
        WindowSurfaceReadError::InvalidWindowOrigin {
            row: window_row,
            col: window_col,
        },
    )?;
    let window_size = ViewportBounds::new(window_height, window_width).ok_or(
        WindowSurfaceReadError::InvalidWindowSize {
            width: window_width,
            height: window_height,
        },
    )?;

    Ok(WindowSurfaceSnapshot::new(
        id,
        top_buffer_line,
        non_negative_u32(left_col0, "leftcol")?,
        non_negative_u32(text_offset0, "textoff")?,
        window_origin,
        window_size,
    ))
}

pub(crate) fn current_window_surface_snapshot(
    window: &api::Window,
) -> WindowSurfaceReadResult<WindowSurfaceSnapshot> {
    let dict = getwininfo_dict(window)?;
    let window_handle = i64::from(window.handle());
    let buffer_handle = i64::from(window.get_buf()?.handle());
    parse_window_surface_snapshot(window_handle, buffer_handle, &dict)
}

#[cfg(test)]
mod tests {
    use super::WindowSurfaceReadError;
    use super::parse_window_surface_snapshot;
    use crate::position::BufferLine;
    use crate::position::ScreenCell;
    use crate::position::SurfaceId;
    use crate::position::ViewportBounds;
    use crate::position::WindowSurfaceSnapshot;
    use nvim_oxi::Dictionary;
    use nvim_oxi::Object;
    use pretty_assertions::assert_eq;

    fn getwininfo_dict(entries: impl IntoIterator<Item = (&'static str, i64)>) -> Dictionary {
        let mut dict = Dictionary::new();
        for (key, value) in entries {
            dict.insert(key, Object::from(value));
        }
        dict
    }

    fn canonical_getwininfo_dict() -> Dictionary {
        getwininfo_dict([
            ("topline", 23),
            ("leftcol", 5),
            ("textoff", 2),
            ("winrow", 7),
            ("wincol", 13),
            ("width", 80),
            ("height", 24),
        ])
    }

    #[test]
    fn parse_window_surface_snapshot_maps_getwininfo_into_canonical_surface_facts() {
        let snapshot = parse_window_surface_snapshot(11, 17, &canonical_getwininfo_dict())
            .expect("surface snapshot should parse");

        assert_eq!(
            snapshot,
            WindowSurfaceSnapshot::new(
                SurfaceId::new(11, 17).expect("positive handles"),
                BufferLine::new(23).expect("positive buffer line"),
                5,
                2,
                ScreenCell::new(7, 13).expect("one-based window origin"),
                ViewportBounds::new(24, 80).expect("positive window size"),
            )
        );
    }

    #[test]
    fn parse_window_surface_snapshot_rejects_negative_leftcol() {
        let dict = getwininfo_dict([
            ("topline", 23),
            ("leftcol", -1),
            ("textoff", 2),
            ("winrow", 7),
            ("wincol", 13),
            ("width", 80),
            ("height", 24),
        ]);

        assert!(matches!(
            parse_window_surface_snapshot(11, 17, &dict)
                .expect_err("negative leftcol should be rejected"),
            WindowSurfaceReadError::InvalidNonNegativeField {
                field: "leftcol",
                value: -1,
            }
        ));
    }

    #[test]
    fn parse_window_surface_snapshot_rejects_negative_textoff() {
        let dict = getwininfo_dict([
            ("topline", 23),
            ("leftcol", 5),
            ("textoff", -1),
            ("winrow", 7),
            ("wincol", 13),
            ("width", 80),
            ("height", 24),
        ]);

        assert!(matches!(
            parse_window_surface_snapshot(11, 17, &dict)
                .expect_err("negative textoff should be rejected"),
            WindowSurfaceReadError::InvalidNonNegativeField {
                field: "textoff",
                value: -1,
            }
        ));
    }

    #[test]
    fn parse_window_surface_snapshot_rejects_zero_topline() {
        let dict = getwininfo_dict([
            ("topline", 0),
            ("leftcol", 5),
            ("textoff", 2),
            ("winrow", 7),
            ("wincol", 13),
            ("width", 80),
            ("height", 24),
        ]);

        assert!(matches!(
            parse_window_surface_snapshot(11, 17, &dict)
                .expect_err("zero topline should be rejected"),
            WindowSurfaceReadError::InvalidTopline { topline: 0 }
        ));
    }

    #[test]
    fn parse_window_surface_snapshot_rejects_zero_window_origin() {
        let dict = getwininfo_dict([
            ("topline", 23),
            ("leftcol", 5),
            ("textoff", 2),
            ("winrow", 0),
            ("wincol", 13),
            ("width", 80),
            ("height", 24),
        ]);

        assert!(matches!(
            parse_window_surface_snapshot(11, 17, &dict)
                .expect_err("zero window row should be rejected"),
            WindowSurfaceReadError::InvalidWindowOrigin { row: 0, col: 13 }
        ));
    }

    #[test]
    fn parse_window_surface_snapshot_rejects_zero_window_size() {
        let dict = getwininfo_dict([
            ("topline", 23),
            ("leftcol", 5),
            ("textoff", 2),
            ("winrow", 7),
            ("wincol", 13),
            ("width", 0),
            ("height", 24),
        ]);

        assert!(matches!(
            parse_window_surface_snapshot(11, 17, &dict)
                .expect_err("zero width should be rejected"),
            WindowSurfaceReadError::InvalidWindowSize {
                width: 0,
                height: 24,
            }
        ));
    }

    #[test]
    fn parse_window_surface_snapshot_rejects_non_positive_surface_handles() {
        assert!(matches!(
            parse_window_surface_snapshot(0, 17, &canonical_getwininfo_dict())
                .expect_err("zero window handle should be rejected"),
            WindowSurfaceReadError::InvalidSurfaceId {
                window_handle: 0,
                buffer_handle: 17,
            }
        ));
    }

    #[test]
    fn parse_window_surface_snapshot_rejects_missing_required_field() {
        let dict = getwininfo_dict([
            ("topline", 23),
            ("leftcol", 5),
            ("textoff", 2),
            ("winrow", 7),
            ("wincol", 13),
            ("height", 24),
        ]);

        assert!(matches!(
            parse_window_surface_snapshot(11, 17, &dict)
                .expect_err("missing width should be rejected"),
            WindowSurfaceReadError::MissingField { field: "width" }
        ));
    }
}
