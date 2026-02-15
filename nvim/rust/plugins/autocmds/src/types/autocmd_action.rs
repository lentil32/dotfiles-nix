#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutocmdAction {
    Keep,
}

impl AutocmdAction {
    pub const fn as_bool(self) -> bool {
        match self {
            Self::Keep => false,
        }
    }
}
