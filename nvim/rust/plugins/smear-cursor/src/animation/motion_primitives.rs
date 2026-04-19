fn speed_correction(time_interval: f64) -> f64 {
    time_interval / BASE_TIME_INTERVAL
}

const DAMPING_RATIO_EPS: f64 = 1.0e-3;
const SECOND_ORDER_SETTLE_EPS: f64 = 1.0e-6;

fn velocity_conservation_factor(damping: f64, time_interval: f64) -> f64 {
    let one_minus_damping = (1.0 - damping).clamp(EPSILON, 1.0);
    (one_minus_damping.ln() * speed_correction(time_interval)).exp()
}

fn sanitize_positive(value: f64, fallback: f64) -> f64 {
    if value.is_finite() && value > 0.0 {
        value
    } else {
        fallback
    }
}

pub(crate) fn omega_from_head_response_ms(head_response_ms: f64, damping_ratio: f64) -> f64 {
    let settle_ms = sanitize_positive(head_response_ms, BASE_TIME_INTERVAL).max(1.0);
    let zeta = sanitize_positive(damping_ratio, 1.0).max(DAMPING_RATIO_EPS);
    // 2% settling-time approximation: Ts ~= 4 / (zeta * omega).
    4.0 / (settle_ms * zeta)
}

pub(crate) fn advance_second_order_response(
    position: f64,
    velocity: f64,
    target: f64,
    dt_ms: f64,
    omega: f64,
    damping_ratio: f64,
) -> (f64, f64) {
    let dt = if dt_ms.is_finite() {
        dt_ms.max(0.0)
    } else {
        0.0
    };
    if dt <= EPSILON {
        return (position, velocity);
    }

    let omega = sanitize_positive(omega, 1.0e-3).max(1.0e-3);
    let zeta = sanitize_positive(damping_ratio, 1.0).max(DAMPING_RATIO_EPS);
    let error0 = position - target;
    let velocity0 = velocity;

    let (error, next_velocity) = if zeta < 1.0 {
        let damping = zeta * omega;
        let wd = omega * (1.0 - zeta * zeta).sqrt().max(EPSILON);
        let exp_term = (-damping * dt).exp();
        let cos_term = (wd * dt).cos();
        let sin_term = (wd * dt).sin();
        let b = (velocity0 + damping * error0) / wd;
        let next_error = exp_term * (error0 * cos_term + b * sin_term);
        let velocity_sin_coeff = (damping * velocity0 + omega * omega * error0) / wd;
        let next_velocity = exp_term * (velocity0 * cos_term - velocity_sin_coeff * sin_term);
        (next_error, next_velocity)
    } else if (zeta - 1.0).abs() <= 1.0e-4 {
        let exp_term = (-omega * dt).exp();
        let c = velocity0 + omega * error0;
        let next_error = (error0 + c * dt) * exp_term;
        let next_velocity = (velocity0 - omega * c * dt) * exp_term;
        (next_error, next_velocity)
    } else {
        let root_component = (zeta * zeta - 1.0).sqrt();
        let r1 = -omega * (zeta - root_component);
        let r2 = -omega * (zeta + root_component);
        let denom = (r1 - r2).abs().max(EPSILON);
        let c1 = (velocity0 - r2 * error0) / denom;
        let c2 = error0 - c1;
        let e1 = (r1 * dt).exp();
        let e2 = (r2 * dt).exp();
        let next_error = c1 * e1 + c2 * e2;
        let next_velocity = c1 * r1 * e1 + c2 * r2 * e2;
        (next_error, next_velocity)
    };

    let next_position = target + error;
    if (next_position - target).abs() <= SECOND_ORDER_SETTLE_EPS
        && next_velocity.abs() <= SECOND_ORDER_SETTLE_EPS
    {
        (target, 0.0)
    } else {
        (next_position, next_velocity)
    }
}

pub(crate) fn center(corners: &[RenderPoint; 4]) -> RenderPoint {
    crate::position::corners_center(corners)
}

pub(crate) fn corners_for_cursor(
    row: f64,
    col: f64,
    vertical_bar: bool,
    horizontal_bar: bool,
) -> [RenderPoint; 4] {
    if vertical_bar {
        return [
            RenderPoint { row, col },
            RenderPoint {
                row,
                col: col + 1.0 / 8.0,
            },
            RenderPoint {
                row: row + 1.0,
                col: col + 1.0 / 8.0,
            },
            RenderPoint {
                row: row + 1.0,
                col,
            },
        ];
    }

    if horizontal_bar {
        return [
            RenderPoint {
                row: row + 7.0 / 8.0,
                col,
            },
            RenderPoint {
                row: row + 7.0 / 8.0,
                col: col + 1.0,
            },
            RenderPoint {
                row: row + 1.0,
                col: col + 1.0,
            },
            RenderPoint {
                row: row + 1.0,
                col,
            },
        ];
    }

    [
        RenderPoint { row, col },
        RenderPoint {
            row,
            col: col + 1.0,
        },
        RenderPoint {
            row: row + 1.0,
            col: col + 1.0,
        },
        RenderPoint {
            row: row + 1.0,
            col,
        },
    ]
}

pub(crate) fn zero_velocity_corners() -> [RenderPoint; 4] {
    [RenderPoint::ZERO; 4]
}

pub(crate) fn initial_velocity(
    current_corners: &[RenderPoint; 4],
    target_corners: &[RenderPoint; 4],
    anticipation: f64,
) -> [RenderPoint; 4] {
    let mut velocity_corners = [RenderPoint::ZERO; 4];
    for index in 0..4 {
        velocity_corners[index].row =
            (current_corners[index].row - target_corners[index].row) * anticipation;
        velocity_corners[index].col =
            (current_corners[index].col - target_corners[index].col) * anticipation;
    }
    velocity_corners
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct StopMetrics {
    pub(crate) max_distance: f64,
    pub(crate) max_velocity: f64,
    pub(crate) particles_empty: bool,
}

pub(crate) fn stop_metrics(
    current_corners: &[RenderPoint; 4],
    target_corners: &[RenderPoint; 4],
    velocity_corners: &[RenderPoint; 4],
    block_aspect_ratio: f64,
    particles: &[Particle],
) -> StopMetrics {
    let mut max_distance = 0.0_f64;
    let mut max_velocity = 0.0_f64;

    for index in 0..4 {
        let distance =
            current_corners[index].display_distance(target_corners[index], block_aspect_ratio);
        max_distance = max_distance.max(distance);

        let velocity =
            velocity_corners[index].display_distance(RenderPoint::ZERO, block_aspect_ratio);
        max_velocity = max_velocity.max(velocity);
    }

    StopMetrics {
        max_distance,
        max_velocity,
        particles_empty: particles.is_empty(),
    }
}

pub(crate) fn within_stop_enter(config: &RuntimeConfig, metrics: StopMetrics) -> bool {
    metrics.max_distance <= config.stop_distance_enter
        && metrics.max_velocity <= config.stop_velocity_enter
        && metrics.particles_empty
}

pub(crate) fn outside_stop_exit(config: &RuntimeConfig, metrics: StopMetrics) -> bool {
    metrics.max_distance >= config.stop_distance_exit
        || metrics.max_velocity >= config.stop_velocity_enter
}

pub(crate) fn corners_for_render(
    _config: &RuntimeConfig,
    current_corners: &[RenderPoint; 4],
    _target_corners: &[RenderPoint; 4],
) -> [RenderPoint; 4] {
    *current_corners
}
