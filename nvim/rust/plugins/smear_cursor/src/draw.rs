use crate::types::{Particle, Point};
use nvim_oxi::Result;
use nvim_oxi::api;
use nvim_oxi::api::opts::{GetHighlightOpts, OptionOpts, OptionScope, SetHighlightOpts};
use nvim_oxi::api::types::{GetHlInfos, WindowConfig, WindowRelativeTo, WindowStyle};
use nvim_oxi::{Array, Dictionary, Object};
use nvim_oxi_utils::handles;
use nvim_utils::mode::is_insert_like_mode;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

mod render;

pub(crate) use render::{draw_current, draw_target_hack_block};

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
const HIGHLIGHT_GROUP_NAME_CACHE_MAX_ENTRIES: usize = 16;

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
    pub(crate) max_kept_windows: usize,
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

#[derive(Clone, Debug)]
struct HighlightGroupNames {
    normal: Arc<[String]>,
    inverted: Arc<[String]>,
}

#[derive(Clone, Copy, Debug)]
struct WindowBufferHandle {
    window_id: i32,
    buffer_id: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct WindowPlacement {
    row: i64,
    col: i64,
    zindex: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CachedWindowPayload {
    character: String,
    hl_group: String,
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
    Available {
        last_used_epoch: FrameEpoch,
        visible: bool,
    },
    InUse {
        epoch: FrameEpoch,
    },
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
    placement: Option<WindowPlacement>,
}

impl CachedRenderWindow {
    fn new_in_use(
        handles: WindowBufferHandle,
        epoch: FrameEpoch,
        placement: WindowPlacement,
    ) -> Self {
        Self {
            handles,
            lifecycle: CachedWindowLifecycle::InUse { epoch },
            placement: Some(placement),
        }
    }

    fn is_available_for_reuse(self) -> bool {
        matches!(self.lifecycle, CachedWindowLifecycle::Available { .. })
    }

    fn should_hide(self) -> bool {
        matches!(
            self.lifecycle,
            CachedWindowLifecycle::Available { visible: true, .. }
        )
    }

    fn mark_hidden(&mut self) {
        if let CachedWindowLifecycle::Available {
            last_used_epoch,
            visible: true,
        } = self.lifecycle
        {
            self.lifecycle = CachedWindowLifecycle::Available {
                last_used_epoch,
                visible: false,
            };
        }
    }

    fn needs_reconfigure(self, placement: WindowPlacement) -> bool {
        let is_visible = match self.lifecycle {
            CachedWindowLifecycle::Available { visible, .. } => visible,
            CachedWindowLifecycle::InUse { .. } => true,
        };
        !is_visible || self.placement != Some(placement)
    }

    fn set_placement(&mut self, placement: WindowPlacement) {
        self.placement = Some(placement);
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
                    visible: true,
                };
                EpochRollover::ReleasedForReuse
            }
            CachedWindowLifecycle::InUse { epoch } => {
                self.lifecycle = CachedWindowLifecycle::Available {
                    last_used_epoch: epoch,
                    visible: true,
                };
                EpochRollover::RecoveredStaleInUse
            }
        }
    }

    fn available_epoch(self) -> Option<FrameEpoch> {
        match self.lifecycle {
            CachedWindowLifecycle::Available {
                last_used_epoch, ..
            } => Some(last_used_epoch),
            CachedWindowLifecycle::InUse { .. } => None,
        }
    }
}

#[derive(Debug)]
struct TabWindows {
    current_epoch: FrameEpoch,
    frame_demand: usize,
    reuse_scan_index: usize,
    in_use_indices: Vec<usize>,
    visible_available_indices: Vec<usize>,
    windows: Vec<CachedRenderWindow>,
    payload_by_window: HashMap<i32, CachedWindowPayload>,
    ewma_demand_milli: u64,
    cached_budget: usize,
}

impl Default for TabWindows {
    fn default() -> Self {
        Self {
            current_epoch: FrameEpoch::ZERO,
            frame_demand: 0,
            reuse_scan_index: 0,
            in_use_indices: Vec::new(),
            visible_available_indices: Vec::new(),
            windows: Vec::new(),
            payload_by_window: HashMap::new(),
            ewma_demand_milli: 0,
            cached_budget: ADAPTIVE_POOL_MIN_BUDGET,
        }
    }
}

