use std::time::{SystemTime, UNIX_EPOCH};

use nvim_oxi::api;
use nvim_oxi::api::Buffer;
use nvim_oxi::{Dictionary, Function, Result};
use text_core::{Column, SortDirection};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LineRange {
    start: usize,
    end: usize,
}

impl LineRange {
    fn new(start: usize, end: usize) -> Result<Self> {
        if start == 0 || end == 0 {
            return Err(TextError::InvalidLineIndex { value: 0 }.into());
        }
        let (start, end) = if start <= end {
            (start, end)
        } else {
            (end, start)
        };
        Ok(Self { start, end })
    }

    fn ensure_within(self, line_count: usize) -> Result<Self> {
        if line_count == 0 {
            return Err(TextError::EmptyBuffer.into());
        }
        if self.start > line_count || self.end > line_count {
            return Err(TextError::RangeOutOfBounds {
                start: self.start,
                end: self.end,
                line_count,
            }
            .into());
        }
        Ok(self)
    }

    const fn to_zero_based(self) -> (usize, usize) {
        (self.start - 1, self.end)
    }
}

#[derive(Debug)]
enum TextError {
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

impl std::fmt::Display for TextError {
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

impl std::error::Error for TextError {}

impl From<TextError> for nvim_oxi::Error {
    fn from(err: TextError) -> Self {
        nvim_oxi::api::Error::Other(err.to_string()).into()
    }
}

fn parse_line_index(value: i64) -> Result<usize> {
    if value < 1 {
        return Err(TextError::InvalidLineIndex { value }.into());
    }
    usize::try_from(value).map_err(|_| TextError::InvalidLineIndex { value }.into())
}

fn resolve_line_range(
    buf: &Buffer,
    start_line: Option<i64>,
    end_line: Option<i64>,
) -> Result<LineRange> {
    let line_count = buf.line_count()?;
    let (start, end) = match (start_line, end_line) {
        (Some(start), Some(end)) => (parse_line_index(start)?, parse_line_index(end)?),
        (Some(start), None) => {
            let start = parse_line_index(start)?;
            (start, start)
        }
        (None, Some(end)) => {
            let end = parse_line_index(end)?;
            (end, end)
        }
        (None, None) => {
            let (row, _) = api::get_current_win().get_cursor()?;
            (row, row)
        }
    };
    LineRange::new(start, end)?.ensure_within(line_count)
}

fn current_cursor_col() -> Result<usize> {
    let (_, col) = api::get_current_win().get_cursor()?;
    Ok(col)
}

fn fetch_lines(buf: &Buffer, range: LineRange) -> Result<Vec<String>> {
    let (start, end) = range.to_zero_based();
    let mut lines = Vec::new();
    for line in buf.get_lines(start..end, false)? {
        lines.push(line.to_string_lossy().into_owned());
    }
    Ok(lines)
}

fn replace_lines(buf: &mut Buffer, range: LineRange, lines: Vec<String>) -> Result<()> {
    let (start, end) = range.to_zero_based();
    buf.set_lines(start..end, false, lines)?;
    Ok(())
}

fn insert_lines_after(buf: &mut Buffer, range: LineRange, lines: Vec<String>) -> Result<()> {
    let (_, end) = range.to_zero_based();
    buf.set_lines(end..end, false, lines)?;
    Ok(())
}

fn seed_from_time() -> u64 {
    let duration = match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration,
        Err(err) => err.duration(),
    };
    duration
        .as_secs()
        .wrapping_mul(1_000_000_000)
        .wrapping_add(u64::from(duration.subsec_nanos()))
}

fn sort_lines((start_line, end_line): (Option<i64>, Option<i64>)) -> Result<()> {
    let mut buf = api::get_current_buf();
    let range = resolve_line_range(&buf, start_line, end_line)?;
    let lines = fetch_lines(&buf, range)?;
    let sorted = text_core::sort_lines(&lines, SortDirection::Asc);
    replace_lines(&mut buf, range, sorted)
}

fn sort_lines_reverse((start_line, end_line): (Option<i64>, Option<i64>)) -> Result<()> {
    let mut buf = api::get_current_buf();
    let range = resolve_line_range(&buf, start_line, end_line)?;
    let lines = fetch_lines(&buf, range)?;
    let sorted = text_core::sort_lines(&lines, SortDirection::Desc);
    replace_lines(&mut buf, range, sorted)
}

fn sort_lines_by_column((start_line, end_line): (Option<i64>, Option<i64>)) -> Result<()> {
    let mut buf = api::get_current_buf();
    let range = resolve_line_range(&buf, start_line, end_line)?;
    let lines = fetch_lines(&buf, range)?;
    let column = Column(current_cursor_col()?);
    let sorted = text_core::sort_lines_by_column(&lines, column, SortDirection::Asc);
    replace_lines(&mut buf, range, sorted)
}

fn sort_lines_by_column_reverse((start_line, end_line): (Option<i64>, Option<i64>)) -> Result<()> {
    let mut buf = api::get_current_buf();
    let range = resolve_line_range(&buf, start_line, end_line)?;
    let lines = fetch_lines(&buf, range)?;
    let column = Column(current_cursor_col()?);
    let sorted = text_core::sort_lines_by_column(&lines, column, SortDirection::Desc);
    replace_lines(&mut buf, range, sorted)
}

fn randomize_lines((start_line, end_line): (Option<i64>, Option<i64>)) -> Result<()> {
    let mut buf = api::get_current_buf();
    let range = resolve_line_range(&buf, start_line, end_line)?;
    let lines = fetch_lines(&buf, range)?;
    let shuffled = text_core::randomize_lines(&lines, seed_from_time());
    replace_lines(&mut buf, range, shuffled)
}

fn uniquify_lines((start_line, end_line): (Option<i64>, Option<i64>)) -> Result<()> {
    let mut buf = api::get_current_buf();
    let range = resolve_line_range(&buf, start_line, end_line)?;
    let lines = fetch_lines(&buf, range)?;
    let uniq = text_core::uniquify_lines(&lines);
    replace_lines(&mut buf, range, uniq)
}

fn duplicate_line_or_region((start_line, end_line): (Option<i64>, Option<i64>)) -> Result<()> {
    let mut buf = api::get_current_buf();
    let range = resolve_line_range(&buf, start_line, end_line)?;
    let lines = fetch_lines(&buf, range)?;
    insert_lines_after(&mut buf, range, lines)
}

fn kill_back_to_indentation((start_line, end_line): (Option<i64>, Option<i64>)) -> Result<()> {
    let mut buf = api::get_current_buf();
    let range = resolve_line_range(&buf, start_line, end_line)?;
    let lines = fetch_lines(&buf, range)?;
    let column = Column(current_cursor_col()?);
    let killed = text_core::kill_back_to_indentation(&lines, column);
    replace_lines(&mut buf, range, killed)
}

#[nvim_oxi::plugin]
fn my_text() -> Dictionary {
    let mut api = Dictionary::new();
    api.insert(
        "sort_lines",
        Function::<(Option<i64>, Option<i64>), ()>::from_fn(sort_lines),
    );
    api.insert(
        "sort_lines_reverse",
        Function::<(Option<i64>, Option<i64>), ()>::from_fn(sort_lines_reverse),
    );
    api.insert(
        "sort_lines_by_column",
        Function::<(Option<i64>, Option<i64>), ()>::from_fn(sort_lines_by_column),
    );
    api.insert(
        "sort_lines_by_column_reverse",
        Function::<(Option<i64>, Option<i64>), ()>::from_fn(sort_lines_by_column_reverse),
    );
    api.insert(
        "randomize_lines",
        Function::<(Option<i64>, Option<i64>), ()>::from_fn(randomize_lines),
    );
    api.insert(
        "uniquify_lines",
        Function::<(Option<i64>, Option<i64>), ()>::from_fn(uniquify_lines),
    );
    api.insert(
        "duplicate_line_or_region",
        Function::<(Option<i64>, Option<i64>), ()>::from_fn(duplicate_line_or_region),
    );
    api.insert(
        "kill_back_to_indentation",
        Function::<(Option<i64>, Option<i64>), ()>::from_fn(kill_back_to_indentation),
    );
    api
}
