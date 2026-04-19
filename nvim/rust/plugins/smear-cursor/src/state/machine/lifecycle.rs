use super::CursorLocation;
use super::CursorShape;
use super::RuntimeState;
use super::types::AnimationPhase;
use super::types::DrainingPhase;
use super::types::MotionClock;
use super::types::RunningPhase;
use super::types::SettlingPhase;
use super::types::SettlingWindow;
use crate::animation::center;
use crate::animation::initial_velocity;
use crate::animation::zero_velocity_corners;
use crate::types::Point;
use crate::types::StepOutput;

fn translate_corners(corners: &mut [Point; 4], row_delta: f64, col_delta: f64) {
    for corner in corners {
        corner.row += row_delta;
        corner.col += col_delta;
    }
}

fn row_translation_to_clamp_window(corners: &[Point; 4], min_row: f64, max_row: f64) -> f64 {
    let mut current_min_row = f64::INFINITY;
    let mut current_max_row = f64::NEG_INFINITY;
    for corner in corners {
        current_min_row = current_min_row.min(corner.row);
        current_max_row = current_max_row.max(corner.row);
    }

    let max_boundary_row = max_row + 1.0;
    if current_min_row < min_row {
        min_row - current_min_row
    } else if current_max_row > max_boundary_row {
        max_boundary_row - current_max_row
    } else {
        0.0
    }
}

impl RuntimeState {
    fn motion_clock(&self) -> Option<&MotionClock> {
        match &self.animation_phase {
            AnimationPhase::Running(phase) => Some(&phase.clock),
            AnimationPhase::Draining(phase) => Some(&phase.clock),
            AnimationPhase::Uninitialized | AnimationPhase::Idle | AnimationPhase::Settling(_) => {
                None
            }
        }
    }

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
        let target_corners = self.target_corners();
        self.velocity_corners = initial_velocity(
            &self.current_corners,
            &target_corners,
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
        self.set_target(position, shape);
        self.update_tracking(location);
        self.animation_phase = AnimationPhase::Settling(SettlingPhase {
            settling_window: SettlingWindow {
                stable_since_ms: now_ms,
                settle_deadline_ms: now_ms + delay_ms,
            },
        });
    }

    #[cfg(test)]
    pub(crate) fn refresh_settling_target(
        &mut self,
        position: Point,
        shape: CursorShape,
        location: &CursorLocation,
        now_ms: f64,
    ) {
        let preserve_stable_since =
            self.target_position() == position && self.tracked_location_ref() == Some(location);
        self.refresh_settling_target_with_match(
            position,
            shape,
            location,
            now_ms,
            preserve_stable_since,
        );
    }

    pub(crate) fn refresh_settling_target_with_match(
        &mut self,
        position: Point,
        shape: CursorShape,
        location: &CursorLocation,
        now_ms: f64,
        preserve_stable_since: bool,
    ) {
        let delay_ms = self.config.delay_event_to_smear.max(0.0);
        let stable_since_ms = if preserve_stable_since {
            self.settling_window()
                .map_or(now_ms, |existing| existing.stable_since_ms)
        } else {
            now_ms
        };
        self.set_target(position, shape);
        self.update_tracking(location);
        self.animation_phase = AnimationPhase::Settling(SettlingPhase {
            settling_window: SettlingWindow {
                stable_since_ms,
                settle_deadline_ms: now_ms + delay_ms,
            },
        });
    }

    pub(crate) fn should_promote_settled_target(
        &self,
        now_ms: f64,
        observed_position: Point,
        observed_location: &CursorLocation,
    ) -> bool {
        let Some(settling_window) = self.settling_window() else {
            return false;
        };
        self.target_position() == observed_position
            && self.tracked_location_ref() == Some(observed_location)
            && settling_window.settle_deadline_ms >= settling_window.stable_since_ms
            && now_ms >= settling_window.settle_deadline_ms
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
        self.motion_clock().and_then(|clock| clock.last_tick_ms)
    }

