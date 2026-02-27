use super::types::PendingTarget;
use super::{AnimationState, CursorLocation, CursorShape, RuntimeState};
use crate::animation::{center, corners_for_cursor, initial_velocity, zero_velocity_corners};
use crate::types::{Point, StepOutput};

impl RuntimeState {
    pub(crate) fn start_animation(&mut self) {
        self.mark_initialized();
        self.transient.pending_target = None;
        self.transient.settle_hold_counter = 0;
        self.transient.drain_steps_remaining = 0;
        self.transient.timing.next_frame_at_ms = None;
        self.transient.timing.simulation_accumulator_ms = 0.0;
        self.animation_state = AnimationState::Running;
    }

    pub(crate) fn start_animation_towards_target(&mut self) {
        self.velocity_corners = initial_velocity(
            &self.current_corners,
            &self.target_corners,
            self.config.anticipation,
        );
        self.spring_velocity_corners = self.velocity_corners;
        self.start_animation();
    }

    pub(crate) fn stop_animation(&mut self) {
        if matches!(
            self.animation_state,
            AnimationState::Running | AnimationState::Draining
        ) {
            self.animation_state = AnimationState::Idle;
        }
        self.transient.settle_hold_counter = 0;
        self.transient.drain_steps_remaining = 0;
        self.transient.timing.next_frame_at_ms = None;
        self.transient.timing.simulation_accumulator_ms = 0.0;
    }

    pub(crate) fn clear_pending_target(&mut self) {
        self.transient.pending_target = None;
        if self.animation_state == AnimationState::Settling {
            self.animation_state = AnimationState::Idle;
        }
    }

    pub(crate) fn begin_settling(
        &mut self,
        position: Point,
        shape: CursorShape,
        location: CursorLocation,
        now_ms: f64,
    ) {
        self.transient.drain_steps_remaining = 0;
        let delay_ms = self.config.delay_event_to_smear.max(0.0);
        let pending = PendingTarget::new(position, location, now_ms, now_ms + delay_ms);
        self.set_target(position, shape);
        self.update_tracking(location);
        self.transient.pending_target = Some(pending);
        self.animation_state = AnimationState::Settling;
    }

    pub(crate) fn refresh_settling_target(
        &mut self,
        position: Point,
        shape: CursorShape,
        location: CursorLocation,
        now_ms: f64,
    ) {
        self.transient.drain_steps_remaining = 0;
        let delay_ms = self.config.delay_event_to_smear.max(0.0);
        let pending = match self.transient.pending_target {
            Some(existing) if existing.matches_observation(position, location) => PendingTarget {
                settle_deadline_ms: now_ms + delay_ms,
                ..existing
            },
            _ => PendingTarget::new(position, location, now_ms, now_ms + delay_ms),
        };
        self.set_target(position, shape);
        self.update_tracking(location);
        self.transient.pending_target = Some(pending);
        self.animation_state = AnimationState::Settling;
    }

    pub(crate) fn should_promote_settled_target(
        &self,
        now_ms: f64,
        observed_position: Point,
        observed_location: CursorLocation,
    ) -> bool {
        let Some(pending) = self.transient.pending_target else {
            return false;
        };
        pending.matches_observation(observed_position, observed_location)
            && pending.settle_deadline_ms >= pending.stable_since_ms
            && now_ms >= pending.settle_deadline_ms
    }

    pub(crate) fn note_settle_probe(&mut self, within_enter_threshold: bool) -> bool {
        if within_enter_threshold {
            self.transient.settle_hold_counter =
                self.transient.settle_hold_counter.saturating_add(1);
        } else {
            self.transient.settle_hold_counter = 0;
        }
        self.transient.settle_hold_counter >= self.config.stop_hold_frames
    }

    pub(crate) fn reset_settle_probe(&mut self) {
        self.transient.settle_hold_counter = 0;
    }

    pub(crate) fn start_tail_drain(&mut self, remaining_steps: u32) {
        self.transient.pending_target = None;
        self.transient.settle_hold_counter = 0;
        self.transient.drain_steps_remaining = remaining_steps;
        self.transient.timing.next_frame_at_ms = None;
        self.transient.timing.simulation_accumulator_ms = 0.0;
        self.previous_center = center(&self.current_corners);
        self.animation_state = if remaining_steps == 0 {
            AnimationState::Idle
        } else {
            AnimationState::Draining
        };
    }

