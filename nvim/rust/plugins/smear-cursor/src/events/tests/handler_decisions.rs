use super::super::handlers::select_core_event_source;
use super::super::handlers::should_request_observation_for_autocmd;
use super::super::ingress::AutocmdIngress;
use crate::core::runtime_reducer::EventSource;
use crate::core::runtime_reducer::MotionTarget;
use crate::core::state::SemanticEvent;
use crate::position::RenderPoint;
use crate::position::ScreenCell;
use crate::state::CursorShape;
use crate::state::RuntimeState;
use crate::state::TrackedCursor;
use crate::test_support::proptest::pure_config;
use proptest::prelude::*;

#[derive(Clone, Copy, Debug)]
enum RuntimeMotionPhase {
    Idle,
    Animating,
    Settling,
}

#[derive(Clone, Copy, Debug)]
enum LocationRelation {
    SameLocation,
    SameSurfaceDifferentLocation,
    DifferentSurface,
}

#[derive(Clone, Copy, Debug)]
enum MotionTargetCase {
    Unavailable,
    Same,
    Changed,
}

fn autocmd_ingress_strategy() -> BoxedStrategy<AutocmdIngress> {
    prop_oneof![
        Just(AutocmdIngress::CmdlineChanged),
        Just(AutocmdIngress::CursorMoved),
        Just(AutocmdIngress::CursorMovedInsert),
        Just(AutocmdIngress::ModeChanged),
        Just(AutocmdIngress::TextChanged),
        Just(AutocmdIngress::TextChangedInsert),
        Just(AutocmdIngress::WinEnter),
        Just(AutocmdIngress::WinScrolled),
        Just(AutocmdIngress::BufEnter),
        Just(AutocmdIngress::ColorScheme),
        Just(AutocmdIngress::Unknown),
    ]
    .boxed()
}

fn semantic_event_strategy() -> BoxedStrategy<SemanticEvent> {
    prop_oneof![
        Just(SemanticEvent::FrameCommitted),
        Just(SemanticEvent::ModeChanged),
        Just(SemanticEvent::CursorMovedWithoutTextMutation),
        Just(SemanticEvent::TextMutatedAtCursorContext),
        Just(SemanticEvent::ViewportOrWindowMoved),
    ]
    .boxed()
}

fn runtime_motion_phase_strategy() -> BoxedStrategy<RuntimeMotionPhase> {
    prop_oneof![
        Just(RuntimeMotionPhase::Idle),
        Just(RuntimeMotionPhase::Animating),
        Just(RuntimeMotionPhase::Settling),
    ]
    .boxed()
}

fn location_relation_strategy() -> BoxedStrategy<LocationRelation> {
    prop_oneof![
        Just(LocationRelation::SameLocation),
        Just(LocationRelation::SameSurfaceDifferentLocation),
        Just(LocationRelation::DifferentSurface),
    ]
    .boxed()
}

fn motion_target_strategy() -> BoxedStrategy<MotionTargetCase> {
    prop_oneof![
        Just(MotionTargetCase::Unavailable),
        Just(MotionTargetCase::Same),
        Just(MotionTargetCase::Changed),
    ]
    .boxed()
}

fn initialized_state() -> RuntimeState {
    let mut state = RuntimeState::default();
    state.initialize_cursor(
        RenderPoint {
            row: 10.0,
            col: 20.0,
        },
        CursorShape::block(),
        7,
        &TrackedCursor::fixture(1, 1, 1, 10),
    );
    state
}

fn state_with_phase(initialized: bool, phase: RuntimeMotionPhase) -> RuntimeState {
    let mut state = if initialized {
        initialized_state()
    } else {
        RuntimeState::default()
    };
    if initialized {
        match phase {
            RuntimeMotionPhase::Idle => {}
            RuntimeMotionPhase::Animating => {
                state.start_animation();
            }
            RuntimeMotionPhase::Settling => {
                state.begin_settling(
                    RenderPoint {
                        row: 10.0,
                        col: 20.0,
                    },
                    CursorShape::block(),
                    &TrackedCursor::fixture(1, 1, 1, 10),
                    0.0,
                );
            }
        }
    }
    state
}

