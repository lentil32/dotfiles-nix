use super::*;

#[test]
fn draw_frames_reuse_static_render_config_arc_when_config_is_unchanged() {
    let mut state = RuntimeState::default();
    let first = reduce_cursor_event(&mut state, "n", event(5.0, 6.0), EventSource::External);
    let _ = reduce_cursor_event(
        &mut state,
        "n",
        event_with_location(5.0, 12.0, 116.0, 11, 10, 20),
        EventSource::External,
    );
    let second = reduce_cursor_event(
        &mut state,
        "n",
        event_with_location(5.0, 12.0, 132.0, 11, 10, 20),
        EventSource::AnimationTick,
    );

    let first_static = draw_frame(&first)
        .map(|frame| Arc::clone(&frame.static_config))
        .expect("first transition should draw");
    let second_static = draw_frame(&second)
        .map(|frame| Arc::clone(&frame.static_config))
        .expect("second transition should draw");
    assert!(Arc::ptr_eq(&first_static, &second_static));
}
