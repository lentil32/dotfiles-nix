use std::collections::HashMap;
use std::env;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;

use autocmds_core::{WeztermTabState, derive_tab_title, format_cli_failure};
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
    state::StateCell,
};
use support::{NonEmptyString, ProjectRoot, TabTitle};

use nvim_utils::path::{has_uri_scheme, path_is_dir, strip_known_prefixes};

type OilMap = HashMap<WinHandle, BufHandle>;

#[derive(Debug)]
struct OilMoveAction {
    src_url: NonEmptyString,
    dest_url: NonEmptyString,
}

#[derive(Debug)]
enum OilAction {
    Move(OilMoveAction),
    Other,
}

#[derive(Debug)]
struct OilActionsPostArgs {
    action: OilAction,
}

impl OilActionsPostArgs {
    fn parse(data: Object) -> Option<Self> {
        let dict = Dictionary::try_from(data).ok()?;
        let actions_key = NvimString::from("actions");
        let actions_obj = dict.get(&actions_key)?;
        let actions = Vec::<Dictionary>::from_object(actions_obj.clone()).ok()?;
        let first = actions.into_iter().next()?;
        let action = OilAction::parse(&first)?;
        Some(Self { action })
    }
}

impl OilAction {
    fn parse(action: &Dictionary) -> Option<Self> {
        let action_type = dict::get_string_nonempty(action, "type")?;
        if action_type != "move" {
            return Some(Self::Other);
        }
        let src = dict::get_string_nonempty(action, "src_url")?;
        let dest = dict::get_string_nonempty(action, "dest_url")?;
        let src = NonEmptyString::try_new(src).ok()?;
        let dest = NonEmptyString::try_new(dest).ok()?;
        Some(Self::Move(OilMoveAction {
            src_url: src,
            dest_url: dest,
        }))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AutocmdAction {
    Keep,
}

impl AutocmdAction {
    const fn as_bool(self) -> bool {
        match self {
            Self::Keep => false,
        }
    }
}

const LOG_CONTEXT: &str = "autocmds";
const WEZTERM_LOG_CONTEXT: &str = "wezterm_tab";
const PROJECT_ROOT_VAR: &str = "project_root";

static WEZTERM_TAB_STATE: StateCell<WeztermTabState> = StateCell::new(WeztermTabState::new());

fn wezterm_state_lock() -> nvim_oxi_utils::state::StateGuard<'static, WeztermTabState> {
    let guard = WEZTERM_TAB_STATE.lock();
    if guard.poisoned() {
        notify::warn(WEZTERM_LOG_CONTEXT, "state mutex poisoned; continuing");
    }
    guard
}

fn warn_cli_unavailable(state: &mut WeztermTabState, err: &std::io::Error) {
    if !state.take_warn_cli_unavailable() {
        return;
    }
    let message = format!("wezterm cli unavailable: {err}");
    notify::warn(WEZTERM_LOG_CONTEXT, &message);
}

fn warn_cli_failed(state: &mut WeztermTabState, message: &str) {
    if state.take_warn_cli_failed() {
        notify::warn(WEZTERM_LOG_CONTEXT, message);
    }
}

fn report_panic(label: &str, info: &guard::PanicInfo) {
    notify::error(LOG_CONTEXT, &format!("{label} panic: {}", info.render()));
}

fn run_autocmd<F>(label: &str, f: F) -> bool
where
    F: FnOnce() -> Result<AutocmdAction>,
{
    let result = guard::with_panic(Ok(AutocmdAction::Keep), f, |info| {
        report_panic(label, &info)
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

#[derive(Debug, Clone)]
struct WeztermContext {
    home: Option<PathBuf>,
}

impl WeztermContext {
    fn detect() -> Option<Self> {
        let in_wezterm = env::var_os("WEZTERM_PANE").is_some_and(|value| !value.is_empty());
        if !in_wezterm {
            return None;
        }
        let home = env::var_os("HOME").map(PathBuf::from);
        Some(Self { home })
    }
}

fn current_buf_project_root() -> Result<Option<ProjectRoot>> {
    let buf = api::get_current_buf();
    if !buf.is_valid() {
        return Ok(None);
    }
    let args = Array::from_iter([
        Object::from(buf.handle()),
        Object::from(PROJECT_ROOT_VAR),
        Object::from(""),
    ]);
    let root: NvimString = api::call_function("getbufvar", args)?;
    Ok(ProjectRoot::try_new(root.to_string_lossy().into_owned()).ok())
}

fn spawn_wezterm_cli(title: &TabTitle) -> std::io::Result<std::process::Child> {
    Command::new("wezterm")
        .args(["cli", "set-tab-title", title.as_str()])
        .spawn()
}

fn schedule_cli_failed_warning(message: String) {
    schedule(move |()| {
        let mut state = wezterm_state_lock();
        warn_cli_failed(&mut state, &message);
    });
}

fn monitor_wezterm_cli(child: std::process::Child) {
    thread::spawn(move || {
        let mut child = child;
        let result = child.wait();
        let message = match result {
            Ok(status) if status.success() => return,
            Ok(status) => format_cli_failure(status),
            Err(err) => format!("wezterm cli wait failed: {err}"),
        };
        schedule_cli_failed_warning(message);
    });
}

fn set_wezterm_tab_title(state: &mut WeztermTabState, title: &TabTitle) -> bool {
    if !state.cli_enabled() {
        return false;
    }
    match spawn_wezterm_cli(title) {
        Ok(child) => {
            monitor_wezterm_cli(child);
            true
        }
        Err(err) => {
            warn_cli_unavailable(state, &err);
            if err.kind() == ErrorKind::NotFound {
                state.disable_cli();
            }
            false
        }
    }
}

fn update_wezterm_tab_title() -> Result<AutocmdAction> {
    let Some(context) = WeztermContext::detect() else {
        return Ok(AutocmdAction::Keep);
    };

    let root = current_buf_project_root()?;
    let Some(title) = derive_tab_title(root, context.home.as_deref()) else {
        return Ok(AutocmdAction::Keep);
    };

    let mut state = wezterm_state_lock();

    if !state.should_update(&title) {
        return Ok(AutocmdAction::Keep);
    }

    if set_wezterm_tab_title(&mut state, &title) {
        state.record_title(title);
    }
    drop(state);
    Ok(AutocmdAction::Keep)
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
    let _: () = win.call(move |()| -> Result<()> {
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

fn on_dashboard_delete() -> AutocmdAction {
    schedule(|()| run_scheduled("dashboard", maybe_show_dashboard));
    AutocmdAction::Keep
}

fn on_file_cwd(args: &AutocmdCallbackArgs) -> Result<AutocmdAction> {
    let Some(dir) = file_dir_for_buf(&args.buffer)? else {
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
    let mut map = oil_last_buf_map();
    map.insert(win_handle, buf_handle);
    write_oil_last_buf(&map)?;
    set_win_cwd(&win, &dir)?;
    Ok(AutocmdAction::Keep)
}

fn on_win_closed(args: &AutocmdCallbackArgs) -> Result<AutocmdAction> {
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

fn on_buf_wipeout(args: &AutocmdCallbackArgs) -> Result<AutocmdAction> {
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
    let Some(parsed) = OilActionsPostArgs::parse(args.data) else {
        return Ok(AutocmdAction::Keep);
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

fn setup_oil_cwd_autocmd() -> Result<()> {
    let group = api::create_augroup(
        "UserOilCwd",
        &CreateAugroupOpts::builder().clear(true).build(),
    )?;
    let opts = CreateAutocmdOpts::builder()
        .group(group)
        .patterns(["oil://*"])
        .callback(|args: AutocmdCallbackArgs| run_autocmd("on_oil_buf", || on_oil_buf(&args)))
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
        .callback(|args: AutocmdCallbackArgs| run_autocmd("on_win_closed", || on_win_closed(&args)))
        .build();
    api::create_autocmd(["WinClosed"], &win_closed_opts)?;

    let wipeout_opts = CreateAutocmdOpts::builder()
        .group(group)
        .callback(|args: AutocmdCallbackArgs| {
            run_autocmd("on_buf_wipeout", || on_buf_wipeout(&args))
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

fn setup_wezterm_tab_autocmd() -> Result<()> {
    let group = api::create_augroup(
        "WeztermProjectTab",
        &CreateAugroupOpts::builder().clear(true).build(),
    )?;
    let opts = CreateAutocmdOpts::builder()
        .group(group)
        .callback(|_args: AutocmdCallbackArgs| {
            run_autocmd("wezterm_tab_title", update_wezterm_tab_title)
        })
        .build();
    api::create_autocmd(["VimEnter", "BufEnter", "DirChanged"], &opts)?;
    Ok(())
}

fn setup() -> Result<()> {
    setup_dashboard_autocmd()?;
    setup_file_cwd_autocmd()?;
    setup_oil_cwd_autocmd()?;
    setup_oil_last_buf_autocmds()?;
    setup_oil_rename_autocmd()?;
    setup_wezterm_tab_autocmd()?;
    Ok(())
}

#[nvim_oxi::plugin]
fn my_autocmds() -> Dictionary {
    let mut api = Dictionary::new();
    api.insert("setup", Function::<(), ()>::from_fn(|()| setup()));
    api
}
