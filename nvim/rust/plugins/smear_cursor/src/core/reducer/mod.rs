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
pub(crate) fn reduce(state: &CoreState, event: Event) -> Transition {
    machine::reduce(state, event)
}

#[cfg(test)]
mod tests;
