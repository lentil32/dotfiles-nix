use std::collections::HashMap;
use std::path::Path;

use nvim_oxi::api;
use nvim_oxi::api::opts::{CreateAugroupOpts, CreateAutocmdOpts, OptionOpts};
use nvim_oxi::api::types::AutocmdCallbackArgs;
use nvim_oxi::api::{Buffer, Window};
use nvim_oxi::conversion::FromObject;
use nvim_oxi::{Array, Dictionary, Function, Object, Result, String as NvimString, schedule};

use nvim_utils::path::{has_uri_scheme, path_is_dir, strip_known_prefixes};

type OilMap = HashMap<String, i64>;

type ShouldDeleteAutocmd = bool;

const SNACKS_DASHBOARD_LUA: &str = "(function() local ok, snacks = pcall(require, 'snacks'); if ok and snacks.dashboard then snacks.dashboard() end end)()";
const OIL_DIR_LUA: &str = "(function(buf) local ok, oil = pcall(require, 'oil'); if not ok then return nil end; return oil.get_current_dir(buf) end)(_A)";
const SNACKS_RENAME_LUA: &str = "(function(args) local ok, snacks = pcall(require, 'snacks'); if not ok then return end; local rename = snacks.rename and snacks.rename.on_rename_file; if rename then rename(args.src_url, args.dest_url) end end)(_A)";

