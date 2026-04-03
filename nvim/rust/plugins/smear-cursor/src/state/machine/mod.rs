use super::CursorLocation;
use super::CursorShape;
use super::RuntimeOptionsEffects;
use super::RuntimeOptionsPatch;
use crate::config::RuntimeConfig;
use crate::types::DEFAULT_RNG_STATE;
use crate::types::Particle;
use crate::types::Point;
use crate::types::StaticRenderConfig;
use std::sync::Arc;

mod accessors;
mod lifecycle;
mod transitions;
mod types;

use self::types::AnimationPhase;
#[cfg(test)]
pub(crate) use self::types::JumpCuePhase;
use self::types::PluginState;
use self::types::TransientRuntimeState;

#[derive(Debug, Clone, PartialEq)]
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
            previous_center: Point::ZERO,
            rng_state: DEFAULT_RNG_STATE,
            transient: TransientRuntimeState::default(),
        }
    }
}

#[cfg(test)]
mod tests;
