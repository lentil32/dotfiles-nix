use crate::core::types::Millis;
use crate::core::types::TimerGeneration;
use crate::core::types::TimerId;
use crate::core::types::TimerSlots;
use crate::core::types::TimerToken;

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub(crate) enum BufferPerfClass {
    #[default]
    Full,
    FastMotion,
    Skip,
}

impl BufferPerfClass {
    pub(crate) const fn diagnostic_name(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::FastMotion => "fast",
            Self::Skip => "skip",
        }
    }

    pub(crate) const fn keeps_ornamental_effects(self) -> bool {
        matches!(self, Self::Full)
    }
}

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub(crate) struct RecoveryPolicyState {
    retry_attempt: u8,
}

impl RecoveryPolicyState {
    pub(crate) const fn retry_attempt(self) -> u8 {
        self.retry_attempt
    }

    pub(crate) fn with_retry_attempt(self, retry_attempt: u8) -> Self {
        Self { retry_attempt }
    }
}

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub(crate) struct IngressPolicyState {
    last_cursor_autocmd_at: Option<Millis>,
    pending_delay_until: Option<Millis>,
}

impl IngressPolicyState {
    #[cfg(test)]
    pub(crate) const fn last_cursor_autocmd_at(self) -> Option<Millis> {
        self.last_cursor_autocmd_at
    }

    pub(crate) const fn pending_delay_until(self) -> Option<Millis> {
        self.pending_delay_until
    }

    pub(crate) fn note_cursor_autocmd(self, observed_at: Millis) -> Self {
        let next_cursor_autocmd_at = match self.last_cursor_autocmd_at {
            Some(previous) if previous.value() > observed_at.value() => previous,
            _ => observed_at,
        };
        Self {
            last_cursor_autocmd_at: Some(next_cursor_autocmd_at),
            ..self
        }
    }

    pub(crate) fn note_pending_delay_until(self, pending_delay_until: Millis) -> Self {
        let next_pending_delay_until = match self.pending_delay_until {
            Some(previous) if previous.value() > pending_delay_until.value() => previous,
            _ => pending_delay_until,
        };
        Self {
            pending_delay_until: Some(next_pending_delay_until),
            ..self
        }
    }

    pub(crate) fn clear_pending_delay(self) -> Self {
        Self {
            pending_delay_until: None,
            ..self
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
struct TimerSlotState {
    generation: TimerGeneration,
    armed: bool,
}

impl TimerSlotState {
    const INITIAL: Self = Self {
        generation: TimerGeneration::INITIAL,
        armed: false,
    };

    fn arm(self, timer_id: TimerId) -> (Self, TimerToken) {
        let generation = self.generation.next();
        let token = TimerToken::new(timer_id, generation);
        (
            Self {
                generation,
                armed: true,
            },
            token,
        )
    }

    fn active_token(self, timer_id: TimerId) -> Option<TimerToken> {
        self.armed
            .then_some(TimerToken::new(timer_id, self.generation))
    }

    fn is_active(self, token: TimerToken) -> bool {
        self.armed && self.generation == token.generation()
    }

    fn clear_active(self) -> Self {
        Self {
            armed: false,
            ..self
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct TimerState {
    slots: TimerSlots<TimerSlotState>,
}

impl Default for TimerState {
    fn default() -> Self {
        Self {
            slots: TimerSlots::filled(TimerSlotState::INITIAL),
        }
    }
}

impl TimerState {
    fn slot(self, timer_id: TimerId) -> TimerSlotState {
        self.slots.copied(timer_id)
    }

    fn with_slot(mut self, timer_id: TimerId, slot: TimerSlotState) -> Self {
        *self.slots.get_mut(timer_id) = slot;
        self
    }

    pub(crate) fn active_token(self, timer_id: TimerId) -> Option<TimerToken> {
        self.slot(timer_id).active_token(timer_id)
    }

    pub(crate) fn active_tokens(self) -> impl Iterator<Item = TimerToken> {
        TimerId::ALL
            .into_iter()
            .filter_map(move |timer_id| self.active_token(timer_id))
    }

    pub(crate) fn arm(self, timer_id: TimerId) -> (Self, TimerToken) {
        let (slot, token) = self.slot(timer_id).arm(timer_id);
        (self.with_slot(timer_id, slot), token)
    }

    pub(crate) fn is_active(self, token: TimerToken) -> bool {
        self.slot(token.id()).is_active(token)
    }

    pub(crate) fn clear_active(self, timer_id: TimerId) -> Self {
        self.with_slot(timer_id, self.slot(timer_id).clear_active())
    }

    pub(crate) fn clear_matching(self, token: TimerToken) -> Self {
        if self.slot(token.id()).is_active(token) {
            self.clear_active(token.id())
        } else {
            self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::proptest::stateful_config;
    use crate::test_support::proptest::timer_id;
    use proptest::collection::vec;
    use proptest::prelude::*;

    proptest! {
        #![proptest_config(stateful_config())]

        #[test]
        fn prop_timer_state_latest_token_per_slot_wins_across_rearm_sequences(
            sequence in vec(timer_id(), 1..=64),
        ) {
            let mut state = TimerState::default();
            let mut history: [Vec<TimerToken>; 4] = std::array::from_fn(|_| Vec::new());

            for timer_id in sequence {
                let (next_state, token) = state.arm(timer_id);
                state = next_state;
                let slot = timer_id.slot_index();

                prop_assert_eq!(state.active_token(timer_id), Some(token));
                prop_assert!(state.is_active(token));

                for stale_token in history[slot].iter().copied() {
                    prop_assert!(!state.is_active(stale_token));
                    prop_assert_eq!(state.clear_matching(stale_token), state);
                }

                for other_id in TimerId::ALL {
                    if other_id == timer_id {
                        continue;
                    }

                    prop_assert_eq!(
                        state.clear_matching(TimerToken::new(other_id, TimerGeneration::INITIAL)),
                        state,
                    );
                }

                history[slot].push(token);
            }

            for timer_id in TimerId::ALL {
                let slot = timer_id.slot_index();
                let expected = history[slot].last().copied();

                prop_assert_eq!(state.active_token(timer_id), expected);

                if let Some(active_token) = expected {
                    prop_assert_eq!(state.clear_matching(active_token).active_token(timer_id), None);
                }
            }
        }

        #[test]
        fn prop_ingress_policy_pending_delay_deadline_only_moves_forward(
            pending_deadlines in vec(any::<u64>(), 1..=64),
        ) {
            let mut policy = IngressPolicyState::default();
            let mut expected_deadline: Option<Millis> = None;

            for pending_deadline in pending_deadlines {
                let millis = Millis::new(pending_deadline);
                policy = policy.note_pending_delay_until(millis);
                expected_deadline = Some(match expected_deadline {
                    Some(previous) if previous.value() > millis.value() => previous,
                    _ => millis,
                });
                prop_assert_eq!(policy.pending_delay_until(), expected_deadline);
            }

            prop_assert_eq!(policy.clear_pending_delay().pending_delay_until(), None);
        }

    }
}
