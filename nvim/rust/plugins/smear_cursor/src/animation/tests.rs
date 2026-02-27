use super::*;
use crate::types::{DEFAULT_RNG_STATE, StepInput};

fn make_input() -> StepInput {
    StepInput {
        mode: "n".to_string(),
        time_interval: BASE_TIME_INTERVAL,
        config_time_interval: BASE_TIME_INTERVAL,
        head_response_ms: 110.0,
        damping_ratio: 1.0,
        current_corners: [
            Point { row: 1.0, col: 1.0 },
            Point { row: 1.0, col: 2.0 },
            Point { row: 2.0, col: 2.0 },
            Point { row: 2.0, col: 1.0 },
        ],
        trail_origin_corners: [
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
        spring_velocity_corners: [Point::ZERO; 4],
        trail_elapsed_ms: [0.0; 4],
        max_length: 25.0,
        max_length_insert_mode: 1.0,
        trail_duration_ms: 200.0,
        trail_short_duration_ms: 40.0,
        trail_size: 0.8,
        trail_min_distance: 0.0,
        trail_thickness: 1.0,
        trail_thickness_x: 1.0,
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

fn center_col(corners: &[Point; 4]) -> f64 {
    center(corners).col
}

fn replay_step(mut input: StepInput, steps: usize) -> Vec<([Point; 4], [Point; 4])> {
    let mut trace = Vec::with_capacity(steps);
    for _ in 0..steps {
        let output = simulate_step(input.clone());
        trace.push((output.current_corners, output.spring_velocity_corners));
        input.current_corners = output.current_corners;
        input.spring_velocity_corners = output.spring_velocity_corners;
        input.trail_elapsed_ms = output.trail_elapsed_ms;
        input.previous_center = output.previous_center;
        input.rng_state = output.rng_state;
        input.particles = output.particles;
    }
    trace
}

#[test]
fn update_moves_toward_target_with_rigid_corner_offsets() {
    let input = make_input();
    let target_corners = input.target_corners;
    let output = simulate_step(input);
    assert!(output.current_corners[0].col > 1.0);
    assert!(output.index_head < 4);
    assert!(output.index_tail < 4);

    let reference_offset = Point {
        row: output.current_corners[0].row - target_corners[0].row,
        col: output.current_corners[0].col - target_corners[0].col,
    };
    for (index, corner) in output.current_corners.into_iter().enumerate() {
        let offset = Point {
            row: corner.row - target_corners[index].row,
            col: corner.col - target_corners[index].col,
        };
        assert!(
            (offset.row - reference_offset.row).abs() <= 1.0e-9
                && (offset.col - reference_offset.col).abs() <= 1.0e-9
        );
    }
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

#[test]
fn stop_enter_boundary_is_inclusive() {
    let config = RuntimeConfig {
        stop_distance_enter: 0.25,
        stop_velocity_enter: 0.15,
        ..RuntimeConfig::default()
    };

    let metrics = StopMetrics {
        max_distance: 0.25,
        max_velocity: 0.15,
        particles_empty: true,
    };

    assert!(within_stop_enter(&config, metrics));
}

#[test]
fn stop_metrics_use_display_metric_for_vertical_distance_and_velocity() {
    let current_corners = corners_for_cursor(1.0, 1.0, false, false);
    let target_corners = corners_for_cursor(1.25, 1.0, false, false);
    let velocity_corners = [Point {
        row: 0.25,
        col: 0.0,
    }; 4];

    let metrics = stop_metrics(
        &current_corners,
        &target_corners,
        &velocity_corners,
        2.0,
        &[],
    );

    assert!((metrics.max_distance - 0.5).abs() <= 1.0e-9);
    assert!((metrics.max_velocity - 0.5).abs() <= 1.0e-9);
}

#[test]
fn stop_exit_boundary_is_inclusive_for_distance_and_velocity() {
    let config = RuntimeConfig {
        stop_distance_exit: 0.4,
        stop_velocity_enter: 0.2,
        ..RuntimeConfig::default()
    };

    let at_distance_boundary = StopMetrics {
        max_distance: 0.4,
        max_velocity: 0.01,
        particles_empty: true,
    };
    assert!(outside_stop_exit(&config, at_distance_boundary));

    let at_velocity_boundary = StopMetrics {
        max_distance: 0.01,
        max_velocity: 0.2,
        particles_empty: true,
    };
    assert!(outside_stop_exit(&config, at_velocity_boundary));
}

#[test]
fn stop_enter_thresholds_match_for_equal_display_space_motion() {
    let config = RuntimeConfig {
        stop_distance_enter: 0.5,
        stop_velocity_enter: 0.5,
        ..RuntimeConfig::default()
    };

    let aspect_one = stop_metrics(
        &corners_for_cursor(1.0, 1.0, false, false),
        &corners_for_cursor(1.5, 1.0, false, false),
        &[Point { row: 0.5, col: 0.0 }; 4],
        1.0,
        &[],
    );
    let aspect_two = stop_metrics(
        &corners_for_cursor(1.0, 1.0, false, false),
        &corners_for_cursor(1.25, 1.0, false, false),
        &[Point {
            row: 0.25,
            col: 0.0,
        }; 4],
        2.0,
        &[],
    );

    assert_eq!(
        within_stop_enter(&config, aspect_one),
        within_stop_enter(&config, aspect_two)
    );
}

#[test]
fn trail_min_distance_animates_when_movement_is_over_scaled_threshold() {
    let mut input = make_input();
    input.trail_min_distance = 0.25;
    input.target_corners = [
        Point { row: 1.0, col: 2.0 },
        Point { row: 1.0, col: 3.0 },
        Point { row: 2.0, col: 3.0 },
        Point { row: 2.0, col: 2.0 },
    ];
    let origin_col = input.trail_origin_corners[0].col;
    let target_corners = input.target_corners;

    let output = simulate_step(input);
    assert_ne!(output.current_corners, target_corners);
    assert!(
        output.current_corners[0].col > origin_col,
        "animation should advance toward target when over threshold"
    );
}

#[test]
fn trail_min_distance_snaps_when_movement_is_under_scaled_threshold() {
    let mut input = make_input();
    input.trail_min_distance = 1.0;
    input.target_corners = [
        Point { row: 1.0, col: 1.5 },
        Point { row: 1.0, col: 2.5 },
        Point { row: 2.0, col: 2.5 },
        Point { row: 2.0, col: 1.5 },
    ];
    let target_corners = input.target_corners;

    let output = simulate_step(input);
    assert_eq!(output.current_corners, target_corners);
}

#[test]
fn trail_min_distance_uses_display_metric_for_vertical_motion() {
    let mut input = make_input();
    input.block_aspect_ratio = 2.0;
    input.trail_min_distance = 1.0;
    input.target_corners = corners_for_cursor(1.6, 1.0, false, false);
    let target_corners = input.target_corners;

    let output = simulate_step(input);
    assert_ne!(output.current_corners, target_corners);
}

#[test]
fn step_response_is_monotonic_for_overdamped_filter() {
    let mut input = make_input();
    input.damping_ratio = 1.4;
    input.head_response_ms = 90.0;
    input.current_corners = corners_for_cursor(5.0, 1.0, false, false);
    input.trail_origin_corners = input.current_corners;
    input.target_corners = corners_for_cursor(5.0, 11.0, false, false);
    input.previous_center = center(&input.current_corners);

    let target_col = center_col(&input.target_corners);
    let mut previous_col = center_col(&input.current_corners);
    for _ in 0..240 {
        let output = simulate_step(input.clone());
        let next_col = center_col(&output.current_corners);
        assert!(
            next_col + 1.0e-9 >= previous_col,
            "overdamped response must not move backward: prev={previous_col} next={next_col}"
        );
        assert!(
            next_col <= target_col + 1.0e-9,
            "overdamped response must not overshoot: next={next_col} target={target_col}"
        );
        previous_col = next_col;
        input.current_corners = output.current_corners;
        input.spring_velocity_corners = output.spring_velocity_corners;
        input.trail_elapsed_ms = output.trail_elapsed_ms;
        input.previous_center = output.previous_center;
    }
    assert!(
        (previous_col - target_col).abs() <= 1.0e-3,
        "overdamped response should settle near target: settled={previous_col} target={target_col}"
    );
}

#[test]
fn step_response_underdamped_overshoot_is_bounded() {
    let mut input = make_input();
    input.damping_ratio = 0.4;
    input.head_response_ms = 90.0;
    input.current_corners = corners_for_cursor(5.0, 1.0, false, false);
    input.trail_origin_corners = input.current_corners;
    input.target_corners = corners_for_cursor(5.0, 11.0, false, false);
    input.previous_center = center(&input.current_corners);

    let target_col = center_col(&input.target_corners);
    let mut peak_col = center_col(&input.current_corners);
    let mut settled_col = peak_col;
    for _ in 0..360 {
        let output = simulate_step(input.clone());
        let next_col = center_col(&output.current_corners);
        peak_col = peak_col.max(next_col);
        settled_col = next_col;
        input.current_corners = output.current_corners;
        input.spring_velocity_corners = output.spring_velocity_corners;
        input.trail_elapsed_ms = output.trail_elapsed_ms;
        input.previous_center = output.previous_center;
    }

    let overshoot = peak_col - target_col;
    assert!(overshoot > 0.01, "underdamped response should overshoot");
    assert!(
        overshoot < 2.6,
        "underdamped overshoot should remain bounded: overshoot={overshoot}"
    );
    assert!(
        (settled_col - target_col).abs() <= 0.02,
        "underdamped response should settle near target: settled={settled_col} target={target_col}"
    );
}

#[test]
fn step_response_replay_is_deterministic() {
    let mut input_a = make_input();
    input_a.damping_ratio = 0.8;
    input_a.head_response_ms = 75.0;
    input_a.current_corners = corners_for_cursor(3.0, 2.0, false, false);
    input_a.trail_origin_corners = input_a.current_corners;
    input_a.target_corners = corners_for_cursor(7.0, 15.0, false, false);
    input_a.previous_center = center(&input_a.current_corners);

    let mut input_b = make_input();
    input_b.damping_ratio = 0.8;
    input_b.head_response_ms = 75.0;
    input_b.current_corners = corners_for_cursor(3.0, 2.0, false, false);
    input_b.trail_origin_corners = input_b.current_corners;
    input_b.target_corners = corners_for_cursor(7.0, 15.0, false, false);
    input_b.previous_center = center(&input_b.current_corners);

    let trace_a = replay_step(input_a, 180);
    let trace_b = replay_step(input_b, 180);
    assert_eq!(trace_a, trace_b);
}

#[test]
fn scaled_corners_for_trail_identity_keeps_geometry() {
    let corners = corners_for_cursor(2.0, 3.0, false, false);
    let scaled = scaled_corners_for_trail(&corners, 1.0, 1.0);
    assert_eq!(scaled, corners);
}

#[test]
fn scaled_corners_for_trail_shrinks_when_factor_is_below_one() {
    let corners = corners_for_cursor(2.0, 3.0, false, false);
    let scaled = scaled_corners_for_trail(&corners, 0.5, 0.5);

    let original_height = corners[3].row - corners[0].row;
    let original_width = corners[1].col - corners[0].col;
    let scaled_height = scaled[3].row - scaled[0].row;
    let scaled_width = scaled[1].col - scaled[0].col;

    assert!(scaled_height < original_height);
    assert!(scaled_width < original_width);
}

#[test]
fn scaled_corners_for_trail_expands_without_non_finite_values() {
    let corners = corners_for_cursor(2.0, 3.0, false, false);
    let scaled = scaled_corners_for_trail(&corners, 1.5, 1.25);

    let original_height = corners[3].row - corners[0].row;
    let original_width = corners[1].col - corners[0].col;
    let scaled_height = scaled[3].row - scaled[0].row;
    let scaled_width = scaled[1].col - scaled[0].col;

    assert!(scaled_height > original_height);
    assert!(scaled_width > original_width);
    assert!(
        scaled
            .iter()
            .all(|point| point.row.is_finite() && point.col.is_finite())
    );
}
