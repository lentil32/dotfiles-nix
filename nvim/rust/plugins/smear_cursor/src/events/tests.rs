use nvim_oxi::{Array, Dictionary, Object};

fn options_dict<'a>(entries: impl IntoIterator<Item = (&'a str, Object)>) -> Dictionary {
    let mut opts = Dictionary::new();
    for (key, value) in entries {
        opts.insert(key, value);
    }
    opts
}

fn cterm_colors_object(colors: &[i64]) -> Object {
    Object::from(Array::from_iter(colors.iter().copied().map(Object::from)))
}

fn cursor_snapshot(mode: &str, row: f64, col: f64) -> crate::state::CursorSnapshot {
    crate::state::CursorSnapshot {
        mode: mode.to_string(),
        row,
        col,
    }
}

mod cleanup_tests {
    use super::super::timers::{render_cleanup_delay_ms, render_hard_cleanup_delay_ms};
    use super::super::{
        EngineState, MIN_RENDER_CLEANUP_DELAY_MS, MIN_RENDER_HARD_PURGE_DELAY_MS,
        RENDER_HARD_PURGE_DELAY_MULTIPLIER, RenderCleanupGeneration, RenderGeneration,
    };
    use crate::config::RuntimeConfig;

    #[test]
    fn cleanup_delay_has_floor() {
        let config = RuntimeConfig {
            time_interval: 0.0,
            delay_event_to_smear: 0.0,
            delay_after_key: 0.0,
            ..RuntimeConfig::default()
        };

        assert_eq!(
            render_cleanup_delay_ms(&config),
            MIN_RENDER_CLEANUP_DELAY_MS
        );
    }

    #[test]
    fn cleanup_delay_tracks_config_when_above_floor() {
        let config = RuntimeConfig {
            time_interval: 160.0,
            delay_event_to_smear: 40.0,
            delay_after_key: 20.0,
            ..RuntimeConfig::default()
        };

        assert_eq!(render_cleanup_delay_ms(&config), 220);
    }

    #[test]
    fn hard_cleanup_delay_scales_with_soft_delay() {
        let config = RuntimeConfig {
            time_interval: 160.0,
            delay_event_to_smear: 40.0,
            delay_after_key: 20.0,
            ..RuntimeConfig::default()
        };

        assert_eq!(
            render_hard_cleanup_delay_ms(&config),
            (220 * RENDER_HARD_PURGE_DELAY_MULTIPLIER).max(MIN_RENDER_HARD_PURGE_DELAY_MS)
        );
    }

    #[test]
    fn hard_cleanup_delay_has_independent_floor() {
        let config = RuntimeConfig {
            time_interval: 0.0,
            delay_event_to_smear: 0.0,
            delay_after_key: 0.0,
            ..RuntimeConfig::default()
        };

        assert_eq!(
            render_hard_cleanup_delay_ms(&config),
            MIN_RENDER_HARD_PURGE_DELAY_MS
        );
    }

    #[test]
    fn cleanup_generation_bumps_and_wraps() {
        let mut generation = RenderCleanupGeneration::default();
        assert_eq!(generation.current(), 0);
        assert_eq!(generation.bump(), 1);
        assert_eq!(generation.current(), 1);

        generation = RenderCleanupGeneration { value: u64::MAX };
        assert_eq!(generation.bump(), 0);
        assert_eq!(generation.current(), 0);
    }

    #[test]
    fn engine_state_exposes_cleanup_generation_transitions() {
        let mut state = EngineState::default();
        assert_eq!(state.current_render_cleanup_generation(), 0);
        assert_eq!(state.bump_render_cleanup_generation(), 1);
        assert_eq!(state.bump_render_cleanup_generation(), 2);
        assert_eq!(state.current_render_cleanup_generation(), 2);
    }

    #[test]
    fn render_generation_bumps_and_wraps() {
        let mut generation = RenderGeneration::default();
        assert_eq!(generation.current(), 0);
        assert_eq!(generation.bump(), 1);
        assert_eq!(generation.current(), 1);

        generation = RenderGeneration { value: u64::MAX };
        assert_eq!(generation.bump(), 0);
        assert_eq!(generation.current(), 0);
    }

