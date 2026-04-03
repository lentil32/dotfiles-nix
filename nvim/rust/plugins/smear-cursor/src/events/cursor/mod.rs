mod buffer_meta;
mod color_probe;
mod conceal;
mod screenpos;

pub(super) use buffer_meta::BufferMetadata;
pub(super) use color_probe::sampled_cursor_color_at_current_position;
pub(super) use screenpos::cursor_position_for_mode;
pub(super) use screenpos::cursor_position_read_for_mode_with_probe_policy;
pub(super) use screenpos::line_value;
pub(super) use screenpos::mode_string;
pub(super) use screenpos::smear_outside_cmd_row;

use crate::lua::LuaParseError;

type ScreenCell = (i64, i64);
type ScreenPoint = (f64, f64);
type CursorResult<T> = std::result::Result<T, CursorReadError>;

#[derive(Debug, thiserror::Error)]
enum CursorParseError {
    #[error("screenpos returned invalid dictionary")]
    ScreenposDictionary,
    #[error("screenpos missing row/col pair")]
    ScreenposMissingRowCol,
    #[error("getwininfo returned invalid list")]
    GetwininfoInvalidList,
    #[error("getwininfo returned unexpected list length")]
    GetwininfoUnexpectedLen,
    #[error("getwininfo returned invalid dictionary")]
    GetwininfoDictionary,
    #[error("synconcealed returned invalid list")]
    SynconcealedInvalidList,
    #[error("synconcealed returned unexpected list length")]
    SynconcealedUnexpectedLen,
    #[error("{context} missing {field}")]
    DictionaryMissingField {
        context: &'static str,
        field: &'static str,
    },
    #[error("{context} parse failed: {source}")]
    Value {
        context: String,
        #[source]
        source: LuaParseError,
    },
}

#[derive(Debug, thiserror::Error)]
enum CursorReadError {
    #[error(transparent)]
    Shell(#[from] nvim_oxi::Error),
    #[error(transparent)]
    Parse(#[from] CursorParseError),
}

impl From<CursorReadError> for nvim_oxi::Error {
    fn from(error: CursorReadError) -> Self {
        match error {
            CursorReadError::Shell(error) => error,
            CursorReadError::Parse(error) => nvim_oxi::api::Error::Other(error.to_string()).into(),
        }
    }
}

impl From<nvim_oxi::api::Error> for CursorReadError {
    fn from(error: nvim_oxi::api::Error) -> Self {
        Self::Shell(error.into())
    }
}

fn cursor_parse_error(context: impl Into<String>, source: LuaParseError) -> CursorReadError {
    CursorParseError::Value {
        context: context.into(),
        source,
    }
    .into()
}
