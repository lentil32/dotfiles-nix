use super::super::options::apply_runtime_options;
use super::options_dict;
use crate::config::BufferPerfMode;
use crate::config::RuntimeConfig;
use crate::state::ColorOptionsPatch;
use crate::state::OptionalChange;
use crate::state::RuntimeOptionsPatch;
use crate::state::RuntimeState;
use crate::state::RuntimeSwitchesPatch;
use crate::test_support::proptest::pure_config;
use nvim_oxi::Object;
use proptest::prelude::*;
use std::collections::HashSet;
use std::sync::Arc;

fn apply_options_expect_ok(
    state: &mut RuntimeState,
    entries: impl IntoIterator<Item = (&'static str, Object)>,
) {
    let opts = options_dict(entries);
    let result = apply_runtime_options(state, &opts);
    assert!(
        result.is_ok(),
        "unexpected runtime option error: {result:?}"
    );
}

fn buffer_perf_mode_strategy() -> BoxedStrategy<BufferPerfMode> {
    prop_oneof![
        Just(BufferPerfMode::Auto),
        Just(BufferPerfMode::Full),
        Just(BufferPerfMode::Fast),
        Just(BufferPerfMode::Off),
    ]
    .boxed()
}

#[derive(Clone, Debug)]
enum InvalidRuntimeOptionCase {
    NonPositiveTimeInterval(f64),
    SubMillisecondTimeInterval(f64),
    TailResponseBelowHead {
        head_response_ms: f64,
        tail_response_ms: f64,
    },
    NonPositiveDampingRatio(f64),
    StopExitBelowEnter {
        stop_distance_enter: f64,
        stop_distance_exit: f64,
    },
    ZeroStopHoldFrames,
    NegativeTrailThickness(f64),
    TopKBelowTwo,
}

impl InvalidRuntimeOptionCase {
    fn entries(&self) -> Vec<(&'static str, Object)> {
        match self {
            Self::NonPositiveTimeInterval(time_interval) => {
                vec![("time_interval", Object::from(*time_interval))]
            }
            Self::SubMillisecondTimeInterval(time_interval) => {
                vec![("time_interval", Object::from(*time_interval))]
            }
            Self::TailResponseBelowHead {
                head_response_ms,
                tail_response_ms,
            } => vec![
                ("head_response_ms", Object::from(*head_response_ms)),
                ("tail_response_ms", Object::from(*tail_response_ms)),
            ],
            Self::NonPositiveDampingRatio(damping_ratio) => {
                vec![("damping_ratio", Object::from(*damping_ratio))]
            }
            Self::StopExitBelowEnter {
                stop_distance_enter,
                stop_distance_exit,
            } => vec![
                ("stop_distance_enter", Object::from(*stop_distance_enter)),
                ("stop_distance_exit", Object::from(*stop_distance_exit)),
            ],
            Self::ZeroStopHoldFrames => {
                vec![("stop_hold_frames", Object::from(0_i64))]
            }
            Self::NegativeTrailThickness(trail_thickness) => {
                vec![("trail_thickness", Object::from(*trail_thickness))]
            }
            Self::TopKBelowTwo => vec![("top_k_per_cell", Object::from(1_i64))],
        }
    }

    const fn expected_key(&self) -> &'static str {
        match self {
            Self::NonPositiveTimeInterval(_) | Self::SubMillisecondTimeInterval(_) => {
                "time_interval"
            }
            Self::TailResponseBelowHead { .. } => "tail_response_ms",
            Self::NonPositiveDampingRatio(_) => "damping_ratio",
            Self::StopExitBelowEnter { .. } => "stop_distance_exit",
            Self::ZeroStopHoldFrames => "stop_hold_frames",
            Self::NegativeTrailThickness(_) => "trail_thickness",
            Self::TopKBelowTwo => "top_k_per_cell",
        }
    }
}

