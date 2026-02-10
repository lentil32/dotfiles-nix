use crate::config::RuntimeConfig;
use crate::draw::{
    RenderFrame, clear_active_render_windows, clear_all_namespaces, clear_highlight_cache,
    draw_current, draw_target_hack_block, purge_render_windows,
};
use crate::lua::{
    bool_from_object, f64_from_object, i64_from_object, invalid_key, parse_indexed_objects,
    string_from_object,
};
use crate::reducer::{
    CursorEventContext, EventSource, RenderAction, RenderCleanupAction, ScrollShift, as_delay_ms,
    build_render_frame, external_settle_delay_ms, next_animation_delay_ms, reduce_cursor_event,
};
use crate::state::{CursorLocation, CursorShape, CursorSnapshot, RuntimeState};
use crate::types::{DEFAULT_RNG_STATE, EPSILON, Point};
use nvim_oxi::api;
use nvim_oxi::api::opts::{
    ClearAutocmdsOpts, CreateAugroupOpts, CreateAutocmdOpts, CreateCommandOpts, OptionOpts,
    WinTextHeightOpts,
};
use nvim_oxi::api::types::AutocmdCallbackArgs;
use nvim_oxi::conversion::FromObject;
use nvim_oxi::libuv::TimerHandle;
use nvim_oxi::schedule;
use nvim_oxi::{Array, Dictionary, Object, Result, String as NvimString};
use std::cell::{Cell, RefCell};
use std::ops::{Deref, DerefMut};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const LOG_SOURCE_NAME: &str = "smear_cursor";
const LOG_LEVEL_TRACE: i64 = 0;
const LOG_LEVEL_DEBUG: i64 = 1;
const LOG_LEVEL_WARN: i64 = 3;
const LOG_LEVEL_INFO: i64 = 2;
const LOG_LEVEL_ERROR: i64 = 4;
const AUTOCMD_GROUP_NAME: &str = "RsSmearCursor";
const MIN_RENDER_CLEANUP_DELAY_MS: u64 = 200;
const CURSOR_COLOR_LUAEVAL_EXPR: &str = r##"(function()
  local function get_hl_color(group, attr)
    local hl = vim.api.nvim_get_hl(0, { name = group, link = false })
    if hl[attr] then
      return string.format("#%06x", hl[attr])
    end
    return nil
  end

  local cursor = vim.api.nvim_win_get_cursor(0)
  cursor[1] = cursor[1] - 1

  if vim.b.ts_highlight then
    local ts_hl_group
    for _, capture in pairs(vim.treesitter.get_captures_at_pos(0, cursor[1], cursor[2])) do
      ts_hl_group = "@" .. capture.capture .. "." .. capture.lang
    end
    if ts_hl_group then
      return get_hl_color(ts_hl_group, "fg")
    end
  end

  local extmarks = vim.api.nvim_buf_get_extmarks(0, -1, cursor, cursor, { details = true, overlap = true })
  for _, extmark in ipairs(extmarks) do
    local details = extmark[4]
    local hl_group = details and details.hl_group
    if hl_group then
      local color = get_hl_color(hl_group, "fg")
      if color then
        return color
      end
    end
  end

  return nil
end)()"##;

#[derive(Debug, Clone, Copy, Default)]
struct RenderCleanupGeneration {
    value: u64,
}

impl RenderCleanupGeneration {
    fn bump(&mut self) -> u64 {
        self.value = self.value.wrapping_add(1);
        self.value
    }

    const fn current(self) -> u64 {
        self.value
    }
}

#[derive(Debug, Default)]
struct EngineState {
    runtime: RuntimeState,
    render_cleanup_generation: RenderCleanupGeneration,
}

impl EngineState {
    fn bump_render_cleanup_generation(&mut self) -> u64 {
        self.render_cleanup_generation.bump()
    }

    const fn current_render_cleanup_generation(&self) -> u64 {
        self.render_cleanup_generation.current()
    }
}

struct RuntimeStateGuard(std::sync::MutexGuard<'static, EngineState>);

impl Deref for RuntimeStateGuard {
    type Target = RuntimeState;

    fn deref(&self) -> &Self::Target {
        &self.0.runtime
    }
}

impl DerefMut for RuntimeStateGuard {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0.runtime
    }
}

static ENGINE_STATE: LazyLock<Mutex<EngineState>> =
    LazyLock::new(|| Mutex::new(EngineState::default()));
static LOG_LEVEL: AtomicI64 = AtomicI64::new(LOG_LEVEL_INFO);
thread_local! {
    static ANIMATION_TIMER: RefCell<Option<TimerHandle>> = const { RefCell::new(None) };
    static EXTERNAL_EVENT_TIMER: RefCell<Option<TimerHandle>> = const { RefCell::new(None) };
    static KEY_EVENT_TIMER: RefCell<Option<TimerHandle>> = const { RefCell::new(None) };
    static RENDER_CLEANUP_TIMER: RefCell<Option<TimerHandle>> = const { RefCell::new(None) };
    static EXTERNAL_TRIGGER_PENDING: Cell<bool> = const { Cell::new(false) };
    static LAST_AUTOCMD_EVENT_MS: Cell<f64> = const { Cell::new(0.0) };
}

fn set_log_level(level: i64) {
    let normalized = if level < 0 { 0 } else { level };
    LOG_LEVEL.store(normalized, Ordering::Relaxed);
}

fn should_log(level: i64) -> bool {
    LOG_LEVEL.load(Ordering::Relaxed) <= level
}

fn log_level_name(level: i64) -> &'static str {
    match level {
        LOG_LEVEL_TRACE => "TRACE",
        LOG_LEVEL_DEBUG => "DEBUG",
        LOG_LEVEL_INFO => "INFO",
        LOG_LEVEL_WARN => "WARNING",
        LOG_LEVEL_ERROR => "ERROR",
        _ => "INFO",
    }
}

fn notify_log(level: i64, message: &str) {
    if !should_log(level) {
        return;
    }

    let payload_message = format!("[{LOG_SOURCE_NAME}][{}] {message}", log_level_name(level));
    let payload = Array::from_iter([Object::from(payload_message), Object::from(level)]);
    let args = Array::from_iter([
        Object::from("vim.notify(_A[1], _A[2])"),
        Object::from(payload),
    ]);
    if let Err(err) = api::call_function::<_, Object>("luaeval", args) {
        eprintln!("[{LOG_SOURCE_NAME}] vim.notify failed: {err}");
    }
}

fn warn(message: &str) {
    notify_log(LOG_LEVEL_WARN, message);
}

fn debug(message: &str) {
    notify_log(LOG_LEVEL_DEBUG, message);
}

fn ensure_hideable_guicursor() {
    let opts = nvim_oxi::api::opts::OptionOpts::builder().build();
    let Ok(mut guicursor) = api::get_option_value::<String>("guicursor", &opts) else {
        return;
    };
    if guicursor
        .split(',')
        .any(|entry| entry.trim() == "a:SmearCursorHideable")
    {
        return;
    }
    if !guicursor.is_empty() {
        guicursor.push(',');
    }
    guicursor.push_str("a:SmearCursorHideable");
    if let Err(err) = api::set_option_value("guicursor", guicursor, &opts) {
        warn(&format!("set guicursor failed: {err}"));
    }
}

fn hide_real_cursor() {
    let opts = nvim_oxi::api::opts::SetHighlightOpts::builder()
        .foreground("white")
        .blend(100)
        .build();
    if let Err(err) = api::set_hl(0, "SmearCursorHideable", &opts) {
        warn(&format!("set highlight failed: {err}"));
    }
}

fn unhide_real_cursor() {
    let opts = nvim_oxi::api::opts::SetHighlightOpts::builder()
        .foreground("none")
        .blend(0)
        .build();
    if let Err(err) = api::set_hl(0, "SmearCursorHideable", &opts) {
        warn(&format!("restore highlight failed: {err}"));
    }
}

fn clear_animation_timer() {
    ANIMATION_TIMER.with(|timer_slot| {
        let _ = timer_slot.borrow_mut().take();
    });
}

fn clear_external_event_timer() {
    EXTERNAL_EVENT_TIMER.with(|timer_slot| {
        let _ = timer_slot.borrow_mut().take();
    });
}

fn clear_key_event_timer() {
    KEY_EVENT_TIMER.with(|timer_slot| {
        let _ = timer_slot.borrow_mut().take();
    });
}

fn clear_render_cleanup_timer() {
    RENDER_CLEANUP_TIMER.with(|timer_slot| {
        let _ = timer_slot.borrow_mut().take();
    });
}

fn bump_render_cleanup_generation() -> u64 {
    let mut state = engine_lock();
    state.bump_render_cleanup_generation()
}

fn current_render_cleanup_generation() -> u64 {
    let state = engine_lock();
    state.current_render_cleanup_generation()
}

fn invalidate_render_cleanup() {
    bump_render_cleanup_generation();
    clear_render_cleanup_timer();
}

fn clear_external_trigger_pending() {
    EXTERNAL_TRIGGER_PENDING.with(|pending| pending.set(false));
}

fn mark_external_trigger_pending_if_idle() -> bool {
    EXTERNAL_TRIGGER_PENDING.with(|pending| {
        if pending.get() {
            false
        } else {
            pending.set(true);
            true
        }
    })
}

