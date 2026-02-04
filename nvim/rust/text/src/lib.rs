use std::cmp::Ordering;
use std::time::{SystemTime, UNIX_EPOCH};

use nvim_oxi::api;
use nvim_oxi::api::Buffer;
use nvim_oxi::{Dictionary, Function, Result};

mod core {
    use super::Ordering;
    use std::collections::HashSet;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Column(pub usize);

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum SortDirection {
        Asc,
        Desc,
    }

    #[derive(Debug)]
    struct IndexedLine {
        idx: usize,
        line: String,
    }

    fn apply_direction(order: Ordering, direction: SortDirection) -> Ordering {
        match direction {
            SortDirection::Asc => order,
            SortDirection::Desc => order.reverse(),
        }
    }

    fn stable_cmp(
        order: Ordering,
        direction: SortDirection,
        left_idx: usize,
        right_idx: usize,
    ) -> Ordering {
        let ordered = apply_direction(order, direction);
        if ordered == Ordering::Equal {
            left_idx.cmp(&right_idx)
        } else {
            ordered
        }
    }

    fn sort_with<F>(lines: &[String], mut cmp: F) -> Vec<String>
    where
        F: FnMut(&IndexedLine, &IndexedLine) -> Ordering,
    {
        let mut indexed: Vec<IndexedLine> = lines
            .iter()
            .cloned()
            .enumerate()
            .map(|(idx, line)| IndexedLine { idx, line })
            .collect();
        indexed.sort_by(|left, right| cmp(left, right));
        indexed.into_iter().map(|entry| entry.line).collect()
    }

    pub fn sort_lines(lines: &[String], direction: SortDirection) -> Vec<String> {
        sort_with(lines, |left, right| {
            stable_cmp(left.line.cmp(&right.line), direction, left.idx, right.idx)
        })
    }

    fn slice_from_column(line: &str, column: Column) -> &str {
        let col = column.0;
        if col == 0 {
            return line;
        }
        if col >= line.len() {
            return "";
        }
        if line.is_char_boundary(col) {
            return &line[col..];
        }
        for (idx, _) in line.char_indices() {
            if idx >= col {
                return &line[idx..];
            }
        }
        ""
    }

    pub fn sort_lines_by_column(
        lines: &[String],
        column: Column,
        direction: SortDirection,
    ) -> Vec<String> {
        sort_with(lines, |left, right| {
            let left_key = slice_from_column(&left.line, column);
            let right_key = slice_from_column(&right.line, column);
            stable_cmp(left_key.cmp(right_key), direction, left.idx, right.idx)
        })
    }

    pub fn uniquify_lines(lines: &[String]) -> Vec<String> {
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for line in lines.iter().cloned() {
            if seen.insert(line.clone()) {
                out.push(line);
            }
        }
        out
    }

    pub trait RngCore {
        fn next_u64(&mut self) -> u64;
    }

    #[derive(Debug, Clone)]
    pub struct SmallRng {
        state: u64,
    }

    impl SmallRng {
        pub fn new(seed: u64) -> Self {
            let seed = if seed == 0 { 0x9E3779B97F4A7C15 } else { seed };
            Self { state: seed }
        }
    }

