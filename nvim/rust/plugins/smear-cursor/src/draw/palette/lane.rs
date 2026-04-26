use super::PaletteState;
use std::cell::RefCell;
use std::panic::AssertUnwindSafe;
use std::panic::catch_unwind;
use std::panic::resume_unwind;

#[derive(Debug)]
pub(crate) struct PaletteStateLane {
    state: RefCell<PaletteStateSlot>,
}

impl Default for PaletteStateLane {
    fn default() -> Self {
        Self {
            state: RefCell::new(PaletteStateSlot::Ready(Box::new(PaletteState::new()))),
        }
    }
}

impl PaletteStateLane {
    pub(super) fn read_state<R>(&self, reader: impl FnOnce(&PaletteState) -> R) -> R {
        let state = self.state.borrow();
        let PaletteStateSlot::Ready(state) = &*state else {
            panic!("palette state read while detached");
        };
        reader(state)
    }

    pub(crate) fn next_recovery_epoch(&self) -> Option<u64> {
        let Ok(slot) = self.state.try_borrow() else {
            return None;
        };
        match &*slot {
            PaletteStateSlot::Ready(state) => Some(state.core.epoch().wrapping_add(1)),
            PaletteStateSlot::InUse => None,
        }
    }

    pub(crate) fn recover_to_epoch(&self, epoch: u64) -> bool {
        let Ok(mut slot) = self.state.try_borrow_mut() else {
            return false;
        };
        if matches!(&*slot, PaletteStateSlot::InUse) {
            return false;
        }
        *slot = PaletteStateSlot::Ready(Box::new(PaletteState::new_with_epoch(epoch)));
        true
    }

    pub(super) fn mutate_state<R>(&self, mutator: impl FnOnce(&mut PaletteState) -> R) -> R {
        // Detach palette state so palette mutations never hold the runtime RefCell borrow while
        // callers decide which host work to run next.
        let mut state = self.take_state();
        match catch_unwind(AssertUnwindSafe(|| mutator(&mut state))) {
            Ok(output) => {
                self.restore_state(state);
                output
            }
            Err(panic_payload) => {
                self.restore_state(PaletteState::new_with_epoch(
                    state.core.epoch().wrapping_add(1),
                ));
                resume_unwind(panic_payload);
            }
        }
    }

    fn take_state(&self) -> PaletteState {
        let mut slot = self.state.borrow_mut();
        match std::mem::replace(&mut *slot, PaletteStateSlot::InUse) {
            PaletteStateSlot::Ready(state) => *state,
            PaletteStateSlot::InUse => panic!("palette state mutation while detached"),
        }
    }

    fn restore_state(&self, state: PaletteState) {
        let mut slot = self.state.borrow_mut();
        let previous = std::mem::replace(&mut *slot, PaletteStateSlot::Ready(Box::new(state)));
        debug_assert!(matches!(previous, PaletteStateSlot::InUse));
    }

    #[cfg(test)]
    pub(crate) fn epoch_for_test(&self) -> u64 {
        self.read_state(|state| state.core.epoch())
    }
}

#[derive(Debug)]
enum PaletteStateSlot {
    Ready(Box<PaletteState>),
    InUse,
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn palette_lane_resets_to_next_epoch_when_detached_mutation_panics() {
        let lane = PaletteStateLane::default();

        let result = catch_unwind(AssertUnwindSafe(|| {
            lane.mutate_state(|_| panic!("forced palette failure"));
        }));

        assert_eq!(result.is_err(), true);
        assert_eq!(lane.read_state(|state| state.core.epoch()), 1);
    }

    #[test]
    fn palette_lane_recovers_after_nested_access_attempt() {
        let lane = PaletteStateLane::default();

        let result = catch_unwind(AssertUnwindSafe(|| {
            lane.mutate_state(|_| lane.read_state(|_| ()));
        }));

        assert_eq!(result.is_err(), true);
        assert_eq!(lane.read_state(|state| state.core.epoch()), 1);
    }

    #[test]
    fn palette_lane_can_recover_to_a_fixed_epoch_idempotently() {
        let lane = PaletteStateLane::default();

        assert_eq!(lane.next_recovery_epoch(), Some(1));
        assert_eq!(lane.recover_to_epoch(/*epoch*/ 7), true);
        assert_eq!(lane.recover_to_epoch(/*epoch*/ 7), true);

        assert_eq!(lane.read_state(|state| state.core.epoch()), 7);
    }
}
