mod cleanup;
mod decision;
mod frame;
mod geometry;
mod policy;
mod reducer;

#[cfg(test)]
pub(crate) use cleanup::{
    CleanupDirective, CleanupPolicyInput, MIN_RENDER_CLEANUP_DELAY_MS,
    MIN_RENDER_HARD_PURGE_DELAY_MS, RENDER_HARD_PURGE_DELAY_MULTIPLIER, decide_cleanup_directive,
    keep_warm_until_ms, next_cleanup_check_delay_ms,
};
pub(crate) use cleanup::{as_delay_ms, render_cleanup_delay_ms, render_hard_cleanup_delay_ms};
pub(crate) use decision::{
    CursorEventContext, CursorTransition, CursorVisibilityEffect, EventSource, MotionClass,
    RenderAction, RenderAllocationPolicy, RenderCleanupAction, RenderDecision, RenderSideEffects,
    ScrollShift, TargetCellPresentation,
};
#[cfg(test)]
pub(crate) use frame::build_render_frame;
pub(crate) use policy::select_event_source;
pub(crate) use reducer::reduce_cursor_event;

#[cfg(test)]
mod tests;
