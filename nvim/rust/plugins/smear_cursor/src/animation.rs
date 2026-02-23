use crate::config::RuntimeConfig;
use crate::types::{BASE_TIME_INTERVAL, EPSILON, Particle, Point, Rng32, StepInput, StepOutput};
use nvim_utils::mode::is_insert_like_mode;

fn speed_correction(time_interval: f64) -> f64 {
    time_interval / BASE_TIME_INTERVAL
}

fn velocity_conservation_factor(damping: f64, time_interval: f64) -> f64 {
    let one_minus_damping = (1.0 - damping).clamp(EPSILON, 1.0);
    (one_minus_damping.ln() * speed_correction(time_interval)).exp()
}

pub(crate) fn center(corners: &[Point; 4]) -> Point {
    Point {
        row: (corners[0].row + corners[1].row + corners[2].row + corners[3].row) / 4.0,
        col: (corners[0].col + corners[1].col + corners[2].col + corners[3].col) / 4.0,
    }
}

pub(crate) fn corners_for_cursor(
    row: f64,
    col: f64,
    vertical_bar: bool,
    horizontal_bar: bool,
) -> [Point; 4] {
    if vertical_bar {
        return [
            Point { row, col },
            Point {
                row,
                col: col + 1.0 / 8.0,
            },
            Point {
                row: row + 1.0,
                col: col + 1.0 / 8.0,
            },
            Point {
                row: row + 1.0,
                col,
            },
        ];
    }

    if horizontal_bar {
        return [
            Point {
                row: row + 7.0 / 8.0,
                col,
            },
            Point {
                row: row + 7.0 / 8.0,
                col: col + 1.0,
            },
            Point {
                row: row + 1.0,
                col: col + 1.0,
            },
            Point {
                row: row + 1.0,
                col,
            },
        ];
    }

    [
        Point { row, col },
        Point {
            row,
            col: col + 1.0,
        },
        Point {
            row: row + 1.0,
            col: col + 1.0,
        },
        Point {
            row: row + 1.0,
            col,
        },
    ]
}

pub(crate) fn zero_velocity_corners() -> [Point; 4] {
    [Point::ZERO; 4]
}

pub(crate) fn initial_velocity(
    current_corners: &[Point; 4],
    target_corners: &[Point; 4],
    anticipation: f64,
) -> [Point; 4] {
    let mut velocity_corners = [Point::ZERO; 4];
    for index in 0..4 {
        velocity_corners[index].row =
            (current_corners[index].row - target_corners[index].row) * anticipation;
        velocity_corners[index].col =
            (current_corners[index].col - target_corners[index].col) * anticipation;
    }
    velocity_corners
}

pub(crate) fn compute_stiffnesses(
    config: &RuntimeConfig,
    mode: &str,
    current_corners: &[Point; 4],
    target_corners: &[Point; 4],
) -> [f64; 4] {
    let target_center = center(target_corners);
    let mut distances = [0.0; 4];
    let mut min_distance = f64::INFINITY;
    let mut max_distance = 0.0_f64;

    let (head_stiffness, trailing_stiffness, trailing_exponent) = if is_insert_like_mode(mode) {
        (
            config.stiffness_insert_mode,
            config.trailing_stiffness_insert_mode,
            config.trailing_exponent_insert_mode,
        )
    } else {
        (
            config.stiffness,
            config.trailing_stiffness,
            config.trailing_exponent,
        )
    };

    for (index, distance_ref) in distances.iter_mut().enumerate() {
        let distance = current_corners[index]
            .distance_squared(target_center)
            .sqrt();
        *distance_ref = distance;
        min_distance = min_distance.min(distance);
        max_distance = max_distance.max(distance);
    }

    if (max_distance - min_distance).abs() <= EPSILON {
        return [head_stiffness; 4];
    }

    let mut stiffnesses = [head_stiffness; 4];
    for (index, distance) in distances.iter().enumerate() {
        let x = (distance - min_distance) / (max_distance - min_distance);
        let stiffness =
            head_stiffness + (trailing_stiffness - head_stiffness) * x.powf(trailing_exponent);
        stiffnesses[index] = stiffness.min(1.0);
    }

    stiffnesses
}

pub(crate) fn reached_target(
    config: &RuntimeConfig,
    _mode: &str,
    current_corners: &[Point; 4],
    target_corners: &[Point; 4],
    velocity_corners: &[Point; 4],
    particles: &[Particle],
) -> bool {
    let mut max_distance = 0.0_f64;
    let mut max_velocity = 0.0_f64;
    let mut left_bound = f64::INFINITY;
    let mut right_bound = f64::NEG_INFINITY;

    for index in 0..4 {
        let distance = current_corners[index]
            .distance_squared(target_corners[index])
            .sqrt();
        max_distance = max_distance.max(distance);

        let velocity = velocity_corners[index].distance_squared(Point::ZERO).sqrt();
        max_velocity = max_velocity.max(velocity);

        left_bound = left_bound.min(current_corners[index].col);
        right_bound = right_bound.max(current_corners[index].col);
    }

    let thickness = right_bound - left_bound;
    let default_reached = max_distance <= config.distance_stop_animating
        && max_velocity <= config.distance_stop_animating;
    let vertical_bar_reached = thickness <= 1.5 / 8.0
        && max_distance <= config.distance_stop_animating_vertical_bar
        && max_velocity <= config.distance_stop_animating_vertical_bar;

    (default_reached || vertical_bar_reached) && particles.is_empty()
}

