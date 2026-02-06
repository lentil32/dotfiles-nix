mod core;

use std::time::{SystemTime, UNIX_EPOCH};

use crate::core::{Column, LineRange, SortDirection, TextRangeError};
use nvim_oxi::api;
use nvim_oxi::api::Buffer;
use nvim_oxi::{Dictionary, Function, Result};

fn text_error_to_nvim(err: TextRangeError) -> nvim_oxi::Error {
    nvim_oxi::api::Error::Other(err.to_string()).into()
}

fn resolve_line_range(
    buf: &Buffer,
    start_line: Option<i64>,
    end_line: Option<i64>,
) -> Result<LineRange> {
    let line_count = buf.line_count()?;
    let cursor_row = if start_line.is_none() && end_line.is_none() {
        let (row, _) = api::get_current_win().get_cursor()?;
        row
    } else {
        1
    };
    core::resolve_line_range(start_line, end_line, cursor_row, line_count)
        .map_err(text_error_to_nvim)
}

fn line_index_to_i64(line: usize) -> Result<i64> {
    i64::try_from(line)
        .map_err(|_| nvim_oxi::api::Error::Other("line index overflow".into()).into())
}

fn is_visual_mode() -> bool {
    let mode = api::get_mode();
    matches!(mode.mode.as_bytes().first(), Some(b'v' | b'V' | b'\x16'))
}

fn visual_line_range(buf: &Buffer) -> Result<Option<(i64, i64)>> {
    if !is_visual_mode() {
        return Ok(None);
    }
    let (start_line, _) = buf.get_mark('<')?;
    let (end_line, _) = buf.get_mark('>')?;
    if start_line == 0 || end_line == 0 {
        return Ok(None);
    }
    Ok(Some((
        line_index_to_i64(start_line)?,
        line_index_to_i64(end_line)?,
    )))
}

fn resolve_target_range(buf: &Buffer) -> Result<LineRange> {
    let (start_line, end_line) = match visual_line_range(buf)? {
        Some((start_line, end_line)) => (Some(start_line), Some(end_line)),
        None => (None, None),
    };
    resolve_line_range(buf, start_line, end_line)
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

fn load_target_lines() -> Result<(Buffer, LineRange, Vec<String>)> {
    let buf = api::get_current_buf();
    let range = resolve_target_range(&buf)?;
    let lines = fetch_lines(&buf, range)?;
    Ok((buf, range, lines))
}

fn sort_lines() -> Result<()> {
    let (mut buf, range, lines) = load_target_lines()?;
    let sorted = core::sort_lines(&lines, SortDirection::Asc);
    replace_lines(&mut buf, range, sorted)
}

fn sort_lines_reverse() -> Result<()> {
    let (mut buf, range, lines) = load_target_lines()?;
    let sorted = core::sort_lines(&lines, SortDirection::Desc);
    replace_lines(&mut buf, range, sorted)
}

fn sort_lines_by_column() -> Result<()> {
    let (mut buf, range, lines) = load_target_lines()?;
    let column = Column(current_cursor_col()?);
    let sorted = core::sort_lines_by_column(&lines, column, SortDirection::Asc);
    replace_lines(&mut buf, range, sorted)
}

fn sort_lines_by_column_reverse() -> Result<()> {
    let (mut buf, range, lines) = load_target_lines()?;
    let column = Column(current_cursor_col()?);
    let sorted = core::sort_lines_by_column(&lines, column, SortDirection::Desc);
    replace_lines(&mut buf, range, sorted)
}

fn randomize_lines() -> Result<()> {
    let (mut buf, range, lines) = load_target_lines()?;
    let shuffled = core::randomize_lines(&lines, seed_from_time());
    replace_lines(&mut buf, range, shuffled)
}

fn uniquify_lines() -> Result<()> {
    let (mut buf, range, lines) = load_target_lines()?;
    let uniq = core::uniquify_lines(&lines);
    replace_lines(&mut buf, range, uniq)
}

fn duplicate_line_or_region() -> Result<()> {
    let (mut buf, range, lines) = load_target_lines()?;
    insert_lines_after(&mut buf, range, lines)
}

fn kill_back_to_indentation() -> Result<()> {
    let (mut buf, range, lines) = load_target_lines()?;
    let column = Column(current_cursor_col()?);
    let killed = core::kill_back_to_indentation(&lines, column);
    replace_lines(&mut buf, range, killed)
}

#[nvim_oxi::plugin]
fn rs_text() -> Dictionary {
    let mut api = Dictionary::new();
    api.insert("sort_lines", Function::<(), ()>::from_fn(|()| sort_lines()));
    api.insert(
        "sort_lines_reverse",
        Function::<(), ()>::from_fn(|()| sort_lines_reverse()),
    );
    api.insert(
        "sort_lines_by_column",
        Function::<(), ()>::from_fn(|()| sort_lines_by_column()),
    );
    api.insert(
        "sort_lines_by_column_reverse",
        Function::<(), ()>::from_fn(|()| sort_lines_by_column_reverse()),
    );
    api.insert(
        "randomize_lines",
        Function::<(), ()>::from_fn(|()| randomize_lines()),
    );
    api.insert(
        "uniquify_lines",
        Function::<(), ()>::from_fn(|()| uniquify_lines()),
    );
    api.insert(
        "duplicate_line_or_region",
        Function::<(), ()>::from_fn(|()| duplicate_line_or_region()),
    );
    api.insert(
        "kill_back_to_indentation",
        Function::<(), ()>::from_fn(|()| kill_back_to_indentation()),
    );
    api
}
