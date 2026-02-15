use crate::reducer::PreviewToken;
use nvim_oxi::conversion::FromObject;
use nvim_oxi::serde::Deserializer;
use nvim_oxi::{Dictionary, Object, String as NvimString};
use nvim_oxi_utils::handles::{BufHandle, WinHandle};
use serde::Deserialize;
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

fn decode<T>(args: &Dictionary) -> ParseResult<T>
where
    T: for<'de> Deserialize<'de>,
{
    T::deserialize(Deserializer::new(Object::from(args.clone()))).map_err(|err| {
        ArgsError::Unexpected {
            message: err.to_string(),
        }
    })
}

fn require_from_object<T>(
    maybe_obj: Option<Object>,
    key: &'static str,
    expected: &'static str,
) -> ParseResult<T>
where
    T: FromObject,
{
    let obj = maybe_obj.ok_or_else(|| ArgsError::MissingKey {
        key: key.to_string(),
    })?;
    T::from_object(obj).map_err(|_| ArgsError::InvalidValue {
        key: key.to_string(),
        expected,
    })
}

fn require_i64(maybe_obj: Option<Object>, key: &'static str) -> ParseResult<i64> {
    require_from_object(maybe_obj, key, "i64")
}

fn require_string(maybe_obj: Option<Object>, key: &'static str) -> ParseResult<String> {
    let value: NvimString = require_from_object(maybe_obj, key, "string")?;
    Ok(value.to_string_lossy().into_owned())
}

fn parse_buf_handle(value: i64, key: &'static str) -> ParseResult<BufHandle> {
    BufHandle::try_from_i64(value).ok_or_else(|| ArgsError::InvalidHandle {
        key: key.to_string(),
        value,
    })
}

fn parse_win_handle(value: i64, key: &'static str) -> ParseResult<WinHandle> {
    WinHandle::try_from_i64(value).ok_or_else(|| ArgsError::InvalidHandle {
        key: key.to_string(),
        value,
    })
}

fn parse_preview_token(value: i64, key: &'static str) -> ParseResult<PreviewToken> {
    PreviewToken::try_new(value).ok_or_else(|| ArgsError::InvalidToken {
        key: key.to_string(),
        value,
    })
}

fn parse_nonempty_string(value: String, key: &'static str) -> ParseResult<NonEmptyString> {
    NonEmptyString::try_new(value).map_err(|_| ArgsError::EmptyValue {
        key: key.to_string(),
    })
}

fn first_img_src(imgs: Option<Object>) -> Option<NonEmptyString> {
    let imgs = Vec::<RawImage>::deserialize(Deserializer::new(imgs?)).ok()?;
    let src = imgs.into_iter().next()?.src?;
    NonEmptyString::try_new(src).ok()
}

#[derive(Debug, Deserialize)]
struct RawImage {
    #[serde(default)]
    src: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawDocFindArgs {
    #[serde(default)]
    buf: Option<Object>,
    #[serde(default)]
    token: Option<Object>,
    #[serde(default)]
    win: Option<Object>,
    #[serde(default)]
    imgs: Option<Object>,
}

#[derive(Debug, Deserialize)]
struct RawAttachDocPreviewArgs {
    #[serde(default)]
    buf: Option<Object>,
    #[serde(default)]
    win: Option<Object>,
    #[serde(default)]
    path: Option<Object>,
}

fn decode_doc_find_args(args: &Dictionary) -> ParseResult<DocFindArgs> {
    let raw: RawDocFindArgs = decode(args)?;
    let buf_handle = parse_buf_handle(require_i64(raw.buf, "buf")?, "buf")?;
    let token = parse_preview_token(require_i64(raw.token, "token")?, "token")?;
    let win_handle = parse_win_handle(require_i64(raw.win, "win")?, "win")?;
    let img_src = first_img_src(raw.imgs);
    Ok(DocFindArgs {
        buf_handle,
        token,
        win_handle,
        img_src,
    })
}

fn decode_attach_doc_preview_args(args: &Dictionary) -> ParseResult<AttachDocPreviewArgs> {
    let raw: RawAttachDocPreviewArgs = decode(args)?;
    let buf_handle = parse_buf_handle(require_i64(raw.buf, "buf")?, "buf")?;
    let win_handle = parse_win_handle(require_i64(raw.win, "win")?, "win")?;
    let path = parse_nonempty_string(require_string(raw.path, "path")?, "path")?;
    Ok(AttachDocPreviewArgs {
        buf_handle,
        win_handle,
        path,
    })
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
        decode_doc_find_args(args)
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
        decode_attach_doc_preview_args(args)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nvim_oxi::Array;

    fn dict(entries: impl IntoIterator<Item = (&'static str, Object)>) -> Dictionary {
        Dictionary::from_iter(entries)
    }

    #[test]
    fn parse_doc_find_with_img_src() {
        let args = dict([
            ("buf", Object::from(10_i64)),
            ("token", Object::from(1_i64)),
            ("win", Object::from(20_i64)),
            (
                "imgs",
                Object::from(Array::from_iter([Object::from(Dictionary::from_iter([(
                    "src",
                    Object::from("https://example.test/image.png"),
                )]))])),
            ),
        ]);

        let parsed = DocFindArgs::parse(&args).expect("expected valid args");
        assert_eq!(parsed.buf_handle.raw(), 10);
        assert_eq!(parsed.win_handle.raw(), 20);
        assert_eq!(parsed.token.raw(), 1);
        let img = parsed.img_src.expect("expected image source");
        assert_eq!(img.as_str(), "https://example.test/image.png");
    }

    #[test]
    fn parse_doc_find_ignores_empty_img_src() {
        let args = dict([
            ("buf", Object::from(10_i64)),
            ("token", Object::from(1_i64)),
            ("win", Object::from(20_i64)),
            (
                "imgs",
                Object::from(Array::from_iter([Object::from(Dictionary::from_iter([(
                    "src",
                    Object::from(""),
                )]))])),
            ),
        ]);

        let parsed = DocFindArgs::parse(&args).expect("expected valid args");
        assert!(parsed.img_src.is_none());
    }

    #[test]
    fn parse_doc_find_rejects_missing_buf() {
        let args = dict([
            ("token", Object::from(1_i64)),
            ("win", Object::from(20_i64)),
        ]);
        let err = DocFindArgs::parse(&args).expect_err("expected parse failure");
        assert!(matches!(err, ArgsError::MissingKey { key } if key == "buf"));
    }

    #[test]
    fn parse_attach_doc_preview_rejects_empty_path() {
        let args = dict([
            ("buf", Object::from(10_i64)),
            ("win", Object::from(20_i64)),
            ("path", Object::from("")),
        ]);
        let err = AttachDocPreviewArgs::parse(&args).expect_err("expected parse failure");
        assert!(matches!(err, ArgsError::EmptyValue { key } if key == "path"));
    }

    #[test]
    fn parse_attach_doc_preview_rejects_invalid_handle() {
        let args = dict([
            ("buf", Object::from(0_i64)),
            ("win", Object::from(20_i64)),
            ("path", Object::from("/tmp/doc.md")),
        ]);
        let err = AttachDocPreviewArgs::parse(&args).expect_err("expected parse failure");
        assert!(matches!(
            err,
            ArgsError::InvalidHandle { key, value } if key == "buf" && value == 0
        ));
    }
}