fn note_autocmd_event_now() {
    LAST_AUTOCMD_EVENT_MS.with(|value| value.set(now_ms()));
}

fn clear_autocmd_event_timestamp() {
    LAST_AUTOCMD_EVENT_MS.with(|value| value.set(0.0));
}

fn elapsed_ms_since_last_autocmd_event(now_ms: f64) -> f64 {
    LAST_AUTOCMD_EVENT_MS.with(|value| {
        let last = value.get();
        if last <= 0.0 {
            f64::INFINITY
        } else {
            (now_ms - last).max(0.0)
        }
    })
}

fn reset_transient_event_state() {
    clear_animation_timer();
    clear_external_event_timer();
    clear_key_event_timer();
    clear_external_trigger_pending();
    clear_autocmd_event_timestamp();
    invalidate_render_cleanup();
}

fn reset_transient_event_state_without_generation_bump() {
    clear_animation_timer();
    clear_external_event_timer();
    clear_key_event_timer();
    clear_external_trigger_pending();
    clear_autocmd_event_timestamp();
    clear_render_cleanup_timer();
}

fn engine_lock() -> std::sync::MutexGuard<'static, EngineState> {
    loop {
        match ENGINE_STATE.lock() {
            Ok(guard) => return guard,
            Err(poisoned) => {
                let mut guard = poisoned.into_inner();
                let namespace_id = guard.runtime.namespace_id;
                *guard = EngineState::default();
                drop(guard);

                ENGINE_STATE.clear_poison();
                set_log_level(RuntimeConfig::default().logging_level);
                warn("state mutex poisoned; resetting runtime state");
                if let Some(namespace_id) = namespace_id {
                    clear_all_namespaces(namespace_id);
                }
                reset_transient_event_state_without_generation_bump();
            }
        }
    }
}

fn state_lock() -> RuntimeStateGuard {
    RuntimeStateGuard(engine_lock())
}

fn is_enabled() -> bool {
    let state = state_lock();
    state.is_enabled()
}

fn now_ms() -> f64 {
    let duration = match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration,
        Err(err) => err.duration(),
    };
    duration.as_secs_f64() * 1000.0
}

fn seed_from_clock() -> u32 {
    let duration = match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration,
        Err(err) => err.duration(),
    };

    let mut seed = (duration.as_nanos() & u128::from(u32::MAX)) as u32;
    if seed == 0 {
        seed = DEFAULT_RNG_STATE;
    }
    seed
}

fn mode_string() -> String {
    api::get_mode().mode.to_string_lossy().into_owned()
}

fn screen_cursor_position(window: &api::Window) -> Result<Option<(f64, f64)>> {
    let mut row = i64_from_object("screenrow", api::call_function("screenrow", Array::new())?)?;
    let mut col = i64_from_object("screencol", api::call_function("screencol", Array::new())?)?;

    let config_args = Array::from_iter([Object::from(window.handle())]);
    let win_config: Dictionary = api::call_function("nvim_win_get_config", config_args)?;
    let relative = win_config
        .get(&NvimString::from("relative"))
        .cloned()
        .map(|value| string_from_object("nvim_win_get_config.relative", value))
        .transpose()?
        .unwrap_or_default();

    if !relative.is_empty() {
        let wininfo_args = Array::from_iter([Object::from(window.handle())]);
        let wininfo = api::call_function("getwininfo", wininfo_args)?;
        let entries = parse_indexed_objects("getwininfo", wininfo, Some(1))
            .map_err(|_| nvim_oxi::api::Error::Other("getwininfo returned no entries".into()))?;
        let wininfo_entry = Dictionary::from_object(entries[0].clone())
            .map_err(|_| nvim_oxi::api::Error::Other("getwininfo[1] invalid dictionary".into()))?;

        let winrow_obj = wininfo_entry
            .get(&NvimString::from("winrow"))
            .cloned()
            .ok_or_else(|| nvim_oxi::api::Error::Other("getwininfo.winrow missing".into()))?;
        let wincol_obj = wininfo_entry
            .get(&NvimString::from("wincol"))
            .cloned()
            .ok_or_else(|| nvim_oxi::api::Error::Other("getwininfo.wincol missing".into()))?;
        let info_row = i64_from_object("getwininfo.winrow", winrow_obj)?;
        let info_col = i64_from_object("getwininfo.wincol", wincol_obj)?;

        row = row.saturating_add(info_row.saturating_sub(1));
        col = col.saturating_add(info_col.saturating_sub(1));
    }

    Ok(Some((row as f64, col as f64)))
}

fn cmdline_cursor_position() -> Result<Option<(f64, f64)>> {
    let cmdpos_value = api::call_function("getcmdpos", Array::new())?;
    let cmdpos = i64_from_object("getcmdpos", cmdpos_value)?;

    if let Ok(ui_cmdline_pos) = api::get_var::<Object>("ui_cmdline_pos") {
        if let Ok(indexed) =
            parse_indexed_objects("ui_cmdline_pos", ui_cmdline_pos.clone(), Some(2))
        {
            let row = i64_from_object("ui_cmdline_pos[1]", indexed[0].clone())?;
            let col = i64_from_object("ui_cmdline_pos[2]", indexed[1].clone())?;
            let final_col = col.saturating_add(cmdpos).saturating_add(1);
            return Ok(Some((row as f64, final_col as f64)));
        } else if let Ok(dict) = Dictionary::from_object(ui_cmdline_pos) {
            let maybe_row = dict.get(&NvimString::from("row")).cloned();
            let maybe_col = dict.get(&NvimString::from("col")).cloned();
            if let (Some(row_obj), Some(col_obj)) = (maybe_row, maybe_col) {
                let row = i64_from_object("ui_cmdline_pos.row", row_obj)?;
                let col = i64_from_object("ui_cmdline_pos.col", col_obj)?;
                let final_col = col.saturating_add(cmdpos).saturating_add(1);
                return Ok(Some((row as f64, final_col as f64)));
            }
        }
    }

    let opts = nvim_oxi::api::opts::OptionOpts::builder().build();
    let lines: i64 = api::get_option_value("lines", &opts)?;
    let cmdheight: i64 = api::get_option_value("cmdheight", &opts)?;
    let row = lines.saturating_sub(cmdheight).saturating_add(1);
    let col = cmdpos.saturating_add(1);
    Ok(Some((row as f64, col as f64)))
}

fn cursor_position_for_mode(
    window: &api::Window,
    mode: &str,
    smear_to_cmd: bool,
) -> Result<Option<(f64, f64)>> {
    if mode == "c" {
        if !smear_to_cmd {
            return Ok(None);
        }
        return cmdline_cursor_position();
    }
    screen_cursor_position(window)
}

fn current_buffer_filetype(buffer: &api::Buffer) -> Result<String> {
    let opts = OptionOpts::builder().buf(buffer.clone()).build();
    let filetype: String = api::get_option_value("filetype", &opts)?;
    Ok(filetype)
}

fn skip_current_buffer_events(buffer: &api::Buffer) -> Result<bool> {
    let should_check_filetype = {
        let state = state_lock();
        if state.is_delay_disabled() {
            return Ok(true);
        }
        !state.config.filetypes_disabled.is_empty()
    };

    if !should_check_filetype {
        return Ok(false);
    }

    let filetype = current_buffer_filetype(buffer)?;
    let state = state_lock();
    Ok(state
        .config
        .filetypes_disabled
        .iter()
        .any(|entry| entry == &filetype))
}

fn should_track_cursor_color(config: &RuntimeConfig) -> bool {
    config.cursor_color.as_deref() == Some("none")
        || config.cursor_color_insert_mode.as_deref() == Some("none")
}

fn cursor_color_at_current_position() -> Result<Option<String>> {
    let args = Array::from_iter([Object::from(CURSOR_COLOR_LUAEVAL_EXPR)]);
    let value: Object = api::call_function("luaeval", args)?;
    if value.is_nil() {
        return Ok(None);
    }
    Ok(Some(string_from_object("cursor_color_luaeval", value)?))
}

fn update_tracked_cursor_color(state: &mut RuntimeState) {
    if !should_track_cursor_color(&state.config) {
        state.color_at_cursor = None;
        return;
    }

    match cursor_color_at_current_position() {
        Ok(color) => state.color_at_cursor = color,
        Err(err) => warn(&format!("cursor color sampling failed: {err}")),
    }
}

fn line_value(key: &str) -> Result<i64> {
    let args = Array::from_iter([Object::from(key)]);
    let value = api::call_function("line", args)?;
    i64_from_object("line", value)
}

fn command_row() -> Result<f64> {
    let opts = nvim_oxi::api::opts::OptionOpts::builder().build();
    let lines: i64 = api::get_option_value("lines", &opts)?;
    let cmdheight: i64 = api::get_option_value("cmdheight", &opts)?;
    Ok(lines.saturating_sub(cmdheight).saturating_add(1) as f64)
}

fn smear_outside_cmd_row(corners: &[Point; 4]) -> Result<bool> {
    let cmd_row = command_row()?;
    Ok(corners.iter().any(|point| point.row < cmd_row))
}

fn point_inside_target_bounds(
    point: Point,
    target_min_row: f64,
    target_max_row: f64,
    target_min_col: f64,
    target_max_col: f64,
) -> bool {
    point.row >= target_min_row
        && point.row <= target_max_row
        && point.col >= target_min_col
        && point.col <= target_max_col
}

