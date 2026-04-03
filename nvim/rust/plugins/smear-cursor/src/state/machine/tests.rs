pub(super) use super::CursorLocation;
pub(super) use super::CursorShape;
pub(super) use super::RuntimeState;
pub(super) use crate::animation::center;
pub(super) use crate::animation::initial_velocity;
pub(super) use crate::animation::zero_velocity_corners;
pub(super) use crate::core::types::StrokeId;
pub(super) use crate::test_support::proptest::finite_point;
pub(super) use crate::test_support::proptest::stateful_config;
use crate::types::Particle;
use crate::types::Point;
use proptest::prelude::*;

mod epochs;
mod fixtures;
mod lifecycle;
mod lifecycle_model;
mod particle_cache;
mod prepared_motion;
mod scroll;
mod settling;
mod step_output;
mod sync_ops;
mod trail_strokes;

use fixtures::*;
use lifecycle_model::*;
