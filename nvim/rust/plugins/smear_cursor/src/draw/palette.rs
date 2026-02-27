use super::render_plan::HighlightLevel;
use crate::core::realization::PaletteSpec;
use crate::events::{schedule_guarded, warn};
#[cfg(test)]
use crate::types::RenderFrame;
use nvim_oxi::Result;
use nvim_oxi::api;
use nvim_oxi::api::opts::{GetHighlightOpts, SetHighlightOpts};
use nvim_oxi::api::types::GetHlInfos;
use nvim_oxi::{Array, Dictionary, Object};
use nvim_utils::mode::is_insert_like_mode;
use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RawPaletteInputKey {
    fingerprint: u64,
}

#[derive(Clone, Copy, Debug)]
struct ResolvedPalette<'a> {
    cursor_color: u32,
    normal_background: Option<u32>,
    transparent_fallback: u32,
    non_inverted_blend: u8,
    color_levels: u32,
    gamma_bits: u64,
    cterm_cursor_colors: Option<&'a [u16]>,
    cterm_bg: Option<u16>,
}

#[derive(Clone, Debug)]
pub(crate) struct HighlightGroupNames {
    pub(crate) normal: Arc<[String]>,
    pub(crate) inverted: Arc<[String]>,
}

impl HighlightGroupNames {
    pub(crate) fn normal_name(&self, level: HighlightLevel) -> &str {
        let level_index = level.index_for_len(self.normal.len());
        self.normal
            .get(level_index)
            .map_or("SmearCursor1", String::as_str)
    }

    pub(crate) fn inverted_name(&self, level: HighlightLevel) -> &str {
        let level_index = level.index_for_len(self.inverted.len());
        self.inverted
            .get(level_index)
            .map_or("SmearCursorInverted1", String::as_str)
    }
}

#[derive(Debug, Default)]
struct PaletteState {
    raw_input_key: Option<RawPaletteInputKey>,
    palette_key: Option<HighlightPaletteKey>,
    pending_refresh_key: Option<RawPaletteInputKey>,
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
                raw_input_key: None,
                palette_key: None,
                pending_refresh_key: None,
                group_name_cache: HashMap::with_capacity(HIGHLIGHT_GROUP_NAME_CACHE_MAX_ENTRIES),
            }),
        }
    }
}

