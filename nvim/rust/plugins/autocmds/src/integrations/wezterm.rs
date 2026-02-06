use std::io::ErrorKind;
use std::path::PathBuf;
use std::process::Command;

use super::wezterm_core::{
    WeztermState, derive_tab_title, format_cli_failure, format_set_working_dir_failure,
};
use nvim_oxi::api;
use nvim_oxi::api::opts::{CreateAugroupOpts, CreateAutocmdOpts};
use nvim_oxi::api::types::AutocmdCallbackArgs;
use nvim_oxi::{Array, Object, Result, String as NvimString};
use nvim_oxi_utils::{notify, state::StateCell};
use support::{ProjectRoot, TabTitle};

use crate::types::AutocmdAction;

const WEZTERM_LOG_CONTEXT: &str = "wezterm_tab";
const PROJECT_ROOT_VAR: &str = "project_root";

static WEZTERM_STATE: StateCell<WeztermState> = StateCell::new(WeztermState::new());

#[derive(Debug, Clone)]
struct WeztermContext {
    home: Option<PathBuf>,
}

impl WeztermContext {
    fn detect() -> Option<Self> {
        let in_wezterm = std::env::var_os("WEZTERM_PANE").is_some_and(|value| !value.is_empty());
        if !in_wezterm {
            return None;
        }
        let home = std::env::var_os("HOME").map(PathBuf::from);
        Some(Self { home })
    }
}

fn wezterm_state_lock() -> nvim_oxi_utils::state::StateGuard<'static, WeztermState> {
    let mut guard = WEZTERM_STATE.lock();
    if guard.poisoned() {
        notify::warn(
            WEZTERM_LOG_CONTEXT,
            "state mutex poisoned; resetting wezterm state",
        );
        *guard = WeztermState::default();
        WEZTERM_STATE.clear_poison();
    }
    guard
}

fn wezterm_cli_enabled() -> bool {
    let state = wezterm_state_lock();
    state.cli_enabled()
}

fn warn_cli_unavailable(state: &mut WeztermState, err: &std::io::Error) {
    if !state.take_warn_cli_unavailable() {
        return;
    }
    let message = format!("wezterm command unavailable: {err}");
    notify::warn(WEZTERM_LOG_CONTEXT, &message);
}

fn warn_title_failed(state: &mut WeztermState, message: &str) {
    if state.take_warn_title_failed() {
        notify::warn(WEZTERM_LOG_CONTEXT, message);
    }
}

