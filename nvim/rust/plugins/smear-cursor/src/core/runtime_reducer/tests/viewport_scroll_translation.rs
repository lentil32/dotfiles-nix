use super::*;
use pretty_assertions::assert_eq;

#[test]
fn pure_vertical_viewport_scroll_translates_the_live_head() {
    let mut state = RuntimeState::default();
    state.config.delay_event_to_smear = 0.0;
    state.config.max_simulation_steps_per_frame = 0;

    let previous_location = TrackedCursor::fixture(10, 20, 4, 12);
    state.initialize_cursor(
        RenderPoint {
            row: 15.0,
            col: 6.0,
        },
        CursorShape::block(),
        7,
        &previous_location,
    );
    replace_target_preserving_tracking(
        &mut state,
        RenderPoint {
            row: 20.0,
            col: 6.0,
        },
        CursorShape::block(),
    );
    state.start_animation_towards_target();
    state.set_last_tick_ms(Some(100.0));

    let center_before = trajectory_center(&state);
    let baseline_epoch = state.retarget_epoch();
    let current_location = TrackedCursor::fixture(10, 20, 6, 12);
    let transition = reduce_cursor_event(
        &mut state,
        "n",
        CursorEventContext {
            row: 18.0,
            col: 6.0,
            now_ms: 116.0,
            seed: 9,
            tracked_cursor: current_location.clone(),
            scroll_shift: Some(ScrollShift {
                row_shift: 2.0,
                col_shift: 0.0,
                min_row: 1.0,
                max_row: 60.0,
            }),
            semantic_event: SemanticEvent::ViewportOrWindowMoved,
        },
        EventSource::External,
    );

    assert!(matches!(render_action(&transition), RenderAction::Draw(_)));
    assert_eq!(
        trajectory_center(&state),
        RenderPoint {
            row: center_before.row - 2.0,
            col: center_before.col,
        }
    );
    assert_eq!(
        state.target_position(),
        RenderPoint {
            row: 18.0,
            col: 6.0
        }
    );
    assert_eq!(state.retarget_epoch(), baseline_epoch.wrapping_add(1));
    assert_eq!(state.tracked_cursor(), Some(current_location));
}

#[test]
fn scroll_translation_can_preserve_the_target_key_while_updating_tracking() {
    let mut state = RuntimeState::default();
    state.config.delay_event_to_smear = 0.0;
    state.config.max_simulation_steps_per_frame = 0;

    let previous_location = TrackedCursor::fixture(10, 20, 4, 12);
    state.initialize_cursor(
        RenderPoint {
            row: 15.0,
            col: 6.0,
        },
        CursorShape::block(),
        7,
        &previous_location,
    );
    replace_target_preserving_tracking(
        &mut state,
        RenderPoint {
            row: 20.0,
            col: 6.0,
        },
        CursorShape::block(),
    );
    state.start_animation_towards_target();
    state.set_last_tick_ms(Some(100.0));

    let center_before = trajectory_center(&state);
    let baseline_target = state.target_position();
    let baseline_epoch = state.retarget_epoch();
    let current_location = TrackedCursor::fixture(10, 20, 6, 12);
    let transition = reduce_cursor_event(
        &mut state,
        "n",
        CursorEventContext {
            row: 20.0,
            col: 6.0,
            now_ms: 116.0,
            seed: 9,
            tracked_cursor: current_location.clone(),
            scroll_shift: Some(ScrollShift {
                row_shift: 2.0,
                col_shift: 0.0,
                min_row: 1.0,
                max_row: 60.0,
            }),
            semantic_event: SemanticEvent::ViewportOrWindowMoved,
        },
        EventSource::External,
    );
    let frame =
        draw_frame(&transition).expect("scroll-translated same-target motion should still draw");

    assert!(matches!(render_action(&transition), RenderAction::Draw(_)));
    assert_eq!(
        trajectory_center(&state),
        RenderPoint {
            row: center_before.row - 2.0,
            col: center_before.col,
        }
    );
    assert_eq!(frame.target, baseline_target);
    assert_eq!(state.target_position(), baseline_target);
    assert_eq!(frame.retarget_epoch, baseline_epoch);
    assert_eq!(state.retarget_epoch(), baseline_epoch);
    assert_eq!(state.tracked_cursor(), Some(current_location));
}

