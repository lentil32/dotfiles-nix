use super::CursorLocation;
use super::CursorShape;
use super::RuntimeOptionsEffects;
use super::RuntimeOptionsPatch;
use crate::config::RuntimeConfig;
use crate::types::DEFAULT_RNG_STATE;
use crate::types::Particle;
use crate::types::ParticleAggregationScratch;
use crate::types::Point;
use crate::types::RenderStepSample;
use crate::types::SharedAggregatedParticleCells;
use crate::types::SharedParticleScreenCells;
use crate::types::StaticRenderConfig;
use std::sync::Arc;

mod accessors;
mod lifecycle;
mod prepared_motion;
mod transitions;
mod types;

use self::types::AnimationPhase;
use self::types::PluginState;
use self::types::TransientRuntimeState;

pub(crate) use prepared_motion::PreparedRuntimeMotion;

#[derive(Debug, Clone, Copy)]
struct PreviewParticlesSource {
    ptr: *const Particle,
    len: usize,
}

impl PreviewParticlesSource {
    fn from_slice(slice: &[Particle]) -> Self {
        Self {
            ptr: slice.as_ptr(),
            len: slice.len(),
        }
    }

    fn as_slice(&self) -> &[Particle] {
        // SAFETY: preview sources are captured from the live runtime particle slice and are only
        // read while that runtime still outlives the preview runtime that references it.
        unsafe { std::slice::from_raw_parts(self.ptr, self.len) }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct RuntimePreviewBaseline {
    config: RuntimeConfig,
    render_static_config: Arc<StaticRenderConfig>,
    plugin_state: PluginState,
    animation_phase: AnimationPhase,
    current_corners: [Point; 4],
    trail_origin_corners: [Point; 4],
    target_corners: [Point; 4],
    velocity_corners: [Point; 4],
    spring_velocity_corners: [Point; 4],
    trail_elapsed_ms: [f64; 4],
    previous_center: Point,
    rng_state: u32,
    transient: TransientRuntimeState,
}

impl RuntimePreviewBaseline {
    fn from_runtime(runtime: &RuntimeState) -> Self {
        Self {
            config: runtime.config.clone(),
            render_static_config: runtime.render_static_config.clone(),
            plugin_state: runtime.plugin_state,
            animation_phase: runtime.animation_phase.clone(),
            current_corners: runtime.current_corners,
            trail_origin_corners: runtime.trail_origin_corners,
            target_corners: runtime.target_corners,
            velocity_corners: runtime.velocity_corners,
            spring_velocity_corners: runtime.spring_velocity_corners,
            trail_elapsed_ms: runtime.trail_elapsed_ms,
            previous_center: runtime.previous_center,
            rng_state: runtime.rng_state,
            transient: runtime.transient.clone(),
        }
    }
}

#[derive(Debug)]
pub(crate) struct RuntimeState {
    // phase 5 moves the legacy motion/planning model into core-owned
    // runtime state so reducers no longer depend on a shell-local render bridge.
    pub(crate) config: RuntimeConfig,
    render_static_config: Arc<StaticRenderConfig>,
    plugin_state: PluginState,
    animation_phase: AnimationPhase,
    current_corners: [Point; 4],
    trail_origin_corners: [Point; 4],
    target_corners: [Point; 4],
    velocity_corners: [Point; 4],
    spring_velocity_corners: [Point; 4],
    trail_elapsed_ms: [f64; 4],
    particles: Vec<Particle>,
    preview_particles_source: Option<PreviewParticlesSource>,
    preview_particles_materialized: bool,
    preview_baseline: Option<RuntimePreviewBaseline>,
    preview_particles_scratch: Vec<Particle>,
    render_step_samples_scratch: Vec<RenderStepSample>,
    aggregated_particle_cells: SharedAggregatedParticleCells,
    aggregated_particle_cells_dirty: bool,
    particle_screen_cells: SharedParticleScreenCells,
    particle_screen_cells_dirty: bool,
    particle_aggregation_scratch: ParticleAggregationScratch,
    previous_center: Point,
    rng_state: u32,
    transient: TransientRuntimeState,
}

impl Default for RuntimeState {
    fn default() -> Self {
        let config = RuntimeConfig::default();
        let render_static_config = Arc::new(StaticRenderConfig::from(&config));
        Self {
            config,
            render_static_config,
            plugin_state: PluginState::Enabled,
            animation_phase: AnimationPhase::Uninitialized,
            current_corners: [Point::ZERO; 4],
            trail_origin_corners: [Point::ZERO; 4],
            target_corners: [Point::ZERO; 4],
            velocity_corners: [Point::ZERO; 4],
            spring_velocity_corners: [Point::ZERO; 4],
            trail_elapsed_ms: [0.0; 4],
            particles: Vec::new(),
            preview_particles_source: None,
            preview_particles_materialized: false,
            preview_baseline: None,
            preview_particles_scratch: Vec::new(),
            render_step_samples_scratch: Vec::new(),
            aggregated_particle_cells: Arc::default(),
            aggregated_particle_cells_dirty: false,
            particle_screen_cells: Arc::default(),
            particle_screen_cells_dirty: false,
            particle_aggregation_scratch: ParticleAggregationScratch::default(),
            previous_center: Point::ZERO,
            rng_state: DEFAULT_RNG_STATE,
            transient: TransientRuntimeState::default(),
        }
    }
}

impl Clone for RuntimeState {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            render_static_config: self.render_static_config.clone(),
            plugin_state: self.plugin_state,
            animation_phase: self.animation_phase.clone(),
            current_corners: self.current_corners,
            trail_origin_corners: self.trail_origin_corners,
            target_corners: self.target_corners,
            velocity_corners: self.velocity_corners,
            spring_velocity_corners: self.spring_velocity_corners,
            trail_elapsed_ms: self.trail_elapsed_ms,
            particles: self.particles().to_vec(),
            preview_particles_source: None,
            preview_particles_materialized: false,
            preview_baseline: self.preview_baseline.clone(),
            preview_particles_scratch: self.preview_particles_scratch.clone(),
            render_step_samples_scratch: Vec::new(),
            aggregated_particle_cells: self.aggregated_particle_cells.clone(),
            aggregated_particle_cells_dirty: self.aggregated_particle_cells_dirty,
            particle_screen_cells: self.particle_screen_cells.clone(),
            particle_screen_cells_dirty: self.particle_screen_cells_dirty,
            particle_aggregation_scratch: ParticleAggregationScratch::default(),
            previous_center: self.previous_center,
            rng_state: self.rng_state,
            transient: self.transient.clone(),
        }
    }
}

impl PartialEq for RuntimeState {
    fn eq(&self, other: &Self) -> bool {
        // Lazy particle-artifact caches are intentionally excluded so equality tracks
        // logical runtime state rather than whether a hot-path accessor was invoked.
        self.config == other.config
            && self.render_static_config == other.render_static_config
            && self.plugin_state == other.plugin_state
            && self.animation_phase == other.animation_phase
            && self.current_corners == other.current_corners
            && self.trail_origin_corners == other.trail_origin_corners
            && self.target_corners == other.target_corners
            && self.velocity_corners == other.velocity_corners
            && self.spring_velocity_corners == other.spring_velocity_corners
            && self.trail_elapsed_ms == other.trail_elapsed_ms
            && self.particles() == other.particles()
            && self.previous_center == other.previous_center
            && self.rng_state == other.rng_state
            && self.transient == other.transient
    }
}

#[cfg(test)]
mod tests;
