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

mod cleanup_tests {
    use crate::config::RuntimeConfig;
    use crate::core::runtime_reducer::{
        MIN_RENDER_CLEANUP_DELAY_MS, MIN_RENDER_HARD_PURGE_DELAY_MS,
        RENDER_HARD_PURGE_DELAY_MULTIPLIER, render_cleanup_delay_ms, render_hard_cleanup_delay_ms,
    };

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
}

mod event_loop_tests {
    use super::super::event_loop::EventLoopState;
    use super::super::handlers::{KeyEventAction, decide_key_event_action};
    use super::super::policy::BufferEventPolicy;

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

    #[test]
    fn key_fallback_decision_uses_buffer_policy_and_debounced_delay_only() {
        let action_enabled = decide_key_event_action(BufferEventPolicy::Normal, 17);
        assert_eq!(
            action_enabled,
            KeyEventAction::QueueKeyFallback { delay_ms: 17 }
        );
    }

    #[test]
    fn key_fallback_zero_delay_still_schedules_post_key_timer() {
        let action = decide_key_event_action(BufferEventPolicy::Normal, 0);
        assert_eq!(action, KeyEventAction::QueueKeyFallback { delay_ms: 1 });
    }

    #[test]
    fn event_loop_state_tracks_structured_ingress_counters() {
        let mut state = EventLoopState::new();
        state.record_ingress_received();
        state.record_ingress_received();
        state.record_ingress_applied();
        state.record_ingress_dropped();
        state.record_ingress_coalesced();
        state.record_ingress_starved();
        state.record_observation_request_executed();
        state.record_degraded_draw_application();
        state.record_stale_token_event();
        state.record_planner_compile_duration(320);
        state.record_planner_compile_duration(1280);
        state.record_planner_decode_duration(640);

        let metrics = state.runtime_metrics();
        assert_eq!(metrics.ingress_received, 2);
        assert_eq!(metrics.ingress_applied, 1);
        assert_eq!(metrics.ingress_dropped, 1);
        assert_eq!(metrics.ingress_coalesced, 1);
        assert_eq!(metrics.ingress_starved, 1);
        assert_eq!(metrics.observation_requests_executed, 1);
        assert_eq!(metrics.degraded_draw_applications, 1);
        assert_eq!(metrics.stale_token_events, 1);
        assert_eq!(metrics.planner_compile.samples, 2);
        assert_eq!(metrics.planner_compile.total_micros, 1600);
        assert_eq!(metrics.planner_compile.max_micros, 1280);
        assert_eq!(metrics.planner_decode.samples, 1);
        assert_eq!(metrics.planner_decode.total_micros, 640);
        assert_eq!(metrics.planner_decode.max_micros, 640);
    }
}

mod runtime_options_apply_tests {
    use super::super::options::apply_runtime_options;
    use super::{cterm_colors_object, options_dict};
    use crate::config::RuntimeConfig;
    use crate::state::{
        ColorOptionsPatch, OptionalChange, RuntimeOptionsPatch, RuntimeState, RuntimeSwitchesPatch,
    };
    use nvim_oxi::Object;

