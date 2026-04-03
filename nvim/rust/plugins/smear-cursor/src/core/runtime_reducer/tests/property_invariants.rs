use super::*;
use crate::test_support::proptest::stateful_config;

proptest! {
    #![proptest_config(stateful_config())]

    #[test]
    fn prop_window_change_clears_when_smear_between_windows_disabled(
        start_row in 1_u16..120,
        start_col in 1_u16..220,
        switched_row in 120_u16..280,
        switched_col in 120_u16..280,
        initial_window in any::<i64>(),
        initial_buffer in any::<i64>(),
        window_delta in 1_i16..32,
        initial_seed in any::<u32>(),
        switched_seed in any::<u32>(),
    ) {
        let mut state = RuntimeState::default();
        state.config.smear_between_windows = false;
        state.config.smear_between_buffers = true;

        let initial = event_with_location(
            f64::from(start_row),
            f64::from(start_col),
            100.0,
            initial_seed,
            initial_window,
            initial_buffer,
        );
        let _ = reduce_cursor_event(&mut state, "n", initial, EventSource::External);

        let switched = event_with_location(
            f64::from(switched_row),
            f64::from(switched_col),
            116.0,
            switched_seed,
            initial_window.wrapping_add(i64::from(window_delta)),
            initial_buffer,
        );
        let effects = reduce_cursor_event(&mut state, "n", switched, EventSource::External);
        prop_assert!(matches!(render_action(&effects), RenderAction::ClearAll));
        prop_assert_eq!(
            render_cleanup_action(&effects),
            RenderCleanupAction::Schedule
        );
    }

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
        let tracked_before = state.tracked_location();
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
            prop_assert_eq!(state.tracked_location(), tracked_before.clone());
            prop_assert_eq!(state.last_tick_ms(), last_tick_before);
            now_ms += 16.0;
        }
    }

    #[test]
    fn prop_clear_all_and_external_noop_always_emit_cleanup_intent(
        steps in vec((any::<bool>(), 0_u16..320, 0_u16..320, any::<u32>(), any::<i64>(), any::<i64>()), 1..160),
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