fn frame_center(corners: &[Point; 4]) -> Point {
    let mut row = 0.0_f64;
    let mut col = 0.0_f64;
    for point in corners {
        row += point.row;
        col += point.col;
    }
    Point {
        row: row / 4.0,
        col: col / 4.0,
    }
}

fn frame_reaches_target_cell(frame: &RenderFrame) -> bool {
    let target_min_row = frame.target_corners[0].row;
    let target_max_row = frame.target_corners[2].row;
    let target_min_col = frame.target_corners[0].col;
    let target_max_col = frame.target_corners[2].col;
    let center = frame_center(&frame.corners);
    if point_inside_target_bounds(
        center,
        target_min_row,
        target_max_row,
        target_min_col,
        target_max_col,
    ) {
        return true;
    }

    frame.corners.iter().copied().any(|point| {
        point_inside_target_bounds(
            point,
            target_min_row,
            target_max_row,
            target_min_col,
            target_max_col,
        )
    })
}

fn line_index_1_to_0(row: i64) -> usize {
    let clamped = row.max(1).saturating_sub(1);
    usize::try_from(clamped).unwrap_or_default()
}

fn screen_distance(window: &api::Window, row_start: i64, row_end: i64) -> Result<f64> {
    let mut start = row_start;
    let mut end = row_end;
    let mut reversed = false;
    if start > end {
        std::mem::swap(&mut start, &mut end);
        reversed = true;
    }

    let window_height = i64::from(window.get_height()?);
    let distance = if end.saturating_sub(start) >= window_height {
        window_height.saturating_sub(1)
    } else {
        let opts = WinTextHeightOpts::builder()
            .start_row(line_index_1_to_0(start))
            .end_row(line_index_1_to_0(end))
            .build();
        match window.text_height(&opts) {
            Ok(height) => i64::from(height.all).saturating_sub(1),
            Err(_) => 0,
        }
    };

    if reversed {
        Ok(-(distance as f64))
    } else {
        Ok(distance as f64)
    }
}

fn maybe_scroll_shift(
    window: &api::Window,
    scroll_buffer_space: bool,
    current_corners: &[Point; 4],
    previous_location: Option<CursorLocation>,
    current_location: CursorLocation,
) -> Result<Option<ScrollShift>> {
    if !scroll_buffer_space {
        return Ok(None);
    }
    let Some(previous_location) = previous_location else {
        return Ok(None);
    };
    if previous_location.window_handle != current_location.window_handle
        || previous_location.buffer_handle != current_location.buffer_handle
    {
        return Ok(None);
    }
    if !smear_outside_cmd_row(current_corners)? {
        return Ok(None);
    }
    if previous_location.top_row == current_location.top_row
        || previous_location.line == current_location.line
    {
        return Ok(None);
    }

    let shift = screen_distance(window, previous_location.top_row, current_location.top_row)?;
    let (window_row_zero, _) = window.get_position()?;
    let window_height = f64::from(window.get_height()?);
    let min_row = window_row_zero as f64 + 1.0;
    let max_row = min_row + window_height - 1.0;

    Ok(Some(ScrollShift {
        shift,
        min_row,
        max_row,
    }))
}

fn current_cursor_snapshot(smear_to_cmd: bool) -> Result<Option<CursorSnapshot>> {
    let mode = mode_string();

    let window = api::get_current_win();
    if !window.is_valid() {
        return Ok(None);
    }

    let Some((row, col)) = cursor_position_for_mode(&window, &mode, smear_to_cmd)? else {
        return Ok(None);
    };

    Ok(Some(CursorSnapshot { mode, row, col }))
}

fn snapshots_match(lhs: &CursorSnapshot, rhs: &CursorSnapshot) -> bool {
    lhs.mode == rhs.mode
        && (lhs.row - rhs.row).abs() <= EPSILON
        && (lhs.col - rhs.col).abs() <= EPSILON
}

fn on_animation_tick() -> Result<()> {
    on_cursor_event_impl(EventSource::AnimationTick)
}

fn on_external_settle_tick() -> Result<()> {
    let mode = mode_string();
    let (smear_to_cmd, delay_ms, expected_snapshot) = {
        let state = state_lock();
        (
            state.config.smear_to_cmd,
            external_settle_delay_ms(state.config.delay_event_to_smear),
            state.pending_external_event_cloned(),
        )
    };

    if mode == "c" && !smear_to_cmd {
        let mut state = state_lock();
        state.clear_pending_external_event();
        return Ok(());
    }

    let Some(expected_snapshot) = expected_snapshot else {
        return Ok(());
    };

    let current_snapshot = current_cursor_snapshot(smear_to_cmd)?;
    let Some(current_snapshot) = current_snapshot else {
        {
            let mut state = state_lock();
            state.clear_pending_external_event();
        }
        return Ok(());
    };

    if snapshots_match(&current_snapshot, &expected_snapshot) {
        {
            let mut state = state_lock();
            state.clear_pending_external_event();
        }
        return on_cursor_event_impl(EventSource::External);
    }

    {
        let mut state = state_lock();
        state.set_pending_external_event(Some(current_snapshot));
    }
    schedule_external_event_timer(delay_ms);
    Ok(())
}

fn schedule_animation_tick(delay_ms: u64) {
    let already_scheduled = ANIMATION_TIMER.with(|timer_slot| timer_slot.borrow().is_some());
    if already_scheduled {
        return;
    }

    let timeout = Duration::from_millis(delay_ms);
    match TimerHandle::once(timeout, || {
        schedule(|_| {
            clear_animation_timer();
            if let Err(err) = on_animation_tick() {
                warn(&format!("animation tick failed: {err}"));
            }
        });
    }) {
        Ok(handle) => {
            ANIMATION_TIMER.with(|timer_slot| {
                *timer_slot.borrow_mut() = Some(handle);
            });
        }
        Err(err) => {
            warn(&format!("failed to schedule animation tick: {err}"));
        }
    }
}

fn schedule_external_event_timer(delay_ms: u64) {
    clear_external_event_timer();

    let timeout = Duration::from_millis(delay_ms);
    match TimerHandle::once(timeout, || {
        schedule(|_| {
            clear_external_event_timer();
            if let Err(err) = on_external_settle_tick() {
                warn(&format!("external settle tick failed: {err}"));
            }
        });
    }) {
        Ok(handle) => {
            EXTERNAL_EVENT_TIMER.with(|timer_slot| {
                *timer_slot.borrow_mut() = Some(handle);
            });
        }
        Err(err) => {
            warn(&format!("failed to schedule external settle tick: {err}"));
        }
    }
}

fn schedule_key_event_timer(delay_ms: u64) {
    clear_key_event_timer();

    if delay_ms == 0 {
        schedule_external_event_trigger();
        return;
    }

    let timeout = Duration::from_millis(delay_ms);
    match TimerHandle::once(timeout, || {
        schedule(|_| {
            clear_key_event_timer();
            schedule_external_event_trigger();
        });
    }) {
        Ok(handle) => {
            KEY_EVENT_TIMER.with(|timer_slot| {
                *timer_slot.borrow_mut() = Some(handle);
            });
        }
        Err(err) => {
            warn(&format!("failed to schedule key-event tick: {err}"));
        }
    }
}

fn render_cleanup_delay_ms(config: &RuntimeConfig) -> u64 {
    let baseline =
        as_delay_ms(config.time_interval + config.delay_event_to_smear + config.delay_after_key);
    baseline.max(MIN_RENDER_CLEANUP_DELAY_MS)
}

fn schedule_render_cleanup_timer(namespace_id: u32, delay_ms: u64) {
    let generation = bump_render_cleanup_generation();
    clear_render_cleanup_timer();

    let timeout = Duration::from_millis(delay_ms);
    match TimerHandle::once(timeout, move || {
        schedule(move |_| {
            clear_render_cleanup_timer();

            if current_render_cleanup_generation() != generation {
                return;
            }
            purge_render_windows(namespace_id);
        });
    }) {
        Ok(handle) => {
            RENDER_CLEANUP_TIMER.with(|timer_slot| {
                *timer_slot.borrow_mut() = Some(handle);
            });
        }
        Err(err) => {
            warn(&format!("failed to schedule render cleanup: {err}"));
        }
    }
}

fn schedule_render_cleanup(namespace_id: u32) {
    let delay_ms = {
        let state = state_lock();
        render_cleanup_delay_ms(&state.config)
    };
    schedule_render_cleanup_timer(namespace_id, delay_ms);
}

fn apply_render_cleanup_action(namespace_id: u32, action: RenderCleanupAction) {
    match action {
        RenderCleanupAction::None => {}
        RenderCleanupAction::Schedule => schedule_render_cleanup(namespace_id),
        RenderCleanupAction::Invalidate => invalidate_render_cleanup(),
    }
}

fn set_on_key_listener(namespace_id: u32, enabled: bool) -> Result<()> {
    let command = if enabled {
        format!(
            "lua vim.on_key(function(_, _) require('rs_smear_cursor').on_key() end, {namespace_id})"
        )
    } else {
        format!("lua vim.on_key(nil, {namespace_id})")
    };
    api::command(&command)?;
    Ok(())
}

fn clear_autocmd_group() {
    let opts = ClearAutocmdsOpts::builder()
        .group(AUTOCMD_GROUP_NAME)
        .build();
    if let Err(err) = api::clear_autocmds(&opts) {
        warn(&format!("clear autocmd group failed: {err}"));
    }
}

