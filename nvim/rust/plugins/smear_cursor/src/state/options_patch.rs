use super::RuntimeState;
use crate::config::RuntimeConfig;
use crate::lua::invalid_key;
use nvim_oxi::Result;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub(crate) struct RuntimeOptionsEffects {
    pub(crate) logging_level: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum OptionalChange<T> {
    Set(T),
    Clear,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CtermCursorColorsPatch {
    pub(crate) colors: Vec<u16>,
    pub(crate) color_levels: u32,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct RuntimeSwitchesPatch {
    pub(crate) enabled: Option<bool>,
    pub(crate) time_interval: Option<f64>,
    pub(crate) fps: Option<f64>,
    pub(crate) simulation_hz: Option<f64>,
    pub(crate) max_simulation_steps_per_frame: Option<u32>,
    pub(crate) delay_event_to_smear: Option<f64>,
    pub(crate) delay_after_key: Option<f64>,
    pub(crate) smear_to_cmd: Option<bool>,
    pub(crate) smear_insert_mode: Option<bool>,
    pub(crate) smear_replace_mode: Option<bool>,
    pub(crate) smear_terminal_mode: Option<bool>,
    pub(crate) animate_in_insert_mode: Option<bool>,
    pub(crate) animate_command_line: Option<bool>,
    pub(crate) vertical_bar_cursor: Option<bool>,
    pub(crate) vertical_bar_cursor_insert_mode: Option<bool>,
    pub(crate) horizontal_bar_cursor_replace_mode: Option<bool>,
    pub(crate) hide_target_hack: Option<bool>,
    pub(crate) max_kept_windows: Option<usize>,
    pub(crate) windows_zindex: Option<u32>,
    pub(crate) filetypes_disabled: Option<Vec<String>>,
    pub(crate) logging_level: Option<i64>,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct ColorOptionsPatch {
    pub(crate) cursor_color: Option<OptionalChange<String>>,
    pub(crate) cursor_color_insert_mode: Option<OptionalChange<String>>,
    pub(crate) normal_bg: Option<OptionalChange<String>>,
    pub(crate) transparent_bg_fallback_color: Option<String>,
    pub(crate) cterm_bg: Option<OptionalChange<u16>>,
    pub(crate) cterm_cursor_colors: Option<OptionalChange<CtermCursorColorsPatch>>,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct SmearBehaviorPatch {
    pub(crate) smear_between_windows: Option<bool>,
    pub(crate) smear_between_buffers: Option<bool>,
    pub(crate) smear_between_neighbor_lines: Option<bool>,
    pub(crate) min_horizontal_distance_smear: Option<f64>,
    pub(crate) min_vertical_distance_smear: Option<f64>,
    pub(crate) smear_horizontally: Option<bool>,
    pub(crate) smear_vertically: Option<bool>,
    pub(crate) smear_diagonally: Option<bool>,
    pub(crate) scroll_buffer_space: Option<bool>,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct MotionOptionsPatch {
    pub(crate) anticipation: Option<f64>,
    pub(crate) head_response_ms: Option<f64>,
    pub(crate) damping_ratio: Option<f64>,
    pub(crate) tail_response_ms: Option<f64>,
    pub(crate) stop_distance_enter: Option<f64>,
    pub(crate) stop_distance_exit: Option<f64>,
    pub(crate) stop_velocity_enter: Option<f64>,
    pub(crate) stop_hold_frames: Option<u32>,
    pub(crate) max_length: Option<f64>,
    pub(crate) max_length_insert_mode: Option<f64>,
    pub(crate) trail_duration_ms: Option<f64>,
    pub(crate) trail_short_duration_ms: Option<f64>,
    pub(crate) trail_size: Option<f64>,
    pub(crate) trail_min_distance: Option<f64>,
    pub(crate) trail_thickness: Option<f64>,
    pub(crate) trail_thickness_x: Option<f64>,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct ParticleOptionsPatch {
    pub(crate) particles_enabled: Option<bool>,
    pub(crate) particle_max_num: Option<usize>,
    pub(crate) particle_spread: Option<f64>,
    pub(crate) particles_per_second: Option<f64>,
    pub(crate) particles_per_length: Option<f64>,
    pub(crate) particle_max_lifetime: Option<f64>,
    pub(crate) particle_lifetime_distribution_exponent: Option<f64>,
    pub(crate) particle_max_initial_velocity: Option<f64>,
    pub(crate) particle_velocity_from_cursor: Option<f64>,
    pub(crate) particle_random_velocity: Option<f64>,
    pub(crate) particle_damping: Option<f64>,
    pub(crate) particle_gravity: Option<f64>,
    pub(crate) min_distance_emit_particles: Option<f64>,
    pub(crate) particle_switch_octant_braille: Option<f64>,
    pub(crate) particles_over_text: Option<bool>,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct RenderingOptionsPatch {
    pub(crate) never_draw_over_target: Option<bool>,
    pub(crate) color_levels: Option<u32>,
    pub(crate) gamma: Option<f64>,
    pub(crate) tail_duration_ms: Option<f64>,
    pub(crate) spatial_coherence_weight: Option<f64>,
    pub(crate) temporal_stability_weight: Option<f64>,
    pub(crate) top_k_per_cell: Option<u8>,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct RuntimeOptionsPatch {
    pub(crate) runtime: RuntimeSwitchesPatch,
    pub(crate) color: ColorOptionsPatch,
    pub(crate) smear: SmearBehaviorPatch,
    pub(crate) motion: MotionOptionsPatch,
    pub(crate) particles: ParticleOptionsPatch,
    pub(crate) rendering: RenderingOptionsPatch,
}

fn apply_optional_change<T>(target: &mut Option<T>, change: OptionalChange<T>) {
    match change {
        OptionalChange::Set(value) => *target = Some(value),
        OptionalChange::Clear => *target = None,
    }
}

fn apply_value<T>(target: &mut T, patch: &mut Option<T>) {
    if let Some(value) = patch.take() {
        *target = value;
    }
}

fn apply_optional_value<T>(target: &mut Option<T>, patch: &mut Option<OptionalChange<T>>) {
    if let Some(change) = patch.take() {
        apply_optional_change(target, change);
    }
}

macro_rules! apply_config_fields {
    ($config:expr, $patch:expr, [$($field:ident),+ $(,)?]) => {
        $(
            apply_value(&mut $config.$field, &mut $patch.$field);
        )+
    };
}

impl RuntimeOptionsPatch {
    pub(crate) fn validate_against(&self, config: &RuntimeConfig) -> Result<()> {
        let head_response_ms = self
            .motion
            .head_response_ms
            .unwrap_or(config.head_response_ms);
        let tail_response_ms = self
            .motion
            .tail_response_ms
            .unwrap_or(config.tail_response_ms);
        if tail_response_ms < head_response_ms {
            return Err(invalid_key(
                "tail_response_ms",
                "positive number greater than or equal to head_response_ms",
            ));
        }

        let damping_ratio = self.motion.damping_ratio.unwrap_or(config.damping_ratio);
        if !damping_ratio.is_finite() || damping_ratio <= 0.0 {
            return Err(invalid_key("damping_ratio", "positive number"));
        }

        let stop_distance_enter = self
            .motion
            .stop_distance_enter
            .unwrap_or(config.stop_distance_enter);
        let stop_distance_exit = self
            .motion
            .stop_distance_exit
            .unwrap_or(config.stop_distance_exit);
        if stop_distance_exit < stop_distance_enter {
            return Err(invalid_key(
                "stop_distance_exit",
                "non-negative number greater than or equal to stop_distance_enter",
            ));
        }

        let trail_size = self.motion.trail_size.unwrap_or(config.trail_size);
        if !(0.0..=1.0).contains(&trail_size) {
            return Err(invalid_key("trail_size", "number between 0.0 and 1.0"));
        }
        let trail_short_duration_ms = self
            .motion
            .trail_short_duration_ms
            .unwrap_or(config.trail_short_duration_ms);
        if !trail_short_duration_ms.is_finite() || trail_short_duration_ms <= 0.0 {
            return Err(invalid_key("trail_short_duration_ms", "positive number"));
        }

        let top_k_per_cell = self
            .rendering
            .top_k_per_cell
            .unwrap_or(config.top_k_per_cell);
        if top_k_per_cell < 2 {
            return Err(invalid_key(
                "top_k_per_cell",
                "integer greater than or equal to 2",
            ));
        }

        Ok(())
    }

    pub(crate) fn apply(mut self, state: &mut RuntimeState) -> RuntimeOptionsEffects {
        let mut effects = RuntimeOptionsEffects::default();

        self.runtime.apply(state, &mut effects);
        self.color.apply(&mut state.config);
        self.smear.apply(&mut state.config);
        self.motion.apply(&mut state.config);
        self.particles.apply(&mut state.config);
        self.rendering.apply(&mut state.config);

        if !state.config.requires_cursor_color_sampling() {
            state.clear_color_at_cursor();
        }
        state.refresh_render_static_config();

        effects
    }
}

impl RuntimeSwitchesPatch {
    fn apply(&mut self, state: &mut RuntimeState, effects: &mut RuntimeOptionsEffects) {
        if let Some(value) = self.enabled.take() {
            state.set_enabled(value);
        }

        let config = &mut state.config;
        if let Some(value) = self.time_interval.take() {
            config.time_interval = value;
            config.fps = (1000.0 / value).max(1.0);
        }
        if let Some(value) = self.fps.take() {
            config.fps = value;
            config.time_interval = RuntimeConfig::interval_ms_for_fps(value);
        }
        apply_config_fields!(
            config,
            self,
            [
                simulation_hz,
                max_simulation_steps_per_frame,
                delay_event_to_smear,
                delay_after_key,
                smear_to_cmd,
                smear_insert_mode,
                smear_replace_mode,
                smear_terminal_mode,
                animate_in_insert_mode,
                animate_command_line,
                vertical_bar_cursor,
                vertical_bar_cursor_insert_mode,
                horizontal_bar_cursor_replace_mode,
                hide_target_hack,
                max_kept_windows,
                windows_zindex,
            ]
        );
        if let Some(value) = self.filetypes_disabled.take() {
            config.filetypes_disabled = Arc::from(value);
        }

        if let Some(value) = self.logging_level.take() {
            config.logging_level = value;
            effects.logging_level = Some(value);
        }
    }
}

impl ColorOptionsPatch {
    fn apply(&mut self, config: &mut RuntimeConfig) {
        apply_optional_value(&mut config.cursor_color, &mut self.cursor_color);
        apply_optional_value(
            &mut config.cursor_color_insert_mode,
            &mut self.cursor_color_insert_mode,
        );
        apply_optional_value(&mut config.normal_bg, &mut self.normal_bg);
        apply_value(
            &mut config.transparent_bg_fallback_color,
            &mut self.transparent_bg_fallback_color,
        );
        apply_optional_value(&mut config.cterm_bg, &mut self.cterm_bg);
        if let Some(change) = self.cterm_cursor_colors.take() {
            match change {
                OptionalChange::Set(patch) => {
                    config.color_levels = patch.color_levels;
                    config.cterm_cursor_colors = Some(patch.colors);
                }
                OptionalChange::Clear => config.cterm_cursor_colors = None,
            }
        }
    }
}

impl SmearBehaviorPatch {
    fn apply(&mut self, config: &mut RuntimeConfig) {
        apply_config_fields!(
            config,
            self,
            [
                smear_between_windows,
                smear_between_buffers,
                smear_between_neighbor_lines,
                min_horizontal_distance_smear,
                min_vertical_distance_smear,
                smear_horizontally,
                smear_vertically,
                smear_diagonally,
                scroll_buffer_space,
            ]
        );
    }
}

impl MotionOptionsPatch {
    fn apply(&mut self, config: &mut RuntimeConfig) {
        apply_config_fields!(
            config,
            self,
            [
                anticipation,
                head_response_ms,
                damping_ratio,
                tail_response_ms,
                stop_distance_enter,
                stop_distance_exit,
                stop_velocity_enter,
                stop_hold_frames,
                max_length,
                max_length_insert_mode,
                trail_duration_ms,
                trail_short_duration_ms,
                trail_size,
                trail_min_distance,
                trail_thickness,
                trail_thickness_x,
            ]
        );
    }
}

impl ParticleOptionsPatch {
    fn apply(&mut self, config: &mut RuntimeConfig) {
        apply_config_fields!(
            config,
            self,
            [
                particles_enabled,
                particle_max_num,
                particle_spread,
                particles_per_second,
                particles_per_length,
                particle_max_lifetime,
                particle_lifetime_distribution_exponent,
                particle_max_initial_velocity,
                particle_velocity_from_cursor,
                particle_random_velocity,
                particle_damping,
                particle_gravity,
                min_distance_emit_particles,
                particle_switch_octant_braille,
                particles_over_text,
            ]
        );
    }
}

impl RenderingOptionsPatch {
    fn apply(&mut self, config: &mut RuntimeConfig) {
        apply_config_fields!(
            config,
            self,
            [
                never_draw_over_target,
                color_levels,
                gamma,
                tail_duration_ms,
                spatial_coherence_weight,
                temporal_stability_weight,
                top_k_per_cell,
            ]
        );
    }
}
