use super::*;
use crate::test_support::proptest::approx_eq_f64;
use crate::test_support::proptest::approx_eq_point;
use crate::test_support::proptest::cursor_rectangle;
use crate::test_support::proptest::positive_aspect_ratio;
use crate::test_support::proptest::positive_scale;
use crate::test_support::proptest::pure_config;
use crate::types::DEFAULT_RNG_STATE;
use crate::types::ModeClass;
use crate::types::Particle;
use crate::types::StepInput;
use proptest::prelude::*;
use std::f64::consts::PI;

fn make_input() -> StepInput {
    StepInput {
        mode: ModeClass::NormalLike,
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

#[derive(Clone, Debug, PartialEq)]
struct StepTrace {
    current_corners: [Point; 4],
    velocity_corners: [Point; 4],
    spring_velocity_corners: [Point; 4],
    trail_elapsed_ms: [f64; 4],
    particles: Vec<Particle>,
    previous_center: Point,
    index_head: usize,
    index_tail: usize,
    rng_state: u32,
}

fn step_trace(output: StepOutput) -> StepTrace {
    StepTrace {
        current_corners: output.current_corners,
        velocity_corners: output.velocity_corners,
        spring_velocity_corners: output.spring_velocity_corners,
        trail_elapsed_ms: output.trail_elapsed_ms,
        particles: output.particles,
        previous_center: output.previous_center,
        index_head: output.index_head,
        index_tail: output.index_tail,
        rng_state: output.rng_state,
    }
}

fn replay_step(mut input: StepInput, steps: usize) -> Vec<StepTrace> {
    let mut trace = Vec::with_capacity(steps);
    for _ in 0..steps {
        let output = simulate_step(input.clone());
        trace.push(step_trace(output));
        let output = trace.last().expect("trace entry was just pushed");
        input.current_corners = output.current_corners;
        input.spring_velocity_corners = output.spring_velocity_corners;
        input.trail_elapsed_ms = output.trail_elapsed_ms;
        input.previous_center = output.previous_center;
        input.rng_state = output.rng_state;
        input.particles = output.particles.clone();
    }
    trace
}

fn cursor_shape_flags() -> BoxedStrategy<(bool, bool)> {
    prop_oneof![
        Just((false, false)),
        Just((true, false)),
        Just((false, true)),
    ]
    .boxed()
}

#[derive(Clone, Copy, Debug)]
struct PoseInputSpec {
    start: Point,
    target: Point,
    vertical_bar: bool,
    horizontal_bar: bool,
    damping_ratio: f64,
    head_response_ms: f64,
    block_aspect_ratio: f64,
    rng_state: u32,
}

fn make_pose_input(spec: PoseInputSpec) -> StepInput {
    let mut input = make_input();
    input.damping_ratio = spec.damping_ratio;
    input.head_response_ms = spec.head_response_ms;
    input.block_aspect_ratio = spec.block_aspect_ratio;
    input.rng_state = spec.rng_state;
    input.vertical_bar = spec.vertical_bar;
    input.horizontal_bar = spec.horizontal_bar;
    input.current_corners = corners_for_cursor(
        spec.start.row,
        spec.start.col,
        spec.vertical_bar,
        spec.horizontal_bar,
    );
    input.trail_origin_corners = input.current_corners;
    input.target_corners = corners_for_cursor(
        spec.target.row,
        spec.target.col,
        spec.vertical_bar,
        spec.horizontal_bar,
    );
    input.previous_center = center(&input.current_corners);
    input
}

fn theoretical_overshoot_ratio(damping_ratio: f64) -> f64 {
    if damping_ratio >= 1.0 {
        return 0.0;
    }

    let denominator = (1.0 - damping_ratio * damping_ratio).sqrt();
    (-damping_ratio * PI / denominator).exp()
}

fn cursor_height_factor(vertical_bar: bool, horizontal_bar: bool) -> f64 {
    match (vertical_bar, horizontal_bar) {
        (_, true) => 1.0 / 8.0,
        _ => 1.0,
    }
}

fn corners_height(corners: &[Point; 4]) -> f64 {
    corners[3].row - corners[0].row
}

fn corners_width(corners: &[Point; 4]) -> f64 {
    corners[1].col - corners[0].col
}

mod simulation_step_behavior {
    use super::*;

    proptest! {
        #![proptest_config(pure_config())]

        #[test]
        fn prop_update_moves_toward_target_with_rigid_corner_offsets(
            start_row in 12_i64..52_i64,
            start_col in 12_i64..52_i64,
            delta_row in -8_i64..9_i64,
            delta_col in 1_i64..25_i64,
            (vertical_bar, horizontal_bar) in cursor_shape_flags(),
            damping_ratio in 0.25_f64..1.8_f64,
            head_response_ms in 45.0_f64..180.0_f64,
            block_aspect_ratio in 0.5_f64..4.0_f64,
        ) {
            let start = Point {
                row: start_row as f64,
                col: start_col as f64,
            };
            let target = Point {
                row: (start_row + delta_row) as f64,
                col: (start_col + delta_col) as f64,
            };
            let input = make_pose_input(PoseInputSpec {
                start,
                target,
                vertical_bar,
                horizontal_bar,
                damping_ratio,
                head_response_ms,
                block_aspect_ratio,
                rng_state: DEFAULT_RNG_STATE,
            });
            let target_corners = input.target_corners;
            let start_distance = center(&input.current_corners)
                .display_distance(center(&target_corners), block_aspect_ratio);

            let output = simulate_step(input);
            let next_distance = center(&output.current_corners)
                .display_distance(center(&target_corners), block_aspect_ratio);
            let reference_offset = Point {
                row: output.current_corners[0].row - target_corners[0].row,
                col: output.current_corners[0].col - target_corners[0].col,
            };

            prop_assert!(next_distance < start_distance, "next_distance={next_distance} start_distance={start_distance}");
            prop_assert!(output.index_head < 4, "head index out of bounds: {}", output.index_head);
            prop_assert!(output.index_tail < 4, "tail index out of bounds: {}", output.index_tail);

            for (index, corner) in output.current_corners.into_iter().enumerate() {
                let offset = Point {
                    row: corner.row - target_corners[index].row,
                    col: corner.col - target_corners[index].col,
                };
                prop_assert!(
                    approx_eq_point(offset, reference_offset, 1.0e-9),
                    "corner {index} lost rigid offset: offset={offset:?} reference={reference_offset:?}"
                );
            }
        }

        #[test]
        fn prop_seeded_replay_is_deterministic(
            start_row in 12_i64..52_i64,
            start_col in 12_i64..52_i64,
            delta_row in -8_i64..9_i64,
            delta_col in 1_i64..25_i64,
            (vertical_bar, horizontal_bar) in cursor_shape_flags(),
            damping_ratio in 0.25_f64..1.8_f64,
            head_response_ms in 45.0_f64..180.0_f64,
            block_aspect_ratio in 0.5_f64..4.0_f64,
            rng_state in any::<u32>(),
            steps in 1_usize..181_usize,
        ) {
            let start = Point {
                row: start_row as f64,
                col: start_col as f64,
            };
            let target = Point {
                row: (start_row + delta_row) as f64,
                col: (start_col + delta_col) as f64,
            };
            let spec = PoseInputSpec {
                start,
                target,
                vertical_bar,
                horizontal_bar,
                damping_ratio,
                head_response_ms,
                block_aspect_ratio,
                rng_state,
            };
            let input_a = make_pose_input(spec);
            let input_b = make_pose_input(spec);

            let trace_a = replay_step(input_a, steps);
            let trace_b = replay_step(input_b, steps);
            prop_assert_eq!(trace_a, trace_b);
        }

        #[test]
        fn prop_step_response_is_monotonic_for_overdamped_filter(
            row in 2_i64..64_i64,
            start_col in 1_i64..24_i64,
            travel_cols in 1_i64..25_i64,
            damping_ratio in 1.05_f64..2.5_f64,
            head_response_ms in 45.0_f64..180.0_f64,
        ) {
            let start = Point {
                row: row as f64,
                col: start_col as f64,
            };
            let target = Point {
                row: row as f64,
                col: (start_col + travel_cols) as f64,
            };
            let mut input = make_pose_input(PoseInputSpec {
                start,
                target,
                vertical_bar: false,
                horizontal_bar: false,
                damping_ratio,
                head_response_ms,
                block_aspect_ratio: 2.0,
                rng_state: DEFAULT_RNG_STATE,
            });

            let target_col = center_col(&input.target_corners);
            let mut previous_col = center_col(&input.current_corners);
            for _ in 0..480 {
                let output = simulate_step(input.clone());
                let next_col = center_col(&output.current_corners);
                prop_assert!(
                    next_col + 1.0e-9 >= previous_col,
                    "overdamped response moved backward: prev={previous_col} next={next_col}"
                );
                prop_assert!(
                    next_col <= target_col + 1.0e-9,
                    "overdamped response overshot: next={next_col} target={target_col}"
                );
                previous_col = next_col;
                input.current_corners = output.current_corners;
                input.spring_velocity_corners = output.spring_velocity_corners;
                input.trail_elapsed_ms = output.trail_elapsed_ms;
                input.previous_center = output.previous_center;
            }

            prop_assert!(
                approx_eq_f64(previous_col, target_col, 0.03),
                "overdamped response did not settle: settled={previous_col} target={target_col}"
            );
        }

        #[test]
        fn prop_step_response_underdamped_overshoot_is_bounded_and_settles(
            row in 2_i64..64_i64,
            start_col in 1_i64..24_i64,
            travel_cols in 2_i64..25_i64,
            damping_ratio in 0.2_f64..0.85_f64,
            head_response_ms in 45.0_f64..180.0_f64,
        ) {
            let start = Point {
                row: row as f64,
                col: start_col as f64,
            };
            let target = Point {
                row: row as f64,
                col: (start_col + travel_cols) as f64,
            };
            let mut input = make_pose_input(PoseInputSpec {
                start,
                target,
                vertical_bar: false,
                horizontal_bar: false,
                damping_ratio,
                head_response_ms,
                block_aspect_ratio: 2.0,
                rng_state: DEFAULT_RNG_STATE,
            });

            let target_col = center_col(&input.target_corners);
            let start_col = center_col(&input.current_corners);
            let travel_distance = target_col - start_col;
            let mut peak_col = start_col;
            let mut settled_col = start_col;
            for _ in 0..480 {
                let output = simulate_step(input.clone());
                let next_col = center_col(&output.current_corners);
                peak_col = peak_col.max(next_col);
                settled_col = next_col;
                input.current_corners = output.current_corners;
                input.spring_velocity_corners = output.spring_velocity_corners;
                input.trail_elapsed_ms = output.trail_elapsed_ms;
                input.previous_center = output.previous_center;
            }

            let overshoot = (peak_col - target_col).max(0.0);
            let theoretical_bound = travel_distance * theoretical_overshoot_ratio(damping_ratio);
            prop_assert!(
                overshoot <= theoretical_bound + 0.05,
                "underdamped overshoot exceeded bound: overshoot={overshoot} bound={theoretical_bound} zeta={damping_ratio}"
            );
            prop_assert!(
                approx_eq_f64(settled_col, target_col, 0.02),
                "underdamped response did not settle: settled={settled_col} target={target_col}"
            );
        }
    }

    #[test]
    fn overdamped_horizontal_motion_regression_stays_monotone_for_checked_in_seed() {
        let mut input = make_pose_input(PoseInputSpec {
            start: Point { row: 2.0, col: 1.0 },
            target: Point { row: 2.0, col: 8.0 },
            vertical_bar: false,
            horizontal_bar: false,
            damping_ratio: 2.49469574985472,
            head_response_ms: 173.19606000702925,
            block_aspect_ratio: 2.0,
            rng_state: DEFAULT_RNG_STATE,
        });

        let target_col = center_col(&input.target_corners);
        let mut previous_col = center_col(&input.current_corners);
        for _ in 0..480 {
            let output = simulate_step(input.clone());
            let next_col = center_col(&output.current_corners);
            assert!(
                next_col + 1.0e-9 >= previous_col,
                "checked-in overdamped regression moved backward: prev={previous_col} next={next_col}"
            );
            assert!(
                next_col <= target_col + 1.0e-9,
                "checked-in overdamped regression overshot: next={next_col} target={target_col}"
            );
            previous_col = next_col;
            input.current_corners = output.current_corners;
            input.spring_velocity_corners = output.spring_velocity_corners;
            input.trail_elapsed_ms = output.trail_elapsed_ms;
            input.previous_center = output.previous_center;
        }

        assert!(
            approx_eq_f64(previous_col, target_col, 0.03),
            "checked-in overdamped regression did not settle: settled={previous_col} target={target_col}"
        );
    }
}

mod stop_threshold_logic {
    use super::*;

    proptest! {
        #![proptest_config(pure_config())]

        #[test]
        fn prop_stop_enter_and_exit_boundaries_are_inclusive(
            stop_distance in 0.01_f64..8.0_f64,
            stop_velocity in 0.01_f64..8.0_f64,
        ) {
            let config = RuntimeConfig {
                stop_distance_enter: stop_distance,
                stop_distance_exit: stop_distance,
                stop_velocity_enter: stop_velocity,
                ..RuntimeConfig::default()
            };

            let boundary_metrics = StopMetrics {
                max_distance: stop_distance,
                max_velocity: stop_velocity,
                particles_empty: true,
            };
            prop_assert!(within_stop_enter(&config, boundary_metrics));

            let distance_boundary = StopMetrics {
                max_distance: stop_distance,
                max_velocity: stop_velocity / 2.0,
                particles_empty: true,
            };
            prop_assert!(outside_stop_exit(&config, distance_boundary));

            let velocity_boundary = StopMetrics {
                max_distance: stop_distance / 2.0,
                max_velocity: stop_velocity,
                particles_empty: true,
            };
            prop_assert!(outside_stop_exit(&config, velocity_boundary));
        }

        #[test]
        fn prop_stop_metrics_match_vertical_display_space_motion(
            row in 1_i64..128_i64,
            col in 1_i64..128_i64,
            display_distance in 0.0_f64..4.0_f64,
            display_velocity in 0.0_f64..4.0_f64,
            block_aspect_ratio in positive_aspect_ratio(),
            (vertical_bar, horizontal_bar) in cursor_shape_flags(),
        ) {
            let current_corners =
                corners_for_cursor(row as f64, col as f64, vertical_bar, horizontal_bar);
            let target_corners = corners_for_cursor(
                row as f64 + display_distance / block_aspect_ratio,
                col as f64,
                vertical_bar,
                horizontal_bar,
            );
            let velocity_corners = [Point {
                row: display_velocity / block_aspect_ratio,
                col: 0.0,
            }; 4];

            let metrics = stop_metrics(
                &current_corners,
                &target_corners,
                &velocity_corners,
                block_aspect_ratio,
                &[],
            );

            prop_assert!(approx_eq_f64(metrics.max_distance, display_distance, 1.0e-9));
            prop_assert!(approx_eq_f64(metrics.max_velocity, display_velocity, 1.0e-9));
        }
    }
}

mod trail_distance_thresholds {
    use super::*;

    proptest! {
        #![proptest_config(pure_config())]

        #[test]
        fn prop_trail_min_distance_snaps_at_inclusive_boundary(
            row in 1_i64..128_i64,
            col in 1_i64..128_i64,
            display_distance in 0.0_f64..4.0_f64,
            block_aspect_ratio in positive_aspect_ratio(),
            (vertical_bar, horizontal_bar) in cursor_shape_flags(),
        ) {
            let mut input = make_input();
            input.block_aspect_ratio = block_aspect_ratio;
            input.vertical_bar = vertical_bar;
            input.horizontal_bar = horizontal_bar;
            input.current_corners =
                corners_for_cursor(row as f64, col as f64, vertical_bar, horizontal_bar);
            input.trail_origin_corners = input.current_corners;
            input.target_corners = corners_for_cursor(
                row as f64 + display_distance / block_aspect_ratio,
                col as f64,
                vertical_bar,
                horizontal_bar,
            );
            input.previous_center = center(&input.current_corners);
            let travel_distance = center(&input.trail_origin_corners)
                .display_distance(center(&input.target_corners), block_aspect_ratio);
            input.trail_min_distance =
                travel_distance / cursor_height_factor(vertical_bar, horizontal_bar);
            let target_corners = input.target_corners;

            let output = simulate_step(input);
            prop_assert_eq!(output.current_corners, target_corners);
        }

        #[test]
        fn prop_trail_min_distance_animation_uses_display_metric_across_aspect_ratios(
            row in 1_i64..128_i64,
            col in 1_i64..128_i64,
            display_distance in 0.25_f64..4.0_f64,
            threshold_margin in 0.01_f64..0.2_f64,
            aspect_one in positive_aspect_ratio(),
            aspect_two in positive_aspect_ratio(),
            (vertical_bar, horizontal_bar) in cursor_shape_flags(),
        ) {
            let movement = display_distance.max(threshold_margin + 0.05);
            let threshold = movement - threshold_margin;
            let threshold_input =
                threshold / cursor_height_factor(vertical_bar, horizontal_bar);

            let mut input_one = make_input();
            input_one.block_aspect_ratio = aspect_one;
            input_one.vertical_bar = vertical_bar;
            input_one.horizontal_bar = horizontal_bar;
            input_one.current_corners =
                corners_for_cursor(row as f64, col as f64, vertical_bar, horizontal_bar);
            input_one.trail_origin_corners = input_one.current_corners;
            input_one.target_corners = corners_for_cursor(
                row as f64 + movement / aspect_one,
                col as f64,
                vertical_bar,
                horizontal_bar,
            );
            input_one.previous_center = center(&input_one.current_corners);
            input_one.trail_min_distance = threshold_input;

            let mut input_two = input_one.clone();
            input_two.block_aspect_ratio = aspect_two;
            input_two.target_corners = corners_for_cursor(
                row as f64 + movement / aspect_two,
                col as f64,
                vertical_bar,
                horizontal_bar,
            );

            let start_one = center(&input_one.current_corners);
            let start_two = center(&input_two.current_corners);
            let target_one = input_one.target_corners;
            let target_two = input_two.target_corners;

            let output_one = simulate_step(input_one);
            let output_two = simulate_step(input_two);

            prop_assert_ne!(output_one.current_corners, target_one);
            prop_assert_ne!(output_two.current_corners, target_two);

            let display_advance_one = start_one.display_distance(
                center(&output_one.current_corners),
                aspect_one,
            );
            let display_advance_two = start_two.display_distance(
                center(&output_two.current_corners),
                aspect_two,
            );
            prop_assert!(
                approx_eq_f64(display_advance_one, display_advance_two, 1.0e-9),
                "display-space advancement diverged across aspect ratios: one={display_advance_one} two={display_advance_two}"
            );
        }
    }

    #[test]
    fn trail_min_distance_regression_snaps_at_checked_in_seed_boundary() {
        let mut input = make_input();
        input.block_aspect_ratio = 6.938583027910313;
        input.vertical_bar = false;
        input.horizontal_bar = false;
        input.current_corners = corners_for_cursor(1.0, 1.0, false, false);
        input.trail_origin_corners = input.current_corners;
        input.target_corners = corners_for_cursor(
            1.0 + 2.968038521796358 / input.block_aspect_ratio,
            1.0,
            false,
            false,
        );
        input.previous_center = center(&input.current_corners);
        let travel_distance = center(&input.trail_origin_corners)
            .display_distance(center(&input.target_corners), input.block_aspect_ratio);
        input.trail_min_distance = travel_distance;
        let target_corners = input.target_corners;

        let output = simulate_step(input);

        assert_eq!(output.current_corners, target_corners);
    }
}

mod corner_scaling {
    use super::*;

    proptest! {
        #![proptest_config(pure_config())]

        #[test]
        fn prop_scaled_corners_preserve_center_scale_extents_and_stay_finite(
            fixture in cursor_rectangle(),
            trail_thickness in positive_scale(),
            trail_thickness_x in positive_scale(),
        ) {
            let scaled =
                scaled_corners_for_trail(&fixture.corners, trail_thickness, trail_thickness_x);

            prop_assert!(
                scaled
                    .iter()
                    .all(|point| point.row.is_finite() && point.col.is_finite())
            );
            prop_assert!(approx_eq_point(center(&scaled), center(&fixture.corners), 1.0e-9));
            prop_assert!(approx_eq_f64(
                corners_height(&scaled),
                corners_height(&fixture.corners) * trail_thickness,
                1.0e-9,
            ));
            prop_assert!(approx_eq_f64(
                corners_width(&scaled),
                corners_width(&fixture.corners) * trail_thickness_x,
                1.0e-9,
            ));
        }
    }
}
