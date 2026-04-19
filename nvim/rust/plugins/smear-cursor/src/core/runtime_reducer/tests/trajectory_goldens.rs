use super::*;

fn scroll_while_animating_steps() -> [TrajectoryStep; 7] {
    [
        external_step(15.0, 6.0, 100.0, 41, 10, 20),
        external_step(24.0, 6.0, 108.0, 42, 10, 20),
        animation_tick_step(24.0, 6.0, 116.0, 43, 10, 20),
        external_with_scroll_step(
            24.0,
            12.0,
            124.0,
            44,
            10,
            20,
            ScrollShift {
                row_shift: 2.0,
                col_shift: 0.0,
                min_row: 1.0,
                max_row: 60.0,
            },
        ),
        animation_tick_step(24.0, 12.0, 132.0, 45, 10, 20),
        animation_tick_with_scroll_step(
            24.0,
            12.0,
            140.0,
            46,
            10,
            20,
            ScrollShift {
                row_shift: 1.0,
                col_shift: 0.0,
                min_row: 1.0,
                max_row: 60.0,
            },
        ),
        animation_tick_step(24.0, 12.0, 148.0, 47, 10, 20),
    ]
}

#[test]
fn rapid_horizontal_motion() {
    let mut state = RuntimeState::default();
    state.config.delay_event_to_smear = 0.0;
    state.config.simulation_hz = 240.0;
    state.config.max_simulation_steps_per_frame = 16;
    let seed_state = state.clone();

    let steps = [
        external_step(5.0, 6.0, 100.0, 7, 10, 20),
        external_step(5.0, 16.0, 108.0, 8, 10, 20),
        animation_tick_step(5.0, 16.0, 116.0, 9, 10, 20),
        external_step(5.0, 24.0, 120.0, 10, 10, 20),
        animation_tick_step(5.0, 24.0, 128.0, 11, 10, 20),
        external_step(5.0, 36.0, 132.0, 12, 10, 20),
        animation_tick_step(5.0, 36.0, 140.0, 13, 10, 20),
        animation_tick_step(5.0, 36.0, 148.0, 14, 10, 20),
    ];

    let transcript = trajectory_transcript(&mut state, "n", &steps);
    let replay = trajectory_transcript_with_fresh_state(&seed_state, "n", &steps);
    pretty_assertions::assert_eq!(transcript, replay, "trajectory must be deterministic");
    insta::assert_snapshot!(transcript.render());
}

#[test]
fn diagonal_zig_zag_motion() {
    let mut state = RuntimeState::default();
    state.config.delay_event_to_smear = 0.0;
    state.config.simulation_hz = 240.0;
    state.config.max_simulation_steps_per_frame = 16;
    let seed_state = state.clone();

    let steps = [
        external_step(5.0, 6.0, 100.0, 21, 10, 20),
        external_step(9.0, 12.0, 108.0, 22, 10, 20),
        animation_tick_step(9.0, 12.0, 116.0, 23, 10, 20),
        external_step(4.0, 20.0, 124.0, 24, 10, 20),
        animation_tick_step(4.0, 20.0, 132.0, 25, 10, 20),
        external_step(10.0, 28.0, 140.0, 26, 10, 20),
        animation_tick_step(10.0, 28.0, 148.0, 27, 10, 20),
        animation_tick_step(10.0, 28.0, 156.0, 28, 10, 20),
    ];

    let transcript = trajectory_transcript(&mut state, "n", &steps);
    let replay = trajectory_transcript_with_fresh_state(&seed_state, "n", &steps);
    pretty_assertions::assert_eq!(transcript, replay, "trajectory must be deterministic");
    insta::assert_snapshot!(transcript.render());
}

#[test]
fn surface_retarget_with_overlay_for_small_cross_window_motion() {
    let mut state = RuntimeState::default();
    state.config.delay_event_to_smear = 0.0;
    state.config.simulation_hz = 240.0;
    state.config.max_simulation_steps_per_frame = 16;
    state.config.hide_target_hack = true;
    state.config.smear_between_windows = true;
    state.commit_runtime_config_update();
    let seed_state = state.clone();

    let steps = [
        external_step(5.0, 6.0, 100.0, 51, 10, 20),
        external_step(8.0, 12.0, 108.0, 52, 11, 20),
        animation_tick_step(8.0, 12.0, 116.0, 53, 11, 20),
    ];

    let transcript = trajectory_transcript(&mut state, "n", &steps);
    let replay = trajectory_transcript_with_fresh_state(&seed_state, "n", &steps);
    pretty_assertions::assert_eq!(transcript, replay, "trajectory must be deterministic");
    insta::assert_snapshot!(transcript.render());
}

