use std::{
    collections::HashMap,
    panic::{catch_unwind, AssertUnwindSafe},
    path::Path,
    sync::Mutex,
};

use once_cell::sync::Lazy;

use nvim_oxi::api;
use nvim_oxi::api::opts::{CreateAugroupOpts, CreateAutocmdOpts, OptionOpts};
use nvim_oxi::api::types::AutocmdCallbackArgs;
use nvim_oxi::api::{Buffer, Window};
use nvim_oxi::conversion::FromObject;
use nvim_oxi::{
    schedule, Array, Dictionary, Function, Object, Result, String as NvimString,
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
  snacks.image.doc.find(args.buf, function(imgs)
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
    tokens: HashMap<i64, i64>,
    previews: HashMap<i64, DocPreviewState>,
}

static STATE: Lazy<Mutex<State>> = Lazy::new(|| Mutex::new(State::default()));

fn state_lock() -> std::sync::MutexGuard<'static, State> {
    match STATE.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn with_unwind_guard<F, R>(fallback: R, f: F) -> R
where
    F: FnOnce() -> R,
{
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(result) => result,
        Err(_) => fallback,
    }
}

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

fn valid_buffer(handle: i64) -> Option<Buffer> {
    let handle = i32::try_from(handle).ok()?;
    let buf = Buffer::from(handle);
    buf.is_valid().then_some(buf)
}

fn valid_window(handle: i64) -> Option<Window> {
    let handle = i32::try_from(handle).ok()?;
    let win = Window::from(handle);
    win.is_valid().then_some(win)
}

fn filetype_for_path(path: &str) -> Result<String> {
    let obj: Object = lua_eval(FILETYPE_MATCH_LUA, Some(Object::from(path)))?;
    if obj.is_nil() {
        return Ok(String::new());
    }
    let ft = NvimString::from_object(obj)
        .map(|val| val.to_string_lossy().into_owned())
        .unwrap_or_default();
    Ok(ft)
}

fn is_doc_preview_filetype(ft: &str) -> bool {
    matches!(
        ft,
        "markdown" | "markdown.mdx" | "mdx" | "typst" | "tex" | "plaintex" | "latex"
    )
}

fn snacks_has_doc_preview() -> bool {
    lua_eval::<bool>(SNACKS_HAS_DOC_LUA, None).unwrap_or(false)
}

