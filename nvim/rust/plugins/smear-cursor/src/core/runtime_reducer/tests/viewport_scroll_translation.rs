use super::*;
use pretty_assertions::assert_eq;

#[test]
fn pure_vertical_viewport_scroll_translates_the_live_head() {
    let mut state = RuntimeState::default();
    state.config.delay_event_to_smear = 0.0;
    state.config.max_simulation_steps_per_frame = 0;

    let previous_location = CursorLocation::new(10, 20, 4, 12);
    state.initialize_cursor(
        Point {
            row: 15.0,
            col: 6.0,
        },
        CursorShape::new(false, false),
        7,
        &previous_location,
    );
    state.set_target(
        Point {
            row: 20.0,
            col: 6.0,
        },
        CursorShape::new(false, false),
    );
    state.start_animation_towards_target();
    state.set_last_tick_ms(Some(100.0));

    let center_before = trajectory_center(&state);
    let current_location = CursorLocation::new(10, 20, 6, 12);
    let transition = reduce_cursor_event(
        &mut state,
        "n",
        CursorEventContext {
            row: 18.0,
            col: 6.0,
            now_ms: 116.0,
            seed: 9,
            cursor_location: current_location.clone(),
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
        Point {
            row: center_before.row - 2.0,
            col: center_before.col,
        }
    );
    assert_eq!(
        state.target_position(),
        Point {
            row: 18.0,
            col: 6.0
        }
    );
    assert_eq!(state.tracked_location(), Some(current_location));
}

#[test]
fn pure_horizontal_viewport_scroll_translates_the_live_head() {
    let mut state = RuntimeState::default();
    state.config.delay_event_to_smear = 0.0;
    state.config.max_simulation_steps_per_frame = 0;

    let previous_location = CursorLocation::new(10, 20, 4, 12).with_viewport_columns(2, 0);
    state.initialize_cursor(
        Point {
            row: 15.0,
            col: 18.0,
        },
        CursorShape::new(false, false),
        7,
        &previous_location,
    );
    state.set_target(
        Point {
            row: 15.0,
            col: 24.0,
        },
        CursorShape::new(false, false),
    );
    state.start_animation_towards_target();
    state.set_last_tick_ms(Some(100.0));

    let center_before = trajectory_center(&state);
    let current_location = CursorLocation::new(10, 20, 4, 12).with_viewport_columns(5, 0);
    let transition = reduce_cursor_event(
        &mut state,
        "n",
        CursorEventContext {
            row: 15.0,
            col: 21.0,
            now_ms: 116.0,
            seed: 9,
            cursor_location: current_location.clone(),
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
        Point {
            row: center_before.row,
            col: center_before.col - 3.0,
        }
    );
    assert_eq!(
        state.target_position(),
        Point {
            row: 15.0,
            col: 21.0
        }
    );
    assert_eq!(state.tracked_location(), Some(current_location));
}

#[test]
fn window_origin_shift_translates_the_live_head_without_buffer_motion() {
    let mut state = RuntimeState::default();
    state.config.delay_event_to_smear = 0.0;
    state.config.max_simulation_steps_per_frame = 0;

    let previous_location = CursorLocation::new(10, 20, 4, 12)
        .with_viewport_columns(2, 0)
        .with_window_origin(3, 4);
    state.initialize_cursor(
        Point {
            row: 15.0,
            col: 18.0,
        },
        CursorShape::new(false, false),
        7,
        &previous_location,
    );
    state.set_target(
        Point {
            row: 15.0,
            col: 24.0,
        },
        CursorShape::new(false, false),
    );
    state.start_animation_towards_target();
    state.set_last_tick_ms(Some(100.0));

    let center_before = trajectory_center(&state);
    let current_location = CursorLocation::new(10, 20, 4, 12)
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
            cursor_location: current_location.clone(),
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
        Point {
            row: center_before.row + 2.0,
            col: center_before.col + 3.0,
        }
    );
    assert_eq!(
        state.target_position(),
        Point {
            row: 17.0,
            col: 27.0
        }
    );
    assert_eq!(state.tracked_location(), Some(current_location));
}
