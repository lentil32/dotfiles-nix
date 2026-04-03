use super::*;
use crate::test_support::proptest::stateful_config;
use proptest::collection::vec;

proptest! {
    #![proptest_config(stateful_config())]

    #[test]
    fn prop_same_surface_external_retargets_draw_immediately_and_only_bump_epoch_for_new_targets(
        target_bands in vec(1_u8..=8, 1..=16),
    ) {
        let (mut state, kickoff) = animating_runtime_after_kickoff(|state| {
            state.config.delay_event_to_smear = 0.0;
        });
        let kickoff_frame = draw_frame(&kickoff)
            .expect("kickoff should draw");
        let kickoff_stroke_id = kickoff_frame.trail_stroke_id;
        let mut expected_epoch = kickoff_frame.retarget_epoch;
        let mut current_target = state.target_position();
        let mut now_ms = 120.0_f64;

        for band in target_bands {
            let target_position = Point {
                row: 5.0,
                col: 12.0 * f64::from(band),
            };
            let before_epoch = expected_epoch;
            let retarget = reduce_cursor_event(
                &mut state,
                "n",
                event_at(target_position.row, target_position.col, now_ms),
                EventSource::External,
            );
            let frame = draw_frame(&retarget)
                .expect("same-surface external retarget should draw immediately");

            if target_position != current_target {
                expected_epoch = expected_epoch.wrapping_add(1);
            }

            prop_assert!(matches!(render_action(&retarget), RenderAction::Draw(_)));
            prop_assert!(retarget.should_schedule_next_animation);
            prop_assert_eq!(
                render_cleanup_action(&retarget),
                RenderCleanupAction::Invalidate
            );
            prop_assert!(state.is_animating());
            prop_assert_eq!(frame.trail_stroke_id, kickoff_stroke_id);
            prop_assert_eq!(state.trail_stroke_id(), kickoff_stroke_id);
            prop_assert_eq!(frame.retarget_epoch, expected_epoch);
            prop_assert_eq!(state.retarget_epoch(), expected_epoch);
            prop_assert!(frame.retarget_epoch >= before_epoch);
            prop_assert_eq!(state.target_position(), target_position);

            current_target = target_position;
            now_ms += 4.0;
        }

        let follow_up_tick = reduce_cursor_event(
            &mut state,
            "n",
            event_at(current_target.row, current_target.col, now_ms + 8.0),
            EventSource::AnimationTick,
        );
        let follow_up_frame = draw_frame(&follow_up_tick)
            .expect("animation should continue drawing after same-surface retargets");

        prop_assert!(matches!(
            render_action(&follow_up_tick),
            RenderAction::Draw(_)
        ));
        prop_assert!(follow_up_tick.should_schedule_next_animation);
        prop_assert_eq!(
            render_cleanup_action(&follow_up_tick),
            RenderCleanupAction::Invalidate
        );
        prop_assert!(state.is_animating());
        prop_assert_eq!(follow_up_frame.trail_stroke_id, kickoff_stroke_id);
        prop_assert_eq!(follow_up_frame.retarget_epoch, expected_epoch);
        prop_assert_eq!(state.target_position(), current_target);
    }
}
