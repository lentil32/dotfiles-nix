use super::*;
use crate::test_support::proptest::stateful_config;

proptest! {
    #![proptest_config(stateful_config())]

    #[test]
    fn prop_repeated_window_switches_do_not_clear_when_smear_between_windows_enabled(
        switches in vec((20_u16..260, 20_u16..260, any::<u32>()), 1..96),
    ) {
        let mut state = RuntimeState::default();
        state.config.smear_between_windows = true;

        let initial = event_with_location(5.0, 6.0, 100.0, 7, 10, 20);
        let _ = reduce_cursor_event(&mut state, "n", initial, EventSource::External);

        let mut now_ms = 120.0;
        for (index, (row, col, seed)) in switches.into_iter().enumerate() {
            let window_handle = if index % 2 == 0 { 100_i64 } else { 101_i64 };
            let step_event = event_with_location(
                f64::from(row),
                f64::from(col),
                now_ms,
                seed,
                window_handle,
                20,
            );
            let _ = reduce_cursor_event(&mut state, "n", step_event, EventSource::External);
            let effects = reduce_cursor_event(
                &mut state,
                "n",
                event_with_location(
                    f64::from(row),
                    f64::from(col),
                    now_ms + 2.0,
                    seed,
                    window_handle,
                    20,
                ),
                EventSource::AnimationTick,
            );
            prop_assert!(
                !matches!(render_action(&effects), RenderAction::ClearAll),
                "unexpected clear-all at step {index}"
            );
            prop_assert_ne!(
                render_cleanup_action(&effects),
                RenderCleanupAction::Schedule,
                "unexpected cleanup scheduling at step {}",
                index
            );
            now_ms += 16.0;
        }
    }

    #[test]
    fn prop_idle_animation_ticks_are_idempotent(
        ticks in vec((0_u16..320, 0_u16..320, any::<u32>()), 1..96),
    ) {
        let mut state = RuntimeState::default();
        state.mark_initialized();
        state.stop_animation();
        let tracked_before = state.tracked_cursor();
        let last_tick_before = state.last_tick_ms();

        let mut now_ms = 100.0;
        for (row, col, seed) in ticks {
            let tick = event_with_location(
                f64::from(row),
                f64::from(col),
                now_ms,
                seed,
                10,
                20,
            );
            let effects = reduce_cursor_event(&mut state, "n", tick, EventSource::AnimationTick);
            prop_assert!(matches!(render_action(&effects), RenderAction::Noop));
            prop_assert_eq!(
                render_cleanup_action(&effects),
                RenderCleanupAction::NoAction
            );
            prop_assert!(state.is_initialized());
            prop_assert!(!state.is_animating());
            prop_assert_eq!(state.tracked_cursor(), tracked_before.clone());
            prop_assert_eq!(state.last_tick_ms(), last_tick_before);
            now_ms += 16.0;
        }
    }

    #[test]
    fn prop_clear_all_and_external_noop_always_emit_cleanup_intent(
        steps in vec((any::<bool>(), 0_u16..320, 0_u16..320, any::<u32>(), 1_i64..1_000_000, 1_i64..1_000_000), 1..160),
    ) {
        let mut state = RuntimeState::default();
        let mut now_ms = 100.0;

        for (is_external, row, col, seed, window_handle, buffer_handle) in steps {
            let source = if is_external {
                EventSource::External
            } else {
                EventSource::AnimationTick
            };
            let step_event = event_with_location(
                f64::from(row),
                f64::from(col),
                now_ms,
                seed,
                window_handle,
                buffer_handle,
            );
            let effects = reduce_cursor_event(&mut state, "n", step_event, source);
            if matches!(render_action(&effects), RenderAction::ClearAll) {
                prop_assert_ne!(
                    render_cleanup_action(&effects),
                    RenderCleanupAction::NoAction
                );
            }
            if source == EventSource::External
                && matches!(render_action(&effects), RenderAction::Noop)
            {
                prop_assert_ne!(
                    render_cleanup_action(&effects),
                    RenderCleanupAction::NoAction
                );
            }

            now_ms += 8.0;
        }
    }
}
