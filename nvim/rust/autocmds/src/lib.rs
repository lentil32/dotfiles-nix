use std::collections::HashMap;
use std::path::Path;

use nvim_oxi::api;
use nvim_oxi::api::opts::{CreateAugroupOpts, CreateAutocmdOpts, OptionOpts};
use nvim_oxi::api::types::AutocmdCallbackArgs;
use nvim_oxi::api::{Buffer, Window};
use nvim_oxi::conversion::FromObject;
use nvim_oxi::{Array, Dictionary, Function, Object, Result, String as NvimString, mlua, schedule};
use nvim_oxi_utils::{
    dict, guard,
    handles::{BufHandle, WinHandle},
    lua, notify,
};

use nvim_utils::path::{has_uri_scheme, path_is_dir, strip_known_prefixes};

type OilMap = HashMap<WinHandle, BufHandle>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AutocmdAction {
    Keep,
}

impl AutocmdAction {
    fn as_bool(self) -> bool {
        false
    }
}

const LOG_CONTEXT: &str = "autocmds";

fn report_panic(label: &str, info: guard::PanicInfo) {
    notify::error(LOG_CONTEXT, &format!("{label} panic: {}", info.render()));
}

fn run_autocmd<F>(label: &str, f: F) -> Result<bool>
where
    F: FnOnce() -> Result<AutocmdAction>,
{
    let result = guard::with_panic(Ok(AutocmdAction::Keep), f, |info| report_panic(label, info));
    match result {
        Ok(value) => Ok(value.as_bool()),
        Err(err) => {
            notify::warn(LOG_CONTEXT, &format!("{label} failed: {err}"));
            Ok(false)
        }
    }
}

fn run_scheduled<F>(label: &str, f: F)
where
    F: FnOnce() -> Result<()>,
{
    guard::with_panic(
        (),
        || {
            if let Err(err) = f() {
                notify::warn(LOG_CONTEXT, &format!("{label} failed: {err}"));
            }
        },
        |info| report_panic(label, info),
    );
}

fn snacks_table(lua: &mlua::Lua) -> Option<mlua::Table> {
    lua::try_require_table(lua, "snacks")
}

fn oil_table(lua: &mlua::Lua) -> Option<mlua::Table> {
    lua::try_require_table(lua, "oil")
}

fn snacks_dashboard() -> Result<()> {
    let lua = lua::state();
    let Some(snacks) = snacks_table(&lua) else {
        return Ok(());
    };
    let Ok(dashboard) = snacks.get::<mlua::Function>("dashboard") else {
        return Ok(());
    };
    dashboard.call::<()>(()).map_err(Into::into)
}

fn oil_current_dir_lua(buf: BufHandle) -> Result<Option<String>> {
    let lua = lua::state();
    let Some(oil) = oil_table(&lua) else {
        return Ok(None);
    };
    let Ok(get_current_dir) = oil.get::<mlua::Function>("get_current_dir") else {
        return Ok(None);
    };
    let dir: Option<String> = get_current_dir
        .call::<Option<String>>(buf.raw())
        .map_err(nvim_oxi::Error::from)?;
    Ok(dir.filter(|val| !val.is_empty()))
}

fn snacks_rename_file(src: &str, dest: &str) -> Result<()> {
    let lua = lua::state();
    let Some(snacks) = snacks_table(&lua) else {
        return Ok(());
    };
    let Ok(rename) = snacks.get::<mlua::Table>("rename") else {
        return Ok(());
    };
    let Ok(on_rename_file) = rename.get::<mlua::Function>("on_rename_file") else {
        return Ok(());
    };
    on_rename_file.call::<()>((src, dest)).map_err(Into::into)
}

fn is_dir(path: &str) -> bool {
    if path.is_empty() {
        return false;
    }
    let path = strip_known_prefixes(path);
    if path.is_empty() {
        return false;
    }
    path_is_dir(Path::new(path))
}

