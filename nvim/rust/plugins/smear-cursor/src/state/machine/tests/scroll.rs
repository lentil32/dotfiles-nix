use super::*;
use crate::animation::scaled_corners_for_trail;
use crate::test_support::proptest::approx_eq_f64;
use proptest::collection::vec;

fn maybe_scaled_corners(
    corners: [Point; 4],
    scale_y: f64,
    scale_x: f64,
    scaled: bool,
) -> [Point; 4] {
    if scaled {
        scaled_corners_for_trail(&corners, scale_y, scale_x)
    } else {
        corners
    }
}

proptest! {
    #![proptest_config(stateful_config())]

    #[test]
    fn prop_apply_scroll_shift_preserves_translation_and_clamp_invariants(
        head_position in (2_i64..192_i64, 2_i64..192_i64),
        trail_position in (2_i64..192_i64, 2_i64..192_i64),
        shape in cursor_shape_strategy(),
        tracked in cursor_location_strategy(),
        scale_head in any::<bool>(),
        scale_trail in any::<bool>(),
        head_scale_y in 0.5_f64..2.5_f64,
        head_scale_x in 0.5_f64..2.5_f64,
        trail_scale_y in 0.5_f64..2.5_f64,
        trail_scale_x in 0.5_f64..2.5_f64,
        row_shift in -24.0_f64..24.0_f64,
        col_shift in -24.0_f64..24.0_f64,
        min_row in 0.0_f64..40.0_f64,
        viewport_height in 6.0_f64..64.0_f64,
        particles in vec((finite_point(), finite_point(), 0.1_f64..1.0_f64), 0..6),
    ) {
        let mut state = RuntimeState::default();
        let head_position = point(head_position.0 as f64, head_position.1 as f64);
        let trail_position = point(trail_position.0 as f64, trail_position.1 as f64);
        state.initialize_cursor(head_position, shape, 11, &tracked);

        let current_corners = maybe_scaled_corners(
            state.current_corners(),
            head_scale_y,
            head_scale_x,
            scale_head,
        );
        let trail_origin = maybe_scaled_corners(
            shape.corners(trail_position),
            trail_scale_y,
            trail_scale_x,
            scale_trail,
        );
        let expected_particles = particles
            .iter()
            .map(|(position, velocity, lifetime)| Particle {
                position: point(position.row - row_shift, position.col - col_shift),
                velocity: *velocity,
                lifetime: *lifetime,
            })
            .collect::<Vec<_>>();
        let unclamped_head = translate_corners(current_corners, -row_shift, -col_shift);
        let (unclamped_min_row, unclamped_max_row) = row_bounds(&unclamped_head);
        let max_row = min_row + viewport_height;
        let clamp_row = if unclamped_min_row < min_row {
            min_row - unclamped_min_row
        } else if unclamped_max_row > max_row + 1.0 {
            max_row + 1.0 - unclamped_max_row
        } else {
            0.0
        };
        let expected_head = translate_corners(unclamped_head, clamp_row, 0.0);
        let expected_trail = translate_corners(trail_origin, -row_shift, -col_shift);
        let baseline_target = state.target_corners();
        let baseline_target_position = state.target_position();

        state.current_corners = current_corners;
        state.previous_center = center(&current_corners);
        state.trail_origin_corners = trail_origin;
        state.particles = particles
            .into_iter()
            .map(|(position, velocity, lifetime)| Particle {
                position,
                velocity,
                lifetime,
            })
            .collect();

        state.apply_scroll_shift(row_shift, col_shift, min_row, max_row);

        let (actual_min_row, actual_max_row) = row_bounds(&state.current_corners());
        let available_height = max_row + 1.0 - min_row;
        let unclamped_height = unclamped_max_row - unclamped_min_row;
        prop_assert_eq!(state.current_corners(), expected_head);
        prop_assert_eq!(state.trail_origin_corners(), expected_trail);
        prop_assert_eq!(state.previous_center(), center(&expected_head));
        prop_assert_eq!(state.particles(), expected_particles.as_slice());
        prop_assert_eq!(state.target_corners(), baseline_target);
        prop_assert_eq!(state.target_position(), baseline_target_position);
        if unclamped_height <= available_height {
            prop_assert!(actual_min_row + 1.0e-6 >= min_row);
            prop_assert!(actual_max_row <= max_row + 1.0 + 1.0e-6);
        } else if clamp_row > 0.0 {
            prop_assert!(approx_eq_f64(actual_min_row, min_row, 1.0e-6));
        } else if clamp_row < 0.0 {
            prop_assert!(approx_eq_f64(actual_max_row, max_row + 1.0, 1.0e-6));
        }
    }
}