#[test]
fn window_and_buffer_switch_motion() {
    let mut state = RuntimeState::default();
    state.config.delay_event_to_smear = 0.0;
    state.config.simulation_hz = 240.0;
    state.config.max_simulation_steps_per_frame = 16;
    state.config.smear_between_windows = true;
    state.config.smear_between_buffers = true;
    let seed_state = state.clone();

    let steps = [
        external_step(5.0, 6.0, 100.0, 31, 10, 20),
        external_step(20.0, 30.0, 108.0, 32, 11, 21),
        animation_tick_step(20.0, 30.0, 116.0, 33, 11, 21),
        external_step(9.0, 14.0, 124.0, 34, 10, 20),
        animation_tick_step(9.0, 14.0, 132.0, 35, 10, 20),
        external_step(24.0, 4.0, 140.0, 36, 12, 22),
        animation_tick_step(24.0, 4.0, 148.0, 37, 12, 22),
        animation_tick_step(24.0, 4.0, 156.0, 38, 12, 22),
    ];

    let transcript = trajectory_transcript(&mut state, "n", &steps);
    let replay = trajectory_transcript_with_fresh_state(&seed_state, "n", &steps);
    pretty_assertions::assert_eq!(transcript, replay, "trajectory must be deterministic");
    insta::assert_snapshot!(transcript.render());
}

#[test]
fn scroll_while_animating() {
    let mut state = RuntimeState::default();
    state.config.delay_event_to_smear = 0.0;
    state.config.simulation_hz = 240.0;
    state.config.max_simulation_steps_per_frame = 16;
    let seed_state = state.clone();

    let steps = scroll_while_animating_steps();

    let transcript = trajectory_transcript(&mut state, "n", &steps);
    let replay = trajectory_transcript_with_fresh_state(&seed_state, "n", &steps);
    pretty_assertions::assert_eq!(transcript, replay, "trajectory must be deterministic");
    insta::assert_snapshot!(transcript.render());
}

#[test]
fn horizontal_scroll_while_animating() {
    let mut state = RuntimeState::default();
    state.config.delay_event_to_smear = 0.0;
    state.config.simulation_hz = 240.0;
    state.config.max_simulation_steps_per_frame = 16;
    let seed_state = state.clone();

    let steps = [
        TrajectoryStep {
            source: EventSource::External,
            event: CursorEventContext {
                row: 15.0,
                col: 18.0,
                now_ms: 100.0,
                seed: 71,
                tracked_cursor: TrackedCursor::fixture(10, 20, 4, 12).with_viewport_columns(2, 0),
                scroll_shift: None,
                semantic_event: SemanticEvent::FrameCommitted,
            },
        },
        TrajectoryStep {
            source: EventSource::External,
            event: CursorEventContext {
                row: 15.0,
                col: 24.0,
                now_ms: 108.0,
                seed: 72,
                tracked_cursor: TrackedCursor::fixture(10, 20, 4, 12).with_viewport_columns(2, 0),
                scroll_shift: None,
                semantic_event: SemanticEvent::FrameCommitted,
            },
        },
        TrajectoryStep {
            source: EventSource::AnimationTick,
            event: CursorEventContext {
                row: 15.0,
                col: 24.0,
                now_ms: 116.0,
                seed: 73,
                tracked_cursor: TrackedCursor::fixture(10, 20, 4, 12).with_viewport_columns(2, 0),
                scroll_shift: None,
                semantic_event: SemanticEvent::FrameCommitted,
            },
        },
        TrajectoryStep {
            source: EventSource::External,
            event: CursorEventContext {
                row: 15.0,
                col: 21.0,
                now_ms: 124.0,
                seed: 74,
                tracked_cursor: TrackedCursor::fixture(10, 20, 4, 12).with_viewport_columns(5, 0),
                scroll_shift: Some(ScrollShift {
                    row_shift: 0.0,
                    col_shift: 3.0,
                    min_row: 1.0,
                    max_row: 60.0,
                }),
                semantic_event: SemanticEvent::ViewportOrWindowMoved,
            },
        },
        TrajectoryStep {
            source: EventSource::AnimationTick,
            event: CursorEventContext {
                row: 15.0,
                col: 21.0,
                now_ms: 132.0,
                seed: 75,
                tracked_cursor: TrackedCursor::fixture(10, 20, 4, 12).with_viewport_columns(5, 0),
                scroll_shift: None,
                semantic_event: SemanticEvent::FrameCommitted,
            },
        },
    ];

    let transcript = trajectory_transcript(&mut state, "n", &steps);
    let replay = trajectory_transcript_with_fresh_state(&seed_state, "n", &steps);
    pretty_assertions::assert_eq!(transcript, replay, "trajectory must be deterministic");
    insta::assert_snapshot!(transcript.render());
}