    impl RngCore for SmallRng {
        fn next_u64(&mut self) -> u64 {
            self.state = self
                .state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1);
            self.state
        }
    }

    fn shuffle_with_rng<T>(items: &mut [T], rng: &mut impl RngCore) {
        let len = items.len();
        if len <= 1 {
            return;
        }
        for i in (1..len).rev() {
            let upper = (i + 1) as u64;
            let j = (rng.next_u64() % upper) as usize;
            items.swap(i, j);
        }
    }

    pub fn randomize_lines(lines: &[String], seed: u64) -> Vec<String> {
        let mut out = lines.to_vec();
        let mut rng = SmallRng::new(seed);
        shuffle_with_rng(&mut out, &mut rng);
        out
    }

    fn indentation_end(line: &str) -> usize {
        for (idx, ch) in line.char_indices() {
            if !ch.is_whitespace() {
                return idx;
            }
        }
        line.len()
    }

    fn clamp_to_boundary(line: &str, col: usize) -> usize {
        let col = col.min(line.len());
        if line.is_char_boundary(col) {
            return col;
        }
        let mut last = 0;
        for (idx, _) in line.char_indices() {
            if idx > col {
                break;
            }
            last = idx;
        }
        last
    }

    fn kill_line_back_to_indentation(line: &str, column: Column) -> String {
        let indent = indentation_end(line);
        let col = clamp_to_boundary(line, column.0);
        let (start, end) = if col <= indent {
            (0, col)
        } else {
            (indent, col)
        };
        if start >= end {
            return line.to_string();
        }
        let mut out = String::with_capacity(line.len().saturating_sub(end - start));
        out.push_str(&line[..start]);
        out.push_str(&line[end..]);
        out
    }

    pub fn kill_back_to_indentation(lines: &[String], column: Column) -> Vec<String> {
        lines
            .iter()
            .map(|line| kill_line_back_to_indentation(line, column))
            .collect()
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn sort_lines_asc_and_desc() {
            let lines = vec![
                "beta".to_string(),
                "alpha".to_string(),
                "gamma".to_string(),
            ];
            let asc = sort_lines(&lines, SortDirection::Asc);
            let desc = sort_lines(&lines, SortDirection::Desc);
            assert_eq!(
                asc,
                vec![
                    "alpha".to_string(),
                    "beta".to_string(),
                    "gamma".to_string()
                ]
            );
            assert_eq!(
                desc,
                vec![
                    "gamma".to_string(),
                    "beta".to_string(),
                    "alpha".to_string()
                ]
            );
        }

        #[test]
        fn sort_lines_by_column_uses_substring() {
            let lines = vec![
                "x:2".to_string(),
                "x:10".to_string(),
                "x:1".to_string(),
            ];
            let sorted = sort_lines_by_column(&lines, Column(2), SortDirection::Asc);
            assert_eq!(
                sorted,
                vec!["x:1".to_string(), "x:10".to_string(), "x:2".to_string()]
            );
        }

        #[test]
        fn uniquify_keeps_first_occurrence() {
            let lines = vec![
                "a".to_string(),
                "b".to_string(),
                "a".to_string(),
                "c".to_string(),
                "b".to_string(),
            ];
            let uniq = uniquify_lines(&lines);
            assert_eq!(uniq, vec!["a".to_string(), "b".to_string(), "c".to_string()]);
        }

        #[test]
        fn kill_back_to_indentation_respects_indent() {
            let lines = vec!["  foo bar".to_string()];
            let out = kill_back_to_indentation(&lines, Column(5));
            assert_eq!(out, vec!["   bar".to_string()]);
        }

        #[test]
        fn kill_back_to_indentation_before_indent() {
            let lines = vec!["  foo".to_string()];
            let out = kill_back_to_indentation(&lines, Column(1));
            assert_eq!(out, vec![" foo".to_string()]);
        }

        struct SeqRng {
            values: Vec<u64>,
            idx: usize,
        }

        impl SeqRng {
            fn new(values: Vec<u64>) -> Self {
                Self { values, idx: 0 }
            }
        }

        impl RngCore for SeqRng {
            fn next_u64(&mut self) -> u64 {
                let value = self.values.get(self.idx).copied().unwrap_or(0);
                self.idx = self.idx.saturating_add(1);
                value
            }
        }

        #[test]
        fn randomize_lines_uses_rng() {
            let mut lines = vec![
                "a".to_string(),
                "b".to_string(),
                "c".to_string(),
                "d".to_string(),
            ];
            let mut rng = SeqRng::new(vec![0, 0, 0]);
            shuffle_with_rng(&mut lines, &mut rng);
            assert_eq!(
                lines,
                vec!["b".to_string(), "c".to_string(), "d".to_string(), "a".to_string()]
            );
        }
    }
}

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
        let (start, end) = if start <= end { (start, end) } else { (end, start) };
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

    fn to_zero_based(self) -> (usize, usize) {
        (self.start - 1, self.end)
    }
}

#[derive(Debug)]
enum TextError {
    InvalidLineIndex { value: i64 },
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
            TextError::InvalidLineIndex { value } => {
                write!(f, "line index must be >= 1, got {value}")
            }
            TextError::RangeOutOfBounds {
                start,
                end,
                line_count,
            } => write!(
                f,
                "line range {start}-{end} exceeds buffer line count {line_count}"
            ),
            TextError::EmptyBuffer => write!(f, "buffer has no lines"),
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
    let sorted = core::sort_lines(&lines, core::SortDirection::Asc);
    replace_lines(&mut buf, range, sorted)
}

fn sort_lines_reverse((start_line, end_line): (Option<i64>, Option<i64>)) -> Result<()> {
    let mut buf = api::get_current_buf();
    let range = resolve_line_range(&buf, start_line, end_line)?;
    let lines = fetch_lines(&buf, range)?;
    let sorted = core::sort_lines(&lines, core::SortDirection::Desc);
    replace_lines(&mut buf, range, sorted)
}

fn sort_lines_by_column((start_line, end_line): (Option<i64>, Option<i64>)) -> Result<()> {
    let mut buf = api::get_current_buf();
    let range = resolve_line_range(&buf, start_line, end_line)?;
    let lines = fetch_lines(&buf, range)?;
    let column = core::Column(current_cursor_col()?);
    let sorted = core::sort_lines_by_column(&lines, column, core::SortDirection::Asc);
    replace_lines(&mut buf, range, sorted)
}

fn sort_lines_by_column_reverse((start_line, end_line): (Option<i64>, Option<i64>)) -> Result<()> {
    let mut buf = api::get_current_buf();
    let range = resolve_line_range(&buf, start_line, end_line)?;
    let lines = fetch_lines(&buf, range)?;
    let column = core::Column(current_cursor_col()?);
    let sorted = core::sort_lines_by_column(&lines, column, core::SortDirection::Desc);
    replace_lines(&mut buf, range, sorted)
}

fn randomize_lines((start_line, end_line): (Option<i64>, Option<i64>)) -> Result<()> {
    let mut buf = api::get_current_buf();
    let range = resolve_line_range(&buf, start_line, end_line)?;
    let lines = fetch_lines(&buf, range)?;
    let shuffled = core::randomize_lines(&lines, seed_from_time());
    replace_lines(&mut buf, range, shuffled)
}

fn uniquify_lines((start_line, end_line): (Option<i64>, Option<i64>)) -> Result<()> {
    let mut buf = api::get_current_buf();
    let range = resolve_line_range(&buf, start_line, end_line)?;
    let lines = fetch_lines(&buf, range)?;
    let uniq = core::uniquify_lines(&lines);
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
    let column = core::Column(current_cursor_col()?);
    let killed = core::kill_back_to_indentation(&lines, column);
    replace_lines(&mut buf, range, killed)
}

#[nvim_oxi::plugin]
fn my_text() -> Result<Dictionary> {
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
    Ok(api)
}
