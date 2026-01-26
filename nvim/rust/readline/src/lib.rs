use nvim_oxi::api;
use nvim_oxi::{Dictionary, Function, Result, String as NvimString};

fn is_insert_mode() -> Result<bool> {
    let mode = api::get_mode()?;
    Ok(matches!(mode.mode.as_bytes().first(), Some(b'i')))
}

fn feedkeys(keys: &str) -> Result<()> {
    let keys = api::replace_termcodes(keys, true, false, true);
    let mode = NvimString::from("n");
    api::feedkeys(keys.as_nvim_str(), &mode, false);
    Ok(())
}

fn beginning_of_line() -> Result<()> {
    if is_insert_mode()? {
        feedkeys("<C-o>0")?;
    }
    Ok(())
}

fn end_of_line() -> Result<()> {
    if is_insert_mode()? {
        feedkeys("<C-o>$")?;
    }
    Ok(())
}

fn forward_word() -> Result<()> {
    if is_insert_mode()? {
        feedkeys("<C-o>w")?;
    }
    Ok(())
}

fn backward_word() -> Result<()> {
    if is_insert_mode()? {
        feedkeys("<C-o>b")?;
    }
    Ok(())
}

fn kill_word() -> Result<()> {
    if is_insert_mode()? {
        feedkeys("<C-o>dw")?;
    }
    Ok(())
}

fn transpose_chars() -> Result<()> {
    if !is_insert_mode()? {
        return Ok(());
    }
    let mut win = api::get_current_win();
    let (row, col) = win.get_cursor()?;
    let line = api::get_current_line()?;
    let mut chars: Vec<char> = line.chars().collect();
    let char_count = chars.len();

    if char_count < 2 || col == 0 {
        return Ok(());
    }

    let byte_len = line.len();
    if col >= byte_len {
        chars.swap(char_count - 2, char_count - 1);
        let new_line: String = chars.into_iter().collect();
        api::set_current_line(new_line)?;
        win.set_cursor(row, byte_len)?;
        return Ok(());
    }

    let mut char_index = None;
    for (idx, (byte_idx, _)) in line.char_indices().enumerate() {
        if byte_idx > col {
            break;
        }
        char_index = Some(idx);
    }

    let Some(char_index) = char_index else {
        return Ok(());
    };
    if char_index == 0 || char_index >= chars.len() {
        return Ok(());
    }

    chars.swap(char_index - 1, char_index);
    let new_col: usize = chars
        .iter()
        .take(char_index + 1)
        .map(|ch| ch.len_utf8())
        .sum();
    let new_line: String = chars.into_iter().collect();
    api::set_current_line(new_line)?;
    win.set_cursor(row, new_col)?;
    Ok(())
}

#[nvim_oxi::plugin]
fn my_readline() -> Result<Dictionary> {
    let mut api = Dictionary::new();
    api.insert(
        "beginning_of_line",
        Function::<(), ()>::from_fn(|()| beginning_of_line()),
    );
    api.insert(
        "end_of_line",
        Function::<(), ()>::from_fn(|()| end_of_line()),
    );
    api.insert(
        "forward_word",
        Function::<(), ()>::from_fn(|()| forward_word()),
    );
    api.insert(
        "backward_word",
        Function::<(), ()>::from_fn(|()| backward_word()),
    );
    api.insert("kill_word", Function::<(), ()>::from_fn(|()| kill_word()));
    api.insert(
        "transpose_chars",
        Function::<(), ()>::from_fn(|()| transpose_chars()),
    );
    Ok(api)
}