    #[cfg(test)]
    pub(crate) fn drain_steps_remaining(&self) -> u32 {
        self.transient.drain_steps_remaining
    }

    pub(crate) fn consume_tail_drain_step(&mut self, step_ms: f64) -> bool {
        if self.transient.drain_steps_remaining == 0 || !self.consume_simulation_step(step_ms) {
            return false;
        }
        self.transient.drain_steps_remaining =
            self.transient.drain_steps_remaining.saturating_sub(1);
        if self.transient.drain_steps_remaining == 0
            && self.animation_state == AnimationState::Draining
        {
            self.animation_state = AnimationState::Idle;
            self.transient.timing.next_frame_at_ms = None;
        }
        true
    }

    pub(crate) fn last_tick_ms(&self) -> Option<f64> {
        self.transient.timing.last_tick_ms
    }

    pub(crate) fn set_last_tick_ms(&mut self, value: Option<f64>) {
        self.transient.timing.last_tick_ms = value;
    }

    pub(crate) fn settle_deadline_ms(&self) -> Option<f64> {
        self.transient
            .pending_target
            .map(|pending| pending.settle_deadline_ms)
    }

    pub(crate) fn advance_next_frame_deadline(&mut self, now_ms: f64) -> f64 {
        let frame_period_ms = self.config.time_interval.max(1.0);
        let mut next_frame_at_ms = self
            .transient
            .timing
            .next_frame_at_ms
            .unwrap_or(now_ms + frame_period_ms);
        if next_frame_at_ms <= now_ms {
            let missed_frames = ((now_ms - next_frame_at_ms) / frame_period_ms).floor() + 1.0;
            next_frame_at_ms += missed_frames * frame_period_ms;
        }
        self.transient.timing.next_frame_at_ms = Some(next_frame_at_ms);
        next_frame_at_ms
    }

    pub(crate) fn push_simulation_elapsed(&mut self, elapsed_ms: f64) {
        let clamped = if elapsed_ms.is_finite() {
            elapsed_ms.max(0.0)
        } else {
            0.0
        };
        self.transient.timing.simulation_accumulator_ms += clamped;
    }

    pub(crate) fn consume_simulation_step(&mut self, step_ms: f64) -> bool {
        if !step_ms.is_finite() || step_ms <= 0.0 {
            self.transient.timing.simulation_accumulator_ms = 0.0;
            return false;
        }
        if self.transient.timing.simulation_accumulator_ms < step_ms {
            return false;
        }
        self.transient.timing.simulation_accumulator_ms -= step_ms;
        true
    }

    pub(crate) fn reset_animation_timing(&mut self) {
        self.transient.timing.reset();
    }

    pub(crate) fn tracked_location(&self) -> Option<CursorLocation> {
        self.transient.tracking.tracked_location()
    }

    pub(crate) fn update_tracking(&mut self, location: CursorLocation) {
        let surface_changed = self
            .transient
            .tracking
            .tracked_location()
            .is_some_and(|tracked| {
                tracked.window_handle != location.window_handle
                    || tracked.buffer_handle != location.buffer_handle
            });
        if surface_changed {
            self.transient.retarget_epoch = self.transient.retarget_epoch.wrapping_add(1);
        }
        self.transient.tracking.update(location);
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
        for corner in &mut self.trail_origin_corners {
            corner.row -= shift;
        }
        for particle in &mut self.particles {
            particle.position.row -= shift;
        }
    }

    pub(crate) fn apply_step_output(&mut self, output: StepOutput) {
        self.current_corners = output.current_corners;
        self.velocity_corners = output.velocity_corners;
        self.spring_velocity_corners = output.spring_velocity_corners;
        self.trail_elapsed_ms = output.trail_elapsed_ms;
        self.previous_center = output.previous_center;
        self.rng_state = output.rng_state;
        self.particles = output.particles;
    }

    pub(crate) fn settle_at_target(&mut self) {
        self.current_corners = self.target_corners;
        self.trail_origin_corners = self.target_corners;
        self.trail_elapsed_ms = [0.0; 4];
        self.velocity_corners = zero_velocity_corners();
        self.spring_velocity_corners = zero_velocity_corners();
    }
}
