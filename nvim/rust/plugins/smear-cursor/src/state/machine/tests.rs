pub(super) use super::CursorShape;
pub(super) use super::RuntimePreview;
pub(super) use super::RuntimeState;
pub(super) use super::RuntimeTargetRetargetKey;
pub(super) use super::RuntimeTargetSnapshot;
pub(super) use super::TrackedCursor;
pub(super) use super::types::AnimationClockSample;
pub(super) use super::types::SettlingWindow;
pub(super) use crate::animation::center;
pub(super) use crate::animation::initial_velocity;
pub(super) use crate::animation::zero_velocity_corners;
pub(super) use crate::core::types::StrokeId;
use crate::position::RenderPoint;
pub(super) use crate::test_support::proptest::finite_point;
pub(super) use crate::test_support::proptest::stateful_config;
use crate::types::Particle;
pub(super) use crate::types::RenderStepSample;
use proptest::prelude::*;

mod config_updates;
mod epochs;
mod fixtures;
mod lifecycle;
mod lifecycle_model;
mod particle_cache;
mod prepared_motion;
mod render_step_samples;
mod scroll;
mod settling;
mod step_output;
mod sync_ops;
mod trail_strokes;

use fixtures::*;
use lifecycle_model::*;

pub(super) fn replace_target_preserving_tracking(
    state: &mut RuntimeState,
    position: RenderPoint,
    shape: CursorShape,
) {
    let snapshot =
        RuntimeTargetSnapshot::preserving_tracking(position, shape, state.tracked_cursor_ref());
    state.retarget_preserving_current_pose(snapshot);
}

pub(super) fn replace_target_with_tracking(
    state: &mut RuntimeState,
    position: RenderPoint,
    shape: CursorShape,
    tracked_cursor: &TrackedCursor,
) {
    state.retarget_tracked_preserving_current_pose(position, shape, tracked_cursor);
}
