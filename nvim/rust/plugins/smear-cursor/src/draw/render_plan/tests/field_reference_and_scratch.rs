use super::*;
use crate::test_support::proptest::pure_config;
use proptest::collection::vec;
use proptest::prelude::*;

fn compact_origins(max_len: usize) -> BoxedStrategy<Vec<(i16, i16)>> {
    vec((8_i16..=18_i16, 8_i16..=18_i16), 1..=max_len).boxed()
}

proptest! {
    #![proptest_config(pure_config())]

    #[test]
    fn prop_render_plan_matches_reference_compile_on_generated_frame_sequences(
        origins in compact_origins(5),
    ) {
        let frames = frames_from_origins(&origins);
        let viewport = test_viewport();
        let mut optimized_state = PlannerState::default();
        let mut reference_state = PlannerState::default();

        for frame in &frames {
            let optimized = render_frame_to_plan(frame, optimized_state, viewport);
            let reference = render_frame_to_plan_reference(frame, reference_state, viewport);

            prop_assert_eq!(optimized.plan, reference.plan);
            prop_assert_eq!(optimized.signature, reference.signature);
            prop_assert_eq!(&optimized.next_state, &reference.next_state);

            optimized_state = optimized.next_state;
            reference_state = reference.next_state;
        }
    }

    #[test]
    fn prop_planner_idle_steps_age_history_without_new_motion_samples(
        row in 8_i16..=18_i16,
        col in 8_i16..=18_i16,
        extra_idle_steps in 0_u16..=4_u16,
    ) {
        let viewport = test_viewport();
        let first = render_frame_to_plan(&single_sample_frame(row, col), PlannerState::default(), viewport);
        let mut draining = quiescent_frame(row, col);
        draining.planner_idle_steps = u32::try_from(latent_field::max_comet_support_steps(
            draining.tail_duration_ms,
            draining.simulation_hz,
        ))
        .unwrap_or(u32::MAX)
        .saturating_add(u32::from(extra_idle_steps));

        let drained = render_frame_to_plan(&draining, first.next_state, viewport);

        prop_assert!(drained.plan.cell_ops.is_empty());
        prop_assert!(drained.next_state.previous_cells.is_empty());
    }
}