    #[test]
    fn runtime_options_patch_apply_clears_nullable_fields() {
        let mut state = RuntimeState::default();
        state.config.cursor_color = Some("#abcdef".to_string());
        let patch = RuntimeOptionsPatch {
            color: ColorOptionsPatch {
                cursor_color: Some(OptionalChange::Clear),
                ..ColorOptionsPatch::default()
            },
            ..RuntimeOptionsPatch::default()
        };

        patch.apply(&mut state);
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
        state.config.filetypes_disabled = vec!["lua".to_string(), "nix".to_string()].into();
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

    #[test]
    fn runtime_options_patch_apply_fps_derives_time_interval() {
        let mut state = RuntimeState::default();
        let opts = options_dict([("fps", Object::from(120.0_f64))]);

        let result = apply_runtime_options(&mut state, &opts);
        assert!(
            result.is_ok(),
            "unexpected runtime option error: {result:?}"
        );
        assert_eq!(
            state.config.time_interval,
            RuntimeConfig::interval_ms_for_fps(120.0)
        );
    }

    #[test]
    fn runtime_options_patch_apply_fps_overrides_explicit_time_interval() {
        let mut state = RuntimeState::default();
        let opts = options_dict([
            ("time_interval", Object::from(40.0_f64)),
            ("fps", Object::from(200.0_f64)),
        ]);

        let result = apply_runtime_options(&mut state, &opts);
        assert!(
            result.is_ok(),
            "unexpected runtime option error: {result:?}"
        );
        assert_eq!(
            state.config.time_interval,
            RuntimeConfig::interval_ms_for_fps(200.0)
        );
    }

    #[test]
    fn runtime_options_patch_apply_rejects_non_positive_fps() {
        let mut state = RuntimeState::default();
        let opts = options_dict([("fps", Object::from(0.0_f64))]);

        let result = apply_runtime_options(&mut state, &opts);
        assert!(result.is_err(), "expected fps=0 to be rejected");
    }

    #[test]
    fn runtime_options_patch_apply_sets_stop_hysteresis_thresholds() {
        let mut state = RuntimeState::default();
        let opts = options_dict([
            ("stop_distance_enter", Object::from(0.05_f64)),
            ("stop_distance_exit", Object::from(0.25_f64)),
            ("stop_velocity_enter", Object::from(0.08_f64)),
            ("stop_hold_frames", Object::from(3_i64)),
        ]);

        let result = apply_runtime_options(&mut state, &opts);
        assert!(
            result.is_ok(),
            "unexpected runtime option error: {result:?}"
        );
        assert_eq!(state.config.stop_distance_enter, 0.05_f64);
        assert_eq!(state.config.stop_distance_exit, 0.25_f64);
        assert_eq!(state.config.stop_velocity_enter, 0.08_f64);
        assert_eq!(state.config.stop_hold_frames, 3_u32);
    }

    #[test]
    fn runtime_options_patch_apply_sets_decode_pipeline_options() {
        let mut state = RuntimeState::default();
        let opts = options_dict([
            ("tail_duration_ms", Object::from(260.0_f64)),
            ("spatial_coherence_weight", Object::from(1.4_f64)),
            ("temporal_stability_weight", Object::from(0.22_f64)),
            ("top_k_per_cell", Object::from(6_i64)),
        ]);

        let result = apply_runtime_options(&mut state, &opts);
        assert!(
            result.is_ok(),
            "unexpected runtime option error: {result:?}"
        );
        assert_eq!(state.config.tail_duration_ms, 260.0_f64);
        assert_eq!(state.config.spatial_coherence_weight, 1.4_f64);
        assert_eq!(state.config.temporal_stability_weight, 0.22_f64);
        assert_eq!(state.config.top_k_per_cell, 6_u8);
    }

    #[test]
    fn runtime_options_patch_apply_sets_simulation_clock_options() {
        let mut state = RuntimeState::default();
        let opts = options_dict([
            ("fps", Object::from(144.0_f64)),
            ("simulation_hz", Object::from(240.0_f64)),
            ("max_simulation_steps_per_frame", Object::from(12_i64)),
        ]);

        let result = apply_runtime_options(&mut state, &opts);
        assert!(
            result.is_ok(),
            "unexpected runtime option error: {result:?}"
        );
        assert_eq!(state.config.simulation_hz, 240.0_f64);
        assert_eq!(state.config.max_simulation_steps_per_frame, 12_u32);
    }

    #[test]
    fn runtime_options_patch_apply_sets_animation_mode_flags() {
        let mut state = RuntimeState::default();
        let opts = options_dict([
            ("animate_in_insert_mode", Object::from(false)),
            ("animate_command_line", Object::from(false)),
        ]);

        let result = apply_runtime_options(&mut state, &opts);
        assert!(
            result.is_ok(),
            "unexpected runtime option error: {result:?}"
        );
        assert!(!state.config.animate_in_insert_mode);
        assert!(!state.config.animate_command_line);
    }

    #[test]
    fn runtime_options_patch_apply_sets_window_buffer_smear_flags_for_all_combinations() {
        let combos = [(false, false), (false, true), (true, false), (true, true)];
        for (smear_between_windows, smear_between_buffers) in combos {
            let mut state = RuntimeState::default();
            let opts = options_dict([
                ("smear_between_windows", Object::from(smear_between_windows)),
                ("smear_between_buffers", Object::from(smear_between_buffers)),
            ]);

            let result = apply_runtime_options(&mut state, &opts);
            assert!(
                result.is_ok(),
                "unexpected runtime option error for window/buffer flags: {result:?}"
            );
            assert_eq!(state.config.smear_between_windows, smear_between_windows);
            assert_eq!(state.config.smear_between_buffers, smear_between_buffers);
        }
    }

    #[test]
    fn runtime_options_patch_apply_sets_time_domain_motion_options() {
        let mut state = RuntimeState::default();
        let opts = options_dict([
            ("head_response_ms", Object::from(40.0_f64)),
            ("damping_ratio", Object::from(0.75_f64)),
            ("tail_response_ms", Object::from(120.0_f64)),
            ("trail_duration_ms", Object::from(220.0_f64)),
            ("trail_short_duration_ms", Object::from(50.0_f64)),
            ("trail_size", Object::from(0.7_f64)),
            ("trail_min_distance", Object::from(1.5_f64)),
            ("trail_thickness", Object::from(0.9_f64)),
            ("trail_thickness_x", Object::from(1.1_f64)),
        ]);

        let result = apply_runtime_options(&mut state, &opts);
        assert!(
            result.is_ok(),
            "unexpected runtime option error: {result:?}"
        );
        assert_eq!(state.config.head_response_ms, 40.0_f64);
        assert_eq!(state.config.damping_ratio, 0.75_f64);
        assert_eq!(state.config.tail_response_ms, 120.0_f64);
        assert_eq!(state.config.trail_duration_ms, 220.0_f64);
        assert_eq!(state.config.trail_short_duration_ms, 50.0_f64);
        assert_eq!(state.config.trail_size, 0.7_f64);
        assert_eq!(state.config.trail_min_distance, 1.5_f64);
        assert_eq!(state.config.trail_thickness, 0.9_f64);
        assert_eq!(state.config.trail_thickness_x, 1.1_f64);
    }

    #[test]
    fn runtime_options_patch_apply_rejects_tail_response_below_head_response() {
        let mut state = RuntimeState::default();
        let opts = options_dict([
            ("head_response_ms", Object::from(80.0_f64)),
            ("tail_response_ms", Object::from(40.0_f64)),
        ]);

        let result = apply_runtime_options(&mut state, &opts);
        assert!(result.is_err(), "expected incompatible response range");
    }

    #[test]
    fn runtime_options_patch_apply_rejects_non_positive_damping_ratio() {
        let mut state = RuntimeState::default();
        let opts = options_dict([("damping_ratio", Object::from(0.0_f64))]);

        let result = apply_runtime_options(&mut state, &opts);
        assert!(
            result.is_err(),
            "expected non-positive damping_ratio to be rejected"
        );
    }

    #[test]
    fn runtime_options_patch_apply_rejects_stop_exit_below_stop_enter() {
        let mut state = RuntimeState::default();
        let opts = options_dict([
            ("stop_distance_enter", Object::from(0.4_f64)),
            ("stop_distance_exit", Object::from(0.2_f64)),
        ]);

        let result = apply_runtime_options(&mut state, &opts);
        assert!(result.is_err(), "expected invalid stop distance range");
    }

    #[test]
    fn runtime_options_patch_apply_rejects_zero_stop_hold_frames() {
        let mut state = RuntimeState::default();
        let opts = options_dict([("stop_hold_frames", Object::from(0_i64))]);

        let result = apply_runtime_options(&mut state, &opts);
        assert!(
            result.is_err(),
            "expected stop_hold_frames=0 to be rejected"
        );
    }

    #[test]
    fn runtime_options_patch_apply_rejects_out_of_range_trail_size() {
        let mut state = RuntimeState::default();
        let opts = options_dict([("trail_size", Object::from(1.5_f64))]);

        let result = apply_runtime_options(&mut state, &opts);
        assert!(result.is_err(), "expected invalid trail_size");
    }

    #[test]
    fn runtime_options_patch_apply_rejects_negative_trail_thickness() {
        let mut state = RuntimeState::default();
        let opts = options_dict([("trail_thickness", Object::from(-0.1_f64))]);

        let result = apply_runtime_options(&mut state, &opts);
        assert!(result.is_err(), "expected invalid trail_thickness");
    }

    #[test]
    fn runtime_options_patch_apply_rejects_top_k_per_cell_below_two() {
        let mut state = RuntimeState::default();
        let opts = options_dict([("top_k_per_cell", Object::from(1_i64))]);

        let result = apply_runtime_options(&mut state, &opts);
        assert!(
            result.is_err(),
            "expected top_k_per_cell lower-bound failure"
        );
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
            "cursor_color",
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

    #[test]
    fn runtime_options_patch_parse_rejects_non_positive_simulation_hz() {
        let opts = options_dict([("simulation_hz", Object::from(0.0_f64))]);

        let err = RuntimeOptionsPatch::parse(&opts).expect_err("expected parse failure");
        assert!(
            err.to_string().contains("simulation_hz"),
            "unexpected error: {err}"
        );
        assert!(
            err.to_string().contains("positive number"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn runtime_options_patch_parse_rejects_top_k_per_cell_out_of_u8_range() {
        let opts = options_dict([("top_k_per_cell", Object::from(999_i64))]);

        let err = RuntimeOptionsPatch::parse(&opts).expect_err("expected parse failure");
        assert!(
            err.to_string().contains("top_k_per_cell"),
            "unexpected error: {err}"
        );
        assert!(
            err.to_string().contains("between 2 and 255"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn runtime_options_patch_parse_rejects_legacy_and_removed_option_keys() {
        for (legacy_key, value) in [
            ("stiffness", Object::from(0.5_f64)),
            ("neovide_parity_mode", Object::from(true)),
            ("aa_band_min", Object::from(0.5_f64)),
            ("aa_band_max", Object::from(0.5_f64)),
            ("edge_gate_low", Object::from(0.5_f64)),
            ("edge_gate_high", Object::from(0.5_f64)),
            ("temporal_hysteresis_enter", Object::from(0.5_f64)),
            ("temporal_hysteresis_exit", Object::from(0.5_f64)),
        ] {
            let opts = options_dict([(legacy_key, value)]);
            let err = RuntimeOptionsPatch::parse(&opts).expect_err("expected parse failure");
            assert!(
                err.to_string().contains(legacy_key),
                "unexpected error: {err}"
            );
            let expects_removed_message = matches!(
                legacy_key,
                "aa_band_min"
                    | "aa_band_max"
                    | "edge_gate_low"
                    | "edge_gate_high"
                    | "temporal_hysteresis_enter"
                    | "temporal_hysteresis_exit"
            );
            if expects_removed_message {
                assert!(err.to_string().contains("removed render heuristic"));
            } else {
                assert!(
                    err.to_string().contains("supported option key"),
                    "unexpected error: {err}"
                );
            }
        }
    }
}

mod buffer_policy_tests {
    use super::super::policy::{
        BufferEventPolicy, IngressCursorPresentationContext, IngressCursorPresentationPolicy,
    };
    use crate::types::ScreenCell;

    #[test]
    fn normal_policy_enables_key_fallback_and_explicit_ingress_prepaint() {
        let policy = BufferEventPolicy::from_buffer_metadata("terminal", true, 1, 0.0);
        let cell = ScreenCell::new(3, 7).expect("valid test cell");
        assert!(policy.use_key_fallback());
        assert_eq!(
            policy.ingress_cursor_presentation_policy(IngressCursorPresentationContext::new(
                true,
                false,
                true,
                false,
                true,
                Some(cell),
                90,
            )),
            IngressCursorPresentationPolicy::HideCursorAndPrepaint { cell, zindex: 90 }
        );
    }

    #[test]
    fn normal_policy_hides_without_prepaint_when_target_cell_cannot_round() {
        let policy = BufferEventPolicy::Normal;
        assert_eq!(
            policy.ingress_cursor_presentation_policy(IngressCursorPresentationContext::new(
                true, false, true, false, true, None, 90,
            )),
            IngressCursorPresentationPolicy::HideCursor
        );
    }

    #[test]
    fn normal_policy_skips_ingress_cursor_presentation_when_runtime_is_ineligible() {
        let policy = BufferEventPolicy::Normal;
        let cell = ScreenCell::new(3, 7).expect("valid test cell");
        for context in [
            IngressCursorPresentationContext::new(true, true, true, false, true, Some(cell), 90),
            IngressCursorPresentationContext::new(false, false, true, false, true, Some(cell), 90),
            IngressCursorPresentationContext::new(true, false, false, false, true, Some(cell), 90),
            IngressCursorPresentationContext::new(true, false, true, true, true, Some(cell), 90),
            IngressCursorPresentationContext::new(true, false, true, false, false, Some(cell), 90),
        ] {
            assert_eq!(
                policy.ingress_cursor_presentation_policy(context),
                IngressCursorPresentationPolicy::NoAction
            );
        }
    }
}

mod handler_decision_tests {
    use super::super::handlers::{
        select_core_event_source, should_request_observation_for_autocmd,
    };
    use super::super::ingress::AutocmdIngress;
    use crate::core::runtime_reducer::EventSource;
    use crate::state::{CursorLocation, CursorShape, RuntimeState};
    use crate::types::Point;

    #[test]
    fn core_observation_request_autocmd_filter_includes_cmdline_changed() {
        assert!(should_request_observation_for_autocmd(
            AutocmdIngress::CursorMoved
        ));
        assert!(should_request_observation_for_autocmd(
            AutocmdIngress::CursorMovedInsert
        ));
        assert!(should_request_observation_for_autocmd(
            AutocmdIngress::WinEnter
        ));
        assert!(should_request_observation_for_autocmd(
            AutocmdIngress::WinScrolled
        ));
        assert!(should_request_observation_for_autocmd(
            AutocmdIngress::CmdlineChanged
        ));
        assert!(!should_request_observation_for_autocmd(
            AutocmdIngress::ModeChanged
        ));
        assert!(!should_request_observation_for_autocmd(
            AutocmdIngress::BufEnter
        ));
        assert!(!should_request_observation_for_autocmd(
            AutocmdIngress::Unknown
        ));
    }

    fn initialized_state() -> RuntimeState {
        let mut state = RuntimeState::default();
        state.initialize_cursor(
            Point {
                row: 10.0,
                col: 20.0,
            },
            CursorShape::new(false, false),
            7,
            &CursorLocation::new(1, 1, 1, 10),
        );
        state
    }

    fn animating_state() -> RuntimeState {
        let mut state = initialized_state();
        state.start_animation();
        state
    }

    #[test]
    fn select_core_event_source_uses_external_when_uninitialized() {
        let state = RuntimeState::default();
        let source = select_core_event_source(
            "n",
            &state,
            Some(Point {
                row: 10.0,
                col: 20.0,
            }),
            &CursorLocation::new(1, 1, 1, 10),
        );
        assert_eq!(source, EventSource::External);
    }

    #[test]
    fn select_core_event_source_uses_external_when_target_changes_while_idle() {
        let state = initialized_state();
        let source = select_core_event_source(
            "n",
            &state,
            Some(Point {
                row: 18.0,
                col: 28.0,
            }),
            &CursorLocation::new(1, 1, 1, 10),
        );
        assert_eq!(source, EventSource::External);
    }

    #[test]
    fn select_core_event_source_uses_animation_tick_for_inflight_target_change() {
        let state = animating_state();
        let source = select_core_event_source(
            "n",
            &state,
            Some(Point {
                row: 14.0,
                col: 26.0,
            }),
            &CursorLocation::new(1, 1, 1, 12),
        );
        assert_eq!(source, EventSource::AnimationTick);
    }

    #[test]
    fn select_core_event_source_uses_external_for_inflight_target_change_after_window_switch() {
        let state = animating_state();
        let source = select_core_event_source(
            "n",
            &state,
            Some(Point {
                row: 14.0,
                col: 26.0,
            }),
            &CursorLocation::new(2, 1, 1, 12),
        );
        assert_eq!(source, EventSource::External);
    }

    #[test]
    fn select_core_event_source_uses_external_when_location_changes_without_target_delta() {
        let state = initialized_state();
        let source = select_core_event_source(
            "n",
            &state,
            Some(Point {
                row: 10.0,
                col: 20.0,
            }),
            &CursorLocation::new(1, 1, 5, 15),
        );
        assert_eq!(source, EventSource::External);
    }

    #[test]
    fn select_core_event_source_uses_animation_tick_when_state_and_target_are_stable() {
        let state = initialized_state();
        let source = select_core_event_source(
            "n",
            &state,
            Some(Point {
                row: 10.0,
                col: 20.0,
            }),
            &CursorLocation::new(1, 1, 1, 10),
        );
        assert_eq!(source, EventSource::AnimationTick);
    }
}
