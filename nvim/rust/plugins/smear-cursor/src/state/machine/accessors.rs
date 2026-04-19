use super::RuntimeOptionsEffects;
use super::RuntimeOptionsPatch;
use super::RuntimeState;
use super::types::AnimationPhase;
use super::types::SettlingWindow;
use crate::core::types::StrokeId;
use crate::types::Particle;
use crate::types::ParticleScreenCellsMode;
use crate::types::Point;
use crate::types::RenderStepSample;
use crate::types::SharedAggregatedParticleCells;
use crate::types::SharedParticleScreenCells;
use crate::types::StaticRenderConfig;
use crate::types::aggregate_particle_artifacts_with_scratch;
use crate::types::aggregate_particle_screen_cells;
use crate::types::current_visual_cursor_anchor;
use std::sync::Arc;

impl RuntimeState {
    fn active_particles(&self) -> &[Particle] {
        if let Some(source) = self.preview_particles_source.as_ref()
            && !self.preview_particles_materialized
        {
            return source.as_slice();
        }
        self.particles.as_slice()
    }

    pub(super) fn materialize_preview_particles(&mut self) {
        let Some(source) = self.preview_particles_source.as_ref() else {
            return;
        };
        if self.preview_particles_materialized {
            return;
        }
        let source_particles = source.as_slice();
        crate::events::record_planning_preview_copied_particles(source_particles.len());
        self.particles.clear();
        self.particles.extend_from_slice(source_particles);
        self.preview_particles_materialized = true;
    }

    pub(super) fn set_particles_vec(&mut self, particles: Vec<Particle>) {
        if self.preview_particles_source.is_some() {
            self.preview_particles_materialized = true;
        }
        self.particles = particles;
        self.invalidate_cached_particle_artifacts();
    }

    pub(crate) fn reclaim_preview_particles_scratch(&mut self, mut scratch: Vec<Particle>) {
        scratch.clear();
        if self.preview_particles_scratch.capacity() < scratch.capacity() {
            self.preview_particles_scratch = scratch;
        } else {
            self.preview_particles_scratch.clear();
        }
    }

    pub(crate) fn take_render_step_samples_scratch(&mut self) -> Vec<RenderStepSample> {
        std::mem::take(&mut self.render_step_samples_scratch)
    }