fn warn_cwd_failed(state: &mut WeztermState, message: &str) {
    if state.take_warn_cwd_failed() {
        notify::warn(WEZTERM_LOG_CONTEXT, message);
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

fn current_window_cwd() -> Result<Option<String>> {
    let cwd: NvimString = api::call_function("getcwd", Array::new())?;
    let cwd = cwd.to_string_lossy().into_owned();
    if !crate::is_dir(&cwd) {
        return Ok(None);
    }
    Ok(Some(cwd))
}

fn run_wezterm_cli(title: &TabTitle) -> std::io::Result<std::process::ExitStatus> {
    Command::new("wezterm")
        .args(["cli", "set-tab-title", title.as_str()])
        .status()
}

fn run_wezterm_set_working_dir(cwd: &str) -> std::io::Result<std::process::ExitStatus> {
    Command::new("wezterm")
        .args(["set-working-directory", cwd])
        .status()
}

fn on_wezterm_tab_title_command_error(title: &TabTitle, err: &std::io::Error) {
    let disable_cli = err.kind() == ErrorKind::NotFound;
    let next_title = {
        let mut state = wezterm_state_lock();
        if disable_cli {
            warn_cli_unavailable(&mut state, err);
        } else {
            warn_title_failed(&mut state, &format!("wezterm cli failed: {err}"));
        }
        let next = state.complete_title_failure(title);
        if disable_cli {
            state.disable_cli();
            state.clear_pending_updates();
        }
        next
    };
    if disable_cli {
        return;
    }
    if let Some(next_title) = next_title {
        start_wezterm_tab_title_update(next_title);
    }
}

fn on_wezterm_working_dir_command_error(cwd: &String, err: &std::io::Error) {
    let disable_cli = err.kind() == ErrorKind::NotFound;
    let next_cwd = {
        let mut state = wezterm_state_lock();
        if disable_cli {
            warn_cli_unavailable(&mut state, err);
        } else {
            warn_cwd_failed(
                &mut state,
                &format!("wezterm set-working-directory failed: {err}"),
            );
        }
        let next = state.complete_cwd_failure(cwd);
        if disable_cli {
            state.disable_cli();
            state.clear_pending_updates();
        }
        next
    };
    if disable_cli {
        return;
    }
    if let Some(next_cwd) = next_cwd {
        start_wezterm_working_dir_update(next_cwd);
    }
}

fn on_wezterm_tab_title_status(title: &TabTitle, status: std::process::ExitStatus) {
    let next_title = {
        let mut state = wezterm_state_lock();
        if status.success() {
            state.complete_title_success(title)
        } else {
            warn_title_failed(&mut state, &format_cli_failure(status));
            state.complete_title_failure(title)
        }
    };
    if let Some(next_title) = next_title {
        start_wezterm_tab_title_update(next_title);
    }
}

fn on_wezterm_working_dir_status(cwd: &String, status: std::process::ExitStatus) {
    let next_cwd = {
        let mut state = wezterm_state_lock();
        if status.success() {
            state.complete_cwd_success(cwd)
        } else {
            warn_cwd_failed(&mut state, &format_set_working_dir_failure(status));
            state.complete_cwd_failure(cwd)
        }
    };
    if let Some(next_cwd) = next_cwd {
        start_wezterm_working_dir_update(next_cwd);
    }
}

fn start_wezterm_tab_title_update(title: TabTitle) {
    match run_wezterm_cli(&title) {
        Ok(status) => on_wezterm_tab_title_status(&title, status),
        Err(err) => on_wezterm_tab_title_command_error(&title, &err),
    }
}

fn start_wezterm_working_dir_update(cwd: String) {
    match run_wezterm_set_working_dir(&cwd) {
        Ok(status) => on_wezterm_working_dir_status(&cwd, status),
        Err(err) => on_wezterm_working_dir_command_error(&cwd, &err),
    }
}

fn update_wezterm_tab_title(context: &WeztermContext) -> Result<AutocmdAction> {
    if !wezterm_cli_enabled() {
        return Ok(AutocmdAction::Keep);
    }

    let root = current_buf_project_root()?;
    let Some(title) = derive_tab_title(root, context.home.as_deref()) else {
        return Ok(AutocmdAction::Keep);
    };

    let next_title = {
        let mut state = wezterm_state_lock();
        if state.cli_enabled() {
            state.request_title_update(title)
        } else {
            None
        }
    };
    if let Some(next_title) = next_title {
        start_wezterm_tab_title_update(next_title);
    }
    Ok(AutocmdAction::Keep)
}

fn update_wezterm_working_dir() -> Result<AutocmdAction> {
    if !wezterm_cli_enabled() {
        return Ok(AutocmdAction::Keep);
    }

    let Some(cwd) = current_window_cwd()? else {
        return Ok(AutocmdAction::Keep);
    };

    let next_cwd = {
        let mut state = wezterm_state_lock();
        if state.cli_enabled() {
            state.request_cwd_update(cwd)
        } else {
            None
        }
    };
    if let Some(next_cwd) = next_cwd {
        start_wezterm_working_dir_update(next_cwd);
    }
    Ok(AutocmdAction::Keep)
}

fn sync_wezterm_state() -> Result<AutocmdAction> {
    let Some(context) = WeztermContext::detect() else {
        return Ok(AutocmdAction::Keep);
    };
    update_wezterm_tab_title(&context)?;
    update_wezterm_working_dir()?;
    Ok(AutocmdAction::Keep)
}

pub fn setup_wezterm_autocmd() -> Result<()> {
    let group = api::create_augroup(
        "WeztermProjectTab",
        &CreateAugroupOpts::builder().clear(true).build(),
    )?;
    let opts = CreateAutocmdOpts::builder()
        .group(group)
        .callback(|_args: AutocmdCallbackArgs| {
            crate::run_autocmd("wezterm_sync", sync_wezterm_state)
        })
        .build();
    api::create_autocmd(["VimEnter", "BufEnter", "DirChanged"], &opts)?;
    Ok(())
}
