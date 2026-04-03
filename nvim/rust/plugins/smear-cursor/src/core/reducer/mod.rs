//! Pure reducer entrypoint for the core state machine.
//!
//! One immutable `CoreState` plus one `Event` yields one `Transition`; deferred
//! effects are described in the transition and are never executed inline here.

mod machine;
mod transition;

use crate::core::event::Event;
use crate::core::state::CoreState;

pub(crate) use machine::build_planned_render;
pub(crate) use transition::Transition;

/// Reduces a single core event into the next state transition.
#[cfg(test)]
pub(crate) fn reduce(state: &CoreState, event: Event) -> Transition {
    machine::reduce_owned(state.clone(), event)
}

/// Reduces a single core event by consuming the current state.
pub(crate) fn reduce_owned(state: CoreState, event: Event) -> Transition {
    machine::reduce_owned(state, event)
}

#[cfg(test)]
mod tests;
