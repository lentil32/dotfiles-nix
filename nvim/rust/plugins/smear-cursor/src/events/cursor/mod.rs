mod buffer_meta;
mod color_probe;
mod conceal;
mod screenpos;

pub(super) use buffer_meta::BufferMetadata;
pub(in crate::events) use buffer_meta::BufferMetadataCache;
pub(super) use color_probe::sampled_cursor_color_at_current_position;
pub(super) use screenpos::current_mode;
pub(super) use screenpos::cursor_observation_for_mode_with_probe_policy;
pub(in crate::events) use screenpos::cursor_observation_for_mode_with_probe_policy_typed;
pub(super) use screenpos::smear_outside_cmd_row;

use crate::lua::LuaParseError;

pub(in crate::events) type CursorResult<T> = std::result::Result<T, CursorReadError>;

#[derive(Debug, thiserror::Error)]
pub(in crate::events) enum CursorParseError {
    #[error("{context} returned invalid dictionary")]
    InvalidDictionary { context: &'static str },
    #[error("screenpos returned invalid dictionary")]
    ScreenposDictionary,
    #[error("screenpos missing row/col pair")]
    ScreenposMissingRowCol,
    #[error("screenpos returned invalid one-based cell row={row} col={col}")]
    ScreenposInvalidCell { row: i64, col: i64 },
    #[error("window cursor returned invalid one-based buffer line {line}")]
    InvalidBufferLine { line: usize },
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
pub(in crate::events) enum CursorReadError {
    #[error(transparent)]
    Shell(#[from] nvim_oxi::Error),
    #[error(transparent)]
    Parse(#[from] CursorParseError),
}

impl From<CursorReadError> for nvim_oxi::Error {
    fn from(error: CursorReadError) -> Self {
        match error {
            CursorReadError::Shell(error) => error,
            CursorReadError::Parse(error) => {
                crate::host::api::Error::Other(error.to_string()).into()
            }
        }
    }
}

impl From<crate::host::api::Error> for CursorReadError {
    fn from(error: crate::host::api::Error) -> Self {
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
