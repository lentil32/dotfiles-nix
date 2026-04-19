use super::ProjectionPolicySnapshot;
use super::RuntimeState;
use super::types::AnimationPhase;
use super::types::CursorTarget;
use super::types::PluginState;
use super::types::TrailState;
use super::types::TransientRuntimeState;
use crate::config::RuntimeConfig;
use crate::core::types::ConfigRevision;
use crate::types::Particle;
use crate::types::Point;

// Cache-free projection of authoritative runtime state. Equality on this view
// intentionally ignores purgeable scratch buffers and rebuildable caches.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct RuntimeSemanticView<'a> {
    config: &'a RuntimeConfig,
    config_revision: ConfigRevision,
    projection_policy: ProjectionPolicySnapshot,
    plugin_state: PluginState,
    animation_phase: &'a AnimationPhase,
    current_corners: &'a [Point; 4],
    target: &'a CursorTarget,
    trail: &'a TrailState,
    velocity_corners: &'a [Point; 4],
    spring_velocity_corners: &'a [Point; 4],
    particles: &'a [Particle],
    previous_center: Point,
    rng_state: u32,
    transient: &'a TransientRuntimeState,
}

impl RuntimeState {
    pub(crate) fn semantic_view(&self) -> RuntimeSemanticView<'_> {
        RuntimeSemanticView {
            config: &self.config,
            config_revision: self.config_revision,
            projection_policy: self.projection_policy,
            plugin_state: self.plugin_state,
            animation_phase: &self.animation_phase,
            current_corners: &self.current_corners,
            target: &self.target,
            trail: &self.trail,
            velocity_corners: &self.velocity_corners,
            spring_velocity_corners: &self.spring_velocity_corners,
            particles: &self.particles,
            previous_center: self.previous_center,
            rng_state: self.rng_state,
            transient: &self.transient,
        }
    }
}
