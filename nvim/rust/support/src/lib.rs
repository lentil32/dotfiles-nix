use std::path::{Path, PathBuf};

use derive_more::Display;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmptyStringError;

impl std::fmt::Display for EmptyStringError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "value must be non-empty")
    }
}

impl std::error::Error for EmptyStringError {}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd, Display)]
pub struct NonEmptyString(String);

impl NonEmptyString {
    pub fn try_new(value: String) -> Result<Self, EmptyStringError> {
        if value.is_empty() {
            Err(EmptyStringError)
        } else {
            Ok(Self(value))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }
}

impl TryFrom<String> for NonEmptyString {
    type Error = EmptyStringError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::try_new(value)
    }
}

impl From<NonEmptyString> for String {
    fn from(value: NonEmptyString) -> Self {
        value.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectRoot(PathBuf);

impl ProjectRoot {
    pub fn try_new(value: String) -> Result<Self, EmptyStringError> {
        let value = NonEmptyString::try_new(value)?;
        Ok(Self(PathBuf::from(value.into_string())))
    }

    pub fn as_path(&self) -> &Path {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Display)]
pub struct TabTitle(NonEmptyString);

impl TabTitle {
    pub fn try_new(value: String) -> Result<Self, EmptyStringError> {
        NonEmptyString::try_new(value).map(Self)
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    pub fn into_string(self) -> String {
        self.0.into_string()
    }
}

impl From<NonEmptyString> for TabTitle {
    fn from(value: NonEmptyString) -> Self {
        Self(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_empty_string_rejects_empty() {
        assert!(NonEmptyString::try_new(String::new()).is_err());
    }

    #[test]
    fn non_empty_string_accepts_value() -> Result<(), &'static str> {
        let value = NonEmptyString::try_new("ok".to_string()).map_err(|_| "expected non-empty")?;
        assert_eq!(value.as_str(), "ok");
        Ok(())
    }
}
