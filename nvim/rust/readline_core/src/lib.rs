#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertAction {
    BeginningOfLine,
    EndOfLine,
    ForwardWord,
    BackwardWord,
    KillWord,
}

impl InsertAction {
    pub const fn key_sequence(self) -> &'static str {
        match self {
            Self::BeginningOfLine => "<C-o>0",
            Self::EndOfLine => "<C-o>$",
            Self::ForwardWord => "<C-o>w",
            Self::BackwardWord => "<C-o>b",
            Self::KillWord => "<C-o>dw",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransposeResult {
    pub new_line: String,
    pub new_col: usize,
}

pub fn transpose_chars(line: &str, cursor_col: usize) -> Option<TransposeResult> {
    let mut chars: Vec<char> = line.chars().collect();
    let char_count = chars.len();

    if char_count < 2 || cursor_col == 0 {
        return None;
    }

    let byte_len = line.len();
    if cursor_col >= byte_len {
        chars.swap(char_count - 2, char_count - 1);
        let new_line: String = chars.into_iter().collect();
        return Some(TransposeResult {
            new_line,
            new_col: byte_len,
        });
    }

    let mut char_index = None;
    for (idx, (byte_idx, _)) in line.char_indices().enumerate() {
        if byte_idx > cursor_col {
            break;
        }
        char_index = Some(idx);
    }

    let char_index = char_index?;
    if char_index == 0 || char_index >= chars.len() {
        return None;
    }

    chars.swap(char_index - 1, char_index);
    let new_col: usize = chars
        .iter()
        .take(char_index + 1)
        .map(|ch| ch.len_utf8())
        .sum();
    let new_line: String = chars.into_iter().collect();
    Some(TransposeResult { new_line, new_col })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transpose_ascii_middle() -> Result<(), &'static str> {
        let result = transpose_chars("abcd", 2).ok_or("expected transpose")?;
        assert_eq!(result.new_line, "acbd");
        assert_eq!(result.new_col, 3);
        Ok(())
    }

    #[test]
    fn transpose_end_swaps_last_two() -> Result<(), &'static str> {
        let result = transpose_chars("ab", 2).ok_or("expected transpose")?;
        assert_eq!(result.new_line, "ba");
        assert_eq!(result.new_col, 2);
        Ok(())
    }

    #[test]
    fn transpose_unicode_preserves_boundaries() -> Result<(), &'static str> {
        let result = transpose_chars("a💡b", 5).ok_or("expected transpose")?;
        assert_eq!(result.new_line, "ab💡");
        assert_eq!(result.new_col, "ab💡".len());
        Ok(())
    }
}