fn invalid_runtime_option_case_strategy() -> BoxedStrategy<InvalidRuntimeOptionCase> {
    prop_oneof![
        prop_oneof![Just(0.0_f64), -64.0_f64..0.0_f64]
            .prop_map(InvalidRuntimeOptionCase::NonPositiveTimeInterval),
        (0.0_f64..1.0_f64)
            .prop_filter("time interval must be positive", |value| *value > 0.0)
            .prop_map(InvalidRuntimeOptionCase::SubMillisecondTimeInterval),
        ((1.0_f64..320.0_f64), (0.0_f64..320.0_f64))
            .prop_filter(
                "tail response must be lower than the head response",
                |(head, tail)| tail < head
            )
            .prop_map(|(head_response_ms, tail_response_ms)| {
                InvalidRuntimeOptionCase::TailResponseBelowHead {
                    head_response_ms,
                    tail_response_ms,
                }
            }),
        prop_oneof![Just(0.0_f64), -4.0_f64..0.0_f64]
            .prop_map(InvalidRuntimeOptionCase::NonPositiveDampingRatio),
        ((0.01_f64..2.0_f64), (0.0_f64..2.0_f64))
            .prop_filter(
                "stop exit must be lower than stop enter",
                |(enter, exit)| exit < enter
            )
            .prop_map(|(stop_distance_enter, stop_distance_exit)| {
                InvalidRuntimeOptionCase::StopExitBelowEnter {
                    stop_distance_enter,
                    stop_distance_exit,
                }
            }),
        Just(InvalidRuntimeOptionCase::ZeroStopHoldFrames),
        (-2.0_f64..0.0_f64).prop_map(InvalidRuntimeOptionCase::NegativeTrailThickness),
        Just(InvalidRuntimeOptionCase::TopKBelowTwo),
    ]
    .boxed()
}

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
fn runtime_options_patch_apply_clears_filetypes_list() {
    let mut state = RuntimeState::default();
    state.config.filetypes_disabled = Arc::new(
        ["lua".to_string(), "nix".to_string()]
            .into_iter()
            .collect::<HashSet<_>>(),
    );
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

proptest! {
    #![proptest_config(pure_config())]

    #[test]
    fn prop_apply_runtime_options_sets_requested_fields_and_preserves_others(
        explicit_time_interval in 1.0_f64..80.0_f64,
        simulation_hz in 1.0_f64..400.0_f64,
        max_simulation_steps_per_frame in 1_u32..32_u32,
        animate_in_insert_mode in any::<bool>(),
        animate_command_line in any::<bool>(),
        smear_between_windows in any::<bool>(),
        smear_between_buffers in any::<bool>(),
        max_kept_windows in 0_usize..128_usize,
        buffer_perf_mode in buffer_perf_mode_strategy(),
        stop_distance_enter in 0.0_f64..2.0_f64,
        stop_distance_exit_delta in 0.0_f64..2.0_f64,
        stop_velocity_enter in 0.0_f64..2.0_f64,
        stop_hold_frames in 1_u32..8_u32,
        tail_duration_ms in 1.0_f64..500.0_f64,
        spatial_coherence_weight in 0.0_f64..3.0_f64,
        temporal_stability_weight in 0.0_f64..3.0_f64,
        top_k_per_cell in 2_u8..32_u8,
        head_response_ms in 1.0_f64..300.0_f64,
        damping_ratio in 0.01_f64..3.0_f64,
        tail_response_delta in 0.0_f64..300.0_f64,
        trail_duration_ms in 1.0_f64..500.0_f64,
        trail_min_distance in 0.0_f64..5.0_f64,
        trail_thickness in 0.0_f64..5.0_f64,
        trail_thickness_x in 0.0_f64..5.0_f64,
    ) {
        let mut state = RuntimeState::default();
        let expected_tail_response_ms = head_response_ms + tail_response_delta;
        let expected_stop_distance_exit = stop_distance_enter + stop_distance_exit_delta;
        let entries = vec![
            ("simulation_hz", Object::from(simulation_hz)),
            (
                "max_simulation_steps_per_frame",
                Object::from(i64::from(max_simulation_steps_per_frame)),
            ),
            ("animate_in_insert_mode", Object::from(animate_in_insert_mode)),
            ("animate_command_line", Object::from(animate_command_line)),
            ("smear_between_windows", Object::from(smear_between_windows)),
            ("smear_between_buffers", Object::from(smear_between_buffers)),
            (
                "max_kept_windows",
                Object::from(i64::try_from(max_kept_windows).unwrap_or(i64::MAX)),
            ),
            ("buffer_perf_mode", Object::from(buffer_perf_mode.option_name())),
            ("stop_distance_enter", Object::from(stop_distance_enter)),
            ("stop_distance_exit", Object::from(expected_stop_distance_exit)),
            ("stop_velocity_enter", Object::from(stop_velocity_enter)),
            ("stop_hold_frames", Object::from(i64::from(stop_hold_frames))),
            ("tail_duration_ms", Object::from(tail_duration_ms)),
            (
                "spatial_coherence_weight",
                Object::from(spatial_coherence_weight),
            ),
            (
                "temporal_stability_weight",
                Object::from(temporal_stability_weight),
            ),
            ("top_k_per_cell", Object::from(i64::from(top_k_per_cell))),
            ("head_response_ms", Object::from(head_response_ms)),
            ("damping_ratio", Object::from(damping_ratio)),
            ("tail_response_ms", Object::from(expected_tail_response_ms)),
            ("trail_duration_ms", Object::from(trail_duration_ms)),
            ("trail_min_distance", Object::from(trail_min_distance)),
            ("trail_thickness", Object::from(trail_thickness)),
            ("trail_thickness_x", Object::from(trail_thickness_x)),
            ("time_interval", Object::from(explicit_time_interval)),
        ];
        apply_options_expect_ok(&mut state, entries);

        pretty_assertions::assert_eq!(
            state.config,
            RuntimeConfig {
                time_interval: explicit_time_interval,
                simulation_hz,
                max_simulation_steps_per_frame,
                animate_in_insert_mode,
                animate_command_line,
                smear_between_windows,
                smear_between_buffers,
                max_kept_windows,
                buffer_perf_mode,
                stop_distance_enter,
                stop_distance_exit: expected_stop_distance_exit,
                stop_velocity_enter,
                stop_hold_frames,
                tail_duration_ms,
                spatial_coherence_weight,
                temporal_stability_weight,
                top_k_per_cell,
                head_response_ms,
                damping_ratio,
                tail_response_ms: expected_tail_response_ms,
                trail_duration_ms,
                trail_min_distance,
                trail_thickness,
                trail_thickness_x,
                ..RuntimeConfig::default()
            }
        );
    }

    #[test]
    fn prop_apply_runtime_options_rejects_invalid_runtime_ranges(
        invalid_case in invalid_runtime_option_case_strategy()
    ) {
        let mut state = RuntimeState::default();
        let baseline = state.clone();
        let err = apply_runtime_options(&mut state, &options_dict(invalid_case.entries()))
            .expect_err("expected runtime option validation failure");

        prop_assert!(err.to_string().contains(invalid_case.expected_key()));
        prop_assert_eq!(state, baseline);
    }
}
