mod integrations;
mod types;

use nvim_oxi::api;
use nvim_oxi::api::opts::{CreateAugroupOpts, CreateAutocmdOpts, OptionOpts};
use nvim_oxi::api::types::AutocmdCallbackArgs;
use nvim_oxi::api::{Buffer, Window};
use nvim_oxi::{Array, Dictionary, Function, Result, String as NvimString, mlua, schedule};
use nvim_oxi_utils::{
    guard,
    handles::{self, BufHandle, WinHandle},
    lua, notify,
    state::StateCell,
};
use std::path::Path;
use std::sync::LazyLock;
use types::{AutocmdAction, OilAction, OilActionsPostArgs, OilLastBufEvent, OilLastBufState};

use nvim_utils::path::{has_uri_scheme, normalize_path, path_is_dir, strip_known_prefixes};

const LOG_CONTEXT: &str = "autocmds";

#[derive(Default)]
struct AutocmdState {
    oil_last_buf: OilLastBufState,
}

static STATE: LazyLock<StateCell<AutocmdState>> =
    LazyLock::new(|| StateCell::new(AutocmdState::default()));

fn state_lock() -> nvim_oxi_utils::state::StateGuard<'static, AutocmdState> {
    let mut guard = STATE.lock();
    if guard.poisoned() {
        notify::warn(LOG_CONTEXT, "state mutex poisoned; resetting autocmd state");
        *guard = AutocmdState::default();
        STATE.clear_poison();
    }
    guard
}

fn report_panic(label: &str, info: &guard::PanicInfo) {
    notify::error(LOG_CONTEXT, &format!("{label} panic: {}", info.render()));
}

