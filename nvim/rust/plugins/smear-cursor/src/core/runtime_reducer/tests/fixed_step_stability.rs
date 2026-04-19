use super::*;
use crate::test_support::proptest::pure_config;
use proptest::collection::vec;

#[derive(Clone, Copy, Debug)]
struct StabilitySummary {
    settle_at_ms: f64,
    peak_distance: f64,
    final_center: RenderPoint,
}

fn run_fps_stability_scenario(render_fps: f64) -> StabilitySummary {
    let mut state = RuntimeState::default();
    state.config.time_interval = RuntimeConfig::interval_ms_for_fps(render_fps);
    state.config.simulation_hz = 240.0;
    state.config.max_simulation_steps_per_frame = 16;
    state.config.delay_event_to_smear = 0.0;
    state.config.stop_distance_enter = 0.08;
    state.config.stop_distance_exit = 0.16;
    state.config.stop_velocity_enter = 0.05;
    state.config.stop_hold_frames = 2;

    let _ = reduce_cursor_event(
        &mut state,
        "n",
        event_with_location(5.0, 6.0, 100.0, 51, 10, 20),
        EventSource::External,
    );
    let _ = reduce_cursor_event(
        &mut state,
        "n",
        event_with_location(5.0, 56.0, 108.0, 52, 10, 20),
        EventSource::External,
    );

    let frame_dt = RuntimeConfig::interval_ms_for_fps(render_fps);
    let target = RenderPoint {
        row: 5.0,
        col: 56.0,
    };
    let mut now_ms = 108.0;
    let mut peak_distance = 0.0_f64;
    let mut settle_at_ms = now_ms;

    for _ in 0..600 {
        now_ms += frame_dt;
        let _ = reduce_cursor_event(
            &mut state,
            "n",
            event_with_location(target.row, target.col, now_ms, 53, 10, 20),
            EventSource::AnimationTick,
        );

        let center = trajectory_center(&state);
        peak_distance = peak_distance.max((center.col - target.col).abs());
        if !state.is_animating() && !state.is_settling() {
            settle_at_ms = now_ms;
            break;
        }
    }

    StabilitySummary {
        settle_at_ms,
        peak_distance,
        final_center: trajectory_center(&state),
    }
}

proptest! {
    #![proptest_config(pure_config())]

    #[test]
    fn prop_fixed_simulation_rate_keeps_motion_stable_across_render_fps(
        render_fps_values in vec(48_u16..=240_u16, 2..=6),
    ) {
        let summaries = render_fps_values
            .into_iter()
            .map(|render_fps| run_fps_stability_scenario(f64::from(render_fps)))
            .collect::<Vec<_>>();
        let settle_min = summaries
            .iter()
            .map(|summary| summary.settle_at_ms)
            .fold(f64::INFINITY, f64::min);
        let settle_max = summaries
            .iter()
            .map(|summary| summary.settle_at_ms)
            .fold(0.0_f64, f64::max);
        let peak_min = summaries
            .iter()
            .map(|summary| summary.peak_distance)
            .fold(f64::INFINITY, f64::min);
        let peak_max = summaries
            .iter()
            .map(|summary| summary.peak_distance)
            .fold(0.0_f64, f64::max);

        for summary in &summaries {
            prop_assert!((summary.final_center.row - 5.5).abs() <= 0.2);
            prop_assert!((summary.final_center.col - 56.5).abs() <= 0.2);
        }
        prop_assert!(
            settle_max - settle_min <= 220.0,
            "settle timing drifted too much across fps: min={settle_min} max={settle_max} summaries={summaries:?}"
        );
        prop_assert!(
            peak_max - peak_min <= 20.0,
            "peak motion envelope drifted too much across fps: min={peak_min} max={peak_max} summaries={summaries:?}"
        );
    }
}
