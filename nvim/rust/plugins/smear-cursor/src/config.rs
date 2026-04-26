mod derived;

pub(crate) use derived::DerivedConfigCache;

use crate::core::state::BufferPerfClass;
use crate::types::BASE_TIME_INTERVAL;
use crate::types::CursorCellShape;
use nvimrs_nvim_utils::mode::is_insert_like_mode;
use nvimrs_nvim_utils::mode::is_replace_like_mode;
use std::collections::HashSet;
use std::sync::Arc;

pub(crate) const DEFAULT_ANIMATION_FPS: f64 = 72.0;
pub(crate) const DEFAULT_BLOCK_ASPECT_RATIO: f64 = 2.0;
// Keep the default cap aligned with the measured window-switch scenarios: the
// refreshed `perf/current.md` snapshot keeps peak requested windows well below
// the 64-window cap, preserving burst headroom with zero cap hits.
pub(crate) const DEFAULT_MAX_KEPT_WINDOWS: usize = 64;
pub(crate) const MAX_COLOR_LEVELS: u32 = 256;

pub(crate) const fn normalize_color_levels(color_levels: u32) -> u32 {
    if color_levels == 0 {
        1
    } else if color_levels > MAX_COLOR_LEVELS {
        MAX_COLOR_LEVELS
    } else {
        color_levels
    }
}

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub(crate) enum BufferPerfMode {
    #[default]
    Auto,
    Full,
    Fast,
    Off,
}

