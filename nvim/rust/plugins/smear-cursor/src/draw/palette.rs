use super::render_plan::HighlightLevel;
use crate::config::normalize_color_levels;
use crate::core::realization::PaletteSpec;
use crate::events::schedule_guarded;
use crate::events::warn;
#[cfg(test)]
use crate::types::RenderFrame;
use nvim_oxi::Array;
use nvim_oxi::Dictionary;
use nvim_oxi::Object;
use nvim_oxi::Result;
use nvim_oxi::api;
use nvim_oxi::api::opts::GetHighlightOpts;
use nvim_oxi::api::opts::SetHighlightOpts;
use nvim_oxi::api::types::GetHlInfos;
use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::Hash;
use std::hash::Hasher;
use std::panic::AssertUnwindSafe;
use std::panic::catch_unwind;
use std::panic::resume_unwind;
use std::sync::Arc;

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

impl PaletteState {
    fn new() -> Self {
        Self {
            raw_input_key: None,
            palette_key: None,
            pending_refresh_key: None,
            group_name_cache: HashMap::with_capacity(HIGHLIGHT_GROUP_NAME_CACHE_MAX_ENTRIES),
        }
    }
}

thread_local! {
    static PALETTE_STATE: RefCell<PaletteState> = RefCell::new(PaletteState::new());
}

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

fn with_palette_state<R>(reader: impl FnOnce(&PaletteState) -> R) -> R {
    PALETTE_STATE.with(|state| {
        let state = state.borrow();
        reader(&state)
    })
}

fn with_palette_state_mut<R>(mutator: impl FnOnce(&mut PaletteState) -> R) -> R {
    PALETTE_STATE.with(|state| {
        // Keep palette mutators cache-local. Shell calls and logging must happen after this
        // borrow is released so palette refresh cannot self-reenter through the RefCell.
        let mut state = state.borrow_mut();
        match catch_unwind(AssertUnwindSafe(|| mutator(&mut state))) {
            Ok(output) => output,
            Err(panic_payload) => {
                *state = PaletteState::new();
                resume_unwind(panic_payload);
            }
        }
    })
}

