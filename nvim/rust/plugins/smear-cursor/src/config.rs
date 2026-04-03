use crate::types::BASE_TIME_INTERVAL;
use crate::types::StaticRenderConfig;
#[cfg(test)]
use nvimrs_nvim_utils::mode::is_cmdline_mode;
use nvimrs_nvim_utils::mode::is_insert_like_mode;
use nvimrs_nvim_utils::mode::is_replace_like_mode;
#[cfg(test)]
use nvimrs_nvim_utils::mode::is_terminal_like_mode;
use std::collections::HashSet;
use std::sync::Arc;

pub(crate) const DEFAULT_ANIMATION_FPS: f64 = 144.0;
pub(crate) const DEFAULT_BLOCK_ASPECT_RATIO: f64 = 2.0;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RuntimeConfig {
    pub(crate) fps: f64,
    pub(crate) time_interval: f64,
    pub(crate) simulation_hz: f64,
    pub(crate) max_simulation_steps_per_frame: u32,
    pub(crate) delay_event_to_smear: f64,
    pub(crate) anticipation: f64,
    pub(crate) head_response_ms: f64,
    pub(crate) damping_ratio: f64,
    pub(crate) tail_response_ms: f64,
    pub(crate) max_length: f64,
    pub(crate) max_length_insert_mode: f64,
    pub(crate) trail_duration_ms: f64,
    pub(crate) trail_min_distance: f64,
    pub(crate) trail_thickness: f64,
    pub(crate) trail_thickness_x: f64,
    pub(crate) tail_duration_ms: f64,
    pub(crate) stop_distance_enter: f64,
    pub(crate) stop_distance_exit: f64,
    pub(crate) stop_velocity_enter: f64,
    pub(crate) stop_hold_frames: u32,
    pub(crate) smear_between_windows: bool,
    pub(crate) smear_between_buffers: bool,
    pub(crate) smear_between_neighbor_lines: bool,
    pub(crate) min_horizontal_distance_smear: f64,
    pub(crate) min_vertical_distance_smear: f64,
    pub(crate) smear_horizontally: bool,
    pub(crate) smear_vertically: bool,
    pub(crate) smear_diagonally: bool,
    pub(crate) scroll_buffer_space: bool,
    pub(crate) smear_insert_mode: bool,
    pub(crate) smear_replace_mode: bool,
    pub(crate) smear_terminal_mode: bool,
    pub(crate) animate_in_insert_mode: bool,
    pub(crate) animate_command_line: bool,
    pub(crate) smear_to_cmd: bool,
    pub(crate) hide_target_hack: bool,
    pub(crate) jump_cues_enabled: bool,
    pub(crate) jump_cue_min_display_distance: f64,
    pub(crate) jump_cue_duration_ms: f64,
    pub(crate) jump_cue_strength: f64,
    pub(crate) jump_cue_max_chain: u8,
    pub(crate) jump_intent_window_ms: f64,
    pub(crate) cross_window_jump_bridges: bool,
    pub(crate) cross_window_bridge_strength_scale: f64,
    pub(crate) max_kept_windows: usize,
    pub(crate) windows_zindex: u32,
    pub(crate) filetypes_disabled: Arc<HashSet<String>>,
    pub(crate) logging_level: i64,
    pub(crate) cursor_color: Option<String>,
    pub(crate) cursor_color_insert_mode: Option<String>,
    pub(crate) normal_bg: Option<String>,
    pub(crate) transparent_bg_fallback_color: String,
    pub(crate) cterm_cursor_colors: Option<Vec<u16>>,
    pub(crate) cterm_bg: Option<u16>,
    pub(crate) vertical_bar_cursor: bool,
    pub(crate) vertical_bar_cursor_insert_mode: bool,
    pub(crate) horizontal_bar_cursor_replace_mode: bool,
    pub(crate) particle_damping: f64,
    pub(crate) particles_enabled: bool,
    pub(crate) particle_gravity: f64,
    pub(crate) particle_random_velocity: f64,
    pub(crate) particle_max_num: usize,
    pub(crate) particle_spread: f64,
    pub(crate) particles_per_second: f64,
    pub(crate) particles_per_length: f64,
    pub(crate) particle_max_initial_velocity: f64,
    pub(crate) particle_velocity_from_cursor: f64,
    pub(crate) particle_max_lifetime: f64,
    pub(crate) particle_lifetime_distribution_exponent: f64,
    pub(crate) min_distance_emit_particles: f64,
    pub(crate) particle_switch_octant_braille: f64,
    pub(crate) particles_over_text: bool,
    pub(crate) block_aspect_ratio: f64,
    pub(crate) never_draw_over_target: bool,
    pub(crate) color_levels: u32,
    pub(crate) gamma: f64,
    pub(crate) spatial_coherence_weight: f64,
    pub(crate) temporal_stability_weight: f64,
    pub(crate) top_k_per_cell: u8,
}

