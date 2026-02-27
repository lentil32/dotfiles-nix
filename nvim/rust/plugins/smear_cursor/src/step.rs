use crate::animation::simulate_step;
use crate::lua::{
    bool_from_object, f64_from_object, i64_from_object, invalid_key, parse_indexed_objects,
    require_object, require_with, string_from_object,
};
use crate::types::{DEFAULT_RNG_STATE, Particle, Point, StepInput};
use nvim_oxi::serde::Deserializer;
use nvim_oxi::{Array, Dictionary, Object, Result};
use serde::Deserialize;

fn error(message: impl Into<String>) -> nvim_oxi::Error {
    nvim_oxi::api::Error::Other(message.into()).into()
}

fn require_f64(value: Option<Object>, key: &str) -> Result<f64> {
    require_with(value, key, f64_from_object)
}

fn require_i64(value: Option<Object>, key: &str) -> Result<i64> {
    require_with(value, key, i64_from_object)
}

fn require_bool(value: Option<Object>, key: &str) -> Result<bool> {
    require_with(value, key, bool_from_object)
}

fn require_string(value: Option<Object>, key: &str) -> Result<String> {
    require_with(value, key, string_from_object)
}

fn require_positive_f64(value: Option<Object>, key: &str) -> Result<f64> {
    let parsed = require_f64(value, key)?;
    if !parsed.is_finite() || parsed <= 0.0 {
        return Err(invalid_key(key, "positive number"));
    }
    Ok(parsed)
}

fn require_non_negative_f64(value: Option<Object>, key: &str) -> Result<f64> {
    let parsed = require_f64(value, key)?;
    if !parsed.is_finite() || parsed < 0.0 {
        return Err(invalid_key(key, "non-negative number"));
    }
    Ok(parsed)
}