fn set_win_cwd(win: &Window, dir: &str) -> Result<()> {
    if !win.is_valid() {
        return Ok(());
    }
    if dir.is_empty() || !is_dir(dir) {
        return Ok(());
    }
    let dir = dir.to_string();
    let _: () = win.call(move |_| -> Result<()> {
        let escaped: NvimString =
            api::call_function("fnameescape", Array::from_iter([dir.as_str()]))?;
        let cmd = format!("lcd {}", escaped.to_string_lossy());
        api::command(&cmd)?;
        Ok(())
    })?;
    Ok(())
}

fn file_dir_for_buf(buf: &Buffer) -> Result<Option<String>> {
    if !buf.is_valid() {
        return Ok(None);
    }
    let bt: NvimString =
        api::get_option_value("buftype", &OptionOpts::builder().buf(buf.clone()).build())?;
    if !bt.is_empty() {
        return Ok(None);
    }
    let name = buf.get_name()?;
    if name.is_empty() {
        return Ok(None);
    }
    let name_str = name.to_string_lossy();
    if has_uri_scheme(name_str.as_ref()) {
        return Ok(None);
    }
    let dir: NvimString =
        api::call_function("fnamemodify", Array::from_iter([name_str.as_ref(), ":p:h"]))?;
    let dir = dir.to_string_lossy().into_owned();
    if dir.is_empty() {
        return Ok(None);
    }
    Ok(Some(dir))
}

fn win_for_buf(buf: &Buffer) -> Result<Option<Window>> {
    let win_id: i64 = api::call_function("bufwinid", Array::from_iter([buf.handle()]))?;
    if win_id == -1 {
        return Ok(None);
    }
    let Ok(handle) = i32::try_from(win_id) else {
        return Ok(None);
    };
    let win = Window::from(handle);
    if !win.is_valid() {
        return Ok(None);
    }
    Ok(Some(win))
}

fn maybe_show_dashboard() -> Result<()> {
    let current = api::get_current_buf();
    let bt: NvimString =
        api::get_option_value("buftype", &OptionOpts::builder().buf(current).build())?;
    if !bt.is_empty() {
        return Ok(());
    }

    for buf in api::list_bufs() {
        if !buf.is_valid() {
            continue;
        }
        let opt_opts = OptionOpts::builder().buf(buf.clone()).build();
        let listed = match api::get_option_value::<bool>("buflisted", &opt_opts) {
            Ok(value) => value,
            Err(err) => {
                notify::warn(LOG_CONTEXT, &format!("buflisted failed: {err}"));
                false
            }
        };
        if !listed {
            continue;
        }
        let name = buf.get_name()?;
        if !name.is_empty() {
            return Ok(());
        }
    }

    if let Err(err) = snacks_dashboard() {
        notify::warn(LOG_CONTEXT, &format!("snacks dashboard failed: {err}"));
    }
    Ok(())
}

fn oil_current_dir(buf: BufHandle) -> Result<Option<String>> {
    oil_current_dir_lua(buf)
}

fn win_handle_from_key(key: &str) -> Option<WinHandle> {
    key.parse::<i64>().ok().and_then(WinHandle::try_from_i64)
}

fn oil_last_buf_map() -> OilMap {
    // Missing var is expected; treat as an empty map.
    let Ok(obj) = api::get_var::<Object>("oil_last_buf") else {
        return HashMap::new();
    };
    let Ok(dict) = Dictionary::try_from(obj) else {
        return HashMap::new();
    };
    let mut map = HashMap::new();
    for (key, value) in dict.iter() {
        let Some(win) = win_handle_from_key(&key.to_string_lossy()) else {
            continue;
        };
        let Ok(buf) = i64::from_object(value.clone()) else {
            continue;
        };
        let Some(buf) = BufHandle::try_from_i64(buf) else {
            continue;
        };
        map.insert(win, buf);
    }
    map
}

fn map_to_dict(map: &OilMap) -> Dictionary {
    let mut dict = Dictionary::new();
    for (win, buf) in map {
        dict.insert(win.raw().to_string(), buf.raw());
    }
    dict
}

