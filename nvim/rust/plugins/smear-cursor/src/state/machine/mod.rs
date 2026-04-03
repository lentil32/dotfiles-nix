use super::CursorLocation;
use super::CursorShape;
use super::RuntimeOptionsEffects;
use super::RuntimeOptionsPatch;
use crate::config::RuntimeConfig;
use crate::types::DEFAULT_RNG_STATE;
use crate::types::Point;
use crate::types::SharedAggregatedParticleCells;
use crate::types::SharedParticleScreenCells;
use crate::types::SharedParticles;
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

#[derive(Debug, Clone)]
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
    particles: SharedParticles,
    aggregated_particle_cells: SharedAggregatedParticleCells,
    aggregated_particle_cells_dirty: bool,
    particle_screen_cells: SharedParticleScreenCells,
    particle_screen_cells_dirty: bool,
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
            particles: Arc::default(),
            aggregated_particle_cells: Arc::default(),
            aggregated_particle_cells_dirty: false,
            particle_screen_cells: Arc::default(),
            particle_screen_cells_dirty: false,
            previous_center: Point::ZERO,
            rng_state: DEFAULT_RNG_STATE,
            transient: TransientRuntimeState::default(),
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
            && self.particles == other.particles
            && self.previous_center == other.previous_center
            && self.rng_state == other.rng_state
            && self.transient == other.transient
    }
}

#[cfg(test)]
mod tests;
