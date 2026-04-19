use super::super::ingress::AutocmdIngress;
#[cfg(test)]
use crate::core::runtime_reducer::EventSource;
#[cfg(test)]
use crate::core::runtime_reducer::MotionTarget;
#[cfg(test)]
use crate::core::runtime_reducer::select_event_source;
#[cfg(test)]
use crate::core::state::SemanticEvent;
#[cfg(test)]
use crate::state::RuntimeState;
#[cfg(test)]
use crate::state::TrackedCursor;

#[cfg(test)]
pub(crate) fn select_core_event_source(
    mode: &str,
    state: &RuntimeState,
    semantic_event: SemanticEvent,
    motion_target: MotionTarget,
    tracked_cursor: &TrackedCursor,
) -> EventSource {
    select_event_source(mode, state, semantic_event, motion_target, tracked_cursor)
}

pub(crate) fn should_request_observation_for_autocmd(ingress: AutocmdIngress) -> bool {
    ingress.requests_observation_base()
}