fn ensure_namespace_id() -> u32 {
    if let Some(namespace_id) = {
        let state = state_lock();
        state.namespace_id
    } {
        return namespace_id;
    }

    let created = api::create_namespace("rs_smear_cursor");
    let mut state = state_lock();
    match state.namespace_id {
        Some(existing) => existing,
        None => {
            state.namespace_id = Some(created);
            created
        }
    }
}

fn validated_f64(key: &str, value: Object) -> Result<f64> {
    let parsed = f64_from_object(key, value)?;
    if !parsed.is_finite() {
        return Err(invalid_key(key, "finite number"));
    }
    Ok(parsed)
}

fn validated_non_negative_f64(key: &str, value: Object) -> Result<f64> {
    let parsed = validated_f64(key, value)?;
    if parsed < 0.0 {
        return Err(invalid_key(key, "non-negative number"));
    }
    Ok(parsed)
}

fn validated_positive_f64(key: &str, value: Object) -> Result<f64> {
    let parsed = validated_f64(key, value)?;
    if parsed <= 0.0 {
        return Err(invalid_key(key, "positive number"));
    }
    Ok(parsed)
}

fn validated_cterm_color_index(key: &str, value: Object) -> Result<u16> {
    let parsed = i64_from_object(key, value)?;
    if !(0..=255).contains(&parsed) {
        return Err(invalid_key(key, "integer between 0 and 255"));
    }
    Ok(parsed as u16)
}

#[derive(Debug, Clone, PartialEq)]
enum OptionalChange<T> {
    Set(T),
    Clear,
}

#[derive(Debug, Clone, PartialEq)]
struct CtermCursorColorsPatch {
    colors: Vec<u16>,
    color_levels: u32,
}

#[derive(Debug, Default, Clone, PartialEq)]
struct RuntimeOptionsPatch {
    enabled: Option<bool>,
    time_interval: Option<f64>,
    delay_disable: Option<OptionalChange<f64>>,
    delay_event_to_smear: Option<f64>,
    delay_after_key: Option<f64>,
    smear_to_cmd: Option<bool>,
    smear_insert_mode: Option<bool>,
    smear_replace_mode: Option<bool>,
    smear_terminal_mode: Option<bool>,
    vertical_bar_cursor: Option<bool>,
    vertical_bar_cursor_insert_mode: Option<bool>,
    horizontal_bar_cursor_replace_mode: Option<bool>,
    hide_target_hack: Option<bool>,
    windows_zindex: Option<u32>,
    filetypes_disabled: Option<Vec<String>>,
    logging_level: Option<i64>,
    cursor_color: Option<OptionalChange<String>>,
    cursor_color_insert_mode: Option<OptionalChange<String>>,
    normal_bg: Option<OptionalChange<String>>,
    transparent_bg_fallback_color: Option<String>,
    cterm_bg: Option<OptionalChange<u16>>,
    cterm_cursor_colors: Option<OptionalChange<CtermCursorColorsPatch>>,
    smear_between_buffers: Option<bool>,
    smear_between_neighbor_lines: Option<bool>,
    min_horizontal_distance_smear: Option<f64>,
    min_vertical_distance_smear: Option<f64>,
    smear_horizontally: Option<bool>,
    smear_vertically: Option<bool>,
    smear_diagonally: Option<bool>,
    scroll_buffer_space: Option<bool>,
    stiffness: Option<f64>,
    trailing_stiffness: Option<f64>,
    trailing_exponent: Option<f64>,
    stiffness_insert_mode: Option<f64>,
    trailing_stiffness_insert_mode: Option<f64>,
    trailing_exponent_insert_mode: Option<f64>,
    anticipation: Option<f64>,
    damping: Option<f64>,
    damping_insert_mode: Option<f64>,
    distance_stop_animating: Option<f64>,
    distance_stop_animating_vertical_bar: Option<f64>,
    max_length: Option<f64>,
    max_length_insert_mode: Option<f64>,
    particles_enabled: Option<bool>,
    particle_max_num: Option<usize>,
    particle_spread: Option<f64>,
    particles_per_second: Option<f64>,
    particles_per_length: Option<f64>,
    particle_max_lifetime: Option<f64>,
    particle_lifetime_distribution_exponent: Option<f64>,
    particle_max_initial_velocity: Option<f64>,
    particle_velocity_from_cursor: Option<f64>,
    particle_random_velocity: Option<f64>,
    particle_damping: Option<f64>,
    particle_gravity: Option<f64>,
    min_distance_emit_particles: Option<f64>,
    particle_switch_octant_braille: Option<f64>,
    particles_over_text: Option<bool>,
    volume_reduction_exponent: Option<f64>,
    minimum_volume_factor: Option<f64>,
    never_draw_over_target: Option<bool>,
    legacy_computing_symbols_support: Option<bool>,
    legacy_computing_symbols_support_vertical_bars: Option<bool>,
    use_diagonal_blocks: Option<bool>,
    max_slope_horizontal: Option<f64>,
    min_slope_vertical: Option<f64>,
    max_angle_difference_diagonal: Option<f64>,
    max_offset_diagonal: Option<f64>,
    min_shade_no_diagonal: Option<f64>,
    min_shade_no_diagonal_vertical_bar: Option<f64>,
    max_shade_no_matrix: Option<f64>,
    color_levels: Option<u32>,
    gamma: Option<f64>,
    gradient_exponent: Option<f64>,
    matrix_pixel_threshold: Option<f64>,
    matrix_pixel_threshold_vertical_bar: Option<f64>,
    matrix_pixel_min_factor: Option<f64>,
}

fn option_object(opts: &Dictionary, key: &str) -> Option<Object> {
    opts.get(&NvimString::from(key)).cloned()
}

