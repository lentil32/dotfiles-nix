use std::{collections::HashMap, path::Path};

use once_cell::sync::Lazy;

use nvim_oxi::api;
use nvim_oxi::api::Buffer;
use nvim_oxi::api::opts::{CreateAugroupOpts, CreateAutocmdOpts, OptionOpts};
use nvim_oxi::api::types::AutocmdCallbackArgs;
use nvim_oxi::conversion::FromObject;
use nvim_oxi::{Array, Dictionary, Function, Object, Result, String as NvimString, schedule};
use nvim_oxi_utils::{
    dict, guard,
    handles::{BufHandle, WinHandle},
    lua, notify,
    state::StateCell,
};

const FILETYPE_MATCH_LUA: &str = r#"(function(path)
  return vim.filetype.match({ filename = path })
end)(_A)"#;

const SNACKS_HAS_DOC_LUA: &str = r#"(function()
  local ok, snacks = pcall(require, "snacks")
  if not ok then
    return false
  end
  return snacks and snacks.image and snacks.image.doc and snacks.image.terminal and true or false
end)()"#;

const SNACKS_DOC_FIND_LUA: &str = r#"(function(args)
  local ok, snacks = pcall(require, "snacks")
  if not ok then
    return
  end
  if not (snacks.image and snacks.image.doc) then
    return
  end
  snacks.image.doc.find_visible(args.buf, function(imgs)
    require("snacks_preview").on_doc_find({
      buf = args.buf,
      token = args.token,
      win = args.win,
      imgs = imgs,
    })
  end)
end)(_A)"#;

const SNACKS_OPEN_PREVIEW_LUA: &str = r#"(function(args)
  local ok, snacks = pcall(require, "snacks")
  if not ok then
    return nil
  end
  local win_id = args.win
  local src = args.src
  if not (win_id and src) then
    return nil
  end
  if not (snacks.image and snacks.image.placement and snacks.image.config and snacks.win) then
    return nil
  end
  local max_width = snacks.image.config.doc.max_width or 80
  local max_height = snacks.image.config.doc.max_height or 40
  local base_width = vim.api.nvim_win_get_width(win_id)
  local base_height = vim.api.nvim_win_get_height(win_id)
  local win = snacks.win(snacks.win.resolve(snacks.image.config.doc, "snacks_image", {
    relative = "win",
    win = win_id,
    row = 1,
    col = 1,
    width = math.min(max_width, base_width),
    height = math.min(max_height, base_height),
    show = true,
    enter = false,
  }))
  win:open_buf()
  local updated = false
  local opts = snacks.config.merge({}, snacks.image.config.doc, {
    inline = false,
    auto_resize = true,
    on_update_pre = function(p)
      if not updated then
        updated = true
        local loc = p:state().loc
        win.opts.width = loc.width
        win.opts.height = loc.height
        win:show()
      end
    end,
  })
  local placement = snacks.image.placement.new(win.buf, src, opts)
  return function()
    pcall(function()
      placement:close()
    end)
    pcall(function()
      win:close()
    end)
  end
end)(_A)"#;
const LOG_CONTEXT: &str = "snacks_preview";

#[derive(Clone)]
struct DocPreviewState {
    token: i64,
    group: Option<u32>,
    cleanup: Option<Function<(), ()>>,
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

fn filetype_for_path(path: &str) -> Result<String> {
    let obj: Object = lua::eval(FILETYPE_MATCH_LUA, Some(Object::from(path)))?;
    if obj.is_nil() {
        return Ok(String::new());
    }
    let ft = match NvimString::from_object(obj) {
        Ok(val) => val.to_string_lossy().into_owned(),
        Err(err) => {
            notify::warn(
                LOG_CONTEXT,
                &format!("filetype match failed to decode string: {err}"),
            );
            String::new()
        }
    };
    Ok(ft)
}

fn is_doc_preview_filetype(ft: &str) -> bool {
    matches!(
        ft,
        "markdown" | "markdown.mdx" | "mdx" | "typst" | "tex" | "plaintex" | "latex"
    )
}

fn snacks_has_doc_preview() -> bool {
    match lua::eval::<bool>(SNACKS_HAS_DOC_LUA, None) {
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

fn get_buf_filetype(buf: &Buffer) -> String {
    let opt_opts = OptionOpts::builder().buffer(buf.clone()).build();
    match api::get_option_value::<NvimString>("filetype", &opt_opts) {
        Ok(value) => value.to_string_lossy().into_owned(),
        Err(err) => {
            notify::warn(LOG_CONTEXT, &format!("get filetype failed: {err}"));
            String::new()
        }
    }
}

fn set_buf_filetype(buf: &Buffer, ft: &str) -> Result<()> {
    let opt_opts = OptionOpts::builder().buffer(buf.clone()).build();
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

    if let Some(cleanup) = state.cleanup.take() {
        if let Err(err) = cleanup.call(()) {
            notify::warn(LOG_CONTEXT, &format!("preview cleanup failed: {err}"));
        }
        cleanup.remove_from_lua_registry();
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

    let args = Dictionary::from_iter([
        ("buf", buf_handle.raw()),
        ("token", token),
        ("win", win_handle.raw()),
    ]);
    if let Err(err) = lua::eval::<Object>(SNACKS_DOC_FIND_LUA, Some(Object::from(args))) {
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

fn cleanup_from_object(obj: Object) -> Option<Function<(), ()>> {
    if obj.is_nil() {
        return None;
    }
    Function::<(), ()>::from_object(obj).ok()
}

fn create_preview_cleanup(win_handle: WinHandle, src: &str) -> Option<Function<(), ()>> {
    let mut args = Dictionary::new();
    args.insert("win", win_handle.raw());
    args.insert("src", src);
    let obj: Object = match lua::eval(SNACKS_OPEN_PREVIEW_LUA, Some(Object::from(args))) {
        Ok(value) => value,
        Err(err) => {
            notify::warn(LOG_CONTEXT, &format!("snacks open preview failed: {err}"));
            return None;
        }
    };
    cleanup_from_object(obj)
}

fn on_doc_find_inner(args: Dictionary) -> Result<()> {
    let Some(buf_handle) = dict::get_i64(&args, "buf").and_then(BufHandle::try_from_i64) else {
        return Ok(());
    };
    let token = dict::get_i64(&args, "token").unwrap_or_default();
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

    let Some(win_handle) = dict::get_i64(&args, "win").and_then(WinHandle::try_from_i64) else {
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
                let Some(cleanup) = create_preview_cleanup(win_handle, &src) else {
                    return;
                };
                let mut cleanup_to_run = Some(cleanup);
                {
                    let mut state = state_lock();
                    if let Some(entry) = state.previews.get_mut(&buf_handle) {
                        entry.cleanup = cleanup_to_run.take();
                    }
                }
                if let Some(cleanup) = cleanup_to_run {
                    if let Err(err) = cleanup.call(()) {
                        notify::warn(LOG_CONTEXT, &format!("preview cleanup failed: {err}"));
                    }
                    cleanup.remove_from_lua_registry();
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
    let Some(buf_handle) = dict::get_i64(&args, "buf").and_then(BufHandle::try_from_i64) else {
        return Ok(());
    };
    let Some(win_handle) = dict::get_i64(&args, "win").and_then(WinHandle::try_from_i64) else {
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
