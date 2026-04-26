use super::geometry::render_side_effects_for_action;
use crate::core::state::SemanticEvent;
use crate::core::types::AnimationSchedule;
use crate::core::types::Millis;
use crate::position::ScreenCell;
use crate::state::TrackedCursor;
use crate::types::CursorCellShape;
use crate::types::RenderFrame;

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub(crate) enum MotionClass {
    #[default]
    Continuous,
    DiscontinuousJump,
    SurfaceRetarget,
}

#[derive(Debug, Clone)]
pub(crate) struct CursorEventContext {
    pub(crate) row: f64,
    pub(crate) col: f64,
    pub(crate) now_ms: f64,
    pub(crate) seed: u32,
    pub(crate) tracked_cursor: TrackedCursor,
    pub(crate) scroll_shift: Option<ScrollShift>,
    pub(crate) semantic_event: SemanticEvent,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ScrollShift {
    pub(crate) row_shift: f64,
    pub(crate) col_shift: f64,
    pub(crate) min_row: f64,
    pub(crate) max_row: f64,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum EventSource {
    External,
    AnimationTick,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum MotionTarget {
    Available(ScreenCell),
    Unavailable,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum RenderAction {
    Draw(Box<RenderFrame>),
    ClearAll,
    Noop,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum RenderCleanupAction {
    NoAction,
    Schedule,
    Invalidate,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum RenderAllocationPolicy {
    ReuseOnly,
    BootstrapIfPoolEmpty,
}

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub(crate) enum CursorVisibilityEffect {
    #[default]
    Keep,
    Hide,
    Show,
}

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub(crate) enum TargetCellPresentation {
    #[default]
    None,
    OverlayCursorCell(CursorCellShape),
}

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub(crate) struct RenderSideEffects {
    pub(crate) redraw_after_draw_if_cmdline: bool,
    pub(crate) redraw_after_clear_if_cmdline: bool,
    pub(crate) target_cell_presentation: TargetCellPresentation,
    pub(crate) cursor_visibility: CursorVisibilityEffect,
    // apply consumes this reducer-owned policy directly instead of re-reading
    // `hide_target_hack` from live runtime config.
    pub(crate) allow_real_cursor_updates: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RenderDecision {
    pub(crate) render_action: RenderAction,
    pub(crate) render_cleanup_action: RenderCleanupAction,
    pub(crate) render_allocation_policy: RenderAllocationPolicy,
    pub(crate) render_side_effects: RenderSideEffects,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CursorTransition {
    pub(crate) render_decision: RenderDecision,
    pub(crate) motion_class: MotionClass,
    pub(crate) animation_schedule: AnimationSchedule,
}

impl CursorTransition {
    pub(super) fn with_render_action(
        mode: &str,
        render_action: RenderAction,
        render_allocation_policy: RenderAllocationPolicy,
        allow_real_cursor_updates: bool,
    ) -> Self {
        let render_side_effects =
            render_side_effects_for_action(mode, &render_action, allow_real_cursor_updates);
        Self {
            render_decision: RenderDecision {
                render_action,
                render_cleanup_action: RenderCleanupAction::NoAction,
                render_allocation_policy,
                render_side_effects,
            },
            motion_class: MotionClass::Continuous,
            animation_schedule: AnimationSchedule::Idle,
        }
    }

    pub(super) fn with_motion_class(mut self, motion_class: MotionClass) -> Self {
        self.motion_class = motion_class;
        self
    }

    pub(super) fn with_animation_schedule(mut self, schedule: AnimationSchedule) -> Self {
        self.animation_schedule = schedule;
        self
    }

    pub(super) fn with_render_cleanup_action(mut self, action: RenderCleanupAction) -> Self {
        self.render_decision.render_cleanup_action = action;
        self
    }

    pub(super) fn with_next_animation_deadline(self, deadline: Option<Millis>) -> Self {
        let schedule =
            deadline.map_or(AnimationSchedule::DefaultDelay, AnimationSchedule::Deadline);
        self.with_animation_schedule(schedule)
    }

    #[cfg(test)]
    pub(crate) const fn should_schedule_next_animation(&self) -> bool {
        self.animation_schedule.should_schedule()
    }

    #[cfg(test)]
    pub(crate) const fn next_animation_at_ms(&self) -> Option<u64> {
        self.animation_schedule.next_animation_at_ms()
    }

    pub(super) fn with_cursor_visibility(mut self, effect: CursorVisibilityEffect) -> Self {
        self.render_decision.render_side_effects.cursor_visibility = effect;
        self
    }
}

pub(super) struct CursorTransitions;

impl CursorTransitions {
    pub(super) fn clear_all(mode: &str, allow_real_cursor_updates: bool) -> CursorTransition {
        CursorTransition::with_render_action(
            mode,
            RenderAction::ClearAll,
            RenderAllocationPolicy::ReuseOnly,
            allow_real_cursor_updates,
        )
    }

    pub(super) fn draw(
        mode: &str,
        frame: RenderFrame,
        animation_schedule: AnimationSchedule,
        render_allocation_policy: RenderAllocationPolicy,
    ) -> CursorTransition {
        let allow_real_cursor_updates = !frame.hide_target_hack;
        let render_action = RenderAction::Draw(Box::new(frame));
        CursorTransition::with_render_action(
            mode,
            render_action,
            render_allocation_policy,
            allow_real_cursor_updates,
        )
        .with_animation_schedule(animation_schedule)
    }

    pub(super) fn noop(mode: &str, allow_real_cursor_updates: bool) -> CursorTransition {
        CursorTransition::with_render_action(
            mode,
            RenderAction::Noop,
            RenderAllocationPolicy::ReuseOnly,
            allow_real_cursor_updates,
        )
    }
}