static PALETTE_CONTEXT: LazyLock<PaletteContext> = LazyLock::new(PaletteContext::new);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PaletteCacheLookup {
    RawHit,
    ResolvedHit,
    Miss,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PaletteRefreshDisposition {
    Ready,
    RefreshAlreadyPending,
    BootstrapSynchronously,
    ScheduleDeferred,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PaletteRefreshOutcome {
    SkippedStale,
    ReusedCommitted,
    AppliedHighlights,
}

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
    state.raw_input_key = None;
    state.palette_key = None;
    state.pending_refresh_key = None;
    state.group_name_cache.clear();
}

fn lookup_cached_palette(
    state: &PaletteState,
    raw_input_key: RawPaletteInputKey,
    resolved_palette: Option<&ResolvedPalette<'_>>,
) -> PaletteCacheLookup {
    if state.raw_input_key == Some(raw_input_key) {
        return PaletteCacheLookup::RawHit;
    }

    if let Some(resolved_palette) = resolved_palette
        && state
            .palette_key
            .as_ref()
            .is_some_and(|cached| resolved_palette_matches(cached, resolved_palette))
    {
        return PaletteCacheLookup::ResolvedHit;
    }

    PaletteCacheLookup::Miss
}

fn stage_palette_refresh(
    state: &mut PaletteState,
    raw_input_key: RawPaletteInputKey,
) -> PaletteRefreshDisposition {
    if state.raw_input_key == Some(raw_input_key) {
        return PaletteRefreshDisposition::Ready;
    }

    if state.pending_refresh_key == Some(raw_input_key) {
        return PaletteRefreshDisposition::RefreshAlreadyPending;
    }

    state.pending_refresh_key = Some(raw_input_key);
    if state.palette_key.is_some() {
        PaletteRefreshDisposition::ScheduleDeferred
    } else {
        PaletteRefreshDisposition::BootstrapSynchronously
    }
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

fn resolve_mode_cursor_color_for_spec(spec: &PaletteSpec) -> u32 {
    let setting = if is_insert_like_mode(spec.mode()) {
        spec.cursor_color_insert_mode()
    } else {
        spec.cursor_color()
    };

    let explicit_color =
        resolve_cursor_color_setting(setting).and_then(|resolved| match resolved {
            ResolvedCursorColor::Direct(color) => Some(color),
            ResolvedCursorColor::FromCursorText => spec.color_at_cursor().and_then(parse_hex_color),
        });

    explicit_color
        .or_else(|| highlight_color("Cursor", false))
        .or_else(|| highlight_color("Normal", true))
        .unwrap_or(DEFAULT_CURSOR_COLOR)
}

fn resolve_normal_background_for_spec(spec: &PaletteSpec) -> Option<u32> {
    match spec.normal_bg() {
        Some("none") => None,
        Some(value) => parse_hex_color(value).or_else(|| highlight_color(value, false)),
        None => highlight_color("Normal", false),
    }
}

fn resolve_transparent_fallback_for_spec(spec: &PaletteSpec) -> u32 {
    parse_hex_color(spec.transparent_bg_fallback_color()).unwrap_or(DEFAULT_BACKGROUND_COLOR)
}

fn effective_cursor_color_setting_for_spec(spec: &PaletteSpec) -> Option<&str> {
    if is_insert_like_mode(spec.mode()) {
        spec.cursor_color_insert_mode()
    } else {
        spec.cursor_color()
    }
}

fn cursor_color_depends_on_cursor_text(spec: &PaletteSpec) -> bool {
    matches!(effective_cursor_color_setting_for_spec(spec), Some("none"))
}

fn raw_palette_input_key_for_spec(spec: &PaletteSpec) -> RawPaletteInputKey {
    let mut hasher = DefaultHasher::new();
    effective_cursor_color_setting_for_spec(spec).hash(&mut hasher);
    spec.normal_bg().hash(&mut hasher);
    spec.transparent_bg_fallback_color().hash(&mut hasher);
    spec.cterm_cursor_colors().hash(&mut hasher);
    spec.cterm_bg().hash(&mut hasher);
    spec.color_levels().hash(&mut hasher);
    spec.gamma_bits().hash(&mut hasher);
    if cursor_color_depends_on_cursor_text(spec) {
        spec.color_at_cursor().hash(&mut hasher);
    }
    RawPaletteInputKey {
        fingerprint: hasher.finish(),
    }
}

#[cfg(test)]
fn raw_palette_input_key(frame: &RenderFrame) -> RawPaletteInputKey {
    raw_palette_input_key_for_spec(&PaletteSpec::from_frame(frame))
}

fn resolved_palette_matches(cached: &HighlightPaletteKey, resolved: &ResolvedPalette<'_>) -> bool {
    cached.cursor_color == resolved.cursor_color
        && cached.normal_background == resolved.normal_background
        && cached.transparent_fallback == resolved.transparent_fallback
        && cached.non_inverted_blend == resolved.non_inverted_blend
        && cached.color_levels == resolved.color_levels
        && cached.gamma_bits == resolved.gamma_bits
        && cached.cterm_cursor_colors.as_deref() == resolved.cterm_cursor_colors
        && cached.cterm_bg == resolved.cterm_bg
}

impl ResolvedPalette<'_> {
    fn into_owned(self) -> HighlightPaletteKey {
        HighlightPaletteKey {
            cursor_color: self.cursor_color,
            normal_background: self.normal_background,
            transparent_fallback: self.transparent_fallback,
            non_inverted_blend: self.non_inverted_blend,
            color_levels: self.color_levels,
            gamma_bits: self.gamma_bits,
            cterm_cursor_colors: self.cterm_cursor_colors.map(<[u16]>::to_vec),
            cterm_bg: self.cterm_bg,
        }
    }
}

fn resolve_palette_for_spec(spec: &PaletteSpec) -> ResolvedPalette<'_> {
    let color_levels = spec.color_levels();
    let gamma = spec.gamma();
    let cursor_color = resolve_mode_cursor_color_for_spec(spec);
    let normal_background = resolve_normal_background_for_spec(spec);
    let transparent_fallback = resolve_transparent_fallback_for_spec(spec);
    let non_inverted_blend = 0;
    ResolvedPalette {
        cursor_color,
        normal_background,
        transparent_fallback,
        non_inverted_blend,
        color_levels,
        gamma_bits: gamma.to_bits(),
        cterm_cursor_colors: spec.cterm_cursor_colors(),
        cterm_bg: spec.cterm_bg(),
    }
}