    pub(crate) fn reclaim_render_step_samples_scratch(
        &mut self,
        mut scratch: Vec<RenderStepSample>,
    ) {
        scratch.clear();
        if self.render_step_samples_scratch.capacity() < scratch.capacity() {
            self.render_step_samples_scratch = scratch;
        } else {
            self.render_step_samples_scratch.clear();
        }
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

    pub(crate) fn settling_window(&self) -> Option<&SettlingWindow> {
        match &self.animation_phase {
            AnimationPhase::Settling(phase) => Some(&phase.settling_window),
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
        self.active_particles()
    }

    #[cfg(debug_assertions)]
    pub(crate) fn debug_assert_invariants(&self) {
        debug_assert_eq!(
            self.preview_particles_source.is_some(),
            self.preview_baseline.is_some(),
            "preview particle source and baseline must be present together"
        );
        if self.preview_particles_source.is_none() {
            debug_assert!(
                !self.preview_particles_materialized,
                "preview particles cannot be materialized without a preview source"
            );
        }
        if matches!(
            self.animation_phase,
            AnimationPhase::Uninitialized | AnimationPhase::Idle | AnimationPhase::Settling(_)
        ) {
            debug_assert!(
                self.last_tick_ms().is_none(),
                "inactive runtime phases must not retain animation tick timing"
            );
        }
        if let AnimationPhase::Settling(phase) = &self.animation_phase {
            debug_assert!(
                self.transient.tracked_location.is_some(),
                "settling requires transient tracking ownership"
            );
            debug_assert!(
                phase.settling_window.stable_since_ms <= phase.settling_window.settle_deadline_ms,
                "settling deadline must not precede the stable-since timestamp"
            );
        }
    }

    #[cfg(not(debug_assertions))]
    pub(crate) fn debug_assert_invariants(&self) {}

    pub(crate) fn shared_particle_artifacts(
        &mut self,
        screen_cells_mode: ParticleScreenCellsMode,
    ) -> (SharedAggregatedParticleCells, SharedParticleScreenCells) {
        match screen_cells_mode {
            ParticleScreenCellsMode::Skip => {
                if self.aggregated_particle_cells_dirty {
                    crate::events::record_particle_aggregation(self.particles().len());
                    let mut scratch = std::mem::take(&mut self.particle_aggregation_scratch);
                    let artifacts = {
                        let particles = self.particles();
                        aggregate_particle_artifacts_with_scratch(
                            particles,
                            ParticleScreenCellsMode::Skip,
                            &mut scratch,
                        )
                    };
                    self.particle_aggregation_scratch = scratch;
                    self.aggregated_particle_cells = artifacts.aggregated_particle_cells;
                    self.aggregated_particle_cells_dirty = false;
                }

                (Arc::clone(&self.aggregated_particle_cells), Arc::default())
            }
            ParticleScreenCellsMode::Collect => {
                if self.particle_screen_cells_dirty {
                    if self.aggregated_particle_cells_dirty {
                        crate::events::record_particle_aggregation(self.particles().len());
                        let mut scratch = std::mem::take(&mut self.particle_aggregation_scratch);
                        let artifacts = {
                            let particles = self.particles();
                            aggregate_particle_artifacts_with_scratch(
                                particles,
                                ParticleScreenCellsMode::Collect,
                                &mut scratch,
                            )
                        };
                        self.particle_aggregation_scratch = scratch;
                        self.aggregated_particle_cells = artifacts.aggregated_particle_cells;
                        self.aggregated_particle_cells_dirty = false;
                        self.particle_screen_cells = artifacts.particle_screen_cells;
                    } else {
                        self.particle_screen_cells =
                            aggregate_particle_screen_cells(&self.aggregated_particle_cells);
                    }
                    self.particle_screen_cells_dirty = false;
                }

                (
                    Arc::clone(&self.aggregated_particle_cells),
                    Arc::clone(&self.particle_screen_cells),
                )
            }
        }
    }

    pub(crate) fn shared_aggregated_particle_cells(&mut self) -> SharedAggregatedParticleCells {
        self.shared_particle_artifacts(ParticleScreenCellsMode::Skip)
            .0
    }

    pub(crate) fn shared_particle_screen_cells(&mut self) -> SharedParticleScreenCells {
        self.shared_particle_artifacts(ParticleScreenCellsMode::Collect)
            .1
    }

    pub(crate) fn take_particles(&mut self) -> Vec<Particle> {
        self.materialize_preview_particles();
        let particles = std::mem::take(&mut self.particles);
        self.preview_particles_materialized = self.preview_particles_source.is_some();
        self.invalidate_cached_particle_artifacts();
        particles
    }

    pub(crate) fn clear_particles(&mut self) {
        self.set_particles_vec(Vec::new());
    }

    #[cfg(test)]
    pub(crate) fn preview_particles_scratch_capacity(&self) -> usize {
        self.preview_particles_scratch.capacity()
    }

    #[cfg(test)]
    pub(crate) fn preview_particles_scratch_ptr(&self) -> *const Particle {
        self.preview_particles_scratch.as_ptr()
    }

    #[cfg(test)]
    pub(crate) fn particles_storage_capacity(&self) -> usize {
        self.particles.capacity()
    }

    #[cfg(test)]
    pub(crate) fn particles_storage_ptr(&self) -> *const Particle {
        self.particles.as_ptr()
    }

    #[cfg(test)]
    pub(crate) fn render_step_samples_scratch_capacity(&self) -> usize {
        self.render_step_samples_scratch.capacity()
    }

    #[cfg(test)]
    pub(crate) fn render_step_samples_scratch_ptr(&self) -> *const RenderStepSample {
        self.render_step_samples_scratch.as_ptr()
    }

    #[cfg(test)]
    pub(crate) fn preview_particles_are_materialized(&self) -> bool {
        self.preview_particles_materialized
    }

    #[cfg(test)]
    pub(crate) fn aggregated_particle_cells_cache_is_dirty(&self) -> bool {
        self.aggregated_particle_cells_dirty
    }

    #[cfg(test)]
    pub(crate) fn particle_screen_cells_cache_is_dirty(&self) -> bool {
        self.particle_screen_cells_dirty
    }

    #[cfg(test)]
    pub(crate) fn particle_aggregation_scratch_index_capacity(&self) -> usize {
        self.particle_aggregation_scratch.cell_index_capacity()
    }

    #[cfg(test)]
    pub(crate) fn particle_aggregation_scratch_cells_capacity(&self) -> usize {
        self.particle_aggregation_scratch
            .aggregated_cells_capacity()
    }

    #[cfg(test)]
    pub(crate) fn particle_aggregation_scratch_screen_cells_capacity(&self) -> usize {
        self.particle_aggregation_scratch
            .particle_screen_cells_capacity()
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
