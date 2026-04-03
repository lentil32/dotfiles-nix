use super::*;
use pretty_assertions::assert_eq;

fn jump_bridge_origin(frame: &RenderFrame) -> Point {
    let first_sample = frame
        .step_samples
        .first()
        .expect("jump bridge should start with an origin sample");
    crate::types::current_visual_cursor_anchor(
        &first_sample.corners,
        &frame.target_corners,
        frame.target,
    )
}

#[derive(Clone, Copy, Debug)]
enum InsertModeJumpBridgeCase {
    ImmediateSnap,
    ForcedJump,
}

impl InsertModeJumpBridgeCase {
    fn configure(self, state: &mut RuntimeState) {
        match self {
            Self::ImmediateSnap => {
                state.config.smear_insert_mode = true;
                state.config.animate_in_insert_mode = false;
            }
            Self::ForcedJump => {
                state.config.smear_insert_mode = false;
                state.config.animate_in_insert_mode = true;
            }
        }
    }

    fn frame_context(self) -> &'static str {
        match self {
            Self::ImmediateSnap => "insert immediate snap",
            Self::ForcedJump => "mode-forced insert jumps",
        }
    }
}

fn insert_mode_jump_scroll_shift() -> ScrollShift {
    ScrollShift {
        row_shift: 2.0,
        col_shift: 3.0,
        min_row: 1.0,
        max_row: 60.0,
    }
}

fn translated_by_scroll_shift(point: Point, scroll_shift: ScrollShift) -> Point {
    Point {
        row: point.row - scroll_shift.row_shift,
        col: point.col - scroll_shift.col_shift,
    }
}

fn render_jump_bridge_snapshot(transition: &CursorTransition, state: &RuntimeState) -> String {
    let frame = draw_frame(transition)
        .expect("jump-bridge snapshot tests require a discontinuous jump frame");
    let samples = frame
        .step_samples
        .iter()
        .map(StepSampleSummary::from_step_sample)
        .map(|sample| sample.render())
        .collect::<Vec<_>>()
        .join(";");

    format!(
        "motion={}\norigin={}\nframe_target={}\nstate_center={}\nstate_target={}\nsamples=[{}]",
        render_motion_class(transition.motion_class),
        PointSummary::from_point(jump_bridge_origin(frame)).render(),
        PointSummary::from_point(frame.target).render(),
        PointSummary::from_point(trajectory_center(state)).render(),
        PointSummary::from_point(state.target_position()).render(),
        samples
    )
}

fn assert_jump_bridge_origin_matches(
    transition: &CursorTransition,
    expected_origin: Point,
    unexpected_origin: Point,
    frame_context: &str,
) {
    let frame = draw_frame(transition)
        .unwrap_or_else(|| panic!("{frame_context} should emit a discontinuous jump bridge frame"));
    let bridge_origin = jump_bridge_origin(frame);

    assert_eq!(
        PointSummary::from_point(bridge_origin),
        PointSummary::from_point(expected_origin)
    );
    assert_ne!(
        PointSummary::from_point(bridge_origin),
        PointSummary::from_point(unexpected_origin)
    );
}

fn assert_insert_mode_jump_bridge_starts_from_live_head(case: InsertModeJumpBridgeCase) {
    let (_, live_head, stale_target, transition) = mid_animation_insert_mode_jump(|state| {
        case.configure(state);
    });

    assert_jump_bridge_origin_matches(&transition, live_head, stale_target, case.frame_context());
}

fn assert_insert_mode_jump_bridge_starts_from_translated_live_head(case: InsertModeJumpBridgeCase) {
    let (_, translated_live_head, live_head_before_scroll, transition) =
        mid_animation_insert_mode_jump_with_scroll(|state| {
            case.configure(state);
        });

    assert_jump_bridge_origin_matches(
        &transition,
        translated_live_head,
        live_head_before_scroll,
        case.frame_context(),
    );
}

fn mid_animation_insert_mode_jump(
    configure: impl FnOnce(&mut RuntimeState),
) -> (RuntimeState, Point, Point, CursorTransition) {
    let (mut state, _) = animating_runtime_after_kickoff(|state| {
        state.config.delay_event_to_smear = 0.0;
        configure(state);
    });
    let _ = reduce_cursor_event(
        &mut state,
        "n",
        event_at(5.0, 12.0, 124.0),
        EventSource::AnimationTick,
    );
    let live_head = state.current_visual_cursor_anchor();
    let stale_target = state.target_position();
    assert_ne!(
        PointSummary::from_point(live_head),
        PointSummary::from_point(stale_target),
        "precondition failed: expected the live head to move away from the stale target before the forced jump"
    );
    let transition = reduce_cursor_event(
        &mut state,
        "i",
        event_with_location(5.0, 20.0, 132.0, 17, 10, 20),
        EventSource::External,
    );
    (state, live_head, stale_target, transition)
}

