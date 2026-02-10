use crate::core::PreviewToken;
use nvim_oxi::conversion::FromObject;
use nvim_oxi::{Array, Dictionary};
use nvim_oxi_utils::Error as OxiError;
use nvim_oxi_utils::dict;
use nvim_oxi_utils::handles::{BufHandle, WinHandle};
use support::NonEmptyString;

#[derive(Debug)]
pub enum ArgsError {
    MissingKey { key: String },
    InvalidValue { key: String, expected: &'static str },
    InvalidHandle { key: String, value: i64 },
    InvalidToken { key: String, value: i64 },
    EmptyValue { key: String },
    Unexpected { message: String },
}

pub type ParseResult<T> = std::result::Result<T, ArgsError>;

impl std::fmt::Display for ArgsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingKey { key } => write!(f, "missing key '{key}'"),
            Self::InvalidValue { key, expected } => {
                write!(f, "invalid value for '{key}', expected {expected}")
            }
            Self::InvalidHandle { key, value } => {
                write!(f, "invalid handle for '{key}': {value}")
            }
            Self::InvalidToken { key, value } => {
                write!(f, "invalid token for '{key}': {value}")
            }
            Self::EmptyValue { key } => write!(f, "empty value for '{key}'"),
            Self::Unexpected { message } => write!(f, "{message}"),
        }
    }
}

impl From<OxiError> for ArgsError {
    fn from(err: OxiError) -> Self {
        match err {
            OxiError::MissingKey { key } => Self::MissingKey { key },
            OxiError::InvalidValue { key, expected } => Self::InvalidValue { key, expected },
            OxiError::Nvim(err) => Self::Unexpected {
                message: err.to_string(),
            },
        }
    }
}

fn require_i64(args: &Dictionary, key: &str) -> ParseResult<i64> {
    dict::require_i64(args, key).map_err(ArgsError::from)
}

fn require_buf_handle(args: &Dictionary, key: &str) -> ParseResult<BufHandle> {
    let value = require_i64(args, key)?;
    BufHandle::try_from_i64(value).ok_or_else(|| ArgsError::InvalidHandle {
        key: key.to_string(),
        value,
    })
}

fn require_win_handle(args: &Dictionary, key: &str) -> ParseResult<WinHandle> {
    let value = require_i64(args, key)?;
    WinHandle::try_from_i64(value).ok_or_else(|| ArgsError::InvalidHandle {
        key: key.to_string(),
        value,
    })
}

fn require_preview_token(args: &Dictionary, key: &str) -> ParseResult<PreviewToken> {
    let value = require_i64(args, key)?;
    PreviewToken::try_new(value).ok_or_else(|| ArgsError::InvalidToken {
        key: key.to_string(),
        value,
    })
}

fn require_nonempty_string(args: &Dictionary, key: &str) -> ParseResult<NonEmptyString> {
    let value = dict::require_string_nonempty(args, key).map_err(|err| match err {
        OxiError::InvalidValue {
            expected: "non-empty string",
            ..
        } => ArgsError::EmptyValue {
            key: key.to_string(),
        },
        _ => ArgsError::from(err),
    })?;
    NonEmptyString::try_new(value).map_err(|_| ArgsError::EmptyValue {
        key: key.to_string(),
    })
}

fn first_img_src(args: &Dictionary) -> Option<NonEmptyString> {
    let imgs_obj = dict::get_object(args, "imgs")?;
    let imgs = Array::from_object(imgs_obj).ok()?;
    let first = imgs.iter().next()?;
    let img = Dictionary::from_object(first.clone()).ok()?;
    let src = dict::get_string_nonempty(&img, "src")?;
    NonEmptyString::try_new(src).ok()
}

#[derive(Debug)]
pub struct DocFindArgs {
    pub buf_handle: BufHandle,
    pub token: PreviewToken,
    pub win_handle: WinHandle,
    pub img_src: Option<NonEmptyString>,
}

impl DocFindArgs {
    pub fn parse(args: &Dictionary) -> ParseResult<Self> {
        let buf_handle = require_buf_handle(args, "buf")?;
        let token = require_preview_token(args, "token")?;
        let win_handle = require_win_handle(args, "win")?;
        let img_src = first_img_src(args);
        Ok(Self {
            buf_handle,
            token,
            win_handle,
            img_src,
        })
    }
}

#[derive(Debug)]
pub struct AttachDocPreviewArgs {
    pub buf_handle: BufHandle,
    pub win_handle: WinHandle,
    pub path: NonEmptyString,
}

impl AttachDocPreviewArgs {
    pub fn parse(args: &Dictionary) -> ParseResult<Self> {
        let buf_handle = require_buf_handle(args, "buf")?;
        let win_handle = require_win_handle(args, "win")?;
        let path = require_nonempty_string(args, "path")?;
        Ok(Self {
            buf_handle,
            win_handle,
            path,
        })
    }
}
