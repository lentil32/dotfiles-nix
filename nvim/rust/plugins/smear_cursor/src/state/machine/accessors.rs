use super::types::{GlobalDisplayPose, JumpCue, JumpCuePhase, PendingTarget};
use super::{CursorLocation, RuntimeOptionsEffects, RuntimeOptionsPatch, RuntimeState};
use crate::core::types::StrokeId;
use crate::types::{EPSILON, Particle, Point, StaticRenderConfig};
use std::sync::Arc;

impl RuntimeState {
    fn jump_cue_phase_for_progress(progress: f64) -> JumpCuePhase {
        if progress < 0.18 {
            JumpCuePhase::Launch
        } else if progress < 0.72 {
            JumpCuePhase::Transfer
        } else if progress < 0.90 {
            JumpCuePhase::Catch
        } else {
            JumpCuePhase::Fade
        }
    }

    pub(crate) fn render_static_config(&self) -> Arc<StaticRenderConfig> {
        Arc::clone(&self.render_static_config)
    }

    pub(crate) fn refresh_render_static_config(&mut self) {
        self.render_static_config = Arc::new(StaticRenderConfig::from(&self.config));
    }

    pub(crate) fn is_enabled(&self) -> bool {
        self.plugin_state.is_enabled()
    }

    pub(crate) fn set_enabled(&mut self, enabled: bool) {
        self.plugin_state = super::PluginState::from_enabled(enabled);
    }

    pub(crate) fn apply_runtime_options_patch(
        &mut self,
        patch: RuntimeOptionsPatch,
    ) -> RuntimeOptionsEffects {
        patch.apply(self)
    }

    pub(crate) fn is_initialized(&self) -> bool {
        self.animation_state.is_initialized()
    }

    pub(crate) fn mark_initialized(&mut self) {
        if self.animation_state == super::AnimationState::Uninitialized {
            self.animation_state = super::AnimationState::Idle;
        }
    }

    pub(crate) fn clear_initialization(&mut self) {
        self.animation_state = super::AnimationState::Uninitialized;
    }

    pub(crate) fn is_animating(&self) -> bool {
        self.animation_state.is_animating()
    }

    pub(crate) fn is_settling(&self) -> bool {
        self.animation_state.is_settling()
    }

    pub(crate) fn is_draining(&self) -> bool {
        self.animation_state.is_draining()
    }

    pub(crate) fn pending_target(&self) -> Option<&PendingTarget> {
        self.transient.pending_target.as_ref()
    }

    pub(crate) fn current_corners(&self) -> [Point; 4] {
        self.current_corners
    }

    pub(crate) fn trail_origin_corners(&self) -> [Point; 4] {
        self.trail_origin_corners
    }

    pub(crate) fn target_corners(&self) -> [Point; 4] {
        self.target_corners
    }

    pub(crate) fn target_position(&self) -> Point {
        self.transient.target_position
    }

    pub(crate) fn retarget_epoch(&self) -> u64 {
        self.transient.retarget_epoch
    }

    pub(crate) fn trail_stroke_id(&self) -> StrokeId {
        self.transient.trail_stroke_id
    }

    pub(crate) fn active_jump_cues(&self) -> &[JumpCue] {
        &self.transient.active_jump_cues
    }

    pub(crate) fn refresh_jump_cues(&mut self, now_ms: f64) -> bool {
        let mut changed = false;
        self.transient.active_jump_cues.retain_mut(|cue| {
            let duration_ms = cue.duration_ms.max(1.0);
            let elapsed_ms = (now_ms - cue.started_at_ms).max(0.0);
            let progress = elapsed_ms / duration_ms;
            if !progress.is_finite() || progress >= 1.0 {
                changed = true;
                return false;
            }

            let next_phase = Self::jump_cue_phase_for_progress(progress);
            if cue.phase != next_phase {
                cue.phase = next_phase;
                changed = true;
            }
            true
        });
        changed
    }

    pub(crate) fn start_new_trail_stroke(&mut self) {
        self.transient.trail_stroke_id = self.transient.trail_stroke_id.next();
    }

    pub(crate) fn record_jump_cue(
        &mut self,
        from_position: Point,
        from_location: CursorLocation,
        to_position: Point,
        to_location: CursorLocation,
        started_at_ms: f64,
    ) {
        if !self.config.jump_cues_enabled {
            return;
        }
        let cross_window = from_location.window_handle != to_location.window_handle;
        if cross_window && !self.config.cross_window_jump_bridges {
            return;
        }
        if from_position.distance_squared(to_position) <= EPSILON {
            return;
        }
        let display_distance =
            from_position.display_distance(to_position, self.config.block_aspect_ratio);
        if display_distance < self.config.jump_cue_min_display_distance.max(0.0) {
            return;
        }

        let cue = JumpCue {
            cue_id: self.transient.next_jump_cue_id,
            // Comment: reducer records the cue before applying the cursor transition so the cue
            // carries the acknowledgement epoch that the upcoming state change will expose.
            epoch: self.transient.retarget_epoch.wrapping_add(1),
            from_pose: GlobalDisplayPose::new(from_position, from_location),
            to_pose: GlobalDisplayPose::new(to_position, to_location),
            started_at_ms,
            duration_ms: self.config.jump_cue_duration_ms.max(1.0),
            strength: if cross_window {
                self.config.jump_cue_strength * self.config.cross_window_bridge_strength_scale
            } else {
                self.config.jump_cue_strength
            },
            phase: JumpCuePhase::Launch,
        };
        self.transient.next_jump_cue_id = self.transient.next_jump_cue_id.wrapping_add(1);
        self.transient.active_jump_cues.push(cue);

        let max_chain = usize::from(self.config.jump_cue_max_chain.max(1));
        while self.transient.active_jump_cues.len() > max_chain {
            self.transient.active_jump_cues.remove(0);
        }
    }

    pub(crate) fn last_mode_was_cmdline(&self) -> Option<bool> {
        self.transient.last_mode_was_cmdline
    }

    pub(crate) fn set_last_mode_was_cmdline(&mut self, value: bool) {
        self.transient.last_mode_was_cmdline = Some(value);
    }

    pub(crate) fn velocity_corners(&self) -> [Point; 4] {
        self.velocity_corners
    }

    pub(crate) fn spring_velocity_corners(&self) -> [Point; 4] {
        self.spring_velocity_corners
    }

    pub(crate) fn trail_elapsed_ms(&self) -> [f64; 4] {
        self.trail_elapsed_ms
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
        self.transient.color_at_cursor.as_deref()
    }

    pub(crate) fn set_color_at_cursor(&mut self, color: Option<String>) {
        self.transient.color_at_cursor = color;
    }

    pub(crate) fn clear_color_at_cursor(&mut self) {
        self.transient.color_at_cursor = None;
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
        self.transient.reset();
    }
}