    #[test]
    fn engine_state_exposes_render_generation_transitions() {
        let mut state = EngineState::default();
        assert_eq!(state.current_render_generation(), 0);
        assert_eq!(state.bump_render_generation(), 1);
        assert_eq!(state.bump_render_generation(), 2);
        assert_eq!(state.current_render_generation(), 2);
    }
}

mod event_loop_tests {
    use super::super::event_loop::EventLoopState;

    #[test]
    fn event_loop_state_pending_flag_is_idempotent_until_cleared() {
        let mut state = EventLoopState::new();
        assert!(state.mark_external_trigger_pending_if_idle());
        assert!(!state.mark_external_trigger_pending_if_idle());
        state.clear_external_trigger_pending();
        assert!(state.mark_external_trigger_pending_if_idle());
    }

    #[test]
    fn event_loop_state_coalesces_reentrant_external_triggers() {
        let mut state = EventLoopState::new();
        assert!(state.mark_external_trigger_pending_if_idle());
        assert!(!state.mark_external_trigger_pending_if_idle());

        // A re-entrant trigger while dispatch is pending should request one extra drain pass.
        assert!(state.complete_external_trigger_dispatch());
        // After the extra pass, pending should clear.
        assert!(!state.complete_external_trigger_dispatch());
        assert!(state.mark_external_trigger_pending_if_idle());
    }

    #[test]
    fn event_loop_state_coalesces_cmdline_redraw_requests() {
        let mut state = EventLoopState::new();
        assert!(state.mark_cmdline_redraw_pending_if_idle());
        assert!(!state.mark_cmdline_redraw_pending_if_idle());
        state.clear_cmdline_redraw_pending();
        assert!(state.mark_cmdline_redraw_pending_if_idle());
    }

    #[test]
    fn event_loop_state_elapsed_autocmd_time_handles_unset_and_monotonicity() {
        let mut state = EventLoopState::new();
        assert!(
            state
                .elapsed_ms_since_last_autocmd_event(10.0)
                .is_infinite()
        );

        state.note_autocmd_event(20.0);
        assert_eq!(state.elapsed_ms_since_last_autocmd_event(25.0), 5.0);
        assert_eq!(state.elapsed_ms_since_last_autocmd_event(19.0), 0.0);

        state.clear_autocmd_event_timestamp();
        assert!(
            state
                .elapsed_ms_since_last_autocmd_event(30.0)
                .is_infinite()
        );
    }
}

mod runtime_options_apply_tests {
    use super::super::options::apply_runtime_options;
    use super::{cterm_colors_object, options_dict};
    use crate::state::{
        ColorOptionsPatch, OptionalChange, RuntimeOptionsPatch, RuntimeState, RuntimeSwitchesPatch,
    };
    use nvim_oxi::Object;

    #[test]
    fn runtime_options_patch_apply_clears_nullable_fields() {
        let mut state = RuntimeState::default();
        state.config.delay_disable = Some(12.0);
        state.config.cursor_color = Some("#abcdef".to_string());
        let patch = RuntimeOptionsPatch {
            runtime: RuntimeSwitchesPatch {
                delay_disable: Some(OptionalChange::Clear),
                ..RuntimeSwitchesPatch::default()
            },
            color: ColorOptionsPatch {
                cursor_color: Some(OptionalChange::Clear),
                ..ColorOptionsPatch::default()
            },
            ..RuntimeOptionsPatch::default()
        };

        patch.apply(&mut state);
        assert_eq!(state.config.delay_disable, None);
        assert_eq!(state.config.cursor_color, None);
    }

    #[test]
    fn runtime_options_patch_explicit_color_levels_override_cterm_array_length() {
        let mut state = RuntimeState::default();
        let opts = options_dict([
            (
                "cterm_cursor_colors",
                cterm_colors_object(&[17_i64, 42_i64]),
            ),
            ("color_levels", Object::from(9_i64)),
        ]);

        let result = apply_runtime_options(&mut state, &opts);
        assert!(
            result.is_ok(),
            "unexpected runtime option error: {result:?}"
        );
        assert_eq!(state.config.cterm_cursor_colors, Some(vec![17_u16, 42_u16]));
        assert_eq!(state.config.color_levels, 9_u32);
    }

