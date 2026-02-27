use super::{CursorLocation, CursorShape, RuntimeOptionsEffects, RuntimeOptionsPatch};
use crate::config::RuntimeConfig;
use crate::types::{DEFAULT_RNG_STATE, Particle, Point, StaticRenderConfig};
use std::sync::Arc;

mod accessors;
mod lifecycle;
mod transitions;
mod types;

pub(crate) use self::types::JumpCuePhase;
use self::types::{AnimationState, PluginState, TransientRuntimeState};

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RuntimeState {
    // Comment: phase 5 moves the legacy motion/planning model into core-owned
    // runtime state so reducers no longer depend on a shell-local render bridge.
    pub(crate) config: RuntimeConfig,
    render_static_config: Arc<StaticRenderConfig>,
    plugin_state: PluginState,
    animation_state: AnimationState,
    current_corners: [Point; 4],
    trail_origin_corners: [Point; 4],
    target_corners: [Point; 4],
    velocity_corners: [Point; 4],
    spring_velocity_corners: [Point; 4],
    trail_elapsed_ms: [f64; 4],
    particles: Vec<Particle>,
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
            animation_state: AnimationState::Uninitialized,
            current_corners: [Point::ZERO; 4],
            trail_origin_corners: [Point::ZERO; 4],
            target_corners: [Point::ZERO; 4],
            velocity_corners: [Point::ZERO; 4],
            spring_velocity_corners: [Point::ZERO; 4],
            trail_elapsed_ms: [0.0; 4],
            particles: Vec::new(),
            previous_center: Point::ZERO,
            rng_state: DEFAULT_RNG_STATE,
            transient: TransientRuntimeState::default(),
        }
    }
}

#[cfg(test)]
mod tests;