#[test]
fn pure_horizontal_viewport_scroll_translates_the_live_head() {
    let mut state = RuntimeState::default();
    state.config.delay_event_to_smear = 0.0;
    state.config.max_simulation_steps_per_frame = 0;

    let previous_location = TrackedCursor::fixture(10, 20, 4, 12).with_viewport_columns(2, 0);
    state.initialize_cursor(
        RenderPoint {
            row: 15.0,
            col: 18.0,
        },
        CursorShape::block(),
        7,
        &previous_location,
    );
    replace_target_preserving_tracking(
        &mut state,
        RenderPoint {
            row: 15.0,
            col: 24.0,
        },
        CursorShape::block(),
    );
    state.start_animation_towards_target();
    state.set_last_tick_ms(Some(100.0));

    let center_before = trajectory_center(&state);
    let baseline_epoch = state.retarget_epoch();
    let current_location = TrackedCursor::fixture(10, 20, 4, 12).with_viewport_columns(5, 0);
    let transition = reduce_cursor_event(
        &mut state,
        "n",
        CursorEventContext {
            row: 15.0,
            col: 21.0,
            now_ms: 116.0,
            seed: 9,
            tracked_cursor: current_location.clone(),
            scroll_shift: Some(ScrollShift {
                row_shift: 0.0,
                col_shift: 3.0,
                min_row: 1.0,
                max_row: 60.0,
            }),
            semantic_event: SemanticEvent::ViewportOrWindowMoved,
        },
        EventSource::External,
    );

    assert!(matches!(render_action(&transition), RenderAction::Draw(_)));
    assert_eq!(
        trajectory_center(&state),
        RenderPoint {
            row: center_before.row,
            col: center_before.col - 3.0,
        }
    );
    assert_eq!(
        state.target_position(),
        RenderPoint {
            row: 15.0,
            col: 21.0
        }
    );
    assert_eq!(state.retarget_epoch(), baseline_epoch.wrapping_add(1));
    assert_eq!(state.tracked_cursor(), Some(current_location));
}

#[test]
fn window_origin_shift_translates_the_live_head_without_buffer_motion() {
    let mut state = RuntimeState::default();
    state.config.delay_event_to_smear = 0.0;
    state.config.max_simulation_steps_per_frame = 0;

    let previous_location = TrackedCursor::fixture(10, 20, 4, 12)
        .with_viewport_columns(2, 0)
        .with_window_origin(3, 4);
    state.initialize_cursor(
        RenderPoint {
            row: 15.0,
            col: 18.0,
        },
        CursorShape::block(),
        7,
        &previous_location,
    );
    replace_target_preserving_tracking(
        &mut state,
        RenderPoint {
            row: 15.0,
            col: 24.0,
        },
        CursorShape::block(),
    );
    state.start_animation_towards_target();
    state.set_last_tick_ms(Some(100.0));

    let center_before = trajectory_center(&state);
    let baseline_epoch = state.retarget_epoch();
    let current_location = TrackedCursor::fixture(10, 20, 4, 12)
        .with_viewport_columns(2, 0)
        .with_window_origin(5, 7);
    let transition = reduce_cursor_event(
        &mut state,
        "n",
        CursorEventContext {
            row: 17.0,
            col: 27.0,
            now_ms: 116.0,
            seed: 9,
            tracked_cursor: current_location.clone(),
            scroll_shift: Some(ScrollShift {
                row_shift: -2.0,
                col_shift: -3.0,
                min_row: 5.0,
                max_row: 64.0,
            }),
            semantic_event: SemanticEvent::ViewportOrWindowMoved,
        },
        EventSource::External,
    );

    assert!(matches!(render_action(&transition), RenderAction::Draw(_)));
    assert_eq!(
        trajectory_center(&state),
        RenderPoint {
            row: center_before.row + 2.0,
            col: center_before.col + 3.0,
        }
    );
    assert_eq!(
        state.target_position(),
        RenderPoint {
            row: 17.0,
            col: 27.0
        }
    );
    assert_eq!(state.retarget_epoch(), baseline_epoch.wrapping_add(1));
    assert_eq!(state.tracked_cursor(), Some(current_location));
}
