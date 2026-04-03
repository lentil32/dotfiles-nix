use crate::core::types::Millis;
use crate::core::types::TimerGeneration;
use crate::core::types::TimerId;
use crate::core::types::TimerToken;

const DEFAULT_IDLE_RETENTION_BUDGET: usize = 2;

const fn default_idle_target_budget(max_kept_windows: usize) -> usize {
    if max_kept_windows < DEFAULT_IDLE_RETENTION_BUDGET {
        max_kept_windows
    } else {
        DEFAULT_IDLE_RETENTION_BUDGET
    }
}

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
pub(crate) struct RenderCleanupState {
    thermal: RenderThermalState,
    max_kept_windows: usize,
    idle_target_budget: usize,
    max_prune_per_tick: usize,
    next_compaction_due_at: Option<Millis>,
    entered_cooling_at: Option<Millis>,
    hard_purge_due_at: Option<Millis>,
}

impl Default for RenderCleanupState {
    fn default() -> Self {
        Self::cold()
    }
}

impl RenderCleanupState {
    pub(crate) const fn cold() -> Self {
        Self {
            thermal: RenderThermalState::Cold,
            max_kept_windows: 0,
            idle_target_budget: 0,
            max_prune_per_tick: 0,
            next_compaction_due_at: None,
            entered_cooling_at: None,
            hard_purge_due_at: None,
        }
    }

    pub(crate) const fn converge_to_cold(self) -> Self {
        Self {
            thermal: RenderThermalState::Cold,
            next_compaction_due_at: None,
            entered_cooling_at: None,
            hard_purge_due_at: None,
            ..self
        }
    }

    pub(crate) fn scheduled(
        observed_at: Millis,
        soft_delay_ms: u64,
        hard_delay_ms: u64,
        max_kept_windows: usize,
    ) -> Self {
        let soft_delay_ms = soft_delay_ms.max(1);
        let hard_delay_ms = hard_delay_ms.max(soft_delay_ms);
        Self {
            thermal: RenderThermalState::Hot,
            max_kept_windows,
            // Surprising: Hot still honors the large adaptive reuse budget, but Cooling must
            // converge to a tiny idle pool so post-burst cost does not inherit the hot-path floor.
            idle_target_budget: default_idle_target_budget(max_kept_windows),
            max_prune_per_tick: max_kept_windows.max(1),
            next_compaction_due_at: Some(Millis::new(
                observed_at.value().saturating_add(soft_delay_ms),
            )),
            entered_cooling_at: None,
            hard_purge_due_at: Some(Millis::new(
                observed_at.value().saturating_add(hard_delay_ms),
            )),
        }
    }

    pub(crate) const fn thermal(self) -> RenderThermalState {
        self.thermal
    }

    pub(crate) const fn max_kept_windows(self) -> usize {
        self.max_kept_windows
    }

    pub(crate) const fn idle_target_budget(self) -> usize {
        self.idle_target_budget
    }

    pub(crate) const fn max_prune_per_tick(self) -> usize {
        self.max_prune_per_tick
    }

    pub(crate) const fn next_compaction_due_at(self) -> Option<Millis> {
        self.next_compaction_due_at
    }

    pub(crate) const fn entered_cooling_at(self) -> Option<Millis> {
        self.entered_cooling_at
    }

    pub(crate) const fn hard_purge_due_at(self) -> Option<Millis> {
        self.hard_purge_due_at
    }

    pub(crate) const fn next_deadline(self) -> Option<Millis> {
        match self.thermal {
            RenderThermalState::Hot => match self.next_compaction_due_at {
                Some(next_compaction_due_at) => Some(next_compaction_due_at),
                None => self.hard_purge_due_at,
            },
            RenderThermalState::Cooling => {
                match (self.next_compaction_due_at, self.hard_purge_due_at) {
                    (Some(next_compaction_due_at), Some(hard_purge_due_at)) => Some(
                        if next_compaction_due_at.value() <= hard_purge_due_at.value() {
                            next_compaction_due_at
                        } else {
                            hard_purge_due_at
                        },
                    ),
                    (Some(next_compaction_due_at), None) => Some(next_compaction_due_at),
                    (None, Some(hard_purge_due_at)) => Some(hard_purge_due_at),
                    (None, None) => None,
                }
            }
            RenderThermalState::Cold => None,
        }
    }

