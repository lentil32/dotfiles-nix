use crate::types::RenderFrame;
use nvim_oxi::Result;
use nvim_oxi::api;
use nvim_oxi::api::opts::{GetHighlightOpts, SetHighlightOpts};
use nvim_oxi::api::types::GetHlInfos;
use nvim_oxi::{Array, Dictionary, Object};
use nvim_utils::mode::is_insert_like_mode;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

const DEFAULT_CURSOR_COLOR: u32 = 0x00D0_D0D0;
const DEFAULT_BACKGROUND_COLOR: u32 = 0x0030_3030;
const HIGHLIGHT_GROUP_NAME_CACHE_MAX_ENTRIES: usize = 16;

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
pub(crate) struct HighlightGroupNames {
    pub(crate) normal: Arc<[String]>,
    pub(crate) inverted: Arc<[String]>,
}

#[derive(Debug, Default)]
struct PaletteState {
    palette_key: Option<HighlightPaletteKey>,
    group_name_cache: HashMap<u32, HighlightGroupNames>,
}

#[derive(Debug)]
struct PaletteContext {
    state: Mutex<PaletteState>,
}

impl PaletteContext {
    fn new() -> Self {
        Self {
            state: Mutex::new(PaletteState {
                palette_key: None,
                group_name_cache: HashMap::with_capacity(HIGHLIGHT_GROUP_NAME_CACHE_MAX_ENTRIES),
            }),
        }
    }
}

static PALETTE_CONTEXT: LazyLock<PaletteContext> = LazyLock::new(PaletteContext::new);

fn state_lock() -> std::sync::MutexGuard<'static, PaletteState> {
    loop {
        match PALETTE_CONTEXT.state.lock() {
            Ok(guard) => return guard,
            Err(poisoned) => {
                let mut guard = poisoned.into_inner();
                *guard = PaletteState::default();
                drop(guard);
                PALETTE_CONTEXT.state.clear_poison();
            }
        }
    }
}

pub(crate) fn clear_highlight_cache() {
    let mut state = state_lock();
    state.palette_key = None;
    state.group_name_cache.clear();
}

fn hl_group_name(level: u32) -> String {
    format!("SmearCursor{level}")
}

fn inverted_hl_group_name(level: u32) -> String {
    format!("SmearCursorInverted{level}")
}

pub(crate) fn highlight_group_names(color_levels: u32) -> HighlightGroupNames {
    let levels = color_levels.max(1);
    {
        let state = state_lock();
        if let Some(cached) = state.group_name_cache.get(&levels) {
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

    let mut state = state_lock();
    if !state.group_name_cache.contains_key(&levels)
        && state.group_name_cache.len() >= HIGHLIGHT_GROUP_NAME_CACHE_MAX_ENTRIES
    {
        state.group_name_cache.clear();
    }
    state.group_name_cache.insert(levels, names.clone());
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

pub(crate) fn ensure_highlight_palette(frame: &RenderFrame) -> Result<()> {
    let color_levels = frame.color_levels.max(1);
    let gamma = frame.gamma;
    let cursor_color = resolve_mode_cursor_color(frame);
    let normal_background = resolve_normal_background(frame);
    let transparent_fallback = resolve_transparent_fallback(frame);
    let interpolation_background = normal_background.unwrap_or(transparent_fallback);
    let non_inverted_blend = 0;
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
        let state = state_lock();
        if state
            .palette_key
            .as_ref()
            .is_some_and(|cached| cached == &palette_key)
        {
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

    let mut state = state_lock();
    state.palette_key = Some(palette_key);
    Ok(())
}
