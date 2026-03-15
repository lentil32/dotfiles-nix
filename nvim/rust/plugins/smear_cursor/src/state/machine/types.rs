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

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(super) enum AnimationState {
    Uninitialized,
    Idle,
    Settling,
    Running,
    Draining,
}

impl AnimationState {
    pub(super) fn is_initialized(self) -> bool {
        !matches!(self, Self::Uninitialized)
    }

    pub(super) fn is_animating(self) -> bool {
        matches!(self, Self::Running)
    }

    pub(super) fn is_settling(self) -> bool {
        matches!(self, Self::Settling)
    }

    pub(super) fn is_draining(self) -> bool {
        matches!(self, Self::Draining)
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct CursorTracking {
    pub(super) location: Option<CursorLocation>,
}

impl CursorTracking {
    pub(super) fn update(&mut self, location: &CursorLocation) {
        self.location = Some(location.clone());
    }

    pub(super) fn tracked_location(&self) -> Option<&CursorLocation> {
        self.location.as_ref()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(super) struct AnimationTiming {
    pub(super) last_tick_ms: Option<f64>,
}

impl AnimationTiming {
    pub(super) fn reset(&mut self) {
        *self = Self::default();
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

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct GlobalDisplayPose {
    pub(crate) row_display: f64,
    pub(crate) col_display: f64,
    pub(crate) window_handle: i64,
    pub(crate) buffer_handle: i64,
}

impl GlobalDisplayPose {
    pub(super) fn new(position: Point, location: &CursorLocation) -> Self {
        // cue bookkeeping stores display-metric coordinates only for now. Window-origin
        // projection into a global screen plane lands in the follow-up render-plan work.
        Self {
            row_display: position.row,
            col_display: position.col,
            window_handle: location.window_handle,
            buffer_handle: location.buffer_handle,
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum JumpCuePhase {
    Launch,
    Transfer,
    Catch,
    Fade,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct JumpCue {
    pub(crate) cue_id: u64,
    pub(crate) epoch: u64,
    pub(crate) from_pose: GlobalDisplayPose,
    pub(crate) to_pose: GlobalDisplayPose,
    pub(crate) started_at_ms: f64,
    pub(crate) duration_ms: f64,
    pub(crate) strength: f64,
    pub(crate) phase: JumpCuePhase,
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
    pub(super) fn state(&self) -> AnimationState {
        match self {
            Self::Uninitialized => AnimationState::Uninitialized,
            Self::Idle => AnimationState::Idle,
            Self::Settling(_) => AnimationState::Settling,
            Self::Running(_) => AnimationState::Running,
            Self::Draining(_) => AnimationState::Draining,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct TransientRuntimeState {
    pub(super) target_position: Point,
    pub(super) retarget_epoch: u64,
    pub(super) trail_stroke_id: StrokeId,
    pub(super) next_jump_cue_id: u64,
    pub(super) active_jump_cues: Vec<JumpCue>,
    pub(super) last_mode_was_cmdline: Option<bool>,
    pub(super) timing: AnimationTiming,
    pub(super) tracking: CursorTracking,
    pub(super) color_at_cursor: Option<String>,
}

impl Default for TransientRuntimeState {
    fn default() -> Self {
        Self {
            target_position: Point::ZERO,
            retarget_epoch: 0,
            trail_stroke_id: StrokeId::INITIAL,
            next_jump_cue_id: 1,
            active_jump_cues: Vec::new(),
            last_mode_was_cmdline: None,
            timing: AnimationTiming::default(),
            tracking: CursorTracking::default(),
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