#[test]
fn window_origin_shift_while_animating() {
    let mut state = RuntimeState::default();
    state.config.delay_event_to_smear = 0.0;
    state.config.simulation_hz = 240.0;
    state.config.max_simulation_steps_per_frame = 16;
    let seed_state = state.clone();

    let steps = [
        TrajectoryStep {
            source: EventSource::External,
            event: CursorEventContext {
                row: 15.0,
                col: 18.0,
                now_ms: 100.0,
                seed: 81,
                tracked_cursor: TrackedCursor::fixture(10, 20, 4, 12)
                    .with_viewport_columns(2, 0)
                    .with_window_origin(3, 4),
                scroll_shift: None,
                semantic_event: SemanticEvent::FrameCommitted,
            },
        },
        TrajectoryStep {
            source: EventSource::External,
            event: CursorEventContext {
                row: 15.0,
                col: 24.0,
                now_ms: 108.0,
                seed: 82,
                tracked_cursor: TrackedCursor::fixture(10, 20, 4, 12)
                    .with_viewport_columns(2, 0)
                    .with_window_origin(3, 4),
                scroll_shift: None,
                semantic_event: SemanticEvent::FrameCommitted,
            },
        },
        TrajectoryStep {
            source: EventSource::AnimationTick,
            event: CursorEventContext {
                row: 15.0,
                col: 24.0,
                now_ms: 116.0,
                seed: 83,
                tracked_cursor: TrackedCursor::fixture(10, 20, 4, 12)
                    .with_viewport_columns(2, 0)
                    .with_window_origin(3, 4),
                scroll_shift: None,
                semantic_event: SemanticEvent::FrameCommitted,
            },
        },
        TrajectoryStep {
            source: EventSource::External,
            event: CursorEventContext {
                row: 17.0,
                col: 27.0,
                now_ms: 124.0,
                seed: 84,
                tracked_cursor: TrackedCursor::fixture(10, 20, 4, 12)
                    .with_viewport_columns(2, 0)
                    .with_window_origin(5, 7),
                scroll_shift: Some(ScrollShift {
                    row_shift: -2.0,
                    col_shift: -3.0,
                    min_row: 5.0,
                    max_row: 64.0,
                }),
                semantic_event: SemanticEvent::ViewportOrWindowMoved,
            },
        },
        TrajectoryStep {
            source: EventSource::AnimationTick,
            event: CursorEventContext {
                row: 17.0,
                col: 27.0,
                now_ms: 132.0,
                seed: 85,
                tracked_cursor: TrackedCursor::fixture(10, 20, 4, 12)
                    .with_viewport_columns(2, 0)
                    .with_window_origin(5, 7),
                scroll_shift: None,
                semantic_event: SemanticEvent::FrameCommitted,
            },
        },
    ];

    let transcript = trajectory_transcript(&mut state, "n", &steps);
    let replay = trajectory_transcript_with_fresh_state(&seed_state, "n", &steps);
    pretty_assertions::assert_eq!(transcript, replay, "trajectory must be deterministic");
    insta::assert_snapshot!(transcript.render());
}