fn parse_point_from_value(key: &str, value: Option<Object>) -> Result<Point> {
    parse_point_from_object(key, require_object(value, key)?)
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

fn parse_elapsed_ms_from_object(key: &str, value: Object) -> Result<[f64; 4]> {
    let elapsed_values =
        parse_indexed_objects(key, value, Some(4)).map_err(|_| invalid_key(key, "array[4]"))?;

    let mut parsed = [0.0; 4];
    for (index, elapsed_ms) in elapsed_values.into_iter().enumerate() {
        let value = f64_from_object(key, elapsed_ms)?;
        if !value.is_finite() || value < 0.0 {
            return Err(invalid_key(key, "array[4] of non-negative numbers"));
        }
        parsed[index] = value;
    }
    Ok(parsed)
}

fn parse_particles_from_object(key: &str, value: Object) -> Result<Vec<Particle>> {
    let entries =
        parse_indexed_objects(key, value, None).map_err(|_| invalid_key(key, "array[particle]"))?;
    let mut particles = Vec::with_capacity(entries.len());

    for entry in entries {
        let parsed = RawParticle::deserialize(Deserializer::new(entry))
            .map_err(|_| invalid_key(key, "array[particle]"))?;
        let position = parse_point_from_value("particles.position", parsed.position)?;
        let velocity = parse_point_from_value("particles.velocity", parsed.velocity)?;
        let lifetime = require_f64(parsed.lifetime, "particles.lifetime")?;
        particles.push(Particle {
            position,
            velocity,
            lifetime,
        });
    }

    Ok(particles)
}

fn parse_rng_state(value: Option<Object>) -> Result<u32> {
    match value {
        Some(value) if !value.is_nil() => {
            let parsed = i64_from_object("rng_state", value)?;
            let normalized = parsed.rem_euclid(i64::from(u32::MAX));
            u32::try_from(normalized).map_err(|_| invalid_key("rng_state", "integer"))
        }
        _ => Ok(DEFAULT_RNG_STATE),
    }
}

#[derive(Debug, Deserialize)]
struct RawParticle {
    #[serde(default)]
    position: Option<Object>,
    #[serde(default)]
    velocity: Option<Object>,
    #[serde(default)]
    lifetime: Option<Object>,
}

#[derive(Debug, Deserialize)]
struct RawStepInput {
    #[serde(default)]
    mode: Option<Object>,
    #[serde(default)]
    time_interval: Option<Object>,
    #[serde(default)]
    config_time_interval: Option<Object>,
    #[serde(default)]
    head_response_ms: Option<Object>,
    #[serde(default)]
    damping_ratio: Option<Object>,
    #[serde(default)]
    current_corners: Option<Object>,
    #[serde(default)]
    trail_origin_corners: Option<Object>,
    #[serde(default)]
    target_corners: Option<Object>,
    #[serde(default)]
    spring_velocity_corners: Option<Object>,
    #[serde(default)]
    trail_elapsed_ms: Option<Object>,
    #[serde(default)]
    max_length: Option<Object>,
    #[serde(default)]
    max_length_insert_mode: Option<Object>,
    #[serde(default)]
    trail_duration_ms: Option<Object>,
    #[serde(default)]
    trail_short_duration_ms: Option<Object>,
    #[serde(default)]
    trail_size: Option<Object>,
    #[serde(default)]
    trail_min_distance: Option<Object>,
    #[serde(default)]
    trail_thickness: Option<Object>,
    #[serde(default)]
    trail_thickness_x: Option<Object>,
    #[serde(default)]
    particles: Option<Object>,
    #[serde(default)]
    previous_center: Option<Object>,
    #[serde(default)]
    particle_damping: Option<Object>,
    #[serde(default)]
    particles_enabled: Option<Object>,
    #[serde(default)]
    particle_gravity: Option<Object>,
    #[serde(default)]
    particle_random_velocity: Option<Object>,
    #[serde(default)]
    particle_max_num: Option<Object>,
    #[serde(default)]
    particle_spread: Option<Object>,
    #[serde(default)]
    particles_per_second: Option<Object>,
    #[serde(default)]
    particles_per_length: Option<Object>,
    #[serde(default)]
    particle_max_initial_velocity: Option<Object>,
    #[serde(default)]
    particle_velocity_from_cursor: Option<Object>,
    #[serde(default)]
    particle_max_lifetime: Option<Object>,
    #[serde(default)]
    particle_lifetime_distribution_exponent: Option<Object>,
    #[serde(default)]
    min_distance_emit_particles: Option<Object>,
    #[serde(default)]
    vertical_bar: Option<Object>,
    #[serde(default)]
    horizontal_bar: Option<Object>,
    #[serde(default)]
    block_aspect_ratio: Option<Object>,
    #[serde(default)]
    rng_state: Option<Object>,
}

impl RawStepInput {
    fn into_step_input(self) -> Result<StepInput> {
        let mode = require_string(self.mode, "mode")?;
        let time_interval = require_f64(self.time_interval, "time_interval")?;
        let config_time_interval = require_f64(self.config_time_interval, "config_time_interval")?;
        let head_response_ms = require_positive_f64(self.head_response_ms, "head_response_ms")?;
        let damping_ratio = require_positive_f64(self.damping_ratio, "damping_ratio")?;
        let current_corners = parse_corners_from_object(
            "current_corners",
            require_object(self.current_corners, "current_corners")?,
        )?;
        let trail_origin_corners = match self.trail_origin_corners {
            Some(value) if !value.is_nil() => {
                parse_corners_from_object("trail_origin_corners", value)?
            }
            _ => current_corners,
        };
        let target_corners = parse_corners_from_object(
            "target_corners",
            require_object(self.target_corners, "target_corners")?,
        )?;
        let spring_velocity_corners = match self.spring_velocity_corners {
            Some(value) if !value.is_nil() => {
                parse_corners_from_object("spring_velocity_corners", value)?
            }
            _ => [Point::ZERO; 4],
        };
        let trail_elapsed_ms = match self.trail_elapsed_ms {
            Some(value) if !value.is_nil() => {
                parse_elapsed_ms_from_object("trail_elapsed_ms", value)?
            }
            _ => [0.0; 4],
        };
        let max_length = require_f64(self.max_length, "max_length")?;
        let max_length_insert_mode =
            require_f64(self.max_length_insert_mode, "max_length_insert_mode")?;
        let trail_duration_ms = require_positive_f64(self.trail_duration_ms, "trail_duration_ms")?;
        let trail_short_duration_ms =
            require_positive_f64(self.trail_short_duration_ms, "trail_short_duration_ms")?;
        let trail_size = require_f64(self.trail_size, "trail_size")?;
        if !(0.0..=1.0).contains(&trail_size) {
            return Err(invalid_key("trail_size", "number between 0.0 and 1.0"));
        }
        let trail_min_distance =
            require_non_negative_f64(self.trail_min_distance, "trail_min_distance")?;
        let trail_thickness = require_non_negative_f64(self.trail_thickness, "trail_thickness")?;
        let trail_thickness_x =
            require_non_negative_f64(self.trail_thickness_x, "trail_thickness_x")?;
        let particles =
            parse_particles_from_object("particles", require_object(self.particles, "particles")?)?;
        let previous_center = parse_point_from_object(
            "previous_center",
            require_object(self.previous_center, "previous_center")?,
        )?;
        let particle_damping = require_f64(self.particle_damping, "particle_damping")?;
        let particles_enabled = require_bool(self.particles_enabled, "particles_enabled")?;
        let particle_gravity = require_f64(self.particle_gravity, "particle_gravity")?;
        let particle_random_velocity =
            require_f64(self.particle_random_velocity, "particle_random_velocity")?;

        let particle_max_num_raw = require_i64(self.particle_max_num, "particle_max_num")?;
        let particle_max_num = usize::try_from(particle_max_num_raw)
            .map_err(|_| invalid_key("particle_max_num", "non-negative integer"))?;

        let particle_spread = require_f64(self.particle_spread, "particle_spread")?;
        let particles_per_second = require_f64(self.particles_per_second, "particles_per_second")?;
        let particles_per_length = require_f64(self.particles_per_length, "particles_per_length")?;
        let particle_max_initial_velocity = require_f64(
            self.particle_max_initial_velocity,
            "particle_max_initial_velocity",
        )?;
        let particle_velocity_from_cursor = require_f64(
            self.particle_velocity_from_cursor,
            "particle_velocity_from_cursor",
        )?;
        let particle_max_lifetime =
            require_f64(self.particle_max_lifetime, "particle_max_lifetime")?;
        let particle_lifetime_distribution_exponent = require_f64(
            self.particle_lifetime_distribution_exponent,
            "particle_lifetime_distribution_exponent",
        )?;
        let min_distance_emit_particles = require_f64(
            self.min_distance_emit_particles,
            "min_distance_emit_particles",
        )?;
        let vertical_bar = require_bool(self.vertical_bar, "vertical_bar")?;
        let horizontal_bar = require_bool(self.horizontal_bar, "horizontal_bar")?;
        let block_aspect_ratio = require_f64(self.block_aspect_ratio, "block_aspect_ratio")?;
        let rng_state = parse_rng_state(self.rng_state)?;

        Ok(StepInput {
            mode,
            time_interval,
            config_time_interval,
            head_response_ms,
            damping_ratio,
            current_corners,
            trail_origin_corners,
            target_corners,
            spring_velocity_corners,
            trail_elapsed_ms,
            max_length,
            max_length_insert_mode,
            trail_duration_ms,
            trail_short_duration_ms,
            trail_size,
            trail_min_distance,
            trail_thickness,
            trail_thickness_x,
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
}

fn parse_step_input(args: &Dictionary) -> Result<StepInput> {
    let raw = RawStepInput::deserialize(Deserializer::new(Object::from(args.clone())))
        .map_err(|err| error(format!("invalid step args: {err}")))?;
    raw.into_step_input()
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
    result.insert(
        "spring_velocity_corners",
        corners_to_object(&output.spring_velocity_corners),
    );
    result.insert(
        "trail_elapsed_ms",
        Object::from(Array::from_iter(output.trail_elapsed_ms)),
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
    result.insert("rng_state", i64::from(output.rng_state));
    Ok(result)
}

pub(crate) fn step(args: Dictionary) -> Result<Dictionary> {
    step_impl(args)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nvim_oxi::{Array, String as NvimString};

    fn point_object(row: f64, col: f64) -> Object {
        Object::from(Array::from_iter([Object::from(row), Object::from(col)]))
    }

    fn rect_object(row: f64, col: f64) -> Object {
        Object::from(Array::from_iter([
            point_object(row, col),
            point_object(row, col + 1.0),
            point_object(row + 1.0, col + 1.0),
            point_object(row + 1.0, col),
        ]))
    }

    fn particles_object() -> Object {
        let mut particle = Dictionary::new();
        particle.insert("position", point_object(1.2, 1.3));
        particle.insert("velocity", point_object(0.1, 0.2));
        particle.insert("lifetime", 100.0_f64);
        Object::from(Array::from_iter([Object::from(particle)]))
    }

    fn valid_step_args() -> Dictionary {
        let mut args = Dictionary::new();
        args.insert("mode", "n");
        args.insert("time_interval", 8.0_f64);
        args.insert("config_time_interval", 8.0_f64);
        args.insert("head_response_ms", 110.0_f64);
        args.insert("damping_ratio", 1.0_f64);
        args.insert("current_corners", rect_object(1.0, 1.0));
        args.insert("trail_origin_corners", rect_object(1.0, 1.0));
        args.insert("target_corners", rect_object(1.5, 2.0));
        args.insert(
            "trail_elapsed_ms",
            Array::from_iter([0.0_f64, 0.0, 0.0, 0.0]),
        );
        args.insert("max_length", 25.0_f64);
        args.insert("max_length_insert_mode", 1.0_f64);
        args.insert("trail_duration_ms", 200.0_f64);
        args.insert("trail_short_duration_ms", 40.0_f64);
        args.insert("trail_size", 0.8_f64);
        args.insert("trail_min_distance", 0.0_f64);
        args.insert("trail_thickness", 1.0_f64);
        args.insert("trail_thickness_x", 1.0_f64);
        args.insert("particles", particles_object());
        args.insert("previous_center", point_object(1.0, 1.0));
        args.insert("particle_damping", 0.2_f64);
        args.insert("particles_enabled", true);
        args.insert("particle_gravity", 20.0_f64);
        args.insert("particle_random_velocity", 100.0_f64);
        args.insert("particle_max_num", 100_i64);
        args.insert("particle_spread", 0.5_f64);
        args.insert("particles_per_second", 200.0_f64);
        args.insert("particles_per_length", 1.0_f64);
        args.insert("particle_max_initial_velocity", 10.0_f64);
        args.insert("particle_velocity_from_cursor", 0.2_f64);
        args.insert("particle_max_lifetime", 300.0_f64);
        args.insert("particle_lifetime_distribution_exponent", 5.0_f64);
        args.insert("min_distance_emit_particles", 0.1_f64);
        args.insert("vertical_bar", false);
        args.insert("horizontal_bar", false);
        args.insert("block_aspect_ratio", 2.0_f64);
        args
    }

    fn set_arg(args: &mut Dictionary, key: &str, value: Object) {
        let Some(slot) = args.get_mut(&NvimString::from(key)) else {
            panic!("missing key in fixture: {key}");
        };
        *slot = value;
    }

    #[test]
    fn parse_step_input_success() {
        let args = valid_step_args();
        let parsed = parse_step_input(&args).expect("expected valid step args");
        assert_eq!(parsed.mode, "n");
        assert_eq!(parsed.particle_max_num, 100);
        assert_eq!(parsed.rng_state, DEFAULT_RNG_STATE);
        assert_eq!(parsed.particles.len(), 1);
    }

    #[test]
    fn parse_step_input_missing_key() {
        let mut args = Dictionary::new();
        for (key, value) in valid_step_args() {
            if key.to_string_lossy() == "mode" {
                continue;
            }
            args.insert(key, value);
        }

        let err = parse_step_input(&args).expect_err("expected parse failure");
        assert!(
            err.to_string().contains("missing key"),
            "unexpected error: {err}"
        );
        assert!(err.to_string().contains("mode"), "unexpected error: {err}");
    }

    #[test]
    fn parse_step_input_rejects_negative_particle_max_num() {
        let mut args = valid_step_args();
        set_arg(&mut args, "particle_max_num", Object::from(-1_i64));
        let err = parse_step_input(&args).expect_err("expected parse failure");
        assert!(
            err.to_string().contains("particle_max_num"),
            "unexpected error: {err}"
        );
        assert!(
            err.to_string().contains("non-negative integer"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn parse_step_input_accepts_integral_float_particle_max_num() {
        let mut args = valid_step_args();
        set_arg(&mut args, "particle_max_num", Object::from(12.0_f64));
        let parsed = parse_step_input(&args).expect("expected parse success");
        assert_eq!(parsed.particle_max_num, 12);
    }

    #[test]
    fn parse_step_input_nil_rng_uses_default() {
        let mut args = valid_step_args();
        args.insert("rng_state", Object::nil());
        let parsed = parse_step_input(&args).expect("expected parse success");
        assert_eq!(parsed.rng_state, DEFAULT_RNG_STATE);
    }

    #[test]
    fn parse_step_input_rejects_out_of_range_trail_size() {
        let mut args = valid_step_args();
        set_arg(&mut args, "trail_size", Object::from(1.5_f64));
        let err = parse_step_input(&args).expect_err("expected parse failure");
        assert!(
            err.to_string().contains("trail_size"),
            "unexpected error: {err}"
        );
    }
}
