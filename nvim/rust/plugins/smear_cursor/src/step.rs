use crate::animation::simulate_step;
use crate::lua::{
    f64_from_object, get_bool, get_f64, get_i64, get_object, get_optional_f64, get_string,
    i64_from_object, invalid_key, parse_indexed_objects,
};
use crate::types::{DEFAULT_RNG_STATE, Particle, Point, StepInput};
use nvim_oxi::conversion::FromObject;
use nvim_oxi::{Array, Dictionary, Object, Result, String as NvimString};

fn error(message: impl Into<String>) -> nvim_oxi::Error {
    nvim_oxi::api::Error::Other(message.into()).into()
}

fn parse_point_from_object(key: &str, value: Object) -> Result<Point> {
    let values = parse_indexed_objects(key, value, Some(2))?;
    let row = f64_from_object(key, values[0].clone())?;
    let col = f64_from_object(key, values[1].clone())?;
    Ok(Point { row, col })
}

fn parse_corners_from_object(key: &str, value: Object) -> Result<[Point; 4]> {
    let corners =
        parse_indexed_objects(key, value, Some(4)).map_err(|_| invalid_key(key, "array[4][2]"))?;

    let mut parsed = [Point::ZERO; 4];
    for (index, corner) in corners.into_iter().enumerate() {
        parsed[index] = parse_point_from_object(key, corner)?;
    }

    Ok(parsed)
}

fn parse_stiffnesses_from_object(key: &str, value: Object) -> Result<[f64; 4]> {
    let stiffnesses =
        parse_indexed_objects(key, value, Some(4)).map_err(|_| invalid_key(key, "array[4]"))?;

    let mut parsed = [0.0; 4];
    for (index, stiffness) in stiffnesses.into_iter().enumerate() {
        parsed[index] = f64_from_object(key, stiffness)?;
    }
    Ok(parsed)
}

fn parse_particles_from_object(key: &str, value: Object) -> Result<Vec<Particle>> {
    let entries =
        parse_indexed_objects(key, value, None).map_err(|_| invalid_key(key, "array[particle]"))?;
    let mut particles = Vec::with_capacity(entries.len());

    for entry in entries {
        let entry =
            Dictionary::from_object(entry).map_err(|_| invalid_key(key, "array[particle]"))?;
        let position =
            parse_point_from_object("particles.position", get_object(&entry, "position")?)?;
        let velocity =
            parse_point_from_object("particles.velocity", get_object(&entry, "velocity")?)?;
        let lifetime = get_f64(&entry, "lifetime")?;
        particles.push(Particle {
            position,
            velocity,
            lifetime,
        });
    }

    Ok(particles)
}

