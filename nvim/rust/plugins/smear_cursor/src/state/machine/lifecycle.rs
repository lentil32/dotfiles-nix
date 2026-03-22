use super::types::{
    AnimationPhase, DrainingPhase, MotionClock, PendingTarget, RunningPhase, SettlingPhase,
};
use super::{CursorLocation, CursorShape, RuntimeState};
use crate::animation::{center, corners_for_cursor, initial_velocity, zero_velocity_corners};
use crate::types::{Point, StepOutput};

impl RuntimeState {
    fn motion_clock_mut(&mut self) -> Option<&mut MotionClock> {
        match &mut self.animation_phase {
            AnimationPhase::Running(phase) => Some(&mut phase.clock),
            AnimationPhase::Draining(phase) => Some(&mut phase.clock),
            AnimationPhase::Uninitialized | AnimationPhase::Idle | AnimationPhase::Settling(_) => {
                None
            }
        }
    }

    fn running_phase_mut(&mut self) -> Option<&mut RunningPhase> {
        match &mut self.animation_phase {
            AnimationPhase::Running(phase) => Some(phase),
            AnimationPhase::Uninitialized
            | AnimationPhase::Idle
            | AnimationPhase::Settling(_)
            | AnimationPhase::Draining(_) => None,
        }
    }