pub(crate) fn clear_highlight_cache() {
    let stale_groups = with_palette_state_mut(|state| {
        let stale_groups = stale_highlight_group_names(
            state
                .palette_key
                .as_ref()
                .map(|palette| palette.color_levels),
            None,
        );
        state.raw_input_key = None;
        state.palette_key = None;
        state.pending_refresh_key = None;
        state.group_name_cache.clear();
        stale_groups
    });

    if let Err(err) = clear_highlight_groups(stale_groups.iter().map(String::as_str)) {
        warn(&format!("palette cache clear failed: {err}"));
    }
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

fn stale_highlight_group_names(
    previous_levels: Option<u32>,
    retained_levels: Option<u32>,
) -> Vec<String> {
    let Some(previous_levels) = previous_levels.map(normalize_color_levels) else {
        return Vec::new();
    };
    let retained_levels = retained_levels.map(normalize_color_levels).unwrap_or(0);
    if previous_levels <= retained_levels {
        return Vec::new();
    }

    let stale_capacity = usize::try_from(previous_levels.saturating_sub(retained_levels))
        .unwrap_or(0)
        .saturating_mul(2);
    let mut names = Vec::with_capacity(stale_capacity);
    for level in retained_levels.saturating_add(1)..=previous_levels {
        names.push(hl_group_name(level));
        names.push(inverted_hl_group_name(level));
    }
    names
}

pub(crate) fn highlight_group_names(color_levels: u32) -> HighlightGroupNames {
    let levels = normalize_color_levels(color_levels);
    if let Some(cached) = with_palette_state(|state| state.group_name_cache.get(&levels).cloned()) {
        return cached;
    }

    let normal: Arc<[String]> = Arc::from((0..=levels).map(hl_group_name).collect::<Vec<String>>());
    let inverted: Arc<[String]> = Arc::from(
        (0..=levels)
            .map(inverted_hl_group_name)
            .collect::<Vec<String>>(),
    );
    let names = HighlightGroupNames { normal, inverted };

    with_palette_state_mut(|state| {
        if !state.group_name_cache.contains_key(&levels)
            && state.group_name_cache.len() >= HIGHLIGHT_GROUP_NAME_CACHE_MAX_ENTRIES
        {
            state.group_name_cache.clear();
        }
        state.group_name_cache.insert(levels, names.clone());
    });
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
    let setting = if spec.mode().is_insert_like() {
        spec.cursor_color_insert_mode()
    } else {
        spec.cursor_color()
    };

    let explicit_color =
        resolve_cursor_color_setting(setting).and_then(|resolved| match resolved {
            ResolvedCursorColor::Direct(color) => Some(color),
            ResolvedCursorColor::FromCursorText => spec.color_at_cursor(),
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
    if spec.mode().is_insert_like() {
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
    with_palette_state_mut(|state| {
        if state.pending_refresh_key == Some(raw_input_key) {
            state.pending_refresh_key = None;
        }
    });
}

fn cterm_color_at_level(cterm_cursor_colors: Option<&[u16]>, level: u32) -> Option<u16> {
    let colors = cterm_cursor_colors?;
    let index = usize::try_from(level.saturating_sub(1)).ok()?;
    colors.get(index).copied()
}

#[cfg(not(test))]
fn clear_highlight_group(group: &str) -> Result<()> {
    let args = Array::from_iter([
        Object::from(0_i64),
        Object::from(group),
        Object::from(Dictionary::new()),
    ]);
    let _: Object = api::call_function("nvim_set_hl", args)?;
    Ok(())
}

#[cfg(not(test))]
fn clear_highlight_groups<'a>(groups: impl IntoIterator<Item = &'a str>) -> Result<()> {
    for group in groups {
        clear_highlight_group(group)?;
    }
    Ok(())
}

#[cfg(test)]
fn clear_highlight_groups<'a>(groups: impl IntoIterator<Item = &'a str>) -> Result<()> {
    let _ = groups.into_iter().count();
    Ok(())
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
    let previous_levels = with_palette_state(|state| {
        state
            .palette_key
            .as_ref()
            .map(|palette_key| palette_key.color_levels)
    });
    {
        let cache_lookup = with_palette_state_mut(|state| {
            if state.pending_refresh_key != Some(raw_input_key)
                && state.raw_input_key != Some(raw_input_key)
            {
                return PaletteRefreshOutcome::SkippedStale;
            }

            match lookup_cached_palette(state, raw_input_key, Some(&resolved_palette)) {
                PaletteCacheLookup::RawHit => {
                    state.pending_refresh_key = None;
                    PaletteRefreshOutcome::ReusedCommitted
                }
                PaletteCacheLookup::ResolvedHit => {
                    state.raw_input_key = Some(raw_input_key);
                    state.pending_refresh_key = None;
                    PaletteRefreshOutcome::ReusedCommitted
                }
                PaletteCacheLookup::Miss => PaletteRefreshOutcome::AppliedHighlights,
            }
        });
        match cache_lookup {
            PaletteRefreshOutcome::SkippedStale => {
                return Ok(PaletteRefreshOutcome::SkippedStale);
            }
            PaletteRefreshOutcome::ReusedCommitted => {
                return Ok(PaletteRefreshOutcome::ReusedCommitted);
            }
            PaletteRefreshOutcome::AppliedHighlights => {}
        }
    }

    let color_levels = resolved_palette.color_levels;
    let interpolation_background = resolved_palette
        .normal_background
        .unwrap_or(resolved_palette.transparent_fallback);
    let group_names = highlight_group_names(color_levels);
    let inverted_foreground = rgb_to_hex(
        resolved_palette
            .normal_background
            .unwrap_or(resolved_palette.transparent_fallback),
    );
    let inverted_ctermfg = spec.cterm_bg().or_else(|| {
        resolved_palette
            .cterm_cursor_colors
            .and_then(|colors| colors.first().copied())
    });

    for level in 1..=color_levels {
        let level_ref = HighlightLevel::from_raw_clamped(level);
        let opacity = (f64::from(level) / f64::from(color_levels)).powf(1.0 / spec.gamma());
        let blended = interpolate_color(
            interpolation_background,
            resolved_palette.cursor_color,
            opacity,
        );
        let blended_hex = rgb_to_hex(blended);
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

        set_highlight_group(
            inverted_hl_group,
            inverted_foreground.as_str(),
            blended_hex.as_str(),
            0,
            inverted_ctermfg,
            cterm_level_color,
        )?;
    }
    let stale_groups = stale_highlight_group_names(previous_levels, Some(color_levels));
    clear_highlight_groups(stale_groups.iter().map(String::as_str))?;

    with_palette_state_mut(|state| {
        state.raw_input_key = Some(raw_input_key);
        state.palette_key = Some(resolved_palette.into_owned());
        state.pending_refresh_key = None;
    });
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
    let disposition = with_palette_state_mut(|state| stage_palette_refresh(state, raw_input_key));

    match disposition {
        PaletteRefreshDisposition::Ready | PaletteRefreshDisposition::RefreshAlreadyPending => {
            Ok(())
        }
        PaletteRefreshDisposition::BootstrapSynchronously => {
            // first draw has no committed smear highlight groups yet, so keep a one-time
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
    use crate::config::MAX_COLOR_LEVELS;
    use crate::core::types::StrokeId;
    use crate::position::RenderPoint;
    use crate::test_support::proptest::ModeCase;
    use crate::test_support::proptest::cache_key_mutation_axis;
    use crate::test_support::proptest::mode_case;
    use crate::test_support::proptest::pure_config;
    use crate::types::ModeClass;
    use crate::types::RenderFrame;
    use crate::types::StaticRenderConfig;
    use pretty_assertions::assert_eq;
    use proptest::collection::vec;
    use proptest::option;
    use proptest::prelude::*;

    const RAW_KEY_COMMON_AXIS_COUNT: usize = 7;
    const RESOLVED_PALETTE_AXIS_COUNT: usize = 8;

    fn test_frame() -> RenderFrame {
        RenderFrame {
            mode: ModeClass::NormalLike,
            corners: [RenderPoint::ZERO; 4],
            step_samples: Vec::new().into(),
            planner_idle_steps: 0,
            target: RenderPoint::ZERO,
            target_corners: [RenderPoint::ZERO; 4],
            vertical_bar: false,
            trail_stroke_id: StrokeId::INITIAL,
            retarget_epoch: 0,
            particle_count: 0,
            aggregated_particle_cells: Arc::default(),
            particle_screen_cells: Arc::default(),
            color_at_cursor: Some(0x00FF_FFFF),
            projection_policy_revision: crate::core::types::ProjectionPolicyRevision::INITIAL,
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

    fn mutate_static_config(
        frame: &mut RenderFrame,
        mutator: impl FnOnce(&mut StaticRenderConfig),
    ) {
        let mut config = (*frame.static_config).clone();
        mutator(&mut config);
        frame.static_config = Arc::new(config);
    }

    fn set_active_cursor_color_setting(frame: &mut RenderFrame, setting: &str) {
        let insert_like = frame.mode.is_insert_like();
        mutate_static_config(frame, |config| {
            if insert_like {
                config.cursor_color_insert_mode = Some(setting.to_string());
            } else {
                config.cursor_color = Some(setting.to_string());
            }
        });
    }

    fn set_inactive_cursor_color_setting(frame: &mut RenderFrame, setting: &str) {
        let insert_like = frame.mode.is_insert_like();
        mutate_static_config(frame, |config| {
            if insert_like {
                config.cursor_color = Some(setting.to_string());
            } else {
                config.cursor_color_insert_mode = Some(setting.to_string());
            }
        });
    }

    fn frame_for_raw_key_properties(mode: &ModeCase, depends_on_cursor_text: bool) -> RenderFrame {
        let mut frame = test_frame();
        frame.mode = mode.mode().into();
        set_active_cursor_color_setting(
            &mut frame,
            if depends_on_cursor_text {
                "none"
            } else {
                "#112233"
            },
        );
        set_inactive_cursor_color_setting(&mut frame, "#445566");
        frame.color_at_cursor = Some(0x00FF_FFFF);
        frame
    }

    fn mutate_raw_key_common_axis(frame: &mut RenderFrame, axis: usize) {
        match axis {
            0 => set_active_cursor_color_setting(frame, "#ABCDEF"),
            1 => mutate_static_config(frame, |config| {
                config.normal_bg = Some("#202021".to_string());
            }),
            2 => mutate_static_config(frame, |config| {
                config.transparent_bg_fallback_color = "#303031".to_string();
            }),
            3 => mutate_static_config(frame, |config| {
                config.cterm_cursor_colors = Some(vec![18_u16, 42_u16, 99_u16]);
            }),
            4 => mutate_static_config(frame, |config| {
                config.cterm_bg = Some(236_u16);
            }),
            5 => mutate_static_config(frame, |config| {
                config.color_levels = 17;
            }),
            6 => mutate_static_config(frame, |config| {
                config.gamma = 1.8;
            }),
            _ => panic!("unexpected raw key axis {axis}"),
        }
    }

    #[derive(Clone, Debug)]
    struct ResolvedPaletteFixture {
        cursor_color: u32,
        normal_background: Option<u32>,
        transparent_fallback: u32,
        non_inverted_blend: u8,
        color_levels: u32,
        gamma_bits: u64,
        cterm_cursor_colors: Option<Vec<u16>>,
        cterm_bg: Option<u16>,
    }

    impl ResolvedPaletteFixture {
        fn borrowed(&self) -> ResolvedPalette<'_> {
            ResolvedPalette {
                cursor_color: self.cursor_color,
                normal_background: self.normal_background,
                transparent_fallback: self.transparent_fallback,
                non_inverted_blend: self.non_inverted_blend,
                color_levels: self.color_levels,
                gamma_bits: self.gamma_bits,
                cterm_cursor_colors: self.cterm_cursor_colors.as_deref(),
                cterm_bg: self.cterm_bg,
            }
        }
    }

    fn resolved_palette_fixture() -> BoxedStrategy<ResolvedPaletteFixture> {
        (
            0_u32..=0x00FF_FFFF,
            option::of(0_u32..=0x00FF_FFFF),
            0_u32..=0x00FF_FFFF,
            any::<u8>(),
            1_u32..=32_u32,
            0.5_f64..4.0_f64,
            option::of(vec(any::<u16>(), 1..=4)),
            option::of(any::<u16>()),
        )
            .prop_map(
                |(
                    cursor_color,
                    normal_background,
                    transparent_fallback,
                    non_inverted_blend,
                    color_levels,
                    gamma,
                    cterm_cursor_colors,
                    cterm_bg,
                )| ResolvedPaletteFixture {
                    cursor_color,
                    normal_background,
                    transparent_fallback,
                    non_inverted_blend,
                    color_levels,
                    gamma_bits: gamma.to_bits(),
                    cterm_cursor_colors,
                    cterm_bg,
                },
            )
            .boxed()
    }

    fn different_raw_key(raw_key: RawPaletteInputKey) -> RawPaletteInputKey {
        RawPaletteInputKey {
            fingerprint: raw_key.fingerprint ^ 1,
        }
    }

    fn mutate_palette_key_axis(
        fixture: &ResolvedPaletteFixture,
        axis: usize,
    ) -> HighlightPaletteKey {
        let mut cached = fixture.borrowed().into_owned();
        match axis {
            0 => cached.cursor_color ^= 1,
            1 => {
                cached.normal_background = Some(
                    fixture
                        .normal_background
                        .map_or(0x0012_3456, |background| background ^ 1),
                );
            }
            2 => cached.transparent_fallback ^= 1,
            3 => cached.non_inverted_blend = cached.non_inverted_blend.wrapping_add(1),
            4 => cached.color_levels = cached.color_levels.saturating_add(1),
            5 => {
                cached.gamma_bits = if cached.gamma_bits == 1.0_f64.to_bits() {
                    2.0_f64.to_bits()
                } else {
                    1.0_f64.to_bits()
                };
            }
            6 => {
                cached.cterm_cursor_colors = Some(match fixture.cterm_cursor_colors.as_deref() {
                    Some(colors) => {
                        let mut mutated = colors.to_vec();
                        mutated[0] = mutated[0].wrapping_add(1);
                        mutated
                    }
                    None => vec![7_u16, 11_u16],
                });
            }
            7 => {
                cached.cterm_bg = Some(
                    fixture
                        .cterm_bg
                        .map_or(1_u16, |value| value.wrapping_add(1)),
                );
            }
            _ => panic!("unexpected resolved palette axis {axis}"),
        }
        cached
    }

    proptest! {
        #![proptest_config(pure_config())]

        #[test]
        fn prop_raw_palette_input_key_changes_when_common_effective_inputs_change(
            mode in mode_case(),
            depends_on_cursor_text in any::<bool>(),
            axis in cache_key_mutation_axis(RAW_KEY_COMMON_AXIS_COUNT),
        ) {
            let base = frame_for_raw_key_properties(&mode, depends_on_cursor_text);
            let mut mutated = base.clone();
            mutate_raw_key_common_axis(&mut mutated, axis.index());

            prop_assert_ne!(raw_palette_input_key(&base), raw_palette_input_key(&mutated));
        }

        #[test]
        fn prop_raw_palette_input_key_uses_cursor_text_only_when_the_effective_setting_is_none(
            mode in mode_case(),
            depends_on_cursor_text in any::<bool>(),
        ) {
            let base = frame_for_raw_key_properties(&mode, depends_on_cursor_text);
            let mut mutated = base.clone();
            mutated.color_at_cursor = Some(0x00AB_CDEF);

            if depends_on_cursor_text {
                prop_assert_ne!(raw_palette_input_key(&base), raw_palette_input_key(&mutated));
            } else {
                prop_assert_eq!(raw_palette_input_key(&base), raw_palette_input_key(&mutated));
            }
        }

        #[test]
        fn prop_raw_palette_input_key_ignores_inactive_mode_specific_cursor_settings(
            mode in mode_case(),
            depends_on_cursor_text in any::<bool>(),
        ) {
            let base = frame_for_raw_key_properties(&mode, depends_on_cursor_text);
            let mut mutated = base.clone();
            set_inactive_cursor_color_setting(&mut mutated, "#654321");

            prop_assert_eq!(raw_palette_input_key(&base), raw_palette_input_key(&mutated));
        }

        #[test]
        fn prop_resolved_palette_matches_owned_form_and_rejects_single_axis_mismatches(
            fixture in resolved_palette_fixture(),
            axis in cache_key_mutation_axis(RESOLVED_PALETTE_AXIS_COUNT),
        ) {
            let resolved = fixture.borrowed();
            let cached = resolved.into_owned();

            prop_assert!(resolved_palette_matches(&cached, &resolved));
            prop_assert!(!resolved_palette_matches(
                &mutate_palette_key_axis(&fixture, axis.index()),
                &resolved,
            ));
        }

        #[test]
        fn prop_lookup_cached_palette_distinguishes_raw_resolved_and_miss_paths(
            fixture in resolved_palette_fixture(),
            raw_matches in any::<bool>(),
            provide_resolved in any::<bool>(),
            resolved_matches in any::<bool>(),
            mismatch_axis in cache_key_mutation_axis(RESOLVED_PALETTE_AXIS_COUNT),
            query_fingerprint in any::<u64>(),
        ) {
            let raw_key = RawPaletteInputKey {
                fingerprint: query_fingerprint,
            };
            let resolved = fixture.borrowed();
            let palette_key = if resolved_matches {
                resolved.into_owned()
            } else {
                mutate_palette_key_axis(&fixture, mismatch_axis.index())
            };
            let state = PaletteState {
                raw_input_key: Some(if raw_matches {
                    raw_key
                } else {
                    different_raw_key(raw_key)
                }),
                palette_key: Some(palette_key),
                pending_refresh_key: None,
                group_name_cache: HashMap::new(),
            };
            let expected = if raw_matches {
                PaletteCacheLookup::RawHit
            } else if provide_resolved && resolved_matches {
                PaletteCacheLookup::ResolvedHit
            } else {
                PaletteCacheLookup::Miss
            };

            prop_assert_eq!(
                lookup_cached_palette(
                    &state,
                    raw_key,
                    provide_resolved.then_some(&resolved),
                ),
                expected
            );
        }

        #[test]
        fn prop_stage_palette_refresh_returns_ready_without_changing_pending_state(
            fixture in resolved_palette_fixture(),
            query_fingerprint in any::<u64>(),
            pending_matches in any::<bool>(),
        ) {
            let raw_key = RawPaletteInputKey {
                fingerprint: query_fingerprint,
            };
            let pending_refresh_key = Some(if pending_matches {
                raw_key
            } else {
                different_raw_key(raw_key)
            });
            let mut state = PaletteState {
                raw_input_key: Some(raw_key),
                palette_key: Some(fixture.borrowed().into_owned()),
                pending_refresh_key,
                group_name_cache: HashMap::new(),
            };

            prop_assert_eq!(
                stage_palette_refresh(&mut state, raw_key),
                PaletteRefreshDisposition::Ready
            );
            prop_assert_eq!(state.pending_refresh_key, pending_refresh_key);
        }

        #[test]
        fn prop_stage_palette_refresh_deduplicates_matching_pending_requests(
            fixture in resolved_palette_fixture(),
            query_fingerprint in any::<u64>(),
            has_committed_palette in any::<bool>(),
        ) {
            let raw_key = RawPaletteInputKey {
                fingerprint: query_fingerprint,
            };
            let mut state = PaletteState {
                raw_input_key: Some(different_raw_key(raw_key)),
                palette_key: has_committed_palette.then(|| fixture.borrowed().into_owned()),
                pending_refresh_key: Some(raw_key),
                group_name_cache: HashMap::new(),
            };

            prop_assert_eq!(
                stage_palette_refresh(&mut state, raw_key),
                PaletteRefreshDisposition::RefreshAlreadyPending
            );
            prop_assert_eq!(state.pending_refresh_key, Some(raw_key));
        }

        #[test]
        fn prop_stage_palette_refresh_bootstraps_or_defers_based_on_committed_palette(
            fixture in resolved_palette_fixture(),
            query_fingerprint in any::<u64>(),
            has_committed_palette in any::<bool>(),
            had_other_pending_refresh in any::<bool>(),
        ) {
            let raw_key = RawPaletteInputKey {
                fingerprint: query_fingerprint,
            };
            let mut state = PaletteState {
                raw_input_key: Some(different_raw_key(raw_key)),
                palette_key: has_committed_palette.then(|| fixture.borrowed().into_owned()),
                pending_refresh_key: had_other_pending_refresh.then(|| different_raw_key(raw_key)),
                group_name_cache: HashMap::new(),
            };

            prop_assert_eq!(
                stage_palette_refresh(&mut state, raw_key),
                if has_committed_palette {
                    PaletteRefreshDisposition::ScheduleDeferred
                } else {
                    PaletteRefreshDisposition::BootstrapSynchronously
                }
            );
            prop_assert_eq!(state.pending_refresh_key, Some(raw_key));
        }
    }

    #[test]
    fn clear_highlight_cache_resets_raw_and_resolved_keys() {
        clear_highlight_cache();
        with_palette_state_mut(|state| {
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
        });

        clear_highlight_cache();

        with_palette_state(|state| {
            assert_eq!(state.raw_input_key, None);
            assert_eq!(state.palette_key, None);
            assert_eq!(state.pending_refresh_key, None);
        });
    }

    #[test]
    fn stale_highlight_group_names_clears_only_the_truncated_palette_tail() {
        assert_eq!(
            stale_highlight_group_names(Some(5), Some(3)),
            vec![
                "SmearCursor4".to_string(),
                "SmearCursorInverted4".to_string(),
                "SmearCursor5".to_string(),
                "SmearCursorInverted5".to_string(),
            ]
        );
    }

    #[test]
    fn stale_highlight_group_names_clears_every_managed_group_on_full_reset() {
        assert_eq!(
            stale_highlight_group_names(Some(2), None),
            vec![
                "SmearCursor1".to_string(),
                "SmearCursorInverted1".to_string(),
                "SmearCursor2".to_string(),
                "SmearCursorInverted2".to_string(),
            ]
        );
    }

    #[test]
    fn highlight_group_names_clamps_requests_to_the_palette_cap() {
        let capped = highlight_group_names(MAX_COLOR_LEVELS);
        let oversized = highlight_group_names(MAX_COLOR_LEVELS.saturating_add(32));

        assert_eq!(oversized.normal, capped.normal);
        assert_eq!(oversized.inverted, capped.inverted);
    }

    #[test]
    fn stale_palette_refresh_returns_skipped_stale_directly() {
        clear_highlight_cache();
        let spec = PaletteSpec::from_frame(&test_frame());
        let stale_key = raw_palette_input_key_for_spec(&spec);

        with_palette_state_mut(|state| {
            state.raw_input_key = Some(RawPaletteInputKey {
                fingerprint: stale_key.fingerprint.saturating_add(1),
            });
            state.pending_refresh_key = Some(RawPaletteInputKey {
                fingerprint: stale_key.fingerprint.saturating_add(2),
            });
        });

        assert_eq!(
            refresh_highlight_palette_for_spec(&spec, stale_key)
                .expect("stale refresh should short-circuit"),
            PaletteRefreshOutcome::SkippedStale
        );
    }
}
