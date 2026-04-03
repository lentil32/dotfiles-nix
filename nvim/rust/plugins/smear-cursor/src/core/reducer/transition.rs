use crate::core::effect::Effect;
use crate::core::state::CoreState;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Transition {
    pub(crate) next: CoreState,
    pub(crate) effects: Vec<Effect>,
}

impl Transition {
    pub(super) fn new(next: CoreState, effects: Vec<Effect>) -> Self {
        Self { next, effects }
    }

    pub(super) fn stay_owned(state: CoreState) -> Self {
        Self::new(state, Vec::new())
    }

    pub(super) fn stay(state: &CoreState) -> Self {
        Self::new(state.clone(), Vec::new())
    }
}