fn get_buf_filetype(buf: &Buffer) -> String {
    let opt_opts = OptionOpts::builder().buffer(buf.clone()).build();
    api::get_option_value::<NvimString>("filetype", &opt_opts)
        .map(|val| val.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn set_buf_filetype(buf: &Buffer, ft: &str) -> Result<()> {
    let opt_opts = OptionOpts::builder().buffer(buf.clone()).build();
    api::set_option_value("filetype", ft, &opt_opts)?;
    Ok(())
}

fn next_doc_preview_token(buf_handle: i64) -> i64 {
    let mut state = state_lock();
    let entry = state.tokens.entry(buf_handle).or_insert(0);
    *entry += 1;
    *entry
}

fn state_ok(buf_handle: i64, token: i64) -> bool {
    let state = state_lock();
    state
        .previews
        .get(&buf_handle)
        .map(|entry| entry.token == token)
        .unwrap_or(false)
}

fn restore_doc_preview_name(buf_handle: i64, state: &DocPreviewState) {
    if !state.restore_name {
        return;
    }
    let Some(buf) = valid_buffer(buf_handle) else {
        return;
    };
    let Ok(name) = buf.get_name() else {
        return;
    };
    if name.to_string_lossy() == state.preview_name {
        let mut buf = buf.clone();
        let _ = buf.set_name(Path::new(&state.name));
    }
}

fn close_doc_preview(buf_handle: i64) {
    let state = {
        let mut state = state_lock();
        state.previews.remove(&buf_handle)
    };

    let Some(mut state) = state else {
        return;
    };

    restore_doc_preview_name(buf_handle, &state);

    if let Some(group) = state.group {
        let _ = api::del_augroup_by_id(group);
    }

    if let Some(cleanup) = state.cleanup.take() {
        let _ = cleanup.call(());
        cleanup.remove_from_lua_registry();
    }
}

fn attach_doc_preview(buf_handle: i64, path: &str, win_id: i64) -> Result<()> {
    let Some(buf) = valid_buffer(buf_handle) else {
        return Ok(());
    };

    let ft = filetype_for_path(path)?;
    if !is_doc_preview_filetype(&ft) {
        close_doc_preview(buf_handle);
        return Ok(());
    }

    if get_buf_filetype(&buf) != ft {
        let _ = set_buf_filetype(&buf, &ft);
    }

    if !snacks_has_doc_preview() {
        return Ok(());
    }

    close_doc_preview(buf_handle);

    if valid_window(win_id).is_none() {
        return Ok(());
    }

    let group_name = format!("snacks.doc_preview.{buf_handle}");
    let group = api::create_augroup(
        &group_name,
        &CreateAugroupOpts::builder().clear(true).build(),
    )?;

    let buf_handle_for_event = buf_handle;
    let opts = CreateAutocmdOpts::builder()
        .group(group)
        .buffer(buf.clone())
        .callback(move |_args: AutocmdCallbackArgs| {
            with_unwind_guard(false, || {
                close_doc_preview(buf_handle_for_event);
                false
            })
        })
        .build();
    api::create_autocmd(["BufWipeout", "BufHidden"], &opts)?;

    let buf_handle_for_win = buf_handle;
    let win_id_str = win_id.to_string();
    let win_opts = CreateAutocmdOpts::builder()
        .group(group)
        .patterns([win_id_str.as_str()])
        .callback(move |_args: AutocmdCallbackArgs| {
            with_unwind_guard(false, || {
                close_doc_preview(buf_handle_for_win);
                false
            })
        })
        .build();
    api::create_autocmd(["WinClosed"], &win_opts)?;

    let original_name = buf.get_name()?.to_string_lossy().into_owned();
    let preview_name = format!("{path}.snacks-preview");
    let restore_name = original_name != preview_name;
    if restore_name {
        let mut buf = buf.clone();
        let _ = buf.set_name(Path::new(&preview_name));
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
        ("buf", buf_handle),
        ("token", token),
        ("win", win_id),
    ]);
    let _ = lua_eval::<Object>(SNACKS_DOC_FIND_LUA, Some(Object::from(args)));

    Ok(())
}

fn dict_get_i64(dict: &Dictionary, key: &str) -> Option<i64> {
    let key = NvimString::from(key);
    let obj = dict.get(&key)?.clone();
    i64::from_object(obj).ok()
}

fn dict_get_string(dict: &Dictionary, key: &str) -> Option<String> {
    let key = NvimString::from(key);
    let obj = dict.get(&key)?.clone();
    if obj.is_nil() {
        return None;
    }
    NvimString::from_object(obj)
        .ok()
        .map(|val| val.to_string_lossy().into_owned())
        .filter(|val| !val.is_empty())
}

fn dict_get_object(dict: &Dictionary, key: &str) -> Option<Object> {
    let key = NvimString::from(key);
    dict.get(&key).cloned()
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
    if src.is_empty() {
        None
    } else {
        Some(src)
    }
}

fn cleanup_from_object(obj: Object) -> Option<Function<(), ()>> {
    if obj.is_nil() {
        return None;
    }
    Function::<(), ()>::from_object(obj).ok()
}

fn create_preview_cleanup(win_id: i64, src: &str) -> Option<Function<(), ()>> {
    let mut args = Dictionary::new();
    args.insert("win", win_id);
    args.insert("src", src);
    let obj: Object = lua_eval(SNACKS_OPEN_PREVIEW_LUA, Some(Object::from(args))).ok()?;
    cleanup_from_object(obj)
}

fn on_doc_find_inner(args: Dictionary) -> Result<()> {
    let buf_handle = dict_get_i64(&args, "buf").unwrap_or_default();
    if buf_handle == 0 {
        return Ok(());
    }
    let token = dict_get_i64(&args, "token").unwrap_or_default();
    if !state_ok(buf_handle, token) {
        return Ok(());
    }

    if let Some(state) = {
        let state = state_lock();
        state.previews.get(&buf_handle).cloned()
    } {
        restore_doc_preview_name(buf_handle, &state);
    }

    let imgs_obj = dict_get_object(&args, "imgs");
    let Some(imgs_obj) = imgs_obj else {
        return Ok(());
    };
    let Some(src) = first_img_src(&imgs_obj) else {
        return Ok(());
    };

    let win_id = dict_get_i64(&args, "win").unwrap_or_default();
    if win_id == 0 {
        return Ok(());
    }

    schedule(move |()| {
        let _ = catch_unwind(AssertUnwindSafe(|| {
            if !state_ok(buf_handle, token) {
                return;
            }
            if valid_window(win_id).is_none() {
                return;
            }
            let Some(cleanup) = create_preview_cleanup(win_id, &src) else {
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
                let _ = cleanup.call(());
                cleanup.remove_from_lua_registry();
            }
        }));
    });

    Ok(())
}

fn on_doc_find(args: Dictionary) -> Result<()> {
    let result = catch_unwind(AssertUnwindSafe(|| on_doc_find_inner(args)));
    match result {
        Ok(result) => result,
        Err(_) => Ok(()),
    }
}

fn attach_doc_preview_lua(args: Dictionary) -> Result<()> {
    let buf_handle = dict_get_i64(&args, "buf").unwrap_or_default();
    if buf_handle == 0 {
        return Ok(());
    }
    let win_id = dict_get_i64(&args, "win").unwrap_or_default();
    if win_id == 0 {
        return Ok(());
    }
    let Some(path) = dict_get_string(&args, "path") else {
        return Ok(());
    };
    let _ = attach_doc_preview(buf_handle, &path, win_id);
    Ok(())
}

fn close_doc_preview_lua(buf_handle: i64) -> Result<()> {
    if buf_handle != 0 {
        close_doc_preview(buf_handle);
    }
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
