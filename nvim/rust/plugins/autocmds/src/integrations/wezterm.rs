use std::collections::VecDeque;
use std::io::{Error, ErrorKind};
use std::path::PathBuf;
use std::process::{Command, ExitStatus};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, TrySendError};
use std::sync::{Arc, LazyLock, Mutex};
use std::thread;

use super::wezterm_core::{
    WeztermCommand, WeztermCompletion, WeztermEvent, WeztermState, derive_tab_title,
    format_cli_failure, format_set_working_dir_failure,
};
use nvim_oxi::api;
use nvim_oxi::api::opts::{CreateAugroupOpts, CreateAutocmdOpts, CreateCommandOpts, OptionOpts};
use nvim_oxi::api::types::{AutocmdCallbackArgs, CommandArgs};
use nvim_oxi::libuv::AsyncHandle;
use nvim_oxi::{Array, Result, String as NvimString};
use nvim_oxi_utils::{notify, state::StateCell};
use support::{ProjectRoot, TabTitle};

use crate::types::AutocmdAction;

const WEZTERM_LOG_CONTEXT: &str = "wezterm_tab";
const PROJECT_ROOT_VAR: &str = "project_root";
const WEZTERM_WORKER_QUEUE_CAPACITY: usize = 64;

static WEZTERM_STATE: StateCell<WeztermState> = StateCell::new(WeztermState::new());
static WEZTERM_DISPATCHER: LazyLock<WeztermDispatcher> = LazyLock::new(WeztermDispatcher::new);

