use crate::lua::i64_from_object;
use crate::octant_chars::OCTANT_CHARACTERS;
use crate::types::{Particle, Point};
use nvim_oxi::Result;
use nvim_oxi::api;
use nvim_oxi::api::opts::{GetHighlightOpts, OptionOpts, OptionScope, SetExtmarkOpts};
use nvim_oxi::api::types::{
    ExtmarkVirtTextPosition, GetHlInfos, WindowConfig, WindowRelativeTo, WindowStyle,
};
use nvim_oxi::{Array, Dictionary, Object};
use nvim_oxi_utils::handles;
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

const EXTMARK_ID: u32 = 999;
const DEFAULT_CURSOR_COLOR: u32 = 0x00D0_D0D0;
const DEFAULT_BACKGROUND_COLOR: u32 = 0x0030_3030;
const BRAILLE_CODE_MIN: i64 = 0x2800;
const BRAILLE_CODE_MAX: i64 = 0x28FF;
const OCTANT_CODE_MIN: i64 = 0x1CD00;
const OCTANT_CODE_MAX: i64 = 0x1CDE7;
const PARTICLE_ZINDEX_OFFSET: u32 = 1;
const BLOCK_ASPECT_RATIO: f64 = 2.0;
const ADAPTIVE_POOL_MIN_BUDGET: usize = 16;
const ADAPTIVE_POOL_HARD_MAX_BUDGET: usize = 256;
const ADAPTIVE_POOL_BUDGET_MARGIN: usize = 8;
const ADAPTIVE_POOL_EWMA_SCALE: u64 = 1000;
const ADAPTIVE_POOL_EWMA_PREV_WEIGHT: u64 = 7;
const ADAPTIVE_POOL_EWMA_NEW_WEIGHT: u64 = 3;

const BOTTOM_BLOCKS: [&str; 9] = ["█", "▇", "▆", "▅", "▄", "▃", "▂", "▁", " "];
const LEFT_BLOCKS: [&str; 9] = [" ", "▏", "▎", "▍", "▌", "▋", "▊", "▉", "█"];
const TOP_BLOCKS: [&str; 9] = [" ", "▔", "🮂", "🮃", "▀", "🮄", "🮅", "🮆", "█"];
const RIGHT_BLOCKS: [&str; 9] = ["█", "🮋", "🮊", "🮉", "▐", "🮈", "🮇", "▕", " "];
const VERTICAL_BARS: [&str; 8] = ["▏", "🭰", "🭱", "🭲", "🭳", "🭳", "🭵", "▕"];
const MATRIX_CHARACTERS: [&str; 16] = [
    "", "▘", "▝", "▀", "▖", "▌", "▞", "▛", "▗", "▚", "▐", "▜", "▄", "▙", "▟", "█",
];

#[derive(Clone, Copy, Debug)]
pub(crate) struct GradientInfo {
    pub(crate) origin: Point,
    pub(crate) direction_scaled: Point,
}

#[derive(Clone, Debug)]
pub(crate) struct RenderFrame {
    pub(crate) mode: String,
    pub(crate) corners: [Point; 4],
    pub(crate) target: Point,
    pub(crate) target_corners: [Point; 4],
    pub(crate) vertical_bar: bool,
    pub(crate) particles: Vec<Particle>,
    pub(crate) cursor_color: Option<String>,
    pub(crate) cursor_color_insert_mode: Option<String>,
    pub(crate) normal_bg: Option<String>,
    pub(crate) transparent_bg_fallback_color: String,
    pub(crate) cterm_cursor_colors: Option<Vec<u16>>,
    pub(crate) cterm_bg: Option<u16>,
    pub(crate) color_at_cursor: Option<String>,
    pub(crate) hide_target_hack: bool,
    pub(crate) never_draw_over_target: bool,
    pub(crate) legacy_computing_symbols_support: bool,
    pub(crate) legacy_computing_symbols_support_vertical_bars: bool,
    pub(crate) use_diagonal_blocks: bool,
    pub(crate) max_slope_horizontal: f64,
    pub(crate) min_slope_vertical: f64,
    pub(crate) max_angle_difference_diagonal: f64,
    pub(crate) max_offset_diagonal: f64,
    pub(crate) min_shade_no_diagonal: f64,
    pub(crate) min_shade_no_diagonal_vertical_bar: f64,
    pub(crate) max_shade_no_matrix: f64,
    pub(crate) particle_max_lifetime: f64,
    pub(crate) particle_switch_octant_braille: f64,
    pub(crate) particles_over_text: bool,
    pub(crate) color_levels: u32,
    pub(crate) gamma: f64,
    pub(crate) gradient_exponent: f64,
    pub(crate) matrix_pixel_threshold: f64,
    pub(crate) matrix_pixel_threshold_vertical_bar: f64,
    pub(crate) matrix_pixel_min_factor: f64,
    pub(crate) windows_zindex: u32,
    pub(crate) gradient: Option<GradientInfo>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct HighlightPaletteKey {
    cursor_color: u32,
    normal_background: Option<u32>,
    transparent_fallback: u32,
    non_inverted_blend: u8,
    color_levels: u32,
    gamma_bits: u64,
    cterm_cursor_colors: Option<Vec<u16>>,
    cterm_bg: Option<u16>,
}

#[derive(Clone, Copy, Debug)]
struct WindowBufferHandle {
    window_id: i32,
    buffer_id: i32,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct FrameEpoch(u64);

impl FrameEpoch {
    const ZERO: Self = Self(0);

