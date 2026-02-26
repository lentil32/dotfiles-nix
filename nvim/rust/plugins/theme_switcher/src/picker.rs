use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use crate::args::{CycleArgs, OpenArgs};
use crate::core::{
    ThemeCatalog, ThemeCycleDirection, ThemeIndex, ThemeSwitcherEffect, ThemeSwitcherEvent,
    ThemeSwitcherMachine, cycle_theme_index_from_index, resolve_effective_theme_index,
};
use nvim_oxi::api;
use nvim_oxi::api::opts::{CmdOpts, OptionOpts, OptionScope, SetKeymapOpts};
use nvim_oxi::api::types::{
    CmdInfos, Mode, WindowBorder, WindowConfig, WindowRelativeTo, WindowStyle,
};
use nvim_oxi::api::{Buffer, Window};
use nvim_oxi::{Dictionary, Result};
use nvim_oxi_utils::handles::{BufHandle, WinHandle};
use nvim_oxi_utils::notify;
use nvim_oxi_utils::state::{StateCell, StateGuard};
use nvim_oxi_utils::state_machine::Machine;

const LOG_CONTEXT: &str = "rs_theme_switcher";
const HELP_LINE: &str = "<C-n> next  <C-p> prev  <CR> confirm  <Esc>/q cancel";
const THEME_LINE_START: usize = 4;

#[derive(Debug, Clone)]
struct PickerSession {
    machine: ThemeSwitcherMachine,
    title: String,
    state_path: Option<PathBuf>,
    restore_colorscheme_on_cancel: Option<String>,
    buf_handle: BufHandle,
    win_handle: WinHandle,
}

impl PickerSession {
    fn theme_line_number_for(index: ThemeIndex) -> usize {
        THEME_LINE_START + index.raw()
    }

    fn theme_line_number(&self) -> usize {
        Self::theme_line_number_for(self.machine.cursor_index())
    }

    fn theme_line(&self, index: ThemeIndex, selected: bool) -> Option<String> {
        let prefix = if selected { "> " } else { "  " };
        self.machine
            .catalog()
            .get(index)
            .map(|theme| format!("{prefix}{}", theme.name().as_str()))
    }

    fn lines(&self) -> Vec<String> {
        let mut lines = Vec::with_capacity(self.machine.catalog().len() + (THEME_LINE_START - 1));
        lines.push(self.title.clone());
        lines.push(HELP_LINE.to_string());
        lines.push(String::new());
        for (raw_index, theme) in self.machine.catalog().iter().enumerate() {
            let selected = raw_index == self.machine.cursor_index().raw();
            let prefix = if selected { "> " } else { "  " };
            lines.push(format!("{prefix}{}", theme.name().as_str()));
        }
        lines
    }

    fn colorscheme_for_index(&self, index: ThemeIndex) -> Option<&str> {
        self.machine
            .catalog()
            .get(index)
            .map(|theme| theme.colorscheme().as_str())
    }

    fn has_valid_handles(&self) -> bool {
        self.buf_handle.valid_buffer().is_some() && self.win_handle.valid_window().is_some()
    }
}

#[derive(Debug)]
struct ThemeSwitcherContext {
    state: StateCell<Option<PickerSession>>,
}

impl ThemeSwitcherContext {
    fn new() -> Self {
        Self {
            state: StateCell::new(None),
        }
    }

    fn state_lock(&self) -> StateGuard<'_, Option<PickerSession>> {
        self.state.lock_recover(|state| {
            let had_session = state.is_some();
            if had_session {
                notify::warn(
                    LOG_CONTEXT,
                    "state mutex poisoned; dropping active theme-switcher session",
                );
            }
            *state = None;
        })
    }
}

#[derive(Debug)]
enum ThemeStateStoreError {
    CreateDirectory { path: PathBuf, message: String },
    ReadFile { path: PathBuf, message: String },
    WriteFile { path: PathBuf, message: String },
}

impl std::fmt::Display for ThemeStateStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CreateDirectory { path, message } => {
                write!(
                    f,
                    "failed to create persistence directory '{}': {message}",
                    path.display()
                )
            }
            Self::ReadFile { path, message } => {
                write!(
                    f,
                    "failed to read persistence file '{}': {message}",
                    path.display()
                )
            }
            Self::WriteFile { path, message } => {
                write!(
                    f,
                    "failed to write persistence file '{}': {message}",
                    path.display()
                )
            }
        }
    }
}

