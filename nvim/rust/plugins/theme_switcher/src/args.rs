use std::path::PathBuf;

use crate::core::{ThemeSpec, ThemeSpecError};
use nvim_oxi::serde::Deserializer;
use nvim_oxi::{Dictionary, Object, String as NvimString};
use nvim_oxi_utils::{Error as DecodeError, decode};
use serde::Deserialize;

#[derive(Debug)]
pub enum ArgsError {
    MissingKey {
        key: String,
    },
    InvalidValue {
        key: String,
        expected: &'static str,
    },
    EmptyThemes,
    InvalidTheme {
        index: usize,
        reason: ThemeSpecError,
    },
    Unexpected {
        message: String,
    },
}

pub type ParseResult<T> = std::result::Result<T, ArgsError>;

impl std::fmt::Display for ArgsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingKey { key } => write!(f, "missing key '{key}'"),
            Self::InvalidValue { key, expected } => {
                write!(f, "invalid value for '{key}', expected {expected}")
            }
            Self::EmptyThemes => write!(f, "theme list must be non-empty"),
            Self::InvalidTheme { index, reason } => {
                write!(f, "theme[{index}] is invalid: {reason}")
            }
            Self::Unexpected { message } => write!(f, "{message}"),
        }
    }
}

impl From<DecodeError> for ArgsError {
    fn from(value: DecodeError) -> Self {
        match value {
            DecodeError::MissingKey { key } => Self::MissingKey { key },
            DecodeError::InvalidValue { key, expected } => Self::InvalidValue { key, expected },
            DecodeError::EmptyValue { key } => Self::InvalidValue {
                key,
                expected: "non-empty value",
            },
            DecodeError::Unexpected { message } => Self::Unexpected { message },
            DecodeError::Nvim(err) => Self::Unexpected {
                message: err.to_string(),
            },
        }
    }
}

#[derive(Debug, Deserialize)]
struct RawThemeSpec {
    name: String,
    colorscheme: String,
}

fn parse_theme_specs(value: Object) -> ParseResult<Vec<ThemeSpec>> {
    let raw = Vec::<RawThemeSpec>::deserialize(Deserializer::new(value)).map_err(|_| {
        ArgsError::InvalidValue {
            key: "themes".to_string(),
            expected: "array[{ name: string, colorscheme: string }]",
        }
    })?;

    if raw.is_empty() {
        return Err(ArgsError::EmptyThemes);
    }

    raw.into_iter()
        .enumerate()
        .map(|(index, theme)| {
            ThemeSpec::try_new(theme.name, theme.colorscheme).map_err(|reason| {
                ArgsError::InvalidTheme {
                    index: index + 1,
                    reason,
                }
            })
        })
        .collect()
}

fn parse_optional_string(dict: &Dictionary, key: &'static str) -> ParseResult<Option<String>> {
    let value =
        decode::optional_from_object::<NvimString>(decode::get_object(dict, key), key, "string")
            .map_err(ArgsError::from)?;
    Ok(value
        .map(|value| value.to_string_lossy().into_owned())
        .filter(|value| !value.is_empty()))
}

#[derive(Debug)]
pub struct OpenArgs {
    pub themes: Vec<ThemeSpec>,
    pub title: String,
    pub current_colorscheme: Option<String>,
    pub state_path: Option<PathBuf>,
}

impl OpenArgs {
    pub fn parse(dict: &Dictionary) -> ParseResult<Self> {
        let themes_value = decode::require_object(dict, "themes").map_err(ArgsError::from)?;
        let themes = parse_theme_specs(themes_value)?;
        let title =
            parse_optional_string(dict, "title")?.unwrap_or_else(|| "Theme Switcher".to_string());
        let current_colorscheme = parse_optional_string(dict, "current_colorscheme")?;
        let state_path = parse_optional_string(dict, "state_path")?.map(PathBuf::from);
        Ok(Self {
            themes,
            title,
            current_colorscheme,
            state_path,
        })
    }
}

#[derive(Debug)]
pub struct CycleArgs {
    pub themes: Vec<ThemeSpec>,
    pub current_colorscheme: Option<String>,
    pub state_path: Option<PathBuf>,
}

impl CycleArgs {
    pub fn parse(dict: &Dictionary) -> ParseResult<Self> {
        let OpenArgs {
            themes,
            current_colorscheme,
            state_path,
            ..
        } = OpenArgs::parse(dict)?;
        Ok(Self {
            themes,
            current_colorscheme,
            state_path,
        })
    }
}
