use super::PreparedRuntimeMotion;
use super::ProjectionPolicySnapshot;
use super::RuntimeState;
use super::types::AnimationPhase;
use super::types::CursorTarget;
use super::types::PluginState;
use super::types::RuntimeCaches;
use super::types::RuntimeScratchBuffers;
use super::types::TrailState;
use super::types::TransientRuntimeState;
use crate::config::RuntimeConfig;
use crate::core::types::ConfigRevision;
use crate::position::RenderPoint;
use crate::types::Particle;

// Single-preview copy of authoritative runtime inputs. Keeping this baseline local
// to `RuntimePreview` avoids adding retained preview fields to `RuntimeState`.
#[derive(Debug, Clone, PartialEq)]
struct RuntimePreviewBaseline {
    config: RuntimeConfig,
    config_revision: ConfigRevision,
    projection_policy: ProjectionPolicySnapshot,
    plugin_state: PluginState,
    animation_phase: AnimationPhase,
    current_corners: [RenderPoint; 4],
    target: CursorTarget,
    trail: TrailState,
    velocity_corners: [RenderPoint; 4],
    spring_velocity_corners: [RenderPoint; 4],
    previous_center: RenderPoint,
    rng_state: u32,
    transient: TransientRuntimeState,
}

impl RuntimePreviewBaseline {
    fn from_runtime(runtime: &RuntimeState) -> Self {
        Self {
            config: runtime.config.clone(),
            config_revision: runtime.config_revision,
            projection_policy: runtime.projection_policy,
            plugin_state: runtime.plugin_state,
            animation_phase: runtime.animation_phase.clone(),
            current_corners: runtime.current_corners,
            target: runtime.target.clone(),
            trail: runtime.trail.clone(),
            velocity_corners: runtime.velocity_corners,
            spring_velocity_corners: runtime.spring_velocity_corners,
            previous_center: runtime.previous_center,
            rng_state: runtime.rng_state,
            transient: runtime.transient.clone(),
        }
    }
}

// Operation-scoped planning copy. The preview owns a detached runtime clone for one
// speculative planning pass and returns scratch storage instead of retaining an
// alternate runtime owner.
#[derive(Debug)]
pub(crate) struct RuntimePreview<'a> {
    baseline: RuntimePreviewBaseline,
    baseline_particles: &'a [Particle],
    runtime: RuntimeState,
}

impl<'a> RuntimePreview<'a> {
    pub(crate) fn new(runtime: &'a mut RuntimeState) -> Self {
        crate::events::record_planning_preview_invocation();

        let baseline = RuntimePreviewBaseline::from_runtime(runtime);
        let config = runtime.config.clone();
        let config_revision = runtime.config_revision;
        let derived_config = runtime.derived_config.clone();
        let projection_policy = runtime.projection_policy;
        let plugin_state = runtime.plugin_state;
        let animation_phase = runtime.animation_phase.clone();
        let current_corners = runtime.current_corners;
        let target = runtime.target.clone();
        let trail = runtime.trail.clone();
        let velocity_corners = runtime.velocity_corners;
        let spring_velocity_corners = runtime.spring_velocity_corners;
        let mut preview_particles =
            std::mem::take(&mut runtime.caches.scratch_buffers.preview_particles);
        let particle_artifacts = runtime.caches.particle_artifacts.clone();
        let previous_center = runtime.previous_center;
        let rng_state = runtime.rng_state;
        let transient = runtime.transient.clone();
        let baseline_particles = runtime.particles.as_slice();

        preview_particles.clear();
        preview_particles.extend_from_slice(baseline_particles);
        crate::events::record_planning_preview_copied_particles(baseline_particles.len());

        Self {
            baseline,
            baseline_particles,
            runtime: RuntimeState {
                config,
                config_revision,
                derived_config,
                projection_policy,
                plugin_state,
                animation_phase,
                current_corners,
                target,
                trail,
                velocity_corners,
                spring_velocity_corners,
                particles: preview_particles,
                caches: RuntimeCaches {
                    scratch_buffers: RuntimeScratchBuffers::default(),
                    particle_artifacts,
                },
                previous_center,
                rng_state,
                transient,
            },
        }
    }

    pub(crate) fn runtime_mut(&mut self) -> &mut RuntimeState {
        &mut self.runtime
    }

    pub(crate) fn runtime_changed_since_preview(&self) -> bool {
        self.baseline != RuntimePreviewBaseline::from_runtime(&self.runtime)
            || self.runtime.particles() != self.baseline_particles
    }

    pub(crate) fn into_preview_particle_storage(self) -> Vec<Particle> {
        self.runtime.particles
    }

    pub(crate) fn into_prepared_motion(self) -> PreparedRuntimeMotion {
        PreparedRuntimeMotion::from_runtime(self.runtime)
    }

    #[cfg(test)]
    pub(crate) fn particles(&self) -> &[Particle] {
        self.runtime.particles()
    }

    #[cfg(test)]
    pub(crate) fn particles_storage_capacity(&self) -> usize {
        self.runtime.particles.capacity()
    }

    #[cfg(test)]
    pub(crate) fn particles_storage_ptr(&self) -> *const Particle {
        self.runtime.particles.as_ptr()
    }
}