impl RuntimeConfig {
    pub(crate) fn interval_ms_for_fps(fps: f64) -> f64 {
        if !fps.is_finite() || fps <= 0.0 {
            return BASE_TIME_INTERVAL;
        }
        (1000.0 / fps).max(1.0)
    }

    #[cfg(test)]
    pub(crate) fn mode_allowed(&self, mode: &str) -> bool {
        if is_insert_like_mode(mode) {
            self.smear_insert_mode
        } else if is_replace_like_mode(mode) {
            self.smear_replace_mode
        } else if is_terminal_like_mode(mode) {
            self.smear_terminal_mode
        } else if is_cmdline_mode(mode) {
            self.smear_to_cmd
        } else {
            true
        }
    }

    pub(crate) fn cursor_is_vertical_bar(&self, mode: &str) -> bool {
        if is_insert_like_mode(mode) {
            self.vertical_bar_cursor_insert_mode
        } else {
            self.vertical_bar_cursor
        }
    }

    pub(crate) fn cursor_is_horizontal_bar(&self, mode: &str) -> bool {
        is_replace_like_mode(mode) && self.horizontal_bar_cursor_replace_mode
    }

    pub(crate) fn requires_cursor_color_sampling(&self) -> bool {
        self.cursor_color.as_deref() == Some("none")
            || self.cursor_color_insert_mode.as_deref() == Some("none")
    }

    pub(crate) fn requires_background_sampling(&self) -> bool {
        self.particles_enabled && !self.particles_over_text
    }

