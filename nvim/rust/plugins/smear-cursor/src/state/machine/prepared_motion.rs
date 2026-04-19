use super::RuntimeState;
use super::types::AnimationPhase;
use super::types::CursorTarget;
use super::types::RuntimeParticleArtifactsCache;
use super::types::TrailState;
use super::types::TransientRuntimeState;
use crate::types::Particle;
use crate::types::Point;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PreparedRuntimeMotion {
    animation_phase: AnimationPhase,
    current_corners: [Point; 4],
    target: CursorTarget,
    trail: TrailState,
    velocity_corners: [Point; 4],
    spring_velocity_corners: [Point; 4],
    particles: Vec<Particle>,
    particle_artifacts: RuntimeParticleArtifactsCache,
    previous_center: Point,
    rng_state: u32,
    transient: TransientRuntimeState,
}

impl PreparedRuntimeMotion {
    pub(crate) fn into_particles(self) -> Vec<Particle> {
        self.particles
    }

    pub(super) fn from_runtime(runtime: RuntimeState) -> Self {
        let RuntimeState {
            config: _,
            config_revision: _,
            derived_config: _,
            projection_policy: _,
            plugin_state: _,
            animation_phase,
            current_corners,
            target,
            trail,
            velocity_corners,
            spring_velocity_corners,
            particles,
            caches,
            previous_center,
            rng_state,
            transient,
        } = runtime;
        let particle_artifacts = caches.particle_artifacts;

        Self {
            animation_phase,
            current_corners,
            target,
            trail,
            velocity_corners,
            spring_velocity_corners,
            particles,
            particle_artifacts,
            previous_center,
            rng_state,
            transient,
        }
    }

    #[cfg(test)]
    pub(crate) fn particles_capacity(&self) -> usize {
        self.particles.capacity()
    }
}

impl RuntimeState {
    #[cfg(test)]
    pub(crate) fn prepared_motion(&self) -> PreparedRuntimeMotion {
        PreparedRuntimeMotion {
            animation_phase: self.animation_phase.clone(),
            current_corners: self.current_corners,
            target: self.target.clone(),
            trail: self.trail.clone(),
            velocity_corners: self.velocity_corners,
            spring_velocity_corners: self.spring_velocity_corners,
            particles: self.particles().to_vec(),
            particle_artifacts: self.caches.particle_artifacts.clone(),
            previous_center: self.previous_center,
            rng_state: self.rng_state,
            transient: self.transient.clone(),
        }
    }

    pub(crate) fn apply_prepared_motion(&mut self, prepared_motion: PreparedRuntimeMotion) {
        let PreparedRuntimeMotion {
            animation_phase,
            current_corners,
            target,
            trail,
            velocity_corners,
            spring_velocity_corners,
            particles,
            particle_artifacts,
            previous_center,
            rng_state,
            transient,
        } = prepared_motion;
        let previous_particles = std::mem::replace(&mut self.particles, particles);
        self.reclaim_preview_particles_scratch(previous_particles);

        self.animation_phase = animation_phase;
        self.current_corners = current_corners;
        self.target = target;
        self.trail = trail;
        self.velocity_corners = velocity_corners;
        self.spring_velocity_corners = spring_velocity_corners;
        self.caches.particle_artifacts = particle_artifacts;
        self.previous_center = previous_center;
        self.rng_state = rng_state;
        self.transient = transient;
    }
}