impl std::error::Error for ThemeStateStoreError {}

trait ThemeStateStore {
    fn read_colorscheme(
        &self,
        path: &Path,
    ) -> std::result::Result<Option<String>, ThemeStateStoreError>;

    fn write_colorscheme(
        &self,
        path: &Path,
        colorscheme: &str,
    ) -> std::result::Result<(), ThemeStateStoreError>;
}

#[derive(Debug, Clone, Copy, Default)]
struct FsThemeStateStore;

impl FsThemeStateStore {
    const fn new() -> Self {
        Self
    }
}

impl ThemeStateStore for FsThemeStateStore {
    fn read_colorscheme(
        &self,
        path: &Path,
    ) -> std::result::Result<Option<String>, ThemeStateStoreError> {
        let content = match fs::read_to_string(path) {
            Ok(content) => content,
            Err(err) if err.kind() == ErrorKind::NotFound => return Ok(None),
            Err(err) => {
                return Err(ThemeStateStoreError::ReadFile {
                    path: path.to_path_buf(),
                    message: err.to_string(),
                });
            }
        };
        Ok(parse_first_non_empty_line(&content))
    }

    fn write_colorscheme(
        &self,
        path: &Path,
        colorscheme: &str,
    ) -> std::result::Result<(), ThemeStateStoreError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|err| ThemeStateStoreError::CreateDirectory {
                path: parent.to_path_buf(),
                message: err.to_string(),
            })?;
        }
        fs::write(path, format!("{colorscheme}\n")).map_err(|err| {
            ThemeStateStoreError::WriteFile {
                path: path.to_path_buf(),
                message: err.to_string(),
            }
        })?;
        Ok(())
    }
}

#[derive(Debug)]
enum RuntimeAction {
    ApplyColorscheme {
        colorscheme: String,
    },
    ApplyAndPersistColorscheme {
        colorscheme: String,
        path: Option<PathBuf>,
    },
    ClosePicker {
        win_handle: WinHandle,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RenderPlan {
    NoRender,
    Full,
    CursorDelta {
        previous: ThemeIndex,
        current: ThemeIndex,
    },
}

#[derive(Debug)]
struct DispatchPlan {
    actions: Vec<RuntimeAction>,
    render: RenderPlan,
}

static CONTEXT: LazyLock<ThemeSwitcherContext> = LazyLock::new(ThemeSwitcherContext::new);
static STATE_STORE: FsThemeStateStore = FsThemeStateStore::new();

fn context() -> &'static ThemeSwitcherContext {
    &CONTEXT
}

fn state_store() -> &'static dyn ThemeStateStore {
    &STATE_STORE
}

fn dimensions() -> (usize, usize) {
    let opts = OptionOpts::builder().build();
    let columns = api::get_option_value::<i64>("columns", &opts)
        .ok()
        .and_then(|value| usize::try_from(value).ok())
        .filter(|value| *value > 0)
        .unwrap_or(120);
    let lines = api::get_option_value::<i64>("lines", &opts)
        .ok()
        .and_then(|value| usize::try_from(value).ok())
        .filter(|value| *value > 0)
        .unwrap_or(40);
    (columns, lines)
}

fn build_window_config(title: &str, catalog: &ThemeCatalog) -> WindowConfig {
    let (columns, lines) = dimensions();
    let longest_theme = catalog
        .iter()
        .map(|theme| theme.name().as_str().chars().count())
        .max()
        .unwrap_or(0);
    let longest_content = [
        title.chars().count(),
        HELP_LINE.chars().count(),
        longest_theme + 2,
    ]
    .into_iter()
    .max()
    .unwrap_or(40);

    let desired_width = longest_content + 4;
    let max_width = columns.saturating_sub(2).max(1);
    let min_width = 32_usize.min(max_width);
    let width = desired_width.clamp(min_width, max_width);

    let desired_height = catalog.len() + (THEME_LINE_START - 1);
    let max_height = lines.saturating_sub(2).max(1);
    let min_height = 6_usize.min(max_height);
    let height = desired_height.clamp(min_height, max_height);

    let row = (lines.saturating_sub(height)) as f64 / 2.0;
    let col = (columns.saturating_sub(width)) as f64 / 2.0;

    let mut builder = WindowConfig::builder();
    builder
        .relative(WindowRelativeTo::Editor)
        .style(WindowStyle::Minimal)
        .border(WindowBorder::Rounded)
        .width(width as u32)
        .height(height as u32)
        .row(row)
        .col(col)
        .focusable(true)
        .zindex(120);
    builder.build()
}

