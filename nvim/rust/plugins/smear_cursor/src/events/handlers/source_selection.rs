use super::super::ingress::AutocmdIngress;
#[cfg(test)]
use crate::core::runtime_reducer::{EventSource, select_event_source};
#[cfg(test)]
use crate::state::{CursorLocation, RuntimeState};
#[cfg(test)]
use crate::types::Point;

#[cfg(test)]
pub(crate) fn select_core_event_source(
    mode: &str,
    state: &RuntimeState,
    requested_target: Option<Point>,
    cursor_location: &CursorLocation,
) -> EventSource {
    select_event_source(mode, state, requested_target, cursor_location)
}

pub(crate) fn should_request_observation_for_autocmd(ingress: AutocmdIngress) -> bool {
    ingress.requests_observation_base()
}
