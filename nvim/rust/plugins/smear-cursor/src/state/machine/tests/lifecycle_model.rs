use super::CursorShape;
use super::RuntimeState;
use super::RuntimeTargetRetargetKey;
use super::StrokeId;
use super::TrackedCursor;
use super::cursor_shape_strategy;
use super::finite_point;
use super::replace_target_preserving_tracking;
use super::replace_target_with_tracking;
use super::tracked_cursor_strategy;
use crate::position::RenderPoint;
use proptest::prelude::*;

#[derive(Clone, Debug)]
pub(super) enum LifecycleSequenceOperation {
    MarkInitialized,
    StartAnimation,
    StopAnimation,
    ClearInitialization,
    InitializeCursor {
        position: RenderPoint,
        shape: CursorShape,
        seed: u32,
        location: TrackedCursor,
    },
    SetTarget {
        position: RenderPoint,
        shape: CursorShape,
    },
    JumpPreservingMotion {
        position: RenderPoint,
        shape: CursorShape,
        location: TrackedCursor,
    },
    JumpAndStopAnimation {
        position: RenderPoint,
        shape: CursorShape,
        location: TrackedCursor,
    },
    SyncToCurrentCursor {
        position: RenderPoint,
        shape: CursorShape,
        location: TrackedCursor,
    },
    UpdateTracking {
        location: TrackedCursor,
    },
}

pub(super) fn lifecycle_sequence_operation_strategy() -> BoxedStrategy<LifecycleSequenceOperation> {
    prop_oneof![
        Just(LifecycleSequenceOperation::MarkInitialized),
        Just(LifecycleSequenceOperation::StartAnimation),
        Just(LifecycleSequenceOperation::StopAnimation),
        Just(LifecycleSequenceOperation::ClearInitialization),
        (
            finite_point(),
            cursor_shape_strategy(),
            any::<u32>(),
            tracked_cursor_strategy()
        )
            .prop_map(|(position, shape, seed, location)| {
                LifecycleSequenceOperation::InitializeCursor {
                    position,
                    shape,
                    seed,
                    location,
                }
            }),
        (finite_point(), cursor_shape_strategy()).prop_map(|(position, shape)| {
            LifecycleSequenceOperation::SetTarget { position, shape }
        }),
        (
            finite_point(),
            cursor_shape_strategy(),
            tracked_cursor_strategy()
        )
            .prop_map(|(position, shape, location)| {
                LifecycleSequenceOperation::JumpPreservingMotion {
                    position,
                    shape,
                    location,
                }
            }),
        (
            finite_point(),
            cursor_shape_strategy(),
            tracked_cursor_strategy()
        )
            .prop_map(|(position, shape, location)| {
                LifecycleSequenceOperation::JumpAndStopAnimation {
                    position,
                    shape,
                    location,
                }
            }),
        (
            finite_point(),
            cursor_shape_strategy(),
            tracked_cursor_strategy()
        )
            .prop_map(|(position, shape, location)| {
                LifecycleSequenceOperation::SyncToCurrentCursor {
                    position,
                    shape,
                    location,
                }
            }),
        tracked_cursor_strategy()
            .prop_map(|location| LifecycleSequenceOperation::UpdateTracking { location }),
    ]
    .boxed()
}

pub(super) fn expected_lifecycle_flags(
    state: &RuntimeState,
    operation: &LifecycleSequenceOperation,
) -> (bool, bool) {
    let was_initialized = state.is_initialized();
    let was_animating = state.is_animating();

    let expected_initialized = match operation {
        LifecycleSequenceOperation::ClearInitialization => false,
        LifecycleSequenceOperation::MarkInitialized
        | LifecycleSequenceOperation::StartAnimation
        | LifecycleSequenceOperation::InitializeCursor { .. }
        | LifecycleSequenceOperation::JumpPreservingMotion { .. }
        | LifecycleSequenceOperation::SyncToCurrentCursor { .. } => true,
        LifecycleSequenceOperation::JumpAndStopAnimation { .. }
        | LifecycleSequenceOperation::StopAnimation
        | LifecycleSequenceOperation::SetTarget { .. }
        | LifecycleSequenceOperation::UpdateTracking { .. } => was_initialized,
    };
    let expected_animating = match operation {
        LifecycleSequenceOperation::StartAnimation => true,
        LifecycleSequenceOperation::ClearInitialization
        | LifecycleSequenceOperation::InitializeCursor { .. }
        | LifecycleSequenceOperation::JumpAndStopAnimation { .. }
        | LifecycleSequenceOperation::SyncToCurrentCursor { .. }
        | LifecycleSequenceOperation::StopAnimation => false,
        LifecycleSequenceOperation::MarkInitialized
        | LifecycleSequenceOperation::SetTarget { .. }
        | LifecycleSequenceOperation::UpdateTracking { .. }
        | LifecycleSequenceOperation::JumpPreservingMotion { .. } => was_animating,
    };

    (expected_initialized, expected_animating)
}