fn normalize(vector: Point) -> Point {
    let length = (vector.row * vector.row + vector.col * vector.col).sqrt();
    if length <= EPSILON {
        return Point::ZERO;
    }

    Point {
        row: vector.row / length,
        col: vector.col / length,
    }
}

fn shrink_volume(
    config: &RuntimeConfig,
    current_corners: &[Point; 4],
    target_corners: &[Point; 4],
) -> [Point; 4] {
    let edge_1 = Point {
        row: current_corners[1].row - current_corners[0].row,
        col: current_corners[1].col - current_corners[0].col,
    };
    let edge_2 = Point {
        row: current_corners[2].row - current_corners[0].row,
        col: current_corners[2].col - current_corners[0].col,
    };

    let volume = edge_1.col * edge_2.row - edge_1.row * edge_2.col;
    if !volume.is_finite() || volume <= EPSILON {
        return *current_corners;
    }

    let mut factor = (1.0 / volume).powf(config.volume_reduction_exponent / 2.0);
    factor = factor.max(config.minimum_volume_factor);
    if !factor.is_finite() {
        return *current_corners;
    }

    let center = center(current_corners);
    let mut shrunk = [Point::ZERO; 4];

    for index in 0..4 {
        let corner_to_target = Point {
            row: target_corners[index].row - current_corners[index].row,
            col: target_corners[index].col - current_corners[index].col,
        };
        let center_to_corner = Point {
            row: current_corners[index].row - center.row,
            col: current_corners[index].col - center.col,
        };
        let normal = normalize(Point {
            row: -corner_to_target.col,
            col: corner_to_target.row,
        });
        let projection = center_to_corner.row * normal.row + center_to_corner.col * normal.col;
        let shift = projection * (1.0 - factor);

        shrunk[index] = Point {
            row: current_corners[index].row - normal.row * shift,
            col: current_corners[index].col - normal.col * shift,
        };
    }

    shrunk
}

pub(crate) fn corners_for_render(
    config: &RuntimeConfig,
    current_corners: &[Point; 4],
    target_corners: &[Point; 4],
) -> [Point; 4] {
    let current_center = center(current_corners);
    let target_center = center(target_corners);
    let straight_line = (target_center.row - current_center.row).abs() < 1.0 / 8.0
        || (target_center.col - current_center.col).abs() < 1.0 / 8.0;

    if straight_line {
        *current_corners
    } else {
        shrink_volume(config, current_corners, target_corners)
    }
}

fn update_corners(input: &StepInput) -> ([Point; 4], [Point; 4], usize, usize, bool) {
    let mut current_corners = input.current_corners;
    let mut velocity_corners = input.velocity_corners;

    let mut distance_head_to_target_squared = f64::INFINITY;
    let mut distance_tail_to_target_squared = 0.0;
    let mut index_head = 0_usize;
    let mut index_tail = 0_usize;

    let max_length = if is_insert_like_mode(&input.mode) {
        input.max_length_insert_mode
    } else {
        input.max_length
    };

    let damping = if is_insert_like_mode(&input.mode) {
        input.damping_insert_mode
    } else {
        input.damping
    };

    let speed = speed_correction(input.time_interval);
    let velocity_conservation = velocity_conservation_factor(damping, input.time_interval);
    let damping_correction_factor = 1.0 / (1.0 + 2.5 * velocity_conservation);

    for index in 0..4 {
        let distance_squared = current_corners[index].distance_squared(input.target_corners[index]);

        if distance_squared < distance_head_to_target_squared {
            distance_head_to_target_squared = distance_squared;
            index_head = index;
        }
        if distance_squared > distance_tail_to_target_squared {
            distance_tail_to_target_squared = distance_squared;
            index_tail = index;
        }

        let stiffness_base =
            (1.0 - input.stiffnesses[index] * damping_correction_factor).clamp(EPSILON, 1.0);
        let stiffness = 1.0 - (stiffness_base.ln() * speed).exp();

        velocity_corners[index].row +=
            (input.target_corners[index].row - current_corners[index].row) * stiffness;
        velocity_corners[index].col +=
            (input.target_corners[index].col - current_corners[index].col) * stiffness;

        current_corners[index].row += velocity_corners[index].row;
        current_corners[index].col += velocity_corners[index].col;

        velocity_corners[index].row *= velocity_conservation;
        velocity_corners[index].col *= velocity_conservation;
    }

    let disabled_due_to_delay = input.delay_disable.is_some_and(|delay_disable| {
        distance_head_to_target_squared > (1.0_f64 / 8.0).powi(2)
            && input.time_interval > delay_disable
    });

    let mut smear_length = 0.0_f64;
    for index in 0..4 {
        if index == index_head {
            continue;
        }
        let distance = current_corners[index]
            .distance_squared(current_corners[index_head])
            .sqrt();
        smear_length = smear_length.max(distance);
    }

    if smear_length > max_length {
        let factor = max_length / smear_length;
        for index in 0..4 {
            if index == index_head {
                continue;
            }
            current_corners[index].row = current_corners[index_head].row
                + (current_corners[index].row - current_corners[index_head].row) * factor;
            current_corners[index].col = current_corners[index_head].col
                + (current_corners[index].col - current_corners[index_head].col) * factor;
        }
    }

    (
        current_corners,
        velocity_corners,
        index_head,
        index_tail,
        disabled_due_to_delay,
    )
}

