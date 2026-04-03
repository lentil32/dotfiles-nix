use super::RuntimeOptionsEffects;
use super::RuntimeOptionsPatch;
use super::RuntimeState;
use super::types::AnimationPhase;
use super::types::PendingTarget;
use crate::core::types::StrokeId;
use crate::types::Particle;
use crate::types::Point;
use crate::types::SharedAggregatedParticleCells;
use crate::types::SharedParticleScreenCells;
use crate::types::SharedParticles;
use crate::types::StaticRenderConfig;
use crate::types::aggregate_particle_cells;
use crate::types::aggregate_particle_screen_cells;
use crate::types::current_visual_cursor_anchor;
use std::sync::Arc;

impl RuntimeState {
    pub(super) fn set_shared_particles(&mut self, particles: SharedParticles) {
        self.particles = particles;
        self.invalidate_cached_particle_artifacts();
    }

    pub(super) fn invalidate_cached_particle_artifacts(&mut self) {
        self.aggregated_particle_cells = Arc::default();
        self.aggregated_particle_cells_dirty = !self.particles.is_empty();
        self.particle_screen_cells = Arc::default();
        self.particle_screen_cells_dirty = !self.particles.is_empty();
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
        self.animation_phase.is_initialized()
    }

    pub(crate) fn mark_initialized(&mut self) {
        if matches!(self.animation_phase, AnimationPhase::Uninitialized) {
            self.animation_phase = AnimationPhase::Idle;
        }
    }

    pub(crate) fn clear_initialization(&mut self) {
        self.animation_phase = AnimationPhase::Uninitialized;
    }

    pub(crate) fn is_animating(&self) -> bool {
        self.animation_phase.is_animating()
    }

    pub(crate) fn is_settling(&self) -> bool {
        self.animation_phase.is_settling()
    }

    pub(crate) fn is_draining(&self) -> bool {
        self.animation_phase.is_draining()
    }

    pub(crate) fn pending_target(&self) -> Option<&PendingTarget> {
        match &self.animation_phase {
            AnimationPhase::Settling(phase) => Some(&phase.pending_target),
            AnimationPhase::Uninitialized
            | AnimationPhase::Idle
            | AnimationPhase::Running(_)
            | AnimationPhase::Draining(_) => None,
        }
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

    pub(crate) fn current_visual_cursor_anchor(&self) -> Point {
        current_visual_cursor_anchor(
            &self.current_corners,
            &self.target_corners,
            self.transient.target_position,
        )
    }

    pub(crate) fn retarget_epoch(&self) -> u64 {
        self.transient.retarget_epoch
    }

    pub(crate) fn trail_stroke_id(&self) -> StrokeId {
        self.transient.trail_stroke_id
    }

    #[cfg(test)]
    pub(crate) fn has_motion_clock(&self) -> bool {
        matches!(
            self.animation_phase,
            AnimationPhase::Running(_) | AnimationPhase::Draining(_)
        )
    }

    pub(crate) fn start_new_trail_stroke(&mut self) {
        self.transient.trail_stroke_id = self.transient.trail_stroke_id.next();
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
        self.particles.as_slice()
    }

    pub(crate) fn shared_aggregated_particle_cells(&mut self) -> SharedAggregatedParticleCells {
        if self.aggregated_particle_cells_dirty {
            crate::events::record_particle_aggregation(self.particles.len());
            self.aggregated_particle_cells = aggregate_particle_cells(self.particles.as_slice());
            self.aggregated_particle_cells_dirty = false;
        }
        Arc::clone(&self.aggregated_particle_cells)
    }

    pub(crate) fn shared_particle_screen_cells(&mut self) -> SharedParticleScreenCells {
        if self.particle_screen_cells_dirty {
            let aggregated_particle_cells = self.shared_aggregated_particle_cells();
            self.particle_screen_cells =
                aggregate_particle_screen_cells(&aggregated_particle_cells);
            self.particle_screen_cells_dirty = false;
        }
        Arc::clone(&self.particle_screen_cells)
    }

    pub(crate) fn take_particles(&mut self) -> Vec<Particle> {
        let particles = std::sync::Arc::unwrap_or_clone(std::mem::take(&mut self.particles));
        self.invalidate_cached_particle_artifacts();
        particles
    }

    pub(crate) fn clear_particles(&mut self) {
        self.set_shared_particles(Arc::default());
    }

    #[cfg(test)]
    pub(crate) fn aggregated_particle_cells_cache_is_dirty(&self) -> bool {
        self.aggregated_particle_cells_dirty
    }

    #[cfg(test)]
    pub(crate) fn particle_screen_cells_cache_is_dirty(&self) -> bool {
        self.particle_screen_cells_dirty
    }

    pub(crate) fn previous_center(&self) -> Point {
        self.previous_center
    }

    pub(crate) fn rng_state(&self) -> u32 {
        self.rng_state
    }

    pub(crate) const fn color_at_cursor(&self) -> Option<u32> {
        self.transient.color_at_cursor
    }

    pub(crate) fn set_color_at_cursor(&mut self, color: Option<u32>) {
        self.transient.color_at_cursor = color;
    }

    pub(crate) fn clear_color_at_cursor(&mut self) {
        self.transient.color_at_cursor = None;
    }

    pub(crate) fn clear_runtime_state(&mut self) {
        self.clear_initialization();
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
