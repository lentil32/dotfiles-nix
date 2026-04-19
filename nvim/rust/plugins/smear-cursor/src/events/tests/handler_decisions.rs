use super::super::handlers::select_core_event_source;
use super::super::handlers::should_request_observation_for_autocmd;
use super::super::ingress::AutocmdIngress;
use crate::core::runtime_reducer::EventSource;
use crate::core::state::SemanticEvent;
use crate::state::CursorLocation;
use crate::state::CursorShape;
use crate::state::RuntimeState;
use crate::test_support::proptest::pure_config;
use crate::types::Point;
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
enum RequestedTarget {
    None,
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

fn requested_target_strategy() -> BoxedStrategy<RequestedTarget> {
    prop_oneof![
        Just(RequestedTarget::None),
        Just(RequestedTarget::Same),
        Just(RequestedTarget::Changed),
    ]
    .boxed()
}

fn initialized_state() -> RuntimeState {
    let mut state = RuntimeState::default();
    state.initialize_cursor(
        Point {
            row: 10.0,
            col: 20.0,
        },
        CursorShape::new(false, false),
        7,
        &CursorLocation::new(1, 1, 1, 10),
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
                    Point {
                        row: 10.0,
                        col: 20.0,
                    },
                    CursorShape::new(false, false),
                    &CursorLocation::new(1, 1, 1, 10),
                    0.0,
                );
            }
        }
    }
    state
}

fn cursor_location_for_relation(relation: LocationRelation) -> CursorLocation {
    match relation {
        LocationRelation::SameLocation => CursorLocation::new(1, 1, 1, 10),
        LocationRelation::SameSurfaceDifferentLocation => CursorLocation::new(1, 1, 5, 15),
        LocationRelation::DifferentSurface => CursorLocation::new(2, 3, 1, 10),
    }
}

fn expected_event_source(
    mode: &str,
    state: &RuntimeState,
    semantic_event: SemanticEvent,
    requested_target: Option<Point>,
    cursor_location: &CursorLocation,
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
    let Some(target) = requested_target else {
        return EventSource::AnimationTick;
    };
    if !state.is_initialized() {
        return EventSource::External;
    }
    let target_changed = state.target_position().distance_squared(target) > crate::types::EPSILON;
    if target_changed {
        let same_surface = state.tracked_location_ref().is_some_and(|location| {
            location.window_handle == cursor_location.window_handle
                && location.buffer_handle == cursor_location.buffer_handle
        });
        if (state.is_animating() || state.is_settling()) && same_surface {
            return EventSource::AnimationTick;
        }
        return EventSource::External;
    }

    match state.tracked_location_ref() {
        Some(location) if location == cursor_location => EventSource::AnimationTick,
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
        requested_target in requested_target_strategy(),
        location_relation in location_relation_strategy(),
        cmdline_mode in any::<bool>(),
    ) {
        let mode = if cmdline_mode { "c" } else { "n" };
        let state = state_with_phase(initialized, phase);
        let cursor_location = cursor_location_for_relation(location_relation);
        let requested_target = match requested_target {
            RequestedTarget::None => None,
            RequestedTarget::Same => Some(Point {
                row: 10.0,
                col: 20.0,
            }),
            RequestedTarget::Changed => Some(Point {
                row: 14.0,
                col: 26.0,
            }),
        };
        let expected = expected_event_source(
            mode,
            &state,
            semantic_event,
            requested_target,
            &cursor_location,
        );
        let actual = select_core_event_source(
            mode,
            &state,
            semantic_event,
            requested_target,
            &cursor_location,
        );

        prop_assert_eq!(actual, expected);
    }
}
