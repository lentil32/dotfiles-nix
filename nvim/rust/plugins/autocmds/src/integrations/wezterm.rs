use std::collections::VecDeque;
use std::io::{Error, ErrorKind, Write};
use std::path::PathBuf;
use std::process::ExitStatus;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, TrySendError};
use std::sync::{Arc, LazyLock, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::machines::wezterm::{
    WeztermCommand, WeztermCompletion, WeztermEvent, WeztermState, derive_tab_title,
    format_cli_failure, format_set_working_dir_failure,
};
use nvim_oxi::api;
use nvim_oxi::api::opts::{CreateAugroupOpts, CreateAutocmdOpts, CreateCommandOpts, OptionOpts};
use nvim_oxi::api::types::{AutocmdCallbackArgs, CommandArgs};
use nvim_oxi::libuv::AsyncHandle;
use nvim_oxi::{Array, Result, String as NvimString};
use nvim_oxi_utils::{notify, state::StateCell};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, percent_encode};
use support::{ProjectRoot, TabTitle};

use crate::types::AutocmdAction;

const WEZTERM_LOG_CONTEXT: &str = "wezterm_tab";
const PROJECT_ROOT_VAR: &str = "project_root";
const WEZTERM_WORKER_QUEUE_CAPACITY: usize = 64;
const WEZTERM_DEFAULT_COMPLETION_DRAIN_BATCH_SIZE: usize = 16;
const WEZTERM_DEFAULT_SYNC_DEBOUNCE_WINDOW_MS: u64 = 75;
const FILE_URL_PATH_ENCODE_SET: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'/')
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');

static WEZTERM_STATE: StateCell<WeztermState> = StateCell::new(WeztermState::new());
static WEZTERM_DISPATCHER: LazyLock<WeztermDispatcher> = LazyLock::new(WeztermDispatcher::new);
static WEZTERM_SYNC_POLICY: LazyLock<WeztermSyncPolicy> =
    LazyLock::new(WeztermSyncPolicy::default_policy);
static WEZTERM_SYNC_GATE: LazyLock<Mutex<WeztermSyncGate>> =
    LazyLock::new(|| Mutex::new(WeztermSyncGate::new()));

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
enum WeztermCommandKind {
    TabTitle,
    WorkingDir,
}