fn lua_eval<T>(expr: &str, arg: Option<Object>) -> Result<T>
where
    T: FromObject,
{
    let mut args = Array::new();
    args.push(expr);
    if let Some(arg) = arg {
        args.push(arg);
    }
    Ok(api::call_function("luaeval", args)?)
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
    let bt: NvimString = api::get_option_value(
        "buftype",
        &OptionOpts::builder().buffer(buf.clone()).build(),
    )?;
    if !bt.is_empty() {
        return Ok(None);
    }
    let name = buf.get_name()?;
    if name.as_os_str().is_empty() {
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
        api::get_option_value("buftype", &OptionOpts::builder().buffer(current).build())?;
    if !bt.is_empty() {
        return Ok(());
    }

    for buf in api::list_bufs() {
        if !buf.is_valid() {
            continue;
        }
        let opt_opts = OptionOpts::builder().buffer(buf.clone()).build();
        let listed = api::get_option_value::<bool>("buflisted", &opt_opts).unwrap_or(false);
        if !listed {
            continue;
        }
        let name = buf.get_name()?;
        if !name.as_os_str().is_empty() {
            return Ok(());
        }
    }

    let _ = lua_eval::<Object>(SNACKS_DASHBOARD_LUA, None)?;
    Ok(())
}

fn data_buf(data: &Object, fallback: i64) -> i64 {
    let Ok(dict) = Dictionary::try_from(data.clone()) else {
        return fallback;
    };
    let key = NvimString::from("buf");
    dict.get(&key)
        .and_then(|obj| i64::from_object(obj.clone()).ok())
        .unwrap_or(fallback)
}

fn oil_current_dir(buf: i64) -> Result<Option<String>> {
    let obj: Object = lua_eval(OIL_DIR_LUA, Some(Object::from(buf)))?;
    if obj.is_nil() {
        return Ok(None);
    }
    let dir = NvimString::from_object(obj)?.to_string_lossy().into_owned();
    if dir.is_empty() {
        return Ok(None);
    }
    Ok(Some(dir))
}

fn win_for_buf_handle(buf: i64) -> Result<Option<Window>> {
    let win_id: i64 = api::call_function("bufwinid", Array::from_iter([buf]))?;
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

fn win_key(win: i64) -> String {
    win.to_string()
}

fn oil_last_buf_map() -> OilMap {
    let Ok(obj) = api::get_var::<Object>("oil_last_buf") else {
        return HashMap::new();
    };
    let Ok(dict) = Dictionary::try_from(obj) else {
        return HashMap::new();
    };
    let mut map = HashMap::new();
    for (key, value) in dict.iter() {
        if let Ok(buf) = i64::from_object(value.clone()) {
            map.insert(key.to_string_lossy().into_owned(), buf);
        }
    }
    map
}

fn map_to_dict(map: &OilMap) -> Dictionary {
    Dictionary::from_iter(map.iter().map(|(key, value)| (key.as_str(), *value)))
}

fn write_oil_last_buf(map: &OilMap) -> Result<()> {
    api::set_var("oil_last_buf", map_to_dict(map))?;
    Ok(())
}

fn clean_oil_last_buf() -> Result<()> {
    let mut map = oil_last_buf_map();
    let mut changed = false;
    map.retain(|key, buf_id| {
        let win_ok = key
            .parse::<i64>()
            .ok()
            .and_then(|id| i32::try_from(id).ok())
            .map(Window::from)
            .map(|win| win.is_valid())
            .unwrap_or(false);
        let buf_ok = i32::try_from(*buf_id)
            .ok()
            .map(Buffer::from)
            .map(|buf| buf.is_valid())
            .unwrap_or(false);
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

fn on_dashboard_delete() -> Result<ShouldDeleteAutocmd> {
    schedule(|()| maybe_show_dashboard());
    Ok(false)
}

fn on_file_cwd(args: AutocmdCallbackArgs) -> Result<ShouldDeleteAutocmd> {
    let Some(dir) = file_dir_for_buf(&args.buffer)? else {
        return Ok(false);
    };
    let Some(win) = win_for_buf(&args.buffer)? else {
        return Ok(false);
    };
    set_win_cwd(&win, &dir)?;
    Ok(false)
}

fn on_oil_enter(args: AutocmdCallbackArgs) -> Result<ShouldDeleteAutocmd> {
    let buf_handle = data_buf(&args.data, args.buffer.handle() as i64);
    let Some(dir) = oil_current_dir(buf_handle)? else {
        return Ok(false);
    };
    let Some(win) = win_for_buf_handle(buf_handle)? else {
        return Ok(false);
    };
    let win_id = win.handle() as i64;
    let mut map = oil_last_buf_map();
    map.insert(win_key(win_id), buf_handle);
    write_oil_last_buf(&map)?;
    set_win_cwd(&win, &dir)?;
    Ok(false)
}

fn on_win_closed(args: AutocmdCallbackArgs) -> Result<ShouldDeleteAutocmd> {
    let Ok(win_id) = args.r#match.parse::<i64>() else {
        return Ok(false);
    };
    let mut map = oil_last_buf_map();
    if map.remove(&win_key(win_id)).is_some() {
        write_oil_last_buf(&map)?;
    }
    Ok(false)
}

fn on_buf_wipeout(args: AutocmdCallbackArgs) -> Result<ShouldDeleteAutocmd> {
    let buf_id = args.buffer.handle() as i64;
    let mut map = oil_last_buf_map();
    let mut changed = false;
    map.retain(|_, mapped| {
        let keep = *mapped != buf_id;
        if !keep {
            changed = true;
        }
        keep
    });
    if changed {
        write_oil_last_buf(&map)?;
    }
    Ok(false)
}

fn dict_string(dict: &Dictionary, key: &str) -> Option<String> {
    let key = NvimString::from(key);
    dict.get(&key)
        .and_then(|obj| NvimString::from_object(obj.clone()).ok())
        .map(|val| val.to_string_lossy().into_owned())
}

fn on_oil_actions_post(args: AutocmdCallbackArgs) -> Result<ShouldDeleteAutocmd> {
    let Ok(dict) = Dictionary::try_from(args.data) else {
        return Ok(false);
    };
    let actions_key = NvimString::from("actions");
    let Some(actions_obj) = dict.get(&actions_key) else {
        return Ok(false);
    };
    let Ok(actions) = Vec::<Dictionary>::from_object(actions_obj.clone()) else {
        return Ok(false);
    };
    let Some(first) = actions.into_iter().next() else {
        return Ok(false);
    };
    let action_type = dict_string(&first, "type");
    if action_type.as_deref() != Some("move") {
        return Ok(false);
    }
    let Some(src) = dict_string(&first, "src_url") else {
        return Ok(false);
    };
    let Some(dest) = dict_string(&first, "dest_url") else {
        return Ok(false);
    };

    let mut args = Dictionary::new();
    args.insert("src_url", src);
    args.insert("dest_url", dest);
    let _ = lua_eval::<Object>(SNACKS_RENAME_LUA, Some(Object::from(args)))?;
    Ok(false)
}

fn setup_dashboard_autocmd() -> Result<()> {
    let group = api::create_augroup(
        "UserDashboard",
        &CreateAugroupOpts::builder().clear(true).build(),
    )?;
    let opts = CreateAutocmdOpts::builder()
        .group(group)
        .callback(|_args: AutocmdCallbackArgs| on_dashboard_delete())
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
        .callback(|args: AutocmdCallbackArgs| on_file_cwd(args))
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
        .patterns(["OilEnter"])
        .callback(|args: AutocmdCallbackArgs| on_oil_enter(args))
        .build();
    api::create_autocmd(["User"], &opts)?;
    Ok(())
}

fn setup_oil_last_buf_autocmds() -> Result<()> {
    let group = api::create_augroup(
        "UserOilLastBuf",
        &CreateAugroupOpts::builder().clear(true).build(),
    )?;

    let win_closed_opts = CreateAutocmdOpts::builder()
        .group(group)
        .callback(|args: AutocmdCallbackArgs| on_win_closed(args))
        .build();
    api::create_autocmd(["WinClosed"], &win_closed_opts)?;

    let wipeout_opts = CreateAutocmdOpts::builder()
        .group(group)
        .callback(|args: AutocmdCallbackArgs| on_buf_wipeout(args))
        .build();
    api::create_autocmd(["BufWipeout"], &wipeout_opts)?;

    let resized_opts = CreateAutocmdOpts::builder()
        .group(group)
        .callback(
            |_args: AutocmdCallbackArgs| -> Result<ShouldDeleteAutocmd> {
                clean_oil_last_buf()?;
                Ok(false)
            },
        )
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
        .callback(|args: AutocmdCallbackArgs| on_oil_actions_post(args))
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
