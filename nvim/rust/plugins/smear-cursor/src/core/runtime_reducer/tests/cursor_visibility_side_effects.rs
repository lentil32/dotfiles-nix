use super::*;
use crate::test_support::proptest::pure_config;

proptest! {
    #![proptest_config(pure_config())]

    #[test]
    fn prop_draw_effects_show_cursor_without_cmdline_redraw(
        cmdline_mode in any::<bool>(),
    ) {
        let mode = if cmdline_mode { "c" } else { "n" };
        let (_, transition) = initialized_runtime(mode, |state| {
            if cmdline_mode {
                state.config.smear_to_cmd = true;
            }
        });
        let side_effects = render_side_effects(&transition);

        prop_assert!(matches!(render_action(&transition), RenderAction::Draw(_)));
        prop_assert_eq!(side_effects.cursor_visibility, CursorVisibilityEffect::Show);
        prop_assert!(!side_effects.redraw_after_draw_if_cmdline);
        prop_assert!(!side_effects.redraw_after_clear_if_cmdline);
    }

    #[test]
    fn prop_clear_effects_show_cursor_and_only_cmdline_requests_clear_redraw(
        hide_target_hack in any::<bool>(),
        cmdline_mode in any::<bool>(),
    ) {
        let mode = if cmdline_mode { "c" } else { "n" };
        let mut state = RuntimeState::default();
        state.config.hide_target_hack = hide_target_hack;
        state.set_enabled(false);

        let transition = reduce_cursor_event(
            &mut state,
            mode,
            event(3.0, 8.0),
            EventSource::External,
        );
        let side_effects = render_side_effects(&transition);

        prop_assert!(matches!(render_action(&transition), RenderAction::ClearAll));
        prop_assert_eq!(side_effects.cursor_visibility, CursorVisibilityEffect::Show);
        prop_assert_eq!(side_effects.allow_real_cursor_updates, !hide_target_hack);
        prop_assert!(!side_effects.redraw_after_draw_if_cmdline);
        prop_assert_eq!(side_effects.redraw_after_clear_if_cmdline, cmdline_mode);
    }
}
