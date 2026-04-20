use super::CursorShape;
use super::RuntimeOptionsEffects;
use super::RuntimeOptionsPatch;
use super::TrackedCursor;
use crate::config::DerivedConfigCache;
use crate::config::RuntimeConfig;
use crate::core::types::ConfigRevision;
use crate::core::types::ProjectionPolicyRevision;
use crate::position::RenderPoint;
use crate::types::DEFAULT_RNG_STATE;
use crate::types::Particle;

mod accessors;
mod lifecycle;
mod prepared_motion;
mod preview;
mod semantic;
mod transitions;
mod types;

use self::types::AnimationPhase;
use self::types::CursorTarget;
use self::types::PluginState;
use self::types::RuntimeCaches;
use self::types::RuntimeScratchBuffers;
use self::types::TrailState;
use self::types::TransientRuntimeState;

pub(crate) use prepared_motion::PreparedRuntimeMotion;
pub(crate) use preview::RuntimePreview;
#[cfg(test)]
pub(crate) use semantic::RuntimeSemanticView;
pub(crate) use types::AnimationClockSample;
#[cfg(test)]
pub(crate) use types::RuntimeTargetRetargetKey;
pub(crate) use types::RuntimeTargetSnapshot;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct ProjectionPolicySnapshot {
    revision: ProjectionPolicyRevision,
}

impl ProjectionPolicySnapshot {
    pub(crate) const fn initial() -> Self {
        Self {
            revision: ProjectionPolicyRevision::INITIAL,
        }
    }

    pub(crate) fn refreshed(self, current: &DerivedConfigCache, next: &DerivedConfigCache) -> Self {
        let revision = if current.matches_projection_policy(next) {
            self.revision
        } else {
            self.revision.next()
        };
        Self { revision }
    }

    pub(crate) const fn revision(self) -> ProjectionPolicyRevision {
        self.revision
    }
}

#[derive(Debug)]
pub(crate) struct RuntimeState {
    // authoritative: reducer-owned runtime policy, freshness, and motion facts.
    pub(crate) config: RuntimeConfig,
    config_revision: ConfigRevision,
    projection_policy: ProjectionPolicySnapshot,
    plugin_state: PluginState,
    animation_phase: AnimationPhase,
    current_corners: [RenderPoint; 4],
    target: CursorTarget,
    trail: TrailState,
    velocity_corners: [RenderPoint; 4],
    spring_velocity_corners: [RenderPoint; 4],
    particles: Vec<Particle>,
    previous_center: RenderPoint,
    rng_state: u32,
    transient: TransientRuntimeState,
    // cache: rebuildable config slices and particle artifacts.
    derived_config: DerivedConfigCache,
    caches: RuntimeCaches,
}

impl Default for RuntimeState {
    fn default() -> Self {
        let config = RuntimeConfig::default();
        let config_revision = ConfigRevision::INITIAL;
        let derived_config = DerivedConfigCache::new(&config);
        let projection_policy = ProjectionPolicySnapshot::initial();
        Self {
            config,
            config_revision,
            derived_config,
            projection_policy,
            plugin_state: PluginState::Enabled,
            animation_phase: AnimationPhase::Uninitialized,
            current_corners: [RenderPoint::ZERO; 4],
            target: CursorTarget::default(),
            trail: TrailState::default(),
            velocity_corners: [RenderPoint::ZERO; 4],
            spring_velocity_corners: [RenderPoint::ZERO; 4],
            particles: Vec::new(),
            caches: RuntimeCaches::default(),
            previous_center: RenderPoint::ZERO,
            rng_state: DEFAULT_RNG_STATE,
            transient: TransientRuntimeState::default(),
        }
    }
}

impl Clone for RuntimeState {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            config_revision: self.config_revision,
            derived_config: self.derived_config.clone(),
            projection_policy: self.projection_policy,
            plugin_state: self.plugin_state,
            animation_phase: self.animation_phase.clone(),
            current_corners: self.current_corners,
            target: self.target.clone(),
            trail: self.trail.clone(),
            velocity_corners: self.velocity_corners,
            spring_velocity_corners: self.spring_velocity_corners,
            particles: self.particles.clone(),
            caches: RuntimeCaches {
                scratch_buffers: RuntimeScratchBuffers::default(),
                particle_artifacts: self.caches.particle_artifacts.clone(),
            },
            previous_center: self.previous_center,
            rng_state: self.rng_state,
            transient: self.transient.clone(),
        }
    }
}

impl PartialEq for RuntimeState {
    fn eq(&self, other: &Self) -> bool {
        self.semantic_view() == other.semantic_view()
    }
}

#[cfg(test)]
mod tests;
