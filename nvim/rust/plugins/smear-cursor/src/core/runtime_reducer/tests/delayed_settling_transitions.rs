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
    assert_eq!(state.pending_target(), None);
    assert_eq!(
        render_side_effects(&returned).cursor_visibility,
        CursorVisibilityEffect::Show
    );
}

proptest! {
    #![proptest_config(stateful_config())]

    #[test]
    fn prop_delayed_settling_sequences_refresh_deadlines_until_animation_or_cancel(
        case in delayed_settling_case(),
    ) {
        let target_position = Point {
            row: 5.0,
            col: f64::from(case.target_col),
        };
        let tracked_location = CursorLocation::new(10, 20, 1, 1);
        let delay_ms = f64::from(case.delay_ms);
        let kickoff_now_ms = 116.0_f64;
        let (mut state, kickoff) = delayed_settling_scenario(case.delay_ms, case.target_col);

        prop_assert!(matches!(render_action(&kickoff), RenderAction::Noop));
        prop_assert!(kickoff.should_schedule_next_animation);
        prop_assert_eq!(kickoff.next_animation_at_ms, Some(u64::from(116_u16 + case.delay_ms)));
        prop_assert_eq!(
            render_side_effects(&kickoff).cursor_visibility,
            CursorVisibilityEffect::Show
        );
        prop_assert_eq!(render_cleanup_action(&kickoff), RenderCleanupAction::Invalidate);
        prop_assert!(state.is_settling());
        prop_assert!(!state.is_animating());
        {
            let pending = state
                .pending_target()
                .expect("kickoff should enter settling with a pending target");
            prop_assert_eq!(pending.position, target_position);
            prop_assert_eq!(&pending.cursor_location, &tracked_location);
            prop_assert_eq!(pending.stable_since_ms, kickoff_now_ms);
            prop_assert_eq!(pending.settle_deadline_ms, kickoff_now_ms + delay_ms);
        }

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
            prop_assert!(refresh.should_schedule_next_animation);
            prop_assert_eq!(
                refresh.next_animation_at_ms,
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

            let pending = state
                .pending_target()
                .expect("refresh events should preserve the settling target");
            prop_assert_eq!(pending.position, target_position);
            prop_assert_eq!(&pending.cursor_location, &tracked_location);
            prop_assert_eq!(pending.stable_since_ms, kickoff_now_ms);
            prop_assert_eq!(pending.settle_deadline_ms, expected_deadline_ms);
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
                prop_assert!(ready_tick.should_schedule_next_animation);
                prop_assert!(ready_tick.next_animation_at_ms.is_some());
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
                prop_assert_eq!(state.pending_target(), None);
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
                prop_assert!(!returned.should_schedule_next_animation);
                prop_assert_eq!(returned.next_animation_at_ms, None);
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
                prop_assert_eq!(state.pending_target(), None);
            }
        }
    }
}
