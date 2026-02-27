mod machine;
mod transition;

use crate::core::event::Event;
use crate::core::state::CoreState;

pub(crate) use machine::build_planned_render;
pub(crate) use machine::phase0_smoke_fingerprint;
pub(crate) use transition::Transition;

pub(crate) fn reduce(state: &CoreState, event: Event) -> Transition {
    machine::reduce(state, event)
}

#[cfg(test)]
mod tests;