fn parse_step_input(args: &Dictionary) -> Result<StepInput> {
    let mode = get_string(args, "mode")?;
    let time_interval = get_f64(args, "time_interval")?;
    let config_time_interval = get_f64(args, "config_time_interval")?;
    let current_corners =
        parse_corners_from_object("current_corners", get_object(args, "current_corners")?)?;
    let target_corners =
        parse_corners_from_object("target_corners", get_object(args, "target_corners")?)?;
    let velocity_corners =
        parse_corners_from_object("velocity_corners", get_object(args, "velocity_corners")?)?;
    let stiffnesses =
        parse_stiffnesses_from_object("stiffnesses", get_object(args, "stiffnesses")?)?;
    let max_length = get_f64(args, "max_length")?;
    let max_length_insert_mode = get_f64(args, "max_length_insert_mode")?;
    let damping = get_f64(args, "damping")?;
    let damping_insert_mode = get_f64(args, "damping_insert_mode")?;
    let delay_disable = get_optional_f64(args, "delay_disable")?;
    let particles = parse_particles_from_object("particles", get_object(args, "particles")?)?;
    let previous_center =
        parse_point_from_object("previous_center", get_object(args, "previous_center")?)?;
    let particle_damping = get_f64(args, "particle_damping")?;
    let particles_enabled = get_bool(args, "particles_enabled")?;
    let particle_gravity = get_f64(args, "particle_gravity")?;
    let particle_random_velocity = get_f64(args, "particle_random_velocity")?;

    let particle_max_num_raw = get_i64(args, "particle_max_num")?;
    let particle_max_num = usize::try_from(particle_max_num_raw)
        .map_err(|_| invalid_key("particle_max_num", "non-negative integer"))?;

    let particle_spread = get_f64(args, "particle_spread")?;
    let particles_per_second = get_f64(args, "particles_per_second")?;
    let particles_per_length = get_f64(args, "particles_per_length")?;
    let particle_max_initial_velocity = get_f64(args, "particle_max_initial_velocity")?;
    let particle_velocity_from_cursor = get_f64(args, "particle_velocity_from_cursor")?;
    let particle_max_lifetime = get_f64(args, "particle_max_lifetime")?;
    let particle_lifetime_distribution_exponent =
        get_f64(args, "particle_lifetime_distribution_exponent")?;
    let min_distance_emit_particles = get_f64(args, "min_distance_emit_particles")?;
    let vertical_bar = get_bool(args, "vertical_bar")?;
    let horizontal_bar = get_bool(args, "horizontal_bar")?;
    let block_aspect_ratio = get_f64(args, "block_aspect_ratio")?;

    let rng_state = match args.get(&NvimString::from("rng_state")).cloned() {
        Some(value) if !value.is_nil() => {
            let parsed = i64_from_object("rng_state", value)?;
            let normalized = parsed.rem_euclid(i64::from(u32::MAX));
            u32::try_from(normalized).map_err(|_| invalid_key("rng_state", "integer"))?
        }
        _ => DEFAULT_RNG_STATE,
    };

    Ok(StepInput {
        mode,
        time_interval,
        config_time_interval,
        current_corners,
        target_corners,
        velocity_corners,
        stiffnesses,
        max_length,
        max_length_insert_mode,
        damping,
        damping_insert_mode,
        delay_disable,
        particles,
        previous_center,
        particle_damping,
        particles_enabled,
        particle_gravity,
        particle_random_velocity,
        particle_max_num,
        particle_spread,
        particles_per_second,
        particles_per_length,
        particle_max_initial_velocity,
        particle_velocity_from_cursor,
        particle_max_lifetime,
        particle_lifetime_distribution_exponent,
        min_distance_emit_particles,
        vertical_bar,
        horizontal_bar,
        block_aspect_ratio,
        rng_state,
    })
}

fn point_to_object(point: Point) -> Object {
    Object::from(Array::from_iter([
        Object::from(point.row),
        Object::from(point.col),
    ]))
}

fn corners_to_object(corners: &[Point; 4]) -> Object {
    Object::from(Array::from_iter(
        corners.iter().copied().map(point_to_object),
    ))
}

fn particle_to_object(particle: &Particle) -> Object {
    let mut obj = Dictionary::new();
    obj.insert("position", point_to_object(particle.position));
    obj.insert("velocity", point_to_object(particle.velocity));
    obj.insert("lifetime", particle.lifetime);
    Object::from(obj)
}

fn particles_to_object(particles: &[Particle]) -> Object {
    Object::from(Array::from_iter(particles.iter().map(particle_to_object)))
}

fn one_based_i64(value: usize, field: &str) -> Result<i64> {
    let next = value
        .checked_add(1)
        .ok_or_else(|| error(format!("{field} overflow")))?;
    i64::try_from(next).map_err(|_| error(format!("{field} overflow")))
}

fn step_impl(args: Dictionary) -> Result<Dictionary> {
    let input = parse_step_input(&args)?;
    let output = simulate_step(input);

    let mut result = Dictionary::new();
    result.insert("ok", true);
    result.insert(
        "current_corners",
        corners_to_object(&output.current_corners),
    );
    result.insert(
        "velocity_corners",
        corners_to_object(&output.velocity_corners),
    );
    result.insert("particles", particles_to_object(&output.particles));
    result.insert("previous_center", point_to_object(output.previous_center));
    result.insert(
        "index_head",
        one_based_i64(output.index_head, "index_head")?,
    );
    result.insert(
        "index_tail",
        one_based_i64(output.index_tail, "index_tail")?,
    );
    result.insert("disabled_due_to_delay", output.disabled_due_to_delay);
    result.insert("rng_state", i64::from(output.rng_state));
    Ok(result)
}

pub(crate) fn step(args: Dictionary) -> Result<Dictionary> {
    let guarded = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| step_impl(args)));
    match guarded {
        Ok(result) => result,
        Err(_) => Err(error("rs_smear_cursor.step panicked")),
    }
}
