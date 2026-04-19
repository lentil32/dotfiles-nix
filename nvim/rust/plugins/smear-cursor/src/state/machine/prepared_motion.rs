use super::RuntimeState;
use super::types::AnimationPhase;
use super::types::TransientRuntimeState;
use crate::types::Particle;
use crate::types::ParticleAggregationScratch;
use crate::types::Point;
use crate::types::SharedAggregatedParticleCells;
use crate::types::SharedParticleScreenCells;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PreparedRuntimeMotion {
    animation_phase: AnimationPhase,
    current_corners: [Point; 4],
    trail_origin_corners: [Point; 4],
    target_corners: [Point; 4],
    velocity_corners: [Point; 4],
    spring_velocity_corners: [Point; 4],
    trail_elapsed_ms: [f64; 4],
    particles: Vec<Particle>,
    aggregated_particle_cells: SharedAggregatedParticleCells,
    aggregated_particle_cells_dirty: bool,
    particle_screen_cells: SharedParticleScreenCells,
    particle_screen_cells_dirty: bool,
    previous_center: Point,
    rng_state: u32,
    transient: TransientRuntimeState,
}

impl PreparedRuntimeMotion {
    pub(crate) fn into_particles(self) -> Vec<Particle> {
        self.particles
    }

    #[cfg(test)]
    pub(crate) fn particles_capacity(&self) -> usize {
        self.particles.capacity()
    }
}

impl RuntimeState {
    pub(crate) fn runtime_changed_since_preview(&self) -> bool {
        let Some(preview_baseline) = self.preview_baseline.as_ref() else {
            return false;
        };
        preview_baseline != &super::RuntimePreviewBaseline::from_runtime(self)
            || self.preview_particles_changed_since_preview()
    }

    fn preview_particles_changed_since_preview(&self) -> bool {
        let Some(source) = self.preview_particles_source else {
            return false;
        };
        self.preview_particles_materialized && self.particles.as_slice() != source.as_slice()
    }

    pub(crate) fn planning_preview(&mut self) -> Self {
        crate::events::record_planning_preview_invocation();
        let mut preview_particles = std::mem::take(&mut self.preview_particles_scratch);
        preview_particles.clear();

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
            particles: preview_particles,
            preview_particles_source: Some(super::PreviewParticlesSource::from_slice(
                self.particles(),
            )),
            preview_particles_materialized: false,
            preview_baseline: Some(super::RuntimePreviewBaseline::from_runtime(self)),
            preview_particles_scratch: Vec::new(),
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

    pub(crate) fn prepared_motion(&self) -> PreparedRuntimeMotion {
        PreparedRuntimeMotion {
            animation_phase: self.animation_phase.clone(),
            current_corners: self.current_corners,
            trail_origin_corners: self.trail_origin_corners,
            target_corners: self.target_corners,
            velocity_corners: self.velocity_corners,
            spring_velocity_corners: self.spring_velocity_corners,
            trail_elapsed_ms: self.trail_elapsed_ms,
            particles: self.particles().to_vec(),
            aggregated_particle_cells: self.aggregated_particle_cells.clone(),
            aggregated_particle_cells_dirty: self.aggregated_particle_cells_dirty,
            particle_screen_cells: self.particle_screen_cells.clone(),
            particle_screen_cells_dirty: self.particle_screen_cells_dirty,
            previous_center: self.previous_center,
            rng_state: self.rng_state,
            transient: self.transient.clone(),
        }
    }

    pub(crate) fn into_preview_particle_storage(self) -> Vec<Particle> {
        self.particles
    }

    pub(crate) fn into_prepared_motion(mut self) -> PreparedRuntimeMotion {
        self.materialize_preview_particles();
        let Self {
            config: _,
            render_static_config: _,
            plugin_state: _,
            animation_phase,
            current_corners,
            trail_origin_corners,
            target_corners,
            velocity_corners,
            spring_velocity_corners,
            trail_elapsed_ms,
            particles,
            preview_particles_source: _,
            preview_particles_materialized: _,
            preview_baseline: _,
            preview_particles_scratch: _,
            render_step_samples_scratch: _,
            aggregated_particle_cells,
            aggregated_particle_cells_dirty,
            particle_screen_cells,
            particle_screen_cells_dirty,
            particle_aggregation_scratch: _,
            previous_center,
            rng_state,
            transient,
        } = self;

        PreparedRuntimeMotion {
            animation_phase,
            current_corners,
            trail_origin_corners,
            target_corners,
            velocity_corners,
            spring_velocity_corners,
            trail_elapsed_ms,
            particles,
            aggregated_particle_cells,
            aggregated_particle_cells_dirty,
            particle_screen_cells,
            particle_screen_cells_dirty,
            previous_center,
            rng_state,
            transient,
        }
    }

    pub(crate) fn apply_prepared_motion(&mut self, prepared_motion: PreparedRuntimeMotion) {
        self.materialize_preview_particles();
        let PreparedRuntimeMotion {
            animation_phase,
            current_corners,
            trail_origin_corners,
            target_corners,
            velocity_corners,
            spring_velocity_corners,
            trail_elapsed_ms,
            particles,
            aggregated_particle_cells,
            aggregated_particle_cells_dirty,
            particle_screen_cells,
            particle_screen_cells_dirty,
            previous_center,
            rng_state,
            transient,
        } = prepared_motion;
        let previous_particles = std::mem::replace(&mut self.particles, particles);
        self.preview_particles_source = None;
        self.preview_particles_materialized = false;
        self.preview_baseline = None;
        self.reclaim_preview_particles_scratch(previous_particles);

        self.animation_phase = animation_phase;
        self.current_corners = current_corners;
        self.trail_origin_corners = trail_origin_corners;
        self.target_corners = target_corners;
        self.velocity_corners = velocity_corners;
        self.spring_velocity_corners = spring_velocity_corners;
        self.trail_elapsed_ms = trail_elapsed_ms;
        self.aggregated_particle_cells = aggregated_particle_cells;
        self.aggregated_particle_cells_dirty = aggregated_particle_cells_dirty;
        self.particle_screen_cells = particle_screen_cells;
        self.particle_screen_cells_dirty = particle_screen_cells_dirty;
        self.previous_center = previous_center;
        self.rng_state = rng_state;
        self.transient = transient;
    }
}