fn configure_buffer(buffer: &Buffer) -> Result<()> {
    let opts = OptionOpts::builder().buf(buffer.clone()).build();
    api::set_option_value("buftype", "nofile", &opts)?;
    api::set_option_value("bufhidden", "wipe", &opts)?;
    api::set_option_value("swapfile", false, &opts)?;
    api::set_option_value("modifiable", false, &opts)?;
    api::set_option_value("filetype", "rs_theme_switcher", &opts)?;
    Ok(())
}

fn configure_window(window: &Window) -> Result<()> {
    let opts = OptionOpts::builder()
        .scope(OptionScope::Local)
        .win(window.clone())
        .build();
    api::set_option_value("number", false, &opts)?;
    api::set_option_value("relativenumber", false, &opts)?;
    api::set_option_value("wrap", false, &opts)?;
    api::set_option_value("cursorline", true, &opts)?;
    api::set_option_value("signcolumn", "no", &opts)?;
    Ok(())
}

fn set_picker_keymap(buffer: &mut Buffer, lhs: &str, rhs: &str, desc: &str) -> Result<()> {
    let opts = SetKeymapOpts::builder()
        .noremap(true)
        .silent(true)
        .nowait(true)
        .desc(desc)
        .build();
    buffer.set_keymap(Mode::Normal, lhs, rhs, &opts)?;
    Ok(())
}

fn setup_picker_keymaps(buffer: &mut Buffer) -> Result<()> {
    set_picker_keymap(
        buffer,
        "<C-n>",
        "<cmd>lua require('rs_theme_switcher').move_next()<CR>",
        "Theme switcher next",
    )?;
    set_picker_keymap(
        buffer,
        "<C-p>",
        "<cmd>lua require('rs_theme_switcher').move_prev()<CR>",
        "Theme switcher previous",
    )?;
    set_picker_keymap(
        buffer,
        "<CR>",
        "<cmd>lua require('rs_theme_switcher').confirm()<CR>",
        "Theme switcher confirm",
    )?;
    set_picker_keymap(
        buffer,
        "<Esc>",
        "<cmd>lua require('rs_theme_switcher').cancel()<CR>",
        "Theme switcher cancel",
    )?;
    set_picker_keymap(
        buffer,
        "q",
        "<cmd>lua require('rs_theme_switcher').cancel()<CR>",
        "Theme switcher cancel",
    )?;
    Ok(())
}

fn with_modifiable_buffer<T, F>(buffer: &mut Buffer, update: F) -> Result<T>
where
    F: FnOnce(&mut Buffer) -> Result<T>,
{
    let opts = OptionOpts::builder().buf(buffer.clone()).build();
    api::set_option_value("modifiable", true, &opts)?;
    let update_result = update(buffer);
    let restore_result = api::set_option_value("modifiable", false, &opts);
    match (update_result, restore_result) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(err), Ok(())) => Err(err),
        (Ok(_), Err(err)) => Err(err.into()),
        (Err(err), Err(_)) => Err(err),
    }
}

fn set_buffer_lines(buffer: &mut Buffer, lines: Vec<String>) -> Result<()> {
    with_modifiable_buffer(buffer, |buffer| {
        buffer.set_lines(.., true, lines)?;
        Ok(())
    })
}

fn set_buffer_line_updates(buffer: &mut Buffer, updates: Vec<(usize, String)>) -> Result<()> {
    with_modifiable_buffer(buffer, move |buffer| {
        for (line_number, line) in updates {
            let line_index = line_number.saturating_sub(1);
            buffer.set_lines(line_index..line_index + 1, true, vec![line])?;
        }
        Ok(())
    })
}

fn render_session(session: &PickerSession) -> Result<()> {
    let Some(mut buffer) = session.buf_handle.valid_buffer() else {
        return Ok(());
    };
    let Some(mut window) = session.win_handle.valid_window() else {
        return Ok(());
    };
    let lines = session.lines();
    set_buffer_lines(&mut buffer, lines)?;
    window.set_cursor(session.theme_line_number(), 0)?;
    Ok(())
}

