use super::RuntimeConfig;
use crate::types::StaticRenderConfig;
use std::sync::Arc;

// Retained policy cache rebuilt from `RuntimeConfig`. Freshness stays on
// `RuntimeState.config_revision`; this cache deliberately carries no mirror
// revision of its own.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DerivedConfigCache {
    // cache: config-derived policy slices partitioned by consumer.
    quantization: Arc<QuantizationPolicy>,
    window_pool: Arc<WindowPoolPolicy>,
    planner: Arc<PlannerPolicy>,
    palette: Arc<PalettePolicy>,
}

impl DerivedConfigCache {
    pub(crate) fn new(config: &RuntimeConfig) -> Self {
        Self {
            quantization: Arc::new(QuantizationPolicy::from(config)),
            window_pool: Arc::new(WindowPoolPolicy::from(config)),
            planner: Arc::new(PlannerPolicy::from(config)),
            palette: Arc::new(PalettePolicy::from(config)),
        }
    }

    pub(crate) fn static_render_config(&self) -> StaticRenderConfig {
        StaticRenderConfig::from(self)
    }

    pub(crate) fn matches_projection_policy(&self, other: &Self) -> bool {
        self.window_pool == other.window_pool
            && self.planner == other.planner
            // Highlight gamma only affects shell palette materialization. Projection reuse
            // needs the quantization level, but not the palette curve.
            && self.quantization.color_levels == other.quantization.color_levels
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct QuantizationPolicy {
    color_levels: u32,
    gamma: f64,
}

impl From<&RuntimeConfig> for QuantizationPolicy {
    fn from(config: &RuntimeConfig) -> Self {
        Self {
            color_levels: config.color_levels,
            gamma: config.gamma,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct WindowPoolPolicy {
    max_kept_windows: usize,
    windows_zindex: u32,
}

impl From<&RuntimeConfig> for WindowPoolPolicy {
    fn from(config: &RuntimeConfig) -> Self {
        Self {
            max_kept_windows: config.max_kept_windows,
            windows_zindex: config.windows_zindex,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PlannerPolicy {
    hide_target_hack: bool,
    never_draw_over_target: bool,
    particles_over_text: bool,
    particle_max_lifetime: f64,
    particle_switch_octant_braille: f64,
    block_aspect_ratio: f64,
    tail_duration_ms: f64,
    simulation_hz: f64,
    trail_thickness: f64,
    trail_thickness_x: f64,
    spatial_coherence_weight: f64,
    temporal_stability_weight: f64,
    top_k_per_cell: u8,
}

impl From<&RuntimeConfig> for PlannerPolicy {
    fn from(config: &RuntimeConfig) -> Self {
        Self {
            hide_target_hack: config.hide_target_hack,
            never_draw_over_target: config.never_draw_over_target,
            particles_over_text: config.particles_over_text,
            particle_max_lifetime: config.particle_max_lifetime,
            particle_switch_octant_braille: config.particle_switch_octant_braille,
            block_aspect_ratio: config.block_aspect_ratio,
            tail_duration_ms: config.tail_duration_ms.max(1.0),
            simulation_hz: config.simulation_hz,
            trail_thickness: config.trail_thickness,
            trail_thickness_x: config.trail_thickness_x,
            spatial_coherence_weight: config.spatial_coherence_weight,
            temporal_stability_weight: config.temporal_stability_weight,
            top_k_per_cell: config.top_k_per_cell.max(2),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PalettePolicy {
    cursor_color: Option<String>,
    cursor_color_insert_mode: Option<String>,
    normal_bg: Option<String>,
    transparent_bg_fallback_color: String,
    cterm_cursor_colors: Option<Vec<u16>>,
    cterm_bg: Option<u16>,
}

impl From<&RuntimeConfig> for PalettePolicy {
    fn from(config: &RuntimeConfig) -> Self {
        Self {
            cursor_color: config.cursor_color.clone(),
            cursor_color_insert_mode: config.cursor_color_insert_mode.clone(),
            normal_bg: config.normal_bg.clone(),
            transparent_bg_fallback_color: config.transparent_bg_fallback_color.clone(),
            cterm_cursor_colors: config.cterm_cursor_colors.clone(),
            cterm_bg: config.cterm_bg,
        }
    }
}

impl From<&DerivedConfigCache> for StaticRenderConfig {
    fn from(config: &DerivedConfigCache) -> Self {
        Self {
            cursor_color: config.palette.cursor_color.clone(),
            cursor_color_insert_mode: config.palette.cursor_color_insert_mode.clone(),
            normal_bg: config.palette.normal_bg.clone(),
            transparent_bg_fallback_color: config.palette.transparent_bg_fallback_color.clone(),
            cterm_cursor_colors: config.palette.cterm_cursor_colors.clone(),
            cterm_bg: config.palette.cterm_bg,
            hide_target_hack: config.planner.hide_target_hack,
            max_kept_windows: config.window_pool.max_kept_windows,
            never_draw_over_target: config.planner.never_draw_over_target,
            particle_max_lifetime: config.planner.particle_max_lifetime,
            particle_switch_octant_braille: config.planner.particle_switch_octant_braille,
            particles_over_text: config.planner.particles_over_text,
            color_levels: config.quantization.color_levels,
            gamma: config.quantization.gamma,
            block_aspect_ratio: config.planner.block_aspect_ratio,
            tail_duration_ms: config.planner.tail_duration_ms,
            simulation_hz: config.planner.simulation_hz,
            trail_thickness: config.planner.trail_thickness,
            trail_thickness_x: config.planner.trail_thickness_x,
            spatial_coherence_weight: config.planner.spatial_coherence_weight,
            temporal_stability_weight: config.planner.temporal_stability_weight,
            top_k_per_cell: config.planner.top_k_per_cell,
            windows_zindex: config.window_pool.windows_zindex,
        }
    }
}
