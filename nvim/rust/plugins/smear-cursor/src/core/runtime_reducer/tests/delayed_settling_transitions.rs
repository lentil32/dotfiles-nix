use super::*;
use crate::test_support::proptest::stateful_config;
use pretty_assertions::assert_eq;
use proptest::collection::vec;

#[derive(Clone, Copy, Debug)]
enum RefreshEvent {
    External { gap_ms: u16 },
    AnimationTick { gap_ms: u16 },
}

impl RefreshEvent {
    fn gap_ms(self) -> u16 {
        match self {
            Self::External { gap_ms } | Self::AnimationTick { gap_ms } => gap_ms,
        }
    }

    fn source(self) -> EventSource {
        match self {
            Self::External { .. } => EventSource::External,
            Self::AnimationTick { .. } => EventSource::AnimationTick,
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum TerminalEvent {
    DeadlineTick,
    ReturnToCurrent { gap_ms: u16 },
}

#[derive(Clone, Debug)]
struct DelayedSettlingCase {
    delay_ms: u16,
    target_col: u16,
    refresh_events: Vec<RefreshEvent>,
    terminal: TerminalEvent,
}

fn delayed_settling_case() -> impl Strategy<Value = DelayedSettlingCase> {
    (2_u16..=80, 16_u16..=96).prop_flat_map(|(delay_ms, target_col)| {
        (
            Just(delay_ms),
            Just(target_col),
            vec((any::<bool>(), 1_u16..delay_ms), 0..=8),
            proptest::prop_oneof![
                Just(TerminalEvent::DeadlineTick),
                (1_u16..delay_ms).prop_map(|gap_ms| TerminalEvent::ReturnToCurrent { gap_ms }),
            ],
        )
            .prop_map(|(delay_ms, target_col, refresh_events, terminal)| {
                DelayedSettlingCase {
                    delay_ms,
                    target_col,
                    refresh_events: refresh_events
                        .into_iter()
                        .map(|(is_external, gap_ms)| {
                            if is_external {
                                RefreshEvent::External { gap_ms }
                            } else {
                                RefreshEvent::AnimationTick { gap_ms }
                            }
                        })
                        .collect(),
                    terminal,
                }
            })
    })
}

fn delayed_settling_scenario(delay_ms: u16, target_col: u16) -> (RuntimeState, CursorTransition) {
    let (mut state, _) = initialized_runtime("n", |state| {
        state.config.delay_event_to_smear = f64::from(delay_ms);
    });
    let kickoff = reduce_cursor_event(
        &mut state,
        "n",
        event_at(5.0, f64::from(target_col), 116.0),
        EventSource::External,
    );
    (state, kickoff)
}

#[test]
fn returning_to_the_current_cursor_leaves_the_runtime_idle_and_cursor_visible() {
    let (mut state, _) = delayed_retarget_scenario();
    let returned = reduce_cursor_event(
        &mut state,
        "n",
        event_at(5.0, 6.0, 120.0),
        EventSource::External,
    );

    assert!(matches!(render_action(&returned), RenderAction::Noop));
    assert!(!state.is_animating());
    assert!(!state.is_settling());
    assert_eq!(state.settling_window(), None);
    assert_eq!(
        render_side_effects(&returned).cursor_visibility,
        CursorVisibilityEffect::Show
    );
}

#[test]
fn retargeting_while_settling_on_a_tick_after_the_previous_deadline_refreshes_instead_of_promoting()
{
    let (mut state, kickoff) = delayed_settling_scenario(20, 16);

    assert!(matches!(render_action(&kickoff), RenderAction::Noop));
    assert!(state.is_settling());
    assert!(!state.is_animating());

    let retarget = reduce_cursor_event(
        &mut state,
        "n",
        event_at(5.0, 24.0, 140.0),
        EventSource::AnimationTick,
    );

    assert!(matches!(render_action(&retarget), RenderAction::Noop));
    assert!(retarget.should_schedule_next_animation());
    assert_eq!(retarget.next_animation_at_ms(), Some(160));
    assert!(state.is_settling());
    assert!(!state.is_animating());
    assert_eq!(
        state.target_position(),
        RenderPoint {
            row: 5.0,
            col: 24.0
        }
    );
    let settling_window = state
        .settling_window()
        .copied()
        .expect("retargeted settle should still carry a timing window");
    assert_eq!(settling_window.stable_since_ms, 140.0);
    assert_eq!(settling_window.settle_deadline_ms, 160.0);
}

#[test]
fn shape_change_while_settling_resets_the_deadline_before_promotion() {
    let (mut state, kickoff) = delayed_settling_scenario(20, 16);

    assert!(matches!(render_action(&kickoff), RenderAction::Noop));
    assert!(state.is_settling());
    assert!(!state.is_animating());
    let baseline_epoch = state.retarget_epoch();

    let reshaped = reduce_cursor_event(
        &mut state,
        "i",
        event_at(5.0, 16.0, 136.0),
        EventSource::External,
    );

    assert!(matches!(render_action(&reshaped), RenderAction::Noop));
    assert!(reshaped.should_schedule_next_animation());
    assert_eq!(reshaped.next_animation_at_ms(), Some(156));
    assert!(state.is_settling());
    assert!(!state.is_animating());
    assert_eq!(
        state.target_position(),
        RenderPoint {
            row: 5.0,
            col: 16.0
        }
    );
    assert_eq!(state.target_shape(), CursorShape::vertical_bar());
    assert_eq!(state.retarget_epoch(), baseline_epoch.wrapping_add(1));
    let settling_window = state
        .settling_window()
        .copied()
        .expect("shape retarget should keep the settling window active");
    assert_eq!(settling_window.stable_since_ms, 136.0);
    assert_eq!(settling_window.settle_deadline_ms, 156.0);

    let ready_tick = reduce_cursor_event(
        &mut state,
        "i",
        event_at(5.0, 16.0, 156.0),
        EventSource::AnimationTick,
    );

    assert!(matches!(render_action(&ready_tick), RenderAction::Draw(_)));
    assert!(state.is_animating());
    assert!(!state.is_settling());
    assert_eq!(state.settling_window(), None);
}

#[test]
fn same_key_tracking_update_while_settling_preserves_stable_since() {
    let (mut state, kickoff) = delayed_settling_scenario(20, 16);

    assert!(matches!(render_action(&kickoff), RenderAction::Noop));
    assert!(state.is_settling());
    let baseline_epoch = state.retarget_epoch();

    let translated_tracking = TrackedCursor::fixture(10, 20, 3, 1);
    let refresh = reduce_cursor_event(
        &mut state,
        "n",
        CursorEventContext {
            row: 5.0,
            col: 16.0,
            now_ms: 130.0,
            seed: 7,
            tracked_cursor: translated_tracking.clone(),
            scroll_shift: None,
            semantic_event: SemanticEvent::FrameCommitted,
        },
        EventSource::External,
    );

    assert!(matches!(render_action(&refresh), RenderAction::Noop));
    assert!(refresh.should_schedule_next_animation());
    assert_eq!(refresh.next_animation_at_ms(), Some(150));
    assert!(state.is_settling());
    assert_eq!(
        state.target_position(),
        RenderPoint {
            row: 5.0,
            col: 16.0
        }
    );
    assert_eq!(state.retarget_epoch(), baseline_epoch);
    assert_eq!(state.tracked_cursor_ref(), Some(&translated_tracking));
    let settling_window = state
        .settling_window()
        .copied()
        .expect("same-key tracking refresh should preserve the settling window");
    assert_eq!(settling_window.stable_since_ms, 116.0);
    assert_eq!(settling_window.settle_deadline_ms, 150.0);
}

proptest! {
    #![proptest_config(stateful_config())]

    #[test]
    fn prop_delayed_settling_sequences_refresh_deadlines_until_animation_or_cancel(
        case in delayed_settling_case(),
    ) {
        let target_position = RenderPoint {
            row: 5.0,
            col: f64::from(case.target_col),
        };
        let tracked_cursor = TrackedCursor::fixture(10, 20, 1, 1);
        let delay_ms = f64::from(case.delay_ms);
        let kickoff_now_ms = 116.0_f64;
        let (mut state, kickoff) = delayed_settling_scenario(case.delay_ms, case.target_col);

        prop_assert!(matches!(render_action(&kickoff), RenderAction::Noop));
        prop_assert!(kickoff.should_schedule_next_animation());
        prop_assert_eq!(kickoff.next_animation_at_ms(), Some(u64::from(116_u16 + case.delay_ms)));
        prop_assert_eq!(
            render_side_effects(&kickoff).cursor_visibility,
            CursorVisibilityEffect::Show
        );
        prop_assert_eq!(render_cleanup_action(&kickoff), RenderCleanupAction::Invalidate);
        prop_assert!(state.is_settling());
        prop_assert!(!state.is_animating());
        let settling_window = state
            .settling_window()
            .copied()
            .expect("kickoff should enter settling with a timing window");
        prop_assert_eq!(state.target_position(), target_position);
        prop_assert_eq!(state.tracked_cursor_ref(), Some(&tracked_cursor));
        prop_assert_eq!(settling_window.stable_since_ms, kickoff_now_ms);
        prop_assert_eq!(settling_window.settle_deadline_ms, kickoff_now_ms + delay_ms);

        let mut now_ms = kickoff_now_ms;
        let mut expected_deadline_ms = kickoff_now_ms + delay_ms;
        for refresh_event in case.refresh_events {
            now_ms += f64::from(refresh_event.gap_ms());
            prop_assert!(
                now_ms < expected_deadline_ms,
                "refresh event missed the active settle deadline"
            );

            let refresh = reduce_cursor_event(
                &mut state,
                "n",
                event_at(target_position.row, target_position.col, now_ms),
                refresh_event.source(),
            );
            expected_deadline_ms = now_ms + delay_ms;

            prop_assert!(matches!(render_action(&refresh), RenderAction::Noop));
            prop_assert!(refresh.should_schedule_next_animation());
            prop_assert_eq!(
                refresh.next_animation_at_ms(),
                Some(expected_deadline_ms as u64)
            );
            prop_assert_eq!(
                render_side_effects(&refresh).cursor_visibility,
                CursorVisibilityEffect::Show
            );
            prop_assert_eq!(
                render_cleanup_action(&refresh),
                RenderCleanupAction::Invalidate
            );
            prop_assert!(state.is_settling());
            prop_assert!(!state.is_animating());

            let settling_window = state
                .settling_window()
                .copied()
                .expect("refresh events should preserve the settling window");
            prop_assert_eq!(state.target_position(), target_position);
            prop_assert_eq!(state.tracked_cursor_ref(), Some(&tracked_cursor));
            prop_assert_eq!(settling_window.stable_since_ms, kickoff_now_ms);
            prop_assert_eq!(settling_window.settle_deadline_ms, expected_deadline_ms);
        }

        match case.terminal {
            TerminalEvent::DeadlineTick => {
                now_ms = expected_deadline_ms;
                let ready_tick = reduce_cursor_event(
                    &mut state,
                    "n",
                    event_at(target_position.row, target_position.col, now_ms),
                    EventSource::AnimationTick,
                );

                prop_assert!(matches!(render_action(&ready_tick), RenderAction::Draw(_)));
                prop_assert!(ready_tick.should_schedule_next_animation());
                prop_assert!(ready_tick.next_animation_at_ms().is_some());
                prop_assert_eq!(
                    render_side_effects(&ready_tick).cursor_visibility,
                    CursorVisibilityEffect::Hide
                );
                prop_assert_eq!(
                    render_cleanup_action(&ready_tick),
                    RenderCleanupAction::Invalidate
                );
                prop_assert!(state.is_animating());
                prop_assert!(!state.is_settling());
                prop_assert_eq!(state.settling_window(), None);
            }
            TerminalEvent::ReturnToCurrent { gap_ms } => {
                now_ms += f64::from(gap_ms);
                prop_assert!(
                    now_ms < expected_deadline_ms,
                    "return-to-current must happen before the active settle deadline"
                );
                let returned = reduce_cursor_event(
                    &mut state,
                    "n",
                    event_at(5.0, 6.0, now_ms),
                    EventSource::External,
                );

                prop_assert!(matches!(render_action(&returned), RenderAction::Noop));
                prop_assert!(!returned.should_schedule_next_animation());
                prop_assert_eq!(returned.next_animation_at_ms(), None);
                prop_assert_eq!(
                    render_side_effects(&returned).cursor_visibility,
                    CursorVisibilityEffect::Show
                );
                prop_assert_eq!(
                    render_cleanup_action(&returned),
                    RenderCleanupAction::Schedule
                );
                prop_assert!(!state.is_animating());
                prop_assert!(!state.is_settling());
                prop_assert_eq!(state.settling_window(), None);
            }
        }
    }
}
