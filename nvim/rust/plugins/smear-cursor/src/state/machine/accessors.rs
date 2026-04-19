use super::CursorShape;
use super::RuntimeOptionsEffects;
use super::RuntimeOptionsPatch;
use super::RuntimeState;
use super::TrackedCursor;
use super::types::AnimationPhase;
use super::types::CachedParticleArtifacts;
use super::types::RuntimeTargetRetargetKey;
use super::types::SettlingWindow;
use crate::config::DerivedConfigCache;
use crate::core::types::StrokeId;
use crate::position::RenderPoint;
use crate::position::current_visual_cursor_anchor;
use crate::types::Particle;
use crate::types::ParticleScreenCellsMode;
use crate::types::RenderStepSample;
use crate::types::SharedAggregatedParticleCells;
use crate::types::SharedParticleScreenCells;
use crate::types::StaticRenderConfig;
use crate::types::aggregate_particle_artifacts_with_scratch;
use crate::types::aggregate_particle_screen_cells;
use std::sync::Arc;

impl RuntimeState {
    pub(super) fn set_particles_vec(&mut self, particles: Vec<Particle>) {
        self.particles = particles;
        self.purge_cached_particle_artifacts();
    }

    pub(crate) fn reclaim_preview_particles_scratch(&mut self, mut scratch: Vec<Particle>) {
        scratch.clear();
        if self.caches.scratch_buffers.preview_particles.capacity() < scratch.capacity() {
            self.caches.scratch_buffers.preview_particles = scratch;
        } else {
            self.caches.scratch_buffers.preview_particles.clear();
        }
    }

    pub(crate) fn take_render_step_samples_scratch(&mut self) -> Vec<RenderStepSample> {
        std::mem::take(&mut self.caches.scratch_buffers.render_step_samples)
    }

    pub(crate) fn reclaim_render_step_samples_scratch(
        &mut self,
        mut scratch: Vec<RenderStepSample>,
    ) {
        scratch.clear();
        if self.caches.scratch_buffers.render_step_samples.capacity() < scratch.capacity() {
            self.caches.scratch_buffers.render_step_samples = scratch;
        } else {
            self.caches.scratch_buffers.render_step_samples.clear();
        }
    }

    pub(crate) fn purge_cached_particle_artifacts(&mut self) {
        self.caches.particle_artifacts.cached = None;
    }

    pub(crate) fn static_render_config(&self) -> Arc<StaticRenderConfig> {
        Arc::new(self.derived_config.static_render_config())
    }

    pub(crate) fn projection_policy(&self) -> super::ProjectionPolicySnapshot {
        self.projection_policy
    }

