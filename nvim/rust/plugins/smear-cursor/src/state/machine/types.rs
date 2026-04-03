use super::CursorLocation;
use crate::core::types::StrokeId;
use crate::types::Point;
use std::num::NonZeroU32;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(super) enum PluginState {
    Enabled,
    Disabled,
}

impl PluginState {
    pub(super) fn from_enabled(enabled: bool) -> Self {
        if enabled {
            Self::Enabled
        } else {
            Self::Disabled
        }
    }

    pub(super) fn is_enabled(self) -> bool {
        matches!(self, Self::Enabled)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(super) struct MotionClock {
    pub(super) next_frame_at_ms: Option<f64>,
    pub(super) simulation_accumulator_ms: f64,
}

impl MotionClock {
    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PendingTarget {
    pub(crate) position: Point,
    pub(crate) cursor_location: CursorLocation,
    pub(crate) stable_since_ms: f64,
    pub(crate) settle_deadline_ms: f64,
}

impl PendingTarget {
    pub(super) fn new(
        position: Point,
        cursor_location: &CursorLocation,
        stable_since_ms: f64,
        settle_deadline_ms: f64,
    ) -> Self {
        Self {
            position,
            cursor_location: cursor_location.clone(),
            stable_since_ms,
            settle_deadline_ms,
        }
    }

    pub(super) fn matches_observation(
        &self,
        position: Point,
        cursor_location: &CursorLocation,
    ) -> bool {
        self.position == position && &self.cursor_location == cursor_location
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct SettlingPhase {
    pub(super) pending_target: PendingTarget,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(super) struct RunningPhase {
    pub(super) clock: MotionClock,
    pub(super) settle_hold_counter: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct DrainingPhase {
    pub(super) clock: MotionClock,
    pub(super) remaining_steps: NonZeroU32,
}

impl DrainingPhase {
    pub(super) fn new(remaining_steps: u32) -> Option<Self> {
        NonZeroU32::new(remaining_steps).map(|remaining_steps| Self {
            clock: MotionClock::default(),
            remaining_steps,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum AnimationPhase {
    Uninitialized,
    Idle,
    Settling(SettlingPhase),
    Running(RunningPhase),
    Draining(DrainingPhase),
}

impl AnimationPhase {
    pub(super) fn is_initialized(&self) -> bool {
        !matches!(self, Self::Uninitialized)
    }

    pub(super) fn is_animating(&self) -> bool {
        matches!(self, Self::Running(_))
    }

    pub(super) fn is_settling(&self) -> bool {
        matches!(self, Self::Settling(_))
    }

    pub(super) fn is_draining(&self) -> bool {
        matches!(self, Self::Draining(_))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct TransientRuntimeState {
    pub(super) target_position: Point,
    pub(super) retarget_epoch: u64,
    pub(super) trail_stroke_id: StrokeId,
    pub(super) last_mode_was_cmdline: Option<bool>,
    pub(super) last_tick_ms: Option<f64>,
    pub(super) tracked_location: Option<CursorLocation>,
    pub(super) color_at_cursor: Option<u32>,
}

impl Default for TransientRuntimeState {
    fn default() -> Self {
        Self {
            target_position: Point::ZERO,
            retarget_epoch: 0,
            trail_stroke_id: StrokeId::INITIAL,
            last_mode_was_cmdline: None,
            last_tick_ms: None,
            tracked_location: None,
            color_at_cursor: None,
        }
    }
}

impl TransientRuntimeState {
    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) enum CursorTransitionPolicy {
    Initialize { seed: u32 },
    JumpPreservingMotion,
    JumpAndStopAnimation,
    SyncToCurrentCursor,
}