fn parse_optional_with<T, F>(opts: &Dictionary, key: &'static str, parse: F) -> Result<Option<T>>
where
    F: Fn(&str, Object) -> Result<T>,
{
    option_object(opts, key)
        .map(|value| parse(key, value))
        .transpose()
}

fn parse_optional_change_with<T, F>(
    opts: &Dictionary,
    key: &'static str,
    parse: F,
) -> Result<Option<OptionalChange<T>>>
where
    F: Fn(&str, Object) -> Result<T>,
{
    let Some(value) = option_object(opts, key) else {
        return Ok(None);
    };
    if value.is_nil() {
        return Ok(Some(OptionalChange::Clear));
    }
    parse(key, value).map(|parsed| Some(OptionalChange::Set(parsed)))
}

fn parse_optional_non_negative_i64(opts: &Dictionary, key: &'static str) -> Result<Option<i64>> {
    parse_optional_with(opts, key, i64_from_object)?
        .map(|parsed| {
            if parsed < 0 {
                return Err(invalid_key(key, "non-negative integer"));
            }
            Ok(parsed)
        })
        .transpose()
}

fn parse_optional_non_negative_u32(opts: &Dictionary, key: &'static str) -> Result<Option<u32>> {
    parse_optional_non_negative_i64(opts, key)?
        .map(|parsed| u32::try_from(parsed).map_err(|_| invalid_key(key, "non-negative integer")))
        .transpose()
}

fn parse_optional_non_negative_usize(
    opts: &Dictionary,
    key: &'static str,
) -> Result<Option<usize>> {
    parse_optional_non_negative_i64(opts, key)?
        .map(|parsed| usize::try_from(parsed).map_err(|_| invalid_key(key, "non-negative integer")))
        .transpose()
}

fn parse_optional_positive_u32(opts: &Dictionary, key: &'static str) -> Result<Option<u32>> {
    parse_optional_with(opts, key, i64_from_object)?
        .map(|parsed| {
            if parsed < 1 {
                return Err(invalid_key(key, "positive integer"));
            }
            u32::try_from(parsed).map_err(|_| invalid_key(key, "positive integer"))
        })
        .transpose()
}

fn parse_optional_filetypes_disabled(
    opts: &Dictionary,
    key: &'static str,
) -> Result<Option<Vec<String>>> {
    let Some(value) = option_object(opts, key) else {
        return Ok(None);
    };
    if value.is_nil() {
        return Ok(Some(Vec::new()));
    }

    let values =
        parse_indexed_objects(key, value, None).map_err(|_| invalid_key(key, "array[string]"))?;
    let mut filetypes = Vec::with_capacity(values.len());
    for (index, entry) in values.into_iter().enumerate() {
        let entry_key = format!("{key}[{}]", index + 1);
        filetypes.push(string_from_object(&entry_key, entry)?);
    }
    Ok(Some(filetypes))
}

fn parse_optional_cterm_cursor_colors(
    opts: &Dictionary,
    key: &'static str,
) -> Result<Option<OptionalChange<CtermCursorColorsPatch>>> {
    let Some(value) = option_object(opts, key) else {
        return Ok(None);
    };
    if value.is_nil() {
        return Ok(Some(OptionalChange::Clear));
    }

    let values =
        parse_indexed_objects(key, value, None).map_err(|_| invalid_key(key, "array[integer]"))?;
    let mut colors = Vec::with_capacity(values.len());
    for (index, entry) in values.into_iter().enumerate() {
        let entry_key = format!("{key}[{}]", index + 1);
        colors.push(validated_cterm_color_index(&entry_key, entry)?);
    }
    let color_levels =
        u32::try_from(colors.len()).map_err(|_| invalid_key(key, "array length too large"))?;
    Ok(Some(OptionalChange::Set(CtermCursorColorsPatch {
        colors,
        color_levels,
    })))
}

fn apply_optional_change<T>(target: &mut Option<T>, change: OptionalChange<T>) {
    match change {
        OptionalChange::Set(value) => *target = Some(value),
        OptionalChange::Clear => *target = None,
    }
}

impl RuntimeOptionsPatch {
    fn parse(opts: &Dictionary) -> Result<Self> {
        Ok(Self {
            enabled: parse_optional_with(opts, "enabled", bool_from_object)?,
            time_interval: parse_optional_with(opts, "time_interval", validated_f64)?
                .map(|parsed| parsed.max(1.0)),
            delay_disable: parse_optional_change_with(
                opts,
                "delay_disable",
                validated_non_negative_f64,
            )?,
            delay_event_to_smear: parse_optional_with(
                opts,
                "delay_event_to_smear",
                validated_non_negative_f64,
            )?,
            delay_after_key: parse_optional_with(
                opts,
                "delay_after_key",
                validated_non_negative_f64,
            )?,
            smear_to_cmd: parse_optional_with(opts, "smear_to_cmd", bool_from_object)?,
            smear_insert_mode: parse_optional_with(opts, "smear_insert_mode", bool_from_object)?,
            smear_replace_mode: parse_optional_with(opts, "smear_replace_mode", bool_from_object)?,
            smear_terminal_mode: parse_optional_with(
                opts,
                "smear_terminal_mode",
                bool_from_object,
            )?,
            vertical_bar_cursor: parse_optional_with(
                opts,
                "vertical_bar_cursor",
                bool_from_object,
            )?,
            vertical_bar_cursor_insert_mode: parse_optional_with(
                opts,
                "vertical_bar_cursor_insert_mode",
                bool_from_object,
            )?,
            horizontal_bar_cursor_replace_mode: parse_optional_with(
                opts,
                "horizontal_bar_cursor_replace_mode",
                bool_from_object,
            )?,
            hide_target_hack: parse_optional_with(opts, "hide_target_hack", bool_from_object)?,
            windows_zindex: parse_optional_non_negative_u32(opts, "windows_zindex")?,
            filetypes_disabled: parse_optional_filetypes_disabled(opts, "filetypes_disabled")?,
            logging_level: parse_optional_non_negative_i64(opts, "logging_level")?,
            cursor_color: parse_optional_change_with(opts, "cursor_color", string_from_object)?,
            cursor_color_insert_mode: parse_optional_change_with(
                opts,
                "cursor_color_insert_mode",
                string_from_object,
            )?,
            normal_bg: parse_optional_change_with(opts, "normal_bg", string_from_object)?,
            transparent_bg_fallback_color: parse_optional_with(
                opts,
                "transparent_bg_fallback_color",
                string_from_object,
            )?,
            cterm_bg: parse_optional_change_with(opts, "cterm_bg", validated_cterm_color_index)?,
            cterm_cursor_colors: parse_optional_cterm_cursor_colors(opts, "cterm_cursor_colors")?,
            smear_between_buffers: parse_optional_with(
                opts,
                "smear_between_buffers",
                bool_from_object,
            )?,
            smear_between_neighbor_lines: parse_optional_with(
                opts,
                "smear_between_neighbor_lines",
                bool_from_object,
            )?,
            min_horizontal_distance_smear: parse_optional_with(
                opts,
                "min_horizontal_distance_smear",
                validated_non_negative_f64,
            )?,
            min_vertical_distance_smear: parse_optional_with(
                opts,
                "min_vertical_distance_smear",
                validated_non_negative_f64,
            )?,
            smear_horizontally: parse_optional_with(opts, "smear_horizontally", bool_from_object)?,
            smear_vertically: parse_optional_with(opts, "smear_vertically", bool_from_object)?,
            smear_diagonally: parse_optional_with(opts, "smear_diagonally", bool_from_object)?,
            scroll_buffer_space: parse_optional_with(
                opts,
                "scroll_buffer_space",
                bool_from_object,
            )?,
            stiffness: parse_optional_with(opts, "stiffness", validated_non_negative_f64)?,
            trailing_stiffness: parse_optional_with(
                opts,
                "trailing_stiffness",
                validated_non_negative_f64,
            )?,
            trailing_exponent: parse_optional_with(
                opts,
                "trailing_exponent",
                validated_non_negative_f64,
            )?,
            stiffness_insert_mode: parse_optional_with(
                opts,
                "stiffness_insert_mode",
                validated_non_negative_f64,
            )?,
            trailing_stiffness_insert_mode: parse_optional_with(
                opts,
                "trailing_stiffness_insert_mode",
                validated_non_negative_f64,
            )?,
            trailing_exponent_insert_mode: parse_optional_with(
                opts,
                "trailing_exponent_insert_mode",
                validated_non_negative_f64,
            )?,
            anticipation: parse_optional_with(opts, "anticipation", validated_non_negative_f64)?,
            damping: parse_optional_with(opts, "damping", validated_non_negative_f64)?,
            damping_insert_mode: parse_optional_with(
                opts,
                "damping_insert_mode",
                validated_non_negative_f64,
            )?,
            distance_stop_animating: parse_optional_with(
                opts,
                "distance_stop_animating",
                validated_non_negative_f64,
            )?,
            distance_stop_animating_vertical_bar: parse_optional_with(
                opts,
                "distance_stop_animating_vertical_bar",
                validated_non_negative_f64,
            )?,
            max_length: parse_optional_with(opts, "max_length", validated_non_negative_f64)?,
            max_length_insert_mode: parse_optional_with(
                opts,
                "max_length_insert_mode",
                validated_non_negative_f64,
            )?,
            particles_enabled: parse_optional_with(opts, "particles_enabled", bool_from_object)?,
            particle_max_num: parse_optional_non_negative_usize(opts, "particle_max_num")?,
            particle_spread: parse_optional_with(
                opts,
                "particle_spread",
                validated_non_negative_f64,
            )?,
            particles_per_second: parse_optional_with(
                opts,
                "particles_per_second",
                validated_non_negative_f64,
            )?,
            particles_per_length: parse_optional_with(
                opts,
                "particles_per_length",
                validated_non_negative_f64,
            )?,
            particle_max_lifetime: parse_optional_with(
                opts,
                "particle_max_lifetime",
                validated_non_negative_f64,
            )?,
            particle_lifetime_distribution_exponent: parse_optional_with(
                opts,
                "particle_lifetime_distribution_exponent",
                validated_non_negative_f64,
            )?,
            particle_max_initial_velocity: parse_optional_with(
                opts,
                "particle_max_initial_velocity",
                validated_non_negative_f64,
            )?,
            particle_velocity_from_cursor: parse_optional_with(
                opts,
                "particle_velocity_from_cursor",
                validated_non_negative_f64,
            )?,
            particle_random_velocity: parse_optional_with(
                opts,
                "particle_random_velocity",
                validated_non_negative_f64,
            )?,
            particle_damping: parse_optional_with(
                opts,
                "particle_damping",
                validated_non_negative_f64,
            )?,
            particle_gravity: parse_optional_with(
                opts,
                "particle_gravity",
                validated_non_negative_f64,
            )?,
            min_distance_emit_particles: parse_optional_with(
                opts,
                "min_distance_emit_particles",
                validated_non_negative_f64,
            )?,
            particle_switch_octant_braille: parse_optional_with(
                opts,
                "particle_switch_octant_braille",
                validated_non_negative_f64,
            )?,
            particles_over_text: parse_optional_with(
                opts,
                "particles_over_text",
                bool_from_object,
            )?,
            volume_reduction_exponent: parse_optional_with(
                opts,
                "volume_reduction_exponent",
                validated_non_negative_f64,
            )?,
            minimum_volume_factor: parse_optional_with(
                opts,
                "minimum_volume_factor",
                validated_non_negative_f64,
            )?,
            never_draw_over_target: parse_optional_with(
                opts,
                "never_draw_over_target",
                bool_from_object,
            )?,
            legacy_computing_symbols_support: parse_optional_with(
                opts,
                "legacy_computing_symbols_support",
                bool_from_object,
            )?,
            legacy_computing_symbols_support_vertical_bars: parse_optional_with(
                opts,
                "legacy_computing_symbols_support_vertical_bars",
                bool_from_object,
            )?,
            use_diagonal_blocks: parse_optional_with(
                opts,
                "use_diagonal_blocks",
                bool_from_object,
            )?,
            max_slope_horizontal: parse_optional_with(
                opts,
                "max_slope_horizontal",
                validated_non_negative_f64,
            )?,
            min_slope_vertical: parse_optional_with(
                opts,
                "min_slope_vertical",
                validated_non_negative_f64,
            )?,
            max_angle_difference_diagonal: parse_optional_with(
                opts,
                "max_angle_difference_diagonal",
                validated_non_negative_f64,
            )?,
            max_offset_diagonal: parse_optional_with(
                opts,
                "max_offset_diagonal",
                validated_non_negative_f64,
            )?,
            min_shade_no_diagonal: parse_optional_with(
                opts,
                "min_shade_no_diagonal",
                validated_non_negative_f64,
            )?,
            min_shade_no_diagonal_vertical_bar: parse_optional_with(
                opts,
                "min_shade_no_diagonal_vertical_bar",
                validated_non_negative_f64,
            )?,
            max_shade_no_matrix: parse_optional_with(
                opts,
                "max_shade_no_matrix",
                validated_non_negative_f64,
            )?,
            color_levels: parse_optional_positive_u32(opts, "color_levels")?,
            gamma: parse_optional_with(opts, "gamma", validated_positive_f64)?,
            gradient_exponent: parse_optional_with(
                opts,
                "gradient_exponent",
                validated_non_negative_f64,
            )?,
            matrix_pixel_threshold: parse_optional_with(
                opts,
                "matrix_pixel_threshold",
                validated_non_negative_f64,
            )?,
            matrix_pixel_threshold_vertical_bar: parse_optional_with(
                opts,
                "matrix_pixel_threshold_vertical_bar",
                validated_non_negative_f64,
            )?,
            matrix_pixel_min_factor: parse_optional_with(
                opts,
                "matrix_pixel_min_factor",
                validated_non_negative_f64,
            )?,
        })
    }

    fn apply(self, state: &mut RuntimeState) {
        if let Some(value) = self.enabled {
            state.set_enabled(value);
        }
        if let Some(value) = self.time_interval {
            state.config.time_interval = value;
        }
        if let Some(value) = self.delay_disable {
            match value {
                OptionalChange::Set(delay) => state.config.delay_disable = Some(delay),
                OptionalChange::Clear => state.config.delay_disable = None,
            }
        }
        if let Some(value) = self.delay_event_to_smear {
            state.config.delay_event_to_smear = value;
        }
        if let Some(value) = self.delay_after_key {
            state.config.delay_after_key = value;
        }
        if let Some(value) = self.smear_to_cmd {
            state.config.smear_to_cmd = value;
        }
        if let Some(value) = self.smear_insert_mode {
            state.config.smear_insert_mode = value;
        }
        if let Some(value) = self.smear_replace_mode {
            state.config.smear_replace_mode = value;
        }
        if let Some(value) = self.smear_terminal_mode {
            state.config.smear_terminal_mode = value;
        }
        if let Some(value) = self.vertical_bar_cursor {
            state.config.vertical_bar_cursor = value;
        }
        if let Some(value) = self.vertical_bar_cursor_insert_mode {
            state.config.vertical_bar_cursor_insert_mode = value;
        }
        if let Some(value) = self.horizontal_bar_cursor_replace_mode {
            state.config.horizontal_bar_cursor_replace_mode = value;
        }
        if let Some(value) = self.hide_target_hack {
            state.config.hide_target_hack = value;
        }
        if let Some(value) = self.windows_zindex {
            state.config.windows_zindex = value;
        }
        if let Some(value) = self.filetypes_disabled {
            state.config.filetypes_disabled = value;
        }
        if let Some(value) = self.logging_level {
            state.config.logging_level = value;
            set_log_level(value);
        }
        if let Some(change) = self.cursor_color {
            apply_optional_change(&mut state.config.cursor_color, change);
        }
        if let Some(change) = self.cursor_color_insert_mode {
            apply_optional_change(&mut state.config.cursor_color_insert_mode, change);
        }
        if let Some(change) = self.normal_bg {
            apply_optional_change(&mut state.config.normal_bg, change);
        }
        if let Some(value) = self.transparent_bg_fallback_color {
            state.config.transparent_bg_fallback_color = value;
        }
        if let Some(change) = self.cterm_bg {
            apply_optional_change(&mut state.config.cterm_bg, change);
        }
        if let Some(change) = self.cterm_cursor_colors {
            match change {
                OptionalChange::Set(patch) => {
                    state.config.color_levels = patch.color_levels;
                    state.config.cterm_cursor_colors = Some(patch.colors);
                }
                OptionalChange::Clear => state.config.cterm_cursor_colors = None,
            }
        }
        if let Some(value) = self.smear_between_buffers {
            state.config.smear_between_buffers = value;
        }
        if let Some(value) = self.smear_between_neighbor_lines {
            state.config.smear_between_neighbor_lines = value;
        }
        if let Some(value) = self.min_horizontal_distance_smear {
            state.config.min_horizontal_distance_smear = value;
        }
        if let Some(value) = self.min_vertical_distance_smear {
            state.config.min_vertical_distance_smear = value;
        }
        if let Some(value) = self.smear_horizontally {
            state.config.smear_horizontally = value;
        }
        if let Some(value) = self.smear_vertically {
            state.config.smear_vertically = value;
        }
        if let Some(value) = self.smear_diagonally {
            state.config.smear_diagonally = value;
        }
        if let Some(value) = self.scroll_buffer_space {
            state.config.scroll_buffer_space = value;
        }
        if let Some(value) = self.stiffness {
            state.config.stiffness = value;
        }
        if let Some(value) = self.trailing_stiffness {
            state.config.trailing_stiffness = value;
        }
        if let Some(value) = self.trailing_exponent {
            state.config.trailing_exponent = value;
        }
        if let Some(value) = self.stiffness_insert_mode {
            state.config.stiffness_insert_mode = value;
        }
        if let Some(value) = self.trailing_stiffness_insert_mode {
            state.config.trailing_stiffness_insert_mode = value;
        }
        if let Some(value) = self.trailing_exponent_insert_mode {
            state.config.trailing_exponent_insert_mode = value;
        }
        if let Some(value) = self.anticipation {
            state.config.anticipation = value;
        }
        if let Some(value) = self.damping {
            state.config.damping = value;
        }
        if let Some(value) = self.damping_insert_mode {
            state.config.damping_insert_mode = value;
        }
        if let Some(value) = self.distance_stop_animating {
            state.config.distance_stop_animating = value;
        }
        if let Some(value) = self.distance_stop_animating_vertical_bar {
            state.config.distance_stop_animating_vertical_bar = value;
        }
        if let Some(value) = self.max_length {
            state.config.max_length = value;
        }
        if let Some(value) = self.max_length_insert_mode {
            state.config.max_length_insert_mode = value;
        }
        if let Some(value) = self.particles_enabled {
            state.config.particles_enabled = value;
        }
        if let Some(value) = self.particle_max_num {
            state.config.particle_max_num = value;
        }
        if let Some(value) = self.particle_spread {
            state.config.particle_spread = value;
        }
        if let Some(value) = self.particles_per_second {
            state.config.particles_per_second = value;
        }
        if let Some(value) = self.particles_per_length {
            state.config.particles_per_length = value;
        }
        if let Some(value) = self.particle_max_lifetime {
            state.config.particle_max_lifetime = value;
        }
        if let Some(value) = self.particle_lifetime_distribution_exponent {
            state.config.particle_lifetime_distribution_exponent = value;
        }
        if let Some(value) = self.particle_max_initial_velocity {
            state.config.particle_max_initial_velocity = value;
        }
        if let Some(value) = self.particle_velocity_from_cursor {
            state.config.particle_velocity_from_cursor = value;
        }
        if let Some(value) = self.particle_random_velocity {
            state.config.particle_random_velocity = value;
        }
        if let Some(value) = self.particle_damping {
            state.config.particle_damping = value;
        }
        if let Some(value) = self.particle_gravity {
            state.config.particle_gravity = value;
        }
        if let Some(value) = self.min_distance_emit_particles {
            state.config.min_distance_emit_particles = value;
        }
        if let Some(value) = self.particle_switch_octant_braille {
            state.config.particle_switch_octant_braille = value;
        }
        if let Some(value) = self.particles_over_text {
            state.config.particles_over_text = value;
        }
        if let Some(value) = self.volume_reduction_exponent {
            state.config.volume_reduction_exponent = value;
        }
        if let Some(value) = self.minimum_volume_factor {
            state.config.minimum_volume_factor = value;
        }
        if let Some(value) = self.never_draw_over_target {
            state.config.never_draw_over_target = value;
        }
        if let Some(value) = self.legacy_computing_symbols_support {
            state.config.legacy_computing_symbols_support = value;
        }
        if let Some(value) = self.legacy_computing_symbols_support_vertical_bars {
            state.config.legacy_computing_symbols_support_vertical_bars = value;
        }
        if let Some(value) = self.use_diagonal_blocks {
            state.config.use_diagonal_blocks = value;
        }
        if let Some(value) = self.max_slope_horizontal {
            state.config.max_slope_horizontal = value;
        }
        if let Some(value) = self.min_slope_vertical {
            state.config.min_slope_vertical = value;
        }
        if let Some(value) = self.max_angle_difference_diagonal {
            state.config.max_angle_difference_diagonal = value;
        }
        if let Some(value) = self.max_offset_diagonal {
            state.config.max_offset_diagonal = value;
        }
        if let Some(value) = self.min_shade_no_diagonal {
            state.config.min_shade_no_diagonal = value;
        }
        if let Some(value) = self.min_shade_no_diagonal_vertical_bar {
            state.config.min_shade_no_diagonal_vertical_bar = value;
        }
        if let Some(value) = self.max_shade_no_matrix {
            state.config.max_shade_no_matrix = value;
        }
        if let Some(value) = self.color_levels {
            state.config.color_levels = value;
        }
        if let Some(value) = self.gamma {
            state.config.gamma = value;
        }
        if let Some(value) = self.gradient_exponent {
            state.config.gradient_exponent = value;
        }
        if let Some(value) = self.matrix_pixel_threshold {
            state.config.matrix_pixel_threshold = value;
        }
        if let Some(value) = self.matrix_pixel_threshold_vertical_bar {
            state.config.matrix_pixel_threshold_vertical_bar = value;
        }
        if let Some(value) = self.matrix_pixel_min_factor {
            state.config.matrix_pixel_min_factor = value;
        }

        if !should_track_cursor_color(&state.config) {
            state.color_at_cursor = None;
        }
    }
}

fn apply_runtime_options(state: &mut RuntimeState, opts: &Dictionary) -> Result<()> {
    let patch = RuntimeOptionsPatch::parse(opts)?;
    patch.apply(state);
    Ok(())
}

fn on_cursor_event_impl(source: EventSource) -> Result<()> {
    let mode = mode_string();
    let namespace_id = ensure_namespace_id();
    let smear_to_cmd = {
        let state = state_lock();
        state.config.smear_to_cmd
    };

    let window = api::get_current_win();
    if !window.is_valid() {
        return Ok(());
    }

    let buffer = api::get_current_buf();
    if !buffer.is_valid() {
        return Ok(());
    }

    let fallback_target_position = match source {
        EventSource::AnimationTick => {
            let state = state_lock();
            if state.is_initialized() {
                Some((state.target_position.row, state.target_position.col))
            } else {
                None
            }
        }
        EventSource::External => None,
    };

    let maybe_cursor_position = if let Some(position) = fallback_target_position {
        Some(position)
    } else {
        cursor_position_for_mode(&window, &mode, smear_to_cmd)?
    };

    let Some((cursor_row, cursor_col)) = maybe_cursor_position else {
        // Match upstream behavior: ignore transient cursor-position misses instead of
        // clearing the current smear state.
        return Ok(());
    };

    let current_win_handle = i64::from(window.handle());
    let current_buf_handle = i64::from(buffer.handle());
    let current_top_row = line_value("w0")?;
    let current_line = line_value(".")?;
    let current_location = CursorLocation::new(
        current_win_handle,
        current_buf_handle,
        current_top_row,
        current_line,
    );
    let (scroll_buffer_space, previous_location, current_corners) = {
        let state = state_lock();
        (
            state.config.scroll_buffer_space,
            state.tracked_location(),
            state.current_corners,
        )
    };
    let scroll_shift = match source {
        EventSource::External => maybe_scroll_shift(
            &window,
            scroll_buffer_space,
            &current_corners,
            previous_location,
            current_location,
        )?,
        EventSource::AnimationTick => None,
    };

    let event_now_ms = now_ms();
    let event = CursorEventContext {
        row: cursor_row,
        col: cursor_col,
        now_ms: event_now_ms,
        seed: seed_from_clock(),
        cursor_location: current_location,
        scroll_shift,
    };

    let effects = {
        let mut state = state_lock();
        state.namespace_id = Some(namespace_id);
        reduce_cursor_event(&mut state, &mode, event, source)
    };

    if effects.notify_delay_disabled
        && let Err(err) = api::command(
            "lua vim.notify(\"Smear cursor disabled in the current buffer due to high delay.\")",
        )
    {
        warn(&format!("delay-disabled notify failed: {err}"));
    }

    match effects.render_action {
        RenderAction::Draw(frame) => {
            let redraw_cmd = mode == "c" && smear_outside_cmd_row(&frame.corners)?;
            draw_current(namespace_id, &frame)?;
            if redraw_cmd && let Err(err) = api::command("redraw") {
                debug(&format!("redraw after draw failed: {err}"));
            }

            let should_unhide_while_animating =
                mode == "c" || (frame.never_draw_over_target && frame_reaches_target_cell(&frame));
            if mode != "c" && !should_unhide_while_animating && frame.hide_target_hack {
                draw_target_hack_block(namespace_id, &frame)?;
            }
            let mut state = state_lock();
            if should_unhide_while_animating {
                if state.is_cursor_hidden() {
                    if !state.config.hide_target_hack {
                        unhide_real_cursor();
                    }
                    state.set_cursor_hidden(false);
                }
            } else if !state.is_cursor_hidden() && mode != "c" {
                if !state.config.hide_target_hack {
                    hide_real_cursor();
                }
                state.set_cursor_hidden(true);
            }
        }
        RenderAction::ClearAll => {
            clear_active_render_windows(namespace_id);
            if mode == "c"
                && let Err(err) = api::command("redraw")
            {
                debug(&format!("redraw after clear failed: {err}"));
            }
            let mut state = state_lock();
            if state.is_cursor_hidden() {
                if !state.config.hide_target_hack {
                    unhide_real_cursor();
                }
                state.set_cursor_hidden(false);
            }
        }
        RenderAction::Noop => {}
    }

    apply_render_cleanup_action(namespace_id, effects.render_cleanup_action);

    let callback_duration_ms = (now_ms() - event_now_ms).max(0.0);
    let should_schedule = match source {
        EventSource::AnimationTick => true,
        EventSource::External => effects.step_interval_ms.is_some(),
    };
    let maybe_delay = {
        let mut state = state_lock();
        if should_schedule
            && state.is_enabled()
            && state.is_animating()
            && !state.is_delay_disabled()
        {
            let step_interval_ms = effects
                .step_interval_ms
                .unwrap_or(state.config.time_interval);
            Some(next_animation_delay_ms(
                &mut state,
                step_interval_ms,
                callback_duration_ms,
            ))
        } else {
            None
        }
    };
    match (maybe_delay, source) {
        (Some(delay), _) => schedule_animation_tick(delay),
        (None, EventSource::AnimationTick) => clear_animation_timer(),
        (None, EventSource::External) => {}
    }

    Ok(())
}

fn on_external_event_trigger() -> Result<()> {
    if !is_enabled() {
        let mut state = state_lock();
        state.clear_pending_external_event();
        return Ok(());
    }

    let buffer = api::get_current_buf();
    if !buffer.is_valid() {
        return Ok(());
    }
    if skip_current_buffer_events(&buffer)? {
        let mut state = state_lock();
        state.clear_pending_external_event();
        return Ok(());
    }

    let mode = mode_string();
    let (smear_to_cmd, delay_ms) = {
        let state = state_lock();
        (
            state.config.smear_to_cmd,
            external_settle_delay_ms(state.config.delay_event_to_smear),
        )
    };
    if mode == "c" && !smear_to_cmd {
        clear_external_event_timer();
        let mut state = state_lock();
        state.clear_pending_external_event();
        return Ok(());
    }

    let snapshot = current_cursor_snapshot(smear_to_cmd)?;
    let Some(snapshot) = snapshot else {
        {
            let mut state = state_lock();
            state.clear_pending_external_event();
        }
        return Ok(());
    };

    {
        let mut state = state_lock();
        state.set_pending_external_event(Some(snapshot));
    }
    schedule_external_event_timer(delay_ms);
    Ok(())
}

fn schedule_external_event_trigger() {
    if !mark_external_trigger_pending_if_idle() {
        return;
    }

    schedule(|_| {
        clear_external_trigger_pending();
        if let Err(err) = on_external_event_trigger() {
            warn(&format!("external event trigger failed: {err}"));
        }
    });
}

pub(crate) fn on_key_event() -> Result<()> {
    if !is_enabled() {
        return Ok(());
    }

    let buffer = api::get_current_buf();
    if !buffer.is_valid() {
        return Ok(());
    }
    if skip_current_buffer_events(&buffer)? {
        return Ok(());
    }
    let now = now_ms();
    let (key_delay_ms, delay_after_key_ms) = {
        let state = state_lock();
        (
            as_delay_ms(state.config.delay_after_key),
            state.config.delay_after_key.max(0.0),
        )
    };
    if elapsed_ms_since_last_autocmd_event(now) <= delay_after_key_ms {
        return Ok(());
    }
    schedule_key_event_timer(key_delay_ms);
    Ok(())
}

fn replace_real_cursor_from_event(only_hide_real_cursor: bool) -> Result<()> {
    let mode = mode_string();
    let namespace_id = ensure_namespace_id();

    let maybe_frame = {
        let mut state = state_lock();
        state.namespace_id = Some(namespace_id);

        if !state.is_enabled()
            || state.is_animating()
            || state.config.hide_target_hack
            || !state.config.mode_allowed(&mode)
            || !smear_outside_cmd_row(&state.current_corners)?
        {
            return Ok(());
        }

        if !state.is_cursor_hidden() {
            hide_real_cursor();
            state.set_cursor_hidden(true);
        }

        if only_hide_real_cursor {
            return Ok(());
        }

        let vertical_bar = state.config.cursor_is_vertical_bar(&mode);
        let mut frame = build_render_frame(
            &state,
            &mode,
            state.current_corners,
            Point {
                row: -1.0,
                col: -1.0,
            },
            vertical_bar,
            None,
        );
        frame.particles.clear();
        Some(frame)
    };

    if let Some(frame) = maybe_frame {
        draw_current(namespace_id, &frame)?;
    }

    Ok(())
}

fn on_cursor_event(args: AutocmdCallbackArgs) -> bool {
    let result = (|| -> Result<()> {
        if !is_enabled() {
            return Ok(());
        }

        let buffer = api::get_current_buf();
        if !buffer.is_valid() {
            return Ok(());
        }
        if skip_current_buffer_events(&buffer)? {
            return Ok(());
        }

        note_autocmd_event_now();

        // `vim.on_key` is a fallback for cursor-affecting actions that do not emit
        // movement/window autocmds. Once an autocmd arrived, discard any pending
        // key-triggered callback to avoid duplicate smears after key sequences.
        clear_key_event_timer();

        if args.event == "CursorMoved" || args.event == "CursorMovedI" {
            let mut state = state_lock();
            update_tracked_cursor_color(&mut state);
        }
        replace_real_cursor_from_event(false)?;
        schedule_external_event_trigger();
        Ok(())
    })();

    match result {
        Ok(()) => false,
        Err(err) => {
            warn(&format!("cursor event failed: {err}"));
            false
        }
    }
}

fn on_buf_enter(_args: AutocmdCallbackArgs) -> bool {
    let mut state = state_lock();
    state.set_delay_disabled(false);
    false
}

fn on_colorscheme(_args: AutocmdCallbackArgs) -> bool {
    clear_highlight_cache();
    false
}

fn jump_to_current_cursor() -> Result<()> {
    let namespace_id = ensure_namespace_id();
    let mode = mode_string();
    let window = api::get_current_win();
    if !window.is_valid() {
        return Ok(());
    }

    let buffer = api::get_current_buf();
    if !buffer.is_valid() {
        return Ok(());
    }

    let smear_to_cmd = {
        let state = state_lock();
        state.config.smear_to_cmd
    };
    let Some((row, col)) = cursor_position_for_mode(&window, &mode, smear_to_cmd)? else {
        return Ok(());
    };

    let location = CursorLocation::new(
        i64::from(window.handle()),
        i64::from(buffer.handle()),
        line_value("w0")?,
        line_value(".")?,
    );

    let (hide_target_hack, unhide_cursor) = {
        let mut state = state_lock();
        state.namespace_id = Some(namespace_id);

        let cursor_shape = CursorShape::new(
            state.config.cursor_is_vertical_bar(&mode),
            state.config.cursor_is_horizontal_bar(&mode),
        );
        let unhide_cursor =
            state.sync_to_current_cursor(Point { row, col }, cursor_shape, location);

        (state.config.hide_target_hack, unhide_cursor)
    };

    reset_transient_event_state();
    clear_all_namespaces(namespace_id);
    if unhide_cursor && !hide_target_hack {
        unhide_real_cursor();
    }

    Ok(())
}

fn setup_autocmds() -> Result<()> {
    let group = api::create_augroup(
        AUTOCMD_GROUP_NAME,
        &CreateAugroupOpts::builder().clear(true).build(),
    )?;

    let move_opts = CreateAutocmdOpts::builder()
        .group(group)
        .callback(on_cursor_event)
        .build();
    api::create_autocmd(
        [
            "CmdlineChanged",
            "CursorMoved",
            "CursorMovedI",
            "ModeChanged",
            "WinScrolled",
        ],
        &move_opts,
    )?;

    let buf_enter_opts = CreateAutocmdOpts::builder()
        .group(group)
        .callback(on_buf_enter)
        .build();
    api::create_autocmd(["BufEnter"], &buf_enter_opts)?;

    let colorscheme_opts = CreateAutocmdOpts::builder()
        .group(group)
        .callback(on_colorscheme)
        .build();
    api::create_autocmd(["ColorScheme"], &colorscheme_opts)?;

    Ok(())
}

fn setup_user_command() -> Result<()> {
    if let Err(err) = api::del_user_command("SmearCursorToggle") {
        debug(&format!(
            "delete existing SmearCursorToggle failed (continuing): {err}"
        ));
    }
    api::create_user_command(
        "SmearCursorToggle",
        "lua require('rs_smear_cursor').toggle()",
        &CreateCommandOpts::builder().build(),
    )?;
    Ok(())
}

pub(crate) fn setup(opts: Dictionary) -> Result<()> {
    let namespace_id = ensure_namespace_id();
    ensure_hideable_guicursor();
    unhide_real_cursor();
    clear_highlight_cache();
    let has_enabled_option = opts.get(&NvimString::from("enabled")).is_some();
    let enabled = {
        let mut state = state_lock();
        state.namespace_id = Some(namespace_id);
        // Upstream Lua setup defaults enabled=true when omitted.
        if !has_enabled_option {
            state.set_enabled(true);
        }
        apply_runtime_options(&mut state, &opts)?;
        set_log_level(state.config.logging_level);
        state.clear_runtime_state();
        state.is_enabled()
    };
    reset_transient_event_state();

    setup_user_command()?;
    clear_autocmd_group();
    set_on_key_listener(namespace_id, false)?;
    if enabled {
        setup_autocmds()?;
        set_on_key_listener(namespace_id, true)?;
    }
    jump_to_current_cursor()?;
    Ok(())
}

pub(crate) fn toggle() -> Result<()> {
    let (is_enabled, namespace_id, hide_target_hack) = {
        let mut state = state_lock();
        let toggled_enabled = !state.is_enabled();
        if toggled_enabled {
            state.set_enabled(true);
        } else {
            state.disable();
        }
        (
            state.is_enabled(),
            state.namespace_id,
            state.config.hide_target_hack,
        )
    };

    if let Some(namespace_id) = namespace_id {
        if is_enabled {
            setup_autocmds()?;
            set_on_key_listener(namespace_id, true)?;
            jump_to_current_cursor()?;
        } else {
            clear_autocmd_group();
            set_on_key_listener(namespace_id, false)?;
            reset_transient_event_state();
            clear_all_namespaces(namespace_id);
            if !hide_target_hack {
                unhide_real_cursor();
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        EngineState, MIN_RENDER_CLEANUP_DELAY_MS, OptionalChange, RenderCleanupGeneration,
        RuntimeOptionsPatch, apply_runtime_options, render_cleanup_delay_ms,
    };
    use crate::config::RuntimeConfig;
    use crate::state::RuntimeState;
    use nvim_oxi::{Array, Dictionary, Object};

    #[test]
    fn cleanup_delay_has_floor() {
        let config = RuntimeConfig {
            time_interval: 0.0,
            delay_event_to_smear: 0.0,
            delay_after_key: 0.0,
            ..RuntimeConfig::default()
        };

        assert_eq!(
            render_cleanup_delay_ms(&config),
            MIN_RENDER_CLEANUP_DELAY_MS
        );
    }

    #[test]
    fn cleanup_delay_tracks_config_when_above_floor() {
        let config = RuntimeConfig {
            time_interval: 160.0,
            delay_event_to_smear: 40.0,
            delay_after_key: 20.0,
            ..RuntimeConfig::default()
        };

        assert_eq!(render_cleanup_delay_ms(&config), 220);
    }

    #[test]
    fn cleanup_generation_bumps_and_wraps() {
        let mut generation = RenderCleanupGeneration::default();
        assert_eq!(generation.current(), 0);
        assert_eq!(generation.bump(), 1);
        assert_eq!(generation.current(), 1);

        generation = RenderCleanupGeneration { value: u64::MAX };
        assert_eq!(generation.bump(), 0);
        assert_eq!(generation.current(), 0);
    }

    #[test]
    fn engine_state_exposes_cleanup_generation_transitions() {
        let mut state = EngineState::default();
        assert_eq!(state.current_render_cleanup_generation(), 0);
        assert_eq!(state.bump_render_cleanup_generation(), 1);
        assert_eq!(state.bump_render_cleanup_generation(), 2);
        assert_eq!(state.current_render_cleanup_generation(), 2);
    }

    #[test]
    fn runtime_options_patch_apply_clears_nullable_fields() {
        let mut state = RuntimeState::default();
        state.config.delay_disable = Some(12.0);
        state.config.cursor_color = Some("#abcdef".to_string());
        let patch = RuntimeOptionsPatch {
            delay_disable: Some(OptionalChange::Clear),
            cursor_color: Some(OptionalChange::Clear),
            ..RuntimeOptionsPatch::default()
        };

        patch.apply(&mut state);
        assert_eq!(state.config.delay_disable, None);
        assert_eq!(state.config.cursor_color, None);
    }

    #[test]
    fn runtime_options_patch_explicit_color_levels_override_cterm_array_length() {
        let mut state = RuntimeState::default();
        let mut opts = Dictionary::new();
        opts.insert(
            "cterm_cursor_colors",
            Object::from(Array::from_iter([
                Object::from(17_i64),
                Object::from(42_i64),
            ])),
        );
        opts.insert("color_levels", 9_i64);

        let result = apply_runtime_options(&mut state, &opts);
        assert!(
            result.is_ok(),
            "unexpected runtime option error: {result:?}"
        );
        assert_eq!(state.config.cterm_cursor_colors, Some(vec![17_u16, 42_u16]));
        assert_eq!(state.config.color_levels, 9_u32);
    }

    #[test]
    fn runtime_options_patch_apply_clears_filetypes_list() {
        let mut state = RuntimeState::default();
        state.config.filetypes_disabled = vec!["lua".to_string(), "nix".to_string()];
        let patch = RuntimeOptionsPatch {
            filetypes_disabled: Some(Vec::new()),
            ..RuntimeOptionsPatch::default()
        };

        patch.apply(&mut state);
        assert!(state.config.filetypes_disabled.is_empty());
    }
}
