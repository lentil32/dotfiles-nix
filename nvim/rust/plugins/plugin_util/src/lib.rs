mod definition_flow;

use std::{collections::HashMap, path::Path};

use definition_flow::{
    DefinitionAction, DefinitionItem, parse_definition_items, parse_definition_title,
    plan_definition_actions,
};
use nvim_oxi::api;
use nvim_oxi::api::opts::{ClearAutocmdsOpts, OptionOpts, OptionScope};
use nvim_oxi::api::{Buffer, Window};
use nvim_oxi::{Array, Dictionary, Function, Object, Result, String as NvimString, mlua};
use nvim_oxi_utils::{handles, lua, notify};
use nvim_utils::path::{path_is_dir, split_uri_scheme_and_rest, strip_known_prefixes};
use support::cycle::next_item;

type OptMap = HashMap<String, Object>;
const LOG_CONTEXT: &str = "rs_plugin_util";

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

fn is_dir_path(path: &str) -> bool {
    let path = strip_known_prefixes(path);
    if path.is_empty() {
        return false;
    }
    path_is_dir(Path::new(path))
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "nvim callback signatures pass owned Lua values by value"
)]
fn is_dir(path: Option<String>) -> bool {
    path.as_deref().is_some_and(is_dir_path)
}

fn normalize_oil_target(path: Option<String>) -> Option<String> {
    let path = path?.trim().to_string();
    if path.is_empty() {
        return None;
    }

    let stripped = strip_known_prefixes(path.as_str()).trim();
    if stripped.is_empty() {
        return None;
    }
    Some(stripped.to_string())
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

fn get_buf_opt((buf, opt, default): (Option<i64>, String, Object)) -> Object {
    let Some(buf) = valid_buffer(buf) else {
        return default;
    };
    let opt_opts = OptionOpts::builder().buf(buf).build();
    get_option_value(&opt, &opt_opts, default)
}

fn get_win_opt((win, opt, default): (Option<i64>, String, Object)) -> Object {
    let Some(win) = valid_window(win) else {
        return default;
    };
    let opt_opts = OptionOpts::builder().win(win).build();
    get_option_value(&opt, &opt_opts, default)
}

fn get_var((buf, name, default): (Option<i64>, String, Object)) -> Object {
    let buf = match buf {
        Some(_) => buffer_from_handle(buf),
        None => Some(api::get_current_buf()),
    };
    if let Some(value) = buf
        .filter(Buffer::is_valid)
        .and_then(|buf| buf.get_var::<Object>(&name).ok())
        .and_then(non_nil)
    {
        return value;
    }
    if let Some(value) = api::get_var::<Object>(&name).ok().and_then(non_nil) {
        return value;
    }
    default
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

fn parse_url_scheme_and_rest(url: &str) -> Option<(String, String)> {
    let (scheme_name, rest) = split_uri_scheme_and_rest(url)?;
    Some((format!("{scheme_name}://"), rest.to_string()))
}

fn vim_notify_once_error(lua: &mlua::Lua, message: &str) {
    let Ok(vim) = lua.globals().get::<mlua::Table>("vim") else {
        return;
    };
    let Ok(notify_once) = vim.get::<mlua::Function>("notify_once") else {
        return;
    };
    let level = vim
        .get::<mlua::Table>("log")
        .ok()
        .and_then(|log| log.get::<mlua::Table>("levels").ok())
        .and_then(|levels| levels.get::<mlua::Value>("ERROR").ok())
        .unwrap_or(mlua::Value::Integer(4));
    if let Err(err) = notify_once.call::<()>((message, level)) {
        notify::warn(LOG_CONTEXT, &format!("notify_once failed: {err}"));
    }
}

fn oil_get_adapter(lua: &mlua::Lua, bufnr: i64, silent: bool) -> mlua::Result<mlua::Value> {
    let Some(oil_util) = lua::try_require_table(lua, "oil.util") else {
        return Ok(mlua::Value::Nil);
    };
    let Some(config) = lua::try_require_table(lua, "oil.config") else {
        return Ok(mlua::Value::Nil);
    };
    let vim: mlua::Table = lua.globals().get("vim")?;
    let api_table: mlua::Table = vim.get("api")?;
    let nvim_buf_get_name: mlua::Function = api_table.get("nvim_buf_get_name")?;
    let bufname: String = nvim_buf_get_name.call(bufnr)?;
    let Some((mut scheme, _rest)) = parse_url_scheme_and_rest(&bufname) else {
        return Ok(mlua::Value::Nil);
    };

    let is_oil_bufnr: mlua::Function = match oil_util.get("is_oil_bufnr") {
        Ok(function) => function,
        Err(_) => return Ok(mlua::Value::Nil),
    };
    if !is_oil_bufnr.call::<bool>(bufnr)? {
        return Ok(mlua::Value::Nil);
    }

    if let Ok(adapter_aliases) = config.get::<mlua::Table>("adapter_aliases")
        && let Ok(alias) = adapter_aliases.get::<String>(scheme.as_str())
    {
        scheme = alias;
    }

    let get_adapter_by_scheme: mlua::Function = match config.get("get_adapter_by_scheme") {
        Ok(function) => function,
        Err(_) => return Ok(mlua::Value::Nil),
    };
    let adapter: mlua::Value = get_adapter_by_scheme.call(scheme)?;

    if matches!(adapter, mlua::Value::Nil) && !silent {
        vim_notify_once_error(
            lua,
            &format!("[oil] could not find adapter for buffer '{bufname}'"),
        );
    }

    Ok(adapter)
}

fn patch_oil_parse_url() -> Result<()> {
    let lua = lua::state();
    let Some(oil_util) = lua::try_require_table(&lua, "oil.util") else {
        return Ok(());
    };
    if oil_util.get::<bool>("_strict_parse_url").unwrap_or(false) {
        return Ok(());
    }

    oil_util
        .set("_strict_parse_url", true)
        .map_err(nvim_oxi::Error::from)?;

    let parse_url = lua
        .create_function(|_, url: String| match parse_url_scheme_and_rest(&url) {
            Some((scheme, rest)) => Ok((Some(scheme), Some(rest))),
            None => Ok((None::<String>, None::<String>)),
        })
        .map_err(nvim_oxi::Error::from)?;
    oil_util
        .set("parse_url", parse_url)
        .map_err(nvim_oxi::Error::from)?;

    if lua::try_require_table(&lua, "oil.config").is_none() {
        return Ok(());
    }

    let get_adapter = lua
        .create_function(|lua, (bufnr, silent): (i64, Option<bool>)| {
            oil_get_adapter(lua, bufnr, silent.unwrap_or(false))
        })
        .map_err(nvim_oxi::Error::from)?;
    oil_util
        .set("get_adapter", get_adapter)
        .map_err(nvim_oxi::Error::from)?;
    Ok(())
}

fn open_oil(path: Option<String>) -> Result<()> {
    let Some(path) = normalize_oil_target(path) else {
        return Ok(());
    };
    if is_dir_path(path.as_str()) {
        let lua = lua::state();
        if let Some(oil) = lua::try_require_table(&lua, "oil")
            && let Ok(open) = oil.get::<mlua::Function>("open")
        {
            match open.call::<()>(path.clone()) {
                Ok(()) => return Ok(()),
                Err(err) => notify::warn(LOG_CONTEXT, &format!("oil.open failed: {err}")),
            }
        }
    }
    edit_path(Some(path))
}

fn snacks_table(lua: &mlua::Lua) -> Option<mlua::Table> {
    lua::try_require_table(lua, "snacks")
}

fn rs_autocmds_table(lua: &mlua::Lua) -> Option<mlua::Table> {
    lua::try_require_table(lua, "rs_autocmds")
}

fn autocmds_oil_last_buf_for_win(win: Option<i64>) -> Option<i64> {
    let lua = lua::state();
    let autocmds = rs_autocmds_table(&lua)?;
    let Ok(oil_last_buf_for_win) = autocmds.get::<mlua::Function>("oil_last_buf_for_win") else {
        return None;
    };
    match oil_last_buf_for_win.call::<Option<i64>>(win) {
        Ok(value) => value,
        Err(err) => {
            notify::warn(
                LOG_CONTEXT,
                &format!("oil_last_buf_for_win call failed: {err}"),
            );
            None
        }
    }
}

fn delete_buffer_via_command(buf_handle: i64, force: bool, wipe: bool) -> Result<()> {
    if buf_handle <= 0 {
        return Ok(());
    }
    let cmd = match (wipe, force) {
        (true, true) => format!("bwipeout! {buf_handle}"),
        (true, false) => format!("bwipeout {buf_handle}"),
        (false, true) => format!("bdelete! {buf_handle}"),
        (false, false) => format!("bdelete {buf_handle}"),
    };
    api::command(&cmd)?;
    Ok(())
}

fn snacks_bufdelete_delete(buf_handle: i64, force: bool, wipe: bool) -> Result<()> {
    let lua = lua::state();
    let Some(snacks) = snacks_table(&lua) else {
        return delete_buffer_via_command(buf_handle, force, wipe);
    };
    let Ok(bufdelete) = snacks.get::<mlua::Table>("bufdelete") else {
        return delete_buffer_via_command(buf_handle, force, wipe);
    };
    let Ok(delete) = bufdelete.get::<mlua::Function>("delete") else {
        return delete_buffer_via_command(buf_handle, force, wipe);
    };
    let opts = lua.create_table().map_err(nvim_oxi::Error::from)?;
    opts.set("buf", buf_handle).map_err(nvim_oxi::Error::from)?;
    if force {
        opts.set("force", true).map_err(nvim_oxi::Error::from)?;
    }
    if wipe {
        opts.set("wipe", true).map_err(nvim_oxi::Error::from)?;
    }
    match delete.call::<()>(opts) {
        Ok(()) => Ok(()),
        Err(err) => {
            notify::warn(LOG_CONTEXT, &format!("snacks bufdelete failed: {err}"));
            delete_buffer_via_command(buf_handle, force, wipe)
        }
    }
}

fn oil_util_is_oil_buffer(bufnr: i64) -> bool {
    let lua = lua::state();
    let Some(oil_util) = lua::try_require_table(&lua, "oil.util") else {
        return false;
    };
    let Ok(is_oil_bufnr) = oil_util.get::<mlua::Function>("is_oil_bufnr") else {
        return false;
    };
    is_oil_bufnr.call::<bool>(bufnr).unwrap_or(false)
}

fn buf_has_snacks_terminal(buf: &Buffer) -> bool {
    buf.get_var::<Object>("snacks_terminal")
        .ok()
        .is_some_and(|value| !value.is_nil())
}

fn clear_termclose_autocmd(buf: &Buffer, term: &mlua::Table) {
    let group_id = term.get::<Option<i64>>("augroup").ok().flatten();
    let group_name = if group_id.is_none() {
        term.get::<Option<String>>("augroup").ok().flatten()
    } else {
        None
    };
    let opts = match (group_id, group_name) {
        (Some(group), _) => ClearAutocmdsOpts::builder()
            .events(["TermClose"])
            .buffer(buf.clone())
            .group(group)
            .build(),
        (None, Some(group)) if !group.is_empty() => ClearAutocmdsOpts::builder()
            .events(["TermClose"])
            .buffer(buf.clone())
            .group(group.as_str())
            .build(),
        _ => ClearAutocmdsOpts::builder()
            .events(["TermClose"])
            .buffer(buf.clone())
            .build(),
    };
    if let Err(err) = api::clear_autocmds(&opts) {
        notify::warn(
            LOG_CONTEXT,
            &format!("clear term close autocmd failed: {err}"),
        );
    }
}

fn close_matching_snacks_terminal(buf: &Buffer) -> bool {
    if !buf_has_snacks_terminal(buf) {
        return false;
    }
    let lua = lua::state();
    let Some(snacks) = snacks_table(&lua) else {
        return false;
    };
    let Ok(terminal) = snacks.get::<mlua::Table>("terminal") else {
        return false;
    };
    let Ok(list) = terminal.get::<mlua::Function>("list") else {
        return false;
    };
    let terms = match list.call::<mlua::Table>(()) {
        Ok(value) => value,
        Err(err) => {
            notify::warn(LOG_CONTEXT, &format!("snacks terminal list failed: {err}"));
            return false;
        }
    };

    let buf_handle = i64::from(buf.handle());
    for term_entry in terms.sequence_values::<mlua::Table>() {
        let Ok(term) = term_entry else {
            continue;
        };
        let entry_buf = term.get::<Option<i64>>("buf").ok().flatten();
        if entry_buf != Some(buf_handle) {
            continue;
        }

        clear_termclose_autocmd(buf, &term);

        if let Some(win_id) = term.get::<Option<i64>>("win").ok().flatten()
            && let Some(win) = handles::valid_window(win_id)
            && let Err(err) = win.close(true)
        {
            notify::warn(LOG_CONTEXT, &format!("close terminal window failed: {err}"));
        }

        if let Err(err) = delete_buffer_via_command(buf_handle, true, true) {
            notify::warn(LOG_CONTEXT, &format!("terminal wipeout failed: {err}"));
        }
        return true;
    }

    false
}

fn buf_option_string(buf: &Buffer, name: &str) -> String {
    let opts = OptionOpts::builder().buf(buf.clone()).build();
    api::get_option_value::<NvimString>(name, &opts)
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn delete_current_buffer() -> Result<()> {
    let cur_buf = api::get_current_buf();
    let cur_buf_handle = i64::from(cur_buf.handle());
    let cur_win_handle = i64::from(api::get_current_win().handle());
    let oil_buf_handle = autocmds_oil_last_buf_for_win(Some(cur_win_handle));

    if let Some(oil_buf_handle) = oil_buf_handle
        && oil_buf_handle != cur_buf_handle
        && let Some(oil_buffer) = handles::valid_buffer(oil_buf_handle)
        && oil_util_is_oil_buffer(oil_buf_handle)
    {
        api::set_current_buf(&oil_buffer)?;
        snacks_bufdelete_delete(cur_buf_handle, false, false)?;
        return Ok(());
    }

    snacks_bufdelete_delete(cur_buf_handle, false, false)
}

fn kill_window_and_buffer() -> Result<()> {
    let buf = api::get_current_buf();
    let buf_handle = i64::from(buf.handle());

    if close_matching_snacks_terminal(&buf) {
        return Ok(());
    }

    if api::list_wins().count() > 1 {
        api::command("close")?;
    }

    let buftype = buf_option_string(&buf, "buftype");
    let filetype = buf_option_string(&buf, "filetype");
    let is_terminal = buftype == "terminal" || filetype == "snacks_terminal";
    snacks_bufdelete_delete(buf_handle, is_terminal, is_terminal)
}

#[cfg(test)]
mod tests {
    use super::normalize_oil_target;

    #[test]
    fn normalize_oil_target_rejects_empty_values() {
        assert_eq!(normalize_oil_target(None), None);
        assert_eq!(normalize_oil_target(Some(String::new())), None);
        assert_eq!(normalize_oil_target(Some("   ".to_string())), None);
        assert_eq!(normalize_oil_target(Some("oil://".to_string())), None);
        assert_eq!(normalize_oil_target(Some("file://".to_string())), None);
    }

    #[test]
    fn normalize_oil_target_strips_known_uri_prefixes() {
        assert_eq!(
            normalize_oil_target(Some("oil:///tmp/demo".to_string())),
            Some("/tmp/demo".to_string())
        );
        assert_eq!(
            normalize_oil_target(Some("file:///tmp/demo".to_string())),
            Some("/tmp/demo".to_string())
        );
    }

    #[test]
    fn normalize_oil_target_keeps_regular_paths() {
        assert_eq!(
            normalize_oil_target(Some(" ./src ".to_string())),
            Some("./src".to_string())
        );
    }
}

fn oil_winbar() -> String {
    let lua = lua::state();
    let Some(oil) = lua::try_require_table(&lua, "oil") else {
        return String::new();
    };

    let vim: mlua::Table = match lua.globals().get("vim") {
        Ok(vim) => vim,
        Err(_) => return String::new(),
    };
    let Some(winid) = statusline_winid(&vim) else {
        return String::new();
    };
    let Some(win) = handles::valid_window(winid) else {
        return String::new();
    };
    let bufnr = match win.get_buf() {
        Ok(buf) => i64::from(buf.handle()),
        Err(_) => return String::new(),
    };

    let get_current_dir: mlua::Function = match oil.get("get_current_dir") {
        Ok(function) => function,
        Err(_) => return String::new(),
    };
    let dir = match get_current_dir.call::<Option<String>>(bufnr) {
        Ok(Some(dir)) if !dir.is_empty() => dir,
        _ => return String::new(),
    };

    let fn_table: mlua::Table = match vim.get("fn") {
        Ok(table) => table,
        Err(_) => return String::new(),
    };
    let fnamemodify: mlua::Function = match fn_table.get("fnamemodify") {
        Ok(function) => function,
        Err(_) => return String::new(),
    };
    fnamemodify
        .call::<String>((dir, ":~"))
        .unwrap_or_else(|_| String::new())
}

fn statusline_winid(vim: &mlua::Table) -> Option<i64> {
    let from_scope = |scope: &str| {
        vim.get::<mlua::Table>(scope)
            .ok()
            .and_then(|table| table.get::<Option<i64>>("statusline_winid").ok().flatten())
            .filter(|id| *id > 0)
    };
    from_scope("g").or_else(|| from_scope("v"))
}

fn vim_joinpath(lua: &mlua::Lua, lhs: &str, rhs: &str) -> Option<String> {
    let vim: mlua::Table = lua.globals().get("vim").ok()?;
    let fs: mlua::Table = vim.get("fs").ok()?;
    let joinpath: mlua::Function = fs.get("joinpath").ok()?;
    match joinpath.call((lhs, rhs)) {
        Ok(path) => Some(path),
        Err(err) => {
            notify::warn(LOG_CONTEXT, &format!("vim.fs.joinpath failed: {err}"));
            None
        }
    }
}

fn oil_select_other_window() -> Result<()> {
    let lua = lua::state();
    let Some(oil) = lua::try_require_table(&lua, "oil") else {
        return Ok(());
    };

    let get_cursor_entry: mlua::Function = match oil.get("get_cursor_entry") {
        Ok(function) => function,
        Err(_) => return Ok(()),
    };
    let Some(entry) = get_cursor_entry.call::<Option<mlua::Table>>(())? else {
        return Ok(());
    };
    let Some(name) = entry.get::<Option<String>>("name").ok().flatten() else {
        return Ok(());
    };
    if name.is_empty() {
        return Ok(());
    }

    let get_current_dir: mlua::Function = match oil.get("get_current_dir") {
        Ok(function) => function,
        Err(_) => return Ok(()),
    };
    let Some(dir) = get_current_dir.call::<Option<String>>(())? else {
        return Ok(());
    };
    if dir.is_empty() {
        return Ok(());
    }

    let path = vim_joinpath(&lua, &dir, &name).unwrap_or_else(|| format!("{dir}/{name}"));
    let (target, _) = get_or_create_other_window()?;
    if target.is_valid() {
        api::set_current_win(&target)?;
    }
    open_oil(Some(path))
}

fn next_window(wins: &[Window], cur: &Window) -> Option<Window> {
    if wins.len() <= 1 {
        return None;
    }
    next_item(wins, cur)
        .cloned()
        .or_else(|| wins.first().cloned())
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
    if let Some(win) = next_window(&wins, &cur)
        && win.is_valid()
    {
        return Ok((win, false));
    }
    api::command("vsplit")?;
    let new_win = api::get_current_win();
    if cur.is_valid() {
        api::set_current_win(&cur)?;
    }
    Ok((new_win, true))
}

fn restore_current_window(cur_handle: i64) {
    if let Some(cur) = handles::valid_window(cur_handle)
        && let Err(err) = api::set_current_win(&cur)
    {
        notify::warn(
            LOG_CONTEXT,
            &format!("restore current window failed: {err}"),
        );
    }
}

fn close_created_target_window(target_handle: i64, created: bool) {
    if !created {
        return;
    }
    if let Some(target) = handles::valid_window(target_handle)
        && let Err(err) = target.close(true)
    {
        notify::warn(LOG_CONTEXT, &format!("close created window failed: {err}"));
    }
}

fn set_cursor_safe(target: &mut Window, lnum: i64, col: i64) {
    let Ok(row) = usize::try_from(lnum) else {
        notify::warn(LOG_CONTEXT, "set cursor failed: invalid line number");
        return;
    };
    let col_zero_based = col.saturating_sub(1);
    let Ok(col) = usize::try_from(col_zero_based) else {
        notify::warn(LOG_CONTEXT, "set cursor failed: invalid column");
        return;
    };
    if let Err(err) = target.set_cursor(row, col) {
        notify::warn(LOG_CONTEXT, &format!("set cursor failed: {err}"));
    }
}

fn open_definition_item_in_window(target_handle: i64, item: &DefinitionItem) -> Result<()> {
    let Some(mut target) = handles::valid_window(target_handle) else {
        return Ok(());
    };
    api::set_current_win(&target)?;

    if let Some(bufnr) = item.bufnr.and_then(handles::valid_buffer) {
        api::set_current_buf(&bufnr)?;
    } else if let Some(filename) = item.filename.as_deref() {
        edit_path(Some(filename.to_string()))?;
    }

    set_cursor_safe(&mut target, item.lnum, item.col);
    Ok(())
}

fn push_definition_items_to_qflist(
    lua: &mlua::Lua,
    title: Option<&str>,
    items: &[DefinitionItem],
) -> Result<()> {
    let vim: mlua::Table = lua.globals().get("vim").map_err(nvim_oxi::Error::from)?;
    let fn_table: mlua::Table = vim.get("fn").map_err(nvim_oxi::Error::from)?;
    let setqflist: mlua::Function = fn_table.get("setqflist").map_err(nvim_oxi::Error::from)?;

    let payload = lua.create_table().map_err(nvim_oxi::Error::from)?;
    if let Some(title) = title {
        payload.set("title", title).map_err(nvim_oxi::Error::from)?;
    }
    let qflist_items = lua.create_table().map_err(nvim_oxi::Error::from)?;
    for (index, item) in items.iter().enumerate() {
        let entry = lua.create_table().map_err(nvim_oxi::Error::from)?;
        if let Some(bufnr) = item.bufnr {
            entry.set("bufnr", bufnr).map_err(nvim_oxi::Error::from)?;
        }
        if let Some(filename) = item.filename.as_deref() {
            entry
                .set("filename", filename)
                .map_err(nvim_oxi::Error::from)?;
        }
        entry
            .set("lnum", item.lnum)
            .map_err(nvim_oxi::Error::from)?;
        entry.set("col", item.col).map_err(nvim_oxi::Error::from)?;
        qflist_items
            .raw_set(index + 1, entry)
            .map_err(nvim_oxi::Error::from)?;
    }
    payload
        .set("items", qflist_items)
        .map_err(nvim_oxi::Error::from)?;
    let empty = lua.create_table().map_err(nvim_oxi::Error::from)?;
    setqflist
        .call::<()>((empty, " ", payload))
        .map_err(nvim_oxi::Error::from)?;
    Ok(())
}

fn execute_definition_actions(
    lua: &mlua::Lua,
    target_handle: i64,
    created: bool,
    actions: Vec<DefinitionAction>,
) -> Result<()> {
    for action in actions {
        match action {
            DefinitionAction::CloseCreatedTarget => {
                close_created_target_window(target_handle, created);
            }
            DefinitionAction::OpenPrimary(item) => {
                open_definition_item_in_window(target_handle, &item)?;
            }
            DefinitionAction::PushQuickfix { title, items } => {
                push_definition_items_to_qflist(lua, title.as_deref(), &items)?;
            }
        }
    }
    Ok(())
}

fn on_lsp_definition_list(
    lua: &mlua::Lua,
    opts: &mlua::Table,
    target_handle: i64,
    created: bool,
    cur_handle: i64,
) -> Result<()> {
    let result = (|| -> Result<()> {
        let items_table = match opts.get::<Option<mlua::Table>>("items") {
            Ok(items) => items,
            Err(err) => {
                notify::warn(
                    LOG_CONTEXT,
                    &format!("invalid LSP definition list payload: {err}"),
                );
                close_created_target_window(target_handle, created);
                return Ok(());
            }
        };
        let items = match items_table {
            Some(items) => match parse_definition_items(&items) {
                Ok(parsed) => parsed,
                Err(err) => {
                    notify::warn(
                        LOG_CONTEXT,
                        &format!("invalid LSP definition item; skipping jump: {err}"),
                    );
                    close_created_target_window(target_handle, created);
                    return Ok(());
                }
            },
            None => Vec::new(),
        };
        let actions = plan_definition_actions(items, parse_definition_title(opts));
        execute_definition_actions(lua, target_handle, created, actions)?;
        Ok(())
    })();

    restore_current_window(cur_handle);
    result
}

fn goto_definition_other_window() -> Result<()> {
    let (target, created) = get_or_create_other_window()?;
    let cur = api::get_current_win();
    let target_handle = i64::from(target.handle());
    let cur_handle = i64::from(cur.handle());

    let lua = lua::state();
    let vim: mlua::Table = lua.globals().get("vim").map_err(nvim_oxi::Error::from)?;
    let lsp: mlua::Table = vim.get("lsp").map_err(nvim_oxi::Error::from)?;
    let buf: mlua::Table = lsp.get("buf").map_err(nvim_oxi::Error::from)?;
    let definition: mlua::Function = buf.get("definition").map_err(nvim_oxi::Error::from)?;

    let on_list = lua
        .create_function(move |lua, opts: mlua::Table| {
            if let Err(err) = on_lsp_definition_list(lua, &opts, target_handle, created, cur_handle)
            {
                notify::warn(
                    LOG_CONTEXT,
                    &format!("lsp definition callback failed: {err}"),
                );
            }
            Ok(())
        })
        .map_err(nvim_oxi::Error::from)?;

    let opts = lua.create_table().map_err(nvim_oxi::Error::from)?;
    opts.set("on_list", on_list)
        .map_err(nvim_oxi::Error::from)?;

    match definition.call::<()>(opts) {
        Ok(()) => Ok(()),
        Err(err) => {
            close_created_target_window(target_handle, created);
            restore_current_window(cur_handle);
            Err(nvim_oxi::Error::from(err))
        }
    }
}

#[nvim_oxi::plugin]
fn rs_plugin_util() -> Dictionary {
    let mut api = Dictionary::new();
    api.insert("is_dir", Function::<_, bool>::from_fn(is_dir));
    api.insert("set_buf_opts", Function::<_, ()>::from_fn(set_buf_opts));
    api.insert("set_win_opts", Function::<_, ()>::from_fn(set_win_opts));
    api.insert("get_buf_opt", Function::<_, Object>::from_fn(get_buf_opt));
    api.insert("get_win_opt", Function::<_, Object>::from_fn(get_win_opt));
    api.insert("get_var", Function::<_, Object>::from_fn(get_var));
    api.insert("edit_path", Function::<_, ()>::from_fn(edit_path));
    api.insert(
        "patch_oil_parse_url",
        Function::<(), ()>::from_fn(|()| patch_oil_parse_url()),
    );
    api.insert(
        "open_oil",
        Function::<Option<String>, ()>::from_fn(open_oil),
    );
    api.insert(
        "oil_winbar",
        Function::<(), String>::from_fn(|()| oil_winbar()),
    );
    api.insert(
        "oil_select_other_window",
        Function::<(), ()>::from_fn(|()| oil_select_other_window()),
    );
    api.insert(
        "goto_definition_other_window",
        Function::<(), ()>::from_fn(|()| goto_definition_other_window()),
    );
    api.insert(
        "delete_current_buffer",
        Function::<(), ()>::from_fn(|()| delete_current_buffer()),
    );
    api.insert(
        "kill_window_and_buffer",
        Function::<(), ()>::from_fn(|()| kill_window_and_buffer()),
    );
    api.insert(
        "other_window",
        Function::<(), Option<Window>>::from_fn(|()| other_window()),
    );
    api.insert(
        "get_or_create_other_window",
        Function::<(), (Window, bool)>::from_fn(|()| get_or_create_other_window()),
    );
    api
}