impl WeztermCommand {
    const fn kind(&self) -> WeztermCommandKind {
        match self {
            Self::SetTabTitle(_) => WeztermCommandKind::TabTitle,
            Self::SetWorkingDir(_) => WeztermCommandKind::WorkingDir,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WeztermCommandBatch {
    first: WeztermCommand,
    second: Option<WeztermCommand>,
}

impl WeztermCommandBatch {
    fn single(command: WeztermCommand) -> Self {
        Self {
            first: command,
            second: None,
        }
    }

    fn pair(first: WeztermCommand, second: WeztermCommand) -> Self {
        debug_assert!(
            first.kind() != second.kind(),
            "batch pairs are expected to contain distinct command kinds"
        );
        Self {
            first,
            second: Some(second),
        }
    }

    fn from_optional(
        first: Option<WeztermCommand>,
        second: Option<WeztermCommand>,
    ) -> Option<Self> {
        match (first, second) {
            (Some(first), Some(second)) => Some(Self::pair(first, second)),
            (Some(first), None) | (None, Some(first)) => Some(Self::single(first)),
            (None, None) => None,
        }
    }

    fn for_each<F>(self, mut f: F)
    where
        F: FnMut(WeztermCommand),
    {
        f(self.first);
        if let Some(second) = self.second {
            f(second);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum WeztermWorkItem {
    Single(WeztermCommand),
    Batch(WeztermCommandBatch),
}

impl WeztermWorkItem {
    fn from_command(command: WeztermCommand) -> Self {
        Self::Single(command)
    }

    fn from_batch(batch: WeztermCommandBatch) -> Self {
        if batch.second.is_none() {
            Self::Single(batch.first)
        } else {
            Self::Batch(batch)
        }
    }

    fn for_each_command<F>(self, mut f: F)
    where
        F: FnMut(WeztermCommand),
    {
        match self {
            Self::Single(command) => f(command),
            Self::Batch(batch) => batch.for_each(f),
        }
    }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CompletionDrainBatchSize(usize);

impl CompletionDrainBatchSize {
    const MIN: Self = Self(1);

    const fn get(self) -> usize {
        self.0
    }

    const fn try_new(value: usize) -> Option<Self> {
        if value == 0 { None } else { Some(Self(value)) }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WeztermSyncPolicy {
    completion_drain_batch_size: CompletionDrainBatchSize,
    autocmd_debounce_window: Duration,
}

impl WeztermSyncPolicy {
    const fn try_new(
        completion_drain_batch_size: usize,
        autocmd_debounce_window: Duration,
    ) -> Option<Self> {
        let Some(completion_drain_batch_size) =
            CompletionDrainBatchSize::try_new(completion_drain_batch_size)
        else {
            return None;
        };
        Some(Self {
            completion_drain_batch_size,
            autocmd_debounce_window,
        })
    }

    fn default_policy() -> Self {
        let debounce = Duration::from_millis(WEZTERM_DEFAULT_SYNC_DEBOUNCE_WINDOW_MS);
        match Self::try_new(WEZTERM_DEFAULT_COMPLETION_DRAIN_BATCH_SIZE, debounce) {
            Some(policy) => policy,
            None => Self {
                completion_drain_batch_size: CompletionDrainBatchSize::MIN,
                autocmd_debounce_window: debounce,
            },
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct WeztermSyncGate {
    last_sync_started_at: Option<Instant>,
}

impl WeztermSyncGate {
    const fn new() -> Self {
        Self {
            last_sync_started_at: None,
        }
    }

    fn should_coalesce(&mut self, now: Instant, debounce_window: Duration) -> bool {
        let Some(last_sync_started_at) = self.last_sync_started_at else {
            self.last_sync_started_at = Some(now);
            return false;
        };
        if now.saturating_duration_since(last_sync_started_at) < debounce_window {
            return true;
        }
        self.last_sync_started_at = Some(now);
        false
    }
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

    fn has_completed_results(&self) -> bool {
        self.with_completed_queue(|queue| !queue.is_empty())
    }

    fn request_async_wakeup(&self) {
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

    fn on_worker_result(&self, result: WeztermCommandResult) {
        self.on_worker_results(std::iter::once(result));
    }

    fn on_worker_results<I>(&self, results: I)
    where
        I: IntoIterator<Item = WeztermCommandResult>,
    {
        let mut executed = 0u64;
        self.with_completed_queue(|queue| {
            for result in results {
                queue.push_back(result);
                executed = executed.saturating_add(1);
            }
        });
        if executed == 0 {
            return;
        }
        self.with_stats(|stats| {
            stats.executed = stats.executed.saturating_add(executed);
        });
        self.request_async_wakeup();
    }

    fn snapshot(&self) -> WeztermSyncSnapshot {
        WeztermSyncSnapshot {
            mode: self.mode(),
            stats: self.with_stats(|stats| *stats),
        }
    }
}

struct WeztermDispatcher {
    sender: mpsc::SyncSender<WeztermWorkItem>,
    shared: Arc<WeztermDispatcherShared>,
}

#[derive(Debug)]
enum WeztermDispatchError {
    QueueFull(WeztermWorkItem),
    WorkerDisconnected(WeztermWorkItem),
}

impl WeztermDispatchError {
    fn into_parts(self) -> (WeztermWorkItem, std::io::Error) {
        match self {
            Self::QueueFull(command) => (
                command,
                Error::new(ErrorKind::WouldBlock, "wezterm worker queue is full"),
            ),
            Self::WorkerDisconnected(command) => (
                command,
                Error::new(ErrorKind::BrokenPipe, "wezterm worker channel disconnected"),
            ),
        }
    }
}

trait WeztermCommandRunner: Send + Sync {
    fn run_tab_title(&self, title: &TabTitle) -> std::io::Result<ExitStatus>;
    fn run_working_dir(&self, cwd: &str) -> std::io::Result<ExitStatus>;
}

#[cfg(unix)]
fn successful_exit_status() -> ExitStatus {
    use std::os::unix::process::ExitStatusExt;
    ExitStatus::from_raw(0)
}

#[cfg(windows)]
fn successful_exit_status() -> ExitStatus {
    use std::os::windows::process::ExitStatusExt;
    ExitStatus::from_raw(0)
}

fn tmux_passthrough_enabled() -> bool {
    std::env::var_os("TMUX").is_some()
}

fn with_tmux_passthrough(osc: &str, passthrough_enabled: bool) -> String {
    if !passthrough_enabled {
        return osc.to_string();
    }
    let mut tmux_passthrough = String::from("\u{1b}Ptmux;");
    for ch in osc.chars() {
        if ch == '\u{1b}' {
            tmux_passthrough.push(ch);
        }
        tmux_passthrough.push(ch);
    }
    tmux_passthrough.push_str("\u{1b}\\");
    tmux_passthrough.push_str(osc);
    tmux_passthrough
}

fn build_tab_title_sequence(title: &TabTitle) -> String {
    let osc = format!("\u{1b}]1;{}\u{1b}\\", title.as_str());
    with_tmux_passthrough(&osc, tmux_passthrough_enabled())
}

fn build_working_dir_sequence(cwd: &str) -> std::io::Result<String> {
    let cwd = std::path::Path::new(cwd);
    if !cwd.is_absolute() {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            format!("cwd {cwd:?} is not an absolute path"),
        ));
    }
    let host = hostname::get()
        .ok()
        .and_then(|host| host.into_string().ok())
        .filter(|host| !host.is_empty())
        .unwrap_or_else(|| "localhost".to_string());
    #[cfg(unix)]
    let encoded_path = {
        use std::os::unix::ffi::OsStrExt;
        percent_encode(cwd.as_os_str().as_bytes(), FILE_URL_PATH_ENCODE_SET).to_string()
    };
    #[cfg(windows)]
    let encoded_path = {
        let normalized = cwd.to_string_lossy().replace('\\', "/");
        percent_encode(normalized.as_bytes(), FILE_URL_PATH_ENCODE_SET).to_string()
    };

    let osc = format!("\u{1b}]7;file://{host}{encoded_path}\u{1b}\\");
    Ok(with_tmux_passthrough(&osc, tmux_passthrough_enabled()))
}

#[derive(Debug, Default)]
struct EscapeSequenceWeztermCommandRunner;

impl WeztermCommandRunner for EscapeSequenceWeztermCommandRunner {
    fn run_tab_title(&self, title: &TabTitle) -> std::io::Result<ExitStatus> {
        let sequence = build_tab_title_sequence(title);
        let mut stdout = std::io::stdout().lock();
        stdout.write_all(sequence.as_bytes())?;
        stdout.flush()?;
        Ok(successful_exit_status())
    }

    fn run_working_dir(&self, cwd: &str) -> std::io::Result<ExitStatus> {
        let sequence = build_working_dir_sequence(cwd)?;
        let mut stdout = std::io::stdout().lock();
        stdout.write_all(sequence.as_bytes())?;
        stdout.flush()?;
        Ok(successful_exit_status())
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
        let runner: Arc<dyn WeztermCommandRunner> = Arc::new(EscapeSequenceWeztermCommandRunner);

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

    fn dispatch(&self, command: WeztermWorkItem) -> std::result::Result<(), WeztermDispatchError> {
        self.shared.record_requested();

        match self.sender.try_send(command) {
            Ok(()) => {
                self.shared.record_enqueued();
                Ok(())
            }
            Err(TrySendError::Full(command)) => {
                self.shared.record_enqueue_failure();
                Err(WeztermDispatchError::QueueFull(command))
            }
            Err(TrySendError::Disconnected(command)) => {
                self.shared.record_enqueue_failure();
                Err(WeztermDispatchError::WorkerDisconnected(command))
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

    fn has_pending_results(&self) -> bool {
        self.shared.has_completed_results()
    }

    fn request_async_drain(&self) {
        self.shared.request_async_wakeup();
    }
}

fn run_wezterm_worker(
    receiver: mpsc::Receiver<WeztermWorkItem>,
    shared: Arc<WeztermDispatcherShared>,
    runner: Arc<dyn WeztermCommandRunner>,
) {
    while let Ok(work_item) = receiver.recv() {
        run_wezterm_work_item(work_item, runner.as_ref(), &shared);
    }
}

fn run_wezterm_work_item(
    work_item: WeztermWorkItem,
    runner: &dyn WeztermCommandRunner,
    shared: &WeztermDispatcherShared,
) {
    match work_item {
        WeztermWorkItem::Single(command) => {
            shared.on_worker_result(run_wezterm_command(command, runner));
        }
        WeztermWorkItem::Batch(WeztermCommandBatch { first, second }) => {
            if let Some(second) = second {
                let first_result = run_wezterm_command(first, runner);
                let second_result = run_wezterm_command(second, runner);
                shared.on_worker_results([first_result, second_result]);
            } else {
                shared.on_worker_result(run_wezterm_command(first, runner));
            }
        }
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

fn wezterm_state_lock() -> nvim_oxi_utils::state::StateGuard<'static, WeztermState> {
    WEZTERM_STATE.lock_recover(|state| {
        notify::warn(
            WEZTERM_LOG_CONTEXT,
            "state mutex poisoned; resetting wezterm state",
        );
        *state = WeztermState::default();
    })
}

fn with_sync_gate<R, F>(f: F) -> R
where
    F: FnOnce(&mut WeztermSyncGate) -> R,
{
    match WEZTERM_SYNC_GATE.lock() {
        Ok(mut gate) => f(&mut gate),
        Err(poisoned) => {
            let mut gate = poisoned.into_inner();
            f(&mut gate)
        }
    }
}

fn should_coalesce_sync(now: Instant) -> bool {
    with_sync_gate(|gate| gate.should_coalesce(now, WEZTERM_SYNC_POLICY.autocmd_debounce_window))
}

fn warn_cli_unavailable(state: &mut WeztermState, err: &std::io::Error) {
    if !state.take_warn_cli_unavailable() {
        return;
    }
    let message = format!("wezterm command unavailable: {err}");
    notify::warn(WEZTERM_LOG_CONTEXT, &message);
}

fn warn_title_failed<F>(state: &mut WeztermState, message: F)
where
    F: FnOnce() -> String,
{
    if state.take_warn_title_failed() {
        notify::warn(WEZTERM_LOG_CONTEXT, &message());
    }
}

fn warn_cwd_failed<F>(state: &mut WeztermState, message: F)
where
    F: FnOnce() -> String,
{
    if state.take_warn_cwd_failed() {
        notify::warn(WEZTERM_LOG_CONTEXT, &message());
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
                warn_title_failed(&mut state, || format_cli_failure(exit_status));
                WeztermCompletion::Failed
            }
        }
        Err(err) => {
            if err.kind() == ErrorKind::NotFound {
                warn_cli_unavailable(&mut state, &err);
                WeztermCompletion::Unavailable
            } else {
                warn_title_failed(&mut state, || format!("wezterm cli failed: {err}"));
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
                warn_cwd_failed(&mut state, || format_set_working_dir_failure(exit_status));
                WeztermCompletion::Failed
            }
        }
        Err(err) => {
            if err.kind() == ErrorKind::NotFound {
                warn_cli_unavailable(&mut state, &err);
                WeztermCompletion::Unavailable
            } else {
                warn_cwd_failed(&mut state, || {
                    format!("wezterm set-working-directory failed: {err}")
                });
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

fn on_wezterm_dispatch_error(work_item: WeztermWorkItem, err: std::io::Error) {
    let err_kind = err.kind();
    let err_message = err.to_string();
    if err_kind == ErrorKind::WouldBlock {
        WEZTERM_DISPATCHER.mark_coalesced();
    }
    work_item.for_each_command(|command| {
        let error = Error::new(err_kind, err_message.clone());
        on_wezterm_command_completion(command_error_result(command, error));
    });
}

fn start_wezterm_work_item(work_item: WeztermWorkItem) {
    if let Err(err) = WEZTERM_DISPATCHER.dispatch(work_item) {
        let (work_item, err) = err.into_parts();
        on_wezterm_dispatch_error(work_item, err);
    }
}

fn start_wezterm_update(command: WeztermCommand) {
    start_wezterm_work_item(WeztermWorkItem::from_command(command));
}

fn start_wezterm_batch(batch: WeztermCommandBatch) {
    start_wezterm_work_item(WeztermWorkItem::from_batch(batch));
}

fn drain_wezterm_completions() -> Result<AutocmdAction> {
    if WEZTERM_DISPATCHER.take_degraded_warning_pending() {
        notify::warn(
            WEZTERM_LOG_CONTEXT,
            "wezterm async wakeup failed; falling back to autocmd polling",
        );
    }

    let completion_drain_batch_size = WEZTERM_SYNC_POLICY.completion_drain_batch_size.get();
    let mut drained = 0usize;
    while drained < completion_drain_batch_size {
        let Some(result) = WEZTERM_DISPATCHER.pop_result() else {
            break;
        };
        on_wezterm_command_completion(result);
        drained += 1;
    }

    if drained == completion_drain_batch_size && WEZTERM_DISPATCHER.has_pending_results() {
        WEZTERM_DISPATCHER.request_async_drain();
    }

    Ok(AutocmdAction::Keep)
}

fn next_wezterm_tab_title_command(context: &WeztermContext) -> Result<Option<WeztermCommand>> {
    let root = current_buf_project_root()?;
    let Some(title) = derive_tab_title(root, context.home.as_deref()) else {
        return Ok(None);
    };

    let next_command = {
        let mut state = wezterm_state_lock();
        state.reduce(WeztermEvent::RequestTitle { title }).command
    };
    if next_command.is_none() {
        WEZTERM_DISPATCHER.mark_coalesced();
    }
    Ok(next_command)
}

fn next_wezterm_working_dir_command() -> Result<Option<WeztermCommand>> {
    let Some(cwd) = current_window_cwd()? else {
        return Ok(None);
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
    Ok(next_command)
}

fn should_skip_sync_for_current_buffer() -> Result<bool> {
    let current = api::get_current_buf();
    if !current.is_valid() {
        return Ok(true);
    }
    let buftype: NvimString =
        api::get_option_value("buftype", &OptionOpts::builder().buf(current).build())?;
    Ok(!buftype_requires_wezterm_sync(&buftype.to_string_lossy()))
}

fn buftype_requires_wezterm_sync(buftype: &str) -> bool {
    matches!(buftype, "" | "acwrite")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WeztermSyncTrigger {
    Autocmd,
    Manual,
}

impl WeztermSyncTrigger {
    const fn should_debounce(self) -> bool {
        matches!(self, Self::Autocmd)
    }
}

fn sync_wezterm_state_for(trigger: WeztermSyncTrigger) -> Result<AutocmdAction> {
    let Some(context) = WeztermContext::detect() else {
        return Ok(AutocmdAction::Keep);
    };
    drain_wezterm_completions()?;
    if should_skip_sync_for_current_buffer()? {
        return Ok(AutocmdAction::Keep);
    }
    if trigger.should_debounce() && should_coalesce_sync(Instant::now()) {
        WEZTERM_DISPATCHER.mark_coalesced();
        return Ok(AutocmdAction::Keep);
    }

    let title_command = next_wezterm_tab_title_command(&context)?;
    let cwd_command = next_wezterm_working_dir_command()?;
    if let Some(batch) = WeztermCommandBatch::from_optional(title_command, cwd_command) {
        start_wezterm_batch(batch);
    }
    Ok(AutocmdAction::Keep)
}

fn sync_wezterm_state() -> Result<AutocmdAction> {
    sync_wezterm_state_for(WeztermSyncTrigger::Autocmd)
}

fn sync_wezterm_state_now() -> Result<AutocmdAction> {
    sync_wezterm_state_for(WeztermSyncTrigger::Manual)
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
            let _ = crate::run_autocmd("wezterm_sync_now", sync_wezterm_state_now);
        },
        &sync_now_opts,
    )?;

    Ok(())
}

pub fn setup_wezterm_autocmd() -> Result<()> {
    if WeztermContext::detect().is_none() {
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
            .send(WeztermWorkItem::from_command(WeztermCommand::SetTabTitle(
                first_title.clone(),
            )))
            .map_err(|_| "failed to send title command")?;
        sender
            .send(WeztermWorkItem::from_command(
                WeztermCommand::SetWorkingDir("/tmp".to_string()),
            ))
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
            .dispatch(WeztermWorkItem::from_command(
                WeztermCommand::SetWorkingDir("/tmp".to_string()),
            ))
            .map_err(|_| "expected first dispatch to enqueue")?;

        let second = dispatcher.dispatch(WeztermWorkItem::from_command(
            WeztermCommand::SetWorkingDir("/var".to_string()),
        ));
        let err = match second {
            Ok(()) => return Err("expected queue backpressure"),
            Err(err) => err,
        };
        let (command, io_err) = err.into_parts();
        assert_eq!(
            command,
            WeztermWorkItem::from_command(WeztermCommand::SetWorkingDir("/var".to_string()))
        );
        assert_eq!(io_err.kind(), ErrorKind::WouldBlock);

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
        shared.on_worker_result(WeztermCommandResult::WorkingDir {
            cwd: "/tmp".to_string(),
            status: Ok(exit_status_from_code(0)),
        });
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

    #[test]
    fn buftype_requires_wezterm_sync_allows_normal_and_acwrite_buffers() {
        assert!(super::buftype_requires_wezterm_sync(""));
        assert!(super::buftype_requires_wezterm_sync("acwrite"));
    }

    #[test]
    fn buftype_requires_wezterm_sync_skips_special_ui_buffers() {
        assert!(!super::buftype_requires_wezterm_sync("terminal"));
        assert!(!super::buftype_requires_wezterm_sync("nofile"));
        assert!(!super::buftype_requires_wezterm_sync("prompt"));
    }

    #[test]
    fn sync_gate_coalesces_requests_within_window() {
        let start = Instant::now();
        let mut gate = WeztermSyncGate::new();
        let debounce_window = WEZTERM_SYNC_POLICY.autocmd_debounce_window;
        assert!(!gate.should_coalesce(start, debounce_window));
        assert!(gate.should_coalesce(start + Duration::from_millis(10), debounce_window));
        assert!(!gate.should_coalesce(start + debounce_window, debounce_window));
    }

    #[test]
    fn sync_policy_rejects_zero_drain_batch_size() {
        assert!(WeztermSyncPolicy::try_new(0, Duration::from_millis(1)).is_none());
    }

    #[test]
    fn command_batch_preserves_input_order() -> TestResult {
        let tab = title("batched")?;
        let first = WeztermCommand::SetTabTitle(tab.clone());
        let second = WeztermCommand::SetWorkingDir("/tmp".to_string());
        let Some(batch) =
            WeztermCommandBatch::from_optional(Some(first.clone()), Some(second.clone()))
        else {
            return Err("expected batch");
        };
        let mut seen = Vec::new();
        batch.for_each(|command| seen.push(command));
        assert_eq!(seen, vec![first, second]);
        Ok(())
    }

    #[test]
    fn with_tmux_passthrough_noop_when_disabled() {
        let osc = "\u{1b}]1;tab-title\u{1b}\\";
        assert_eq!(super::with_tmux_passthrough(osc, false), osc);
    }

    #[test]
    fn with_tmux_passthrough_wraps_when_enabled() {
        let osc = "\u{1b}]1;tab-title\u{1b}\\";
        let wrapped = super::with_tmux_passthrough(osc, true);
        assert!(wrapped.starts_with("\u{1b}Ptmux;"));
        assert!(wrapped.ends_with(osc));
    }

    #[test]
    fn working_dir_sequence_contains_osc7_payload() -> TestResult {
        let sequence = super::build_working_dir_sequence("/tmp")
            .map_err(|_| "expected valid OSC7 sequence")?;
        assert!(sequence.contains("]7;file://"));
        Ok(())
    }
}
