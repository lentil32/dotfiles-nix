use std::{collections::HashMap, path::Path};

use once_cell::sync::Lazy;

use nvim_oxi::api;
use nvim_oxi::api::Buffer;
use nvim_oxi::api::opts::{CreateAugroupOpts, CreateAutocmdOpts, OptionOpts};
use nvim_oxi::api::types::AutocmdCallbackArgs;
use nvim_oxi::conversion::FromObject;
use nvim_oxi::{Array, Dictionary, Function, Object, Result, String as NvimString, mlua, schedule};
use nvim_oxi_utils::{
    dict, guard,
    handles::{BufHandle, WinHandle},
    lua, notify,
    state::StateCell,
};

const BRIDGE_MODULE: &str = "myLuaConf.snacks_preview_bridge";
const LOG_CONTEXT: &str = "snacks_preview";

#[derive(Clone)]
struct DocPreviewState {
    token: i64,
    group: Option<u32>,
    cleanup: Option<i64>,
    name: String,
    preview_name: String,
    restore_name: bool,
}

#[derive(Default)]
struct State {
    tokens: HashMap<BufHandle, i64>,
    previews: HashMap<BufHandle, DocPreviewState>,
}

static STATE: Lazy<StateCell<State>> = Lazy::new(|| StateCell::new(State::default()));

fn state_lock() -> nvim_oxi_utils::state::StateGuard<'static, State> {
    let guard = STATE.lock();
    if guard.poisoned() {
        notify::warn(LOG_CONTEXT, "state mutex poisoned; continuing");
    }
    guard
}

fn report_panic(label: &str, info: guard::PanicInfo) {
    notify::error(LOG_CONTEXT, &format!("{label} panic: {}", info.render()));
}

fn bridge_table(lua: &mlua::Lua) -> Result<mlua::Table> {
    lua::require_table(lua, BRIDGE_MODULE)
}

fn call_bridge<A, R>(lua: &mlua::Lua, name: &str, args: A) -> Result<R>
where
    A: mlua::IntoLuaMulti,
    R: mlua::FromLuaMulti,
{
    let bridge = bridge_table(lua)?;
    lua::call_table_function(&bridge, name, args)
}

fn filetype_for_path(path: &str) -> Result<String> {
    let lua = lua::state();
    let ft: Option<String> = call_bridge(&lua, "filetype_match", (path,))?;
    Ok(ft.unwrap_or_default())
}

fn is_doc_preview_filetype(ft: &str) -> bool {
    matches!(
        ft,
        "markdown" | "markdown.mdx" | "mdx" | "typst" | "tex" | "plaintex" | "latex"
    )
}

fn require_i64(args: &Dictionary, key: &str) -> Option<i64> {
    match dict::require_i64(args, key) {
        Ok(value) => Some(value),
        Err(err) => {
            notify::warn(LOG_CONTEXT, &format!("missing/invalid {key}: {err}"));
            None
        }
    }
}

fn require_buf_handle(args: &Dictionary, key: &str) -> Option<BufHandle> {
    let value = require_i64(args, key)?;
    match BufHandle::try_from_i64(value) {
        Some(handle) => Some(handle),
        None => {
            notify::warn(
                LOG_CONTEXT,
                &format!("{key} invalid buffer handle: {value}"),
            );
            None
        }
    }
}

fn require_win_handle(args: &Dictionary, key: &str) -> Option<WinHandle> {
    let value = require_i64(args, key)?;
    match WinHandle::try_from_i64(value) {
        Some(handle) => Some(handle),
        None => {
            notify::warn(
                LOG_CONTEXT,
                &format!("{key} invalid window handle: {value}"),
            );
            None
        }
    }
}

fn snacks_has_doc_preview() -> bool {
    let lua = lua::state();
    match call_bridge::<_, bool>(&lua, "snacks_has_doc", ()) {
        Ok(value) => value,
        Err(err) => {
            notify::warn(
                LOG_CONTEXT,
                &format!("snacks doc preview check failed: {err}"),
            );
            false
        }
    }
}

fn snacks_doc_find(buf_handle: BufHandle, token: i64, win_handle: WinHandle) -> Result<()> {
    let lua = lua::state();
    let args = lua.create_table()?;
    args.set("buf", buf_handle.raw())?;
    args.set("token", token)?;
    args.set("win", win_handle.raw())?;
    call_bridge(&lua, "snacks_doc_find", args)
}

fn snacks_open_preview(win_handle: WinHandle, src: &str) -> Result<Option<i64>> {
    let lua = lua::state();
    let args = lua.create_table()?;
    args.set("win", win_handle.raw())?;
    args.set("src", src)?;
    call_bridge(&lua, "snacks_open_preview", args)
}

fn snacks_close_preview(cleanup_id: i64) -> Result<()> {
    let lua = lua::state();
    call_bridge(&lua, "snacks_close_preview", cleanup_id)
}