    pub(crate) fn set_last_tick_ms(&mut self, value: Option<f64>) {
        let Some(clock) = self.motion_clock_mut() else {
            debug_assert!(
                value.is_none(),
                "animation tick timing only exists while running or draining"
            );
            return;
        };
        clock.last_tick_ms = value;
    }

    pub(crate) fn record_animation_tick(&mut self, now_ms: f64) {
        self.set_last_tick_ms(Some(now_ms));
    }

    pub(crate) fn take_animation_elapsed_ms(
        &mut self,
        now_ms: f64,
        fallback_elapsed_ms: f64,
    ) -> f64 {
        let clamped_fallback = if fallback_elapsed_ms.is_finite() {
            fallback_elapsed_ms.max(0.0)
        } else {
            0.0
        };
        let Some(clock) = self.motion_clock_mut() else {
            debug_assert!(
                false,
                "animation elapsed time should only be sampled while running or draining"
            );
            return clamped_fallback;
        };
        let elapsed_ms = clock
            .last_tick_ms
            .map_or(clamped_fallback, |previous| (now_ms - previous).max(0.0));
        clock.last_tick_ms = Some(now_ms);
        elapsed_ms
    }

    pub(crate) fn settle_deadline_ms(&self) -> Option<f64> {
        self.settling_window()
            .map(|window| window.settle_deadline_ms)
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
        if let Some(clock) = self.motion_clock_mut() {
            clock.reset();
        }
    }

    pub(crate) fn tracked_location(&self) -> Option<CursorLocation> {
        self.tracked_location_ref().cloned()
    }

    pub(crate) fn tracked_location_ref(&self) -> Option<&CursorLocation> {
        self.target.tracked_location.as_ref()
    }

    pub(crate) fn update_tracking(&mut self, location: &CursorLocation) {
        let surface_changed = self.tracked_location_ref().is_some_and(|tracked| {
            tracked.window_handle != location.window_handle
                || tracked.buffer_handle != location.buffer_handle
                || tracked.window_dimensions_changed(location)
        });
        if surface_changed {
            self.target.retarget_epoch = self.target.retarget_epoch.wrapping_add(1);
        }
        self.target.tracked_location = Some(location.clone());
    }

    pub(crate) fn apply_scroll_shift(
        &mut self,
        row_shift: f64,
        col_shift: f64,
        min_row: f64,
        max_row: f64,
    ) {
        translate_corners(&mut self.current_corners, -row_shift, -col_shift);
        let clamp_translation =
            row_translation_to_clamp_window(&self.current_corners, min_row, max_row);
        if clamp_translation != 0.0 {
            translate_corners(&mut self.current_corners, clamp_translation, 0.0);
        }
        self.previous_center = center(&self.current_corners);
        translate_corners(&mut self.trail.origin_corners, -row_shift, -col_shift);
        if !self.particles.is_empty() {
            for particle in &mut self.particles {
                particle.position.row -= row_shift;
                particle.position.col -= col_shift;
            }
        }
        self.purge_cached_particle_artifacts();
    }

    pub(crate) fn apply_step_output(&mut self, output: StepOutput) {
        self.current_corners = output.current_corners;
        self.velocity_corners = output.velocity_corners;
        self.spring_velocity_corners = output.spring_velocity_corners;
        self.trail.elapsed_ms = output.trail_elapsed_ms;
        self.previous_center = output.previous_center;
        self.rng_state = output.rng_state;
        self.set_particles_vec(output.particles);
    }

    pub(crate) fn settle_at_target(&mut self) {
        let target_corners = self.target_corners();
        self.current_corners = target_corners;
        self.trail.origin_corners = target_corners;
        self.trail.elapsed_ms = [0.0; 4];
        self.velocity_corners = zero_velocity_corners();
        self.spring_velocity_corners = zero_velocity_corners();
    }
}
