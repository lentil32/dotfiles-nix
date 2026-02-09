use crate::animation::{
    center, compute_stiffnesses, corners_for_cursor, corners_for_render, initial_velocity,
    reached_target, simulate_step, zero_velocity_corners,
};
use crate::config::RuntimeConfig;
use crate::draw::{
    GradientInfo, RenderFrame, clear_all_namespaces, clear_highlight_cache, draw_current,
    draw_target_hack_block,
};
use crate::lua::{
    bool_from_object, f64_from_object, i64_from_object, invalid_key, parse_indexed_objects,
    string_from_object,
};
use crate::types::{BASE_TIME_INTERVAL, DEFAULT_RNG_STATE, EPSILON, Particle, Point, StepInput};
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
use std::cell::RefCell;
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

#[derive(Debug)]
struct RuntimeState {
    config: RuntimeConfig,
    enabled: bool,
    initialized: bool,
    animating: bool,
    disabled_in_buffer: bool,
    namespace_id: Option<u32>,
    current_corners: [Point; 4],
    target_corners: [Point; 4],
    target_position: Point,
    velocity_corners: [Point; 4],
    stiffnesses: [f64; 4],
    particles: Vec<Particle>,
    previous_center: Point,
    rng_state: u32,
    last_tick_ms: Option<f64>,
    lag_ms: f64,
    last_window_handle: Option<i64>,
    last_buffer_handle: Option<i64>,
    last_top_row: Option<i64>,
    last_line: Option<i64>,
    cursor_hidden: bool,
    pending_external_event: Option<CursorSnapshot>,
    color_at_cursor: Option<String>,
}

impl Default for RuntimeState {
    fn default() -> Self {
        Self {
            config: RuntimeConfig::default(),
            enabled: true,
            initialized: false,
            animating: false,
            disabled_in_buffer: false,
            namespace_id: None,
            current_corners: [Point::ZERO; 4],
            target_corners: [Point::ZERO; 4],
            target_position: Point::ZERO,
            velocity_corners: [Point::ZERO; 4],
            stiffnesses: [0.6; 4],
            particles: Vec::new(),
            previous_center: Point::ZERO,
            rng_state: DEFAULT_RNG_STATE,
            last_tick_ms: None,
            lag_ms: 0.0,
            last_window_handle: None,
            last_buffer_handle: None,
            last_top_row: None,
            last_line: None,
            cursor_hidden: false,
            pending_external_event: None,
            color_at_cursor: None,
        }
    }
}

#[derive(Debug, Clone)]
struct CursorSnapshot {
    mode: String,
    row: f64,
    col: f64,
}

#[derive(Debug, Clone, Copy)]
struct CursorEventContext {
    row: f64,
    col: f64,
    now_ms: f64,
    seed: u32,
    current_win_handle: i64,
    current_buf_handle: i64,
    current_top_row: i64,
    current_line: i64,
    scroll_shift: Option<ScrollShift>,
}

#[derive(Debug, Clone, Copy)]
struct ScrollShift {
    shift: f64,
    min_row: f64,
    max_row: f64,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum EventSource {
    External,
    AnimationTick,
}

#[derive(Debug)]
enum RenderAction {
    Draw(RenderFrame),
    ClearAll,
    Noop,
}

#[derive(Debug)]
struct TransitionEffects {
    render_action: RenderAction,
    step_interval_ms: Option<f64>,
    notify_delay_disabled: bool,
}

impl TransitionEffects {
    fn clear_all() -> Self {
        Self {
            render_action: RenderAction::ClearAll,
            step_interval_ms: None,
            notify_delay_disabled: false,
        }
    }

    fn draw(frame: RenderFrame, step_interval_ms: Option<f64>) -> Self {
        Self {
            render_action: RenderAction::Draw(frame),
            step_interval_ms,
            notify_delay_disabled: false,
        }
    }

    fn noop() -> Self {
        Self {
            render_action: RenderAction::Noop,
            step_interval_ms: None,
            notify_delay_disabled: false,
        }
    }

    fn noop_with_step(step_interval_ms: f64) -> Self {
        Self {
            render_action: RenderAction::Noop,
            step_interval_ms: Some(step_interval_ms),
            notify_delay_disabled: false,
        }
    }