    pub(crate) fn simulation_step_interval_ms(&self) -> f64 {
        Self::interval_ms_for_fps(self.simulation_hz)
    }
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            fps: DEFAULT_ANIMATION_FPS,
            time_interval: Self::interval_ms_for_fps(DEFAULT_ANIMATION_FPS),
            // Neovide slices animation dt at 1/120s; use the same baseline integration rate.
            simulation_hz: 120.0,
            max_simulation_steps_per_frame: 16,
            // Keep a tiny ingress debounce by default so bursty cursor churn coalesces
            // without adding human-noticeable latency.
            delay_event_to_smear: 1.0,
            // Neovide does not inject a retarget anticipation impulse for cursor corners.
            anticipation: 0.0,
            head_response_ms: 110.0,
            damping_ratio: 1.0,
            tail_response_ms: 198.0,
            // Match Neovide semantics: unbounded trail length by default.
            max_length: 0.0,
            max_length_insert_mode: 0.0,
            trail_duration_ms: 150.0,
            trail_min_distance: 0.0,
            trail_thickness: 1.0,
            trail_thickness_x: 1.0,
            tail_duration_ms: 198.0,
            // Keep lifecycle stop guards minimally invasive so spring geometry dominates.
            stop_distance_enter: 0.02,
            stop_distance_exit: 0.04,
            stop_velocity_enter: 0.02,
            stop_hold_frames: 1,
            smear_between_windows: true,
            smear_between_buffers: true,
            smear_between_neighbor_lines: true,
            min_horizontal_distance_smear: 0.0,
            min_vertical_distance_smear: 0.0,
            smear_horizontally: true,
            smear_vertically: true,
            smear_diagonally: true,
            scroll_buffer_space: true,
            smear_insert_mode: true,
            smear_replace_mode: true,
            smear_terminal_mode: true,
            animate_in_insert_mode: true,
            animate_command_line: true,
            smear_to_cmd: true,
            hide_target_hack: false,
            jump_cues_enabled: true,
            jump_cue_min_display_distance: 16.0,
            jump_cue_duration_ms: 84.0,
            jump_cue_strength: 1.0,
            jump_cue_max_chain: 3,
            jump_intent_window_ms: 40.0,
            cross_window_jump_bridges: true,
            cross_window_bridge_strength_scale: 1.0,
            max_kept_windows: 384,
            windows_zindex: 300,
            filetypes_disabled: Arc::default(),
            logging_level: 2,
            cursor_color: None,
            cursor_color_insert_mode: None,
            normal_bg: None,
            transparent_bg_fallback_color: "#303030".to_string(),
            cterm_cursor_colors: Some((240_u16..=255_u16).collect()),
            cterm_bg: Some(235),
            vertical_bar_cursor: false,
            vertical_bar_cursor_insert_mode: true,
            horizontal_bar_cursor_replace_mode: true,
            particle_damping: 0.2,
            particles_enabled: false,
            particle_gravity: 20.0,
            particle_random_velocity: 100.0,
            particle_max_num: 100,
            particle_spread: 0.5,
            particles_per_second: 200.0,
            particles_per_length: 1.0,
            particle_max_initial_velocity: 10.0,
            particle_velocity_from_cursor: 0.2,
            particle_max_lifetime: 300.0,
            particle_lifetime_distribution_exponent: 5.0,
            min_distance_emit_particles: 1.5,
            particle_switch_octant_braille: 0.3,
            particles_over_text: false,
            block_aspect_ratio: DEFAULT_BLOCK_ASPECT_RATIO,
            never_draw_over_target: false,
            color_levels: 128,
            gamma: 2.2,
            spatial_coherence_weight: 1.0,
            temporal_stability_weight: 0.12,
            top_k_per_cell: 5,
        }
    }
}