#[test]
fn window_resize_while_animating() {
    let mut state = RuntimeState::default();
    state.config.delay_event_to_smear = 0.0;
    state.config.simulation_hz = 240.0;
    state.config.max_simulation_steps_per_frame = 16;
    let seed_state = state.clone();

    let steps = [
        TrajectoryStep {
            source: EventSource::External,
            event: CursorEventContext {
                row: 15.0,
                col: 18.0,
                now_ms: 100.0,
                seed: 91,
                tracked_cursor: TrackedCursor::fixture(10, 20, 4, 12)
                    .with_window_dimensions(80, 24),
                scroll_shift: None,
                semantic_event: SemanticEvent::FrameCommitted,
            },
        },
        TrajectoryStep {
            source: EventSource::External,
            event: CursorEventContext {
                row: 15.0,
                col: 24.0,
                now_ms: 108.0,
                seed: 92,
                tracked_cursor: TrackedCursor::fixture(10, 20, 4, 12)
                    .with_window_dimensions(80, 24),
                scroll_shift: None,
                semantic_event: SemanticEvent::FrameCommitted,
            },
        },
        TrajectoryStep {
            source: EventSource::AnimationTick,
            event: CursorEventContext {
                row: 15.0,
                col: 24.0,
                now_ms: 116.0,
                seed: 93,
                tracked_cursor: TrackedCursor::fixture(10, 20, 4, 12)
                    .with_window_dimensions(80, 24),
                scroll_shift: None,
                semantic_event: SemanticEvent::FrameCommitted,
            },
        },
        TrajectoryStep {
            source: EventSource::External,
            event: CursorEventContext {
                row: 15.0,
                col: 24.0,
                now_ms: 124.0,
                seed: 94,
                tracked_cursor: TrackedCursor::fixture(10, 20, 4, 12)
                    .with_window_dimensions(72, 24),
                scroll_shift: None,
                semantic_event: SemanticEvent::ViewportOrWindowMoved,
            },
        },
        TrajectoryStep {
            source: EventSource::AnimationTick,
            event: CursorEventContext {
                row: 15.0,
                col: 24.0,
                now_ms: 132.0,
                seed: 95,
                tracked_cursor: TrackedCursor::fixture(10, 20, 4, 12)
                    .with_window_dimensions(72, 24),
                scroll_shift: None,
                semantic_event: SemanticEvent::FrameCommitted,
            },
        },
    ];

    let transcript = trajectory_transcript(&mut state, "n", &steps);
    let replay = trajectory_transcript_with_fresh_state(&seed_state, "n", &steps);
    pretty_assertions::assert_eq!(transcript, replay, "trajectory must be deterministic");
    insta::assert_snapshot!(transcript.render());
}

#[test]
fn scroll_retarget_keeps_target_in_post_scroll_screen_coordinates() {
    let mut state = RuntimeState::default();
    state.config.delay_event_to_smear = 0.0;
    state.config.simulation_hz = 240.0;
    state.config.max_simulation_steps_per_frame = 16;

    let steps = scroll_while_animating_steps();
    let transcript = trajectory_transcript(&mut state, "n", &steps);
    let scroll_retarget = &transcript.steps[3];
    let frame_target = match &scroll_retarget.transition.render_action {
        RenderActionSummary::Draw(frame) => Some(frame.target),
        RenderActionSummary::ClearAll | RenderActionSummary::Noop => None,
    };
    let expected_target = PointSummary::from_point(RenderPoint {
        row: 24.0,
        col: 12.0,
    });

    pretty_assertions::assert_eq!(
        (scroll_retarget.state.target, frame_target),
        (expected_target, Some(expected_target))
    );
}

#[test]
fn cmdline_clear_redraw_when_runtime_is_disabled() {
    let mut state = RuntimeState::default();
    state.set_enabled(false);
    let seed_state = state.clone();

    let steps = [external_step(3.0, 8.0, 100.0, 61, 10, 20)];

    let transcript = trajectory_transcript(&mut state, "c", &steps);
    let replay = trajectory_transcript_with_fresh_state(&seed_state, "c", &steps);
    pretty_assertions::assert_eq!(transcript, replay, "trajectory must be deterministic");
    insta::assert_snapshot!(transcript.render());
}