impl TabWindows {
    fn cached_payload_matches(&self, window_id: i32, character: &str, hl_group: &str) -> bool {
        self.payload_by_window
            .get(&window_id)
            .is_some_and(|payload| payload.character == character && payload.hl_group == hl_group)
    }

    fn cache_payload(&mut self, window_id: i32, character: &str, hl_group: &str) {
        if let Some(payload) = self.payload_by_window.get_mut(&window_id) {
            payload.character.clear();
            payload.character.push_str(character);
            payload.hl_group.clear();
            payload.hl_group.push_str(hl_group);
            return;
        }

        self.payload_by_window.insert(
            window_id,
            CachedWindowPayload {
                character: character.to_string(),
                hl_group: hl_group.to_string(),
            },
        );
    }

    fn clear_payload(&mut self, window_id: i32) {
        self.payload_by_window.remove(&window_id);
    }
}

#[derive(Debug)]
struct DrawState {
    tabs: HashMap<i32, TabWindows>,
    bulge_above: bool,
}

impl Default for DrawState {
    fn default() -> Self {
        Self {
            tabs: HashMap::with_capacity(4),
            bulge_above: false,
        }
    }
}

#[derive(Debug)]
struct DrawContext {
    draw_state: Mutex<DrawState>,
    highlight_palette_cache: Mutex<Option<HighlightPaletteKey>>,
    highlight_group_names_cache: Mutex<HashMap<u32, HighlightGroupNames>>,
}

impl DrawContext {
    fn new() -> Self {
        Self {
            draw_state: Mutex::new(DrawState::default()),
            highlight_palette_cache: Mutex::new(None),
            highlight_group_names_cache: Mutex::new(HashMap::with_capacity(
                HIGHLIGHT_GROUP_NAME_CACHE_MAX_ENTRIES,
            )),
        }
    }
}

static DRAW_CONTEXT: LazyLock<DrawContext> = LazyLock::new(DrawContext::new);

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
        match DRAW_CONTEXT.draw_state.lock() {
            Ok(guard) => return guard,
            Err(poisoned) => {
                let mut guard = poisoned.into_inner();
                *guard = DrawState::default();
                drop(guard);
                DRAW_CONTEXT.draw_state.clear_poison();
            }
        }
    }
}

fn palette_cache_lock() -> std::sync::MutexGuard<'static, Option<HighlightPaletteKey>> {
    loop {
        match DRAW_CONTEXT.highlight_palette_cache.lock() {
            Ok(guard) => return guard,
            Err(poisoned) => {
                let mut guard = poisoned.into_inner();
                *guard = None;
                drop(guard);
                DRAW_CONTEXT.highlight_palette_cache.clear_poison();
            }
        }
    }
}

fn group_name_cache_lock() -> std::sync::MutexGuard<'static, HashMap<u32, HighlightGroupNames>> {
    loop {
        match DRAW_CONTEXT.highlight_group_names_cache.lock() {
            Ok(guard) => return guard,
            Err(poisoned) => {
                let mut guard = poisoned.into_inner();
                guard.clear();
                drop(guard);
                DRAW_CONTEXT.highlight_group_names_cache.clear_poison();
            }
        }
    }
}

pub(crate) fn clear_highlight_cache() {
    let mut cache = palette_cache_lock();
    *cache = None;
    drop(cache);

    let mut name_cache = group_name_cache_lock();
    name_cache.clear();
}

fn hl_group_name(level: u32) -> String {
    format!("SmearCursor{level}")
}

fn inverted_hl_group_name(level: u32) -> String {
    format!("SmearCursorInverted{level}")
}