    #[test]
    fn runtime_options_patch_apply_clears_filetypes_list() {
        let mut state = RuntimeState::default();
        state.config.filetypes_disabled = vec!["lua".to_string(), "nix".to_string()];
        let patch = RuntimeOptionsPatch {
            runtime: RuntimeSwitchesPatch {
                filetypes_disabled: Some(Vec::new()),
                ..RuntimeSwitchesPatch::default()
            },
            ..RuntimeOptionsPatch::default()
        };

        patch.apply(&mut state);
        assert!(state.config.filetypes_disabled.is_empty());
    }

    #[test]
    fn runtime_options_patch_apply_sets_max_kept_windows() {
        let mut state = RuntimeState::default();
        let patch = RuntimeOptionsPatch {
            runtime: RuntimeSwitchesPatch {
                max_kept_windows: Some(24),
                ..RuntimeSwitchesPatch::default()
            },
            ..RuntimeOptionsPatch::default()
        };

        patch.apply(&mut state);
        assert_eq!(state.config.max_kept_windows, 24);
    }
}

mod runtime_options_parse_tests {
    use super::super::options::{
        parse_optional_change_with, parse_optional_filetypes_disabled, validated_non_negative_f64,
    };
    use super::{cterm_colors_object, options_dict};
    use crate::state::{OptionalChange, RuntimeOptionsPatch};
    use nvim_oxi::Object;

    #[test]
    fn parse_optional_change_with_nil_maps_to_clear() {
        let parsed = parse_optional_change_with(
            Some(Object::nil()),
            "delay_disable",
            validated_non_negative_f64,
        )
        .expect("expected parse success");
        assert_eq!(parsed, Some(OptionalChange::Clear));
    }

    #[test]
    fn parse_optional_filetypes_disabled_nil_maps_to_empty() {
        let parsed = parse_optional_filetypes_disabled(Some(Object::nil()), "filetypes_disabled")
            .expect("expected parse success");
        assert_eq!(parsed, Some(Vec::new()));
    }