fn write_oil_last_buf(map: &OilMap) -> Result<()> {
    api::set_var("oil_last_buf", map_to_dict(map))?;
    Ok(())
}

fn clean_oil_last_buf() -> Result<()> {
    let mut map = oil_last_buf_map();
    let mut changed = false;
    map.retain(|win, buf| {
        let win_ok = win.valid_window().is_some();
        let buf_ok = buf.valid_buffer().is_some();
        let keep = win_ok && buf_ok;
        if !keep {
            changed = true;
        }
        keep
    });
    if changed {
        write_oil_last_buf(&map)?;
    }
    Ok(())
}

fn on_dashboard_delete() -> Result<AutocmdAction> {
    schedule(|()| run_scheduled("dashboard", maybe_show_dashboard));
    Ok(AutocmdAction::Keep)
}

fn on_file_cwd(args: AutocmdCallbackArgs) -> Result<AutocmdAction> {
    let Some(dir) = file_dir_for_buf(&args.buffer)? else {
        return Ok(AutocmdAction::Keep);
    };
    let Some(win) = win_for_buf(&args.buffer)? else {
        return Ok(AutocmdAction::Keep);
    };
    set_win_cwd(&win, &dir)?;
    Ok(AutocmdAction::Keep)
}

fn on_oil_buf(args: AutocmdCallbackArgs) -> Result<AutocmdAction> {
    let buf_handle = BufHandle::from_buffer(&args.buffer);
    let Some(dir) = oil_current_dir(buf_handle)? else {
        return Ok(AutocmdAction::Keep);
    };
    let Some(win) = win_for_buf(&args.buffer)? else {
        return Ok(AutocmdAction::Keep);
    };
    let win_handle = WinHandle::from_window(&win);
    let mut map = oil_last_buf_map();
    map.insert(win_handle, buf_handle);
    write_oil_last_buf(&map)?;
    set_win_cwd(&win, &dir)?;
    Ok(AutocmdAction::Keep)
}

fn on_win_closed(args: AutocmdCallbackArgs) -> Result<AutocmdAction> {
    let Ok(win_id) = args.r#match.parse::<i64>() else {
        return Ok(AutocmdAction::Keep);
    };
    let Some(win_handle) = WinHandle::try_from_i64(win_id) else {
        return Ok(AutocmdAction::Keep);
    };
    let mut map = oil_last_buf_map();
    if map.remove(&win_handle).is_some() {
        write_oil_last_buf(&map)?;
    }
    Ok(AutocmdAction::Keep)
}

fn on_buf_wipeout(args: AutocmdCallbackArgs) -> Result<AutocmdAction> {
    let buf_handle = BufHandle::from_buffer(&args.buffer);
    let mut map = oil_last_buf_map();
    let mut changed = false;
    map.retain(|_, mapped| {
        let keep = *mapped != buf_handle;
        if !keep {
            changed = true;
        }
        keep
    });
    if changed {
        write_oil_last_buf(&map)?;
    }
    Ok(AutocmdAction::Keep)
}

fn on_oil_actions_post(args: AutocmdCallbackArgs) -> Result<AutocmdAction> {
    let Ok(dict) = Dictionary::try_from(args.data) else {
        return Ok(AutocmdAction::Keep);
    };
    let actions_key = NvimString::from("actions");
    let Some(actions_obj) = dict.get(&actions_key) else {
        return Ok(AutocmdAction::Keep);
    };
    let Ok(actions) = Vec::<Dictionary>::from_object(actions_obj.clone()) else {
        return Ok(AutocmdAction::Keep);
    };
    let Some(first) = actions.into_iter().next() else {
        return Ok(AutocmdAction::Keep);
    };
    let action_type = dict::get_string(&first, "type");
    if action_type.as_deref() != Some("move") {
        return Ok(AutocmdAction::Keep);
    }
    let Some(src) = dict::get_string(&first, "src_url") else {
        return Ok(AutocmdAction::Keep);
    };
    let Some(dest) = dict::get_string(&first, "dest_url") else {
        return Ok(AutocmdAction::Keep);
    };

    snacks_rename_file(&src, &dest)?;
    Ok(AutocmdAction::Keep)
}