impl BufferPerfMode {
    pub(crate) const fn option_name(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Full => "full",
            Self::Fast => "fast",
            Self::Off => "off",
        }
    }

    pub(crate) const fn forced_perf_class(self) -> Option<BufferPerfClass> {
        match self {
            Self::Auto => None,
            Self::Full => Some(BufferPerfClass::Full),
            Self::Fast => Some(BufferPerfClass::FastMotion),
            Self::Off => Some(BufferPerfClass::Skip),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum LogLevel {
    Trace,
    Debug,
    #[default]
    Info,
    Warn,
    Error,
    Off,
}

impl LogLevel {
    pub(crate) const fn from_i64(level: i64) -> Self {
        if level <= 0 {
            Self::Trace
        } else {
            match level {
                1 => Self::Debug,
                2 => Self::Info,
                3 => Self::Warn,
                4 => Self::Error,
                _ => Self::Off,
            }
        }
    }

    pub(crate) const fn as_vim_level(self) -> i64 {
        match self {
            Self::Trace => 0,
            Self::Debug => 1,
            Self::Info => 2,
            Self::Warn => 3,
            Self::Error => 4,
            Self::Off => 5,
        }
    }

    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Trace => "TRACE",
            Self::Debug => "DEBUG",
            Self::Info => "INFO",
            Self::Warn => "WARNING",
            Self::Error => "ERROR",
            Self::Off => "OFF",
        }
    }

    pub(crate) const fn should_notify(self) -> bool {
        matches!(self, Self::Info | Self::Warn | Self::Error)
    }

    pub(crate) const fn allows(self, message_level: Self) -> bool {
        if matches!(self, Self::Off) {
            false
        } else {
            self.as_vim_level() <= message_level.as_vim_level()
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RuntimeConfig {
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
    pub(crate) max_kept_windows: usize,
    pub(crate) windows_zindex: u32,
    pub(crate) buffer_perf_mode: BufferPerfMode,
    pub(crate) filetypes_disabled: Arc<HashSet<String>>,
    pub(crate) logging_level: LogLevel,
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

    pub(crate) fn cursor_cell_shape(&self, mode: &str) -> CursorCellShape {
        if is_replace_like_mode(mode) && self.horizontal_bar_cursor_replace_mode {
            CursorCellShape::HorizontalBar
        } else if is_insert_like_mode(mode) {
            if self.vertical_bar_cursor_insert_mode {
                CursorCellShape::VerticalBar
            } else {
                CursorCellShape::Block
            }
        } else if self.vertical_bar_cursor {
            CursorCellShape::VerticalBar
        } else {
            CursorCellShape::Block
        }
    }

    pub(crate) fn requires_cursor_color_sampling_for_mode(&self, mode: &str) -> bool {
        let setting = if is_insert_like_mode(mode) {
            self.cursor_color_insert_mode.as_deref()
        } else {
            self.cursor_color.as_deref()
        };

        setting == Some("none")
    }

    pub(crate) fn requires_cursor_color_sampling(&self) -> bool {
        self.requires_cursor_color_sampling_for_mode("n")
            || self.requires_cursor_color_sampling_for_mode("i")
    }

    pub(crate) fn requires_background_sampling_for_perf_class(
        &self,
        buffer_perf_class: BufferPerfClass,
    ) -> bool {
        self.particles_enabled
            && !self.particles_over_text
            && buffer_perf_class.keeps_ornamental_effects()
    }

    pub(crate) fn simulation_step_interval_ms(&self) -> f64 {
        Self::interval_ms_for_fps(self.simulation_hz)
    }
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
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
            // Keep the peak pool cap distinct from the adaptive retained-budget ceiling.
            // Cleanup already converges retained windows toward the adaptive budget floor, but
            // one hot frame can still need more simultaneous windows than we keep warm when idle.
            max_kept_windows: DEFAULT_MAX_KEPT_WINDOWS,
            windows_zindex: 300,
            buffer_perf_mode: BufferPerfMode::Auto,
            filetypes_disabled: Arc::default(),
            logging_level: LogLevel::Info,
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

#[cfg(test)]
mod tests {
    use super::DEFAULT_ANIMATION_FPS;
    use super::LogLevel;
    use super::RuntimeConfig;
    use crate::test_support::proptest::ModeFamily;
    use crate::test_support::proptest::mode_family;
    use crate::test_support::proptest::pure_config;
    use crate::test_support::proptest::representative_mode;
    use crate::types::CursorCellShape;
    use proptest::prelude::*;

    proptest! {
        #![proptest_config(pure_config())]

        #[test]
        fn prop_cursor_cell_shape_uses_mode_specific_precedence(
            mode_family in mode_family(),
            vertical_bar_cursor in any::<bool>(),
            vertical_bar_cursor_insert_mode in any::<bool>(),
            horizontal_bar_cursor_replace_mode in any::<bool>(),
        ) {
            let config = RuntimeConfig {
                vertical_bar_cursor,
                vertical_bar_cursor_insert_mode,
                horizontal_bar_cursor_replace_mode,
                ..RuntimeConfig::default()
            };
            let mode = representative_mode(mode_family);

            let expected = match mode_family {
                ModeFamily::Replace if horizontal_bar_cursor_replace_mode => {
                    CursorCellShape::HorizontalBar
                }
                ModeFamily::Replace if vertical_bar_cursor => CursorCellShape::VerticalBar,
                ModeFamily::Insert if vertical_bar_cursor_insert_mode => {
                    CursorCellShape::VerticalBar
                }
                ModeFamily::Normal
                | ModeFamily::Terminal
                | ModeFamily::Cmdline
                | ModeFamily::Other
                    if vertical_bar_cursor =>
                {
                    CursorCellShape::VerticalBar
                }
                _ => CursorCellShape::Block,
            };

            prop_assert_eq!(config.cursor_cell_shape(mode), expected);
        }

        #[test]
        fn prop_requires_cursor_color_sampling_uses_active_mode_family(
            mode_family in mode_family(),
            normal_mode_uses_sampling in any::<bool>(),
            insert_mode_uses_sampling in any::<bool>(),
        ) {
            let config = RuntimeConfig {
                cursor_color: Some(if normal_mode_uses_sampling {
                    "none".to_string()
                } else {
                    "#112233".to_string()
                }),
                cursor_color_insert_mode: Some(if insert_mode_uses_sampling {
                    "none".to_string()
                } else {
                    "#445566".to_string()
                }),
                ..RuntimeConfig::default()
            };
            let mode = representative_mode(mode_family);

            let expected = if mode_family == ModeFamily::Insert {
                insert_mode_uses_sampling
            } else {
                normal_mode_uses_sampling
            };

            prop_assert_eq!(
                config.requires_cursor_color_sampling_for_mode(mode),
                expected
            );
            prop_assert_eq!(
                config.requires_cursor_color_sampling(),
                normal_mode_uses_sampling || insert_mode_uses_sampling
            );
        }
    }

    #[test]
    fn default_time_interval_matches_the_named_animation_fps() {
        let config = RuntimeConfig::default();

        assert_eq!(
            config.time_interval,
            RuntimeConfig::interval_ms_for_fps(DEFAULT_ANIMATION_FPS)
        );
        assert_eq!(config.simulation_hz, 120.0);
    }

    #[test]
    fn log_level_from_i64_preserves_vim_threshold_semantics() {
        assert_eq!(LogLevel::from_i64(-1), LogLevel::Trace);
        assert_eq!(LogLevel::from_i64(0), LogLevel::Trace);
        assert_eq!(LogLevel::from_i64(2), LogLevel::Info);
        assert_eq!(LogLevel::from_i64(4), LogLevel::Error);
        assert_eq!(LogLevel::from_i64(5), LogLevel::Off);
    }
}