    fn next(self) -> Self {
        Self(self.0.wrapping_add(1))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CachedWindowLifecycle {
    Available { last_used_epoch: FrameEpoch },
    InUse { epoch: FrameEpoch },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EpochRollover {
    AvailableUnchanged,
    ReleasedForReuse,
    RecoveredStaleInUse,
}

#[derive(Clone, Copy, Debug)]
struct CachedRenderWindow {
    handles: WindowBufferHandle,
    lifecycle: CachedWindowLifecycle,
}

impl CachedRenderWindow {
    fn new_in_use(handles: WindowBufferHandle, epoch: FrameEpoch) -> Self {
        Self {
            handles,
            lifecycle: CachedWindowLifecycle::InUse { epoch },
        }
    }

    fn is_available_for_reuse(self) -> bool {
        matches!(self.lifecycle, CachedWindowLifecycle::Available { .. })
    }

    fn mark_in_use(&mut self, epoch: FrameEpoch) -> bool {
        match self.lifecycle {
            CachedWindowLifecycle::Available { .. } => {
                self.lifecycle = CachedWindowLifecycle::InUse { epoch };
                true
            }
            CachedWindowLifecycle::InUse { .. } => false,
        }
    }

    fn rollover_to_next_epoch(&mut self, previous_epoch: FrameEpoch) -> EpochRollover {
        match self.lifecycle {
            CachedWindowLifecycle::Available { .. } => EpochRollover::AvailableUnchanged,
            CachedWindowLifecycle::InUse { epoch } if epoch == previous_epoch => {
                self.lifecycle = CachedWindowLifecycle::Available {
                    last_used_epoch: epoch,
                };
                EpochRollover::ReleasedForReuse
            }
            CachedWindowLifecycle::InUse { epoch } => {
                self.lifecycle = CachedWindowLifecycle::Available {
                    last_used_epoch: epoch,
                };
                EpochRollover::RecoveredStaleInUse
            }
        }
    }

    fn available_epoch(self) -> Option<FrameEpoch> {
        match self.lifecycle {
            CachedWindowLifecycle::Available { last_used_epoch } => Some(last_used_epoch),
            CachedWindowLifecycle::InUse { .. } => None,
        }
    }
}

#[derive(Debug)]
struct TabWindows {
    current_epoch: FrameEpoch,
    frame_demand: usize,
    reuse_scan_index: usize,
    windows: Vec<CachedRenderWindow>,
    ewma_demand_milli: u64,
    cached_budget: usize,
}

impl Default for TabWindows {
    fn default() -> Self {
        Self {
            current_epoch: FrameEpoch::ZERO,
            frame_demand: 0,
            reuse_scan_index: 0,
            windows: Vec::new(),
            ewma_demand_milli: 0,
            cached_budget: ADAPTIVE_POOL_MIN_BUDGET,
        }
    }
}

#[derive(Debug, Default)]
struct DrawState {
    tabs: HashMap<i32, TabWindows>,
    bulge_above: bool,
}

static DRAW_STATE: LazyLock<Mutex<DrawState>> = LazyLock::new(|| Mutex::new(DrawState::default()));
static HIGHLIGHT_PALETTE_CACHE: LazyLock<Mutex<Option<HighlightPaletteKey>>> =
    LazyLock::new(|| Mutex::new(None));

fn log_draw_error(context: &str, err: &impl std::fmt::Display) {
    api::err_writeln(&format!("[smear_cursor][draw] {context} failed: {err}"));
}

fn clear_render_namespace(buffer: &mut api::Buffer, namespace_id: u32, context: &str) {
    if let Err(err) = buffer.clear_namespace(namespace_id, 0..) {
        log_draw_error(context, &err);
    }
}

struct EventIgnoreGuard {
    previous: Option<String>,
}

impl EventIgnoreGuard {
    fn set_all() -> Self {
        let opts = OptionOpts::builder().build();
        let previous = api::get_option_value::<String>("eventignore", &opts).ok();
        if let Err(err) = api::set_option_value("eventignore", "all", &opts) {
            log_draw_error("set eventignore=all", &err);
        }
        Self { previous }
    }
}

impl Drop for EventIgnoreGuard {
    fn drop(&mut self) {
        let Some(previous) = self.previous.take() else {
            return;
        };
        let opts = OptionOpts::builder().build();
        if let Err(err) = api::set_option_value("eventignore", previous, &opts) {
            log_draw_error("restore eventignore", &err);
        }
    }
}

fn draw_state_lock() -> std::sync::MutexGuard<'static, DrawState> {
    loop {
        match DRAW_STATE.lock() {
            Ok(guard) => return guard,
            Err(poisoned) => {
                let mut guard = poisoned.into_inner();
                *guard = DrawState::default();
                drop(guard);
                DRAW_STATE.clear_poison();
            }
        }
    }
}

fn palette_cache_lock() -> std::sync::MutexGuard<'static, Option<HighlightPaletteKey>> {
    loop {
        match HIGHLIGHT_PALETTE_CACHE.lock() {
            Ok(guard) => return guard,
            Err(poisoned) => {
                let mut guard = poisoned.into_inner();
                *guard = None;
                drop(guard);
                HIGHLIGHT_PALETTE_CACHE.clear_poison();
            }
        }
    }
}

pub(crate) fn clear_highlight_cache() {
    let mut cache = palette_cache_lock();
    *cache = None;
}

fn hl_group_name(level: u32) -> String {
    format!("SmearCursor{level}")
}

fn inverted_hl_group_name(level: u32) -> String {
    format!("SmearCursorInverted{level}")
}

fn rgb_to_hex(rgb: u32) -> String {
    format!("#{:06X}", rgb & 0x00FF_FFFF)
}

fn interpolate_channel(a: u8, b: u8, t: f64) -> u8 {
    let value = f64::from(a) + t * (f64::from(b) - f64::from(a));
    value.round().clamp(0.0, 255.0) as u8
}

fn interpolate_color(color_a: u32, color_b: u32, t: f64) -> u32 {
    let t_clamped = t.clamp(0.0, 1.0);
    let a_r = ((color_a >> 16) & 0xFF) as u8;
    let a_g = ((color_a >> 8) & 0xFF) as u8;
    let a_b = (color_a & 0xFF) as u8;

    let b_r = ((color_b >> 16) & 0xFF) as u8;
    let b_g = ((color_b >> 8) & 0xFF) as u8;
    let b_b = (color_b & 0xFF) as u8;

    let r = interpolate_channel(a_r, b_r, t_clamped);
    let g = interpolate_channel(a_g, b_g, t_clamped);
    let b = interpolate_channel(a_b, b_b, t_clamped);

    (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b)
}

fn highlight_color(group: &str, foreground: bool) -> Option<u32> {
    let opts = GetHighlightOpts::builder()
        .name(group)
        .link(false)
        .create(false)
        .build();
    let infos = api::get_hl(0, &opts).ok()?;
    let GetHlInfos::Single(infos) = infos else {
        return None;
    };

    if foreground {
        infos.foreground
    } else {
        infos.background
    }
}

fn parse_hex_color(color: &str) -> Option<u32> {
    let stripped = color.strip_prefix('#')?;
    if stripped.len() != 6 || !stripped.chars().all(|chr| chr.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(stripped, 16).ok()
}

#[derive(Clone, Copy, Debug)]
enum ResolvedCursorColor {
    Direct(u32),
    FromCursorText,
}

fn resolve_cursor_color_setting(setting: Option<&str>) -> Option<ResolvedCursorColor> {
    let setting = setting?;
    if setting == "none" {
        return Some(ResolvedCursorColor::FromCursorText);
    }
    if let Some(hex_color) = parse_hex_color(setting) {
        return Some(ResolvedCursorColor::Direct(hex_color));
    }
    highlight_color(setting, false).map(ResolvedCursorColor::Direct)
}

fn resolve_mode_cursor_color(frame: &RenderFrame) -> u32 {
    let setting = if frame.mode == "i" {
        frame.cursor_color_insert_mode.as_deref()
    } else {
        frame.cursor_color.as_deref()
    };

    let explicit_color =
        resolve_cursor_color_setting(setting).and_then(|resolved| match resolved {
            ResolvedCursorColor::Direct(color) => Some(color),
            ResolvedCursorColor::FromCursorText => {
                frame.color_at_cursor.as_deref().and_then(parse_hex_color)
            }
        });

    explicit_color
        .or_else(|| highlight_color("Cursor", false))
        .or_else(|| highlight_color("Normal", true))
        .unwrap_or(DEFAULT_CURSOR_COLOR)
}

fn resolve_normal_background(frame: &RenderFrame) -> Option<u32> {
    match frame.normal_bg.as_deref() {
        Some("none") => None,
        Some(value) => parse_hex_color(value).or_else(|| highlight_color(value, false)),
        None => highlight_color("Normal", false),
    }
}

fn resolve_transparent_fallback(frame: &RenderFrame) -> u32 {
    parse_hex_color(frame.transparent_bg_fallback_color.as_str())
        .unwrap_or(DEFAULT_BACKGROUND_COLOR)
}

fn cterm_color_at_level(cterm_cursor_colors: Option<&[u16]>, level: u32) -> Option<u16> {
    let colors = cterm_cursor_colors?;
    let index = usize::try_from(level.saturating_sub(1)).ok()?;
    colors.get(index).copied()
}

fn set_highlight_group(
    group: &str,
    foreground: &str,
    background: &str,
    blend: u8,
    cterm_fg: Option<u16>,
    cterm_bg: Option<u16>,
) -> Result<()> {
    let mut highlight = Dictionary::new();
    highlight.insert("fg", foreground);
    highlight.insert("bg", background);
    highlight.insert("blend", i64::from(blend));
    if let Some(value) = cterm_fg {
        highlight.insert("ctermfg", i64::from(value));
    }
    if let Some(value) = cterm_bg {
        highlight.insert("ctermbg", i64::from(value));
    }

    let args = Array::from_iter([
        Object::from(0_i64),
        Object::from(group),
        Object::from(highlight),
    ]);
    let _: Object = api::call_function("nvim_set_hl", args)?;
    Ok(())
}

fn ensure_highlight_palette(frame: &RenderFrame) -> Result<()> {
    let color_levels = frame.color_levels.max(1);
    let gamma = frame.gamma;
    let cursor_color = resolve_mode_cursor_color(frame);
    let normal_background = resolve_normal_background(frame);
    let transparent_fallback = resolve_transparent_fallback(frame);
    let interpolation_background = normal_background.unwrap_or(transparent_fallback);
    let non_inverted_blend =
        if frame.legacy_computing_symbols_support && normal_background.is_some() {
            100
        } else {
            0
        };
    let cterm_cursor_colors = frame.cterm_cursor_colors.clone();
    let palette_key = HighlightPaletteKey {
        cursor_color,
        normal_background,
        transparent_fallback,
        non_inverted_blend,
        color_levels,
        gamma_bits: gamma.to_bits(),
        cterm_cursor_colors: cterm_cursor_colors.clone(),
        cterm_bg: frame.cterm_bg,
    };

    {
        let cache = palette_cache_lock();
        if cache.as_ref().is_some_and(|cached| cached == &palette_key) {
            return Ok(());
        }
    }

    for level in 1..=color_levels {
        let opacity = (f64::from(level) / f64::from(color_levels)).powf(1.0 / gamma);
        let blended = interpolate_color(interpolation_background, cursor_color, opacity);
        let blended_hex = rgb_to_hex(blended);
        let inverted_foreground = rgb_to_hex(normal_background.unwrap_or(transparent_fallback));
        let cterm_level_color = cterm_color_at_level(cterm_cursor_colors.as_deref(), level);

        set_highlight_group(
            hl_group_name(level).as_str(),
            blended_hex.as_str(),
            "none",
            non_inverted_blend,
            cterm_level_color,
            None,
        )?;

        let inverted_ctermfg = frame.cterm_bg.or_else(|| {
            cterm_cursor_colors
                .as_ref()
                .and_then(|colors| colors.first().copied())
        });
        set_highlight_group(
            inverted_hl_group_name(level).as_str(),
            inverted_foreground.as_str(),
            blended_hex.as_str(),
            0,
            inverted_ctermfg,
            cterm_level_color,
        )?;
    }

    let mut cache = palette_cache_lock();
    *cache = Some(palette_key);
    Ok(())
}

fn window_from_handle_i32(handle: i32) -> Option<api::Window> {
    handles::valid_window(i64::from(handle))
}

fn buffer_from_handle_i32(handle: i32) -> Option<api::Buffer> {
    handles::valid_buffer(i64::from(handle))
}

fn buffer_has_render_marker(buffer: &api::Buffer) -> bool {
    let opts = OptionOpts::builder().buf(buffer.clone()).build();
    let Ok(filetype) = api::get_option_value::<String>("filetype", &opts) else {
        return false;
    };
    if filetype != "smear-cursor" {
        return false;
    }

    let Ok(buftype) = api::get_option_value::<String>("buftype", &opts) else {
        return false;
    };
    buftype == "nofile"
}

fn window_buffer(window: &api::Window) -> Option<api::Buffer> {
    if !window.is_valid() {
        return None;
    }
    let args = Array::from_iter([Object::from(window.handle())]);
    let buffer_handle: i32 = match api::call_function("nvim_win_get_buf", args) {
        Ok(handle) => handle,
        Err(err) => {
            log_draw_error("nvim_win_get_buf", &err);
            return None;
        }
    };
    buffer_from_handle_i32(buffer_handle)
}

fn close_orphan_render_windows(namespace_id: u32) {
    let _event_ignore = EventIgnoreGuard::set_all();
    for window in api::list_wins() {
        let Some(mut buffer) = window_buffer(&window) else {
            continue;
        };
        if !buffer_has_render_marker(&buffer) {
            continue;
        }

        clear_render_namespace(&mut buffer, namespace_id, "clear orphan render namespace");
        if let Err(err) = window.close(true) {
            log_draw_error("close orphan render window", &err);
        }
    }
}

pub(crate) fn clear_buffer_namespace(buffer: &mut api::Buffer, namespace_id: u32) {
    clear_render_namespace(buffer, namespace_id, "clear render namespace");
}

fn float_window_config(row: i64, col: i64, zindex: u32) -> WindowConfig {
    let mut builder = WindowConfig::builder();
    builder
        .relative(WindowRelativeTo::Editor)
        .row(row as f64 - 1.0)
        .col(col as f64 - 1.0)
        .width(1)
        .height(1)
        .focusable(false)
        .style(WindowStyle::Minimal)
        .noautocmd(true)
        .hide(false)
        .zindex(zindex);
    builder.build()
}

fn hide_window_config() -> WindowConfig {
    let mut builder = WindowConfig::builder();
    builder.hide(true);
    builder.build()
}

fn initialize_buffer_options(buffer: &api::Buffer) -> Result<()> {
    let opts = OptionOpts::builder().buf(buffer.clone()).build();
    api::set_option_value("buftype", "nofile", &opts)?;
    api::set_option_value("filetype", "smear-cursor", &opts)?;
    api::set_option_value("bufhidden", "wipe", &opts)?;
    api::set_option_value("swapfile", false, &opts)?;
    Ok(())
}

fn initialize_window_options(window: &api::Window) -> Result<()> {
    let opts = OptionOpts::builder()
        .scope(OptionScope::Local)
        .win(window.clone())
        .build();
    api::set_option_value("winhighlight", "NormalFloat:Normal", &opts)?;
    api::set_option_value("winblend", 100_i64, &opts)?;
    Ok(())
}

fn close_cached_window(namespace_id: u32, handles: WindowBufferHandle) {
    if let Some(mut buffer) = buffer_from_handle_i32(handles.buffer_id) {
        clear_render_namespace(&mut buffer, namespace_id, "clear cached render namespace");
    }
    if let Some(window) = window_from_handle_i32(handles.window_id)
        && let Err(err) = window.close(true)
    {
        log_draw_error("close cached render window", &err);
    }
}

fn get_or_create_window(
    draw_state: &mut DrawState,
    namespace_id: u32,
    tab_handle: i32,
    row: i64,
    col: i64,
    zindex: u32,
) -> Result<(api::Window, api::Buffer)> {
    let tab_windows = draw_state.tabs.entry(tab_handle).or_default();
    while tab_windows.reuse_scan_index < tab_windows.windows.len() {
        let index = tab_windows.reuse_scan_index;
        tab_windows.reuse_scan_index = tab_windows.reuse_scan_index.saturating_add(1);

        let cached = tab_windows.windows[index];
        if !cached.is_available_for_reuse() {
            continue;
        }

        let maybe_window = window_from_handle_i32(cached.handles.window_id);
        let maybe_buffer = buffer_from_handle_i32(cached.handles.buffer_id);

        if let (Some(mut window), Some(buffer)) = (maybe_window, maybe_buffer) {
            let config = float_window_config(row, col, zindex);
            if window.set_config(&config).is_ok()
                && tab_windows.windows[index].mark_in_use(tab_windows.current_epoch)
            {
                tab_windows.frame_demand = tab_windows.frame_demand.saturating_add(1);
                return Ok((window, buffer));
            }
        }

        close_cached_window(namespace_id, cached.handles);
        tab_windows.windows.remove(index);
        if tab_windows.reuse_scan_index > 0 {
            tab_windows.reuse_scan_index -= 1;
        }
    }

    // Match upstream Lua behavior: avoid triggering unrelated autocmds while
    // creating auxiliary float windows.
    let _event_ignore = EventIgnoreGuard::set_all();
    let buffer = api::create_buf(false, true)?;
    let config = float_window_config(row, col, zindex);
    let window = api::open_win(&buffer, false, &config)?;
    initialize_buffer_options(&buffer)?;
    initialize_window_options(&window)?;

    let handles = WindowBufferHandle {
        window_id: window.handle(),
        buffer_id: buffer.handle(),
    };

    tab_windows.windows.push(CachedRenderWindow::new_in_use(
        handles,
        tab_windows.current_epoch,
    ));
    tab_windows.frame_demand = tab_windows.frame_demand.saturating_add(1);
    tab_windows.reuse_scan_index = tab_windows.windows.len();

    Ok((window, buffer))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct AdaptiveBudgetState {
    ewma_demand_milli: u64,
    cached_budget: usize,
}

fn ceil_div_u64(lhs: u64, rhs: u64) -> u64 {
    if rhs == 0 {
        return 0;
    }
    lhs.div_ceil(rhs)
}

fn next_adaptive_budget(previous: AdaptiveBudgetState, frame_demand: usize) -> AdaptiveBudgetState {
    let demand_milli = u64::try_from(frame_demand)
        .unwrap_or(u64::MAX)
        .saturating_mul(ADAPTIVE_POOL_EWMA_SCALE);
    let weighted_prev = previous
        .ewma_demand_milli
        .saturating_mul(ADAPTIVE_POOL_EWMA_PREV_WEIGHT);
    let weighted_new = demand_milli.saturating_mul(ADAPTIVE_POOL_EWMA_NEW_WEIGHT);
    let denominator = ADAPTIVE_POOL_EWMA_PREV_WEIGHT.saturating_add(ADAPTIVE_POOL_EWMA_NEW_WEIGHT);
    let next_ewma = if previous.ewma_demand_milli == 0 {
        demand_milli
    } else {
        weighted_prev
            .saturating_add(weighted_new)
            .saturating_add(denominator.saturating_sub(1))
            / denominator.max(1)
    };
    let ewma_demand =
        usize::try_from(ceil_div_u64(next_ewma, ADAPTIVE_POOL_EWMA_SCALE)).unwrap_or(usize::MAX);
    let target_budget = ewma_demand
        .saturating_add(ADAPTIVE_POOL_BUDGET_MARGIN)
        .clamp(ADAPTIVE_POOL_MIN_BUDGET, ADAPTIVE_POOL_HARD_MAX_BUDGET);
    let next_budget = if target_budget >= previous.cached_budget {
        target_budget
    } else {
        previous
            .cached_budget
            .saturating_sub(ADAPTIVE_POOL_BUDGET_MARGIN)
            .max(target_budget)
            .max(ADAPTIVE_POOL_MIN_BUDGET)
    };

    AdaptiveBudgetState {
        ewma_demand_milli: next_ewma,
        cached_budget: next_budget,
    }
}

fn lru_prune_indices(windows: &[CachedRenderWindow], keep_count: usize) -> Vec<usize> {
    let mut ordered: Vec<(usize, FrameEpoch)> = windows
        .iter()
        .enumerate()
        .filter_map(|(index, cached)| cached.available_epoch().map(|epoch| (index, epoch)))
        .collect();
    if ordered.len() <= keep_count {
        return Vec::new();
    }

    let remove_count = ordered.len().saturating_sub(keep_count);
    ordered.sort_by(|lhs, rhs| lhs.1.cmp(&rhs.1).then(lhs.0.cmp(&rhs.0)));

    let mut remove_indices: Vec<usize> = ordered
        .into_iter()
        .take(remove_count)
        .map(|(index, _)| index)
        .collect();
    remove_indices.sort_unstable();
    remove_indices
}

fn clear_cached_windows(draw_state: &mut DrawState, namespace_id: u32) {
    let hide_config = hide_window_config();
    for tab_windows in draw_state.tabs.values_mut() {
        let next_budget = next_adaptive_budget(
            AdaptiveBudgetState {
                ewma_demand_milli: tab_windows.ewma_demand_milli,
                cached_budget: tab_windows.cached_budget,
            },
            tab_windows.frame_demand,
        );
        tab_windows.ewma_demand_milli = next_budget.ewma_demand_milli;
        tab_windows.cached_budget = next_budget.cached_budget;
        let previous_epoch = tab_windows.current_epoch;
        tab_windows.current_epoch = tab_windows.current_epoch.next();
        tab_windows.frame_demand = 0;
        tab_windows.reuse_scan_index = 0;

        let mut index = 0;
        while index < tab_windows.windows.len() {
            let handles = tab_windows.windows[index].handles;
            let maybe_window = window_from_handle_i32(handles.window_id);
            let maybe_buffer = buffer_from_handle_i32(handles.buffer_id);

            if maybe_window.is_none() || maybe_buffer.is_none() {
                close_cached_window(namespace_id, handles);
                tab_windows.windows.remove(index);
                continue;
            }

            let rollover = tab_windows.windows[index].rollover_to_next_epoch(previous_epoch);
            if matches!(
                rollover,
                EpochRollover::ReleasedForReuse | EpochRollover::RecoveredStaleInUse
            ) {
                if let Some(mut buffer) = maybe_buffer {
                    clear_render_namespace(
                        &mut buffer,
                        namespace_id,
                        "clear reused render namespace",
                    );
                }

                if let Some(mut window) = maybe_window
                    && window.set_config(&hide_config).is_err()
                {
                    close_cached_window(namespace_id, handles);
                    tab_windows.windows.remove(index);
                    continue;
                }
            }

            index += 1;
        }

        debug_assert!(
            tab_windows
                .windows
                .iter()
                .all(|cached| cached.is_available_for_reuse()),
            "cached render windows must be available after epoch rollover"
        );

        let remove_indices = lru_prune_indices(&tab_windows.windows, tab_windows.cached_budget);
        if !remove_indices.is_empty() {
            // Match upstream Lua behavior: close extra windows with events
            // ignored to avoid incidental side effects.
            let _event_ignore = EventIgnoreGuard::set_all();
            for remove_index in remove_indices.into_iter().rev() {
                let cached = tab_windows.windows.remove(remove_index);
                close_cached_window(namespace_id, cached.handles);
            }
        }
    }
}

fn purge_cached_windows(draw_state: &mut DrawState, namespace_id: u32) {
    // Closing helper windows can trigger unrelated autocommands; suppress them
    // while tearing down all cached render state.
    let _event_ignore = EventIgnoreGuard::set_all();

    for tab_windows in draw_state.tabs.values() {
        for cached in tab_windows.windows.iter().copied() {
            close_cached_window(namespace_id, cached.handles);
        }
    }

    draw_state.tabs.clear();
    draw_state.bulge_above = false;
    close_orphan_render_windows(namespace_id);
}

pub(crate) fn clear_active_render_windows(namespace_id: u32) {
    let mut draw_state = draw_state_lock();
    clear_cached_windows(&mut draw_state, namespace_id);
}

pub(crate) fn purge_render_windows(namespace_id: u32) {
    let mut draw_state = draw_state_lock();
    purge_cached_windows(&mut draw_state, namespace_id);
}

pub(crate) fn clear_all_namespaces(namespace_id: u32) {
    {
        let mut draw_state = draw_state_lock();
        purge_cached_windows(&mut draw_state, namespace_id);
    }

    for mut buffer in api::list_bufs() {
        if buffer.is_valid() {
            clear_buffer_namespace(&mut buffer, namespace_id);
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EdgeType {
    Top,
    Bottom,
    Left,
    Right,
    LeftDiagonal,
    RightDiagonal,
    None,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BlockCharacterSet {
    Bottom,
    Left,
    Top,
    Right,
    VerticalBars,
}

#[derive(Clone, Copy, Debug)]
struct PartialBlockProperties {
    character_index: i64,
    character_set: BlockCharacterSet,
    level: u32,
    inverted: bool,
}

#[derive(Clone, Debug, Default)]
struct EdgeIntersections {
    centerlines: HashMap<i64, f64>,
    edges: HashMap<i64, [f64; 2]>,
    fractions: HashMap<i64, [f64; 2]>,
}

#[derive(Clone, Debug)]
struct QuadGeometry {
    top: i64,
    bottom: i64,
    left: i64,
    right: i64,
    slopes: [f64; 4],
    angles: [f64; 4],
    edge_types: [EdgeType; 4],
    intersections: [EdgeIntersections; 4],
}

#[derive(Clone, Copy, Debug, Default)]
struct CellIntersections {
    top: Option<f64>,
    bottom: Option<f64>,
    left: Option<f64>,
    right: Option<f64>,
    diagonal: Option<f64>,
}

const DIAGONAL_SLOPES: [f64; 10] = [
    -2.0,
    -4.0 / 3.0,
    -1.0,
    -2.0 / 3.0,
    -1.0 / 3.0,
    1.0 / 3.0,
    2.0 / 3.0,
    1.0,
    4.0 / 3.0,
    2.0,
];

const LEFT_DIAGONAL_NEG_2: &[(f64, &str)] = &[(-1.0 / 4.0, "🭛"), (1.0 / 4.0, "🭟")];
const LEFT_DIAGONAL_NEG_4_3: &[(f64, &str)] = &[(-3.0 / 8.0, "🭙"), (3.0 / 8.0, "🭟")];
const LEFT_DIAGONAL_NEG_1: &[(f64, &str)] = &[(0.0, "◤")];
const LEFT_DIAGONAL_NEG_2_3: &[(f64, &str)] = &[
    (-3.0 / 4.0, "🭗"),
    (-1.0 / 4.0, "🭚"),
    (1.0 / 4.0, "🭠"),
    (3.0 / 4.0, "🭝"),
];
const LEFT_DIAGONAL_NEG_1_3: &[(f64, &str)] = &[(-1.0, "🭘"), (0.0, "🭜"), (1.0, "🭞")];
const LEFT_DIAGONAL_POS_1_3: &[(f64, &str)] = &[(-1.0, "🬽"), (0.0, "🭑"), (1.0, "🭍")];
const LEFT_DIAGONAL_POS_2_3: &[(f64, &str)] = &[
    (-3.0 / 4.0, "🬼"),
    (-1.0 / 4.0, "🬿"),
    (1.0 / 4.0, "🭏"),
    (3.0 / 4.0, "🭌"),
];
const LEFT_DIAGONAL_POS_1: &[(f64, &str)] = &[(0.0, "◣")];
const LEFT_DIAGONAL_POS_4_3: &[(f64, &str)] = &[(-3.0 / 8.0, "🬾"), (3.0 / 8.0, "🭎")];
const LEFT_DIAGONAL_POS_2: &[(f64, &str)] = &[(-1.0 / 4.0, "🭀"), (1.0 / 4.0, "🭐")];

const RIGHT_DIAGONAL_NEG_2: &[(f64, &str)] = &[(-1.0 / 4.0, "🭅"), (1.0 / 4.0, "🭋")];
const RIGHT_DIAGONAL_NEG_4_3: &[(f64, &str)] = &[(-3.0 / 8.0, "🭃"), (3.0 / 8.0, "🭉")];
const RIGHT_DIAGONAL_NEG_1: &[(f64, &str)] = &[(0.0, "◢")];
const RIGHT_DIAGONAL_NEG_2_3: &[(f64, &str)] = &[
    (-3.0 / 4.0, "🭁"),
    (-1.0 / 4.0, "🭄"),
    (1.0 / 4.0, "🭊"),
    (3.0 / 4.0, "🭇"),
];
const RIGHT_DIAGONAL_NEG_1_3: &[(f64, &str)] = &[(-1.0, "🭂"), (0.0, "🭆"), (1.0, "🭈")];
const RIGHT_DIAGONAL_POS_1_3: &[(f64, &str)] = &[(-1.0, "🭓"), (0.0, "🭧"), (1.0, "🭣")];
const RIGHT_DIAGONAL_POS_2_3: &[(f64, &str)] = &[
    (-3.0 / 4.0, "🭒"),
    (-1.0 / 4.0, "🭕"),
    (1.0 / 4.0, "🭥"),
    (3.0 / 4.0, "🭢"),
];
const RIGHT_DIAGONAL_POS_1: &[(f64, &str)] = &[(0.0, "◥")];
const RIGHT_DIAGONAL_POS_4_3: &[(f64, &str)] = &[(-3.0 / 8.0, "🭔"), (3.0 / 8.0, "🭤")];
const RIGHT_DIAGONAL_POS_2: &[(f64, &str)] = &[(-1.0 / 4.0, "🭖"), (1.0 / 4.0, "🭦")];

fn frac01(value: f64) -> f64 {
    value.rem_euclid(1.0)
}

fn round_lua(value: f64) -> i64 {
    (value + 0.5).floor() as i64
}

fn level_from_shade(shade: f64, color_levels: u32) -> u32 {
    if !shade.is_finite() || color_levels == 0 {
        return 0;
    }

    let rounded = round_lua(shade * f64::from(color_levels));
    if rounded <= 0 {
        0
    } else {
        let clamped = rounded.min(i64::from(color_levels));
        u32::try_from(clamped).unwrap_or(0)
    }
}

fn block_characters(character_set: BlockCharacterSet) -> &'static [&'static str] {
    match character_set {
        BlockCharacterSet::Bottom => &BOTTOM_BLOCKS,
        BlockCharacterSet::Left => &LEFT_BLOCKS,
        BlockCharacterSet::Top => &TOP_BLOCKS,
        BlockCharacterSet::Right => &RIGHT_BLOCKS,
        BlockCharacterSet::VerticalBars => &VERTICAL_BARS,
    }
}

fn ensure_clockwise(corners: &[Point; 4]) -> [Point; 4] {
    let cross = (corners[1].row - corners[0].row) * (corners[3].col - corners[0].col)
        - (corners[1].col - corners[0].col) * (corners[3].row - corners[0].row);

    if cross > 0.0 {
        [corners[2], corners[1], corners[0], corners[3]]
    } else {
        *corners
    }
}

fn precompute_intersections_horizontal(
    corners: &[Point; 4],
    geometry: &mut QuadGeometry,
    edge_index: usize,
) {
    let slope = geometry.slopes[edge_index];
    let corner = corners[edge_index];
    let intersections = &mut geometry.intersections[edge_index];

    for col in geometry.left..=geometry.right {
        let centerline = corner.row + ((col as f64 + 0.5) - corner.col) * slope;
        intersections.centerlines.insert(col, centerline);
        intersections
            .fractions
            .insert(col, [centerline - 0.25 * slope, centerline + 0.25 * slope]);
    }
}

fn precompute_intersections_vertical(
    corners: &[Point; 4],
    geometry: &mut QuadGeometry,
    edge_index: usize,
) {
    let slope = geometry.slopes[edge_index];
    let corner = corners[edge_index];
    let intersections = &mut geometry.intersections[edge_index];

    for row in geometry.top..=geometry.bottom {
        let centerline = corner.col + ((row as f64 + 0.5) - corner.row) / slope;
        intersections.centerlines.insert(row, centerline);
        intersections
            .fractions
            .insert(row, [centerline - 0.25 / slope, centerline + 0.25 / slope]);
    }
}

fn precompute_intersections_diagonal(
    corners: &[Point; 4],
    geometry: &mut QuadGeometry,
    edge_index: usize,
    frame: &RenderFrame,
) {
    let slope = geometry.slopes[edge_index];
    let edge_type = geometry.edge_types[edge_index];
    let corner = corners[edge_index];
    let intersections = &mut geometry.intersections[edge_index];

    for row in geometry.top..=geometry.bottom {
        let centerline = corner.col + ((row as f64 + 0.5) - corner.row) / slope;
        intersections.centerlines.insert(row, centerline);

        let (shift_1, shift_2) = if edge_type == EdgeType::LeftDiagonal {
            (-0.5, 0.5)
        } else {
            (0.5, -0.5)
        };

        intersections.edges.insert(
            row,
            [
                centerline + shift_1 / slope.abs(),
                centerline + shift_2 / slope.abs(),
            ],
        );
        intersections
            .fractions
            .insert(row, [centerline - 0.25 / slope, centerline + 0.25 / slope]);
    }

    let mut min_angle_difference = f64::INFINITY;
    let mut closest_slope = None;
    for block_slope in DIAGONAL_SLOPES {
        let angle_difference =
            ((BLOCK_ASPECT_RATIO * block_slope).atan() - geometry.angles[edge_index]).abs();
        if angle_difference < min_angle_difference {
            min_angle_difference = angle_difference;
            closest_slope = Some(block_slope);
        }
    }

    if let Some(slope) = closest_slope
        && min_angle_difference <= frame.max_angle_difference_diagonal
    {
        geometry.slopes[edge_index] = slope;
    }
}

fn precompute_quad_geometry(corners: &[Point; 4], frame: &RenderFrame) -> QuadGeometry {
    let top = corners
        .iter()
        .fold(f64::INFINITY, |acc, corner| acc.min(corner.row))
        .floor() as i64;
    let bottom = corners
        .iter()
        .fold(f64::NEG_INFINITY, |acc, corner| acc.max(corner.row))
        .ceil() as i64
        - 1;
    let left = corners
        .iter()
        .fold(f64::INFINITY, |acc, corner| acc.min(corner.col))
        .floor() as i64;
    let right = corners
        .iter()
        .fold(f64::NEG_INFINITY, |acc, corner| acc.max(corner.col))
        .ceil() as i64
        - 1;

    let mut slopes = [0.0; 4];
    let mut angles = [0.0; 4];
    let mut edge_types = [EdgeType::None; 4];

    for edge_index in 0..4 {
        let next_index = (edge_index + 1) % 4;
        let edge_row = corners[next_index].row - corners[edge_index].row;
        let edge_col = corners[next_index].col - corners[edge_index].col;
        let slope = edge_row / edge_col;
        slopes[edge_index] = slope;
        angles[edge_index] = (BLOCK_ASPECT_RATIO * slope).atan();

        let abs_slope = slope.abs();
        edge_types[edge_index] = if abs_slope.is_nan() {
            EdgeType::None
        } else if abs_slope <= frame.max_slope_horizontal {
            if edge_col > 0.0 {
                EdgeType::Top
            } else {
                EdgeType::Bottom
            }
        } else if abs_slope >= frame.min_slope_vertical {
            if edge_row > 0.0 {
                EdgeType::Right
            } else {
                EdgeType::Left
            }
        } else if edge_row > 0.0 {
            EdgeType::RightDiagonal
        } else {
            EdgeType::LeftDiagonal
        };
    }

    let mut geometry = QuadGeometry {
        top,
        bottom,
        left,
        right,
        slopes,
        angles,
        edge_types,
        intersections: std::array::from_fn(|_| EdgeIntersections::default()),
    };

    for edge_index in 0..4 {
        match geometry.edge_types[edge_index] {
            EdgeType::Top | EdgeType::Bottom => {
                precompute_intersections_horizontal(corners, &mut geometry, edge_index)
            }
            EdgeType::Left | EdgeType::Right => {
                precompute_intersections_vertical(corners, &mut geometry, edge_index)
            }
            EdgeType::LeftDiagonal | EdgeType::RightDiagonal => {
                precompute_intersections_diagonal(corners, &mut geometry, edge_index, frame)
            }
            EdgeType::None => {}
        }
    }

    geometry
}

fn get_edge_cell_intersection(
    edge_index: usize,
    row: i64,
    col: i64,
    geometry: &QuadGeometry,
    low: bool,
) -> f64 {
    let intersections = &geometry.intersections[edge_index];
    match geometry.edge_types[edge_index] {
        EdgeType::Top => intersections
            .centerlines
            .get(&col)
            .map_or(0.0, |centerline| centerline - row as f64),
        EdgeType::Bottom => intersections
            .centerlines
            .get(&col)
            .map_or(0.0, |centerline| row as f64 + 1.0 - centerline),
        EdgeType::Left => intersections
            .centerlines
            .get(&row)
            .map_or(0.0, |centerline| centerline - col as f64),
        EdgeType::Right => intersections
            .centerlines
            .get(&row)
            .map_or(0.0, |centerline| col as f64 + 1.0 - centerline),
        EdgeType::LeftDiagonal => intersections
            .edges
            .get(&row)
            .map_or(0.0, |edges| edges[if low { 0 } else { 1 }] - col as f64),
        EdgeType::RightDiagonal => intersections.edges.get(&row).map_or(0.0, |edges| {
            col as f64 + 1.0 - edges[if low { 0 } else { 1 }]
        }),
        EdgeType::None => 0.0,
    }
}

fn update_matrix_with_top_edge(
    edge_index: usize,
    fraction_index: usize,
    row: i64,
    col: i64,
    geometry: &QuadGeometry,
    matrix: &mut [[f64; 2]; 2],
) {
    let Some(fractions) = geometry.intersections[edge_index].fractions.get(&col) else {
        return;
    };

    let row_float = 2.0 * (fractions[fraction_index] - row as f64);
    let matrix_index = row_float.floor() as i64 + 1;

    let upper = (matrix_index - 1).min(2);
    if upper >= 1 {
        for index in 1..=upper {
            matrix[(index - 1) as usize][fraction_index] = 0.0;
        }
    }

    if matrix_index == 1 || matrix_index == 2 {
        let shade = 1.0 - row_float.rem_euclid(1.0);
        matrix[(matrix_index - 1) as usize][fraction_index] *= shade;
    }
}

fn update_matrix_with_bottom_edge(
    edge_index: usize,
    fraction_index: usize,
    row: i64,
    col: i64,
    geometry: &QuadGeometry,
    matrix: &mut [[f64; 2]; 2],
) {
    let Some(fractions) = geometry.intersections[edge_index].fractions.get(&col) else {
        return;
    };

    let row_float = 2.0 * (fractions[fraction_index] - row as f64);
    let matrix_index = row_float.floor() as i64 + 1;

    let start = (matrix_index + 1).max(1);
    if start <= 2 {
        for index in start..=2 {
            matrix[(index - 1) as usize][fraction_index] = 0.0;
        }
    }

    if matrix_index == 1 || matrix_index == 2 {
        let shade = row_float.rem_euclid(1.0);
        matrix[(matrix_index - 1) as usize][fraction_index] *= shade;
    }
}

fn update_matrix_with_left_edge(
    edge_index: usize,
    fraction_index: usize,
    row: i64,
    col: i64,
    geometry: &QuadGeometry,
    matrix: &mut [[f64; 2]; 2],
) {
    let Some(fractions) = geometry.intersections[edge_index].fractions.get(&row) else {
        return;
    };

    let col_float = 2.0 * (fractions[fraction_index] - col as f64);
    let matrix_index = col_float.floor() as i64 + 1;

    let upper = (matrix_index - 1).min(2);
    if upper >= 1 {
        for index in 1..=upper {
            matrix[fraction_index][(index - 1) as usize] = 0.0;
        }
    }

    if matrix_index == 1 || matrix_index == 2 {
        let shade = 1.0 - col_float.rem_euclid(1.0);
        matrix[fraction_index][(matrix_index - 1) as usize] *= shade;
    }
}

fn update_matrix_with_right_edge(
    edge_index: usize,
    fraction_index: usize,
    row: i64,
    col: i64,
    geometry: &QuadGeometry,
    matrix: &mut [[f64; 2]; 2],
) {
    let Some(fractions) = geometry.intersections[edge_index].fractions.get(&row) else {
        return;
    };

    let col_float = 2.0 * (fractions[fraction_index] - col as f64);
    let matrix_index = col_float.floor() as i64 + 1;

    let start = (matrix_index + 1).max(1);
    if start <= 2 {
        for index in start..=2 {
            matrix[fraction_index][(index - 1) as usize] = 0.0;
        }
    }

    if matrix_index == 1 || matrix_index == 2 {
        let shade = col_float.rem_euclid(1.0);
        matrix[fraction_index][(matrix_index - 1) as usize] *= shade;
    }
}

fn update_matrix_with_edge(
    edge_index: usize,
    fraction_index: usize,
    row: i64,
    col: i64,
    geometry: &QuadGeometry,
    matrix: &mut [[f64; 2]; 2],
) {
    match geometry.edge_types[edge_index] {
        EdgeType::Top => {
            update_matrix_with_top_edge(edge_index, fraction_index, row, col, geometry, matrix)
        }
        EdgeType::Bottom => {
            update_matrix_with_bottom_edge(edge_index, fraction_index, row, col, geometry, matrix)
        }
        EdgeType::Left | EdgeType::LeftDiagonal => {
            update_matrix_with_left_edge(edge_index, fraction_index, row, col, geometry, matrix)
        }
        EdgeType::Right | EdgeType::RightDiagonal => {
            update_matrix_with_right_edge(edge_index, fraction_index, row, col, geometry, matrix)
        }
        EdgeType::None => {}
    }
}

fn diagonal_blocks_for_slope(
    edge_type: EdgeType,
    slope: f64,
) -> Option<&'static [(f64, &'static str)]> {
    let slope_matches = |expected: f64| (slope - expected).abs() <= 1.0e-9;

    match edge_type {
        EdgeType::LeftDiagonal => {
            if slope_matches(-2.0) {
                Some(RIGHT_DIAGONAL_NEG_2)
            } else if slope_matches(-4.0 / 3.0) {
                Some(RIGHT_DIAGONAL_NEG_4_3)
            } else if slope_matches(-1.0) {
                Some(RIGHT_DIAGONAL_NEG_1)
            } else if slope_matches(-2.0 / 3.0) {
                Some(RIGHT_DIAGONAL_NEG_2_3)
            } else if slope_matches(-1.0 / 3.0) {
                Some(RIGHT_DIAGONAL_NEG_1_3)
            } else if slope_matches(1.0 / 3.0) {
                Some(RIGHT_DIAGONAL_POS_1_3)
            } else if slope_matches(2.0 / 3.0) {
                Some(RIGHT_DIAGONAL_POS_2_3)
            } else if slope_matches(1.0) {
                Some(RIGHT_DIAGONAL_POS_1)
            } else if slope_matches(4.0 / 3.0) {
                Some(RIGHT_DIAGONAL_POS_4_3)
            } else if slope_matches(2.0) {
                Some(RIGHT_DIAGONAL_POS_2)
            } else {
                None
            }
        }
        EdgeType::RightDiagonal => {
            if slope_matches(-2.0) {
                Some(LEFT_DIAGONAL_NEG_2)
            } else if slope_matches(-4.0 / 3.0) {
                Some(LEFT_DIAGONAL_NEG_4_3)
            } else if slope_matches(-1.0) {
                Some(LEFT_DIAGONAL_NEG_1)
            } else if slope_matches(-2.0 / 3.0) {
                Some(LEFT_DIAGONAL_NEG_2_3)
            } else if slope_matches(-1.0 / 3.0) {
                Some(LEFT_DIAGONAL_NEG_1_3)
            } else if slope_matches(1.0 / 3.0) {
                Some(LEFT_DIAGONAL_POS_1_3)
            } else if slope_matches(2.0 / 3.0) {
                Some(LEFT_DIAGONAL_POS_2_3)
            } else if slope_matches(1.0) {
                Some(LEFT_DIAGONAL_POS_1)
            } else if slope_matches(4.0 / 3.0) {
                Some(LEFT_DIAGONAL_POS_4_3)
            } else if slope_matches(2.0) {
                Some(LEFT_DIAGONAL_POS_2)
            } else {
                None
            }
        }
        _ => None,
    }
}

fn compute_gradient_shade(row_center: f64, col_center: f64, frame: &RenderFrame) -> f64 {
    let Some(gradient) = frame.gradient else {
        return 1.0;
    };

    let dy = row_center - gradient.origin.row;
    let dx = col_center - gradient.origin.col;
    let projection =
        (dy * gradient.direction_scaled.row + dx * gradient.direction_scaled.col).clamp(0.0, 1.0);
    (1.0 - projection).powf(frame.gradient_exponent)
}

fn editor_bounds() -> Result<(i64, i64)> {
    let opts = OptionOpts::builder().build();
    let lines: i64 = api::get_option_value("lines", &opts)?;
    let cmdheight: i64 = api::get_option_value("cmdheight", &opts)?;
    let columns: i64 = api::get_option_value("columns", &opts)?;
    let max_row = (lines - cmdheight).max(1);
    let max_col = columns.max(1);
    Ok((max_row, max_col))
}

fn draw_character(
    draw_state: &mut DrawState,
    namespace_id: u32,
    row: i64,
    col: i64,
    character: &str,
    hl_group: &str,
    zindex: u32,
    max_row: i64,
    max_col: i64,
) -> Result<()> {
    if row < 1 || row > max_row || col < 1 || col > max_col {
        return Ok(());
    }

    let tab_handle = api::get_current_tabpage().handle();
    let (_, mut buffer) =
        get_or_create_window(draw_state, namespace_id, tab_handle, row, col, zindex)?;

    let extmark_opts = SetExtmarkOpts::builder()
        .id(EXTMARK_ID)
        .virt_text([(character, hl_group)])
        .virt_text_pos(ExtmarkVirtTextPosition::Overlay)
        .virt_text_win_col(0)
        .build();

    buffer.set_extmark(namespace_id, 0, 0, &extmark_opts)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        ADAPTIVE_POOL_BUDGET_MARGIN, ADAPTIVE_POOL_EWMA_SCALE, ADAPTIVE_POOL_HARD_MAX_BUDGET,
        ADAPTIVE_POOL_MIN_BUDGET, AdaptiveBudgetState, CachedRenderWindow, CachedWindowLifecycle,
        EpochRollover, FrameEpoch, WindowBufferHandle, lru_prune_indices, next_adaptive_budget,
    };

    #[test]
    fn adaptive_budget_has_floor_when_idle() {
        let previous = AdaptiveBudgetState {
            ewma_demand_milli: 0,
            cached_budget: ADAPTIVE_POOL_MIN_BUDGET,
        };

        let next = next_adaptive_budget(previous, 0);
        assert_eq!(next.cached_budget, ADAPTIVE_POOL_MIN_BUDGET);
        assert_eq!(next.ewma_demand_milli, 0);
    }

    #[test]
    fn adaptive_budget_grows_with_demand() {
        let previous = AdaptiveBudgetState {
            ewma_demand_milli: 0,
            cached_budget: ADAPTIVE_POOL_MIN_BUDGET,
        };

        let next = next_adaptive_budget(previous, 120);
        assert_eq!(next.ewma_demand_milli, 120_u64 * ADAPTIVE_POOL_EWMA_SCALE);
        assert_eq!(next.cached_budget, 120 + ADAPTIVE_POOL_BUDGET_MARGIN);
    }

    #[test]
    fn adaptive_budget_shrinks_gradually() {
        let previous = AdaptiveBudgetState {
            ewma_demand_milli: 120_u64 * ADAPTIVE_POOL_EWMA_SCALE,
            cached_budget: 120,
        };

        let next = next_adaptive_budget(previous, 0);
        assert_eq!(next.cached_budget, 112);
    }

    #[test]
    fn adaptive_budget_honors_hard_max() {
        let previous = AdaptiveBudgetState {
            ewma_demand_milli: 0,
            cached_budget: ADAPTIVE_POOL_MIN_BUDGET,
        };

        let next = next_adaptive_budget(previous, 10_000);
        assert_eq!(next.cached_budget, ADAPTIVE_POOL_HARD_MAX_BUDGET);
    }

    fn cached(window_id: i32, buffer_id: i32, last_used_epoch: u64) -> CachedRenderWindow {
        CachedRenderWindow {
            handles: WindowBufferHandle {
                window_id,
                buffer_id,
            },
            lifecycle: CachedWindowLifecycle::Available {
                last_used_epoch: FrameEpoch(last_used_epoch),
            },
        }
    }

    #[test]
    fn rollover_releases_in_use_window_from_previous_epoch() {
        let handles = WindowBufferHandle {
            window_id: 10,
            buffer_id: 11,
        };
        let mut cached = CachedRenderWindow::new_in_use(handles, FrameEpoch(9));
        assert_eq!(
            cached.rollover_to_next_epoch(FrameEpoch(9)),
            EpochRollover::ReleasedForReuse
        );
        assert_eq!(cached.available_epoch(), Some(FrameEpoch(9)));
        assert!(cached.is_available_for_reuse());
    }

    #[test]
    fn rollover_recovers_stale_in_use_window() {
        let handles = WindowBufferHandle {
            window_id: 20,
            buffer_id: 21,
        };
        let mut cached = CachedRenderWindow::new_in_use(handles, FrameEpoch(3));
        assert_eq!(
            cached.rollover_to_next_epoch(FrameEpoch(5)),
            EpochRollover::RecoveredStaleInUse
        );
        assert_eq!(cached.available_epoch(), Some(FrameEpoch(3)));
        assert!(cached.is_available_for_reuse());
    }

    #[test]
    fn lru_prune_indices_empty_when_budget_sufficient() {
        let windows = vec![cached(1, 10, 7), cached(2, 20, 8)];
        assert!(lru_prune_indices(&windows, 2).is_empty());
        assert!(lru_prune_indices(&windows, 3).is_empty());
    }

    #[test]
    fn lru_prune_indices_removes_oldest_epochs_deterministically() {
        let windows = vec![
            cached(1, 10, 9),
            CachedRenderWindow::new_in_use(
                WindowBufferHandle {
                    window_id: 90,
                    buffer_id: 99,
                },
                FrameEpoch(9),
            ),
            cached(2, 20, 1),
            cached(3, 30, 4),
            cached(4, 40, 1),
            cached(5, 50, 7),
        ];

        // Keep three newest reusable epochs; in-use windows are excluded from LRU pruning.
        assert_eq!(lru_prune_indices(&windows, 3), vec![2, 4]);
    }
}

fn draw_partial_block(
    draw_state: &mut DrawState,
    namespace_id: u32,
    row: i64,
    col: i64,
    properties: PartialBlockProperties,
    hl_groups: &[String],
    inverted_hl_groups: &[String],
    windows_zindex: u32,
    max_row: i64,
    max_col: i64,
) -> Result<()> {
    let characters = block_characters(properties.character_set);
    let Ok(character_index) = usize::try_from(properties.character_index) else {
        return Ok(());
    };
    let Some(character) = characters.get(character_index).copied() else {
        return Ok(());
    };

    let level_index = usize::try_from(properties.level)
        .unwrap_or(0)
        .min((hl_groups.len().saturating_sub(1)).min(inverted_hl_groups.len().saturating_sub(1)));
    if level_index == 0 {
        return Ok(());
    }

    let hl_group = if properties.inverted {
        inverted_hl_groups[level_index].as_str()
    } else {
        hl_groups[level_index].as_str()
    };

    draw_character(
        draw_state,
        namespace_id,
        row,
        col,
        character,
        hl_group,
        windows_zindex,
        max_row,
        max_col,
    )
}

fn get_top_block_properties(
    micro_shift: f64,
    thickness: f64,
    shade: f64,
    frame: &RenderFrame,
) -> Option<PartialBlockProperties> {
    let character_index = micro_shift.ceil() as i64;
    if character_index == 0 {
        return None;
    }

    let character_thickness = character_index as f64 / 8.0;
    let adjusted_shade = shade * thickness / character_thickness;
    let level = level_from_shade(adjusted_shade, frame.color_levels);
    if level == 0 {
        return None;
    }

    if frame.legacy_computing_symbols_support {
        Some(PartialBlockProperties {
            character_index,
            character_set: BlockCharacterSet::Top,
            level,
            inverted: false,
        })
    } else {
        Some(PartialBlockProperties {
            character_index,
            character_set: BlockCharacterSet::Bottom,
            level,
            inverted: true,
        })
    }
}

fn get_bottom_block_properties(
    micro_shift: f64,
    thickness: f64,
    shade: f64,
    frame: &RenderFrame,
) -> Option<PartialBlockProperties> {
    let character_index = micro_shift.floor() as i64;
    if character_index == 8 {
        return None;
    }

    let character_thickness = 1.0 - character_index as f64 / 8.0;
    let adjusted_shade = shade * thickness / character_thickness;
    let level = level_from_shade(adjusted_shade, frame.color_levels);
    if level == 0 {
        return None;
    }

    Some(PartialBlockProperties {
        character_index,
        character_set: BlockCharacterSet::Bottom,
        level,
        inverted: false,
    })
}

fn get_vertical_bar_properties(
    micro_shift: f64,
    thickness: f64,
    shade: f64,
    frame: &RenderFrame,
) -> Option<PartialBlockProperties> {
    let character_index = micro_shift.floor() as i64;
    if !(0..8).contains(&character_index) {
        return None;
    }

    let character_thickness = 1.0 / 8.0;
    let adjusted_shade = (shade * thickness / character_thickness).min(1.0);
    let level = level_from_shade(adjusted_shade, frame.color_levels);
    if level == 0 {
        return None;
    }

    Some(PartialBlockProperties {
        character_index,
        character_set: BlockCharacterSet::VerticalBars,
        level,
        inverted: false,
    })
}

fn get_left_block_properties(
    micro_shift: f64,
    thickness: f64,
    shade: f64,
    frame: &RenderFrame,
) -> Option<PartialBlockProperties> {
    let character_index = micro_shift.ceil() as i64;
    if character_index == 0 {
        return None;
    }

    let character_thickness = character_index as f64 / 8.0;
    let adjusted_shade = shade * thickness / character_thickness;
    let level = level_from_shade(adjusted_shade, frame.color_levels);
    if level == 0 {
        return None;
    }

    Some(PartialBlockProperties {
        character_index,
        character_set: BlockCharacterSet::Left,
        level,
        inverted: false,
    })
}

fn get_right_block_properties(
    micro_shift: f64,
    thickness: f64,
    shade: f64,
    frame: &RenderFrame,
) -> Option<PartialBlockProperties> {
    let character_index = micro_shift.floor() as i64;
    if character_index == 8 {
        return None;
    }

    let character_thickness = 1.0 - character_index as f64 / 8.0;
    let adjusted_shade = shade * thickness / character_thickness;
    let level = level_from_shade(adjusted_shade, frame.color_levels);
    if level == 0 {
        return None;
    }

    if frame.legacy_computing_symbols_support {
        Some(PartialBlockProperties {
            character_index,
            character_set: BlockCharacterSet::Right,
            level,
            inverted: false,
        })
    } else {
        Some(PartialBlockProperties {
            character_index,
            character_set: BlockCharacterSet::Left,
            level,
            inverted: true,
        })
    }
}

struct DrawResources<'a> {
    draw_state: &'a mut DrawState,
    hl_groups: &'a [String],
    inverted_hl_groups: &'a [String],
    max_row: i64,
    max_col: i64,
    windows_zindex: u32,
    particle_zindex: u32,
}

fn draw_vertically_shifted_sub_block(
    resources: &mut DrawResources<'_>,
    namespace_id: u32,
    frame: &RenderFrame,
    bulge_above: bool,
    row_top: f64,
    row_bottom: f64,
    col: i64,
    shade: f64,
) -> Result<bool> {
    if row_top >= row_bottom {
        return Ok(false);
    }

    let row = row_top.floor() as i64;
    let center = frac01((row_top + row_bottom) / 2.0);
    let thickness = row_bottom - row_top;
    let gap_top = frac01(row_top);
    let gap_bottom = frac01(1.0 - row_bottom);

    let properties = if gap_top.max(gap_bottom) / 2.0 < gap_top.min(gap_bottom) {
        if bulge_above {
            let micro_shift = frac01(row_bottom) * 8.0;
            get_top_block_properties(micro_shift, thickness, shade, frame)
        } else {
            let micro_shift = frac01(row_top) * 8.0;
            get_bottom_block_properties(micro_shift, thickness, shade, frame)
        }
    } else if center < 0.5 {
        get_top_block_properties(center * 16.0, thickness, shade, frame)
    } else {
        get_bottom_block_properties(center * 16.0 - 8.0, thickness, shade, frame)
    };

    let Some(properties) = properties else {
        return Ok(false);
    };

    draw_partial_block(
        resources.draw_state,
        namespace_id,
        row,
        col,
        properties,
        resources.hl_groups,
        resources.inverted_hl_groups,
        resources.windows_zindex,
        resources.max_row,
        resources.max_col,
    )?;
    Ok(true)
}

fn draw_horizontally_shifted_sub_block(
    resources: &mut DrawResources<'_>,
    namespace_id: u32,
    frame: &RenderFrame,
    bulge_above: bool,
    row: i64,
    col_left: f64,
    col_right: f64,
    shade: f64,
) -> Result<bool> {
    if col_left >= col_right {
        return Ok(false);
    }

    let col = col_left.floor() as i64;
    let center = frac01((col_left + col_right) / 2.0);
    let thickness = col_right - col_left;
    let gap_left = frac01(col_left);
    let gap_right = frac01(1.0 - col_right);

    let properties = if (frame.legacy_computing_symbols_support
        || frame.legacy_computing_symbols_support_vertical_bars)
        && thickness <= 1.5 / 8.0
    {
        get_vertical_bar_properties(center * 8.0, thickness, shade, frame)
    } else if gap_left.max(gap_right) / 2.0 < gap_left.min(gap_right) {
        if bulge_above {
            get_left_block_properties(frac01(col_right) * 8.0, thickness, shade, frame)
        } else {
            get_right_block_properties(frac01(col_left) * 8.0, thickness, shade, frame)
        }
    } else if center < 0.5 {
        get_left_block_properties(center * 16.0, thickness, shade, frame)
    } else {
        get_right_block_properties(center * 16.0 - 8.0, thickness, shade, frame)
    };

    let Some(properties) = properties else {
        return Ok(false);
    };

    draw_partial_block(
        resources.draw_state,
        namespace_id,
        row,
        col,
        properties,
        resources.hl_groups,
        resources.inverted_hl_groups,
        resources.windows_zindex,
        resources.max_row,
        resources.max_col,
    )?;
    Ok(true)
}

fn draw_diagonal_block(
    resources: &mut DrawResources<'_>,
    namespace_id: u32,
    frame: &RenderFrame,
    geometry: &QuadGeometry,
    edge_index: usize,
    row: i64,
    col: i64,
    shade: f64,
) -> Result<bool> {
    let edge_type = geometry.edge_types[edge_index];
    let slope = geometry.slopes[edge_index];
    let Some(candidates) = diagonal_blocks_for_slope(edge_type, slope) else {
        return Ok(false);
    };

    let Some(centerline) = geometry.intersections[edge_index]
        .centerlines
        .get(&row)
        .copied()
    else {
        return Ok(false);
    };

    let mut min_offset = f64::INFINITY;
    let mut matching_character = None;
    for (shift, character) in candidates.iter().copied() {
        let offset = (centerline - col as f64 - 0.5 - shift).abs();
        if offset < min_offset {
            min_offset = offset;
            matching_character = Some(character);
        }
    }

    let Some(character) = matching_character else {
        return Ok(false);
    };
    if min_offset > frame.max_offset_diagonal {
        return Ok(false);
    }

    let adjusted_shade = if frame.vertical_bar {
        shade / 8.0
    } else {
        shade
    };
    let level = level_from_shade(adjusted_shade, frame.color_levels);
    if level == 0 {
        return Ok(false);
    }

    let level_index = usize::try_from(level)
        .unwrap_or(0)
        .min(resources.hl_groups.len().saturating_sub(1));
    if level_index == 0 {
        return Ok(false);
    }
    let hl_group = resources.hl_groups[level_index].as_str();

    draw_character(
        resources.draw_state,
        namespace_id,
        row,
        col,
        character,
        hl_group,
        resources.windows_zindex,
        resources.max_row,
        resources.max_col,
    )?;

    Ok(true)
}

fn draw_matrix_character(
    resources: &mut DrawResources<'_>,
    namespace_id: u32,
    frame: &RenderFrame,
    row: i64,
    col: i64,
    matrix: [[f64; 2]; 2],
    shade: f64,
) -> Result<bool> {
    let max_matrix_coverage = matrix
        .iter()
        .flat_map(|row_values| row_values.iter())
        .copied()
        .fold(0.0_f64, f64::max);

    let matrix_pixel_threshold = if frame.vertical_bar {
        frame.matrix_pixel_threshold_vertical_bar
    } else {
        frame.matrix_pixel_threshold
    };

    if max_matrix_coverage < matrix_pixel_threshold {
        return Ok(false);
    }

    let threshold = max_matrix_coverage * frame.matrix_pixel_min_factor;
    let bit_1 = usize::from(matrix[0][0] > threshold);
    let bit_2 = usize::from(matrix[0][1] > threshold);
    let bit_3 = usize::from(matrix[1][0] > threshold);
    let bit_4 = usize::from(matrix[1][1] > threshold);
    let index = bit_1 + bit_2 * 2 + bit_3 * 4 + bit_4 * 8;
    if index == 0 {
        return Ok(false);
    }

    let matrix_shade = matrix[0][0] + matrix[0][1] + matrix[1][0] + matrix[1][1];
    let max_matrix_shade = bit_1 + bit_2 + bit_3 + bit_4;
    if max_matrix_shade == 0 {
        return Ok(false);
    }

    let level = level_from_shade(
        shade * matrix_shade / max_matrix_shade as f64,
        frame.color_levels,
    );
    if level == 0 {
        return Ok(false);
    }

    let level_index = usize::try_from(level)
        .unwrap_or(0)
        .min(resources.hl_groups.len().saturating_sub(1));
    if level_index == 0 {
        return Ok(false);
    }
    let hl_group = resources.hl_groups[level_index].as_str();

    draw_character(
        resources.draw_state,
        namespace_id,
        row,
        col,
        MATRIX_CHARACTERS[index],
        hl_group,
        resources.windows_zindex,
        resources.max_row,
        resources.max_col,
    )?;
    Ok(true)
}

fn draw_braille_character(
    resources: &mut DrawResources<'_>,
    namespace_id: u32,
    row: i64,
    col: i64,
    cell: &[[f64; 2]; 4],
    hl_group: &str,
    zindex: u32,
) -> Result<bool> {
    let braille_index = usize::from(cell[0][0] > 0.0)
        + usize::from(cell[1][0] > 0.0) * 2
        + usize::from(cell[2][0] > 0.0) * 4
        + usize::from(cell[0][1] > 0.0) * 8
        + usize::from(cell[1][1] > 0.0) * 16
        + usize::from(cell[2][1] > 0.0) * 32
        + usize::from(cell[3][0] > 0.0) * 64
        + usize::from(cell[3][1] > 0.0) * 128;

    if braille_index == 0 {
        return Ok(false);
    }

    let Some(character) = char::from_u32((BRAILLE_CODE_MIN as usize + braille_index) as u32) else {
        return Ok(false);
    };
    let character_text = character.to_string();

    draw_character(
        resources.draw_state,
        namespace_id,
        row,
        col,
        &character_text,
        hl_group,
        zindex,
        resources.max_row,
        resources.max_col,
    )?;

    Ok(true)
}

fn draw_octant_character(
    resources: &mut DrawResources<'_>,
    namespace_id: u32,
    row: i64,
    col: i64,
    cell: &[[f64; 2]; 4],
    hl_group: &str,
    zindex: u32,
) -> Result<bool> {
    let octant_index = usize::from(cell[0][0] > 0.0)
        + usize::from(cell[0][1] > 0.0) * 2
        + usize::from(cell[1][0] > 0.0) * 4
        + usize::from(cell[1][1] > 0.0) * 8
        + usize::from(cell[2][0] > 0.0) * 16
        + usize::from(cell[2][1] > 0.0) * 32
        + usize::from(cell[3][0] > 0.0) * 64
        + usize::from(cell[3][1] > 0.0) * 128;

    if octant_index == 0 {
        return Ok(false);
    }

    let Some(character) = OCTANT_CHARACTERS.get(octant_index - 1).copied() else {
        return Ok(false);
    };

    draw_character(
        resources.draw_state,
        namespace_id,
        row,
        col,
        character,
        hl_group,
        zindex,
        resources.max_row,
        resources.max_col,
    )?;

    Ok(true)
}

fn draw_particles(
    resources: &mut DrawResources<'_>,
    namespace_id: u32,
    frame: &RenderFrame,
    target_row: i64,
    target_col: i64,
) -> Result<()> {
    if frame.particles.is_empty() {
        return Ok(());
    }

    let lifetime_switch_octant_braille = if frame.legacy_computing_symbols_support {
        frame.particle_max_lifetime * frame.particle_switch_octant_braille
    } else {
        f64::INFINITY
    };

    let mut cells: HashMap<(i64, i64), [[f64; 2]; 4]> = HashMap::new();
    for particle in &frame.particles {
        let row = particle.position.row.floor() as i64;
        let col = particle.position.col.floor() as i64;
        let sub_row = round_lua(4.0 * frac01(particle.position.row) + 0.5).clamp(1, 4);
        let sub_col = round_lua(2.0 * frac01(particle.position.col) + 0.5).clamp(1, 2);

        let cell = cells.entry((row, col)).or_insert([[0.0; 2]; 4]);
        cell[(sub_row - 1) as usize][(sub_col - 1) as usize] += particle.lifetime;
    }

    for ((row, col), cell) in cells {
        if row == target_row && col == target_col {
            continue;
        }
        if row < 1 || row > resources.max_row || col < 1 || col > resources.max_col {
            continue;
        }

        if !frame.particles_over_text {
            let args = Array::from_iter([Object::from(row), Object::from(col)]);
            let bg_char_code = match api::call_function("screenchar", args) {
                Ok(value) => match i64_from_object("screenchar", value) {
                    Ok(parsed) => parsed,
                    Err(_) => continue,
                },
                Err(_) => continue,
            };

            let is_space = bg_char_code == 32;
            let is_braille = (BRAILLE_CODE_MIN..=BRAILLE_CODE_MAX).contains(&bg_char_code);
            let is_octant = (OCTANT_CODE_MIN..=OCTANT_CODE_MAX).contains(&bg_char_code);
            if !is_space && !is_braille && !is_octant {
                continue;
            }
        }

        let num_dots = cell
            .iter()
            .flat_map(|row_values| row_values.iter())
            .filter(|value| **value > 0.0)
            .count();
        if num_dots == 0 {
            continue;
        }

        let lifetime_sum = cell
            .iter()
            .flat_map(|row_values| row_values.iter())
            .copied()
            .sum::<f64>();
        let lifetime_average = lifetime_sum / num_dots as f64;

        let shade = if lifetime_average > lifetime_switch_octant_braille {
            let denominator = frame.particle_max_lifetime - lifetime_switch_octant_braille;
            if denominator <= 0.0 {
                1.0
            } else {
                ((lifetime_average - lifetime_switch_octant_braille) / denominator).clamp(0.0, 1.0)
            }
        } else {
            let denominator = frame
                .particle_max_lifetime
                .min(lifetime_switch_octant_braille)
                .max(1.0e-9);
            (lifetime_average / denominator).clamp(0.0, 1.0)
        };

        let level = level_from_shade(shade, frame.color_levels);
        if level == 0 {
            continue;
        }

        let level_index = usize::try_from(level)
            .unwrap_or(0)
            .min(resources.hl_groups.len().saturating_sub(1));
        if level_index == 0 {
            continue;
        }
        let hl_group = resources.hl_groups[level_index].as_str();

        if lifetime_average > lifetime_switch_octant_braille {
            draw_octant_character(
                resources,
                namespace_id,
                row,
                col,
                &cell,
                hl_group,
                resources.particle_zindex,
            )?;
        } else {
            draw_braille_character(
                resources,
                namespace_id,
                row,
                col,
                &cell,
                hl_group,
                resources.particle_zindex,
            )?;
        }
    }

    Ok(())
}

pub(crate) fn draw_target_hack_block(namespace_id: u32, frame: &RenderFrame) -> Result<()> {
    if namespace_id == 0 || !frame.hide_target_hack || frame.vertical_bar {
        return Ok(());
    }

    ensure_highlight_palette(frame)?;
    let (editor_max_row, editor_max_col) = editor_bounds()?;
    let mut draw_state = draw_state_lock();
    let level = frame.color_levels.max(1);
    let hl_group = hl_group_name(level);
    draw_character(
        &mut draw_state,
        namespace_id,
        frame.target.row.round() as i64,
        frame.target.col.round() as i64,
        "█",
        hl_group.as_str(),
        frame.windows_zindex,
        editor_max_row,
        editor_max_col,
    )
}

pub(crate) fn draw_current(namespace_id: u32, frame: &RenderFrame) -> Result<()> {
    if namespace_id == 0 {
        return Ok(());
    }

    ensure_highlight_palette(frame)?;

    let corners = ensure_clockwise(&frame.corners);
    let geometry = precompute_quad_geometry(&corners, frame);
    let (editor_max_row, editor_max_col) = editor_bounds()?;
    let mut draw_state = draw_state_lock();
    clear_cached_windows(&mut draw_state, namespace_id);
    if geometry.top > geometry.bottom || geometry.left > geometry.right {
        return Ok(());
    }

    draw_state.bulge_above = !draw_state.bulge_above;
    let bulge_above = draw_state.bulge_above;

    let target_row = frame.target.row.round() as i64;
    let target_col = frame.target.col.round() as i64;
    let color_levels = frame.color_levels.max(1);
    let hl_groups: Vec<String> = (0..=color_levels).map(hl_group_name).collect();
    let inverted_hl_groups: Vec<String> = (0..=color_levels).map(inverted_hl_group_name).collect();
    let min_shade_no_diagonal = if frame.vertical_bar {
        frame.min_shade_no_diagonal_vertical_bar
    } else {
        frame.min_shade_no_diagonal
    };

    {
        let mut resources = DrawResources {
            draw_state: &mut draw_state,
            hl_groups: &hl_groups,
            inverted_hl_groups: &inverted_hl_groups,
            max_row: editor_max_row,
            max_col: editor_max_col,
            windows_zindex: frame.windows_zindex,
            particle_zindex: frame.windows_zindex.saturating_sub(PARTICLE_ZINDEX_OFFSET),
        };

        draw_particles(&mut resources, namespace_id, frame, target_row, target_col)?;

        for row in geometry.top..=geometry.bottom {
            for col in geometry.left..=geometry.right {
                if frame.never_draw_over_target
                    && !frame.vertical_bar
                    && row == target_row
                    && col == target_col
                {
                    continue;
                }

                let mut intersections = CellIntersections::default();
                let mut single_diagonal = true;
                let mut diagonal_edge_index = None;
                let mut skip_cell = false;

                for edge_index in 0..4 {
                    let intersection =
                        get_edge_cell_intersection(edge_index, row, col, &geometry, false);
                    match geometry.edge_types[edge_index] {
                        EdgeType::LeftDiagonal | EdgeType::RightDiagonal => {
                            let intersection_low =
                                get_edge_cell_intersection(edge_index, row, col, &geometry, true);
                            if intersection_low >= 1.0 {
                                skip_cell = true;
                                break;
                            }

                            if intersection > min_shade_no_diagonal
                                && intersections.diagonal.is_some()
                            {
                                single_diagonal = false;
                            }

                            if intersection > 0.0
                                && intersections
                                    .diagonal
                                    .is_none_or(|current| intersection > current)
                            {
                                intersections.diagonal = Some(intersection);
                                diagonal_edge_index = Some(edge_index);
                            }
                        }
                        edge_type => {
                            if intersection >= 1.0 {
                                skip_cell = true;
                                break;
                            }
                            if intersection > min_shade_no_diagonal {
                                single_diagonal = false;
                            }

                            if intersection > 0.0 {
                                let current = match edge_type {
                                    EdgeType::Top => &mut intersections.top,
                                    EdgeType::Bottom => &mut intersections.bottom,
                                    EdgeType::Left => &mut intersections.left,
                                    EdgeType::Right => &mut intersections.right,
                                    EdgeType::None => continue,
                                    EdgeType::LeftDiagonal | EdgeType::RightDiagonal => continue,
                                };
                                if current.is_none_or(|existing| intersection > existing) {
                                    *current = Some(intersection);
                                }
                            }
                        }
                    }
                }

                if skip_cell {
                    continue;
                }

                if intersections
                    .diagonal
                    .is_none_or(|diagonal| diagonal < 1.0 - frame.max_shade_no_matrix)
                {
                    let top_intersection = intersections.top.unwrap_or(0.0).max(0.0);
                    let bottom_intersection = intersections.bottom.unwrap_or(0.0).max(0.0);
                    let left_intersection = intersections.left.unwrap_or(0.0).max(0.0);
                    let right_intersection = intersections.right.unwrap_or(0.0).max(0.0);

                    let mut is_vertically_shifted =
                        intersections.top.is_some() || intersections.bottom.is_some();
                    let vertical_shade = 1.0 - top_intersection - bottom_intersection;
                    let mut is_horizontally_shifted =
                        intersections.left.is_some() || intersections.right.is_some();
                    let horizontal_shade = 1.0 - left_intersection - right_intersection;

                    if is_vertically_shifted && is_horizontally_shifted {
                        if vertical_shade < frame.max_shade_no_matrix
                            && horizontal_shade < frame.max_shade_no_matrix
                        {
                            is_vertically_shifted = false;
                            is_horizontally_shifted = false;
                        } else if 2.0 * (1.0 - vertical_shade) > (1.0 - horizontal_shade) {
                            is_horizontally_shifted = false;
                        } else {
                            is_vertically_shifted = false;
                        }
                    }

                    if is_vertically_shifted {
                        let shade = horizontal_shade
                            * compute_gradient_shade(row as f64 + 0.5, col as f64 + 0.5, frame);
                        draw_vertically_shifted_sub_block(
                            &mut resources,
                            namespace_id,
                            frame,
                            bulge_above,
                            row as f64 + top_intersection,
                            row as f64 + 1.0 - bottom_intersection,
                            col,
                            shade,
                        )?;
                        continue;
                    }

                    if is_horizontally_shifted {
                        if 1.0 - right_intersection <= 1.0 / 8.0
                            && row == target_row
                            && (col == target_col || col == target_col + 1)
                        {
                            continue;
                        }

                        let shade = vertical_shade
                            * compute_gradient_shade(row as f64 + 0.5, col as f64 + 0.5, frame);
                        draw_horizontally_shifted_sub_block(
                            &mut resources,
                            namespace_id,
                            frame,
                            bulge_above,
                            row,
                            col as f64 + left_intersection,
                            col as f64 + 1.0 - right_intersection,
                            shade,
                        )?;
                        continue;
                    }
                }

                let gradient_shade =
                    compute_gradient_shade(row as f64 + 0.5, col as f64 + 0.5, frame);
                if single_diagonal
                    && frame.use_diagonal_blocks
                    && frame.legacy_computing_symbols_support
                    && diagonal_edge_index.is_some()
                {
                    let has_drawn = draw_diagonal_block(
                        &mut resources,
                        namespace_id,
                        frame,
                        &geometry,
                        diagonal_edge_index.unwrap_or(0),
                        row,
                        col,
                        gradient_shade,
                    )?;
                    if has_drawn {
                        continue;
                    }
                }

                let mut matrix = [[1.0_f64; 2]; 2];
                for edge_index in 0..4 {
                    for fraction_index in 0..2 {
                        update_matrix_with_edge(
                            edge_index,
                            fraction_index,
                            row,
                            col,
                            &geometry,
                            &mut matrix,
                        );
                    }
                }

                draw_matrix_character(
                    &mut resources,
                    namespace_id,
                    frame,
                    row,
                    col,
                    matrix,
                    gradient_shade,
                )?;
            }
        }
    }

    Ok(())
}