    pub(crate) fn commit_runtime_config_update(&mut self) {
        let config_revision = self.config_revision.next();
        let derived_config = DerivedConfigCache::new(&self.config);
        self.projection_policy = self
            .projection_policy
            .refreshed(&self.derived_config, &derived_config);
        // Derived config is cache-only; downstream freshness keys from the
        // authoritative runtime revision instead of a cache-local mirror.
        self.config_revision = config_revision;
        self.derived_config = derived_config;
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

    pub(crate) fn current_corners(&self) -> [RenderPoint; 4] {
        self.current_corners
    }

    pub(crate) fn trail_origin_corners(&self) -> [RenderPoint; 4] {
        self.trail.origin_corners
    }

    pub(crate) fn target_corners(&self) -> [RenderPoint; 4] {
        self.target.corners()
    }

    pub(crate) fn target_position(&self) -> RenderPoint {
        self.target.position
    }

    pub(crate) fn target_shape(&self) -> crate::state::CursorShape {
        self.target.shape
    }

    pub(crate) fn retarget_key(&self) -> RuntimeTargetRetargetKey {
        self.target.retarget_key()
    }

    pub(crate) fn current_visual_cursor_anchor(&self) -> RenderPoint {
        let target_corners = self.target_corners();
        current_visual_cursor_anchor(&self.current_corners, &target_corners, self.target.position)
    }

    pub(crate) fn retarget_epoch(&self) -> u64 {
        self.target.retarget_epoch
    }

    pub(crate) fn settling_target_matches(
        &self,
        position: RenderPoint,
        shape: CursorShape,
        tracked_cursor: &TrackedCursor,
    ) -> bool {
        self.target.position == position
            && self.target.retarget_key()
                == RuntimeTargetRetargetKey::from_snapshot(position, shape, Some(tracked_cursor))
    }

    pub(crate) fn trail_stroke_id(&self) -> StrokeId {
        self.trail.stroke_id
    }

    #[cfg(test)]
    pub(crate) fn has_motion_clock(&self) -> bool {
        matches!(
            self.animation_phase,
            AnimationPhase::Running(_) | AnimationPhase::Draining(_)
        )
    }

    pub(crate) fn start_new_trail_stroke(&mut self) {
        self.trail.stroke_id = self.trail.stroke_id.next();
    }

    pub(crate) fn crossed_cmdline_boundary(&self, current_is_cmdline: bool) -> bool {
        self.transient.last_observed_mode.crossed_cmdline_boundary(
            super::types::LastObservedMode::from_cmdline(current_is_cmdline),
        )
    }

    pub(crate) fn record_observed_mode(&mut self, current_is_cmdline: bool) {
        self.transient.last_observed_mode =
            super::types::LastObservedMode::from_cmdline(current_is_cmdline);
    }

    pub(crate) fn velocity_corners(&self) -> [RenderPoint; 4] {
        self.velocity_corners
    }

    pub(crate) fn spring_velocity_corners(&self) -> [RenderPoint; 4] {
        self.spring_velocity_corners
    }

    pub(crate) fn trail_elapsed_ms(&self) -> [f64; 4] {
        self.trail.elapsed_ms
    }

    pub(crate) fn particles(&self) -> &[Particle] {
        self.particles.as_slice()
    }

    #[cfg(debug_assertions)]
    pub(crate) fn debug_assert_invariants(&self) {
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
                self.target.tracked_cursor.is_some(),
                "settling requires transient tracking ownership"
            );
            debug_assert!(
                phase.settling_window.stable_since_ms <= phase.settling_window.settle_deadline_ms,
                "settling deadline must not precede the stable-since timestamp"
            );
        }
        debug_assert_eq!(
            self.target.cell,
            crate::position::ScreenCell::from_rounded_point(self.target.position),
            "runtime target cell must stay derived from the retained target position"
        );
        debug_assert_eq!(
            self.target.retarget_surface,
            self.target
                .tracked_cursor
                .as_ref()
                .map(super::types::RetargetSurfaceKey::from_tracked_cursor),
            "retarget surface facts must stay in sync with tracked cursor ownership"
        );
        debug_assert_eq!(
            self.target.retarget_key(),
            super::types::RuntimeTargetRetargetKey::from_snapshot(
                self.target.position,
                self.target.shape,
                self.target.tracked_cursor.as_ref(),
            ),
            "runtime target equality must stay derived from the retained target snapshot"
        );
    }

    #[cfg(not(debug_assertions))]
    pub(crate) fn debug_assert_invariants(&self) {}

    pub(crate) fn shared_particle_artifacts(
        &mut self,
        screen_cells_mode: ParticleScreenCellsMode,
    ) -> (SharedAggregatedParticleCells, SharedParticleScreenCells) {
        if self.caches.particle_artifacts.cached.is_none() {
            let cached = if self.particles.is_empty() {
                CachedParticleArtifacts {
                    aggregated_particle_cells: Arc::default(),
                    particle_screen_cells: match screen_cells_mode {
                        ParticleScreenCellsMode::Skip => None,
                        ParticleScreenCellsMode::Collect => Some(Arc::default()),
                    },
                }
            } else {
                crate::events::record_particle_aggregation(self.particles().len());
                let mut scratch =
                    std::mem::take(&mut self.caches.scratch_buffers.particle_aggregation);
                let cached = {
                    let particles = self.particles();
                    let artifacts = aggregate_particle_artifacts_with_scratch(
                        particles,
                        screen_cells_mode,
                        &mut scratch,
                    );
                    CachedParticleArtifacts {
                        aggregated_particle_cells: artifacts.aggregated_particle_cells,
                        particle_screen_cells: match screen_cells_mode {
                            ParticleScreenCellsMode::Skip => None,
                            ParticleScreenCellsMode::Collect => {
                                Some(artifacts.particle_screen_cells)
                            }
                        },
                    }
                };
                self.caches.scratch_buffers.particle_aggregation = scratch;
                cached
            };
            self.caches.particle_artifacts.cached = Some(cached);
        }

        if matches!(screen_cells_mode, ParticleScreenCellsMode::Collect)
            && self
                .caches
                .particle_artifacts
                .cached
                .as_ref()
                .is_some_and(|cached| cached.particle_screen_cells.is_none())
        {
            let particle_screen_cells = {
                let Some(cached) = self.caches.particle_artifacts.cached.as_ref() else {
                    unreachable!("particle artifact cache must exist before deriving screen cells");
                };
                aggregate_particle_screen_cells(&cached.aggregated_particle_cells)
            };
            let Some(cached) = self.caches.particle_artifacts.cached.as_mut() else {
                unreachable!("particle artifact cache must exist before storing screen cells");
            };
            cached.particle_screen_cells = Some(particle_screen_cells);
        }

        let Some(cached) = self.caches.particle_artifacts.cached.as_ref() else {
            unreachable!("particle artifact cache must be materialized before sharing");
        };

        match screen_cells_mode {
            ParticleScreenCellsMode::Skip => (
                Arc::clone(&cached.aggregated_particle_cells),
                Arc::default(),
            ),
            ParticleScreenCellsMode::Collect => {
                let Some(particle_screen_cells) = cached.particle_screen_cells.as_ref() else {
                    unreachable!(
                        "particle screen cells must be materialized before collect-mode access"
                    );
                };
                (
                    Arc::clone(&cached.aggregated_particle_cells),
                    Arc::clone(particle_screen_cells),
                )
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn shared_aggregated_particle_cells(&mut self) -> SharedAggregatedParticleCells {
        self.shared_particle_artifacts(ParticleScreenCellsMode::Skip)
            .0
    }

    #[cfg(test)]
    pub(crate) fn shared_particle_screen_cells(&mut self) -> SharedParticleScreenCells {
        self.shared_particle_artifacts(ParticleScreenCellsMode::Collect)
            .1
    }

    pub(crate) fn take_particles(&mut self) -> Vec<Particle> {
        let particles = std::mem::take(&mut self.particles);
        self.purge_cached_particle_artifacts();
        particles
    }

    pub(crate) fn clear_particles(&mut self) {
        self.set_particles_vec(Vec::new());
    }

    #[cfg(test)]
    pub(crate) fn preview_particles_scratch_capacity(&self) -> usize {
        self.caches.scratch_buffers.preview_particles.capacity()
    }

    #[cfg(test)]
    pub(crate) fn preview_particles_scratch_ptr(&self) -> *const Particle {
        self.caches.scratch_buffers.preview_particles.as_ptr()
    }

    #[cfg(test)]
    pub(crate) fn render_step_samples_scratch_capacity(&self) -> usize {
        self.caches.scratch_buffers.render_step_samples.capacity()
    }

    #[cfg(test)]
    pub(crate) fn render_step_samples_scratch_ptr(&self) -> *const RenderStepSample {
        self.caches.scratch_buffers.render_step_samples.as_ptr()
    }

    #[cfg(test)]
    pub(crate) fn has_cached_aggregated_particle_cells(&self) -> bool {
        self.caches.particle_artifacts.cached.is_some()
    }

    #[cfg(test)]
    pub(crate) fn has_cached_particle_screen_cells(&self) -> bool {
        self.caches
            .particle_artifacts
            .cached
            .as_ref()
            .is_some_and(|cached| cached.particle_screen_cells.is_some())
    }

    #[cfg(test)]
    pub(crate) fn particle_aggregation_scratch_index_capacity(&self) -> usize {
        self.caches
            .scratch_buffers
            .particle_aggregation
            .cell_index_capacity()
    }

    #[cfg(test)]
    pub(crate) fn particle_aggregation_scratch_cells_capacity(&self) -> usize {
        self.caches
            .scratch_buffers
            .particle_aggregation
            .aggregated_cells_capacity()
    }

    #[cfg(test)]
    pub(crate) fn particle_aggregation_scratch_screen_cells_capacity(&self) -> usize {
        self.caches
            .scratch_buffers
            .particle_aggregation
            .particle_screen_cells_capacity()
    }

    pub(crate) fn previous_center(&self) -> RenderPoint {
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