#[derive(Debug)]
struct ParticleStepResult {
    particles: Vec<Particle>,
    previous_center: Point,
}

fn update_particles(
    input: &StepInput,
    current_corners: &[Point; 4],
    velocity_corners: &[Point; 4],
    rng: &mut Rng32,
) -> ParticleStepResult {
    let mut particles = input.particles.clone();
    let velocity_conservation =
        velocity_conservation_factor(input.particle_damping, input.time_interval);
    let dt = input.config_time_interval / 1000.0;

    let mut index = 0_usize;
    while index < particles.len() {
        let particle = &mut particles[index];
        particle.lifetime -= input.time_interval;

        if particle.lifetime <= 0.0 {
            let _ = particles.remove(index);
            continue;
        }

        particle.velocity.row = (particle.velocity.row
            + (input.particle_gravity + input.particle_random_velocity * (rng.next_unit() - 0.5))
                * dt)
            * velocity_conservation;

        particle.velocity.col = particle.velocity.col * velocity_conservation
            + input.particle_random_velocity * (rng.next_unit() - 0.5) * dt;

        particle.position.row += (particle.velocity.row * dt) / input.block_aspect_ratio;
        particle.position.col += particle.velocity.col * dt;

        index += 1;
    }

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

pub(crate) fn simulate_step(input: StepInput) -> StepOutput {
    let (current_corners, velocity_corners, index_head, index_tail, disabled_due_to_delay) =
        update_corners(&input);

    let mut rng = Rng32::from_seed(input.rng_state);
    let particle_step = update_particles(&input, &current_corners, &velocity_corners, &mut rng);

    StepOutput {
        current_corners,
        velocity_corners,
        particles: particle_step.particles,
        previous_center: particle_step.previous_center,
        index_head,
        index_tail,
        disabled_due_to_delay,
        rng_state: rng.state(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{DEFAULT_RNG_STATE, StepInput};

    fn make_input() -> StepInput {
        StepInput {
            mode: "n".to_string(),
            time_interval: 17.0,
            config_time_interval: 17.0,
            current_corners: [
                Point { row: 1.0, col: 1.0 },
                Point { row: 1.0, col: 2.0 },
                Point { row: 2.0, col: 2.0 },
                Point { row: 2.0, col: 1.0 },
            ],
            target_corners: [
                Point {
                    row: 1.0,
                    col: 10.0,
                },
                Point {
                    row: 1.0,
                    col: 11.0,
                },
                Point {
                    row: 2.0,
                    col: 11.0,
                },
                Point {
                    row: 2.0,
                    col: 10.0,
                },
            ],
            velocity_corners: [Point::ZERO; 4],
            stiffnesses: [0.6, 0.55, 0.5, 0.45],
            max_length: 25.0,
            max_length_insert_mode: 1.0,
            damping: 0.85,
            damping_insert_mode: 0.9,
            delay_disable: Some(250.0),
            particles: Vec::new(),
            previous_center: Point { row: 1.5, col: 1.5 },
            particle_damping: 0.2,
            particles_enabled: true,
            particle_gravity: 20.0,
            particle_random_velocity: 100.0,
            particle_max_num: 100,
            particle_spread: 0.5,
            particles_per_second: 200.0,
            particles_per_length: 1.0,
            particle_max_initial_velocity: 10.0,
            particle_velocity_from_cursor: 0.2,
            particle_max_lifetime: 300.0,
            particle_lifetime_distribution_exponent: 5.0,
            min_distance_emit_particles: 0.1,
            vertical_bar: false,
            horizontal_bar: false,
            block_aspect_ratio: 2.0,
            rng_state: DEFAULT_RNG_STATE,
        }
    }

    #[test]
    fn update_moves_toward_target() {
        let input = make_input();
        let output = simulate_step(input);
        assert!(output.current_corners[0].col > 1.0);
        assert!(output.index_head < 4);
        assert!(output.index_tail < 4);
    }

    #[test]
    fn seeded_rng_is_deterministic() {
        let input_a = make_input();
        let input_b = make_input();
        let output_a = simulate_step(input_a);
        let output_b = simulate_step(input_b);
        assert_eq!(output_a.rng_state, output_b.rng_state);
        assert_eq!(output_a.particles.len(), output_b.particles.len());
    }
}