impl From<&RuntimeConfig> for StaticRenderConfig {
    fn from(config: &RuntimeConfig) -> Self {
        Self {
            cursor_color: config.cursor_color.clone(),
            cursor_color_insert_mode: config.cursor_color_insert_mode.clone(),
            normal_bg: config.normal_bg.clone(),
            transparent_bg_fallback_color: config.transparent_bg_fallback_color.clone(),
            cterm_cursor_colors: config.cterm_cursor_colors.clone(),
            cterm_bg: config.cterm_bg,
            hide_target_hack: config.hide_target_hack,
            max_kept_windows: config.max_kept_windows,
            never_draw_over_target: config.never_draw_over_target,
            particle_max_lifetime: config.particle_max_lifetime,
            particle_switch_octant_braille: config.particle_switch_octant_braille,
            particles_over_text: config.particles_over_text,
            color_levels: config.color_levels,
            gamma: config.gamma,
            block_aspect_ratio: config.block_aspect_ratio,
            tail_duration_ms: config.tail_duration_ms.max(1.0),
            simulation_hz: config.simulation_hz,
            trail_thickness: config.trail_thickness,
            trail_thickness_x: config.trail_thickness_x,
            spatial_coherence_weight: config.spatial_coherence_weight,
            temporal_stability_weight: config.temporal_stability_weight,
            top_k_per_cell: config.top_k_per_cell.max(2),
            windows_zindex: config.windows_zindex,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RuntimeConfig;

    fn mode_filter_fixture() -> RuntimeConfig {
        RuntimeConfig {
            smear_insert_mode: false,
            smear_replace_mode: false,
            smear_terminal_mode: false,
            smear_to_cmd: false,
            ..RuntimeConfig::default()
        }
    }

    #[test]
    fn mode_allowed_rejects_insert_composite_modes_when_insert_smear_is_disabled() {
        let config = mode_filter_fixture();
        assert!(!config.mode_allowed("ic"));
    }

    #[test]
    fn mode_allowed_accepts_insert_composite_modes_when_insert_smear_is_enabled() {
        let mut config = mode_filter_fixture();
        config.smear_insert_mode = true;
        assert!(config.mode_allowed("ic"));
    }

    #[test]
    fn mode_allowed_rejects_replace_composite_modes_when_replace_smear_is_disabled() {
        let config = mode_filter_fixture();
        assert!(!config.mode_allowed("Rc"));
    }

    #[test]
    fn mode_allowed_accepts_replace_composite_modes_when_replace_smear_is_enabled() {
        let mut config = mode_filter_fixture();
        config.smear_replace_mode = true;
        assert!(config.mode_allowed("Rc"));
    }

    #[test]
    fn mode_allowed_rejects_cmdline_composite_modes_when_cmdline_smear_is_disabled() {
        let config = mode_filter_fixture();
        assert!(!config.mode_allowed("cv"));
    }

    #[test]
    fn mode_allowed_accepts_cmdline_composite_modes_when_cmdline_smear_is_enabled() {
        let mut config = mode_filter_fixture();
        config.smear_to_cmd = true;
        assert!(config.mode_allowed("cv"));
    }

    #[test]
    fn mode_allowed_rejects_terminal_normal_mode_without_terminal_smear() {
        let config = mode_filter_fixture();
        assert!(!config.mode_allowed("nt"));
    }

    #[test]
    fn mode_allowed_accepts_terminal_normal_mode_with_terminal_smear() {
        let mut config = mode_filter_fixture();
        config.smear_terminal_mode = true;
        assert!(config.mode_allowed("nt"));
    }

    #[test]
    fn mode_allowed_rejects_terminal_pending_mode_without_terminal_smear() {
        let config = mode_filter_fixture();
        assert!(!config.mode_allowed("ntT"));
    }

    #[test]
    fn mode_allowed_accepts_terminal_pending_mode_with_terminal_smear() {
        let mut config = mode_filter_fixture();
        config.smear_terminal_mode = true;
        assert!(config.mode_allowed("ntT"));
    }

    #[test]
    fn mode_allowed_keeps_normal_mode_enabled_without_composite_flags() {
        let config = mode_filter_fixture();
        assert!(config.mode_allowed("n"));
    }

    #[test]
    fn cursor_is_vertical_bar_uses_insert_mode_family_flag() {
        let config = RuntimeConfig {
            vertical_bar_cursor: false,
            vertical_bar_cursor_insert_mode: true,
            horizontal_bar_cursor_replace_mode: true,
            ..RuntimeConfig::default()
        };

        assert!(config.cursor_is_vertical_bar("ic"));
        assert!(!config.cursor_is_vertical_bar("n"));
    }

    #[test]
    fn cursor_is_horizontal_bar_uses_replace_mode_family_flag() {
        let config = RuntimeConfig {
            vertical_bar_cursor: false,
            vertical_bar_cursor_insert_mode: true,
            horizontal_bar_cursor_replace_mode: true,
            ..RuntimeConfig::default()
        };

        assert!(config.cursor_is_horizontal_bar("Rc"));
        assert!(!config.cursor_is_horizontal_bar("n"));
    }

    #[test]
    fn default_delay_event_to_smear_keeps_small_ingress_debounce() {
        let config = RuntimeConfig::default();

        assert_eq!(config.delay_event_to_smear, 1.0);
    }
}