fn with_active_session<F>(update: F) -> Result<()>
where
    F: FnOnce(&mut PickerSession) -> Result<()>,
{
    let mut state = context().state_lock();
    let Some(session) = state.as_mut() else {
        return Ok(());
    };
    if !session.has_valid_handles() {
        *state = None;
        return Ok(());
    }
    update(session)
}

fn render_current_session() -> Result<()> {
    with_active_session(|session| render_session(session))
}

fn render_cursor_delta(previous: ThemeIndex, current: ThemeIndex) -> Result<()> {
    with_active_session(|session| {
        let Some(mut buffer) = session.buf_handle.valid_buffer() else {
            return Ok(());
        };
        let Some(mut window) = session.win_handle.valid_window() else {
            return Ok(());
        };

        let mut updates = Vec::with_capacity(2);
        if previous != current
            && let Some(previous_line) = session.theme_line(previous, false)
        {
            updates.push((
                PickerSession::theme_line_number_for(previous),
                previous_line,
            ));
        }
        if let Some(current_line) = session.theme_line(current, true) {
            updates.push((PickerSession::theme_line_number_for(current), current_line));
        }
        if !updates.is_empty() {
            set_buffer_line_updates(&mut buffer, updates)?;
        }
        window.set_cursor(session.theme_line_number(), 0)?;
        Ok(())
    })
}

fn execute_render_plan(render: RenderPlan) {
    let result = match render {
        RenderPlan::NoRender => Ok(()),
        RenderPlan::Full => render_current_session(),
        RenderPlan::CursorDelta { previous, current } => render_cursor_delta(previous, current),
    };
    if let Err(err) = result {
        notify::warn(LOG_CONTEXT, &format!("render theme picker failed: {err}"));
    }
}

fn close_window(win_handle: WinHandle) {
    if let Some(window) = win_handle.valid_window()
        && let Err(err) = window.close(true)
    {
        notify::warn(LOG_CONTEXT, &format!("close picker window failed: {err}"));
    }
}

fn apply_colorscheme(colorscheme: &str) -> Result<()> {
    let infos = CmdInfos::builder()
        .cmd("colorscheme")
        .args([colorscheme.to_string()])
        .build();
    let opts = CmdOpts::builder().build();
    let _ = api::cmd(&infos, &opts)?;
    Ok(())
}

fn parse_first_non_empty_line(content: &str) -> Option<String> {
    content
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(ToOwned::to_owned)
}

fn read_persisted_colorscheme(path: Option<&Path>, store: &dyn ThemeStateStore) -> Option<String> {
    let Some(path) = path else {
        return None;
    };
    match store.read_colorscheme(path) {
        Ok(name) => name,
        Err(err) => {
            notify::warn(LOG_CONTEXT, &format!("{err}"));
            None
        }
    }
}

fn resolve_effective_index(
    catalog: &ThemeCatalog,
    current_colorscheme: Option<&str>,
    state_path: Option<&Path>,
    store: &dyn ThemeStateStore,
) -> Option<ThemeIndex> {
    let persisted_colorscheme = read_persisted_colorscheme(state_path, store);
    resolve_effective_theme_index(
        catalog,
        persisted_colorscheme.as_deref(),
        current_colorscheme,
    )
}

fn persist_with_store(path: &Path, colorscheme: &str, store: &dyn ThemeStateStore) {
    if let Err(err) = store.write_colorscheme(path, colorscheme) {
        notify::warn(LOG_CONTEXT, &format!("{err}"));
    }
}

fn apply_then_persist<F, E>(
    colorscheme: &str,
    path: Option<&Path>,
    store: &dyn ThemeStateStore,
    apply: F,
) -> std::result::Result<(), E>
where
    F: FnOnce(&str) -> std::result::Result<(), E>,
{
    apply(colorscheme)?;
    if let Some(path) = path {
        persist_with_store(path, colorscheme, store);
    }
    Ok(())
}

fn execute_action(action: RuntimeAction, store: &dyn ThemeStateStore) {
    match action {
        RuntimeAction::ApplyColorscheme { colorscheme } => {
            if let Err(err) = apply_colorscheme(&colorscheme) {
                notify::warn(
                    LOG_CONTEXT,
                    &format!("apply colorscheme '{colorscheme}' failed: {err}"),
                );
            }
        }
        RuntimeAction::ApplyAndPersistColorscheme { colorscheme, path } => {
            if let Err(err) =
                apply_then_persist(&colorscheme, path.as_deref(), store, apply_colorscheme)
            {
                notify::warn(
                    LOG_CONTEXT,
                    &format!("apply colorscheme '{colorscheme}' failed: {err}"),
                );
            }
        }
        RuntimeAction::ClosePicker { win_handle } => close_window(win_handle),
    }
}

