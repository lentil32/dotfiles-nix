use super::CursorLocation;
use super::CursorShape;
use super::RuntimeState;
use super::types::CursorTransitionPolicy;
use crate::animation::center;
use crate::animation::zero_velocity_corners;
use crate::types::Point;

impl RuntimeState {
    fn reset_trail_timeline_from_current(&mut self) {
        self.trail.origin_corners = self.current_corners;
        self.trail.elapsed_ms = [0.0; 4];
    }

    fn sync_cursor_geometry(&mut self, position: Point, shape: CursorShape) {
        if self.target.position != position || self.target.shape != shape {
            self.target.retarget_epoch = self.target.retarget_epoch.wrapping_add(1);
        }
        let corners = shape.corners(position);
        self.current_corners = corners;
        self.trail.origin_corners = corners;
        self.trail.elapsed_ms = [0.0; 4];
        self.target.position = position;
        self.target.shape = shape;
        self.previous_center = center(&self.current_corners);
    }

    fn apply_cursor_transition(
        &mut self,
        position: Point,
        shape: CursorShape,
        location: &CursorLocation,
        policy: CursorTransitionPolicy,
    ) {
        self.sync_cursor_geometry(position, shape);

        // Ordering is policy-specific and intentionally explicit: callers rely on these
        // lifecycle transitions for cursor visibility and animation state behavior.
        match policy {
            CursorTransitionPolicy::Initialize { seed } => {
                self.start_new_trail_stroke();
                self.velocity_corners = zero_velocity_corners();
                self.spring_velocity_corners = zero_velocity_corners();
                self.set_particles_vec(Vec::new());
                self.rng_state = seed;
                self.mark_initialized();
                self.clear_pending_target();
                self.stop_animation();
                self.reset_animation_timing();
                self.update_tracking(location);
            }
            CursorTransitionPolicy::JumpPreservingMotion => {
                self.start_new_trail_stroke();
                self.spring_velocity_corners = zero_velocity_corners();
                self.mark_initialized();
                self.clear_pending_target();
                self.update_tracking(location);
            }
            CursorTransitionPolicy::JumpAndStopAnimation => {
                self.start_new_trail_stroke();
                self.velocity_corners = zero_velocity_corners();
                self.spring_velocity_corners = zero_velocity_corners();
                self.clear_pending_target();
                self.stop_animation();
                self.reset_animation_timing();
                self.update_tracking(location);
            }
            CursorTransitionPolicy::SyncToCurrentCursor => {
                self.start_new_trail_stroke();
                self.velocity_corners = zero_velocity_corners();
                self.spring_velocity_corners = zero_velocity_corners();
                self.set_particles_vec(Vec::new());
                self.clear_pending_target();
                self.stop_animation();
                self.mark_initialized();
                self.update_tracking(location);
                self.reset_animation_timing();
            }
        }
    }

    pub(crate) fn set_target(&mut self, position: Point, shape: CursorShape) {
        let target_changed = self.target.position != position || self.target.shape != shape;
        if target_changed {
            self.target.retarget_epoch = self.target.retarget_epoch.wrapping_add(1);
            self.reset_trail_timeline_from_current();
        }
        self.target.position = position;
        self.target.shape = shape;
    }

    pub(crate) fn initialize_cursor(
        &mut self,
        position: Point,
        shape: CursorShape,
        seed: u32,
        location: &CursorLocation,
    ) {
        self.apply_cursor_transition(
            position,
            shape,
            location,
            CursorTransitionPolicy::Initialize { seed },
        );
    }

    pub(crate) fn jump_preserving_motion(
        &mut self,
        position: Point,
        shape: CursorShape,
        location: &CursorLocation,
    ) {
        self.apply_cursor_transition(
            position,
            shape,
            location,
            CursorTransitionPolicy::JumpPreservingMotion,
        );
    }

    pub(crate) fn jump_and_stop_animation(
        &mut self,
        position: Point,
        shape: CursorShape,
        location: &CursorLocation,
    ) {
        self.apply_cursor_transition(
            position,
            shape,
            location,
            CursorTransitionPolicy::JumpAndStopAnimation,
        );
    }

    pub(crate) fn sync_to_current_cursor(
        &mut self,
        position: Point,
        shape: CursorShape,
        location: &CursorLocation,
    ) {
        self.apply_cursor_transition(
            position,
            shape,
            location,
            CursorTransitionPolicy::SyncToCurrentCursor,
        );
    }
}
