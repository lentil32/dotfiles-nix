use nvim_oxi::api;
use nvim_oxi::{Dictionary, Function, Result, String as NvimString};
use readline_core::{InsertAction, transpose_chars};

fn is_insert_mode() -> bool {
    let mode = api::get_mode();
    matches!(mode.mode.as_bytes().first(), Some(b'i'))
}

fn feedkeys(keys: &str) {
    let keys = api::replace_termcodes(keys, true, false, true);
    let mode = NvimString::from("n");
    api::feedkeys(keys.as_nvim_str(), &mode, false);
}

fn beginning_of_line() {
    if is_insert_mode() {
        feedkeys(InsertAction::BeginningOfLine.key_sequence());
    }
}

fn end_of_line() {
    if is_insert_mode() {
        feedkeys(InsertAction::EndOfLine.key_sequence());
    }
}

fn forward_word() {
    if is_insert_mode() {
        feedkeys(InsertAction::ForwardWord.key_sequence());
    }
}

fn backward_word() {
    if is_insert_mode() {
        feedkeys(InsertAction::BackwardWord.key_sequence());
    }
}

fn kill_word() {
    if is_insert_mode() {
        feedkeys(InsertAction::KillWord.key_sequence());
    }
}

fn transpose_chars_action() -> Result<()> {
    if !is_insert_mode() {
        return Ok(());
    }
    let mut win = api::get_current_win();
    let (row, col) = win.get_cursor()?;
    let line = api::get_current_line()?;
    if let Some(result) = transpose_chars(&line, col) {
        api::set_current_line(result.new_line)?;
        win.set_cursor(row, result.new_col)?;
    }
    Ok(())
}

#[nvim_oxi::plugin]
fn my_readline() -> Dictionary {
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
        Function::<(), ()>::from_fn(|()| transpose_chars_action()),
    );
    api
}