fn get_buf_filetype(buf: &Buffer) -> String {
    let opt_opts = OptionOpts::builder().buf(buf.clone()).build();
    match api::get_option_value::<NvimString>("filetype", &opt_opts) {
        Ok(value) => value.to_string_lossy().into_owned(),
        Err(err) => {
            notify::warn(LOG_CONTEXT, &format!("get filetype failed: {err}"));
            String::new()
        }
    }
}

fn set_buf_filetype(buf: &Buffer, ft: &str) -> Result<()> {
    let opt_opts = OptionOpts::builder().buf(buf.clone()).build();
    api::set_option_value("filetype", ft, &opt_opts)?;
    Ok(())
}

fn next_doc_preview_token(buf_handle: BufHandle) -> i64 {
    let mut state = state_lock();
    let entry = state.tokens.entry(buf_handle).or_insert(0);
    *entry += 1;
    *entry
}

fn state_ok(buf_handle: BufHandle, token: i64) -> bool {
    let state = state_lock();
    state
        .previews
        .get(&buf_handle)
        .map(|entry| entry.token == token)
        .unwrap_or(false)
}

fn restore_doc_preview_name(buf_handle: BufHandle, state: &DocPreviewState) {
    if !state.restore_name {
        return;
    }
    let Some(buf) = buf_handle.valid_buffer() else {
        return;
    };
    let Ok(name) = buf.get_name() else {
        notify::warn(
            LOG_CONTEXT,
            "restore preview name failed to read buffer name",
        );
        return;
    };
    if name.to_string_lossy() == state.preview_name {
        let mut buf = buf.clone();
        if let Err(err) = buf.set_name(Path::new(&state.name)) {
            notify::warn(LOG_CONTEXT, &format!("restore preview name failed: {err}"));
        }
    }
}

fn close_doc_preview(buf_handle: BufHandle) {
    let state = {
        let mut state = state_lock();
        state.tokens.remove(&buf_handle);
        state.previews.remove(&buf_handle)
    };

    let Some(mut state) = state else {
        return;
    };

    restore_doc_preview_name(buf_handle, &state);

    if let Some(group) = state.group {
        if let Err(err) = api::del_augroup_by_id(group) {
            notify::warn(LOG_CONTEXT, &format!("delete augroup failed: {err}"));
        }
    }

    if let Some(cleanup_id) = state.cleanup.take() {
        if let Err(err) = snacks_close_preview(cleanup_id) {
            notify::warn(LOG_CONTEXT, &format!("preview cleanup failed: {err}"));
        }
    }
}

fn attach_doc_preview(buf_handle: BufHandle, path: &str, win_handle: WinHandle) -> Result<()> {
    let Some(buf) = buf_handle.valid_buffer() else {
        return Ok(());
    };

    let ft = filetype_for_path(path)?;
    if !is_doc_preview_filetype(&ft) {
        close_doc_preview(buf_handle);
        return Ok(());
    }

    if get_buf_filetype(&buf) != ft {
        if let Err(err) = set_buf_filetype(&buf, &ft) {
            notify::warn(LOG_CONTEXT, &format!("set filetype failed: {err}"));
        }
    }

    close_doc_preview(buf_handle);

    if !snacks_has_doc_preview() {
        return Ok(());
    }

    if win_handle.valid_window().is_none() {
        return Ok(());
    }

    let group_name = format!("snacks.doc_preview.{}", buf_handle.raw());
    let group = api::create_augroup(
        &group_name,
        &CreateAugroupOpts::builder().clear(true).build(),
    )?;

    let buf_handle_for_event = buf_handle;
    let opts = CreateAutocmdOpts::builder()
        .group(group)
        .buffer(buf.clone())
        .callback(move |_args: AutocmdCallbackArgs| {
            guard::with_panic(
                false,
                || {
                    close_doc_preview(buf_handle_for_event);
                    false
                },
                |info| report_panic("doc_preview_buf_close", info),
            )
        })
        .build();
    api::create_autocmd(["BufWipeout", "BufHidden"], &opts)?;

    let buf_handle_for_win = buf_handle;
    let win_id_str = win_handle.raw().to_string();
    let win_opts = CreateAutocmdOpts::builder()
        .group(group)
        .patterns([win_id_str.as_str()])
        .callback(move |_args: AutocmdCallbackArgs| {
            guard::with_panic(
                false,
                || {
                    close_doc_preview(buf_handle_for_win);
                    false
                },
                |info| report_panic("doc_preview_win_close", info),
            )
        })
        .build();
    api::create_autocmd(["WinClosed"], &win_opts)?;

    let original_name = buf.get_name()?.to_string_lossy().into_owned();
    let preview_name = format!("{path}.snacks-preview");
    let restore_name = original_name != preview_name;
    if restore_name {
        let mut buf = buf.clone();
        if let Err(err) = buf.set_name(Path::new(&preview_name)) {
            notify::warn(LOG_CONTEXT, &format!("set preview name failed: {err}"));
        }
    }

    let token = next_doc_preview_token(buf_handle);
    {
        let mut state = state_lock();
        state.previews.insert(
            buf_handle,
            DocPreviewState {
                token,
                group: Some(group),
                cleanup: None,
                name: original_name,
                preview_name,
                restore_name,
            },
        );
    }

    if let Err(err) = snacks_doc_find(buf_handle, token, win_handle) {
        notify::warn(LOG_CONTEXT, &format!("snacks doc find failed: {err}"));
    }

    Ok(())
}

