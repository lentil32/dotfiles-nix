use super::*;
use crate::test_support::proptest::pure_config;
use crate::types::Particle;
use proptest::collection::vec;
use proptest::prelude::*;
use std::sync::Arc;

fn compact_origins(max_len: usize) -> BoxedStrategy<Vec<(i16, i16)>> {
    vec((8_i16..=20_i16, 8_i16..=20_i16), 1..=max_len).boxed()
}

fn mutate_static_config(frame: &mut RenderFrame, mutator: impl FnOnce(&mut StaticRenderConfig)) {
    let mut config = (*frame.static_config).clone();
    mutator(&mut config);
    frame.static_config = Arc::new(config);
}

fn mutate_signature_axis(frame: &mut RenderFrame, axis: usize, row: i16, col: i16) {
    match axis {
        0 => frame.mode.push('i'),
        1 => frame.vertical_bar = !frame.vertical_bar,
        2 => frame.retarget_epoch = frame.retarget_epoch.saturating_add(1),
        3 => frame.planner_idle_steps = frame.planner_idle_steps.saturating_add(1),
        4 => frame.target.col += 0.5,
        5 => {
            let corners = unit_square_corners_at(row.saturating_add(1), col.saturating_add(2));
            frame.corners = corners;
            frame.target_corners = corners;
        }
        6 => {
            frame.step_samples = vec![
                sample_for_corners(unit_square_corners_at(row, col)),
                sample_for_corners(unit_square_corners_at(row, col.saturating_add(1))),
            ]
            .into();
        }
        7 => mutate_static_config(frame, |config| {
            config.tail_duration_ms += 1.0;
        }),
        8 => mutate_static_config(frame, |config| {
            config.top_k_per_cell = config.top_k_per_cell.saturating_add(1);
        }),
        9 => mutate_static_config(frame, |config| {
            config.block_aspect_ratio += 0.25;
        }),
        _ => panic!("unexpected signature mutation axis {axis}"),
    }
}

proptest! {
    #![proptest_config(pure_config())]

    #[test]
    fn prop_render_plan_is_deterministic_for_identical_generated_frame_sequences(
        origins in compact_origins(5),
    ) {
        let frames = frames_from_origins(&origins);
        let viewport = test_viewport();

        let run = |initial: PlannerState| {
            let mut state = initial;
            let mut outputs = Vec::new();
            for frame in &frames {
                let output = render_frame_to_plan(frame, state, viewport);
                state = output.next_state.clone();
                outputs.push((output.plan, output.signature, output.next_state));
            }
            outputs
        };

        prop_assert_eq!(run(PlannerState::default()), run(PlannerState::default()));
    }

    #[test]
    fn prop_frame_draw_signature_changes_when_any_representative_hashed_axis_changes(
        row in 8_i16..=20_i16,
        col in 8_i16..=20_i16,
        axis in 0_usize..10_usize,
    ) {
        let first = single_sample_frame(row, col);
        let mut second = first.clone();
        mutate_signature_axis(&mut second, axis, row, col);

        prop_assert_ne!(frame_draw_signature(&first), frame_draw_signature(&second));
    }

    #[test]
    fn prop_frame_draw_signature_is_none_when_particles_are_present(
        row in 8_i16..=20_i16,
        col in 8_i16..=20_i16,
        particle_count in 1_usize..=4_usize,
        lifetime in 0.1_f64..5.0_f64,
    ) {
        let mut frame = single_sample_frame(row, col);
        frame.particles = (0..particle_count)
            .map(|index| Particle {
                position: Point {
                    row: f64::from(row) + index as f64 * 0.25,
                    col: f64::from(col) + index as f64 * 0.25,
                },
                velocity: Point::ZERO,
                lifetime,
            })
            .collect::<Vec<_>>()
            .into();

        prop_assert_eq!(frame_draw_signature(&frame), None);
    }
}
