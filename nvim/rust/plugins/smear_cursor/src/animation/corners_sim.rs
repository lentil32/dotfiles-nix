fn update_corners(
    input: &StepInput,
) -> ([Point; 4], [Point; 4], [Point; 4], [f64; 4], usize, usize) {
    let mut current_corners = input.current_corners;
    let mut center_velocity = Point {
        row: (input.spring_velocity_corners[0].row
            + input.spring_velocity_corners[1].row
            + input.spring_velocity_corners[2].row
            + input.spring_velocity_corners[3].row)
            / 4.0,
        col: (input.spring_velocity_corners[0].col
            + input.spring_velocity_corners[1].col
            + input.spring_velocity_corners[2].col
            + input.spring_velocity_corners[3].col)
            / 4.0,
    };
    let mut velocity_corners = [Point::ZERO; 4];
    let mut trail_elapsed_ms = input.trail_elapsed_ms;
    let previous_corners = input.current_corners;
    let target_corners = scaled_corners_for_trail(
        &input.target_corners,
        input.trail_thickness,
        input.trail_thickness_x,
    );

    let mut distance_head_to_target_squared = f64::INFINITY;
    let mut distance_tail_to_target_squared = 0.0;
    let mut index_head = 0_usize;
    let mut index_tail = 0_usize;

    let max_length = if is_insert_like_mode(&input.mode) {
        input.max_length_insert_mode
    } else {
        input.max_length
    };
    let _legacy_timing_knobs = (input.trail_short_duration_ms, input.trail_size);
    let origin_center = center(&input.trail_origin_corners);
    let target_center = center(&input.target_corners);
    let aspect = crate::types::display_metric_row_scale(input.block_aspect_ratio);
    let travel_distance = origin_center.display_distance(target_center, input.block_aspect_ratio);
    let effective_trail_min_distance =
        cursor_height_for_trail_threshold(input.vertical_bar, input.horizontal_bar)
            * input.trail_min_distance;

    // Neovide's default cursor shader has no minimum-distance snap gate; this optional
    // threshold is a project-specific divergence knob when non-zero.
    if travel_distance <= effective_trail_min_distance {
        current_corners = target_corners;
        center_velocity = Point::ZERO;
        trail_elapsed_ms = [input.trail_duration_ms.max(1.0); 4];
    } else {
        let dt = input.time_interval.max(0.0);
        let omega = omega_from_head_response_ms(input.head_response_ms, input.damping_ratio);
        let current_center = center(&input.current_corners);
        let (next_row_display, next_row_velocity_display) = advance_second_order_response(
            current_center.row * aspect,
            center_velocity.row * aspect,
            target_center.row * aspect,
            dt,
            omega,
            input.damping_ratio,
        );
        let (next_col, next_col_velocity) = advance_second_order_response(
            current_center.col,
            center_velocity.col,
            target_center.col,
            dt,
            omega,
            input.damping_ratio,
        );

        let next_center = Point {
            row: next_row_display / aspect,
            col: next_col,
        };
        center_velocity = Point {
            row: next_row_velocity_display / aspect,
            col: next_col_velocity,
        };

        let center_offset = Point {
            row: next_center.row - target_center.row,
            col: next_center.col - target_center.col,
        };
        for index in 0..4 {
            current_corners[index] = Point {
                row: target_corners[index].row + center_offset.row,
                col: target_corners[index].col + center_offset.col,
            };
            trail_elapsed_ms[index] =
                (input.trail_elapsed_ms[index] + dt).clamp(0.0, input.trail_duration_ms.max(1.0));
        }
    }

    let mut spring_velocity_corners = [Point::ZERO; 4];
    spring_velocity_corners.fill(center_velocity);

    for index in 0..4 {
        let distance_squared =
            current_corners[index].display_distance_squared(target_corners[index], aspect);

        if distance_squared < distance_head_to_target_squared {
            distance_head_to_target_squared = distance_squared;
            index_head = index;
        }
        if distance_squared > distance_tail_to_target_squared {
            distance_tail_to_target_squared = distance_squared;
            index_tail = index;
        }
    }

    let mut smear_length = 0.0_f64;
    for index in 0..4 {
        if index == index_head {
            continue;
        }
        let distance = current_corners[index].display_distance(current_corners[index_head], aspect);
        smear_length = smear_length.max(distance);
    }

    if max_length.is_finite() && max_length > EPSILON && smear_length > max_length {
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

    for index in 0..4 {
        velocity_corners[index] = Point {
            row: current_corners[index].row - previous_corners[index].row,
            col: current_corners[index].col - previous_corners[index].col,
        };
    }

    (
        current_corners,
        velocity_corners,
        spring_velocity_corners,
        trail_elapsed_ms,
        index_head,
        index_tail,
    )
}
