use super::*;
use pretty_assertions::assert_eq;

#[test]
fn same_cell_external_retarget_updates_target_position_without_bumping_the_retarget_epoch() {
    let (mut state, kickoff) = animating_runtime_after_kickoff(|state| {
        state.config.delay_event_to_smear = 0.0;
    });
    let baseline_epoch = draw_frame(&kickoff)
        .map(|frame| frame.retarget_epoch)
        .expect("kickoff should draw");

    let retarget = reduce_cursor_event(
        &mut state,
        "n",
        event_at(5.4, 12.4, 124.0),
        EventSource::External,
    );
    let frame =
        draw_frame(&retarget).expect("same-cell external retarget should still draw immediately");

    assert!(matches!(render_action(&retarget), RenderAction::Draw(_)));
    assert!(retarget.should_schedule_next_animation());
    assert_eq!(
        render_cleanup_action(&retarget),
        RenderCleanupAction::Invalidate
    );
    assert!(state.is_animating());
    assert_eq!(frame.retarget_epoch, baseline_epoch);
    assert_eq!(state.retarget_epoch(), baseline_epoch);
    assert_eq!(
        frame.target,
        RenderPoint {
            row: 5.4,
            col: 12.4,
        }
    );
    assert_eq!(
        state.target_position(),
        RenderPoint {
            row: 5.4,
            col: 12.4,
        }
    );
}

#[test]
fn changed_cell_external_retarget_draws_immediately_and_bumps_the_retarget_epoch_once() {
    let (mut state, kickoff) = animating_runtime_after_kickoff(|state| {
        state.config.delay_event_to_smear = 0.0;
    });
    let kickoff_frame = draw_frame(&kickoff).expect("kickoff should draw");
    let baseline_epoch = kickoff_frame.retarget_epoch;
    let kickoff_stroke_id = kickoff_frame.trail_stroke_id;

    let retarget = reduce_cursor_event(
        &mut state,
        "n",
        event_at(5.0, 24.0, 124.0),
        EventSource::External,
    );
    let frame =
        draw_frame(&retarget).expect("changed-cell external retarget should draw immediately");

    assert!(matches!(render_action(&retarget), RenderAction::Draw(_)));
    assert!(retarget.should_schedule_next_animation());
    assert_eq!(
        render_cleanup_action(&retarget),
        RenderCleanupAction::Invalidate
    );
    assert!(state.is_animating());
    assert_eq!(frame.trail_stroke_id, kickoff_stroke_id);
    assert_eq!(state.trail_stroke_id(), kickoff_stroke_id);
    assert_eq!(frame.retarget_epoch, baseline_epoch.wrapping_add(1));
    assert_eq!(state.retarget_epoch(), baseline_epoch.wrapping_add(1));
    assert_eq!(
        frame.target,
        RenderPoint {
            row: 5.0,
            col: 24.0,
        }
    );
    assert_eq!(
        state.target_position(),
        RenderPoint {
            row: 5.0,
            col: 24.0,
        }
    );
}