pub(crate) fn run_autocmd<F>(label: &str, f: F) -> bool
where
    F: FnOnce() -> Result<AutocmdAction>,
{
    let result = guard::with_panic(Ok(AutocmdAction::Keep), f, |info| {
        report_panic(label, &info);
    });
    match result {
        Ok(value) => value.as_bool(),
        Err(err) => {
            notify::warn(LOG_CONTEXT, &format!("{label} failed: {err}"));
            false
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
        |info| report_panic(label, &info),
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

fn oil_current_dir(buf: BufHandle) -> Result<Option<String>> {
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

pub(crate) fn is_dir(path: &str) -> bool {
    if path.is_empty() {
        return false;
    }
    let path = strip_known_prefixes(path);
    if path.is_empty() {
        return false;
    }
    path_is_dir(Path::new(path))
}

fn win_current_cwd(win: &Window) -> Result<Option<String>> {
    if !win.is_valid() {
        return Ok(None);
    }

    let cwd: NvimString = win.call(|()| -> Result<NvimString> {
        let cwd: NvimString = api::call_function("getcwd", Array::new())?;
        Ok(cwd)
    })?;
    let cwd = cwd.to_string_lossy().into_owned();
    if cwd.is_empty() {
        return Ok(None);
    }
    Ok(Some(cwd))
}

fn set_win_cwd(win: &Window, dir: &str) -> Result<()> {
    if !win.is_valid() {
        return Ok(());
    }
    if dir.is_empty() || !is_dir(dir) {
        return Ok(());
    }

    if let Some(current) = win_current_cwd(win)? {
        let unchanged = match (normalize_path(&current), normalize_path(dir)) {
            (Some(current), Some(target)) => current == target,
            _ => current == dir,
        };
        if unchanged {
            return Ok(());
        }
    }

    let dir = dir.to_string();
    let _: () = win.call(move |()| -> Result<()> {
        let escaped: NvimString =
            api::call_function("fnameescape", Array::from_iter([dir.as_str()]))?;
        let cmd = format!("lcd {}", escaped.to_string_lossy());
        api::command(&cmd)?;
        Ok(())
    })?;
    Ok(())
}

fn file_dir_for_buf_name(buf: &Buffer, name_str: &str) -> Result<Option<String>> {
    if !buf.is_valid() {
        return Ok(None);
    }
    let bt: NvimString =
        api::get_option_value("buftype", &OptionOpts::builder().buf(buf.clone()).build())?;
    if !bt.is_empty() {
        return Ok(None);
    }
    if name_str.is_empty() {
        return Ok(None);
    }
    if has_uri_scheme(name_str) {
        return Ok(None);
    }
    let dir: NvimString = api::call_function("fnamemodify", Array::from_iter([name_str, ":p:h"]))?;
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
    Ok(handles::valid_window(win_id))
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

fn is_valid_buffer_handle(handle: BufHandle) -> bool {
    handle.valid_buffer().is_some()
}

fn apply_oil_last_buf_event(event: OilLastBufEvent) {
    let mut state = state_lock();
    state.oil_last_buf.apply(event);
}

fn win_handle_for_query(win: Option<i64>) -> Option<WinHandle> {
    let win = handles::window_from_optional(win.or(Some(0)))?;
    if !win.is_valid() {
        return None;
    }
    Some(WinHandle::from_window(&win))
}

fn oil_last_buf_for_win(win: Option<i64>) -> Option<i64> {
    let win_handle = win_handle_for_query(win)?;
    let buf = {
        let state = state_lock();
        state.oil_last_buf.mapped_buf_for_win(win_handle)
    }?;
    if is_valid_buffer_handle(buf) {
        return Some(buf.raw());
    }

    let mut state = state_lock();
    let _ = state.oil_last_buf.clear_mapping_if_matches(win_handle, buf);
    None
}

fn on_dashboard_delete() -> AutocmdAction {
    schedule(|()| run_scheduled("dashboard", maybe_show_dashboard));
    AutocmdAction::Keep
}

fn on_file_cwd(args: &AutocmdCallbackArgs) -> Result<AutocmdAction> {
    if !args.buffer.is_valid() {
        return Ok(AutocmdAction::Keep);
    }

    let name = args.buffer.get_name()?;
    let name = name.to_string_lossy();

    // Oil buffers use URI names and non-file buftype; route before generic file handling.
    if name.starts_with("oil://") {
        return on_oil_buf(args);
    }

    let Some(dir) = file_dir_for_buf_name(&args.buffer, name.as_ref())? else {
        return Ok(AutocmdAction::Keep);
    };
    let Some(win) = win_for_buf(&args.buffer)? else {
        return Ok(AutocmdAction::Keep);
    };
    set_win_cwd(&win, &dir)?;
    Ok(AutocmdAction::Keep)
}

fn on_oil_buf(args: &AutocmdCallbackArgs) -> Result<AutocmdAction> {
    let buf_handle = BufHandle::from_buffer(&args.buffer);
    let Some(dir) = oil_current_dir(buf_handle)? else {
        return Ok(AutocmdAction::Keep);
    };
    let Some(win) = win_for_buf(&args.buffer)? else {
        return Ok(AutocmdAction::Keep);
    };
    let win_handle = WinHandle::from_window(&win);
    apply_oil_last_buf_event(OilLastBufEvent::OilBufEntered {
        win: win_handle,
        buf: buf_handle,
    });
    set_win_cwd(&win, &dir)?;
    Ok(AutocmdAction::Keep)
}

fn on_win_closed(args: &AutocmdCallbackArgs) -> AutocmdAction {
    let Ok(win_id) = args.r#match.parse::<i64>() else {
        return AutocmdAction::Keep;
    };
    let Some(win_handle) = WinHandle::try_from_i64(win_id) else {
        return AutocmdAction::Keep;
    };
    apply_oil_last_buf_event(OilLastBufEvent::WinClosed { win: win_handle });
    AutocmdAction::Keep
}

fn on_buf_wipeout(args: &AutocmdCallbackArgs) -> AutocmdAction {
    let buf_handle = BufHandle::from_buffer(&args.buffer);
    apply_oil_last_buf_event(OilLastBufEvent::BufWiped { buf: buf_handle });
    AutocmdAction::Keep
}

fn on_oil_actions_post(args: AutocmdCallbackArgs) -> Result<AutocmdAction> {
    let parsed = match OilActionsPostArgs::parse(args.data) {
        Ok(parsed) => parsed,
        Err(err) => {
            notify::warn(
                LOG_CONTEXT,
                &format!("oil actions post args invalid: {err}"),
            );
            return Ok(AutocmdAction::Keep);
        }
    };
    if let OilAction::Move(action) = parsed.action {
        snacks_rename_file(action.src_url.as_str(), action.dest_url.as_str())?;
    }
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
            run_autocmd("on_dashboard_delete", || Ok(on_dashboard_delete()))
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
        .callback(|args: AutocmdCallbackArgs| run_autocmd("on_file_cwd", || on_file_cwd(&args)))
        .build();
    api::create_autocmd(["BufEnter"], &opts)?;
    Ok(())
}

fn setup_oil_last_buf_autocmds() -> Result<()> {
    let group = api::create_augroup(
        "UserOilLastBuf",
        &CreateAugroupOpts::builder().clear(true).build(),
    )?;

    let win_closed_opts = CreateAutocmdOpts::builder()
        .group(group)
        .callback(|args: AutocmdCallbackArgs| {
            run_autocmd("on_win_closed", || Ok(on_win_closed(&args)))
        })
        .build();
    api::create_autocmd(["WinClosed"], &win_closed_opts)?;

    let wipeout_opts = CreateAutocmdOpts::builder()
        .group(group)
        .callback(|args: AutocmdCallbackArgs| {
            run_autocmd("on_buf_wipeout", || Ok(on_buf_wipeout(&args)))
        })
        .build();
    api::create_autocmd(["BufWipeout"], &wipeout_opts)?;

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
    setup_oil_last_buf_autocmds()?;
    setup_oil_rename_autocmd()?;
    integrations::setup_wezterm_autocmd()?;
    Ok(())
}

#[nvim_oxi::plugin]
fn rs_autocmds() -> Dictionary {
    let mut api = Dictionary::new();
    api.insert("setup", Function::<(), ()>::from_fn(|()| setup()));
    api.insert(
        "oil_last_buf_for_win",
        Function::<Option<i64>, Option<i64>>::from_fn(oil_last_buf_for_win),
    );
    api
}