fn session_from_args(
    args: OpenArgs,
    store: &dyn ThemeStateStore,
) -> std::result::Result<PickerSession, String> {
    let OpenArgs {
        themes,
        title,
        current_colorscheme,
        state_path,
    } = args;

    let catalog = ThemeCatalog::try_from_vec(themes).map_err(|err| err.to_string())?;
    let matched_index = resolve_effective_index(
        &catalog,
        current_colorscheme.as_deref(),
        state_path.as_deref(),
        store,
    );
    let persisted_index = matched_index.unwrap_or_else(|| catalog.first_index());
    let restore_colorscheme_on_cancel = if matched_index.is_none() {
        current_colorscheme
    } else {
        None
    };

    let window_config = build_window_config(&title, &catalog);
    let machine = ThemeSwitcherMachine::new(catalog, persisted_index.raw());

    let mut buffer = api::create_buf(false, true).map_err(|err| err.to_string())?;
    configure_buffer(&buffer).map_err(|err| err.to_string())?;
    let window = api::open_win(&buffer, true, &window_config).map_err(|err| err.to_string())?;
    configure_window(&window).map_err(|err| err.to_string())?;
    setup_picker_keymaps(&mut buffer).map_err(|err| err.to_string())?;

    Ok(PickerSession {
        machine,
        title,
        state_path,
        restore_colorscheme_on_cancel,
        buf_handle: BufHandle::from_buffer(&buffer),
        win_handle: WinHandle::from_window(&window),
    })
}

fn close_active_session() {
    let old = {
        let mut state = context().state_lock();
        state.take()
    };
    if let Some(session) = old {
        close_window(session.win_handle);
    }
}

fn dispatch_event(event: ThemeSwitcherEvent) {
    let plan = {
        let mut state = context().state_lock();
        let Some(session) = state.as_mut() else {
            return;
        };
        if !session.has_valid_handles() {
            *state = None;
            return;
        }

        let previous_cursor = session.machine.cursor_index();
        let transition = session.machine.reduce(event);
        if transition.is_empty() {
            return;
        }

        let mut actions = Vec::new();
        let mut render = RenderPlan::NoRender;
        let mut should_close = false;

        for effect in transition.effects {
            match effect {
                ThemeSwitcherEffect::PreviewTheme(index) => {
                    if let Some(colorscheme) = session.colorscheme_for_index(index) {
                        actions.push(RuntimeAction::ApplyColorscheme {
                            colorscheme: colorscheme.to_string(),
                        });
                        if previous_cursor != index {
                            render = match render {
                                RenderPlan::NoRender => RenderPlan::CursorDelta {
                                    previous: previous_cursor,
                                    current: index,
                                },
                                RenderPlan::CursorDelta { .. } | RenderPlan::Full => {
                                    RenderPlan::Full
                                }
                            };
                        }
                    }
                }
                ThemeSwitcherEffect::PersistTheme(index) => {
                    if let Some(colorscheme) = session.colorscheme_for_index(index) {
                        actions.push(RuntimeAction::ApplyAndPersistColorscheme {
                            colorscheme: colorscheme.to_string(),
                            path: session.state_path.clone(),
                        });
                    }
                }
                ThemeSwitcherEffect::RestoreTheme(index) => {
                    let maybe_colorscheme = session
                        .restore_colorscheme_on_cancel
                        .clone()
                        .or_else(|| session.colorscheme_for_index(index).map(str::to_string));
                    if let Some(colorscheme) = maybe_colorscheme {
                        actions.push(RuntimeAction::ApplyColorscheme { colorscheme });
                    }
                }
                ThemeSwitcherEffect::ClosePicker => {
                    should_close = true;
                }
            }
        }

        if should_close {
            actions.push(RuntimeAction::ClosePicker {
                win_handle: session.win_handle,
            });
            *state = None;
            render = RenderPlan::NoRender;
        }

        DispatchPlan { actions, render }
    };

    let store = state_store();
    for action in plan.actions {
        execute_action(action, store);
    }
    execute_render_plan(plan.render);
}