fn mid_animation_insert_mode_jump_with_scroll(
    configure: impl FnOnce(&mut RuntimeState),
) -> (RuntimeState, Point, Point, CursorTransition) {
    let (mut state, _) = animating_runtime_after_kickoff(|state| {
        state.config.delay_event_to_smear = 0.0;
        configure(state);
    });
    let _ = reduce_cursor_event(
        &mut state,
        "n",
        event_at(5.0, 12.0, 124.0),
        EventSource::AnimationTick,
    );
    let live_head_before_scroll = state.current_visual_cursor_anchor();
    let translated_live_head =
        translated_by_scroll_shift(live_head_before_scroll, insert_mode_jump_scroll_shift());
    let transition = reduce_cursor_event(
        &mut state,
        "i",
        CursorEventContext {
            row: 3.0,
            col: 20.0,
            now_ms: 132.0,
            seed: 17,
            cursor_location: CursorLocation::new(10, 20, 3, 2).with_viewport_columns(3, 0),
            scroll_shift: Some(insert_mode_jump_scroll_shift()),
            semantic_event: SemanticEvent::ViewportOrWindowMoved,
        },
        EventSource::External,
    );
    (
        state,
        translated_live_head,
        live_head_before_scroll,
        transition,
    )
}

fn insert_immediate_snap() -> (RuntimeState, CursorTransition) {
    let (mut state, _) = initialized_runtime("n", |state| {
        state.config.delay_event_to_smear = 0.0;
        state.config.smear_insert_mode = true;
        state.config.animate_in_insert_mode = false;
    });
    let transition = reduce_cursor_event(
        &mut state,
        "i",
        event_with_location(5.0, 20.0, 116.0, 17, 10, 20),
        EventSource::External,
    );
    (state, transition)
}

fn cmdline_boundary_transition() -> (RuntimeState, CursorTransition) {
    let (mut state, _) = initialized_runtime("n", |state| {
        state.config.delay_event_to_smear = 0.0;
        state.config.smear_to_cmd = true;
        state.config.animate_command_line = false;
    });
    let boundary = reduce_cursor_event(
        &mut state,
        "c",
        event_with_location(5.0, 12.0, 116.0, 18, 10, 20),
        EventSource::External,
    );
    (state, boundary)
}

#[test]
fn cmdline_external_events_progress_after_a_settle_tick() {
    let mut state = RuntimeState::default();
    state.config.delay_event_to_smear = 0.0;
    state.config.smear_to_cmd = true;

    let _ = reduce_cursor_event(&mut state, "c", event(5.0, 6.0), EventSource::External);
    let _ = reduce_cursor_event(
        &mut state,
        "c",
        event_with_location(5.0, 12.0, 116.0, 11, 10, 20),
        EventSource::External,
    );
    let _ = reduce_cursor_event(
        &mut state,
        "c",
        event_with_location(5.0, 12.0, 118.0, 11, 10, 20),
        EventSource::AnimationTick,
    );
    let effects = reduce_cursor_event(
        &mut state,
        "c",
        event_with_location(5.0, 16.0, 132.0, 12, 10, 20),
        EventSource::External,
    );

    assert!(matches!(render_action(&effects), RenderAction::Draw(_)));
    assert!(state.is_animating());
}

fn text_mutation_snap() -> (RuntimeState, CursorTransition) {
    let (mut state, _) = animating_runtime_after_kickoff(|state| {
        state.config.smear_insert_mode = true;
        state.config.animate_in_insert_mode = true;
    });
    let transition = reduce_cursor_event(
        &mut state,
        "i",
        text_mutation_event(5.0, 20.0, 132.0),
        EventSource::External,
    );
    (state, transition)
}

fn animation_tick_retarget() -> (RuntimeState, u64, CursorTransition) {
    let (mut state, _) = animating_runtime_after_kickoff(|state| {
        state.config.delay_event_to_smear = 0.0;
    });
    let before_retarget = reduce_cursor_event(
        &mut state,
        "n",
        event_with_location(5.0, 12.0, 124.0, 11, 10, 20),
        EventSource::AnimationTick,
    );
    let before_retarget_epoch = draw_frame(&before_retarget)
        .map(|frame| frame.retarget_epoch)
        .expect("expected pre-retarget animation tick to draw");
    let retarget = reduce_cursor_event(
        &mut state,
        "n",
        CursorEventContext {
            row: 12.0,
            col: 24.0,
            now_ms: 132.0,
            seed: 12,
            cursor_location: CursorLocation::new(10, 20, 1, 2),
            scroll_shift: None,
            semantic_event: SemanticEvent::FrameCommitted,
        },
        EventSource::AnimationTick,
    );
    (state, before_retarget_epoch, retarget)
}