#[derive(Debug)]
enum WeztermCommandResult {
    TabTitle {
        title: TabTitle,
        status: std::io::Result<ExitStatus>,
    },
    WorkingDir {
        cwd: String,
        status: std::io::Result<ExitStatus>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WeztermRuntimeMode {
    HealthyAsync,
    DegradedPolling,
}

impl WeztermRuntimeMode {
    const fn as_str(self) -> &'static str {
        match self {
            Self::HealthyAsync => "healthy_async",
            Self::DegradedPolling => "degraded_polling",
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct WeztermSyncStats {
    requested: u64,
    enqueued: u64,
    coalesced: u64,
    executed: u64,
    wakeup_failures: u64,
    enqueue_failures: u64,
}

#[derive(Debug, Clone, Copy)]
struct WeztermSyncSnapshot {
    mode: WeztermRuntimeMode,
    stats: WeztermSyncStats,
}

impl WeztermSyncSnapshot {
    fn render(self) -> String {
        format!(
            "mode={} requested={} enqueued={} coalesced={} executed={} wakeup_failures={} enqueue_failures={}",
            self.mode.as_str(),
            self.stats.requested,
            self.stats.enqueued,
            self.stats.coalesced,
            self.stats.executed,
            self.stats.wakeup_failures,
            self.stats.enqueue_failures
        )
    }
}

struct WeztermDispatcherShared {
    completed: Mutex<VecDeque<WeztermCommandResult>>,
    wakeup: Option<AsyncHandle>,
    mode: Mutex<WeztermRuntimeMode>,
    degraded_warning_pending: AtomicBool,
    stats: Mutex<WeztermSyncStats>,
}

impl WeztermDispatcherShared {
    fn new(wakeup: Option<AsyncHandle>, mode: WeztermRuntimeMode) -> Self {
        Self {
            completed: Mutex::new(VecDeque::new()),
            wakeup,
            mode: Mutex::new(mode),
            degraded_warning_pending: AtomicBool::new(false),
            stats: Mutex::new(WeztermSyncStats::default()),
        }
    }

    fn with_completed_queue<R, F>(&self, f: F) -> R
    where
        F: FnOnce(&mut VecDeque<WeztermCommandResult>) -> R,
    {
        match self.completed.lock() {
            Ok(mut queue) => f(&mut queue),
            Err(poisoned) => {
                let mut queue = poisoned.into_inner();
                f(&mut queue)
            }
        }
    }

    fn with_mode<R, F>(&self, f: F) -> R
    where
        F: FnOnce(&mut WeztermRuntimeMode) -> R,
    {
        match self.mode.lock() {
            Ok(mut mode) => f(&mut mode),
            Err(poisoned) => {
                let mut mode = poisoned.into_inner();
                f(&mut mode)
            }
        }
    }

    fn with_stats<R, F>(&self, f: F) -> R
    where
        F: FnOnce(&mut WeztermSyncStats) -> R,
    {
        match self.stats.lock() {
            Ok(mut stats) => f(&mut stats),
            Err(poisoned) => {
                let mut stats = poisoned.into_inner();
                f(&mut stats)
            }
        }
    }

    fn mode(&self) -> WeztermRuntimeMode {
        self.with_mode(|mode| *mode)
    }

    fn transition_to_degraded_polling(&self) {
        let transitioned = self.with_mode(|mode| {
            if *mode == WeztermRuntimeMode::HealthyAsync {
                *mode = WeztermRuntimeMode::DegradedPolling;
                true
            } else {
                false
            }
        });
        if transitioned {
            self.degraded_warning_pending.store(true, Ordering::Relaxed);
        }
    }

    fn take_degraded_warning_pending(&self) -> bool {
        self.degraded_warning_pending.swap(false, Ordering::Relaxed)
    }

    fn record_requested(&self) {
        self.with_stats(|stats| {
            stats.requested = stats.requested.saturating_add(1);
        });
    }

    fn record_enqueued(&self) {
        self.with_stats(|stats| {
            stats.enqueued = stats.enqueued.saturating_add(1);
        });
    }

    fn record_coalesced(&self) {
        self.with_stats(|stats| {
            stats.coalesced = stats.coalesced.saturating_add(1);
        });
    }

    fn record_executed(&self) {
        self.with_stats(|stats| {
            stats.executed = stats.executed.saturating_add(1);
        });
    }

    fn record_wakeup_failure(&self) {
        self.with_stats(|stats| {
            stats.wakeup_failures = stats.wakeup_failures.saturating_add(1);
        });
    }

    fn record_enqueue_failure(&self) {
        self.with_stats(|stats| {
            stats.enqueue_failures = stats.enqueue_failures.saturating_add(1);
        });
    }

    fn pop_result(&self) -> Option<WeztermCommandResult> {
        self.with_completed_queue(VecDeque::pop_front)
    }

    fn on_worker_result(&self, result: WeztermCommandResult) {
        self.record_executed();
        self.with_completed_queue(|queue| {
            queue.push_back(result);
        });

        if self.mode() != WeztermRuntimeMode::HealthyAsync {
            return;
        }

        let Some(wakeup) = &self.wakeup else {
            self.transition_to_degraded_polling();
            return;
        };

        if wakeup.send().is_err() {
            self.record_wakeup_failure();
            self.transition_to_degraded_polling();
        }
    }

    fn snapshot(&self) -> WeztermSyncSnapshot {
        WeztermSyncSnapshot {
            mode: self.mode(),
            stats: self.with_stats(|stats| *stats),
        }
    }
}

struct WeztermDispatcher {
    sender: mpsc::SyncSender<WeztermCommand>,
    shared: Arc<WeztermDispatcherShared>,
}

trait WeztermCommandRunner: Send + Sync {
    fn run_tab_title(&self, title: &TabTitle) -> std::io::Result<ExitStatus>;
    fn run_working_dir(&self, cwd: &str) -> std::io::Result<ExitStatus>;
}

struct SystemWeztermCommandRunner;

impl WeztermCommandRunner for SystemWeztermCommandRunner {
    fn run_tab_title(&self, title: &TabTitle) -> std::io::Result<ExitStatus> {
        Command::new("wezterm")
            .args(["cli", "set-tab-title", title.as_str()])
            .status()
    }

    fn run_working_dir(&self, cwd: &str) -> std::io::Result<ExitStatus> {
        Command::new("wezterm")
            .args(["set-working-directory", cwd])
            .status()
    }
}

impl WeztermDispatcher {
    fn new() -> Self {
        let wakeup = match AsyncHandle::new(|| {
            let _ = crate::run_autocmd("wezterm_drain", drain_wezterm_completions);
            Ok::<(), nvim_oxi::Error>(())
        }) {
            Ok(handle) => Some(handle),
            Err(err) => {
                notify::warn(
                    WEZTERM_LOG_CONTEXT,
                    &format!("failed to initialize wezterm async dispatcher: {err}"),
                );
                None
            }
        };

        let initial_mode = if wakeup.is_some() {
            WeztermRuntimeMode::HealthyAsync
        } else {
            WeztermRuntimeMode::DegradedPolling
        };
        let shared = Arc::new(WeztermDispatcherShared::new(wakeup, initial_mode));
        let runner: Arc<dyn WeztermCommandRunner> = Arc::new(SystemWeztermCommandRunner);

        let (sender, receiver) = mpsc::sync_channel(WEZTERM_WORKER_QUEUE_CAPACITY);
        let worker_shared = Arc::clone(&shared);
        let worker_runner = Arc::clone(&runner);
        if let Err(err) = thread::Builder::new()
            .name("wezterm-sync-worker".to_string())
            .spawn(move || run_wezterm_worker(receiver, worker_shared, worker_runner))
        {
            shared.record_enqueue_failure();
            notify::warn(
                WEZTERM_LOG_CONTEXT,
                &format!("failed to spawn wezterm worker: {err}"),
            );
        }

        Self { sender, shared }
    }

    fn dispatch(&self, command: WeztermCommand) -> std::io::Result<()> {
        self.shared.record_requested();

        match self.sender.try_send(command) {
            Ok(()) => {
                self.shared.record_enqueued();
                Ok(())
            }
            Err(TrySendError::Full(_)) => {
                self.shared.record_enqueue_failure();
                Err(Error::new(
                    ErrorKind::WouldBlock,
                    "wezterm worker queue is full",
                ))
            }
            Err(TrySendError::Disconnected(_)) => {
                self.shared.record_enqueue_failure();
                Err(Error::new(
                    ErrorKind::BrokenPipe,
                    "wezterm worker channel disconnected",
                ))
            }
        }
    }

    fn pop_result(&self) -> Option<WeztermCommandResult> {
        self.shared.pop_result()
    }

    fn take_degraded_warning_pending(&self) -> bool {
        self.shared.take_degraded_warning_pending()
    }

    fn mark_coalesced(&self) {
        self.shared.record_coalesced();
    }

    fn snapshot(&self) -> WeztermSyncSnapshot {
        self.shared.snapshot()
    }
}

fn run_wezterm_worker(
    receiver: mpsc::Receiver<WeztermCommand>,
    shared: Arc<WeztermDispatcherShared>,
    runner: Arc<dyn WeztermCommandRunner>,
) {
    while let Ok(command) = receiver.recv() {
        shared.on_worker_result(run_wezterm_command(command, runner.as_ref()));
    }
}

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

fn wezterm_cli_available_at_startup() -> bool {
    match Command::new("wezterm").arg("--version").status() {
        Ok(status) if status.success() => true,
        Ok(status) => {
            notify::warn(
                WEZTERM_LOG_CONTEXT,
                &format!("wezterm --version exited with status {status}; disabling integration"),
            );
            false
        }
        Err(err) => {
            notify::warn(
                WEZTERM_LOG_CONTEXT,
                &format!("wezterm command unavailable at startup: {err}; disabling integration"),
            );
            false
        }
    }
}

fn wezterm_state_lock() -> nvim_oxi_utils::state::StateGuard<'static, WeztermState> {
    WEZTERM_STATE.lock_recover(|state| {
        notify::warn(
            WEZTERM_LOG_CONTEXT,
            "state mutex poisoned; resetting wezterm state",
        );
        *state = WeztermState::default();
    })
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
    let root = match buf.get_var::<NvimString>(PROJECT_ROOT_VAR) {
        Ok(value) => value,
        Err(_) => return Ok(None),
    };
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

fn run_wezterm_command(
    command: WeztermCommand,
    runner: &dyn WeztermCommandRunner,
) -> WeztermCommandResult {
    match command {
        WeztermCommand::SetTabTitle(title) => WeztermCommandResult::TabTitle {
            status: runner.run_tab_title(&title),
            title,
        },
        WeztermCommand::SetWorkingDir(cwd) => WeztermCommandResult::WorkingDir {
            status: runner.run_working_dir(&cwd),
            cwd,
        },
    }
}

fn command_error_result(command: WeztermCommand, err: std::io::Error) -> WeztermCommandResult {
    match command {
        WeztermCommand::SetTabTitle(title) => WeztermCommandResult::TabTitle {
            title,
            status: Err(err),
        },
        WeztermCommand::SetWorkingDir(cwd) => WeztermCommandResult::WorkingDir {
            cwd,
            status: Err(err),
        },
    }
}

fn on_wezterm_tab_title_result(
    title: TabTitle,
    status: std::io::Result<ExitStatus>,
) -> Option<WeztermCommand> {
    let mut state = wezterm_state_lock();
    let completion = match status {
        Ok(exit_status) => {
            if exit_status.success() {
                WeztermCompletion::Success
            } else {
                warn_title_failed(&mut state, &format_cli_failure(exit_status));
                WeztermCompletion::Failed
            }
        }
        Err(err) => {
            if err.kind() == ErrorKind::NotFound {
                warn_cli_unavailable(&mut state, &err);
                WeztermCompletion::Unavailable
            } else {
                warn_title_failed(&mut state, &format!("wezterm cli failed: {err}"));
                WeztermCompletion::Failed
            }
        }
    };
    state
        .reduce(WeztermEvent::TitleCompleted { title, completion })
        .command
}

fn on_wezterm_working_dir_result(
    cwd: String,
    status: std::io::Result<ExitStatus>,
) -> Option<WeztermCommand> {
    let mut state = wezterm_state_lock();
    let completion = match status {
        Ok(exit_status) => {
            if exit_status.success() {
                WeztermCompletion::Success
            } else {
                warn_cwd_failed(&mut state, &format_set_working_dir_failure(exit_status));
                WeztermCompletion::Failed
            }
        }
        Err(err) => {
            if err.kind() == ErrorKind::NotFound {
                warn_cli_unavailable(&mut state, &err);
                WeztermCompletion::Unavailable
            } else {
                warn_cwd_failed(
                    &mut state,
                    &format!("wezterm set-working-directory failed: {err}"),
                );
                WeztermCompletion::Failed
            }
        }
    };
    state
        .reduce(WeztermEvent::WorkingDirCompleted { cwd, completion })
        .command
}

fn on_wezterm_command_completion(result: WeztermCommandResult) {
    let next_command = match result {
        WeztermCommandResult::TabTitle { title, status } => {
            on_wezterm_tab_title_result(title, status)
        }
        WeztermCommandResult::WorkingDir { cwd, status } => {
            on_wezterm_working_dir_result(cwd, status)
        }
    };

    if let Some(next_command) = next_command {
        start_wezterm_update(next_command);
    }
}

fn start_wezterm_update(command: WeztermCommand) {
    let command_for_error = command.clone();
    if let Err(err) = WEZTERM_DISPATCHER.dispatch(command) {
        on_wezterm_command_completion(command_error_result(command_for_error, err));
    }
}

fn drain_wezterm_completions() -> Result<AutocmdAction> {
    if WEZTERM_DISPATCHER.take_degraded_warning_pending() {
        notify::warn(
            WEZTERM_LOG_CONTEXT,
            "wezterm async wakeup failed; falling back to autocmd polling",
        );
    }

    while let Some(result) = WEZTERM_DISPATCHER.pop_result() {
        on_wezterm_command_completion(result);
    }

    Ok(AutocmdAction::Keep)
}

fn update_wezterm_tab_title(context: &WeztermContext) -> Result<AutocmdAction> {
    let root = current_buf_project_root()?;
    let Some(title) = derive_tab_title(root, context.home.as_deref()) else {
        return Ok(AutocmdAction::Keep);
    };

    let next_command = {
        let mut state = wezterm_state_lock();
        state.reduce(WeztermEvent::RequestTitle { title }).command
    };
    if next_command.is_none() {
        WEZTERM_DISPATCHER.mark_coalesced();
    }
    if let Some(next_command) = next_command {
        start_wezterm_update(next_command);
    }
    Ok(AutocmdAction::Keep)
}

fn update_wezterm_working_dir() -> Result<AutocmdAction> {
    let Some(cwd) = current_window_cwd()? else {
        return Ok(AutocmdAction::Keep);
    };

    let next_command = {
        let mut state = wezterm_state_lock();
        state
            .reduce(WeztermEvent::RequestWorkingDir { cwd })
            .command
    };
    if next_command.is_none() {
        WEZTERM_DISPATCHER.mark_coalesced();
    }
    if let Some(next_command) = next_command {
        start_wezterm_update(next_command);
    }
    Ok(AutocmdAction::Keep)
}

fn should_skip_sync_for_current_buffer() -> Result<bool> {
    let current = api::get_current_buf();
    if !current.is_valid() {
        return Ok(true);
    }
    let buftype: NvimString =
        api::get_option_value("buftype", &OptionOpts::builder().buf(current).build())?;
    Ok(buftype.to_string_lossy() == "terminal")
}

fn sync_wezterm_state() -> Result<AutocmdAction> {
    let Some(context) = WeztermContext::detect() else {
        return Ok(AutocmdAction::Keep);
    };
    drain_wezterm_completions()?;
    if should_skip_sync_for_current_buffer()? {
        return Ok(AutocmdAction::Keep);
    }
    update_wezterm_tab_title(&context)?;
    update_wezterm_working_dir()?;
    Ok(AutocmdAction::Keep)
}

fn show_wezterm_sync_stats() -> Result<AutocmdAction> {
    let snapshot = WEZTERM_DISPATCHER.snapshot();
    notify::info(WEZTERM_LOG_CONTEXT, &snapshot.render());
    Ok(AutocmdAction::Keep)
}

fn setup_wezterm_commands() -> Result<()> {
    let stats_opts = CreateCommandOpts::builder()
        .force(true)
        .desc("Show WezTerm sync stats")
        .build();
    api::create_user_command(
        "WeztermSyncStats",
        |_args: CommandArgs| {
            let _ = crate::run_autocmd("wezterm_sync_stats", show_wezterm_sync_stats);
        },
        &stats_opts,
    )?;

    let sync_now_opts = CreateCommandOpts::builder()
        .force(true)
        .desc("Force WezTerm sync")
        .build();
    api::create_user_command(
        "WeztermSyncNow",
        |_args: CommandArgs| {
            let _ = crate::run_autocmd("wezterm_sync_now", sync_wezterm_state);
        },
        &sync_now_opts,
    )?;

    Ok(())
}

pub fn setup_wezterm_autocmd() -> Result<()> {
    if WeztermContext::detect().is_none() {
        return Ok(());
    }
    if !wezterm_cli_available_at_startup() {
        return Ok(());
    }

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
    setup_wezterm_commands()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque as StdVecDeque;
    type TestResult<T = ()> = std::result::Result<T, &'static str>;

    fn shared(mode: WeztermRuntimeMode) -> WeztermDispatcherShared {
        WeztermDispatcherShared::new(None, mode)
    }

    fn title(value: &str) -> TestResult<TabTitle> {
        TabTitle::try_new(value.to_string()).map_err(|_| "expected non-empty tab title")
    }

    #[cfg(unix)]
    fn exit_status_from_code(code: i32) -> ExitStatus {
        use std::os::unix::process::ExitStatusExt;
        ExitStatus::from_raw(code << 8)
    }

    #[derive(Debug, Clone, Copy)]
    enum RunnerOutcome {
        Exit(i32),
        Io(ErrorKind),
    }

    impl RunnerOutcome {
        fn into_result(self) -> std::io::Result<ExitStatus> {
            match self {
                Self::Exit(code) => Ok(exit_status_from_code(code)),
                Self::Io(kind) => Err(Error::new(kind, "scripted runner error")),
            }
        }
    }

    struct ScriptedRunner {
        title: Mutex<StdVecDeque<RunnerOutcome>>,
        cwd: Mutex<StdVecDeque<RunnerOutcome>>,
    }

    impl ScriptedRunner {
        fn new(title: Vec<RunnerOutcome>, cwd: Vec<RunnerOutcome>) -> Self {
            Self {
                title: Mutex::new(StdVecDeque::from(title)),
                cwd: Mutex::new(StdVecDeque::from(cwd)),
            }
        }
    }

    impl WeztermCommandRunner for ScriptedRunner {
        fn run_tab_title(&self, _title: &TabTitle) -> std::io::Result<ExitStatus> {
            match self.title.lock() {
                Ok(mut queue) => queue
                    .pop_front()
                    .unwrap_or(RunnerOutcome::Io(ErrorKind::Other))
                    .into_result(),
                Err(poisoned) => {
                    let mut queue = poisoned.into_inner();
                    queue
                        .pop_front()
                        .unwrap_or(RunnerOutcome::Io(ErrorKind::Other))
                        .into_result()
                }
            }
        }

        fn run_working_dir(&self, _cwd: &str) -> std::io::Result<ExitStatus> {
            match self.cwd.lock() {
                Ok(mut queue) => queue
                    .pop_front()
                    .unwrap_or(RunnerOutcome::Io(ErrorKind::Other))
                    .into_result(),
                Err(poisoned) => {
                    let mut queue = poisoned.into_inner();
                    queue
                        .pop_front()
                        .unwrap_or(RunnerOutcome::Io(ErrorKind::Other))
                        .into_result()
                }
            }
        }
    }

    #[test]
    fn degraded_transition_warns_once() {
        let shared = shared(WeztermRuntimeMode::HealthyAsync);
        assert_eq!(shared.mode(), WeztermRuntimeMode::HealthyAsync);
        assert!(!shared.take_degraded_warning_pending());

        shared.transition_to_degraded_polling();
        assert_eq!(shared.mode(), WeztermRuntimeMode::DegradedPolling);
        assert!(shared.take_degraded_warning_pending());
        assert!(!shared.take_degraded_warning_pending());

        shared.transition_to_degraded_polling();
        assert!(!shared.take_degraded_warning_pending());
    }

    #[test]
    fn worker_result_without_wakeup_degrades_and_queues() {
        let shared = shared(WeztermRuntimeMode::HealthyAsync);
        shared.on_worker_result(WeztermCommandResult::WorkingDir {
            cwd: "/tmp".to_string(),
            status: Err(Error::other("synthetic failure")),
        });

        assert_eq!(shared.mode(), WeztermRuntimeMode::DegradedPolling);
        assert!(shared.pop_result().is_some());
        let snapshot = shared.snapshot();
        assert_eq!(snapshot.stats.executed, 1);
    }

    #[test]
    fn worker_uses_injected_runner_and_preserves_command_order() -> TestResult {
        let shared = Arc::new(shared(WeztermRuntimeMode::DegradedPolling));
        let (sender, receiver) = mpsc::sync_channel(4);

        let first_title = title("first")?;
        sender
            .send(WeztermCommand::SetTabTitle(first_title.clone()))
            .map_err(|_| "failed to send title command")?;
        sender
            .send(WeztermCommand::SetWorkingDir("/tmp".to_string()))
            .map_err(|_| "failed to send cwd command")?;
        drop(sender);

        let runner: Arc<dyn WeztermCommandRunner> = Arc::new(ScriptedRunner::new(
            vec![RunnerOutcome::Exit(0)],
            vec![RunnerOutcome::Io(ErrorKind::Other)],
        ));

        run_wezterm_worker(receiver, Arc::clone(&shared), runner);

        let Some(first) = shared.pop_result() else {
            return Err("missing first worker result");
        };
        match first {
            WeztermCommandResult::TabTitle { title, status } => {
                assert_eq!(title, first_title);
                assert!(status.is_ok());
            }
            WeztermCommandResult::WorkingDir { .. } => {
                return Err("expected title result first");
            }
        }

        let Some(second) = shared.pop_result() else {
            return Err("missing second worker result");
        };
        match second {
            WeztermCommandResult::WorkingDir { cwd, status } => {
                assert_eq!(cwd, "/tmp");
                let err = match status {
                    Ok(_) => return Err("expected cwd failure"),
                    Err(err) => err,
                };
                assert_eq!(err.kind(), ErrorKind::Other);
            }
            WeztermCommandResult::TabTitle { .. } => {
                return Err("expected cwd result second");
            }
        }

        let snapshot = shared.snapshot();
        assert_eq!(snapshot.stats.executed, 2);
        Ok(())
    }

    #[test]
    fn dispatch_reports_queue_backpressure() -> TestResult {
        let shared = Arc::new(shared(WeztermRuntimeMode::DegradedPolling));
        let (sender, _receiver) = mpsc::sync_channel(1);
        let dispatcher = WeztermDispatcher { sender, shared };

        dispatcher
            .dispatch(WeztermCommand::SetWorkingDir("/tmp".to_string()))
            .map_err(|_| "expected first dispatch to enqueue")?;

        let second = dispatcher.dispatch(WeztermCommand::SetWorkingDir("/var".to_string()));
        let err = match second {
            Ok(()) => return Err("expected queue backpressure"),
            Err(err) => err,
        };
        assert_eq!(err.kind(), ErrorKind::WouldBlock);

        let snapshot = dispatcher.snapshot();
        assert_eq!(snapshot.stats.requested, 2);
        assert_eq!(snapshot.stats.enqueued, 1);
        assert_eq!(snapshot.stats.enqueue_failures, 1);
        Ok(())
    }

    #[test]
    fn stats_counters_accumulate() {
        let shared = shared(WeztermRuntimeMode::HealthyAsync);
        shared.record_requested();
        shared.record_enqueued();
        shared.record_coalesced();
        shared.record_executed();
        shared.record_wakeup_failure();
        shared.record_enqueue_failure();

        let snapshot = shared.snapshot();
        assert_eq!(snapshot.stats.requested, 1);
        assert_eq!(snapshot.stats.enqueued, 1);
        assert_eq!(snapshot.stats.coalesced, 1);
        assert_eq!(snapshot.stats.executed, 1);
        assert_eq!(snapshot.stats.wakeup_failures, 1);
        assert_eq!(snapshot.stats.enqueue_failures, 1);
    }
}
