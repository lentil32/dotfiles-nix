#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineRange {
    start: usize,
    end: usize,
}

impl LineRange {
    pub const fn new(start: usize, end: usize) -> Result<Self, TextRangeError> {
        if start == 0 || end == 0 {
            return Err(TextRangeError::InvalidLineIndex { value: 0 });
        }
        let (start, end) = if start <= end {
            (start, end)
        } else {
            (end, start)
        };
        Ok(Self { start, end })
    }

    pub const fn ensure_within(self, line_count: usize) -> Result<Self, TextRangeError> {
        if line_count == 0 {
            return Err(TextRangeError::EmptyBuffer);
        }
        if self.start > line_count || self.end > line_count {
            return Err(TextRangeError::RangeOutOfBounds {
                start: self.start,
                end: self.end,
                line_count,
            });
        }
        Ok(self)
    }

    pub const fn to_zero_based(self) -> (usize, usize) {
        (self.start - 1, self.end)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextRangeError {
    InvalidLineIndex {
        value: i64,
    },
    RangeOutOfBounds {
        start: usize,
        end: usize,
        line_count: usize,
    },
    EmptyBuffer,
}

impl std::fmt::Display for TextRangeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidLineIndex { value } => {
                write!(f, "line index must be >= 1, got {value}")
            }
            Self::RangeOutOfBounds {
                start,
                end,
                line_count,
            } => write!(
                f,
                "line range {start}-{end} exceeds buffer line count {line_count}"
            ),
            Self::EmptyBuffer => write!(f, "buffer has no lines"),
        }
    }
}

impl std::error::Error for TextRangeError {}