pub fn open(args: &Dictionary) {
    let parsed = match OpenArgs::parse(args) {
        Ok(value) => value,
        Err(err) => {
            notify::error(LOG_CONTEXT, &format!("invalid open args: {err}"));
            return;
        }
    };

    close_active_session();

    let session = match session_from_args(parsed, state_store()) {
        Ok(value) => value,
        Err(err) => {
            notify::error(LOG_CONTEXT, &format!("failed to build session: {err}"));
            return;
        }
    };

    {
        let mut state = context().state_lock();
        *state = Some(session);
    }

    if let Err(err) = render_current_session() {
        notify::error(LOG_CONTEXT, &format!("render theme picker failed: {err}"));
    }
}

fn execute_cycle(args: &Dictionary, direction: ThemeCycleDirection) {
    let parsed = match CycleArgs::parse(args) {
        Ok(value) => value,
        Err(err) => {
            notify::error(LOG_CONTEXT, &format!("invalid cycle args: {err}"));
            return;
        }
    };

    close_active_session();
    let store = state_store();

    let catalog = match ThemeCatalog::try_from_vec(parsed.themes) {
        Ok(value) => value,
        Err(err) => {
            notify::error(LOG_CONTEXT, &format!("invalid cycle args: {err}"));
            return;
        }
    };

    let effective_index = resolve_effective_index(
        &catalog,
        parsed.current_colorscheme.as_deref(),
        parsed.state_path.as_deref(),
        store,
    );
    let target_index = cycle_theme_index_from_index(&catalog, effective_index, direction);
    let Some(theme) = catalog.get(target_index) else {
        notify::error(LOG_CONTEXT, "failed to resolve cycle target theme");
        return;
    };

    let colorscheme = theme.colorscheme().as_str().to_string();
    if let Err(err) = apply_colorscheme(&colorscheme) {
        notify::warn(
            LOG_CONTEXT,
            &format!("apply colorscheme '{colorscheme}' failed: {err}"),
        );
        return;
    }

    if let Some(path) = parsed.state_path.as_deref() {
        persist_with_store(path, &colorscheme, store);
    }
}

pub fn cycle_next(args: &Dictionary) {
    execute_cycle(args, ThemeCycleDirection::Next);
}

pub fn cycle_prev(args: &Dictionary) {
    execute_cycle(args, ThemeCycleDirection::Prev);
}

pub fn move_next() {
    dispatch_event(ThemeSwitcherEvent::MoveNext);
}

pub fn move_prev() {
    dispatch_event(ThemeSwitcherEvent::MovePrev);
}

pub fn confirm() {
    dispatch_event(ThemeSwitcherEvent::Confirm);
}

pub fn cancel() {
    dispatch_event(ThemeSwitcherEvent::Cancel);
}

pub fn close() {
    close_active_session();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[derive(Debug, Default)]
    struct RecordingStateStore {
        writes: RefCell<Vec<(PathBuf, String)>>,
    }

    impl RecordingStateStore {
        fn writes(&self) -> Vec<(PathBuf, String)> {
            self.writes.borrow().clone()
        }
    }

    impl ThemeStateStore for RecordingStateStore {
        fn read_colorscheme(
            &self,
            _path: &Path,
        ) -> std::result::Result<Option<String>, ThemeStateStoreError> {
            Ok(None)
        }

        fn write_colorscheme(
            &self,
            path: &Path,
            colorscheme: &str,
        ) -> std::result::Result<(), ThemeStateStoreError> {
            self.writes
                .borrow_mut()
                .push((path.to_path_buf(), colorscheme.to_string()));
            Ok(())
        }
    }

    #[test]
    fn apply_then_persist_skips_write_when_apply_fails() {
        let store = RecordingStateStore::default();
        let path = Path::new("/tmp/theme-switcher-state");
        let result = apply_then_persist("broken", Some(path), &store, |_| Err("boom"));
        assert!(result.is_err());
        assert!(store.writes().is_empty());
    }

    #[test]
    fn apply_then_persist_writes_when_apply_succeeds() {
        let store = RecordingStateStore::default();
        let path = Path::new("/tmp/theme-switcher-state");
        let result = apply_then_persist("tokyonight", Some(path), &store, |_| Ok::<(), &str>(()));
        assert!(result.is_ok());
        assert_eq!(
            store.writes(),
            vec![(path.to_path_buf(), "tokyonight".to_string())]
        );
    }
}
