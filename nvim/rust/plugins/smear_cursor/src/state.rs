use crate::animation::{center, corners_for_cursor, initial_velocity, zero_velocity_corners};
use crate::config::RuntimeConfig;
use crate::types::{DEFAULT_RNG_STATE, Particle, Point, StepOutput};

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CursorSnapshot {
    pub(crate) mode: String,
    pub(crate) row: f64,
    pub(crate) col: f64,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct CursorLocation {
    pub(crate) window_handle: i64,
    pub(crate) buffer_handle: i64,
    pub(crate) top_row: i64,
    pub(crate) line: i64,
}

impl CursorLocation {
    pub(crate) const fn new(
        window_handle: i64,
        buffer_handle: i64,
        top_row: i64,
        line: i64,
    ) -> Self {
        Self {
            window_handle,
            buffer_handle,
            top_row,
            line,
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct CursorShape {
    vertical_bar: bool,
    horizontal_bar: bool,
}

impl CursorShape {
    pub(crate) const fn new(vertical_bar: bool, horizontal_bar: bool) -> Self {
        Self {
            vertical_bar,
            horizontal_bar,
        }
    }

    fn corners(self, position: Point) -> [Point; 4] {
        corners_for_cursor(
            position.row,
            position.col,
            self.vertical_bar,
            self.horizontal_bar,
        )
    }
}

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
    pub(crate) delay_disable: Option<OptionalChange<f64>>,
    pub(crate) delay_event_to_smear: Option<f64>,
    pub(crate) delay_after_key: Option<f64>,
    pub(crate) smear_to_cmd: Option<bool>,
    pub(crate) smear_insert_mode: Option<bool>,
    pub(crate) smear_replace_mode: Option<bool>,
    pub(crate) smear_terminal_mode: Option<bool>,
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
    pub(crate) stiffness: Option<f64>,
    pub(crate) trailing_stiffness: Option<f64>,
    pub(crate) trailing_exponent: Option<f64>,
    pub(crate) stiffness_insert_mode: Option<f64>,
    pub(crate) trailing_stiffness_insert_mode: Option<f64>,
    pub(crate) trailing_exponent_insert_mode: Option<f64>,
    pub(crate) anticipation: Option<f64>,
    pub(crate) damping: Option<f64>,
    pub(crate) damping_insert_mode: Option<f64>,
    pub(crate) distance_stop_animating: Option<f64>,
    pub(crate) distance_stop_animating_vertical_bar: Option<f64>,
    pub(crate) max_length: Option<f64>,
    pub(crate) max_length_insert_mode: Option<f64>,
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
    pub(crate) volume_reduction_exponent: Option<f64>,
    pub(crate) minimum_volume_factor: Option<f64>,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct RenderingOptionsPatch {
    pub(crate) never_draw_over_target: Option<bool>,
    pub(crate) use_diagonal_blocks: Option<bool>,
    pub(crate) max_slope_horizontal: Option<f64>,
    pub(crate) min_slope_vertical: Option<f64>,
    pub(crate) max_angle_difference_diagonal: Option<f64>,
    pub(crate) max_offset_diagonal: Option<f64>,
    pub(crate) min_shade_no_diagonal: Option<f64>,
    pub(crate) min_shade_no_diagonal_vertical_bar: Option<f64>,
    pub(crate) max_shade_no_matrix: Option<f64>,
    pub(crate) color_levels: Option<u32>,
    pub(crate) gamma: Option<f64>,
    pub(crate) gradient_exponent: Option<f64>,
    pub(crate) matrix_pixel_threshold: Option<f64>,
    pub(crate) matrix_pixel_threshold_vertical_bar: Option<f64>,
    pub(crate) matrix_pixel_min_factor: Option<f64>,
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

impl RuntimeOptionsPatch {
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

        effects
    }
}

impl RuntimeSwitchesPatch {
    fn apply(&mut self, state: &mut RuntimeState, effects: &mut RuntimeOptionsEffects) {
        if let Some(value) = self.enabled.take() {
            state.set_enabled(value);
        }

        let config = &mut state.config;
        apply_value(&mut config.time_interval, &mut self.time_interval);
        if let Some(change) = self.delay_disable.take() {
            apply_optional_change(&mut config.delay_disable, change);
        }
        apply_value(
            &mut config.delay_event_to_smear,
            &mut self.delay_event_to_smear,
        );
        apply_value(&mut config.delay_after_key, &mut self.delay_after_key);
        apply_value(&mut config.smear_to_cmd, &mut self.smear_to_cmd);
        apply_value(&mut config.smear_insert_mode, &mut self.smear_insert_mode);
        apply_value(&mut config.smear_replace_mode, &mut self.smear_replace_mode);
        apply_value(
            &mut config.smear_terminal_mode,
            &mut self.smear_terminal_mode,
        );
        apply_value(
            &mut config.vertical_bar_cursor,
            &mut self.vertical_bar_cursor,
        );
        apply_value(
            &mut config.vertical_bar_cursor_insert_mode,
            &mut self.vertical_bar_cursor_insert_mode,
        );
        apply_value(
            &mut config.horizontal_bar_cursor_replace_mode,
            &mut self.horizontal_bar_cursor_replace_mode,
        );
        apply_value(&mut config.hide_target_hack, &mut self.hide_target_hack);
        apply_value(&mut config.max_kept_windows, &mut self.max_kept_windows);
        apply_value(&mut config.windows_zindex, &mut self.windows_zindex);
        apply_value(&mut config.filetypes_disabled, &mut self.filetypes_disabled);

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
        apply_value(
            &mut config.smear_between_buffers,
            &mut self.smear_between_buffers,
        );
        apply_value(
            &mut config.smear_between_neighbor_lines,
            &mut self.smear_between_neighbor_lines,
        );
        apply_value(
            &mut config.min_horizontal_distance_smear,
            &mut self.min_horizontal_distance_smear,
        );
        apply_value(
            &mut config.min_vertical_distance_smear,
            &mut self.min_vertical_distance_smear,
        );
        apply_value(&mut config.smear_horizontally, &mut self.smear_horizontally);
        apply_value(&mut config.smear_vertically, &mut self.smear_vertically);
        apply_value(&mut config.smear_diagonally, &mut self.smear_diagonally);
        apply_value(
            &mut config.scroll_buffer_space,
            &mut self.scroll_buffer_space,
        );
    }
}

impl MotionOptionsPatch {
    fn apply(&mut self, config: &mut RuntimeConfig) {
        apply_value(&mut config.stiffness, &mut self.stiffness);
        apply_value(&mut config.trailing_stiffness, &mut self.trailing_stiffness);
        apply_value(&mut config.trailing_exponent, &mut self.trailing_exponent);
        apply_value(
            &mut config.stiffness_insert_mode,
            &mut self.stiffness_insert_mode,
        );
        apply_value(
            &mut config.trailing_stiffness_insert_mode,
            &mut self.trailing_stiffness_insert_mode,
        );
        apply_value(
            &mut config.trailing_exponent_insert_mode,
            &mut self.trailing_exponent_insert_mode,
        );
        apply_value(&mut config.anticipation, &mut self.anticipation);
        apply_value(&mut config.damping, &mut self.damping);
        apply_value(
            &mut config.damping_insert_mode,
            &mut self.damping_insert_mode,
        );
        apply_value(
            &mut config.distance_stop_animating,
            &mut self.distance_stop_animating,
        );
        apply_value(
            &mut config.distance_stop_animating_vertical_bar,
            &mut self.distance_stop_animating_vertical_bar,
        );
        apply_value(&mut config.max_length, &mut self.max_length);
        apply_value(
            &mut config.max_length_insert_mode,
            &mut self.max_length_insert_mode,
        );
    }
}

impl ParticleOptionsPatch {
    fn apply(&mut self, config: &mut RuntimeConfig) {
        apply_value(&mut config.particles_enabled, &mut self.particles_enabled);
        apply_value(&mut config.particle_max_num, &mut self.particle_max_num);
        apply_value(&mut config.particle_spread, &mut self.particle_spread);
        apply_value(
            &mut config.particles_per_second,
            &mut self.particles_per_second,
        );
        apply_value(
            &mut config.particles_per_length,
            &mut self.particles_per_length,
        );
        apply_value(
            &mut config.particle_max_lifetime,
            &mut self.particle_max_lifetime,
        );
        apply_value(
            &mut config.particle_lifetime_distribution_exponent,
            &mut self.particle_lifetime_distribution_exponent,
        );
        apply_value(
            &mut config.particle_max_initial_velocity,
            &mut self.particle_max_initial_velocity,
        );
        apply_value(
            &mut config.particle_velocity_from_cursor,
            &mut self.particle_velocity_from_cursor,
        );
        apply_value(
            &mut config.particle_random_velocity,
            &mut self.particle_random_velocity,
        );
        apply_value(&mut config.particle_damping, &mut self.particle_damping);
        apply_value(&mut config.particle_gravity, &mut self.particle_gravity);
        apply_value(
            &mut config.min_distance_emit_particles,
            &mut self.min_distance_emit_particles,
        );
        apply_value(
            &mut config.particle_switch_octant_braille,
            &mut self.particle_switch_octant_braille,
        );
        apply_value(
            &mut config.particles_over_text,
            &mut self.particles_over_text,
        );
        apply_value(
            &mut config.volume_reduction_exponent,
            &mut self.volume_reduction_exponent,
        );
        apply_value(
            &mut config.minimum_volume_factor,
            &mut self.minimum_volume_factor,
        );
    }
}

impl RenderingOptionsPatch {
    fn apply(&mut self, config: &mut RuntimeConfig) {
        apply_value(
            &mut config.never_draw_over_target,
            &mut self.never_draw_over_target,
        );
        apply_value(
            &mut config.use_diagonal_blocks,
            &mut self.use_diagonal_blocks,
        );
        apply_value(
            &mut config.max_slope_horizontal,
            &mut self.max_slope_horizontal,
        );
        apply_value(&mut config.min_slope_vertical, &mut self.min_slope_vertical);
        apply_value(
            &mut config.max_angle_difference_diagonal,
            &mut self.max_angle_difference_diagonal,
        );
        apply_value(
            &mut config.max_offset_diagonal,
            &mut self.max_offset_diagonal,
        );
        apply_value(
            &mut config.min_shade_no_diagonal,
            &mut self.min_shade_no_diagonal,
        );
        apply_value(
            &mut config.min_shade_no_diagonal_vertical_bar,
            &mut self.min_shade_no_diagonal_vertical_bar,
        );
        apply_value(
            &mut config.max_shade_no_matrix,
            &mut self.max_shade_no_matrix,
        );
        apply_value(&mut config.color_levels, &mut self.color_levels);
        apply_value(&mut config.gamma, &mut self.gamma);
        apply_value(&mut config.gradient_exponent, &mut self.gradient_exponent);
        apply_value(
            &mut config.matrix_pixel_threshold,
            &mut self.matrix_pixel_threshold,
        );
        apply_value(
            &mut config.matrix_pixel_threshold_vertical_bar,
            &mut self.matrix_pixel_threshold_vertical_bar,
        );
        apply_value(
            &mut config.matrix_pixel_min_factor,
            &mut self.matrix_pixel_min_factor,
        );
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum PluginState {
    Enabled,
    Disabled,
}

impl PluginState {
    fn from_enabled(enabled: bool) -> Self {
        if enabled {
            Self::Enabled
        } else {
            Self::Disabled
        }
    }

    fn is_enabled(self) -> bool {
        matches!(self, Self::Enabled)
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum AnimationState {
    Uninitialized,
    Idle,
    Running,
}

impl AnimationState {
    fn is_initialized(self) -> bool {
        !matches!(self, Self::Uninitialized)
    }

    fn is_animating(self) -> bool {
        matches!(self, Self::Running)
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum BufferState {
    Active,
    DelayDisabled,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum CursorVisibility {
    Visible,
    Hidden,
}

#[derive(Debug, Clone, Copy, Default)]
struct CursorTracking {
    location: Option<CursorLocation>,
}

impl CursorTracking {
    fn update(&mut self, location: CursorLocation) {
        self.location = Some(location);
    }

    fn tracked_location(self) -> Option<CursorLocation> {
        self.location
    }

    fn clear(&mut self) {
        self.location = None;
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct AnimationTiming {
    last_tick_ms: Option<f64>,
    lag_ms: f64,
}

impl AnimationTiming {
    fn reset(&mut self) {
        *self = Self::default();
    }
}

#[derive(Debug)]
pub(crate) struct RuntimeState {
    pub(crate) config: RuntimeConfig,
    plugin_state: PluginState,
    animation_state: AnimationState,
    buffer_state: BufferState,
    namespace_id: Option<u32>,
    current_corners: [Point; 4],
    target_corners: [Point; 4],
    target_position: Point,
    velocity_corners: [Point; 4],
    stiffnesses: [f64; 4],
    particles: Vec<Particle>,
    previous_center: Point,
    rng_state: u32,
    timing: AnimationTiming,
    tracking: CursorTracking,
    cursor_visibility: CursorVisibility,
    pending_external_event: Option<CursorSnapshot>,
    color_at_cursor: Option<String>,
}

impl RuntimeState {
    pub(crate) fn is_enabled(&self) -> bool {
        self.plugin_state.is_enabled()
    }

    pub(crate) fn set_enabled(&mut self, enabled: bool) {
        self.plugin_state = PluginState::from_enabled(enabled);
    }

    pub(crate) fn apply_runtime_options_patch(
        &mut self,
        patch: RuntimeOptionsPatch,
    ) -> RuntimeOptionsEffects {
        patch.apply(self)
    }

    pub(crate) fn namespace_id(&self) -> Option<u32> {
        self.namespace_id
    }

    pub(crate) fn set_namespace_id(&mut self, namespace_id: u32) {
        self.namespace_id = Some(namespace_id);
    }

    pub(crate) fn is_initialized(&self) -> bool {
        self.animation_state.is_initialized()
    }

    pub(crate) fn mark_initialized(&mut self) {
        if self.animation_state == AnimationState::Uninitialized {
            self.animation_state = AnimationState::Idle;
        }
    }

    pub(crate) fn clear_initialization(&mut self) {
        self.animation_state = AnimationState::Uninitialized;
    }

    pub(crate) fn is_animating(&self) -> bool {
        self.animation_state.is_animating()
    }

    pub(crate) fn current_corners(&self) -> [Point; 4] {
        self.current_corners
    }

    pub(crate) fn target_corners(&self) -> [Point; 4] {
        self.target_corners
    }

    pub(crate) fn target_position(&self) -> Point {
        self.target_position
    }

    pub(crate) fn velocity_corners(&self) -> [Point; 4] {
        self.velocity_corners
    }

    pub(crate) fn stiffnesses(&self) -> [f64; 4] {
        self.stiffnesses
    }

    pub(crate) fn set_stiffnesses(&mut self, stiffnesses: [f64; 4]) {
        self.stiffnesses = stiffnesses;
    }

    pub(crate) fn particles(&self) -> &[Particle] {
        &self.particles
    }

    pub(crate) fn take_particles(&mut self) -> Vec<Particle> {
        std::mem::take(&mut self.particles)
    }

    pub(crate) fn previous_center(&self) -> Point {
        self.previous_center
    }

    pub(crate) fn rng_state(&self) -> u32 {
        self.rng_state
    }

    pub(crate) fn color_at_cursor(&self) -> Option<&str> {
        self.color_at_cursor.as_deref()
    }

    pub(crate) fn set_color_at_cursor(&mut self, color: Option<String>) {
        self.color_at_cursor = color;
    }

    pub(crate) fn clear_color_at_cursor(&mut self) {
        self.color_at_cursor = None;
    }

    pub(crate) fn start_animation(&mut self) {
        self.mark_initialized();
        self.animation_state = AnimationState::Running;
    }

    pub(crate) fn start_animation_towards_target(&mut self) {
        self.velocity_corners = initial_velocity(
            &self.current_corners,
            &self.target_corners,
            self.config.anticipation,
        );
        self.start_animation();
    }

    pub(crate) fn stop_animation(&mut self) {
        if self.animation_state == AnimationState::Running {
            self.animation_state = AnimationState::Idle;
        }
    }

    pub(crate) fn last_tick_ms(&self) -> Option<f64> {
        self.timing.last_tick_ms
    }

    pub(crate) fn set_last_tick_ms(&mut self, value: Option<f64>) {
        self.timing.last_tick_ms = value;
    }

    pub(crate) fn lag_ms(&self) -> f64 {
        self.timing.lag_ms
    }

    pub(crate) fn set_lag_ms(&mut self, value: f64) {
        self.timing.lag_ms = value;
    }

    pub(crate) fn reset_animation_timing(&mut self) {
        self.timing.reset();
    }

    pub(crate) fn is_delay_disabled(&self) -> bool {
        matches!(self.buffer_state, BufferState::DelayDisabled)
    }

    pub(crate) fn set_delay_disabled(&mut self, disabled: bool) {
        self.buffer_state = if disabled {
            BufferState::DelayDisabled
        } else {
            BufferState::Active
        };
    }

    pub(crate) fn tracked_location(&self) -> Option<CursorLocation> {
        self.tracking.tracked_location()
    }

    pub(crate) fn update_tracking(&mut self, location: CursorLocation) {
        self.tracking.update(location);
    }

    pub(crate) fn clear_tracking(&mut self) {
        self.tracking.clear();
    }

    pub(crate) fn is_cursor_hidden(&self) -> bool {
        matches!(self.cursor_visibility, CursorVisibility::Hidden)
    }

    pub(crate) fn set_cursor_hidden(&mut self, hidden: bool) {
        self.cursor_visibility = if hidden {
            CursorVisibility::Hidden
        } else {
            CursorVisibility::Visible
        };
    }

    pub(crate) fn pending_external_event_cloned(&self) -> Option<CursorSnapshot> {
        self.pending_external_event.clone()
    }

    pub(crate) fn set_pending_external_event(&mut self, snapshot: Option<CursorSnapshot>) {
        self.pending_external_event = snapshot;
    }

    pub(crate) fn clear_pending_external_event(&mut self) {
        self.pending_external_event = None;
    }

    pub(crate) fn apply_scroll_shift(
        &mut self,
        shift: f64,
        min_row: f64,
        max_row: f64,
        vertical_bar: bool,
        horizontal_bar: bool,
    ) {
        let shifted_row = (self.current_corners[0].row - shift)
            .max(min_row)
            .min(max_row);
        let shifted_col = self.current_corners[0].col;
        self.current_corners =
            corners_for_cursor(shifted_row, shifted_col, vertical_bar, horizontal_bar);
        self.previous_center = center(&self.current_corners);
        for particle in &mut self.particles {
            particle.position.row -= shift;
        }
    }

    pub(crate) fn apply_step_output(&mut self, output: StepOutput) {
        self.current_corners = output.current_corners;
        self.velocity_corners = output.velocity_corners;
        self.previous_center = output.previous_center;
        self.rng_state = output.rng_state;
        self.particles = output.particles;
    }

    pub(crate) fn settle_at_target(&mut self) {
        self.current_corners = self.target_corners;
        self.velocity_corners = zero_velocity_corners();
    }

    fn sync_cursor_geometry(&mut self, position: Point, shape: CursorShape) {
        let corners = shape.corners(position);
        self.current_corners = corners;
        self.target_corners = corners;
        self.target_position = position;
        self.previous_center = center(&self.current_corners);
    }

    pub(crate) fn set_target(&mut self, position: Point, shape: CursorShape) {
        self.target_position = position;
        self.target_corners = shape.corners(position);
    }

    pub(crate) fn initialize_cursor(
        &mut self,
        position: Point,
        shape: CursorShape,
        seed: u32,
        location: CursorLocation,
    ) {
        self.sync_cursor_geometry(position, shape);
        self.velocity_corners = zero_velocity_corners();
        self.particles.clear();
        self.rng_state = seed;
        self.set_delay_disabled(false);
        self.mark_initialized();
        self.stop_animation();
        self.reset_animation_timing();
        self.update_tracking(location);
        self.set_cursor_hidden(false);
    }

    pub(crate) fn jump_preserving_motion(
        &mut self,
        position: Point,
        shape: CursorShape,
        location: CursorLocation,
    ) {
        self.sync_cursor_geometry(position, shape);
        self.mark_initialized();
        self.update_tracking(location);
    }

    pub(crate) fn jump_and_stop_animation(
        &mut self,
        position: Point,
        shape: CursorShape,
        location: CursorLocation,
    ) {
        self.sync_cursor_geometry(position, shape);
        self.velocity_corners = zero_velocity_corners();
        self.stop_animation();
        self.reset_animation_timing();
        self.update_tracking(location);
    }

    pub(crate) fn sync_to_current_cursor(
        &mut self,
        position: Point,
        shape: CursorShape,
        location: CursorLocation,
    ) -> bool {
        let was_hidden = self.is_cursor_hidden();
        self.sync_cursor_geometry(position, shape);
        self.velocity_corners = zero_velocity_corners();
        self.particles.clear();
        self.stop_animation();
        self.mark_initialized();
        self.clear_pending_external_event();
        self.update_tracking(location);
        self.reset_animation_timing();
        self.set_cursor_hidden(false);
        was_hidden
    }

    pub(crate) fn clear_runtime_state(&mut self) {
        self.clear_initialization();
        self.stop_animation();
        self.reset_transient_state();
    }

    pub(crate) fn disable(&mut self) {
        self.set_enabled(false);
        self.clear_runtime_state();
    }

    pub(crate) fn reset_transient_state(&mut self) {
        self.target_position = Point::ZERO;
        self.reset_animation_timing();
        self.set_delay_disabled(false);
        self.clear_tracking();
        self.set_cursor_hidden(false);
        self.clear_pending_external_event();
        self.clear_color_at_cursor();
    }
}

impl Default for RuntimeState {
    fn default() -> Self {
        Self {
            config: RuntimeConfig::default(),
            plugin_state: PluginState::Enabled,
            animation_state: AnimationState::Uninitialized,
            buffer_state: BufferState::Active,
            namespace_id: None,
            current_corners: [Point::ZERO; 4],
            target_corners: [Point::ZERO; 4],
            target_position: Point::ZERO,
            velocity_corners: [Point::ZERO; 4],
            stiffnesses: [0.6; 4],
            particles: Vec::new(),
            previous_center: Point::ZERO,
            rng_state: DEFAULT_RNG_STATE,
            timing: AnimationTiming::default(),
            tracking: CursorTracking::default(),
            cursor_visibility: CursorVisibility::Visible,
            pending_external_event: None,
            color_at_cursor: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CursorLocation, CursorShape, CursorSnapshot, RuntimeState};
    use crate::animation::{center, corners_for_cursor, initial_velocity};
    use crate::types::{Particle, Point, StepOutput};

    #[test]
    fn animation_phase_transitions_preserve_invariants() {
        let mut state = RuntimeState::default();
        assert!(state.is_enabled());
        assert!(!state.is_initialized());
        assert!(!state.is_animating());

        state.mark_initialized();
        assert!(state.is_initialized());
        assert!(!state.is_animating());

        state.start_animation();
        assert!(state.is_initialized());
        assert!(state.is_animating());

        state.stop_animation();
        assert!(state.is_initialized());
        assert!(!state.is_animating());

        state.clear_initialization();
        assert!(!state.is_initialized());
        assert!(!state.is_animating());
    }

    #[test]
    fn transient_reset_clears_only_transient_fields() {
        let mut state = RuntimeState {
            target_position: Point { row: 4.0, col: 9.0 },
            ..RuntimeState::default()
        };
        state.set_delay_disabled(true);
        state.update_tracking(CursorLocation::new(11, 22, 33, 44));
        state.set_cursor_hidden(true);
        state.set_pending_external_event(Some(CursorSnapshot {
            mode: "n".to_string(),
            row: 2.0,
            col: 3.0,
        }));
        state.color_at_cursor = Some("#ffffff".to_string());
        state.set_last_tick_ms(Some(99.0));
        state.set_lag_ms(17.0);

        state.reset_transient_state();

        assert_eq!(state.target_position, Point::ZERO);
        assert!(!state.is_delay_disabled());
        assert_eq!(state.tracked_location(), None);
        assert!(!state.is_cursor_hidden());
        assert_eq!(state.pending_external_event_cloned(), None);
        assert_eq!(state.color_at_cursor, None);
        assert_eq!(state.last_tick_ms(), None);
        assert_eq!(state.lag_ms(), 0.0);
    }

    #[test]
    fn tracked_location_retains_window_and_buffer_coordinates() {
        let mut state = RuntimeState::default();
        let location = CursorLocation::new(1, 2, 3, 4);
        assert_eq!(state.tracked_location(), None);

        state.update_tracking(location);
        assert_eq!(state.tracked_location(), Some(location));
    }

    #[test]
    fn initialize_cursor_resets_motion_and_tracks_cursor() {
        let mut state = RuntimeState::default();
        state.start_animation();
        state.velocity_corners = [Point { row: 1.0, col: 2.0 }; 4];
        state.particles.push(crate::types::Particle {
            position: Point { row: 0.0, col: 0.0 },
            velocity: Point { row: 0.0, col: 0.0 },
            lifetime: 1.0,
        });

        let location = CursorLocation::new(10, 20, 30, 40);
        let shape = CursorShape::new(false, false);
        let position = Point { row: 8.0, col: 9.0 };
        state.initialize_cursor(position, shape, 123, location);

        let expected_corners = corners_for_cursor(position.row, position.col, false, false);
        assert!(state.is_initialized());
        assert!(!state.is_animating());
        assert_eq!(state.current_corners, expected_corners);
        assert_eq!(state.target_corners, expected_corners);
        assert_eq!(state.target_position, position);
        assert_eq!(state.velocity_corners, [Point::ZERO; 4]);
        assert!(state.particles.is_empty());
        assert_eq!(state.rng_state, 123);
        assert_eq!(state.tracked_location(), Some(location));
        assert!(!state.is_delay_disabled());
        assert!(!state.is_cursor_hidden());
    }

    #[test]
    fn jump_preserving_motion_keeps_animation_running() {
        let mut state = RuntimeState::default();
        state.mark_initialized();
        state.start_animation();
        state.velocity_corners = [Point { row: 2.0, col: 3.0 }; 4];

        state.jump_preserving_motion(
            Point { row: 4.0, col: 5.0 },
            CursorShape::new(false, false),
            CursorLocation::new(1, 2, 3, 4),
        );

        assert!(state.is_animating());
        assert_eq!(state.velocity_corners, [Point { row: 2.0, col: 3.0 }; 4]);
    }

    #[test]
    fn sync_to_current_cursor_clears_pending_and_returns_hidden_state() {
        let mut state = RuntimeState::default();
        state.set_cursor_hidden(true);
        state.set_pending_external_event(Some(CursorSnapshot {
            mode: "n".to_string(),
            row: 1.0,
            col: 1.0,
        }));

        let was_hidden = state.sync_to_current_cursor(
            Point { row: 6.0, col: 7.0 },
            CursorShape::new(false, false),
            CursorLocation::new(1, 2, 3, 4),
        );

        assert!(was_hidden);
        assert!(!state.is_cursor_hidden());
        assert_eq!(state.pending_external_event_cloned(), None);
    }

    #[test]
    fn apply_scroll_shift_clamps_cursor_row_and_shifts_particles() {
        let mut state = RuntimeState::default();
        state.initialize_cursor(
            Point { row: 5.0, col: 9.0 },
            CursorShape::new(false, false),
            11,
            CursorLocation::new(1, 2, 3, 4),
        );
        state.particles.push(Particle {
            position: Point {
                row: 10.0,
                col: 2.0,
            },
            velocity: Point { row: 0.0, col: 0.0 },
            lifetime: 1.0,
        });

        state.apply_scroll_shift(10.0, 1.0, 99.0, false, false);

        let expected_corners = corners_for_cursor(1.0, 9.0, false, false);
        assert_eq!(state.current_corners(), expected_corners);
        assert_eq!(state.previous_center(), center(&expected_corners));
        assert_eq!(state.particles()[0].position.row, 0.0);
    }

    #[test]
    fn start_animation_towards_target_initializes_velocity_from_target_delta() {
        let mut state = RuntimeState::default();
        state.config.anticipation = 0.42;
        state.initialize_cursor(
            Point { row: 3.0, col: 4.0 },
            CursorShape::new(false, false),
            7,
            CursorLocation::new(1, 2, 3, 4),
        );
        state.set_target(Point { row: 8.0, col: 9.0 }, CursorShape::new(false, false));

        let expected_velocity = initial_velocity(
            &state.current_corners(),
            &state.target_corners(),
            state.config.anticipation,
        );
        state.start_animation_towards_target();

        assert!(state.is_animating());
        assert_eq!(state.velocity_corners(), expected_velocity);
    }

    #[test]
    fn apply_step_output_replaces_simulation_fields() {
        let mut state = RuntimeState::default();
        let output = StepOutput {
            current_corners: [Point { row: 4.0, col: 5.0 }; 4],
            velocity_corners: [Point { row: 1.0, col: 2.0 }; 4],
            particles: vec![Particle {
                position: Point { row: 6.0, col: 7.0 },
                velocity: Point {
                    row: 0.5,
                    col: 0.25,
                },
                lifetime: 0.75,
            }],
            previous_center: Point { row: 8.0, col: 9.0 },
            index_head: 0,
            index_tail: 3,
            disabled_due_to_delay: false,
            rng_state: 1234,
        };

        state.apply_step_output(output);

        assert_eq!(state.current_corners(), [Point { row: 4.0, col: 5.0 }; 4]);
        assert_eq!(state.velocity_corners(), [Point { row: 1.0, col: 2.0 }; 4]);
        assert_eq!(state.previous_center(), Point { row: 8.0, col: 9.0 });
        assert_eq!(state.rng_state(), 1234);
        assert_eq!(state.particles().len(), 1);
    }
}
