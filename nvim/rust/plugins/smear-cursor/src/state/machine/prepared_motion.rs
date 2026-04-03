use super::RuntimeState;
use super::types::AnimationPhase;
use super::types::TransientRuntimeState;
use crate::types::Point;
use crate::types::SharedParticles;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PreparedRuntimeMotion {
    animation_phase: AnimationPhase,
    current_corners: [Point; 4],
    trail_origin_corners: [Point; 4],
    target_corners: [Point; 4],
    velocity_corners: [Point; 4],
    spring_velocity_corners: [Point; 4],
    trail_elapsed_ms: [f64; 4],
    particles: SharedParticles,
    previous_center: Point,
    rng_state: u32,
    transient: TransientRuntimeState,
}

impl RuntimeState {
    pub(crate) fn prepared_motion(&self) -> PreparedRuntimeMotion {
        PreparedRuntimeMotion {
            animation_phase: self.animation_phase.clone(),
            current_corners: self.current_corners,
            trail_origin_corners: self.trail_origin_corners,
            target_corners: self.target_corners,
            velocity_corners: self.velocity_corners,
            spring_velocity_corners: self.spring_velocity_corners,
            trail_elapsed_ms: self.trail_elapsed_ms,
            particles: self.particles.clone(),
            previous_center: self.previous_center,
            rng_state: self.rng_state,
            transient: self.transient.clone(),
        }
    }

    pub(crate) fn apply_prepared_motion(&mut self, prepared_motion: PreparedRuntimeMotion) {
        self.animation_phase = prepared_motion.animation_phase;
        self.current_corners = prepared_motion.current_corners;
        self.trail_origin_corners = prepared_motion.trail_origin_corners;
        self.target_corners = prepared_motion.target_corners;
        self.velocity_corners = prepared_motion.velocity_corners;
        self.spring_velocity_corners = prepared_motion.spring_velocity_corners;
        self.trail_elapsed_ms = prepared_motion.trail_elapsed_ms;
        self.set_shared_particles(prepared_motion.particles);
        self.previous_center = prepared_motion.previous_center;
        self.rng_state = prepared_motion.rng_state;
        self.transient = prepared_motion.transient;
    }
}
