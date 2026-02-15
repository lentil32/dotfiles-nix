use crate::config::RuntimeConfig;
use crate::draw::{
    RenderFrame, clear_active_render_windows, clear_all_namespaces, clear_highlight_cache,
    draw_current, draw_target_hack_block, purge_render_windows,
};
use crate::lua::{i64_from_object, parse_indexed_objects, string_from_object};
use crate::reducer::{
    CursorCommand, CursorEventContext, EventSource, RenderAction, RenderCleanupAction, ScrollShift,
    as_delay_ms, build_render_frame, external_settle_delay_ms, next_animation_delay_ms,
    reduce_cursor_event,
};
use crate::state::{CursorLocation, CursorShape, CursorSnapshot, RuntimeState};
use crate::types::{DEFAULT_RNG_STATE, EPSILON, Point};
use nvim_oxi::api;
use nvim_oxi::api::opts::{
    CreateAugroupOpts, CreateAutocmdOpts, CreateCommandOpts, OptionOpts, WinTextHeightOpts,
};
use nvim_oxi::api::types::AutocmdCallbackArgs;
use nvim_oxi::conversion::FromObject;
use nvim_oxi::libuv::TimerHandle;
use nvim_oxi::schedule;
use nvim_oxi::{Array, Dictionary, Object, Result, String as NvimString};
use std::ops::{Deref, DerefMut};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

mod event_loop;
mod options;

#[cfg(test)]
use event_loop::EventLoopState;
use options::apply_runtime_options;

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

#[derive(Debug)]
struct EngineContext {
    state: Mutex<EngineState>,
    log_level: AtomicI64,
}

impl EngineContext {
    fn new() -> Self {
        Self {
            state: Mutex::new(EngineState::default()),
            log_level: AtomicI64::new(LOG_LEVEL_INFO),
        }
    }
}

static ENGINE_CONTEXT: LazyLock<EngineContext> = LazyLock::new(EngineContext::new);

fn set_log_level(level: i64) {
    let normalized = if level < 0 { 0 } else { level };
    ENGINE_CONTEXT
        .log_level
        .store(normalized, Ordering::Relaxed);
}

fn should_log(level: i64) -> bool {
    ENGINE_CONTEXT.log_level.load(Ordering::Relaxed) <= level
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
        api::err_writeln(&format!("[{LOG_SOURCE_NAME}] vim.notify failed: {err}"));
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
    event_loop::clear_animation_timer();
}

fn clear_external_event_timer() {
    event_loop::clear_external_event_timer();
}

fn clear_key_event_timer() {
    event_loop::clear_key_event_timer();
}

fn clear_render_cleanup_timer() {
    event_loop::clear_render_cleanup_timer();
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
    event_loop::clear_external_trigger_pending();
}

fn mark_external_trigger_pending_if_idle() -> bool {
    event_loop::mark_external_trigger_pending_if_idle()
}

fn note_autocmd_event_now() {
    event_loop::note_autocmd_event(now_ms());
}

fn note_external_dispatch_now() {
    event_loop::note_external_dispatch(now_ms());
}

fn clear_autocmd_event_timestamp() {
    event_loop::clear_autocmd_event_timestamp();
}

fn clear_external_dispatch_timestamp() {
    event_loop::clear_external_dispatch_timestamp();
}

fn elapsed_ms_since_last_autocmd_event(now_ms: f64) -> f64 {
    event_loop::elapsed_ms_since_last_autocmd_event(now_ms)
}

fn elapsed_ms_since_last_external_dispatch(now_ms: f64) -> f64 {
    event_loop::elapsed_ms_since_last_external_dispatch(now_ms)
}

fn reset_transient_event_state() {
    clear_animation_timer();
    clear_external_event_timer();
    clear_key_event_timer();
    clear_external_trigger_pending();
    clear_autocmd_event_timestamp();
    clear_external_dispatch_timestamp();
    invalidate_render_cleanup();
}