fn setup_dashboard_autocmd() -> Result<()> {
    let group = api::create_augroup(
        "UserDashboard",
        &CreateAugroupOpts::builder().clear(true).build(),
    )?;
    let opts = CreateAutocmdOpts::builder()
        .group(group)
        .callback(|_args: AutocmdCallbackArgs| {
            run_autocmd("on_dashboard_delete", on_dashboard_delete)
        })
        .build();
    api::create_autocmd(["BufDelete"], &opts)?;
    Ok(())
}

fn setup_file_cwd_autocmd() -> Result<()> {
    let group = api::create_augroup(
        "UserFileCwd",
        &CreateAugroupOpts::builder().clear(true).build(),
    )?;
    let opts = CreateAutocmdOpts::builder()
        .group(group)
        .callback(|args: AutocmdCallbackArgs| run_autocmd("on_file_cwd", || on_file_cwd(args)))
        .build();
    api::create_autocmd(["BufEnter"], &opts)?;
    Ok(())
}

fn setup_oil_cwd_autocmd() -> Result<()> {
    let group = api::create_augroup(
        "UserOilCwd",
        &CreateAugroupOpts::builder().clear(true).build(),
    )?;
    let opts = CreateAutocmdOpts::builder()
        .group(group)
        .patterns(["oil://*"])
        .callback(|args: AutocmdCallbackArgs| run_autocmd("on_oil_buf", || on_oil_buf(args)))
        .build();
    api::create_autocmd(["BufEnter", "BufReadCmd"], &opts)?;
    Ok(())
}

fn setup_oil_last_buf_autocmds() -> Result<()> {
    let group = api::create_augroup(
        "UserOilLastBuf",
        &CreateAugroupOpts::builder().clear(true).build(),
    )?;

    let win_closed_opts = CreateAutocmdOpts::builder()
        .group(group)
        .callback(|args: AutocmdCallbackArgs| run_autocmd("on_win_closed", || on_win_closed(args)))
        .build();
    api::create_autocmd(["WinClosed"], &win_closed_opts)?;

    let wipeout_opts = CreateAutocmdOpts::builder()
        .group(group)
        .callback(|args: AutocmdCallbackArgs| {
            run_autocmd("on_buf_wipeout", || on_buf_wipeout(args))
        })
        .build();
    api::create_autocmd(["BufWipeout"], &wipeout_opts)?;

    let resized_opts = CreateAutocmdOpts::builder()
        .group(group)
        .callback(|_args: AutocmdCallbackArgs| {
            run_autocmd("clean_oil_last_buf", || {
                clean_oil_last_buf()?;
                Ok(AutocmdAction::Keep)
            })
        })
        .build();
    api::create_autocmd(["VimResized"], &resized_opts)?;

    Ok(())
}

fn setup_oil_rename_autocmd() -> Result<()> {
    let group = api::create_augroup(
        "UserOilRename",
        &CreateAugroupOpts::builder().clear(true).build(),
    )?;
    let opts = CreateAutocmdOpts::builder()
        .group(group)
        .patterns(["OilActionsPost"])
        .callback(|args: AutocmdCallbackArgs| {
            run_autocmd("on_oil_actions_post", || on_oil_actions_post(args))
        })
        .build();
    api::create_autocmd(["User"], &opts)?;
    Ok(())
}

fn setup() -> Result<()> {
    setup_dashboard_autocmd()?;
    setup_file_cwd_autocmd()?;
    setup_oil_cwd_autocmd()?;
    setup_oil_last_buf_autocmds()?;
    setup_oil_rename_autocmd()?;
    Ok(())
}

#[nvim_oxi::plugin]
fn my_autocmds() -> Result<Dictionary> {
    let mut api = Dictionary::new();
    api.insert("setup", Function::<(), ()>::from_fn(|()| setup()));
    Ok(api)
}