#[test]
fn insert_mode_immediate_snap_emits_a_discontinuous_jump_bridge_frame_and_updates_the_target_without_disabling_insert_smear()
 {
    let (state, transition) = insert_immediate_snap();
    let frame = draw_frame(&transition)
        .expect("insert immediate mode should still emit a jump bridge frame");

    assert_eq!(transition.motion_class, MotionClass::DiscontinuousJump);
    assert!(transition.should_schedule_next_animation);
    assert!(state.is_draining());
    assert!(frame.step_samples.len() >= 3);
    assert_eq!(
        state.target_position(),
        Point {
            row: 5.0,
            col: 20.0
        }
    );
}

#[test]
fn insert_mode_motion_still_animates_without_text_mutation() {
    let (mut state, _) = initialized_runtime("n", |state| {
        state.config.delay_event_to_smear = 0.0;
        state.config.smear_insert_mode = true;
        state.config.animate_in_insert_mode = true;
    });
    let transition = reduce_cursor_event(
        &mut state,
        "i",
        event_with_location(5.0, 20.0, 116.0, 17, 10, 20),
        EventSource::External,
    );

    assert!(matches!(render_action(&transition), RenderAction::Draw(_)));
    assert!(state.is_animating());
}

#[test]
fn text_mutation_snap_clears_existing_smear_instead_of_animating() {
    let (state, transition) = text_mutation_snap();

    assert!(matches!(render_action(&transition), RenderAction::ClearAll));
    assert!(!transition.should_schedule_next_animation);
    assert!(!state.is_animating());
    assert_eq!(
        state.target_position(),
        Point {
            row: 5.0,
            col: 20.0,
        }
    );
}

#[test]
fn cmdline_boundary_snap_emits_an_immediate_jump_bridge_frame_and_intra_cmdline_motion_continues_animating_after_the_boundary_snap()
 {
    let (mut state, boundary) = cmdline_boundary_transition();
    let boundary_frame = draw_frame(&boundary)
        .expect("cmdline boundary snap should emit an immediate jump bridge frame");

    assert_eq!(boundary.motion_class, MotionClass::DiscontinuousJump);
    assert!(boundary.should_schedule_next_animation);
    assert!(state.is_draining());
    assert!(boundary_frame.step_samples.len() >= 3);

    let within_cmdline = reduce_cursor_event(
        &mut state,
        "c",
        event_with_location(5.0, 18.0, 132.0, 19, 10, 20),
        EventSource::External,
    );

    assert!(
        matches!(render_action(&within_cmdline), RenderAction::Draw(_)),
        "movement while already in cmdline should keep animating"
    );
    assert!(state.is_animating());
}

#[test]
fn animation_tick_retargets_advance_the_retarget_epoch_before_the_next_draw() {
    let (_, before_retarget_epoch, effects) = animation_tick_retarget();
    let after_retarget_epoch = draw_frame(&effects)
        .map(|frame| frame.retarget_epoch)
        .expect("expected retarget animation tick to draw");

    assert!(
        after_retarget_epoch > before_retarget_epoch,
        "retarget epoch should advance so draw acknowledgement cache does not suppress first retarget frame"
    );
}

#[test]
fn animation_tick_retargets_update_target_and_location_while_keeping_frames_scheduled() {
    let (state, _, effects) = animation_tick_retarget();

    assert!(matches!(render_action(&effects), RenderAction::Draw(_)));
    assert!(effects.should_schedule_next_animation);
    assert!(state.is_animating() || state.is_draining());
    assert_eq!(
        state.target_position(),
        Point {
            row: 12.0,
            col: 24.0
        }
    );
    assert_eq!(
        state.tracked_location(),
        Some(CursorLocation::new(10, 20, 1, 2))
    );
}

#[test]
fn insert_mode_immediate_snap_jump_bridge_starts_from_the_live_head() {
    assert_insert_mode_jump_bridge_starts_from_live_head(InsertModeJumpBridgeCase::ImmediateSnap);
}

#[test]
fn insert_mode_forced_jump_bridge_starts_from_the_live_head() {
    assert_insert_mode_jump_bridge_starts_from_live_head(InsertModeJumpBridgeCase::ForcedJump);
}

#[test]
fn insert_mode_immediate_snap_jump_bridge_starts_from_the_translated_live_head_after_scroll() {
    assert_insert_mode_jump_bridge_starts_from_translated_live_head(
        InsertModeJumpBridgeCase::ImmediateSnap,
    );
}

#[test]
fn insert_mode_forced_jump_bridge_starts_from_the_translated_live_head_after_scroll() {
    assert_insert_mode_jump_bridge_starts_from_translated_live_head(
        InsertModeJumpBridgeCase::ForcedJump,
    );
}

#[test]
fn insert_mode_immediate_snap_with_scroll_renders_the_translated_jump_bridge() {
    let (state, _, _, transition) = mid_animation_insert_mode_jump_with_scroll(|state| {
        state.config.smear_insert_mode = true;
        state.config.animate_in_insert_mode = false;
    });

    insta::assert_snapshot!(render_jump_bridge_snapshot(&transition, &state));
}
