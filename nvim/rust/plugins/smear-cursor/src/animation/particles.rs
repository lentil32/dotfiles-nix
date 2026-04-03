#[derive(Debug)]
struct ParticleStepResult {
    particles: Vec<Particle>,
    previous_center: Point,
}

fn update_particles(
    input: &StepInput,
    current_corners: &[Point; 4],
    velocity_corners: &[Point; 4],
    mut particles: Vec<Particle>,
    rng: &mut Rng32,
) -> ParticleStepResult {
    let velocity_conservation =
        velocity_conservation_factor(input.particle_damping, input.time_interval);
    let dt = input.config_time_interval / 1000.0;

    particles.retain_mut(|particle| {
        particle.lifetime -= input.time_interval;
        if particle.lifetime <= 0.0 {
            return false;
        }

        particle.velocity.row = (particle.velocity.row
            + (input.particle_gravity + input.particle_random_velocity * (rng.next_unit() - 0.5))
                * dt)
            * velocity_conservation;

        particle.velocity.col = particle.velocity.col * velocity_conservation
            + input.particle_random_velocity * (rng.next_unit() - 0.5) * dt;

        particle.position.row += (particle.velocity.row * dt) / input.block_aspect_ratio;
        particle.position.col += particle.velocity.col * dt;
        true
    });

    let mut previous_center = input.previous_center;
    if !input.particles_enabled {
        return ParticleStepResult {
            particles,
            previous_center,
        };
    }

    let current_center = center(current_corners);
    let mut center_velocity = center(velocity_corners);
    if dt > EPSILON {
        center_velocity.row = center_velocity.row / dt * input.block_aspect_ratio;
        center_velocity.col /= dt;
    } else {
        center_velocity = Point::ZERO;
    }

    let movement = Point {
        row: current_center.row - input.previous_center.row,
        col: current_center.col - input.previous_center.col,
    };

    let movement_magnitude =
        ((input.block_aspect_ratio * movement.row).powi(2) + movement.col.powi(2)).sqrt();

    if movement_magnitude <= input.min_distance_emit_particles {
        previous_center = current_center;
        return ParticleStepResult {
            particles,
            previous_center,
        };
    }

    let mut num_new_particles =
        input.particles_per_second * dt + movement_magnitude * input.particles_per_length;
    let floor_count = num_new_particles.floor();
    let remainder = num_new_particles - floor_count;
    let extra = if rng.next_unit() < remainder {
        1.0
    } else {
        0.0
    };
    num_new_particles = floor_count + extra;

    let capacity_left = input.particle_max_num.saturating_sub(particles.len());
    let capped = num_new_particles.max(0.0).min(capacity_left as f64);
    let spawn_count = capped as usize;

    let row_spread = input.particle_spread * if input.vertical_bar { 1.0 / 8.0 } else { 1.0 };
    let col_spread = input.particle_spread * if input.horizontal_bar { 1.0 / 8.0 } else { 1.0 };

    for _ in 0..spawn_count {
        let s = rng.next_unit();

        let particle_position = Point {
            row: input.previous_center.row
                + s * movement.row
                + (rng.next_unit() - 0.5) * row_spread,
            col: input.previous_center.col
                + s * movement.col
                + (rng.next_unit() - 0.5) * col_spread,
        };

        let velocity_magnitude = input.particle_max_initial_velocity * rng.next_unit().sqrt();
        let velocity_angle = rng.next_unit() * 2.0 * std::f64::consts::PI;

        let particle_velocity = Point {
            row: velocity_magnitude * velocity_angle.cos()
                + input.particle_velocity_from_cursor * center_velocity.row,
            col: velocity_magnitude * velocity_angle.sin()
                + input.particle_velocity_from_cursor * center_velocity.col,
        };

        let lifetime = input.particle_max_lifetime
            * rng
                .next_unit()
                .powf(input.particle_lifetime_distribution_exponent);

        particles.push(Particle {
            position: particle_position,
            velocity: particle_velocity,
            lifetime,
        });
    }

    previous_center = current_center;

    ParticleStepResult {
        particles,
        previous_center,
    }
}