pub(super) fn expected_retarget_epoch(
    state: &RuntimeState,
    operation: &LifecycleSequenceOperation,
) -> u64 {
    let baseline_epoch = state.retarget_epoch();
    let baseline_key = state.retarget_key();
    let expected_key = match operation {
        LifecycleSequenceOperation::MarkInitialized
        | LifecycleSequenceOperation::StartAnimation
        | LifecycleSequenceOperation::StopAnimation
        | LifecycleSequenceOperation::ClearInitialization => baseline_key,
        LifecycleSequenceOperation::InitializeCursor {
            position,
            shape,
            location,
            ..
        }
        | LifecycleSequenceOperation::JumpPreservingMotion {
            position,
            shape,
            location,
            ..
        }
        | LifecycleSequenceOperation::JumpAndStopAnimation {
            position,
            shape,
            location,
            ..
        }
        | LifecycleSequenceOperation::SyncToCurrentCursor {
            position,
            shape,
            location,
            ..
        } => RuntimeTargetRetargetKey::from_snapshot(*position, *shape, Some(location)),
        LifecycleSequenceOperation::SetTarget { position, shape } => {
            RuntimeTargetRetargetKey::from_snapshot(*position, *shape, state.tracked_cursor_ref())
        }
        LifecycleSequenceOperation::UpdateTracking { location } => {
            RuntimeTargetRetargetKey::from_snapshot(
                state.target_position(),
                state.target_shape(),
                Some(location),
            )
        }
    };
    let expected_epoch_delta = u64::from(baseline_key != expected_key);

    baseline_epoch.wrapping_add(expected_epoch_delta)
}

pub(super) fn expected_trail_stroke_id(
    state: &RuntimeState,
    operation: &LifecycleSequenceOperation,
) -> StrokeId {
    let baseline_stroke = state.trail_stroke_id();

    match operation {
        LifecycleSequenceOperation::InitializeCursor { .. }
        | LifecycleSequenceOperation::JumpPreservingMotion { .. }
        | LifecycleSequenceOperation::JumpAndStopAnimation { .. }
        | LifecycleSequenceOperation::SyncToCurrentCursor { .. } => baseline_stroke.next(),
        LifecycleSequenceOperation::MarkInitialized
        | LifecycleSequenceOperation::StartAnimation
        | LifecycleSequenceOperation::StopAnimation
        | LifecycleSequenceOperation::ClearInitialization
        | LifecycleSequenceOperation::SetTarget { .. }
        | LifecycleSequenceOperation::UpdateTracking { .. } => baseline_stroke,
    }
}

pub(super) fn apply_lifecycle_sequence_operation(
    state: &mut RuntimeState,
    operation: &LifecycleSequenceOperation,
) {
    match operation {
        LifecycleSequenceOperation::MarkInitialized => state.mark_initialized(),
        LifecycleSequenceOperation::StartAnimation => state.start_animation(),
        LifecycleSequenceOperation::StopAnimation => state.stop_animation(),
        LifecycleSequenceOperation::ClearInitialization => state.clear_initialization(),
        LifecycleSequenceOperation::InitializeCursor {
            position,
            shape,
            seed,
            location,
        } => state.initialize_cursor(*position, *shape, *seed, location),
        LifecycleSequenceOperation::SetTarget { position, shape } => {
            replace_target_preserving_tracking(state, *position, *shape);
        }
        LifecycleSequenceOperation::JumpPreservingMotion {
            position,
            shape,
            location,
        } => state.jump_preserving_motion(*position, *shape, location),
        LifecycleSequenceOperation::JumpAndStopAnimation {
            position,
            shape,
            location,
        } => state.jump_and_stop_animation(*position, *shape, location),
        LifecycleSequenceOperation::SyncToCurrentCursor {
            position,
            shape,
            location,
        } => state.sync_to_current_cursor(*position, *shape, location),
        LifecycleSequenceOperation::UpdateTracking { location } => {
            replace_target_with_tracking(
                state,
                state.target_position(),
                state.target_shape(),
                location,
            );
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) enum CursorTransitionCase {
    Initialize { seed: u32 },
    JumpPreservingMotion,
    JumpAndStopAnimation,
    SyncToCurrentCursor,
}

pub(super) fn cursor_transition_case_strategy() -> BoxedStrategy<CursorTransitionCase> {
    prop_oneof![
        any::<u32>().prop_map(|seed| CursorTransitionCase::Initialize { seed }),
        Just(CursorTransitionCase::JumpPreservingMotion),
        Just(CursorTransitionCase::JumpAndStopAnimation),
        Just(CursorTransitionCase::SyncToCurrentCursor),
    ]
    .boxed()
}

#[derive(Clone, Copy, Debug)]
pub(super) enum TransitionSetupPhase {
    Idle,
    Running,
    Settling,
}

pub(super) fn transition_setup_phase_strategy() -> BoxedStrategy<TransitionSetupPhase> {
    prop_oneof![
        Just(TransitionSetupPhase::Idle),
        Just(TransitionSetupPhase::Running),
        Just(TransitionSetupPhase::Settling),
    ]
    .boxed()
}
