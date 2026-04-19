use crate::core::types::Millis;
use crate::core::types::TimerGeneration;
use crate::core::types::TimerId;
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

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub(crate) enum RenderThermalState {
    Hot,
    Cooling,
    #[default]
    Cold,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct HotRenderCleanupState {
    next_compaction_due_at: Millis,
    hard_purge_due_at: Millis,
}

impl HotRenderCleanupState {
    fn scheduled(observed_at: Millis, soft_delay_ms: u64, hard_delay_ms: u64) -> Self {
        let soft_delay_ms = soft_delay_ms.max(1);
        let hard_delay_ms = hard_delay_ms.max(soft_delay_ms);
        Self {
            next_compaction_due_at: Millis::new(observed_at.value().saturating_add(soft_delay_ms)),
            hard_purge_due_at: Millis::new(observed_at.value().saturating_add(hard_delay_ms)),
        }
    }

    pub(crate) const fn next_compaction_due_at(self) -> Millis {
        self.next_compaction_due_at
    }

    pub(crate) const fn hard_purge_due_at(self) -> Millis {
        self.hard_purge_due_at
    }

    pub(crate) const fn enter_cooling(self, observed_at: Millis) -> CoolingRenderCleanupState {
        CoolingRenderCleanupState {
            entered_cooling_at: observed_at,
            next_compaction_due_at: observed_at,
            hard_purge_due_at: self.hard_purge_due_at,
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct CoolingRenderCleanupState {
    entered_cooling_at: Millis,
    next_compaction_due_at: Millis,
    hard_purge_due_at: Millis,
}

impl CoolingRenderCleanupState {
    pub(crate) const fn entered_cooling_at(self) -> Millis {
        self.entered_cooling_at
    }

    pub(crate) const fn next_compaction_due_at(self) -> Millis {
        self.next_compaction_due_at
    }

    pub(crate) const fn hard_purge_due_at(self) -> Millis {
        self.hard_purge_due_at
    }

    pub(crate) const fn continue_cooling(self, observed_at: Millis) -> Self {
        Self {
            next_compaction_due_at: observed_at,
            ..self
        }
    }

    pub(crate) const fn next_deadline(self) -> Millis {
        if self.next_compaction_due_at.value() <= self.hard_purge_due_at.value() {
            self.next_compaction_due_at
        } else {
            self.hard_purge_due_at
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum RenderCleanupState {
    Hot(HotRenderCleanupState),
    Cooling(CoolingRenderCleanupState),
    Cold,
}

impl Default for RenderCleanupState {
    fn default() -> Self {
        Self::cold()
    }
}

impl RenderCleanupState {
    pub(crate) const fn cold() -> Self {
        Self::Cold
    }

    pub(crate) const fn converge_to_cold(self) -> Self {
        Self::Cold
    }

    pub(crate) fn scheduled(observed_at: Millis, soft_delay_ms: u64, hard_delay_ms: u64) -> Self {
        Self::Hot(HotRenderCleanupState::scheduled(
            observed_at,
            soft_delay_ms,
            hard_delay_ms,
        ))
    }

    pub(crate) const fn thermal(self) -> RenderThermalState {
        match self {
            Self::Hot(_) => RenderThermalState::Hot,
            Self::Cooling(_) => RenderThermalState::Cooling,
            Self::Cold => RenderThermalState::Cold,
        }
    }

    pub(crate) const fn next_compaction_due_at(self) -> Option<Millis> {
        match self {
            Self::Hot(schedule) => Some(schedule.next_compaction_due_at()),
            Self::Cooling(schedule) => Some(schedule.next_compaction_due_at()),
            Self::Cold => None,
        }
    }

    pub(crate) const fn entered_cooling_at(self) -> Option<Millis> {
        match self {
            Self::Cooling(schedule) => Some(schedule.entered_cooling_at()),
            Self::Hot(_) | Self::Cold => None,
        }
    }

    pub(crate) const fn hard_purge_due_at(self) -> Option<Millis> {
        match self {
            Self::Hot(schedule) => Some(schedule.hard_purge_due_at()),
            Self::Cooling(schedule) => Some(schedule.hard_purge_due_at()),
            Self::Cold => None,
        }
    }

    pub(crate) const fn next_deadline(self) -> Option<Millis> {
        match self {
            Self::Hot(schedule) => Some(schedule.next_compaction_due_at()),
            Self::Cooling(schedule) => Some(schedule.next_deadline()),
            Self::Cold => None,
        }
    }

    pub(crate) fn enter_cooling(self, observed_at: Millis) -> Self {
        match self {
            Self::Hot(schedule) => Self::Cooling(schedule.enter_cooling(observed_at)),
            Self::Cooling(schedule) => Self::Cooling(schedule.continue_cooling(observed_at)),
            Self::Cold => Self::Cold,
        }
    }

    pub(crate) fn continue_cooling(self, observed_at: Millis) -> Self {
        match self {
            Self::Hot(_) => self.enter_cooling(observed_at),
            Self::Cooling(schedule) => Self::Cooling(schedule.continue_cooling(observed_at)),
            Self::Cold => Self::Cold,
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
struct TimerSlotState {
    generation: TimerGeneration,
    armed: bool,
}

impl TimerSlotState {
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
    animation: TimerSlotState,
    ingress: TimerSlotState,
    recovery: TimerSlotState,
    cleanup: TimerSlotState,
}

impl Default for TimerState {
    fn default() -> Self {
        Self {
            animation: TimerSlotState {
                generation: TimerGeneration::INITIAL,
                armed: false,
            },
            ingress: TimerSlotState {
                generation: TimerGeneration::INITIAL,
                armed: false,
            },
            recovery: TimerSlotState {
                generation: TimerGeneration::INITIAL,
                armed: false,
            },
            cleanup: TimerSlotState {
                generation: TimerGeneration::INITIAL,
                armed: false,
            },
        }
    }
}

impl TimerState {
    fn slot(self, timer_id: TimerId) -> TimerSlotState {
        match timer_id {
            TimerId::Animation => self.animation,
            TimerId::Ingress => self.ingress,
            TimerId::Recovery => self.recovery,
            TimerId::Cleanup => self.cleanup,
        }
    }

    fn with_slot(self, timer_id: TimerId, slot: TimerSlotState) -> Self {
        match timer_id {
            TimerId::Animation => Self {
                animation: slot,
                ..self
            },
            TimerId::Ingress => Self {
                ingress: slot,
                ..self
            },
            TimerId::Recovery => Self {
                recovery: slot,
                ..self
            },
            TimerId::Cleanup => Self {
                cleanup: slot,
                ..self
            },
        }
    }

    pub(crate) fn active_token(self, timer_id: TimerId) -> Option<TimerToken> {
        self.slot(timer_id).active_token(timer_id)
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

    const TIMER_IDS: [TimerId; 4] = [
        TimerId::Animation,
        TimerId::Ingress,
        TimerId::Recovery,
        TimerId::Cleanup,
    ];

    const fn timer_slot_index(timer_id: TimerId) -> usize {
        match timer_id {
            TimerId::Animation => 0,
            TimerId::Ingress => 1,
            TimerId::Recovery => 2,
            TimerId::Cleanup => 3,
        }
    }

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
                let slot = timer_slot_index(timer_id);

                prop_assert_eq!(state.active_token(timer_id), Some(token));
                prop_assert!(state.is_active(token));

                for stale_token in history[slot].iter().copied() {
                    prop_assert!(!state.is_active(stale_token));
                    prop_assert_eq!(state.clear_matching(stale_token), state);
                }

                for other_id in TIMER_IDS {
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

            for timer_id in TIMER_IDS {
                let slot = timer_slot_index(timer_id);
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

        #[test]
        fn prop_render_cleanup_schedule_clamps_budgets_and_deadlines(
            observed_at in any::<u64>(),
            soft_delay_ms in any::<u64>(),
            hard_delay_ms in any::<u64>(),
        ) {
            let cleanup = RenderCleanupState::scheduled(
                Millis::new(observed_at),
                soft_delay_ms,
                hard_delay_ms,
            );
            let clamped_soft_delay_ms = soft_delay_ms.max(1);
            let clamped_hard_delay_ms = hard_delay_ms.max(clamped_soft_delay_ms);
            let expected_next_compaction_due_at =
                Millis::new(observed_at.saturating_add(clamped_soft_delay_ms));
            let expected_hard_purge_due_at =
                Millis::new(observed_at.saturating_add(clamped_hard_delay_ms));

            match cleanup {
                RenderCleanupState::Hot(schedule) => {
                    prop_assert_eq!(
                        schedule.next_compaction_due_at(),
                        expected_next_compaction_due_at
                    );
                    prop_assert_eq!(schedule.hard_purge_due_at(), expected_hard_purge_due_at);
                    prop_assert_eq!(cleanup.next_deadline(), Some(expected_next_compaction_due_at));
                }
                RenderCleanupState::Cooling(_) | RenderCleanupState::Cold => {
                    prop_assert!(false, "scheduled cleanup should always be hot");
                }
            }
        }

        #[test]
        fn prop_render_cleanup_rearming_preserves_cooling_entry_time_and_budget(
            observed_at in any::<u64>(),
            soft_delay_ms in any::<u64>(),
            hard_delay_ms in any::<u64>(),
            entered_cooling_at in any::<u64>(),
            rearm_sequence in vec((any::<bool>(), any::<u64>()), 0..=32),
        ) {
            let scheduled = RenderCleanupState::scheduled(
                Millis::new(observed_at),
                soft_delay_ms,
                hard_delay_ms,
            );
            let entered_cooling_at = Millis::new(entered_cooling_at);
            let mut cleanup = scheduled.enter_cooling(entered_cooling_at);
            let mut expected_next_compaction_due_at = entered_cooling_at;

            for (use_enter_cooling, observed_at) in rearm_sequence {
                let observed_at = Millis::new(observed_at);
                cleanup = if use_enter_cooling {
                    cleanup.enter_cooling(observed_at)
                } else {
                    cleanup.continue_cooling(observed_at)
                };
                expected_next_compaction_due_at = observed_at;
            }

            let expected_hard_purge_due_at = scheduled
                .hard_purge_due_at()
                .expect("scheduled cleanup should always arm a hard purge deadline");
            let expected_next_deadline =
                if expected_next_compaction_due_at.value() <= expected_hard_purge_due_at.value() {
                    expected_next_compaction_due_at
                } else {
                    expected_hard_purge_due_at
                };

            match cleanup {
                RenderCleanupState::Cooling(schedule) => {
                    prop_assert_eq!(schedule.entered_cooling_at(), entered_cooling_at);
                    prop_assert_eq!(
                        schedule.next_compaction_due_at(),
                        expected_next_compaction_due_at
                    );
                    prop_assert_eq!(schedule.hard_purge_due_at(), expected_hard_purge_due_at);
                    prop_assert_eq!(schedule.next_deadline(), expected_next_deadline);
                }
                RenderCleanupState::Hot(_) | RenderCleanupState::Cold => {
                    prop_assert!(false, "entered cooling cleanup should always stay cooling");
                }
            }

            let cold = cleanup.converge_to_cold();
            prop_assert_eq!(cold, RenderCleanupState::Cold);
            prop_assert_eq!(cold.next_deadline(), None);
        }
    }
}