fn tracked_cursor_for_relation(relation: LocationRelation) -> TrackedCursor {
    match relation {
        LocationRelation::SameLocation => TrackedCursor::fixture(1, 1, 1, 10),
        LocationRelation::SameSurfaceDifferentLocation => TrackedCursor::fixture(1, 1, 5, 15),
        LocationRelation::DifferentSurface => TrackedCursor::fixture(2, 3, 1, 10),
    }
}

fn motion_target_cell(row: i64, col: i64) -> ScreenCell {
    ScreenCell::new(row, col).expect("positive motion target")
}

fn expected_event_source(
    mode: &str,
    state: &RuntimeState,
    semantic_event: SemanticEvent,
    motion_target: MotionTarget,
    tracked_cursor: &TrackedCursor,
) -> EventSource {
    if mode == "c" {
        return EventSource::External;
    }
    if matches!(
        semantic_event,
        SemanticEvent::ModeChanged
            | SemanticEvent::TextMutatedAtCursorContext
            | SemanticEvent::ViewportOrWindowMoved
    ) {
        return EventSource::External;
    }
    let target = match motion_target {
        MotionTarget::Available(target_cell) => RenderPoint::from(target_cell),
        MotionTarget::Unavailable => match state.tracked_cursor_ref() {
            Some(location) if location == tracked_cursor => return EventSource::AnimationTick,
            Some(_) | None => return EventSource::External,
        },
    };
    if !state.is_initialized() {
        return EventSource::External;
    }
    let target_changed = state.target_position().distance_squared(target) > crate::types::EPSILON;
    if target_changed {
        let same_surface = state.tracked_cursor_ref().is_some_and(|location| {
            location.window_handle() == tracked_cursor.window_handle()
                && location.buffer_handle() == tracked_cursor.buffer_handle()
        });
        if (state.is_animating() || state.is_settling()) && same_surface {
            return EventSource::AnimationTick;
        }
        return EventSource::External;
    }

    match state.tracked_cursor_ref() {
        Some(location) if location == tracked_cursor => EventSource::AnimationTick,
        Some(_) | None => EventSource::External,
    }
}

proptest! {
    #![proptest_config(pure_config())]

    #[test]
    fn prop_autocmd_observation_policy_matches_supported_variants(
        ingress in autocmd_ingress_strategy()
    ) {
        let expected = matches!(
            ingress,
            AutocmdIngress::CursorMoved
                | AutocmdIngress::CursorMovedInsert
                | AutocmdIngress::WinEnter
                | AutocmdIngress::WinScrolled
                | AutocmdIngress::CmdlineChanged
                | AutocmdIngress::ModeChanged
                | AutocmdIngress::BufEnter
        );

        prop_assert_eq!(should_request_observation_for_autocmd(ingress), expected);
    }

    #[test]
    fn prop_select_core_event_source_matches_runtime_motion_context(
        initialized in any::<bool>(),
        phase in runtime_motion_phase_strategy(),
        semantic_event in semantic_event_strategy(),
        motion_target in motion_target_strategy(),
        location_relation in location_relation_strategy(),
        cmdline_mode in any::<bool>(),
    ) {
        let mode = if cmdline_mode { "c" } else { "n" };
        let state = state_with_phase(initialized, phase);
        let tracked_cursor = tracked_cursor_for_relation(location_relation);
        let motion_target = match motion_target {
            MotionTargetCase::Unavailable => MotionTarget::Unavailable,
            MotionTargetCase::Same => MotionTarget::Available(motion_target_cell(10, 20)),
            MotionTargetCase::Changed => MotionTarget::Available(motion_target_cell(14, 26)),
        };
        let expected = expected_event_source(
            mode,
            &state,
            semantic_event,
            motion_target,
            &tracked_cursor,
        );
        let actual = select_core_event_source(
            mode,
            &state,
            semantic_event,
            motion_target,
            &tracked_cursor,
        );

        prop_assert_eq!(actual, expected);
    }
}