    pub(crate) fn enter_cooling(self, observed_at: Millis) -> Self {
        match self.thermal {
            RenderThermalState::Cold => self,
            RenderThermalState::Hot => Self {
                thermal: RenderThermalState::Cooling,
                next_compaction_due_at: Some(observed_at),
                entered_cooling_at: Some(observed_at),
                ..self
            },
            RenderThermalState::Cooling => Self {
                next_compaction_due_at: Some(observed_at),
                ..self
            },
        }
    }

    pub(crate) fn continue_cooling(self, observed_at: Millis) -> Self {
        match self.thermal {
            RenderThermalState::Cold => self,
            RenderThermalState::Hot => self.enter_cooling(observed_at),
            RenderThermalState::Cooling => Self {
                next_compaction_due_at: Some(observed_at),
                ..self
            },
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct TimerState {
    animation_generation: TimerGeneration,
    ingress_generation: TimerGeneration,
    recovery_generation: TimerGeneration,
    cleanup_generation: TimerGeneration,
    active_animation: Option<TimerToken>,
    active_ingress: Option<TimerToken>,
    active_recovery: Option<TimerToken>,
    active_cleanup: Option<TimerToken>,
}

impl Default for TimerState {
    fn default() -> Self {
        Self {
            animation_generation: TimerGeneration::INITIAL,
            ingress_generation: TimerGeneration::INITIAL,
            recovery_generation: TimerGeneration::INITIAL,
            cleanup_generation: TimerGeneration::INITIAL,
            active_animation: None,
            active_ingress: None,
            active_recovery: None,
            active_cleanup: None,
        }
    }
}

impl TimerState {
    fn generation_for(self, timer_id: TimerId) -> TimerGeneration {
        match timer_id {
            TimerId::Animation => self.animation_generation,
            TimerId::Ingress => self.ingress_generation,
            TimerId::Recovery => self.recovery_generation,
            TimerId::Cleanup => self.cleanup_generation,
        }
    }

    fn with_generation(self, timer_id: TimerId, generation: TimerGeneration) -> Self {
        match timer_id {
            TimerId::Animation => Self {
                animation_generation: generation,
                ..self
            },
            TimerId::Ingress => Self {
                ingress_generation: generation,
                ..self
            },
            TimerId::Recovery => Self {
                recovery_generation: generation,
                ..self
            },
            TimerId::Cleanup => Self {
                cleanup_generation: generation,
                ..self
            },
        }
    }

    pub(crate) fn active_token(self, timer_id: TimerId) -> Option<TimerToken> {
        match timer_id {
            TimerId::Animation => self.active_animation,
            TimerId::Ingress => self.active_ingress,
            TimerId::Recovery => self.active_recovery,
            TimerId::Cleanup => self.active_cleanup,
        }
    }

    fn with_active_token(self, token: TimerToken) -> Self {
        match token.id() {
            TimerId::Animation => Self {
                active_animation: Some(token),
                ..self
            },
            TimerId::Ingress => Self {
                active_ingress: Some(token),
                ..self
            },
            TimerId::Recovery => Self {
                active_recovery: Some(token),
                ..self
            },
            TimerId::Cleanup => Self {
                active_cleanup: Some(token),
                ..self
            },
        }
    }

    pub(crate) fn arm(self, timer_id: TimerId) -> (Self, TimerToken) {
        let generation = self.generation_for(timer_id).next();
        let token = TimerToken::new(timer_id, generation);
        let next = self
            .with_generation(timer_id, generation)
            .with_active_token(token);
        (next, token)
    }

    pub(crate) fn is_active(self, token: TimerToken) -> bool {
        self.active_token(token.id()) == Some(token)
    }

    pub(crate) fn clear_active(self, timer_id: TimerId) -> Self {
        match timer_id {
            TimerId::Animation => Self {
                active_animation: None,
                ..self
            },
            TimerId::Ingress => Self {
                active_ingress: None,
                ..self
            },
            TimerId::Recovery => Self {
                active_recovery: None,
                ..self
            },
            TimerId::Cleanup => Self {
                active_cleanup: None,
                ..self
            },
        }
    }

    pub(crate) fn clear_matching(self, token: TimerToken) -> Self {
        if self.is_active(token) {
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
            max_kept_windows in 0_usize..=64_usize,
        ) {
            let cleanup = RenderCleanupState::scheduled(
                Millis::new(observed_at),
                soft_delay_ms,
                hard_delay_ms,
                max_kept_windows,
            );
            let clamped_soft_delay_ms = soft_delay_ms.max(1);
            let clamped_hard_delay_ms = hard_delay_ms.max(clamped_soft_delay_ms);
            let expected_next_compaction_due_at =
                Millis::new(observed_at.saturating_add(clamped_soft_delay_ms));
            let expected_hard_purge_due_at =
                Millis::new(observed_at.saturating_add(clamped_hard_delay_ms));

            prop_assert_eq!(cleanup.thermal(), RenderThermalState::Hot);
            prop_assert_eq!(cleanup.max_kept_windows(), max_kept_windows);
            prop_assert_eq!(
                cleanup.idle_target_budget(),
                max_kept_windows.min(DEFAULT_IDLE_RETENTION_BUDGET)
            );
            prop_assert_eq!(cleanup.max_prune_per_tick(), max_kept_windows.max(1));
            prop_assert_eq!(
                cleanup.next_compaction_due_at(),
                Some(expected_next_compaction_due_at)
            );
            prop_assert_eq!(cleanup.entered_cooling_at(), None);
            prop_assert_eq!(cleanup.hard_purge_due_at(), Some(expected_hard_purge_due_at));
            prop_assert_eq!(cleanup.next_deadline(), Some(expected_next_compaction_due_at));
        }

        #[test]
        fn prop_render_cleanup_rearming_preserves_cooling_entry_time_and_budget(
            observed_at in any::<u64>(),
            soft_delay_ms in any::<u64>(),
            hard_delay_ms in any::<u64>(),
            max_kept_windows in 0_usize..=64_usize,
            entered_cooling_at in any::<u64>(),
            rearm_sequence in vec((any::<bool>(), any::<u64>()), 0..=32),
        ) {
            let scheduled = RenderCleanupState::scheduled(
                Millis::new(observed_at),
                soft_delay_ms,
                hard_delay_ms,
                max_kept_windows,
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

            prop_assert_eq!(cleanup.thermal(), RenderThermalState::Cooling);
            prop_assert_eq!(cleanup.max_kept_windows(), scheduled.max_kept_windows());
            prop_assert_eq!(cleanup.idle_target_budget(), scheduled.idle_target_budget());
            prop_assert_eq!(cleanup.max_prune_per_tick(), scheduled.max_prune_per_tick());
            prop_assert_eq!(
                cleanup.next_compaction_due_at(),
                Some(expected_next_compaction_due_at)
            );
            prop_assert_eq!(cleanup.entered_cooling_at(), Some(entered_cooling_at));
            prop_assert_eq!(cleanup.hard_purge_due_at(), Some(expected_hard_purge_due_at));
            prop_assert_eq!(cleanup.next_deadline(), Some(expected_next_deadline));

            let cold = cleanup.converge_to_cold();
            prop_assert_eq!(cold.thermal(), RenderThermalState::Cold);
            prop_assert_eq!(cold.max_kept_windows(), scheduled.max_kept_windows());
            prop_assert_eq!(cold.idle_target_budget(), scheduled.idle_target_budget());
            prop_assert_eq!(cold.max_prune_per_tick(), scheduled.max_prune_per_tick());
            prop_assert_eq!(cold.next_compaction_due_at(), None);
            prop_assert_eq!(cold.entered_cooling_at(), None);
            prop_assert_eq!(cold.hard_purge_due_at(), None);
            prop_assert_eq!(cold.next_deadline(), None);
        }
    }
}
