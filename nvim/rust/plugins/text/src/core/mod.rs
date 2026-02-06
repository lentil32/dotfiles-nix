use std::cmp::Ordering;
use std::collections::HashSet;

mod types;

pub use types::{LineRange, TextRangeError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Column(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDirection {
    Asc,
    Desc,
}

pub fn parse_line_index(value: i64) -> Result<usize, TextRangeError> {
    if value < 1 {
        return Err(TextRangeError::InvalidLineIndex { value });
    }
    usize::try_from(value).map_err(|_| TextRangeError::InvalidLineIndex { value })
}

pub fn resolve_line_range(
    start_line: Option<i64>,
    end_line: Option<i64>,
    cursor_row: usize,
    line_count: usize,
) -> Result<LineRange, TextRangeError> {
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
        (None, None) => (cursor_row, cursor_row),
    };
    LineRange::new(start, end)?.ensure_within(line_count)
}

#[derive(Debug)]
struct IndexedLine {
    idx: usize,
    line: String,
}

const fn apply_direction(order: Ordering, direction: SortDirection) -> Ordering {
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
    pub const fn new(seed: u64) -> Self {
        let seed = if seed == 0 {
            0x9E37_79B9_7F4A_7C15
        } else {
            seed
        };
        Self { state: seed }
    }
}

impl RngCore for SmallRng {
    fn next_u64(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
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
        let value = rng.next_u64() % upper;
        let j = usize::try_from(value).map_or(0, |value| value);
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
        let lines = vec!["beta".to_string(), "alpha".to_string(), "gamma".to_string()];
        let asc = sort_lines(&lines, SortDirection::Asc);
        let desc = sort_lines(&lines, SortDirection::Desc);
        assert_eq!(
            asc,
            vec!["alpha".to_string(), "beta".to_string(), "gamma".to_string()]
        );
        assert_eq!(
            desc,
            vec!["gamma".to_string(), "beta".to_string(), "alpha".to_string()]
        );
    }

    #[test]
    fn sort_lines_by_column_uses_substring() {
        let lines = vec!["x:2".to_string(), "x:10".to_string(), "x:1".to_string()];
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
        assert_eq!(
            uniq,
            vec!["a".to_string(), "b".to_string(), "c".to_string()]
        );
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
            let value = self.values.get(self.idx).map_or(0, |value| *value);
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
            vec![
                "b".to_string(),
                "c".to_string(),
                "d".to_string(),
                "a".to_string()
            ]
        );
    }

    #[test]
    fn resolve_line_range_uses_cursor_when_range_missing() -> Result<(), &'static str> {
        let range = resolve_line_range(None, None, 4, 8).map_err(|_| "expected valid range")?;
        assert_eq!(range.to_zero_based(), (3, 4));
        Ok(())
    }

    #[test]
    fn resolve_line_range_reorders_start_end() -> Result<(), &'static str> {
        let range =
            resolve_line_range(Some(9), Some(3), 1, 10).map_err(|_| "expected valid range")?;
        assert_eq!(range.to_zero_based(), (2, 9));
        Ok(())
    }

    #[test]
    fn resolve_line_range_rejects_out_of_bounds() {
        let result = resolve_line_range(Some(10), Some(11), 1, 5);
        assert!(matches!(
            result,
            Err(TextRangeError::RangeOutOfBounds {
                start: 10,
                end: 11,
                line_count: 5
            })
        ));
    }
}
