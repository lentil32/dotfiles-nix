mod cleanup;
mod decision;
mod frame;
mod geometry;
mod policy;
mod reducer;

#[cfg(test)]
pub(crate) use cleanup::CleanupDirective;
#[cfg(test)]
pub(crate) use cleanup::CleanupPolicyInput;
#[cfg(test)]
pub(crate) use cleanup::MIN_RENDER_CLEANUP_DELAY_MS;
#[cfg(test)]
pub(crate) use cleanup::MIN_RENDER_HARD_PURGE_DELAY_MS;
#[cfg(test)]
pub(crate) use cleanup::RENDER_HARD_PURGE_DELAY_MULTIPLIER;
pub(crate) use cleanup::as_delay_ms;
#[cfg(test)]
pub(crate) use cleanup::decide_cleanup_directive;
#[cfg(test)]
pub(crate) use cleanup::keep_warm_until_ms;
#[cfg(test)]
pub(crate) use cleanup::next_cleanup_check_delay_ms;
pub(crate) use cleanup::render_cleanup_delay_ms;
pub(crate) use cleanup::render_hard_cleanup_delay_ms;
pub(crate) use decision::CursorEventContext;
pub(crate) use decision::CursorTransition;
pub(crate) use decision::CursorVisibilityEffect;
pub(crate) use decision::EventSource;
pub(crate) use decision::MotionClass;
pub(crate) use decision::RenderAction;
pub(crate) use decision::RenderAllocationPolicy;
pub(crate) use decision::RenderCleanupAction;
pub(crate) use decision::RenderDecision;
pub(crate) use decision::RenderSideEffects;
pub(crate) use decision::ScrollShift;
pub(crate) use decision::TargetCellPresentation;
#[cfg(test)]
pub(crate) use frame::build_render_frame;
pub(crate) use policy::select_event_source;
pub(crate) use reducer::reduce_cursor_event;

#[cfg(test)]
mod tests;
