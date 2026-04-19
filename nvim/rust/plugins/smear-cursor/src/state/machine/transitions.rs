use super::CursorShape;
use super::RuntimeState;
use super::RuntimeTargetSnapshot;
use super::TrackedCursor;
use super::types::CursorTransitionPolicy;
use crate::animation::center;
use crate::animation::zero_velocity_corners;
use crate::position::RenderPoint;

impl RuntimeState {
    fn reset_trail_timeline_from_current(&mut self) {
        self.trail.origin_corners = self.current_corners;
        self.trail.elapsed_ms = [0.0; 4];
    }

    pub(crate) fn retarget_preserving_current_pose(&mut self, snapshot: RuntimeTargetSnapshot) {
        if self.target.apply_snapshot(snapshot) {
            self.reset_trail_timeline_from_current();
        }
    }

    pub(crate) fn retarget_tracked_preserving_current_pose(
        &mut self,
        position: RenderPoint,
        shape: CursorShape,
        tracked_cursor: &TrackedCursor,
    ) {
        self.retarget_preserving_current_pose(RuntimeTargetSnapshot::tracked(
            position,
            shape,
            tracked_cursor,
        ));
    }

    fn sync_cursor_geometry(
        &mut self,
        position: RenderPoint,
        shape: CursorShape,
        tracked_cursor: &TrackedCursor,
    ) {
        self.target.apply_snapshot(RuntimeTargetSnapshot::tracked(
            position,
            shape,
            tracked_cursor,
        ));
        let corners = shape.corners(position);
        self.current_corners = corners;
        self.trail.origin_corners = corners;
        self.trail.elapsed_ms = [0.0; 4];
        self.previous_center = center(&self.current_corners);
    }

    fn apply_cursor_transition(
        &mut self,
        position: RenderPoint,
        shape: CursorShape,
        tracked_cursor: &TrackedCursor,
        policy: CursorTransitionPolicy,
    ) {
        self.sync_cursor_geometry(position, shape, tracked_cursor);

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
            }
            CursorTransitionPolicy::JumpPreservingMotion => {
                self.start_new_trail_stroke();
                self.spring_velocity_corners = zero_velocity_corners();
                self.mark_initialized();
                self.clear_pending_target();
            }
            CursorTransitionPolicy::JumpAndStopAnimation => {
                self.start_new_trail_stroke();
                self.velocity_corners = zero_velocity_corners();
                self.spring_velocity_corners = zero_velocity_corners();
                self.clear_pending_target();
                self.stop_animation();
                self.reset_animation_timing();
            }
            CursorTransitionPolicy::SyncToCurrentCursor => {
                self.start_new_trail_stroke();
                self.velocity_corners = zero_velocity_corners();
                self.spring_velocity_corners = zero_velocity_corners();
                self.set_particles_vec(Vec::new());
                self.clear_pending_target();
                self.stop_animation();
                self.mark_initialized();
                self.reset_animation_timing();
            }
        }
    }

    pub(crate) fn initialize_cursor(
        &mut self,
        position: RenderPoint,
        shape: CursorShape,
        seed: u32,
        tracked_cursor: &TrackedCursor,
    ) {
        self.apply_cursor_transition(
            position,
            shape,
            tracked_cursor,
            CursorTransitionPolicy::Initialize { seed },
        );
    }

    pub(crate) fn jump_preserving_motion(
        &mut self,
        position: RenderPoint,
        shape: CursorShape,
        tracked_cursor: &TrackedCursor,
    ) {
        self.apply_cursor_transition(
            position,
            shape,
            tracked_cursor,
            CursorTransitionPolicy::JumpPreservingMotion,
        );
    }

    pub(crate) fn jump_and_stop_animation(
        &mut self,
        position: RenderPoint,
        shape: CursorShape,
        tracked_cursor: &TrackedCursor,
    ) {
        self.apply_cursor_transition(
            position,
            shape,
            tracked_cursor,
            CursorTransitionPolicy::JumpAndStopAnimation,
        );
    }

    pub(crate) fn sync_to_current_cursor(
        &mut self,
        position: RenderPoint,
        shape: CursorShape,
        tracked_cursor: &TrackedCursor,
    ) {
        self.apply_cursor_transition(
            position,
            shape,
            tracked_cursor,
            CursorTransitionPolicy::SyncToCurrentCursor,
        );
    }
}
