use std::{collections::HashMap, path::Path};

use nvim_oxi::api;
use nvim_oxi::api::opts::{OptionOpts, OptionScope};
use nvim_oxi::api::{Buffer, Window};
use nvim_oxi::{Array, Dictionary, Function, Object, Result, String as NvimString};
use nvim_oxi_utils::{handles, notify};
use nvim_utils::path::{path_is_dir, strip_known_prefixes};

type OptMap = HashMap<String, Object>;
const LOG_CONTEXT: &str = "util";

fn buffer_from_handle(handle: Option<i64>) -> Option<Buffer> {
    handles::buffer_from_optional(handle)
}

fn valid_buffer(handle: Option<i64>) -> Option<Buffer> {
    handles::valid_buffer_optional(handle)
}

fn valid_window(handle: Option<i64>) -> Option<Window> {
    handles::valid_window_optional(handle)
}

fn set_option_values(opts: OptMap, opt_opts: &OptionOpts) -> Result<()> {
    for (name, value) in opts {
        api::set_option_value(&name, value, opt_opts)?;
    }
    Ok(())
}

fn get_option_value(opt: &str, opt_opts: &OptionOpts, default: Object) -> Object {
    match api::get_option_value::<Object>(opt, opt_opts) {
        Ok(value) => value,
        Err(err) => {
            notify::warn(
                LOG_CONTEXT,
                &format!("get_option_value failed for {opt}: {err}"),
            );
            default
        }
    }
}

fn non_nil(value: Object) -> Option<Object> {
    if value.is_nil() { None } else { Some(value) }
}

fn is_dir(path: Option<String>) -> bool {
    let Some(path) = path else {
        return false;
    };
    let path = strip_known_prefixes(path.as_str());
    if path.is_empty() {
        return false;
    }
    path_is_dir(Path::new(path))
}

fn set_buf_opts((buf, opts): (Option<i64>, OptMap)) -> Result<()> {
    let Some(buf) = valid_buffer(buf) else {
        return Ok(());
    };
    let opt_opts = OptionOpts::builder().buf(buf).build();
    set_option_values(opts, &opt_opts)
}

fn set_win_opts((win, opts): (Option<i64>, OptMap)) -> Result<()> {
    let Some(win) = valid_window(win) else {
        return Ok(());
    };
    let opt_opts = OptionOpts::builder()
        .scope(OptionScope::Local)
        .win(win)
        .build();
    set_option_values(opts, &opt_opts)
}

fn get_buf_opt((buf, opt, default): (Option<i64>, String, Object)) -> Result<Object> {
    let Some(buf) = valid_buffer(buf) else {
        return Ok(default);
    };
    let opt_opts = OptionOpts::builder().buf(buf).build();
    Ok(get_option_value(&opt, &opt_opts, default))
}

fn get_win_opt((win, opt, default): (Option<i64>, String, Object)) -> Result<Object> {
    let Some(win) = valid_window(win) else {
        return Ok(default);
    };
    let opt_opts = OptionOpts::builder().win(win).build();
    Ok(get_option_value(&opt, &opt_opts, default))
}

fn get_var((buf, name, default): (Option<i64>, String, Object)) -> Result<Object> {
    let buf = match buf {
        Some(_) => buffer_from_handle(buf),
        None => Some(api::get_current_buf()),
    };
    if let Some(value) = buf
        .filter(|buf| buf.is_valid())
        .and_then(|buf| buf.get_var::<Object>(&name).ok())
        .and_then(non_nil)
    {
        return Ok(value);
    }
    if let Some(value) = api::get_var::<Object>(&name).ok().and_then(non_nil) {
        return Ok(value);
    }
    Ok(default)
}

fn edit_path(path: Option<String>) -> Result<()> {
    let Some(path) = path else {
        return Ok(());
    };
    if path.is_empty() {
        return Ok(());
    }
    let escaped: NvimString = api::call_function("fnameescape", Array::from_iter([path.as_str()]))?;
    let cmd = format!("edit {}", escaped.to_string_lossy());
    api::command(&cmd)?;
    Ok(())
}

fn next_window(wins: &[Window], cur: &Window) -> Option<Window> {
    let count = wins.len();
    if count <= 1 {
        return None;
    }
    for (idx, win) in wins.iter().enumerate() {
        if win == cur {
            return Some(wins[(idx + 1) % count].clone());
        }
    }
    wins.first().cloned()
}

fn other_window() -> Result<Option<Window>> {
    let tab = api::get_current_tabpage();
    let wins: Vec<Window> = tab.list_wins()?.collect();
    let cur = api::get_current_win();
    Ok(next_window(&wins, &cur))
}

fn get_or_create_other_window() -> Result<(Window, bool)> {
    let tab = api::get_current_tabpage();
    let wins: Vec<Window> = tab.list_wins()?.collect();
    let cur = api::get_current_win();
    if let Some(win) = next_window(&wins, &cur) {
        if win.is_valid() {
            return Ok((win, false));
        }
    }
    api::command("vsplit")?;
    let new_win = api::get_current_win();
    if cur.is_valid() {
        api::set_current_win(&cur)?;
    }
    Ok((new_win, true))
}

#[nvim_oxi::plugin]
fn my_util() -> Result<Dictionary> {
    let mut api = Dictionary::new();
    api.insert("is_dir", Function::<_, bool>::from_fn(is_dir));
    api.insert("set_buf_opts", Function::<_, ()>::from_fn(set_buf_opts));
    api.insert("set_win_opts", Function::<_, ()>::from_fn(set_win_opts));
    api.insert("get_buf_opt", Function::<_, Object>::from_fn(get_buf_opt));
    api.insert("get_win_opt", Function::<_, Object>::from_fn(get_win_opt));
    api.insert("get_var", Function::<_, Object>::from_fn(get_var));
    api.insert("edit_path", Function::<_, ()>::from_fn(edit_path));
    api.insert(
        "other_window",
        Function::<(), Option<Window>>::from_fn(|()| other_window()),
    );
    api.insert(
        "get_or_create_other_window",
        Function::<(), (Window, bool)>::from_fn(|()| get_or_create_other_window()),
    );
    Ok(api)
}