fn clear_pending_palette_refresh(raw_input_key: RawPaletteInputKey) {
    let mut state = state_lock();
    if state.pending_refresh_key == Some(raw_input_key) {
        state.pending_refresh_key = None;
    }
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

fn refresh_highlight_palette_for_spec(
    spec: &PaletteSpec,
    raw_input_key: RawPaletteInputKey,
) -> Result<PaletteRefreshOutcome> {
    let resolved_palette = resolve_palette_for_spec(spec);
    {
        let mut state = state_lock();
        if state.pending_refresh_key != Some(raw_input_key)
            && state.raw_input_key != Some(raw_input_key)
        {
            return Ok(PaletteRefreshOutcome::SkippedStale);
        }

        match lookup_cached_palette(&state, raw_input_key, Some(&resolved_palette)) {
            PaletteCacheLookup::RawHit => {
                state.pending_refresh_key = None;
                return Ok(PaletteRefreshOutcome::ReusedCommitted);
            }
            PaletteCacheLookup::ResolvedHit => {
                state.raw_input_key = Some(raw_input_key);
                state.pending_refresh_key = None;
                return Ok(PaletteRefreshOutcome::ReusedCommitted);
            }
            PaletteCacheLookup::Miss => {}
        }
    }

    let color_levels = resolved_palette.color_levels;
    let interpolation_background = resolved_palette
        .normal_background
        .unwrap_or(resolved_palette.transparent_fallback);
    let group_names = highlight_group_names(color_levels);

    for level in 1..=color_levels {
        let level_ref = HighlightLevel::from_raw_clamped(level);
        let opacity = (f64::from(level) / f64::from(color_levels)).powf(1.0 / spec.gamma());
        let blended = interpolate_color(
            interpolation_background,
            resolved_palette.cursor_color,
            opacity,
        );
        let blended_hex = rgb_to_hex(blended);
        let inverted_foreground = rgb_to_hex(
            resolved_palette
                .normal_background
                .unwrap_or(resolved_palette.transparent_fallback),
        );
        let cterm_level_color = cterm_color_at_level(resolved_palette.cterm_cursor_colors, level);
        let hl_group = group_names.normal_name(level_ref);
        let inverted_hl_group = group_names.inverted_name(level_ref);

        set_highlight_group(
            hl_group,
            blended_hex.as_str(),
            "none",
            resolved_palette.non_inverted_blend,
            cterm_level_color,
            None,
        )?;

        let inverted_ctermfg = spec.cterm_bg().or_else(|| {
            resolved_palette
                .cterm_cursor_colors
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
    state.raw_input_key = Some(raw_input_key);
    state.palette_key = Some(resolved_palette.into_owned());
    state.pending_refresh_key = None;
    Ok(PaletteRefreshOutcome::AppliedHighlights)
}

fn defer_palette_refresh(spec: PaletteSpec, raw_input_key: RawPaletteInputKey) {
    schedule_guarded(
        "palette_refresh",
        move || match refresh_highlight_palette_for_spec(&spec, raw_input_key) {
            Ok(PaletteRefreshOutcome::AppliedHighlights) => {
                if let Err(err) = super::redraw() {
                    warn(&format!("palette refresh redraw failed: {err}"));
                }
            }
            Ok(PaletteRefreshOutcome::ReusedCommitted | PaletteRefreshOutcome::SkippedStale) => {}
            Err(err) => {
                clear_pending_palette_refresh(raw_input_key);
                warn(&format!("palette refresh failed: {err}"));
            }
        },
    );
}

pub(crate) fn ensure_highlight_palette_for_spec(spec: &PaletteSpec) -> Result<()> {
    let raw_input_key = raw_palette_input_key_for_spec(spec);
    let disposition = {
        let mut state = state_lock();
        stage_palette_refresh(&mut state, raw_input_key)
    };

    match disposition {
        PaletteRefreshDisposition::Ready | PaletteRefreshDisposition::RefreshAlreadyPending => {
            Ok(())
        }
        PaletteRefreshDisposition::BootstrapSynchronously => {
            // Comment: first draw has no committed smear highlight groups yet, so keep a one-time
            // synchronous bootstrap until palette refresh can be primed earlier in the lifecycle.
            refresh_highlight_palette_for_spec(spec, raw_input_key)?;
            Ok(())
        }
        PaletteRefreshDisposition::ScheduleDeferred => {
            defer_palette_refresh(spec.clone(), raw_input_key);
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::StrokeId;
    use crate::types::{Point, RenderFrame, StaticRenderConfig};

    fn test_frame() -> RenderFrame {
        RenderFrame {
            mode: "n".to_string(),
            corners: [Point::ZERO; 4],
            step_samples: Vec::new(),
            planner_idle_steps: 0,
            target: Point::ZERO,
            target_corners: [Point::ZERO; 4],
            vertical_bar: false,
            trail_stroke_id: StrokeId::INITIAL,
            retarget_epoch: 0,
            particles: Vec::new(),
            color_at_cursor: Some("#ffffff".to_string()),
            static_config: Arc::new(StaticRenderConfig {
                cursor_color: Some("#112233".to_string()),
                cursor_color_insert_mode: Some("none".to_string()),
                normal_bg: Some("#202020".to_string()),
                transparent_bg_fallback_color: "#303030".to_string(),
                cterm_cursor_colors: Some(vec![17_u16, 42_u16]),
                cterm_bg: Some(235_u16),
                hide_target_hack: false,
                max_kept_windows: 32,
                never_draw_over_target: false,
                particle_max_lifetime: 250.0,
                particle_switch_octant_braille: 0.5,
                particles_over_text: true,
                color_levels: 16,
                gamma: 2.2,
                block_aspect_ratio: 0.5,
                tail_duration_ms: 120.0,
                simulation_hz: 120.0,
                trail_thickness: 1.0,
                trail_thickness_x: 1.0,
                spatial_coherence_weight: 0.0,
                temporal_stability_weight: 0.0,
                top_k_per_cell: 4,
                windows_zindex: 50,
            }),
        }
    }

    #[test]
    fn raw_palette_input_key_uses_mode_specific_cursor_setting() {
        let mut normal = test_frame();
        let mut insert = test_frame();
        insert.mode = "i".to_string();

        assert_ne!(
            raw_palette_input_key(&normal),
            raw_palette_input_key(&insert)
        );

        let mut config = (*normal.static_config).clone();
        config.cursor_color = Some("none".to_string());
        normal.static_config = Arc::new(config);
        assert_eq!(
            raw_palette_input_key(&normal),
            raw_palette_input_key(&insert)
        );
    }

    #[test]
    fn raw_palette_input_key_ignores_cursor_text_when_effective_cursor_color_is_direct() {
        let direct = test_frame();
        let mut changed_cursor_text = test_frame();
        changed_cursor_text.color_at_cursor = Some("#abcdef".to_string());

        assert_eq!(
            raw_palette_input_key(&direct),
            raw_palette_input_key(&changed_cursor_text)
        );
    }

    #[test]
    fn raw_palette_input_key_uses_cursor_text_when_effective_cursor_color_is_none() {
        let mut frame = test_frame();
        let mut config = (*frame.static_config).clone();
        config.cursor_color = Some("none".to_string());
        frame.static_config = Arc::new(config);

        let mut changed_cursor_text = frame.clone();
        changed_cursor_text.color_at_cursor = Some("#abcdef".to_string());

        assert_ne!(
            raw_palette_input_key(&frame),
            raw_palette_input_key(&changed_cursor_text)
        );
    }

    #[test]
    fn resolved_palette_match_uses_borrowed_cterm_colors() {
        let resolved = ResolvedPalette {
            cursor_color: 0x112233,
            normal_background: Some(0x202020),
            transparent_fallback: 0x303030,
            non_inverted_blend: 0,
            color_levels: 16,
            gamma_bits: 2.2_f64.to_bits(),
            cterm_cursor_colors: Some(&[17_u16, 42_u16]),
            cterm_bg: Some(235_u16),
        };
        let cached = resolved.into_owned();

        assert!(resolved_palette_matches(&cached, &resolved));
    }

    #[test]
    fn lookup_cached_palette_distinguishes_raw_and_resolved_hits() {
        let raw_key = RawPaletteInputKey { fingerprint: 11 };
        let resolved = ResolvedPalette {
            cursor_color: 0x112233,
            normal_background: Some(0x202020),
            transparent_fallback: 0x303030,
            non_inverted_blend: 0,
            color_levels: 16,
            gamma_bits: 2.2_f64.to_bits(),
            cterm_cursor_colors: Some(&[17_u16, 42_u16]),
            cterm_bg: Some(235_u16),
        };
        let cached_key = resolved.into_owned();

        assert_eq!(
            lookup_cached_palette(
                &PaletteState {
                    raw_input_key: Some(raw_key),
                    palette_key: Some(cached_key.clone()),
                    pending_refresh_key: None,
                    group_name_cache: HashMap::new(),
                },
                raw_key,
                None,
            ),
            PaletteCacheLookup::RawHit
        );
        assert_eq!(
            lookup_cached_palette(
                &PaletteState {
                    raw_input_key: Some(RawPaletteInputKey { fingerprint: 12 }),
                    palette_key: Some(cached_key),
                    pending_refresh_key: None,
                    group_name_cache: HashMap::new(),
                },
                raw_key,
                Some(&resolved),
            ),
            PaletteCacheLookup::ResolvedHit
        );
    }

    #[test]
    fn clear_highlight_cache_resets_raw_and_resolved_keys() {
        clear_highlight_cache();
        {
            let mut state = state_lock();
            state.raw_input_key = Some(RawPaletteInputKey { fingerprint: 7 });
            state.palette_key = Some(HighlightPaletteKey {
                cursor_color: 0x112233,
                normal_background: Some(0x202020),
                transparent_fallback: 0x303030,
                non_inverted_blend: 0,
                color_levels: 16,
                gamma_bits: 2.2_f64.to_bits(),
                cterm_cursor_colors: Some(vec![17_u16, 42_u16]),
                cterm_bg: Some(235_u16),
            });
            state.pending_refresh_key = Some(RawPaletteInputKey { fingerprint: 9 });
        }

        clear_highlight_cache();

        let state = state_lock();
        assert_eq!(state.raw_input_key, None);
        assert_eq!(state.palette_key, None);
        assert_eq!(state.pending_refresh_key, None);
    }

    #[test]
    fn stage_palette_refresh_bootstraps_without_committed_palette() {
        let raw_key = RawPaletteInputKey { fingerprint: 3 };
        let mut state = PaletteState::default();

        assert_eq!(
            stage_palette_refresh(&mut state, raw_key),
            PaletteRefreshDisposition::BootstrapSynchronously
        );
        assert_eq!(state.pending_refresh_key, Some(raw_key));
    }

    #[test]
    fn stage_palette_refresh_defers_when_committed_palette_exists() {
        let raw_key = RawPaletteInputKey { fingerprint: 5 };
        let mut state = PaletteState {
            raw_input_key: Some(RawPaletteInputKey { fingerprint: 4 }),
            palette_key: Some(HighlightPaletteKey {
                cursor_color: 0x112233,
                normal_background: Some(0x202020),
                transparent_fallback: 0x303030,
                non_inverted_blend: 0,
                color_levels: 16,
                gamma_bits: 2.2_f64.to_bits(),
                cterm_cursor_colors: Some(vec![17_u16, 42_u16]),
                cterm_bg: Some(235_u16),
            }),
            pending_refresh_key: None,
            group_name_cache: HashMap::new(),
        };

        assert_eq!(
            stage_palette_refresh(&mut state, raw_key),
            PaletteRefreshDisposition::ScheduleDeferred
        );
        assert_eq!(state.pending_refresh_key, Some(raw_key));
    }

    #[test]
    fn stage_palette_refresh_deduplicates_matching_pending_request() {
        let raw_key = RawPaletteInputKey { fingerprint: 6 };
        let mut state = PaletteState {
            raw_input_key: Some(RawPaletteInputKey { fingerprint: 4 }),
            palette_key: Some(HighlightPaletteKey {
                cursor_color: 0x112233,
                normal_background: Some(0x202020),
                transparent_fallback: 0x303030,
                non_inverted_blend: 0,
                color_levels: 16,
                gamma_bits: 2.2_f64.to_bits(),
                cterm_cursor_colors: Some(vec![17_u16, 42_u16]),
                cterm_bg: Some(235_u16),
            }),
            pending_refresh_key: Some(raw_key),
            group_name_cache: HashMap::new(),
        };

        assert_eq!(
            stage_palette_refresh(&mut state, raw_key),
            PaletteRefreshDisposition::RefreshAlreadyPending
        );
        assert_eq!(state.pending_refresh_key, Some(raw_key));
    }
}