fn first_img_src(imgs_obj: &Object) -> Option<String> {
    let imgs = Array::from_object(imgs_obj.clone()).ok()?;
    let first = imgs.iter().next()?;
    let img = Dictionary::from_object(first.clone()).ok()?;
    let src_obj = img.get(&NvimString::from("src"))?.clone();
    let src = NvimString::from_object(src_obj)
        .ok()?
        .to_string_lossy()
        .into_owned();
    if src.is_empty() { None } else { Some(src) }
}

fn create_preview_cleanup(win_handle: WinHandle, src: &str) -> Option<i64> {
    match snacks_open_preview(win_handle, src) {
        Ok(value) => value,
        Err(err) => {
            notify::warn(LOG_CONTEXT, &format!("snacks open preview failed: {err}"));
            None
        }
    }
}

fn on_doc_find_inner(args: Dictionary) -> Result<()> {
    let Some(buf_handle) = require_buf_handle(&args, "buf") else {
        return Ok(());
    };
    let Some(token) = require_i64(&args, "token") else {
        return Ok(());
    };
    if !state_ok(buf_handle, token) {
        return Ok(());
    }

    if let Some(state) = {
        let state = state_lock();
        state.previews.get(&buf_handle).cloned()
    } {
        restore_doc_preview_name(buf_handle, &state);
    }

    let imgs_obj = dict::get_object(&args, "imgs");
    let Some(imgs_obj) = imgs_obj else {
        return Ok(());
    };
    let Some(src) = first_img_src(&imgs_obj) else {
        return Ok(());
    };

    let Some(win_handle) = require_win_handle(&args, "win") else {
        return Ok(());
    };

    schedule(move |()| {
        guard::with_panic(
            (),
            || {
                if !state_ok(buf_handle, token) {
                    return;
                }
                if win_handle.valid_window().is_none() {
                    return;
                }
                let Some(cleanup_id) = create_preview_cleanup(win_handle, &src) else {
                    return;
                };
                let mut cleanup_to_run = Some(cleanup_id);
                {
                    let mut state = state_lock();
                    if let Some(entry) = state.previews.get_mut(&buf_handle) {
                        entry.cleanup = cleanup_to_run.take();
                    }
                }
                if let Some(cleanup_id) = cleanup_to_run {
                    if let Err(err) = snacks_close_preview(cleanup_id) {
                        notify::warn(LOG_CONTEXT, &format!("preview cleanup failed: {err}"));
                    }
                }
            },
            |info| report_panic("doc_preview_schedule", info),
        );
    });

    Ok(())
}

fn on_doc_find(args: Dictionary) -> Result<()> {
    match guard::catch_unwind_result(|| on_doc_find_inner(args)) {
        Ok(result) => result,
        Err(info) => {
            report_panic("on_doc_find", info);
            Ok(())
        }
    }
}

fn attach_doc_preview_lua(args: Dictionary) -> Result<()> {
    let Some(buf_handle) = require_buf_handle(&args, "buf") else {
        return Ok(());
    };
    let Some(win_handle) = require_win_handle(&args, "win") else {
        return Ok(());
    };
    let Some(path) = dict::get_string_nonempty(&args, "path") else {
        return Ok(());
    };
    if let Err(err) = attach_doc_preview(buf_handle, &path, win_handle) {
        notify::warn(LOG_CONTEXT, &format!("attach doc preview failed: {err}"));
    }
    Ok(())
}

fn close_doc_preview_lua(buf_handle: i64) -> Result<()> {
    let Some(buf_handle) = BufHandle::try_from_i64(buf_handle) else {
        return Ok(());
    };
    close_doc_preview(buf_handle);
    Ok(())
}

#[nvim_oxi::plugin]
fn snacks_preview() -> Result<Dictionary> {
    let mut api = Dictionary::new();
    api.insert(
        "on_doc_find",
        Function::<Dictionary, ()>::from_fn(on_doc_find),
    );
    api.insert(
        "attach_doc_preview",
        Function::<Dictionary, ()>::from_fn(attach_doc_preview_lua),
    );
    api.insert(
        "close_doc_preview",
        Function::<i64, ()>::from_fn(close_doc_preview_lua),
    );
    Ok(api)
}
