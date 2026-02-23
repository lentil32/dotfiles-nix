use crate::types::BASE_TIME_INTERVAL;
use nvim_utils::mode::{
    is_cmdline_mode, is_insert_like_mode, is_replace_like_mode, is_terminal_like_mode,
};

#[derive(Clone, Debug)]
pub(crate) struct RuntimeConfig {
    pub(crate) time_interval: f64,
    pub(crate) delay_disable: Option<f64>,
    pub(crate) delay_event_to_smear: f64,
    pub(crate) delay_after_key: f64,
    pub(crate) stiffness: f64,
    pub(crate) trailing_stiffness: f64,
    pub(crate) trailing_exponent: f64,
    pub(crate) stiffness_insert_mode: f64,
    pub(crate) trailing_stiffness_insert_mode: f64,
    pub(crate) trailing_exponent_insert_mode: f64,
    pub(crate) anticipation: f64,
    pub(crate) damping: f64,
    pub(crate) damping_insert_mode: f64,
    pub(crate) max_length: f64,
    pub(crate) max_length_insert_mode: f64,
    pub(crate) distance_stop_animating: f64,
    pub(crate) distance_stop_animating_vertical_bar: f64,
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
    pub(crate) smear_to_cmd: bool,
    pub(crate) hide_target_hack: bool,
    pub(crate) max_kept_windows: usize,
    pub(crate) windows_zindex: u32,
    pub(crate) filetypes_disabled: Vec<String>,
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
    pub(crate) volume_reduction_exponent: f64,
    pub(crate) minimum_volume_factor: f64,
    pub(crate) never_draw_over_target: bool,
    pub(crate) use_diagonal_blocks: bool,
    pub(crate) max_slope_horizontal: f64,
    pub(crate) min_slope_vertical: f64,
    pub(crate) max_angle_difference_diagonal: f64,
    pub(crate) max_offset_diagonal: f64,
    pub(crate) min_shade_no_diagonal: f64,
    pub(crate) min_shade_no_diagonal_vertical_bar: f64,
    pub(crate) max_shade_no_matrix: f64,
    pub(crate) color_levels: u32,
    pub(crate) gamma: f64,
    pub(crate) gradient_exponent: f64,
    pub(crate) matrix_pixel_threshold: f64,
    pub(crate) matrix_pixel_threshold_vertical_bar: f64,
    pub(crate) matrix_pixel_min_factor: f64,
}

impl RuntimeConfig {
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
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            time_interval: BASE_TIME_INTERVAL,
            delay_disable: None,
            delay_event_to_smear: 1.0,
            delay_after_key: 5.0,
            stiffness: 0.6,
            trailing_stiffness: 0.45,
            trailing_exponent: 3.0,
            stiffness_insert_mode: 0.5,
            trailing_stiffness_insert_mode: 0.5,
            trailing_exponent_insert_mode: 1.0,
            anticipation: 0.2,
            damping: 0.85,
            damping_insert_mode: 0.9,
            max_length: 25.0,
            max_length_insert_mode: 1.0,
            distance_stop_animating: 0.1,
            distance_stop_animating_vertical_bar: 0.875,
            smear_between_buffers: true,
            smear_between_neighbor_lines: true,
            min_horizontal_distance_smear: 0.0,
            min_vertical_distance_smear: 0.0,
            smear_horizontally: true,
            smear_vertically: true,
            smear_diagonally: true,
            scroll_buffer_space: true,
            smear_insert_mode: true,
            smear_replace_mode: false,
            smear_terminal_mode: false,
            smear_to_cmd: true,
            hide_target_hack: false,
            max_kept_windows: 256,
            windows_zindex: 300,
            filetypes_disabled: Vec::new(),
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
            block_aspect_ratio: 2.0,
            volume_reduction_exponent: 0.3,
            minimum_volume_factor: 0.7,
            never_draw_over_target: false,
            use_diagonal_blocks: true,
            max_slope_horizontal: (1.0 / 3.0) / 1.5,
            min_slope_vertical: 2.0 * 1.5,
            max_angle_difference_diagonal: std::f64::consts::PI / 16.0,
            max_offset_diagonal: 0.2,
            min_shade_no_diagonal: 0.2,
            min_shade_no_diagonal_vertical_bar: 0.5,
            max_shade_no_matrix: 0.75,
            color_levels: 16,
            gamma: 2.2,
            gradient_exponent: 1.0,
            matrix_pixel_threshold: 0.7,
            matrix_pixel_threshold_vertical_bar: 0.25,
            matrix_pixel_min_factor: 0.5,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RuntimeConfig;

    #[test]
    fn mode_allowed_respects_composite_modes() {
        let mut config = RuntimeConfig {
            smear_insert_mode: false,
            smear_replace_mode: false,
            smear_terminal_mode: false,
            smear_to_cmd: false,
            ..RuntimeConfig::default()
        };

        assert!(!config.mode_allowed("ic"));
        assert!(!config.mode_allowed("Rc"));
        assert!(!config.mode_allowed("cv"));
        assert!(!config.mode_allowed("nt"));
        assert!(!config.mode_allowed("ntT"));
        assert!(config.mode_allowed("n"));

        config.smear_insert_mode = true;
        config.smear_replace_mode = true;
        config.smear_terminal_mode = true;
        config.smear_to_cmd = true;
        assert!(config.mode_allowed("ic"));
        assert!(config.mode_allowed("Rc"));
        assert!(config.mode_allowed("cv"));
        assert!(config.mode_allowed("nt"));
        assert!(config.mode_allowed("ntT"));
    }

    #[test]
    fn cursor_shape_helpers_use_mode_families() {
        let config = RuntimeConfig {
            vertical_bar_cursor: false,
            vertical_bar_cursor_insert_mode: true,
            horizontal_bar_cursor_replace_mode: true,
            ..RuntimeConfig::default()
        };

        assert!(config.cursor_is_vertical_bar("ic"));
        assert!(!config.cursor_is_vertical_bar("n"));
        assert!(config.cursor_is_horizontal_bar("Rc"));
        assert!(!config.cursor_is_horizontal_bar("n"));
    }
}