fn highlight_group_names(color_levels: u32) -> HighlightGroupNames {
    let levels = color_levels.max(1);
    {
        let cache = group_name_cache_lock();
        if let Some(cached) = cache.get(&levels) {
            return cached.clone();
        }
    }

    let normal: Arc<[String]> = Arc::from((0..=levels).map(hl_group_name).collect::<Vec<String>>());
    let inverted: Arc<[String]> = Arc::from(
        (0..=levels)
            .map(inverted_hl_group_name)
            .collect::<Vec<String>>(),
    );
    let names = HighlightGroupNames { normal, inverted };

    let mut cache = group_name_cache_lock();
    if !cache.contains_key(&levels) && cache.len() >= HIGHLIGHT_GROUP_NAME_CACHE_MAX_ENTRIES {
        cache.clear();
    }
    cache.insert(levels, names.clone());
    names
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
    let setting = if is_insert_like_mode(frame.mode.as_str()) {
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
    if cterm_fg.is_none() && cterm_bg.is_none() {
        let opts = SetHighlightOpts::builder()
            .foreground(foreground)
            .background(background)
            .blend(blend)
            .build();
        api::set_hl(0, group, &opts)?;
        return Ok(());
    }

    // nvim-oxi v0.6 SetHighlightOpts accepts cterm colors as string names, but this
    // code needs numeric terminal color indices. Keep the raw API call for parity.
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

    let group_names = highlight_group_names(color_levels);

    for level in 1..=color_levels {
        let opacity = (f64::from(level) / f64::from(color_levels)).powf(1.0 / gamma);
        let blended = interpolate_color(interpolation_background, cursor_color, opacity);
        let blended_hex = rgb_to_hex(blended);
        let inverted_foreground = rgb_to_hex(normal_background.unwrap_or(transparent_fallback));
        let cterm_level_color = cterm_color_at_level(cterm_cursor_colors.as_deref(), level);
        let level_index = usize::try_from(level).unwrap_or(0);
        let hl_group = group_names
            .normal
            .get(level_index)
            .map(String::as_str)
            .unwrap_or("SmearCursor1");
        let inverted_hl_group = group_names
            .inverted
            .get(level_index)
            .map(String::as_str)
            .unwrap_or("SmearCursorInverted1");

        set_highlight_group(
            hl_group,
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
            inverted_hl_group,
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
    match window.get_buf() {
        Ok(buffer) => Some(buffer),
        Err(err) => {
            log_draw_error("window.get_buf", &err);
            None
        }
    }
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

fn adjust_tracking_after_remove(tab_windows: &mut TabWindows, removed_index: usize) {
    if tab_windows.reuse_scan_index > removed_index {
        tab_windows.reuse_scan_index = tab_windows.reuse_scan_index.saturating_sub(1);
    }

    let mut write = 0;
    for read in 0..tab_windows.in_use_indices.len() {
        let index = tab_windows.in_use_indices[read];
        if index == removed_index {
            continue;
        }
        tab_windows.in_use_indices[write] = if index > removed_index {
            index.saturating_sub(1)
        } else {
            index
        };
        write += 1;
    }
    tab_windows.in_use_indices.truncate(write);

    let mut visible_write = 0;
    for read in 0..tab_windows.visible_available_indices.len() {
        let index = tab_windows.visible_available_indices[read];
        if index == removed_index {
            continue;
        }
        tab_windows.visible_available_indices[visible_write] = if index > removed_index {
            index.saturating_sub(1)
        } else {
            index
        };
        visible_write += 1;
    }
    tab_windows
        .visible_available_indices
        .truncate(visible_write);
}

fn remove_cached_window_at(tab_windows: &mut TabWindows, namespace_id: u32, remove_index: usize) {
    let cached = tab_windows.windows.remove(remove_index);
    adjust_tracking_after_remove(tab_windows, remove_index);
    tab_windows.clear_payload(cached.handles.window_id);
    close_cached_window(namespace_id, cached.handles);
}

fn rollover_in_use_windows(tab_windows: &mut TabWindows, previous_epoch: FrameEpoch) {
    let in_use_indices = std::mem::take(&mut tab_windows.in_use_indices);
    let mut visible_available_indices = Vec::with_capacity(in_use_indices.len());
    for index in in_use_indices {
        if let Some(cached) = tab_windows.windows.get_mut(index) {
            let rollover = cached.rollover_to_next_epoch(previous_epoch);
            if matches!(
                rollover,
                EpochRollover::ReleasedForReuse | EpochRollover::RecoveredStaleInUse
            ) {
                visible_available_indices.push(index);
            }
        }
    }
    tab_windows.visible_available_indices = visible_available_indices;
}

fn get_or_create_window(
    draw_state: &mut DrawState,
    namespace_id: u32,
    tab_handle: i32,
    row: i64,
    col: i64,
    zindex: u32,
) -> Result<(i32, api::Buffer)> {
    let requested_placement = WindowPlacement { row, col, zindex };
    let tab_windows = draw_state.tabs.entry(tab_handle).or_default();
    while tab_windows.reuse_scan_index < tab_windows.windows.len() {
        let index = tab_windows.reuse_scan_index;
        tab_windows.reuse_scan_index = tab_windows.reuse_scan_index.saturating_add(1);

        let cached = tab_windows.windows[index];
        if !cached.is_available_for_reuse() {
            continue;
        }

        let maybe_window = window_from_handle_i32(cached.handles.window_id);
        if let Some(mut window) = maybe_window {
            if cached.needs_reconfigure(requested_placement) {
                let config = float_window_config(row, col, zindex);
                if window.set_config(&config).is_err() {
                    remove_cached_window_at(tab_windows, namespace_id, index);
                    continue;
                }
            }

            if let Some(buffer) = buffer_from_handle_i32(cached.handles.buffer_id)
                && tab_windows.windows[index].mark_in_use(tab_windows.current_epoch)
            {
                tab_windows.windows[index].set_placement(requested_placement);
                tab_windows.frame_demand = tab_windows.frame_demand.saturating_add(1);
                tab_windows.in_use_indices.push(index);
                return Ok((cached.handles.window_id, buffer));
            }
        }

        remove_cached_window_at(tab_windows, namespace_id, index);
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
        requested_placement,
    ));
    tab_windows
        .in_use_indices
        .push(tab_windows.windows.len().saturating_sub(1));
    tab_windows.frame_demand = tab_windows.frame_demand.saturating_add(1);
    tab_windows.reuse_scan_index = tab_windows.windows.len();

    Ok((handles.window_id, buffer))
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

fn effective_keep_budget(adaptive_budget: usize, max_kept_windows: usize) -> usize {
    adaptive_budget.min(max_kept_windows)
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
    let mut remove_indices: Vec<usize> = if remove_count == ordered.len() {
        ordered.into_iter().map(|(index, _)| index).collect()
    } else {
        let (remove_slice, _, _) = ordered.select_nth_unstable_by(remove_count, |lhs, rhs| {
            lhs.1.cmp(&rhs.1).then(lhs.0.cmp(&rhs.0))
        });
        remove_slice.iter().map(|(index, _)| *index).collect()
    };
    remove_indices.sort_unstable();
    remove_indices
}

fn clear_cached_windows(draw_state: &mut DrawState, namespace_id: u32, max_kept_windows: usize) {
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
        rollover_in_use_windows(tab_windows, previous_epoch);

        debug_assert!(
            tab_windows.in_use_indices.is_empty(),
            "in-use index set must be empty after rollover"
        );
        debug_assert!(
            tab_windows
                .windows
                .iter()
                .all(|cached| cached.is_available_for_reuse()),
            "cached render windows must be available after epoch rollover"
        );

        let keep_budget = effective_keep_budget(tab_windows.cached_budget, max_kept_windows);
        if tab_windows.windows.len() > keep_budget {
            let remove_indices = lru_prune_indices(&tab_windows.windows, keep_budget);
            if remove_indices.is_empty() {
                continue;
            }
            // Match upstream Lua behavior: close extra windows with events
            // ignored to avoid incidental side effects.
            let _event_ignore = EventIgnoreGuard::set_all();
            for remove_index in remove_indices.into_iter().rev() {
                remove_cached_window_at(tab_windows, namespace_id, remove_index);
            }
        }
    }
}

fn hide_available_windows(draw_state: &mut DrawState, namespace_id: u32) {
    let hide_config = hide_window_config();
    for tab_windows in draw_state.tabs.values_mut() {
        let mut hide_indices = std::mem::take(&mut tab_windows.visible_available_indices);
        if hide_indices.is_empty() {
            continue;
        }
        hide_indices.sort_unstable();
        hide_indices.dedup();

        for index in hide_indices.into_iter().rev() {
            if index >= tab_windows.windows.len() {
                continue;
            }
            if !tab_windows.windows[index].should_hide() {
                continue;
            }

            let handles = tab_windows.windows[index].handles;
            let Some(mut window) = window_from_handle_i32(handles.window_id) else {
                remove_cached_window_at(tab_windows, namespace_id, index);
                continue;
            };

            if window.set_config(&hide_config).is_err() {
                remove_cached_window_at(tab_windows, namespace_id, index);
                continue;
            }

            tab_windows.windows[index].mark_hidden();
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

pub(crate) fn clear_active_render_windows(namespace_id: u32, max_kept_windows: usize) {
    let mut draw_state = draw_state_lock();
    clear_cached_windows(&mut draw_state, namespace_id, max_kept_windows);
    hide_available_windows(&mut draw_state, namespace_id);
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
