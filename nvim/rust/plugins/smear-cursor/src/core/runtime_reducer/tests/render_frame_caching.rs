use super::*;

#[test]
fn draw_frames_rebuild_equivalent_static_render_config_views_when_config_is_unchanged() {
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

    let first_frame = draw_frame(&first).expect("first transition should draw");
    let second_frame = draw_frame(&second).expect("second transition should draw");

    let first_static = Arc::clone(&first_frame.static_config);
    let second_static = Arc::clone(&second_frame.static_config);

    pretty_assertions::assert_eq!(first_static, second_static);
    pretty_assertions::assert_eq!(
        first_frame.projection_policy_revision,
        second_frame.projection_policy_revision
    );
}