    #[test]
    fn runtime_options_patch_parse_rejects_negative_windows_zindex() {
        let opts = options_dict([("windows_zindex", Object::from(-1_i64))]);

        let err = RuntimeOptionsPatch::parse(&opts).expect_err("expected parse failure");
        assert!(
            err.to_string().contains("windows_zindex"),
            "unexpected error: {err}"
        );
        assert!(
            err.to_string().contains("non-negative integer"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn runtime_options_patch_parse_rejects_negative_max_kept_windows() {
        let opts = options_dict([("max_kept_windows", Object::from(-1_i64))]);

        let err = RuntimeOptionsPatch::parse(&opts).expect_err("expected parse failure");
        assert!(
            err.to_string().contains("max_kept_windows"),
            "unexpected error: {err}"
        );
        assert!(
            err.to_string().contains("non-negative integer"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn runtime_options_patch_parse_accepts_integral_float_particle_max_num() {
        let opts = options_dict([("particle_max_num", Object::from(12.0_f64))]);

        let patch = RuntimeOptionsPatch::parse(&opts).expect("expected parse success");
        assert_eq!(patch.particles.particle_max_num, Some(12_usize));
    }

    #[test]
    fn runtime_options_patch_parse_cterm_cursor_colors_sets_color_levels() {
        let opts = options_dict([(
            "cterm_cursor_colors",
            cterm_colors_object(&[17_i64, 42_i64]),
        )]);

        let patch = RuntimeOptionsPatch::parse(&opts).expect("expected parse success");
        let Some(OptionalChange::Set(colors)) = patch.color.cterm_cursor_colors else {
            panic!("expected cterm cursor color patch to be set");
        };
        assert_eq!(colors.colors, vec![17_u16, 42_u16]);
        assert_eq!(colors.color_levels, 2_u32);
    }
}

mod buffer_policy_tests {
    use super::super::policy::BufferEventPolicy;

    #[test]
    fn buffer_event_policy_is_always_normal() {
        assert_eq!(
            BufferEventPolicy::from_buffer_metadata("", true, 1, 0.0),
            BufferEventPolicy::Normal
        );
        assert_eq!(
            BufferEventPolicy::from_buffer_metadata("", true, 1_999, 0.0),
            BufferEventPolicy::Normal
        );
        assert_eq!(
            BufferEventPolicy::from_buffer_metadata("acwrite", true, 1_999, 0.0),
            BufferEventPolicy::Normal
        );
        assert_eq!(
            BufferEventPolicy::from_buffer_metadata("nofile", true, 1, 0.0),
            BufferEventPolicy::Normal
        );
        assert_eq!(
            BufferEventPolicy::from_buffer_metadata("prompt", true, 1, 0.0),
            BufferEventPolicy::Normal
        );
        assert_eq!(
            BufferEventPolicy::from_buffer_metadata("terminal", true, 1, 0.0),
            BufferEventPolicy::Normal
        );
        assert_eq!(
            BufferEventPolicy::from_buffer_metadata("", false, 1, 0.0),
            BufferEventPolicy::Normal
        );
        assert_eq!(
            BufferEventPolicy::from_buffer_metadata("", true, 2_000, 0.0),
            BufferEventPolicy::Normal
        );
        assert_eq!(
            BufferEventPolicy::from_buffer_metadata("", true, 10_000, 0.0),
            BufferEventPolicy::Normal
        );
    }

    #[test]
    fn buffer_event_policy_ignores_adaptive_delay_inputs() {
        assert_eq!(
            BufferEventPolicy::from_buffer_metadata("terminal", true, 1, 70.0),
            BufferEventPolicy::Normal
        );
    }

    #[test]
    fn policy_enables_key_fallback_and_uses_zero_delay_floors() {
        let policy = BufferEventPolicy::from_buffer_metadata("terminal", true, 1, 0.0);
        assert!(policy.use_key_fallback());
        assert_eq!(policy.settle_delay_floor_ms(), 0);
        assert_eq!(policy.animation_delay_floor_ms(), 0);
        assert!(policy.should_use_debounced_external_settle());
        assert!(policy.should_prepaint_cursor());
    }

    #[test]
    fn normal_policy_uses_debounced_external_settle() {
        let policy = BufferEventPolicy::Normal;
        assert!(policy.use_key_fallback());
        assert!(policy.should_use_debounced_external_settle());
        assert_eq!(policy.settle_delay_floor_ms(), 0);
    }
}

mod throttle_tests {
    use super::super::event_loop::ExternalEventTimerKind;
    use super::super::policy::{
        remaining_throttle_delay_ms, should_replace_external_timer_with_throttle,
    };

    #[test]
    fn remaining_throttle_delay_clamps_at_zero_after_interval() {
        assert_eq!(remaining_throttle_delay_ms(12, 4.0), 8);
        assert_eq!(remaining_throttle_delay_ms(12, 12.0), 0);
        assert_eq!(remaining_throttle_delay_ms(12, f64::INFINITY), 0);
        assert_eq!(remaining_throttle_delay_ms(24, 10.0), 14);
    }

    #[test]
    fn throttle_timer_replaces_settle_timer_kind_only() {
        assert!(should_replace_external_timer_with_throttle(Some(
            ExternalEventTimerKind::Settle
        )));
        assert!(!should_replace_external_timer_with_throttle(Some(
            ExternalEventTimerKind::Throttle
        )));
        assert!(!should_replace_external_timer_with_throttle(None));
    }
}

mod external_settle_tests {
    use super::super::timers::{ExternalSettleAction, decide_external_settle_action};
    use super::cursor_snapshot;

    #[test]
    fn external_settle_clears_pending_in_cmd_mode_when_disabled() {
        let expected = cursor_snapshot("c", 3.0, 7.0);
        let current = cursor_snapshot("c", 3.0, 7.0);
        let action = decide_external_settle_action("c", false, Some(&expected), Some(&current));
        assert_eq!(action, ExternalSettleAction::ClearPending);
    }

    #[test]
    fn external_settle_noops_without_pending_snapshot() {
        let action = decide_external_settle_action("n", true, None, None);
        assert_eq!(action, ExternalSettleAction::None);
    }

    #[test]
    fn external_settle_dispatches_when_snapshots_match() {
        let expected = cursor_snapshot("n", 10.0, 2.0);
        let current = cursor_snapshot("n", 10.0, 2.0);
        let action = decide_external_settle_action("n", true, Some(&expected), Some(&current));
        assert_eq!(action, ExternalSettleAction::DispatchExternal);
    }

    #[test]
    fn external_settle_reschedules_when_snapshots_diverge() {
        let expected = cursor_snapshot("n", 10.0, 2.0);
        let current = cursor_snapshot("n", 11.0, 2.0);
        let action = decide_external_settle_action("n", true, Some(&expected), Some(&current));
        assert_eq!(action, ExternalSettleAction::Reschedule(current.clone()));
    }

    #[test]
    fn external_settle_clears_pending_when_current_snapshot_missing() {
        let expected = cursor_snapshot("n", 10.0, 2.0);
        let action = decide_external_settle_action("n", true, Some(&expected), None);
        assert_eq!(action, ExternalSettleAction::ClearPending);
    }
}

mod handler_decision_tests {
    use super::super::handlers::{
        ExternalTriggerAction, KeyEventAction, decide_external_trigger_action,
        decide_key_event_action, should_bump_render_generation,
    };
    use super::super::policy::BufferEventPolicy;
    use super::cursor_snapshot;
    use crate::reducer::EventSource;

    #[test]
    fn external_trigger_clears_pending_in_cmdline_when_smear_to_cmd_disabled() {
        let action = decide_external_trigger_action(
            BufferEventPolicy::Normal,
            "c",
            false,
            25,
            Some(cursor_snapshot("c", 2.0, 3.0)),
            0.0,
        );
        assert_eq!(
            action,
            ExternalTriggerAction::ClearPending { clear_timer: true }
        );
    }

    #[test]
    fn external_trigger_schedules_settle_when_snapshot_exists() {
        let snapshot = cursor_snapshot("n", 5.0, 8.0);
        let action = decide_external_trigger_action(
            BufferEventPolicy::Normal,
            "n",
            true,
            40,
            Some(snapshot.clone()),
            0.0,
        );
        assert_eq!(
            action,
            ExternalTriggerAction::ScheduleSettle {
                delay_ms: 40,
                snapshot,
            }
        );
    }

    #[test]
    fn external_trigger_clears_pending_without_snapshot() {
        let action =
            decide_external_trigger_action(BufferEventPolicy::Normal, "n", true, 33, None, 0.0);
        assert_eq!(
            action,
            ExternalTriggerAction::ClearPending { clear_timer: false }
        );
    }

    #[test]
    fn key_event_noops_when_autocmd_is_recent() {
        let action = decide_key_event_action(BufferEventPolicy::Normal, 17, 30.0, 5.0);
        assert_eq!(action, KeyEventAction::None);
    }

    #[test]
    fn key_event_schedules_key_timer_when_autocmd_is_stale() {
        let action = decide_key_event_action(BufferEventPolicy::Normal, 17, 15.0, 60.0);
        assert_eq!(action, KeyEventAction::ScheduleKeyTimer { delay_ms: 17 });
    }

    #[test]
    fn render_generation_bumps_for_external_event_when_idle() {
        assert!(should_bump_render_generation(EventSource::External, false));
    }

    #[test]
    fn render_generation_does_not_bump_for_external_event_while_animating() {
        assert!(!should_bump_render_generation(EventSource::External, true));
    }

    #[test]
    fn render_generation_never_bumps_for_animation_tick() {
        assert!(!should_bump_render_generation(
            EventSource::AnimationTick,
            false
        ));
        assert!(!should_bump_render_generation(
            EventSource::AnimationTick,
            true
        ));
    }
}