fn reset_transient_event_state_without_generation_bump() {
    clear_animation_timer();
    clear_external_event_timer();
    clear_key_event_timer();
    clear_external_trigger_pending();
    clear_autocmd_event_timestamp();
    clear_external_dispatch_timestamp();
    clear_render_cleanup_timer();
}

fn engine_lock() -> std::sync::MutexGuard<'static, EngineState> {
    loop {
        match ENGINE_CONTEXT.state.lock() {
            Ok(guard) => return guard,
            Err(poisoned) => {
                let mut guard = poisoned.into_inner();
                let namespace_id = guard.runtime.namespace_id();
                *guard = EngineState::default();
                drop(guard);

                ENGINE_CONTEXT.state.clear_poison();
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

    if window.get_config()?.relative.is_some() {
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

fn current_buffer_option_string(buffer: &api::Buffer, option_name: &str) -> Result<String> {
    let opts = OptionOpts::builder().buf(buffer.clone()).build();
    let value: String = api::get_option_value(option_name, &opts)?;
    Ok(value)
}

fn current_buffer_filetype(buffer: &api::Buffer) -> Result<String> {
    current_buffer_option_string(buffer, "filetype")
}

fn current_buffer_buftype(buffer: &api::Buffer) -> Result<String> {
    current_buffer_option_string(buffer, "buftype")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BufferEventPolicy {
    Normal,
    ThrottledUi,
}

impl BufferEventPolicy {
    const THROTTLED_UI_DELAY_FLOOR_MS: u64 = 12;

    fn from_buftype(buftype: &str) -> Self {
        match buftype {
            "" | "acwrite" => Self::Normal,
            _ => Self::ThrottledUi,
        }
    }

    const fn settle_delay_floor_ms(self) -> u64 {
        match self {
            Self::Normal => 0,
            Self::ThrottledUi => Self::THROTTLED_UI_DELAY_FLOOR_MS,
        }
    }

    const fn should_use_debounced_external_settle(self) -> bool {
        matches!(self, Self::Normal)
    }

    const fn use_key_fallback(self) -> bool {
        true
    }

    const fn should_prepaint_cursor(self) -> bool {
        true
    }
}

fn remaining_throttle_delay_ms(throttle_interval_ms: u64, elapsed_ms: f64) -> u64 {
    as_delay_ms((throttle_interval_ms as f64 - elapsed_ms).max(0.0))
}

fn should_replace_external_timer_with_throttle(
    existing_kind: Option<event_loop::ExternalEventTimerKind>,
) -> bool {
    matches!(
        existing_kind,
        Some(event_loop::ExternalEventTimerKind::Settle)
    )
}

fn current_buffer_event_policy(buffer: &api::Buffer) -> Result<BufferEventPolicy> {
    let buftype = current_buffer_buftype(buffer)?;
    Ok(BufferEventPolicy::from_buftype(&buftype))
}

fn skip_current_buffer_events(buffer: &api::Buffer) -> Result<bool> {
    let filetypes_disabled = {
        let state = state_lock();
        if state.is_delay_disabled() {
            return Ok(true);
        }
        state.config.filetypes_disabled.clone()
    };

    if filetypes_disabled.is_empty() {
        return Ok(false);
    }

    let filetype = current_buffer_filetype(buffer)?;
    Ok(filetypes_disabled.iter().any(|entry| entry == &filetype))
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
    if !state.config.requires_cursor_color_sampling() {
        state.clear_color_at_cursor();
        return;
    }

    match cursor_color_at_current_position() {
        Ok(color) => state.set_color_at_cursor(color),
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
    let already_scheduled = event_loop::is_animation_timer_scheduled();
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
            event_loop::set_animation_timer(handle);
        }
        Err(err) => {
            warn(&format!("failed to schedule animation tick: {err}"));
        }
    }
}

fn schedule_external_throttle_timer(delay_ms: u64) {
    let existing_kind = event_loop::external_event_timer_kind();
    if existing_kind == Some(event_loop::ExternalEventTimerKind::Throttle) {
        return;
    }
    if should_replace_external_timer_with_throttle(existing_kind) {
        clear_external_event_timer();
    }

    let timeout = Duration::from_millis(delay_ms);
    match TimerHandle::once(timeout, || {
        schedule(|_| {
            clear_external_event_timer();
            if let Err(err) = on_external_event_trigger() {
                warn(&format!("external throttle tick failed: {err}"));
            }
        });
    }) {
        Ok(handle) => {
            event_loop::set_external_event_timer(
                handle,
                event_loop::ExternalEventTimerKind::Throttle,
            );
        }
        Err(err) => {
            warn(&format!("failed to schedule external throttle tick: {err}"));
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
            event_loop::set_external_event_timer(
                handle,
                event_loop::ExternalEventTimerKind::Settle,
            );
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
            event_loop::set_key_event_timer(handle);
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
            event_loop::set_render_cleanup_timer(handle);
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
    let opts = CreateAugroupOpts::builder().clear(true).build();
    if let Err(err) = api::create_augroup(AUTOCMD_GROUP_NAME, &opts) {
        warn(&format!("clear autocmd group failed: {err}"));
    }
}

fn ensure_namespace_id() -> u32 {
    if let Some(namespace_id) = {
        let state = state_lock();
        state.namespace_id()
    } {
        return namespace_id;
    }

    let created = api::create_namespace("rs_smear_cursor");
    let mut state = state_lock();
    match state.namespace_id() {
        Some(existing) => existing,
        None => {
            state.set_namespace_id(created);
            created
        }
    }
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
                let target = state.target_position();
                Some((target.row, target.col))
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
            state.current_corners(),
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

    let transition = {
        let mut state = state_lock();
        state.set_namespace_id(namespace_id);
        reduce_cursor_event(&mut state, &mode, event, source)
    };
    let crate::reducer::CursorTransition {
        render_decision,
        notify_delay_disabled,
        command,
    } = transition;

    if notify_delay_disabled
        && let Err(err) = api::command(
            "lua vim.notify(\"Smear cursor disabled in the current buffer due to high delay.\")",
        )
    {
        warn(&format!("delay-disabled notify failed: {err}"));
    }

    match render_decision.render_action {
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

    apply_render_cleanup_action(namespace_id, render_decision.render_cleanup_action);

    let callback_duration_ms = (now_ms() - event_now_ms).max(0.0);
    let should_schedule = match source {
        EventSource::AnimationTick => true,
        EventSource::External => command.is_some(),
    };
    let maybe_delay = {
        let mut state = state_lock();
        if should_schedule
            && state.is_enabled()
            && state.is_animating()
            && !state.is_delay_disabled()
        {
            let step_interval_ms = command
                .map(|value| match value {
                    CursorCommand::StepIntervalMs(step_interval_ms) => step_interval_ms,
                })
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
    let policy = current_buffer_event_policy(&buffer)?;
    if skip_current_buffer_events(&buffer)? {
        let mut state = state_lock();
        state.clear_pending_external_event();
        return Ok(());
    }
    if !policy.should_use_debounced_external_settle() {
        return on_throttled_ui_external_event_trigger(policy);
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

fn on_throttled_ui_external_event_trigger(policy: BufferEventPolicy) -> Result<()> {
    let throttle_interval_ms = policy.settle_delay_floor_ms();
    let elapsed_ms = elapsed_ms_since_last_external_dispatch(now_ms());
    let remaining_delay_ms = remaining_throttle_delay_ms(throttle_interval_ms, elapsed_ms);

    if remaining_delay_ms == 0 {
        on_cursor_event_impl(EventSource::External)?;
        note_external_dispatch_now();
        return Ok(());
    }

    schedule_external_throttle_timer(remaining_delay_ms);
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
    let policy = current_buffer_event_policy(&buffer)?;
    if skip_current_buffer_events(&buffer)? {
        return Ok(());
    }
    if !policy.use_key_fallback() {
        return Ok(());
    }
    if !policy.should_use_debounced_external_settle() {
        schedule_external_event_trigger();
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
        state.set_namespace_id(namespace_id);
        let current_corners = state.current_corners();

        if !state.is_enabled()
            || state.is_animating()
            || state.config.hide_target_hack
            || !state.config.mode_allowed(&mode)
            || !smear_outside_cmd_row(&current_corners)?
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
            current_corners,
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
        let policy = current_buffer_event_policy(&buffer)?;
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
        if policy.should_prepaint_cursor() {
            replace_real_cursor_from_event(false)?;
        }
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
        state.set_namespace_id(namespace_id);

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
        state.set_namespace_id(namespace_id);
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
            state.namespace_id(),
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
    use super::options::{
        parse_optional_change_with, parse_optional_filetypes_disabled, validated_non_negative_f64,
    };
    use super::{
        EngineState, EventLoopState, MIN_RENDER_CLEANUP_DELAY_MS, RenderCleanupGeneration,
        apply_runtime_options, render_cleanup_delay_ms,
    };
    use crate::config::RuntimeConfig;
    use crate::state::{
        ColorOptionsPatch, OptionalChange, RuntimeOptionsPatch, RuntimeState, RuntimeSwitchesPatch,
    };
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
    fn event_loop_state_pending_flag_is_idempotent_until_cleared() {
        let mut state = EventLoopState::new();
        assert!(state.mark_external_trigger_pending_if_idle());
        assert!(!state.mark_external_trigger_pending_if_idle());
        state.clear_external_trigger_pending();
        assert!(state.mark_external_trigger_pending_if_idle());
    }

    #[test]
    fn event_loop_state_elapsed_autocmd_time_handles_unset_and_monotonicity() {
        let mut state = EventLoopState::new();
        assert!(
            state
                .elapsed_ms_since_last_autocmd_event(10.0)
                .is_infinite()
        );

        state.note_autocmd_event(20.0);
        assert_eq!(state.elapsed_ms_since_last_autocmd_event(25.0), 5.0);
        assert_eq!(state.elapsed_ms_since_last_autocmd_event(19.0), 0.0);

        state.clear_autocmd_event_timestamp();
        assert!(
            state
                .elapsed_ms_since_last_autocmd_event(30.0)
                .is_infinite()
        );
    }

    #[test]
    fn runtime_options_patch_apply_clears_nullable_fields() {
        let mut state = RuntimeState::default();
        state.config.delay_disable = Some(12.0);
        state.config.cursor_color = Some("#abcdef".to_string());
        let patch = RuntimeOptionsPatch {
            runtime: RuntimeSwitchesPatch {
                delay_disable: Some(OptionalChange::Clear),
                ..RuntimeSwitchesPatch::default()
            },
            color: ColorOptionsPatch {
                cursor_color: Some(OptionalChange::Clear),
                ..ColorOptionsPatch::default()
            },
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
    fn parse_optional_change_with_nil_maps_to_clear() {
        let parsed = parse_optional_change_with(
            Some(Object::nil()),
            "delay_disable",
            validated_non_negative_f64,
        )
        .expect("expected parse success");
        assert_eq!(parsed, Some(OptionalChange::Clear));
    }

    #[test]
    fn parse_optional_filetypes_disabled_nil_maps_to_empty() {
        let parsed = parse_optional_filetypes_disabled(Some(Object::nil()), "filetypes_disabled")
            .expect("expected parse success");
        assert_eq!(parsed, Some(Vec::new()));
    }

    #[test]
    fn runtime_options_patch_parse_rejects_negative_windows_zindex() {
        let mut opts = Dictionary::new();
        opts.insert("windows_zindex", -1_i64);

        let err = RuntimeOptionsPatch::parse(&opts).expect_err("expected parse failure");
        assert!(
            err.to_string().contains("windows_zindex"),
            "unexpected error: {err}"
        );
        assert!(
            err.to_string().contains("non-negative integer"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn runtime_options_patch_parse_accepts_integral_float_particle_max_num() {
        let mut opts = Dictionary::new();
        opts.insert("particle_max_num", 12.0_f64);

        let patch = RuntimeOptionsPatch::parse(&opts).expect("expected parse success");
        assert_eq!(patch.particles.particle_max_num, Some(12_usize));
    }

    #[test]
    fn runtime_options_patch_parse_cterm_cursor_colors_sets_color_levels() {
        let mut opts = Dictionary::new();
        opts.insert(
            "cterm_cursor_colors",
            Object::from(Array::from_iter([
                Object::from(17_i64),
                Object::from(42_i64),
            ])),
        );

        let patch = RuntimeOptionsPatch::parse(&opts).expect("expected parse success");
        let Some(OptionalChange::Set(colors)) = patch.color.cterm_cursor_colors else {
            panic!("expected cterm cursor color patch to be set");
        };
        assert_eq!(colors.colors, vec![17_u16, 42_u16]);
        assert_eq!(colors.color_levels, 2_u32);
    }

    #[test]
    fn runtime_options_patch_apply_clears_filetypes_list() {
        let mut state = RuntimeState::default();
        state.config.filetypes_disabled = vec!["lua".to_string(), "nix".to_string()];
        let patch = RuntimeOptionsPatch {
            runtime: RuntimeSwitchesPatch {
                filetypes_disabled: Some(Vec::new()),
                ..RuntimeSwitchesPatch::default()
            },
            ..RuntimeOptionsPatch::default()
        };

        patch.apply(&mut state);
        assert!(state.config.filetypes_disabled.is_empty());
    }

    #[test]
    fn buffer_event_policy_classifies_normal_and_acwrite_as_normal() {
        assert_eq!(
            super::BufferEventPolicy::from_buftype(""),
            super::BufferEventPolicy::Normal
        );
        assert_eq!(
            super::BufferEventPolicy::from_buftype("acwrite"),
            super::BufferEventPolicy::Normal
        );
    }

    #[test]
    fn buffer_event_policy_throttles_ui_buffers_including_terminal() {
        assert_eq!(
            super::BufferEventPolicy::from_buftype("nofile"),
            super::BufferEventPolicy::ThrottledUi
        );
        assert_eq!(
            super::BufferEventPolicy::from_buftype("prompt"),
            super::BufferEventPolicy::ThrottledUi
        );
        assert_eq!(
            super::BufferEventPolicy::from_buftype("terminal"),
            super::BufferEventPolicy::ThrottledUi
        );
    }

    #[test]
    fn throttled_ui_policy_enables_key_fallback_and_uses_throttle_floor() {
        let policy = super::BufferEventPolicy::ThrottledUi;
        assert!(policy.use_key_fallback());
        assert_eq!(policy.settle_delay_floor_ms(), 12);
        assert!(!policy.should_use_debounced_external_settle());
        assert!(policy.should_prepaint_cursor());
    }

    #[test]
    fn normal_policy_uses_debounced_external_settle() {
        let policy = super::BufferEventPolicy::Normal;
        assert!(policy.use_key_fallback());
        assert!(policy.should_use_debounced_external_settle());
        assert_eq!(policy.settle_delay_floor_ms(), 0);
    }

    #[test]
    fn remaining_throttle_delay_clamps_at_zero_after_interval() {
        assert_eq!(super::remaining_throttle_delay_ms(12, 4.0), 8);
        assert_eq!(super::remaining_throttle_delay_ms(12, 12.0), 0);
        assert_eq!(super::remaining_throttle_delay_ms(12, f64::INFINITY), 0);
    }

    #[test]
    fn throttle_timer_replaces_settle_timer_kind_only() {
        assert!(super::should_replace_external_timer_with_throttle(Some(
            super::event_loop::ExternalEventTimerKind::Settle
        )));
        assert!(!super::should_replace_external_timer_with_throttle(Some(
            super::event_loop::ExternalEventTimerKind::Throttle
        )));
        assert!(!super::should_replace_external_timer_with_throttle(None));
    }
}
