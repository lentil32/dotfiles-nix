use super::*;
use crate::test_support::proptest::pure_config;
use crate::types::ModeClass;
use crate::types::Particle;
use proptest::collection::vec;
use proptest::prelude::*;

fn compact_origins(max_len: usize) -> BoxedStrategy<Vec<(i16, i16)>> {
    vec((8_i16..=20_i16, 8_i16..=20_i16), 1..=max_len).boxed()
}

fn mutate_signature_axis(frame: &mut RenderFrame, axis: usize, row: i16, col: i16) {
    match axis {
        0 => {
            frame.mode = if frame.mode.is_insert_like() {
                ModeClass::NormalLike
            } else {
                ModeClass::InsertLike
            };
        }
        1 => frame.vertical_bar = !frame.vertical_bar,
        2 => frame.trail_stroke_id = frame.trail_stroke_id.next(),
        3 => frame.retarget_epoch = frame.retarget_epoch.saturating_add(1),
        4 => frame.target.col += 0.5,
        5 => {
            let corners = unit_square_corners_at(row.saturating_add(1), col.saturating_add(2));
            frame.corners = corners;
            frame.target_corners = corners;
        }
        _ => panic!("unexpected signature mutation axis {axis}"),
    }
}

#[test]
fn frame_draw_signature_ignores_policy_only_inputs() {
    let baseline = quiescent_frame(10, 10);
    let baseline_signature = frame_draw_signature(&baseline);

    assert_eq!(
        baseline_signature,
        frame_draw_signature(&with_block_aspect_ratio(&baseline, 1.0))
    );
    assert_eq!(
        baseline_signature,
        frame_draw_signature(&with_trail_thickness(&baseline, 2.0))
    );
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
        axis in 0_usize..6_usize,
    ) {
        let first = single_sample_frame(row, col);
        let mut second = first.clone();
        mutate_signature_axis(&mut second, axis, row, col);

        prop_assert_ne!(frame_draw_signature(&first), frame_draw_signature(&second));
    }

    #[test]
    fn prop_frame_draw_signature_ignores_particle_overlay_inputs(
        row in 8_i16..=20_i16,
        col in 8_i16..=20_i16,
        particle_count in 1_usize..=4_usize,
        lifetime in 0.1_f64..5.0_f64,
    ) {
        let mut frame = single_sample_frame(row, col);
        frame.set_particles(std::sync::Arc::new((0..particle_count)
            .map(|index| Particle {
                position: RenderPoint {
                    row: f64::from(row) + index as f64 * 0.25,
                    col: f64::from(col) + index as f64 * 0.25,
                },
                velocity: RenderPoint::ZERO,
                lifetime,
            })
            .collect::<Vec<_>>()));
        let baseline_signature = frame_draw_signature(&frame);
        let mut moved_particles = frame.clone();
        moved_particles.set_particles(std::sync::Arc::new((0..particle_count)
            .map(|index| Particle {
                position: RenderPoint {
                    row: f64::from(row) + index as f64 * 0.25 + 0.5,
                    col: f64::from(col) + index as f64 * 0.25 + 0.5,
                },
                velocity: RenderPoint::ZERO,
                lifetime,
            })
            .collect::<Vec<_>>()));

        prop_assert!(baseline_signature.is_some());
        prop_assert_eq!(baseline_signature, frame_draw_signature(&frame));
        prop_assert_eq!(baseline_signature, frame_draw_signature(&moved_particles));
    }

    #[test]
    fn prop_frame_particle_overlay_signature_tracks_particle_overlay_inputs(
        row in 8_i16..=20_i16,
        col in 8_i16..=20_i16,
        particle_count in 1_usize..=4_usize,
        lifetime in 0.1_f64..5.0_f64,
    ) {
        let mut frame = single_sample_frame(row, col);
        frame.set_particles(std::sync::Arc::new((0..particle_count)
            .map(|index| Particle {
                position: RenderPoint {
                    row: f64::from(row) + index as f64 * 0.25,
                    col: f64::from(col) + index as f64 * 0.25,
                },
                velocity: RenderPoint::ZERO,
                lifetime,
            })
            .collect::<Vec<_>>()));
        let baseline_signature = frame_particle_overlay_signature(&frame);
        let mut moved_particles = frame.clone();
        moved_particles.set_particles(std::sync::Arc::new((0..particle_count)
            .map(|index| Particle {
                position: RenderPoint {
                    row: f64::from(row) + index as f64 * 0.25 + 0.5,
                    col: f64::from(col) + index as f64 * 0.25 + 0.5,
                },
                velocity: RenderPoint::ZERO,
                lifetime,
            })
            .collect::<Vec<_>>()));

        prop_assert!(baseline_signature.is_some());
        prop_assert_eq!(baseline_signature, frame_particle_overlay_signature(&frame));
        prop_assert_ne!(
            baseline_signature,
            frame_particle_overlay_signature(&moved_particles)
        );
    }
}