    fn with_delay_notification(mut self, enabled: bool) -> Self {
        self.notify_delay_disabled = self.notify_delay_disabled || enabled;
        self
    }
}

static RUNTIME_STATE: LazyLock<Mutex<RuntimeState>> =
    LazyLock::new(|| Mutex::new(RuntimeState::default()));
static LOG_LEVEL: AtomicI64 = AtomicI64::new(LOG_LEVEL_INFO);
thread_local! {
    static ANIMATION_TIMER: RefCell<Option<TimerHandle>> = const { RefCell::new(None) };
    static EXTERNAL_EVENT_TIMER: RefCell<Option<TimerHandle>> = const { RefCell::new(None) };
    static KEY_EVENT_TIMER: RefCell<Option<TimerHandle>> = const { RefCell::new(None) };
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
    let _ = api::call_function::<_, Object>("luaeval", args);
}

fn warn(message: &str) {
    notify_log(LOG_LEVEL_WARN, message);
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
    let _ = api::set_option_value("guicursor", guicursor, &opts);
}

fn hide_real_cursor() {
    let opts = nvim_oxi::api::opts::SetHighlightOpts::builder()
        .foreground("white")
        .blend(100)
        .build();
    let _ = api::set_hl(0, "SmearCursorHideable", &opts);
}

fn unhide_real_cursor() {
    let opts = nvim_oxi::api::opts::SetHighlightOpts::builder()
        .foreground("none")
        .blend(0)
        .build();
    let _ = api::set_hl(0, "SmearCursorHideable", &opts);
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

fn state_lock() -> std::sync::MutexGuard<'static, RuntimeState> {
    loop {
        match RUNTIME_STATE.lock() {
            Ok(guard) => return guard,
            Err(poisoned) => {
                let mut guard = poisoned.into_inner();
                let namespace_id = guard.namespace_id;
                *guard = RuntimeState::default();
                drop(guard);

                RUNTIME_STATE.clear_poison();
                set_log_level(RuntimeConfig::default().logging_level);
                warn("state mutex poisoned; resetting runtime state");
                if let Some(namespace_id) = namespace_id {
                    clear_all_namespaces(namespace_id, RuntimeConfig::default().max_kept_windows);
                }
                clear_animation_timer();
                clear_external_event_timer();
                clear_key_event_timer();
            }
        }
    }
}

fn is_enabled() -> bool {
    let state = state_lock();
    state.enabled
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
        let winrow = i64_from_object("getwininfo.winrow", winrow_obj)?;
        let wincol = i64_from_object("getwininfo.wincol", wincol_obj)?;

        row = row.saturating_add(winrow.saturating_sub(1));
        col = col.saturating_add(wincol.saturating_sub(1));
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
    let filetype = current_buffer_filetype(buffer)?;
    let state = state_lock();
    Ok(state.disabled_in_buffer
        || state
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
    previous_window_handle: Option<i64>,
    previous_buffer_handle: Option<i64>,
    current_window_handle: i64,
    current_buffer_handle: i64,
    previous_top_row: Option<i64>,
    previous_line: Option<i64>,
    current_top_row: i64,
    current_line: i64,
) -> Result<Option<ScrollShift>> {
    if !scroll_buffer_space {
        return Ok(None);
    }
    if previous_window_handle != Some(current_window_handle)
        || previous_buffer_handle != Some(current_buffer_handle)
    {
        return Ok(None);
    }
    if !smear_outside_cmd_row(current_corners)? {
        return Ok(None);
    }

    let (Some(previous_top_row), Some(previous_line)) = (previous_top_row, previous_line) else {
        return Ok(None);
    };
    if previous_top_row == current_top_row || previous_line == current_line {
        return Ok(None);
    }

    let shift = screen_distance(window, previous_top_row, current_top_row)?;
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

fn as_delay_ms(value: f64) -> u64 {
    let clamped = if value.is_finite() {
        value.max(0.0).floor()
    } else {
        0.0
    };
    if clamped > u64::MAX as f64 {
        u64::MAX
    } else {
        clamped as u64
    }
}

fn next_animation_delay_ms(
    state: &mut RuntimeState,
    step_interval_ms: f64,
    callback_duration_ms: f64,
) -> u64 {
    state.lag_ms = (state.lag_ms + step_interval_ms - state.config.time_interval).max(0.0);

    let mut delay_ms = (state.config.time_interval - callback_duration_ms).max(0.0);
    if state.lag_ms <= delay_ms {
        delay_ms -= state.lag_ms;
        state.lag_ms = 0.0;
    } else {
        state.lag_ms -= delay_ms;
        delay_ms = 0.0;
    }

    as_delay_ms(delay_ms)
}

fn external_settle_delay_ms(delay_event_to_smear: f64) -> u64 {
    as_delay_ms(delay_event_to_smear)
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
            state.pending_external_event.clone(),
        )
    };

    if mode == "c" && !smear_to_cmd {
        let mut state = state_lock();
        state.pending_external_event = None;
        return Ok(());
    }

    let Some(expected_snapshot) = expected_snapshot else {
        return Ok(());
    };

    let current_snapshot = current_cursor_snapshot(smear_to_cmd)?;
    let Some(current_snapshot) = current_snapshot else {
        {
            let mut state = state_lock();
            state.pending_external_event = None;
        }
        return Ok(());
    };

    if snapshots_match(&current_snapshot, &expected_snapshot) {
        {
            let mut state = state_lock();
            state.pending_external_event = None;
        }
        return on_cursor_event_impl(EventSource::External);
    }

    {
        let mut state = state_lock();
        state.pending_external_event = Some(current_snapshot);
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
    let _ = api::clear_autocmds(&opts);
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

fn build_step_input(
    state: &RuntimeState,
    mode: &str,
    time_interval: f64,
    vertical_bar: bool,
    horizontal_bar: bool,
    particles: Vec<Particle>,
) -> StepInput {
    StepInput {
        mode: mode.to_string(),
        time_interval,
        config_time_interval: state.config.time_interval,
        current_corners: state.current_corners,
        target_corners: state.target_corners,
        velocity_corners: state.velocity_corners,
        stiffnesses: state.stiffnesses,
        max_length: state.config.max_length,
        max_length_insert_mode: state.config.max_length_insert_mode,
        damping: state.config.damping,
        damping_insert_mode: state.config.damping_insert_mode,
        delay_disable: state.config.delay_disable,
        particles,
        previous_center: state.previous_center,
        particle_damping: state.config.particle_damping,
        particles_enabled: state.config.particles_enabled,
        particle_gravity: state.config.particle_gravity,
        particle_random_velocity: state.config.particle_random_velocity,
        particle_max_num: state.config.particle_max_num,
        particle_spread: state.config.particle_spread,
        particles_per_second: state.config.particles_per_second,
        particles_per_length: state.config.particles_per_length,
        particle_max_initial_velocity: state.config.particle_max_initial_velocity,
        particle_velocity_from_cursor: state.config.particle_velocity_from_cursor,
        particle_max_lifetime: state.config.particle_max_lifetime,
        particle_lifetime_distribution_exponent: state
            .config
            .particle_lifetime_distribution_exponent,
        min_distance_emit_particles: state.config.min_distance_emit_particles,
        vertical_bar,
        horizontal_bar,
        block_aspect_ratio: state.config.block_aspect_ratio,
        rng_state: state.rng_state,
    }
}

fn apply_runtime_options(state: &mut RuntimeState, opts: &Dictionary) -> Result<()> {
    if let Some(value) = opts.get(&NvimString::from("enabled")).cloned() {
        state.enabled = bool_from_object("enabled", value)?;
    }
    if let Some(value) = opts.get(&NvimString::from("time_interval")).cloned() {
        state.config.time_interval = validated_f64("time_interval", value)?.max(1.0);
    }
    if let Some(value) = opts.get(&NvimString::from("delay_disable")).cloned() {
        if value.is_nil() {
            state.config.delay_disable = None;
        } else {
            state.config.delay_disable = Some(validated_non_negative_f64("delay_disable", value)?);
        }
    }
    if let Some(value) = opts.get(&NvimString::from("delay_event_to_smear")).cloned() {
        state.config.delay_event_to_smear =
            validated_non_negative_f64("delay_event_to_smear", value)?;
    }
    if let Some(value) = opts.get(&NvimString::from("delay_after_key")).cloned() {
        state.config.delay_after_key = validated_non_negative_f64("delay_after_key", value)?;
    }
    if let Some(value) = opts.get(&NvimString::from("smear_to_cmd")).cloned() {
        state.config.smear_to_cmd = bool_from_object("smear_to_cmd", value)?;
    }
    if let Some(value) = opts.get(&NvimString::from("smear_insert_mode")).cloned() {
        state.config.smear_insert_mode = bool_from_object("smear_insert_mode", value)?;
    }
    if let Some(value) = opts.get(&NvimString::from("smear_replace_mode")).cloned() {
        state.config.smear_replace_mode = bool_from_object("smear_replace_mode", value)?;
    }
    if let Some(value) = opts.get(&NvimString::from("smear_terminal_mode")).cloned() {
        state.config.smear_terminal_mode = bool_from_object("smear_terminal_mode", value)?;
    }
    if let Some(value) = opts.get(&NvimString::from("vertical_bar_cursor")).cloned() {
        state.config.vertical_bar_cursor = bool_from_object("vertical_bar_cursor", value)?;
    }
    if let Some(value) = opts
        .get(&NvimString::from("vertical_bar_cursor_insert_mode"))
        .cloned()
    {
        state.config.vertical_bar_cursor_insert_mode =
            bool_from_object("vertical_bar_cursor_insert_mode", value)?;
    }
    if let Some(value) = opts
        .get(&NvimString::from("horizontal_bar_cursor_replace_mode"))
        .cloned()
    {
        state.config.horizontal_bar_cursor_replace_mode =
            bool_from_object("horizontal_bar_cursor_replace_mode", value)?;
    }
    if let Some(value) = opts.get(&NvimString::from("hide_target_hack")).cloned() {
        state.config.hide_target_hack = bool_from_object("hide_target_hack", value)?;
    }
    if let Some(value) = opts.get(&NvimString::from("max_kept_windows")).cloned() {
        let parsed = i64_from_object("max_kept_windows", value)?;
        if parsed < 0 {
            return Err(invalid_key("max_kept_windows", "non-negative integer"));
        }
        state.config.max_kept_windows = usize::try_from(parsed)
            .map_err(|_| invalid_key("max_kept_windows", "non-negative integer"))?;
    }
    if let Some(value) = opts.get(&NvimString::from("windows_zindex")).cloned() {
        let parsed = i64_from_object("windows_zindex", value)?;
        if parsed < 0 {
            return Err(invalid_key("windows_zindex", "non-negative integer"));
        }
        state.config.windows_zindex = u32::try_from(parsed)
            .map_err(|_| invalid_key("windows_zindex", "non-negative integer"))?;
    }
    if let Some(value) = opts.get(&NvimString::from("filetypes_disabled")).cloned() {
        if value.is_nil() {
            state.config.filetypes_disabled.clear();
        } else {
            let values = parse_indexed_objects("filetypes_disabled", value, None)
                .map_err(|_| invalid_key("filetypes_disabled", "array[string]"))?;
            let mut filetypes = Vec::with_capacity(values.len());
            for (index, entry) in values.into_iter().enumerate() {
                let key = format!("filetypes_disabled[{}]", index + 1);
                filetypes.push(string_from_object(&key, entry)?);
            }
            state.config.filetypes_disabled = filetypes;
        }
    }
    if let Some(value) = opts.get(&NvimString::from("logging_level")).cloned() {
        let parsed = i64_from_object("logging_level", value)?;
        if parsed < 0 {
            return Err(invalid_key("logging_level", "non-negative integer"));
        }
        state.config.logging_level = parsed;
        set_log_level(parsed);
    }
    if let Some(value) = opts.get(&NvimString::from("cursor_color")).cloned() {
        if value.is_nil() {
            state.config.cursor_color = None;
        } else {
            state.config.cursor_color = Some(string_from_object("cursor_color", value)?);
        }
    }
    if let Some(value) = opts
        .get(&NvimString::from("cursor_color_insert_mode"))
        .cloned()
    {
        if value.is_nil() {
            state.config.cursor_color_insert_mode = None;
        } else {
            state.config.cursor_color_insert_mode =
                Some(string_from_object("cursor_color_insert_mode", value)?);
        }
    }
    if let Some(value) = opts.get(&NvimString::from("normal_bg")).cloned() {
        if value.is_nil() {
            state.config.normal_bg = None;
        } else {
            state.config.normal_bg = Some(string_from_object("normal_bg", value)?);
        }
    }
    if let Some(value) = opts
        .get(&NvimString::from("transparent_bg_fallback_color"))
        .cloned()
    {
        state.config.transparent_bg_fallback_color =
            string_from_object("transparent_bg_fallback_color", value)?;
    }
    if let Some(value) = opts.get(&NvimString::from("cterm_bg")).cloned() {
        if value.is_nil() {
            state.config.cterm_bg = None;
        } else {
            state.config.cterm_bg = Some(validated_cterm_color_index("cterm_bg", value)?);
        }
    }
    if let Some(value) = opts.get(&NvimString::from("cterm_cursor_colors")).cloned() {
        if value.is_nil() {
            state.config.cterm_cursor_colors = None;
        } else {
            let values = parse_indexed_objects("cterm_cursor_colors", value, None)
                .map_err(|_| invalid_key("cterm_cursor_colors", "array[integer]"))?;
            let mut colors = Vec::with_capacity(values.len());
            for (index, entry) in values.into_iter().enumerate() {
                let key = format!("cterm_cursor_colors[{}]", index + 1);
                colors.push(validated_cterm_color_index(&key, entry)?);
            }
            state.config.color_levels = u32::try_from(colors.len())
                .map_err(|_| invalid_key("cterm_cursor_colors", "array length too large"))?;
            state.config.cterm_cursor_colors = Some(colors);
        }
    }
    if let Some(value) = opts
        .get(&NvimString::from("smear_between_buffers"))
        .cloned()
    {
        state.config.smear_between_buffers = bool_from_object("smear_between_buffers", value)?;
    }
    if let Some(value) = opts
        .get(&NvimString::from("smear_between_neighbor_lines"))
        .cloned()
    {
        state.config.smear_between_neighbor_lines =
            bool_from_object("smear_between_neighbor_lines", value)?;
    }
    if let Some(value) = opts
        .get(&NvimString::from("min_horizontal_distance_smear"))
        .cloned()
    {
        state.config.min_horizontal_distance_smear =
            validated_non_negative_f64("min_horizontal_distance_smear", value)?;
    }
    if let Some(value) = opts
        .get(&NvimString::from("min_vertical_distance_smear"))
        .cloned()
    {
        state.config.min_vertical_distance_smear =
            validated_non_negative_f64("min_vertical_distance_smear", value)?;
    }
    if let Some(value) = opts.get(&NvimString::from("smear_horizontally")).cloned() {
        state.config.smear_horizontally = bool_from_object("smear_horizontally", value)?;
    }
    if let Some(value) = opts.get(&NvimString::from("smear_vertically")).cloned() {
        state.config.smear_vertically = bool_from_object("smear_vertically", value)?;
    }
    if let Some(value) = opts.get(&NvimString::from("smear_diagonally")).cloned() {
        state.config.smear_diagonally = bool_from_object("smear_diagonally", value)?;
    }
    if let Some(value) = opts.get(&NvimString::from("scroll_buffer_space")).cloned() {
        state.config.scroll_buffer_space = bool_from_object("scroll_buffer_space", value)?;
    }
    if let Some(value) = opts.get(&NvimString::from("stiffness")).cloned() {
        state.config.stiffness = validated_non_negative_f64("stiffness", value)?;
    }
    if let Some(value) = opts.get(&NvimString::from("trailing_stiffness")).cloned() {
        state.config.trailing_stiffness = validated_non_negative_f64("trailing_stiffness", value)?;
    }
    if let Some(value) = opts.get(&NvimString::from("trailing_exponent")).cloned() {
        state.config.trailing_exponent = validated_non_negative_f64("trailing_exponent", value)?;
    }
    if let Some(value) = opts
        .get(&NvimString::from("stiffness_insert_mode"))
        .cloned()
    {
        state.config.stiffness_insert_mode =
            validated_non_negative_f64("stiffness_insert_mode", value)?;
    }
    if let Some(value) = opts
        .get(&NvimString::from("trailing_stiffness_insert_mode"))
        .cloned()
    {
        state.config.trailing_stiffness_insert_mode =
            validated_non_negative_f64("trailing_stiffness_insert_mode", value)?;
    }
    if let Some(value) = opts
        .get(&NvimString::from("trailing_exponent_insert_mode"))
        .cloned()
    {
        state.config.trailing_exponent_insert_mode =
            validated_non_negative_f64("trailing_exponent_insert_mode", value)?;
    }
    if let Some(value) = opts.get(&NvimString::from("anticipation")).cloned() {
        state.config.anticipation = validated_non_negative_f64("anticipation", value)?;
    }
    if let Some(value) = opts.get(&NvimString::from("damping")).cloned() {
        state.config.damping = validated_non_negative_f64("damping", value)?;
    }
    if let Some(value) = opts.get(&NvimString::from("damping_insert_mode")).cloned() {
        state.config.damping_insert_mode =
            validated_non_negative_f64("damping_insert_mode", value)?;
    }
    if let Some(value) = opts
        .get(&NvimString::from("distance_stop_animating"))
        .cloned()
    {
        state.config.distance_stop_animating =
            validated_non_negative_f64("distance_stop_animating", value)?;
    }
    if let Some(value) = opts
        .get(&NvimString::from("distance_stop_animating_vertical_bar"))
        .cloned()
    {
        state.config.distance_stop_animating_vertical_bar =
            validated_non_negative_f64("distance_stop_animating_vertical_bar", value)?;
    }
    if let Some(value) = opts.get(&NvimString::from("max_length")).cloned() {
        state.config.max_length = validated_non_negative_f64("max_length", value)?;
    }
    if let Some(value) = opts
        .get(&NvimString::from("max_length_insert_mode"))
        .cloned()
    {
        state.config.max_length_insert_mode =
            validated_non_negative_f64("max_length_insert_mode", value)?;
    }
    if let Some(value) = opts.get(&NvimString::from("particles_enabled")).cloned() {
        state.config.particles_enabled = bool_from_object("particles_enabled", value)?;
    }
    if let Some(value) = opts.get(&NvimString::from("particle_max_num")).cloned() {
        let parsed = i64_from_object("particle_max_num", value)?;
        if parsed < 0 {
            return Err(invalid_key("particle_max_num", "non-negative integer"));
        }
        state.config.particle_max_num = usize::try_from(parsed)
            .map_err(|_| invalid_key("particle_max_num", "non-negative integer"))?;
    }
    if let Some(value) = opts.get(&NvimString::from("particle_spread")).cloned() {
        state.config.particle_spread = validated_non_negative_f64("particle_spread", value)?;
    }
    if let Some(value) = opts.get(&NvimString::from("particles_per_second")).cloned() {
        state.config.particles_per_second =
            validated_non_negative_f64("particles_per_second", value)?;
    }
    if let Some(value) = opts.get(&NvimString::from("particles_per_length")).cloned() {
        state.config.particles_per_length =
            validated_non_negative_f64("particles_per_length", value)?;
    }
    if let Some(value) = opts
        .get(&NvimString::from("particle_max_lifetime"))
        .cloned()
    {
        state.config.particle_max_lifetime =
            validated_non_negative_f64("particle_max_lifetime", value)?;
    }
    if let Some(value) = opts
        .get(&NvimString::from("particle_lifetime_distribution_exponent"))
        .cloned()
    {
        state.config.particle_lifetime_distribution_exponent =
            validated_non_negative_f64("particle_lifetime_distribution_exponent", value)?;
    }
    if let Some(value) = opts
        .get(&NvimString::from("particle_max_initial_velocity"))
        .cloned()
    {
        state.config.particle_max_initial_velocity =
            validated_non_negative_f64("particle_max_initial_velocity", value)?;
    }
    if let Some(value) = opts
        .get(&NvimString::from("particle_velocity_from_cursor"))
        .cloned()
    {
        state.config.particle_velocity_from_cursor =
            validated_non_negative_f64("particle_velocity_from_cursor", value)?;
    }
    if let Some(value) = opts
        .get(&NvimString::from("particle_random_velocity"))
        .cloned()
    {
        state.config.particle_random_velocity =
            validated_non_negative_f64("particle_random_velocity", value)?;
    }
    if let Some(value) = opts.get(&NvimString::from("particle_damping")).cloned() {
        state.config.particle_damping = validated_non_negative_f64("particle_damping", value)?;
    }
    if let Some(value) = opts.get(&NvimString::from("particle_gravity")).cloned() {
        state.config.particle_gravity = validated_non_negative_f64("particle_gravity", value)?;
    }
    if let Some(value) = opts
        .get(&NvimString::from("min_distance_emit_particles"))
        .cloned()
    {
        state.config.min_distance_emit_particles =
            validated_non_negative_f64("min_distance_emit_particles", value)?;
    }
    if let Some(value) = opts
        .get(&NvimString::from("particle_switch_octant_braille"))
        .cloned()
    {
        state.config.particle_switch_octant_braille =
            validated_non_negative_f64("particle_switch_octant_braille", value)?;
    }
    if let Some(value) = opts.get(&NvimString::from("particles_over_text")).cloned() {
        state.config.particles_over_text = bool_from_object("particles_over_text", value)?;
    }
    if let Some(value) = opts
        .get(&NvimString::from("volume_reduction_exponent"))
        .cloned()
    {
        state.config.volume_reduction_exponent =
            validated_non_negative_f64("volume_reduction_exponent", value)?;
    }
    if let Some(value) = opts
        .get(&NvimString::from("minimum_volume_factor"))
        .cloned()
    {
        state.config.minimum_volume_factor =
            validated_non_negative_f64("minimum_volume_factor", value)?;
    }
    if let Some(value) = opts
        .get(&NvimString::from("never_draw_over_target"))
        .cloned()
    {
        state.config.never_draw_over_target = bool_from_object("never_draw_over_target", value)?;
    }
    if let Some(value) = opts
        .get(&NvimString::from("legacy_computing_symbols_support"))
        .cloned()
    {
        state.config.legacy_computing_symbols_support =
            bool_from_object("legacy_computing_symbols_support", value)?;
    }
    if let Some(value) = opts
        .get(&NvimString::from(
            "legacy_computing_symbols_support_vertical_bars",
        ))
        .cloned()
    {
        state.config.legacy_computing_symbols_support_vertical_bars =
            bool_from_object("legacy_computing_symbols_support_vertical_bars", value)?;
    }
    if let Some(value) = opts.get(&NvimString::from("use_diagonal_blocks")).cloned() {
        state.config.use_diagonal_blocks = bool_from_object("use_diagonal_blocks", value)?;
    }
    if let Some(value) = opts.get(&NvimString::from("max_slope_horizontal")).cloned() {
        state.config.max_slope_horizontal =
            validated_non_negative_f64("max_slope_horizontal", value)?;
    }
    if let Some(value) = opts.get(&NvimString::from("min_slope_vertical")).cloned() {
        state.config.min_slope_vertical = validated_non_negative_f64("min_slope_vertical", value)?;
    }
    if let Some(value) = opts
        .get(&NvimString::from("max_angle_difference_diagonal"))
        .cloned()
    {
        state.config.max_angle_difference_diagonal =
            validated_non_negative_f64("max_angle_difference_diagonal", value)?;
    }
    if let Some(value) = opts.get(&NvimString::from("max_offset_diagonal")).cloned() {
        state.config.max_offset_diagonal =
            validated_non_negative_f64("max_offset_diagonal", value)?;
    }
    if let Some(value) = opts
        .get(&NvimString::from("min_shade_no_diagonal"))
        .cloned()
    {
        state.config.min_shade_no_diagonal =
            validated_non_negative_f64("min_shade_no_diagonal", value)?;
    }
    if let Some(value) = opts
        .get(&NvimString::from("min_shade_no_diagonal_vertical_bar"))
        .cloned()
    {
        state.config.min_shade_no_diagonal_vertical_bar =
            validated_non_negative_f64("min_shade_no_diagonal_vertical_bar", value)?;
    }
    if let Some(value) = opts.get(&NvimString::from("max_shade_no_matrix")).cloned() {
        state.config.max_shade_no_matrix =
            validated_non_negative_f64("max_shade_no_matrix", value)?;
    }
    if let Some(value) = opts.get(&NvimString::from("color_levels")).cloned() {
        let parsed = i64_from_object("color_levels", value)?;
        if parsed < 1 {
            return Err(invalid_key("color_levels", "positive integer"));
        }
        state.config.color_levels =
            u32::try_from(parsed).map_err(|_| invalid_key("color_levels", "positive integer"))?;
    }
    if let Some(value) = opts.get(&NvimString::from("gamma")).cloned() {
        state.config.gamma = validated_positive_f64("gamma", value)?;
    }
    if let Some(value) = opts.get(&NvimString::from("gradient_exponent")).cloned() {
        state.config.gradient_exponent = validated_non_negative_f64("gradient_exponent", value)?;
    }
    if let Some(value) = opts
        .get(&NvimString::from("matrix_pixel_threshold"))
        .cloned()
    {
        state.config.matrix_pixel_threshold =
            validated_non_negative_f64("matrix_pixel_threshold", value)?;
    }
    if let Some(value) = opts
        .get(&NvimString::from("matrix_pixel_threshold_vertical_bar"))
        .cloned()
    {
        state.config.matrix_pixel_threshold_vertical_bar =
            validated_non_negative_f64("matrix_pixel_threshold_vertical_bar", value)?;
    }
    if let Some(value) = opts
        .get(&NvimString::from("matrix_pixel_min_factor"))
        .cloned()
    {
        state.config.matrix_pixel_min_factor =
            validated_non_negative_f64("matrix_pixel_min_factor", value)?;
    }
    if !should_track_cursor_color(&state.config) {
        state.color_at_cursor = None;
    }
    Ok(())
}

fn build_render_frame(
    state: &RuntimeState,
    mode: &str,
    render_corners: [Point; 4],
    target: Point,
    vertical_bar: bool,
    gradient_indexes: Option<(usize, usize)>,
) -> RenderFrame {
    let gradient = gradient_indexes.map(|(index_head, index_tail)| {
        let origin = render_corners[index_head];
        let direction = Point {
            row: render_corners[index_tail].row - origin.row,
            col: render_corners[index_tail].col - origin.col,
        };
        let length_squared = direction.row * direction.row + direction.col * direction.col;
        let direction_scaled = if length_squared > 1.0 {
            Point {
                row: direction.row / length_squared,
                col: direction.col / length_squared,
            }
        } else {
            Point::ZERO
        };

        GradientInfo {
            origin,
            direction_scaled,
        }
    });

    RenderFrame {
        mode: mode.to_string(),
        corners: render_corners,
        target,
        target_corners: state.target_corners,
        vertical_bar,
        particles: state.particles.clone(),
        cursor_color: state.config.cursor_color.clone(),
        cursor_color_insert_mode: state.config.cursor_color_insert_mode.clone(),
        normal_bg: state.config.normal_bg.clone(),
        transparent_bg_fallback_color: state.config.transparent_bg_fallback_color.clone(),
        cterm_cursor_colors: state.config.cterm_cursor_colors.clone(),
        cterm_bg: state.config.cterm_bg,
        color_at_cursor: state.color_at_cursor.clone(),
        hide_target_hack: state.config.hide_target_hack,
        never_draw_over_target: state.config.never_draw_over_target,
        legacy_computing_symbols_support: state.config.legacy_computing_symbols_support,
        legacy_computing_symbols_support_vertical_bars: state
            .config
            .legacy_computing_symbols_support_vertical_bars,
        use_diagonal_blocks: state.config.use_diagonal_blocks,
        max_slope_horizontal: state.config.max_slope_horizontal,
        min_slope_vertical: state.config.min_slope_vertical,
        max_angle_difference_diagonal: state.config.max_angle_difference_diagonal,
        max_offset_diagonal: state.config.max_offset_diagonal,
        min_shade_no_diagonal: state.config.min_shade_no_diagonal,
        min_shade_no_diagonal_vertical_bar: state.config.min_shade_no_diagonal_vertical_bar,
        max_shade_no_matrix: state.config.max_shade_no_matrix,
        particle_max_lifetime: state.config.particle_max_lifetime,
        particle_switch_octant_braille: state.config.particle_switch_octant_braille,
        particles_over_text: state.config.particles_over_text,
        color_levels: state.config.color_levels,
        gamma: state.config.gamma,
        gradient_exponent: state.config.gradient_exponent,
        matrix_pixel_threshold: state.config.matrix_pixel_threshold,
        matrix_pixel_threshold_vertical_bar: state.config.matrix_pixel_threshold_vertical_bar,
        matrix_pixel_min_factor: state.config.matrix_pixel_min_factor,
        max_kept_windows: state.config.max_kept_windows,
        windows_zindex: state.config.windows_zindex,
        gradient,
    }
}

fn gradient_indexes_for_corners(
    current_corners: &[Point; 4],
    target_corners: &[Point; 4],
) -> Option<(usize, usize)> {
    let mut distance_head_to_target_squared = f64::INFINITY;
    let mut distance_tail_to_target_squared = 0.0_f64;
    let mut index_head = 0_usize;
    let mut index_tail = 0_usize;

    for index in 0..4 {
        let distance_squared = current_corners[index].distance_squared(target_corners[index]);
        if distance_squared < distance_head_to_target_squared {
            distance_head_to_target_squared = distance_squared;
            index_head = index;
        }
        if distance_squared > distance_tail_to_target_squared {
            distance_tail_to_target_squared = distance_squared;
            index_tail = index;
        }
    }

    if distance_tail_to_target_squared <= EPSILON {
        None
    } else {
        Some((index_head, index_tail))
    }
}

fn reset_animation_timing(state: &mut RuntimeState) {
    state.last_tick_ms = None;
    state.lag_ms = 0.0;
}

fn clamp_row_to_window(row: f64, scroll_shift: ScrollShift) -> f64 {
    row.max(scroll_shift.min_row).min(scroll_shift.max_row)
}

fn apply_scroll_shift_to_state(
    state: &mut RuntimeState,
    vertical_bar: bool,
    horizontal_bar: bool,
    scroll_shift: ScrollShift,
) {
    let shifted_row = clamp_row_to_window(
        state.current_corners[0].row - scroll_shift.shift,
        scroll_shift,
    );
    let shifted_col = state.current_corners[0].col;
    state.current_corners =
        corners_for_cursor(shifted_row, shifted_col, vertical_bar, horizontal_bar);
    state.previous_center = center(&state.current_corners);

    for particle in &mut state.particles {
        particle.position.row -= scroll_shift.shift;
    }
}

fn should_jump_to_target(
    state: &RuntimeState,
    handles_changed: bool,
    target_row: f64,
    target_col: f64,
) -> bool {
    if handles_changed {
        return !state.config.smear_between_buffers;
    }

    let current_row = state.current_corners[0].row;
    let current_col = state.current_corners[0].col;
    let delta_row = (target_row - current_row).abs();
    let delta_col = (target_col - current_col).abs();

    (!state.config.smear_between_neighbor_lines && delta_row <= 1.5)
        || (delta_row < state.config.min_vertical_distance_smear
            && delta_col < state.config.min_horizontal_distance_smear)
        || (!state.config.smear_horizontally && delta_row <= 0.5)
        || (!state.config.smear_vertically && delta_col <= 0.5)
        || (!state.config.smear_diagonally && delta_row > 0.5 && delta_col > 0.5)
}

fn external_mode_ignores_cursor(config: &RuntimeConfig, mode: &str) -> bool {
    mode == "c" && !config.smear_to_cmd
}

fn external_mode_requires_jump(config: &RuntimeConfig, mode: &str) -> bool {
    (mode == "i" && !config.smear_insert_mode)
        || (mode == "R" && !config.smear_replace_mode)
        || (mode == "t" && !config.smear_terminal_mode)
}

fn transition_cursor_event(
    state: &mut RuntimeState,
    mode: &str,
    event: CursorEventContext,
    source: EventSource,
) -> TransitionEffects {
    if !state.enabled {
        state.animating = false;
        reset_animation_timing(state);
        return TransitionEffects::clear_all();
    }

    if state.disabled_in_buffer {
        if source == EventSource::External {
            return TransitionEffects::noop();
        }
        if !state.animating {
            reset_animation_timing(state);
            return TransitionEffects::clear_all();
        }
    }

    let vertical_bar = state.config.cursor_is_vertical_bar(mode);
    let horizontal_bar = state.config.cursor_is_horizontal_bar(mode);
    let mut target_position = if source == EventSource::AnimationTick {
        state.target_position
    } else {
        Point {
            row: event.row,
            col: event.col,
        }
    };
    let handles_changed = state.last_window_handle != Some(event.current_win_handle)
        || state.last_buffer_handle != Some(event.current_buf_handle);

    if source == EventSource::External && external_mode_ignores_cursor(&state.config, mode) {
        return TransitionEffects::noop();
    }

    if source == EventSource::External && external_mode_requires_jump(&state.config, mode) {
        let corners = corners_for_cursor(
            target_position.row,
            target_position.col,
            vertical_bar,
            horizontal_bar,
        );
        state.current_corners = corners;
        state.target_corners = corners;
        state.target_position = target_position;
        state.previous_center = center(&state.current_corners);
        // Match upstream Lua behavior: jump updates position/target but does not
        // force-stop an in-flight animation loop or clear velocity state.
        state.initialized = true;
        state.last_window_handle = Some(event.current_win_handle);
        state.last_buffer_handle = Some(event.current_buf_handle);
        state.last_top_row = Some(event.current_top_row);
        state.last_line = Some(event.current_line);
        return TransitionEffects::clear_all();
    }

    if !state.initialized {
        let corners = corners_for_cursor(
            target_position.row,
            target_position.col,
            vertical_bar,
            horizontal_bar,
        );
        state.current_corners = corners;
        state.target_corners = corners;
        state.target_position = target_position;
        state.velocity_corners = zero_velocity_corners();
        state.previous_center = center(&state.current_corners);
        state.particles.clear();
        state.rng_state = event.seed;
        state.disabled_in_buffer = false;
        state.initialized = true;
        state.animating = false;
        reset_animation_timing(state);
        state.last_window_handle = Some(event.current_win_handle);
        state.last_buffer_handle = Some(event.current_buf_handle);
        state.last_top_row = Some(event.current_top_row);
        state.last_line = Some(event.current_line);
        state.cursor_hidden = false;
        let frame = build_render_frame(
            state,
            mode,
            state.current_corners,
            target_position,
            vertical_bar,
            None,
        );
        return TransitionEffects::draw(frame, None);
    }

    if source == EventSource::AnimationTick && !state.animating {
        return TransitionEffects::noop();
    }

    if source == EventSource::External {
        if let Some(scroll_shift) = event.scroll_shift {
            apply_scroll_shift_to_state(state, vertical_bar, horizontal_bar, scroll_shift);
            target_position.row =
                clamp_row_to_window(target_position.row - scroll_shift.shift, scroll_shift);
        }

        if should_jump_to_target(
            state,
            handles_changed,
            target_position.row,
            target_position.col,
        ) {
            let corners = corners_for_cursor(
                target_position.row,
                target_position.col,
                vertical_bar,
                horizontal_bar,
            );
            state.current_corners = corners;
            state.target_corners = corners;
            state.target_position = target_position;
            state.velocity_corners = zero_velocity_corners();
            state.previous_center = center(&state.current_corners);
            state.animating = false;
            reset_animation_timing(state);
            state.last_window_handle = Some(event.current_win_handle);
            state.last_buffer_handle = Some(event.current_buf_handle);
            state.last_top_row = Some(event.current_top_row);
            state.last_line = Some(event.current_line);
            return TransitionEffects::clear_all();
        }
        state.target_corners = corners_for_cursor(
            target_position.row,
            target_position.col,
            vertical_bar,
            horizontal_bar,
        );
        state.target_position = target_position;
        state.stiffnesses = compute_stiffnesses(
            &state.config,
            mode,
            &state.current_corners,
            &state.target_corners,
        );
    }

    let was_animating = state.animating;
    if source == EventSource::External && !was_animating {
        state.velocity_corners = initial_velocity(
            &state.current_corners,
            &state.target_corners,
            state.config.anticipation,
        );
        state.animating = true;
    }
    let just_started = source == EventSource::External && !was_animating && state.animating;

    if source == EventSource::External {
        state.last_window_handle = Some(event.current_win_handle);
        state.last_buffer_handle = Some(event.current_buf_handle);
        state.last_top_row = Some(event.current_top_row);
        state.last_line = Some(event.current_line);
    }

    let should_advance = match source {
        EventSource::AnimationTick => state.animating,
        // Match upstream behavior: target updates do not advance physics while already animating.
        // The first transition after a jump starts animation immediately.
        EventSource::External => just_started,
    };

    if should_advance {
        let step_interval = if just_started {
            state.last_tick_ms = Some(event.now_ms);
            BASE_TIME_INTERVAL
        } else {
            let interval = match state.last_tick_ms {
                Some(previous) => (event.now_ms - previous).max(0.0),
                None => BASE_TIME_INTERVAL,
            };
            state.last_tick_ms = Some(event.now_ms);
            interval
        };

        let particles = std::mem::take(&mut state.particles);
        let step_input = build_step_input(
            state,
            mode,
            step_interval,
            vertical_bar,
            horizontal_bar,
            particles,
        );
        let step_output = simulate_step(step_input);

        state.current_corners = step_output.current_corners;
        state.velocity_corners = step_output.velocity_corners;
        state.previous_center = step_output.previous_center;
        state.rng_state = step_output.rng_state;
        state.particles = step_output.particles;
        let mut notify_delay_disabled = false;
        if step_output.disabled_due_to_delay && !state.disabled_in_buffer {
            state.disabled_in_buffer = true;
            notify_delay_disabled = true;
        }

        if reached_target(
            &state.config,
            mode,
            &state.current_corners,
            &state.target_corners,
            &state.velocity_corners,
            &state.particles,
        ) {
            state.current_corners = state.target_corners;
            state.velocity_corners = zero_velocity_corners();
            state.animating = false;
            reset_animation_timing(state);
            return TransitionEffects::clear_all().with_delay_notification(notify_delay_disabled);
        }

        if state.lag_ms > EPSILON {
            return TransitionEffects::noop_with_step(step_interval)
                .with_delay_notification(notify_delay_disabled);
        }

        let render_corners =
            corners_for_render(&state.config, &state.current_corners, &state.target_corners);
        let frame = build_render_frame(
            state,
            mode,
            render_corners,
            target_position,
            vertical_bar,
            Some((step_output.index_head, step_output.index_tail)),
        );
        return TransitionEffects::draw(frame, Some(step_interval))
            .with_delay_notification(notify_delay_disabled);
    }

    if source == EventSource::External {
        return TransitionEffects::noop();
    }

    let gradient_indexes =
        gradient_indexes_for_corners(&state.current_corners, &state.target_corners);
    let render_corners =
        corners_for_render(&state.config, &state.current_corners, &state.target_corners);
    let frame = build_render_frame(
        state,
        mode,
        render_corners,
        target_position,
        vertical_bar,
        gradient_indexes,
    );
    TransitionEffects::draw(frame, None)
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

    let fallback_target_position = if source == EventSource::AnimationTick {
        let state = state_lock();
        if state.initialized {
            Some((state.target_position.row, state.target_position.col))
        } else {
            None
        }
    } else {
        None
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
    let (
        scroll_buffer_space,
        previous_window_handle,
        previous_buffer_handle,
        previous_top_row,
        previous_line,
        current_corners,
    ) = {
        let state = state_lock();
        (
            state.config.scroll_buffer_space,
            state.last_window_handle,
            state.last_buffer_handle,
            state.last_top_row,
            state.last_line,
            state.current_corners,
        )
    };
    let scroll_shift = if source == EventSource::External {
        maybe_scroll_shift(
            &window,
            scroll_buffer_space,
            &current_corners,
            previous_window_handle,
            previous_buffer_handle,
            current_win_handle,
            current_buf_handle,
            previous_top_row,
            previous_line,
            current_top_row,
            current_line,
        )?
    } else {
        None
    };

    let event_now_ms = now_ms();
    let event = CursorEventContext {
        row: cursor_row,
        col: cursor_col,
        now_ms: event_now_ms,
        seed: seed_from_clock(),
        current_win_handle,
        current_buf_handle,
        current_top_row,
        current_line,
        scroll_shift,
    };

    let effects = {
        let mut state = state_lock();
        state.namespace_id = Some(namespace_id);
        transition_cursor_event(&mut state, &mode, event, source)
    };

    if effects.notify_delay_disabled {
        let _ = api::command(
            "lua vim.notify(\"Smear cursor disabled in the current buffer due to high delay.\")",
        );
    }

    match effects.render_action {
        RenderAction::Draw(frame) => {
            let redraw_cmd = mode == "c" && smear_outside_cmd_row(&frame.corners)?;
            draw_current(namespace_id, &frame)?;
            if redraw_cmd {
                let _ = api::command("redraw");
            }

            let should_unhide_while_animating =
                mode == "c" || (frame.never_draw_over_target && frame_reaches_target_cell(&frame));
            if mode != "c" && !should_unhide_while_animating && frame.hide_target_hack {
                draw_target_hack_block(namespace_id, &frame)?;
            }
            let mut state = state_lock();
            if should_unhide_while_animating {
                if state.cursor_hidden {
                    if !state.config.hide_target_hack {
                        unhide_real_cursor();
                    }
                    state.cursor_hidden = false;
                }
            } else if !state.cursor_hidden && mode != "c" {
                if !state.config.hide_target_hack {
                    hide_real_cursor();
                }
                state.cursor_hidden = true;
            }
        }
        RenderAction::ClearAll => {
            let max_kept_windows = {
                let state = state_lock();
                state.config.max_kept_windows
            };
            clear_all_namespaces(namespace_id, max_kept_windows);
            if mode == "c" {
                let _ = api::command("redraw");
            }
            let mut state = state_lock();
            if state.cursor_hidden {
                if !state.config.hide_target_hack {
                    unhide_real_cursor();
                }
                state.cursor_hidden = false;
            }
        }
        RenderAction::Noop => {}
    }

    let callback_duration_ms = (now_ms() - event_now_ms).max(0.0);
    let should_schedule =
        source == EventSource::AnimationTick || effects.step_interval_ms.is_some();
    let maybe_delay = {
        let mut state = state_lock();
        if should_schedule && state.enabled && state.animating && !state.disabled_in_buffer {
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
    if let Some(delay) = maybe_delay {
        schedule_animation_tick(delay);
    } else if source == EventSource::AnimationTick {
        clear_animation_timer();
    }

    Ok(())
}

fn on_external_event_trigger() -> Result<()> {
    if !is_enabled() {
        let mut state = state_lock();
        state.pending_external_event = None;
        return Ok(());
    }

    let buffer = api::get_current_buf();
    if !buffer.is_valid() {
        return Ok(());
    }
    if skip_current_buffer_events(&buffer)? {
        let mut state = state_lock();
        state.pending_external_event = None;
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
        state.pending_external_event = None;
        return Ok(());
    }

    let snapshot = current_cursor_snapshot(smear_to_cmd)?;
    let Some(snapshot) = snapshot else {
        {
            let mut state = state_lock();
            state.pending_external_event = None;
        }
        return Ok(());
    };

    {
        let mut state = state_lock();
        state.pending_external_event = Some(snapshot);
    }
    schedule_external_event_timer(delay_ms);
    Ok(())
}

fn schedule_external_event_trigger() {
    schedule(|_| {
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
    schedule_external_event_trigger();
    Ok(())
}

fn replace_real_cursor_from_event(only_hide_real_cursor: bool) -> Result<()> {
    let mode = mode_string();
    let namespace_id = ensure_namespace_id();

    let maybe_frame = {
        let mut state = state_lock();
        state.namespace_id = Some(namespace_id);

        if !state.enabled
            || state.animating
            || state.config.hide_target_hack
            || !state.config.mode_allowed(&mode)
            || !smear_outside_cmd_row(&state.current_corners)?
        {
            return Ok(());
        }

        if !state.cursor_hidden {
            hide_real_cursor();
            state.cursor_hidden = true;
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
    state.disabled_in_buffer = false;
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

    let current_win_handle = i64::from(window.handle());
    let current_buf_handle = i64::from(buffer.handle());
    let current_top_row = line_value("w0")?;
    let current_line = line_value(".")?;

    let (max_kept_windows, hide_target_hack, unhide_cursor) = {
        let mut state = state_lock();
        state.namespace_id = Some(namespace_id);

        let vertical_bar = state.config.cursor_is_vertical_bar(&mode);
        let horizontal_bar = state.config.cursor_is_horizontal_bar(&mode);
        let corners = corners_for_cursor(row, col, vertical_bar, horizontal_bar);

        state.current_corners = corners;
        state.target_corners = corners;
        state.target_position = Point { row, col };
        state.velocity_corners = zero_velocity_corners();
        state.previous_center = center(&state.current_corners);
        state.particles.clear();
        state.animating = false;
        state.initialized = true;
        state.pending_external_event = None;
        state.last_window_handle = Some(current_win_handle);
        state.last_buffer_handle = Some(current_buf_handle);
        state.last_top_row = Some(current_top_row);
        state.last_line = Some(current_line);
        reset_animation_timing(&mut state);

        let unhide_cursor = state.cursor_hidden;
        state.cursor_hidden = false;

        (
            state.config.max_kept_windows,
            state.config.hide_target_hack,
            unhide_cursor,
        )
    };

    clear_animation_timer();
    clear_external_event_timer();
    clear_key_event_timer();
    clear_all_namespaces(namespace_id, max_kept_windows);
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
    let _ = api::del_user_command("SmearCursorToggle");
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
            state.enabled = true;
        }
        apply_runtime_options(&mut state, &opts)?;
        set_log_level(state.config.logging_level);
        state.initialized = false;
        state.animating = false;
        state.target_position = Point::ZERO;
        state.last_tick_ms = None;
        state.lag_ms = 0.0;
        state.disabled_in_buffer = false;
        state.last_window_handle = None;
        state.last_buffer_handle = None;
        state.last_top_row = None;
        state.last_line = None;
        state.cursor_hidden = false;
        state.pending_external_event = None;
        state.color_at_cursor = None;
        state.enabled
    };
    clear_animation_timer();
    clear_external_event_timer();
    clear_key_event_timer();

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
    let (is_enabled, namespace_id, max_kept_windows, hide_target_hack) = {
        let mut state = state_lock();
        state.enabled = !state.enabled;
        if !state.enabled {
            state.animating = false;
            state.initialized = false;
            state.target_position = Point::ZERO;
            state.last_tick_ms = None;
            state.lag_ms = 0.0;
            state.last_window_handle = None;
            state.last_buffer_handle = None;
            state.last_top_row = None;
            state.last_line = None;
            state.pending_external_event = None;
            state.color_at_cursor = None;
        }
        (
            state.enabled,
            state.namespace_id,
            state.config.max_kept_windows,
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
            clear_animation_timer();
            clear_external_event_timer();
            clear_key_event_timer();
            clear_all_namespaces(namespace_id, max_kept_windows);
            if !hide_target_hack {
                unhide_real_cursor();
            }
        }
    }

    Ok(())
}
