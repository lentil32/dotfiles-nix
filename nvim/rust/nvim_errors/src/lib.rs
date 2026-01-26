use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("missing key `{key}`")]
    MissingKey { key: String },
    #[error("invalid value for `{key}`; expected {expected}")]
    InvalidValue { key: String, expected: &'static str },
    #[error("nvim error: {0}")]
    Nvim(#[from] nvim_oxi::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn missing_key(key: &str) -> Self {
        Self::MissingKey {
            key: key.to_string(),
        }
    }

    pub fn invalid_value(key: &str, expected: &'static str) -> Self {
        Self::InvalidValue {
            key: key.to_string(),
            expected,
        }
    }
}
