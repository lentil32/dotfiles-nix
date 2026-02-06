use std::io::ErrorKind;
use std::path::PathBuf;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use super::wezterm_core::{
    WeztermState, derive_tab_title, format_cli_failure, format_set_working_dir_failure,
};
use nvim_oxi::api;
use nvim_oxi::api::opts::{CreateAugroupOpts, CreateAutocmdOpts};
use nvim_oxi::api::types::AutocmdCallbackArgs;
use nvim_oxi::{Array, Object, Result, String as NvimString, schedule};
use nvim_oxi_utils::{notify, state::StateCell};
use support::{ProjectRoot, TabTitle};

use crate::types::AutocmdAction;

const WEZTERM_LOG_CONTEXT: &str = "wezterm_tab";
const PROJECT_ROOT_VAR: &str = "project_root";
const WEZTERM_WAIT_TIMEOUT: Duration = Duration::from_secs(10);
const WEZTERM_WAIT_POLL_INTERVAL: Duration = Duration::from_millis(50);

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

fn spawn_wezterm_cli(title: &TabTitle) -> std::io::Result<std::process::Child> {
    Command::new("wezterm")
        .args(["cli", "set-tab-title", title.as_str()])
        .spawn()
}

fn spawn_wezterm_set_working_dir(cwd: &str) -> std::io::Result<std::process::Child> {
    Command::new("wezterm")
        .args(["set-working-directory", cwd])
        .spawn()
}

fn process_tab_title_result(
    title: &TabTitle,
    wait_result: std::io::Result<Option<std::process::ExitStatus>>,
) {
    let next_title = {
        let mut state = wezterm_state_lock();
        match wait_result {
            Ok(Some(status)) if status.success() => state.complete_title_success(title),
            Ok(Some(status)) => {
                warn_title_failed(&mut state, &format_cli_failure(status));
                state.complete_title_failure(title)
            }
            Ok(None) => {
                warn_title_failed(
                    &mut state,
                    "wezterm cli timed out; dropped in-flight title update",
                );
                state.complete_title_failure(title)
            }
            Err(err) => {
                warn_title_failed(&mut state, &format!("wezterm cli wait failed: {err}"));
                state.complete_title_failure(title)
            }
        }
    };
    if let Some(next_title) = next_title {
        start_wezterm_tab_title_update(next_title);
    }
}

fn process_working_dir_result(
    cwd: &String,
    wait_result: std::io::Result<Option<std::process::ExitStatus>>,
) {
    let next_cwd = {
        let mut state = wezterm_state_lock();
        match wait_result {
            Ok(Some(status)) if status.success() => state.complete_cwd_success(cwd),
            Ok(Some(status)) => {
                warn_cwd_failed(&mut state, &format_set_working_dir_failure(status));
                state.complete_cwd_failure(cwd)
            }
            Ok(None) => {
                warn_cwd_failed(
                    &mut state,
                    "wezterm set-working-directory timed out; dropped in-flight cwd update",
                );
                state.complete_cwd_failure(cwd)
            }
            Err(err) => {
                warn_cwd_failed(
                    &mut state,
                    &format!("wezterm set-working-directory wait failed: {err}"),
                );
                state.complete_cwd_failure(cwd)
            }
        }
    };
    if let Some(next_cwd) = next_cwd {
        start_wezterm_working_dir_update(next_cwd);
    }
}

fn wait_with_timeout(
    child: &mut std::process::Child,
) -> std::io::Result<Option<std::process::ExitStatus>> {
    wait_with_timeout_for(child, WEZTERM_WAIT_TIMEOUT, WEZTERM_WAIT_POLL_INTERVAL)
}

fn wait_with_timeout_for(
    child: &mut std::process::Child,
    timeout: Duration,
    poll_interval: Duration,
) -> std::io::Result<Option<std::process::ExitStatus>> {
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Some(status));
        }
        if started.elapsed() >= timeout {
            match child.kill() {
                Ok(()) => {}
                Err(err) if err.kind() == ErrorKind::InvalidInput => {}
                Err(err) => return Err(err),
            }
            let _ = child.wait();
            return Ok(None);
        }
        thread::sleep(poll_interval);
    }
}

fn monitor_wezterm_cli(child: std::process::Child, title: TabTitle) {
    thread::spawn(move || {
        let mut child = child;
        let wait_result = wait_with_timeout(&mut child);
        schedule(move |()| process_tab_title_result(&title, wait_result));
    });
}

fn monitor_wezterm_set_working_dir(child: std::process::Child, cwd: String) {
    thread::spawn(move || {
        let mut child = child;
        let wait_result = wait_with_timeout(&mut child);
        schedule(move |()| process_working_dir_result(&cwd, wait_result));
    });
}

fn on_wezterm_tab_title_spawn_error(title: &TabTitle, err: &std::io::Error) {
    let disable_cli = err.kind() == ErrorKind::NotFound;
    let next_title = {
        let mut state = wezterm_state_lock();
        if disable_cli {
            warn_cli_unavailable(&mut state, err);
        } else {
            warn_title_failed(&mut state, &format!("wezterm cli spawn failed: {err}"));
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

fn on_wezterm_working_dir_spawn_error(cwd: &String, err: &std::io::Error) {
    let disable_cli = err.kind() == ErrorKind::NotFound;
    let next_cwd = {
        let mut state = wezterm_state_lock();
        if disable_cli {
            warn_cli_unavailable(&mut state, err);
        } else {
            warn_cwd_failed(
                &mut state,
                &format!("wezterm set-working-directory spawn failed: {err}"),
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

fn start_wezterm_tab_title_update(title: TabTitle) {
    match spawn_wezterm_cli(&title) {
        Ok(child) => {
            monitor_wezterm_cli(child, title);
        }
        Err(err) => on_wezterm_tab_title_spawn_error(&title, &err),
    }
}

fn start_wezterm_working_dir_update(cwd: String) {
    match spawn_wezterm_set_working_dir(&cwd) {
        Ok(child) => {
            monitor_wezterm_set_working_dir(child, cwd);
        }
        Err(err) => on_wezterm_working_dir_spawn_error(&cwd, &err),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wait_with_timeout_for_returns_exit_status_for_completed_process()
    -> std::result::Result<(), &'static str> {
        let mut child = Command::new("sh")
            .args(["-c", "exit 0"])
            .spawn()
            .map_err(|_| "failed to spawn shell")?;
        let status = wait_with_timeout_for(
            &mut child,
            Duration::from_millis(200),
            Duration::from_millis(5),
        )
        .map_err(|_| "failed to wait for process")?;
        let Some(status) = status else {
            return Err("expected completed process");
        };
        if status.success() {
            Ok(())
        } else {
            Err("expected successful process status")
        }
    }

    #[test]
    fn wait_with_timeout_for_returns_none_when_process_times_out()
    -> std::result::Result<(), &'static str> {
        let mut child = Command::new("sh")
            .args(["-c", "sleep 5"])
            .spawn()
            .map_err(|_| "failed to spawn shell")?;
        let status = wait_with_timeout_for(
            &mut child,
            Duration::from_millis(50),
            Duration::from_millis(5),
        )
        .map_err(|_| "failed to wait for process")?;
        if status.is_none() {
            Ok(())
        } else {
            Err("expected timeout result")
        }
    }
}