    pub(crate) fn start_animation(&mut self) {
        self.animation_phase = AnimationPhase::Running(RunningPhase::default());
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
            self.animation_phase,
            AnimationPhase::Running(_) | AnimationPhase::Draining(_)
        ) {
            self.animation_phase = AnimationPhase::Idle;
        }
    }

    pub(crate) fn clear_pending_target(&mut self) {
        if matches!(self.animation_phase, AnimationPhase::Settling(_)) {
            self.animation_phase = AnimationPhase::Idle;
        }
    }

    pub(crate) fn begin_settling(
        &mut self,
        position: Point,
        shape: CursorShape,
        location: &CursorLocation,
        now_ms: f64,
    ) {
        let delay_ms = self.config.delay_event_to_smear.max(0.0);
        let pending = PendingTarget::new(position, location, now_ms, now_ms + delay_ms);
        self.set_target(position, shape);
        self.update_tracking(location);
        self.animation_phase = AnimationPhase::Settling(SettlingPhase {
            pending_target: pending,
        });
    }

    pub(crate) fn refresh_settling_target(
        &mut self,
        position: Point,
        shape: CursorShape,
        location: &CursorLocation,
        now_ms: f64,
    ) {
        let delay_ms = self.config.delay_event_to_smear.max(0.0);
        let pending = match self.pending_target() {
            Some(existing) if existing.matches_observation(position, location) => PendingTarget {
                position: existing.position,
                cursor_location: existing.cursor_location.clone(),
                stable_since_ms: existing.stable_since_ms,
                settle_deadline_ms: now_ms + delay_ms,
            },
            _ => PendingTarget::new(position, location, now_ms, now_ms + delay_ms),
        };
        self.set_target(position, shape);
        self.update_tracking(location);
        self.animation_phase = AnimationPhase::Settling(SettlingPhase {
            pending_target: pending,
        });
    }

    pub(crate) fn should_promote_settled_target(
        &self,
        now_ms: f64,
        observed_position: Point,
        observed_location: &CursorLocation,
    ) -> bool {
        let Some(pending) = self.pending_target() else {
            return false;
        };
        pending.matches_observation(observed_position, observed_location)
            && pending.settle_deadline_ms >= pending.stable_since_ms
            && now_ms >= pending.settle_deadline_ms
    }

    pub(crate) fn note_settle_probe(&mut self, within_enter_threshold: bool) -> bool {
        let Some(phase) = self.running_phase_mut() else {
            return false;
        };
        if within_enter_threshold {
            phase.settle_hold_counter = phase.settle_hold_counter.saturating_add(1);
        } else {
            phase.settle_hold_counter = 0;
        }
        phase.settle_hold_counter >= self.config.stop_hold_frames
    }

    pub(crate) fn reset_settle_probe(&mut self) {
        if let Some(phase) = self.running_phase_mut() {
            phase.settle_hold_counter = 0;
        }
    }

    pub(crate) fn start_tail_drain(&mut self, remaining_steps: u32) {
        self.previous_center = center(&self.current_corners);
        self.animation_phase = DrainingPhase::new(remaining_steps)
            .map(AnimationPhase::Draining)
            .unwrap_or(AnimationPhase::Idle);
    }

    #[cfg(test)]
    pub(crate) fn drain_steps_remaining(&self) -> u32 {
        match &self.animation_phase {
            AnimationPhase::Draining(phase) => phase.remaining_steps.get(),
            AnimationPhase::Uninitialized
            | AnimationPhase::Idle
            | AnimationPhase::Settling(_)
            | AnimationPhase::Running(_) => 0,
        }
    }

    pub(crate) fn consume_tail_drain_step(&mut self, step_ms: f64) -> bool {
        if !matches!(self.animation_phase, AnimationPhase::Draining(_))
            || !self.consume_simulation_step(step_ms)
        {
            return false;
        }

        let should_stop = match &mut self.animation_phase {
            AnimationPhase::Draining(phase) => match phase.remaining_steps.get() - 1 {
                0 => true,
                next_steps => {
                    let Some(next_steps) = std::num::NonZeroU32::new(next_steps) else {
                        return true;
                    };
                    phase.remaining_steps = next_steps;
                    false
                }
            },
            AnimationPhase::Uninitialized
            | AnimationPhase::Idle
            | AnimationPhase::Settling(_)
            | AnimationPhase::Running(_) => false,
        };
        if should_stop {
            self.animation_phase = AnimationPhase::Idle;
        }
        true
    }

    pub(crate) fn last_tick_ms(&self) -> Option<f64> {
        self.transient.last_tick_ms
    }

    pub(crate) fn set_last_tick_ms(&mut self, value: Option<f64>) {
        self.transient.last_tick_ms = value;
    }

    pub(crate) fn settle_deadline_ms(&self) -> Option<f64> {
        self.pending_target()
            .map(|pending| pending.settle_deadline_ms)
    }

    pub(crate) fn advance_next_frame_deadline(&mut self, now_ms: f64) -> f64 {
        let frame_period_ms = self.config.time_interval.max(1.0);
        let Some(clock) = self.motion_clock_mut() else {
            debug_assert!(
                false,
                "next-frame deadlines should only be advanced while running or draining"
            );
            return now_ms + frame_period_ms;
        };
        let mut next_frame_at_ms = clock.next_frame_at_ms.unwrap_or(now_ms + frame_period_ms);
        if next_frame_at_ms <= now_ms {
            let missed_frames = ((now_ms - next_frame_at_ms) / frame_period_ms).floor() + 1.0;
            next_frame_at_ms += missed_frames * frame_period_ms;
        }
        clock.next_frame_at_ms = Some(next_frame_at_ms);
        next_frame_at_ms
    }

    pub(crate) fn push_simulation_elapsed(&mut self, elapsed_ms: f64) {
        let clamped = if elapsed_ms.is_finite() {
            elapsed_ms.max(0.0)
        } else {
            0.0
        };
        let Some(clock) = self.motion_clock_mut() else {
            debug_assert!(
                false,
                "simulation time should only accumulate while running or draining"
            );
            return;
        };
        clock.simulation_accumulator_ms += clamped;
    }

    pub(crate) fn consume_simulation_step(&mut self, step_ms: f64) -> bool {
        let Some(clock) = self.motion_clock_mut() else {
            debug_assert!(
                false,
                "simulation steps should only be consumed while running or draining"
            );
            return false;
        };
        if !step_ms.is_finite() || step_ms <= 0.0 {
            clock.simulation_accumulator_ms = 0.0;
            return false;
        }
        if clock.simulation_accumulator_ms < step_ms {
            return false;
        }
        clock.simulation_accumulator_ms -= step_ms;
        true
    }

    pub(crate) fn reset_animation_timing(&mut self) {
        self.transient.last_tick_ms = None;
        if let Some(clock) = self.motion_clock_mut() {
            clock.reset();
        }
    }

    pub(crate) fn tracked_location(&self) -> Option<CursorLocation> {
        self.tracked_location_ref().cloned()
    }

    pub(crate) fn tracked_location_ref(&self) -> Option<&CursorLocation> {
        self.transient.tracked_location.as_ref()
    }

    pub(crate) fn update_tracking(&mut self, location: &CursorLocation) {
        let surface_changed = self.tracked_location_ref().is_some_and(|tracked| {
            tracked.window_handle != location.window_handle
                || tracked.buffer_handle != location.buffer_handle
        });
        if surface_changed {
            self.transient.retarget_epoch = self.transient.retarget_epoch.wrapping_add(1);
        }
        self.transient.tracked_location = Some(location.clone());
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
