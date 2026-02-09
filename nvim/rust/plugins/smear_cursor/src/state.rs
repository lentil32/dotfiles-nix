use crate::animation::{center, corners_for_cursor, zero_velocity_corners};
use crate::config::RuntimeConfig;
use crate::types::{DEFAULT_RNG_STATE, Particle, Point};

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
    pub(crate) namespace_id: Option<u32>,
    pub(crate) current_corners: [Point; 4],
    pub(crate) target_corners: [Point; 4],
    pub(crate) target_position: Point,
    pub(crate) velocity_corners: [Point; 4],
    pub(crate) stiffnesses: [f64; 4],
    pub(crate) particles: Vec<Particle>,
    pub(crate) previous_center: Point,
    pub(crate) rng_state: u32,
    timing: AnimationTiming,
    tracking: CursorTracking,
    cursor_visibility: CursorVisibility,
    pending_external_event: Option<CursorSnapshot>,
    pub(crate) color_at_cursor: Option<String>,
}

impl RuntimeState {
    pub(crate) fn is_enabled(&self) -> bool {
        self.plugin_state.is_enabled()
    }

    pub(crate) fn set_enabled(&mut self, enabled: bool) {
        self.plugin_state = PluginState::from_enabled(enabled);
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

    pub(crate) fn start_animation(&mut self) {
        self.mark_initialized();
        self.animation_state = AnimationState::Running;
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
        self.color_at_cursor = None;
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
    use crate::animation::corners_for_cursor;
    use crate::types::Point;

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
        let mut state = RuntimeState::default();
        state.target_position = Point { row: 4.0, col: 9.0 };
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
}
