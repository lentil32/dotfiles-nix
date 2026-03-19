use crate::core::types::{Millis, TimerGeneration, TimerId, TimerToken};

const DEFAULT_IDLE_RETENTION_BUDGET: usize = 2;

const fn default_idle_target_budget(max_kept_windows: usize) -> usize {
    if max_kept_windows < DEFAULT_IDLE_RETENTION_BUDGET {
        max_kept_windows
    } else {
        DEFAULT_IDLE_RETENTION_BUDGET
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

    pub(crate) fn generation(self, timer_id: TimerId) -> TimerGeneration {
        self.generation_for(timer_id)
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

    #[test]
    fn timer_state_arm_stores_the_active_token_in_its_matching_slot() {
        let (state, token) = TimerState::default().arm(TimerId::Recovery);

        assert_eq!(state.active_recovery, Some(token));
        assert_eq!(state.active_animation, None);
        assert_eq!(state.active_ingress, None);
        assert_eq!(state.active_cleanup, None);
        assert_eq!(state.active_token(TimerId::Recovery), Some(token));
    }

    #[test]
    fn timer_state_clear_matching_ignores_tokens_from_other_slots() {
        let (state, recovery_token) = TimerState::default().arm(TimerId::Recovery);
        let animation_token = TimerToken::new(TimerId::Animation, recovery_token.generation());

        assert_eq!(state.clear_matching(animation_token), state);
        assert_eq!(state.clear_matching(recovery_token).active_recovery, None);
    }

    #[test]
    fn timer_state_only_treats_the_latest_token_in_each_slot_as_active_across_many_generations() {
        for timer_id in [
            TimerId::Animation,
            TimerId::Ingress,
            TimerId::Recovery,
            TimerId::Cleanup,
        ] {
            let mut state = TimerState::default();
            let mut stale_tokens = Vec::new();

            for expected_generation in 1..=32 {
                let (next_state, token) = state.arm(timer_id);
                state = next_state;

                assert_eq!(
                    token.generation(),
                    TimerGeneration::new(expected_generation)
                );
                assert_eq!(state.active_token(timer_id), Some(token));
                assert!(
                    state.is_active(token),
                    "newly armed token should be the reducer truth for {timer_id:?}"
                );

                for stale_token in stale_tokens.iter().copied() {
                    assert!(
                        !state.is_active(stale_token),
                        "older token generations must go stale for {timer_id:?}"
                    );
                    assert_eq!(
                        state.clear_matching(stale_token),
                        state,
                        "stale tokens must not clear the live slot for {timer_id:?}"
                    );
                }

                stale_tokens.push(token);
            }
        }
    }

    #[test]
    fn ingress_policy_pending_delay_deadline_only_moves_forward() {
        let policy = IngressPolicyState::default()
            .note_pending_delay_until(Millis::new(40))
            .note_pending_delay_until(Millis::new(35))
            .note_pending_delay_until(Millis::new(55));

        assert_eq!(policy.pending_delay_until(), Some(Millis::new(55)));
    }

    #[test]
    fn render_cleanup_schedule_enters_hot_with_reducer_owned_cooling_fields() {
        let cleanup = RenderCleanupState::scheduled(Millis::new(40), 25, 90, 12);

        assert_eq!(cleanup.thermal(), RenderThermalState::Hot);
        assert_eq!(cleanup.max_kept_windows(), 12);
        assert_eq!(cleanup.idle_target_budget(), 2);
        assert_eq!(cleanup.max_prune_per_tick(), 12);
        assert_eq!(cleanup.next_compaction_due_at(), Some(Millis::new(65)));
        assert_eq!(cleanup.entered_cooling_at(), None);
        assert_eq!(cleanup.hard_purge_due_at(), Some(Millis::new(130)));
        assert_eq!(cleanup.next_deadline(), Some(Millis::new(65)));
    }

    #[test]
    fn render_cleanup_soft_clear_transition_moves_hot_state_into_cooling() {
        let cleanup = RenderCleanupState::scheduled(Millis::new(40), 25, 90, 12)
            .enter_cooling(Millis::new(65));

        assert_eq!(cleanup.thermal(), RenderThermalState::Cooling);
        assert_eq!(cleanup.max_kept_windows(), 12);
        assert_eq!(cleanup.idle_target_budget(), 2);
        assert_eq!(cleanup.max_prune_per_tick(), 12);
        assert_eq!(cleanup.next_compaction_due_at(), Some(Millis::new(65)));
        assert_eq!(cleanup.entered_cooling_at(), Some(Millis::new(65)));
        assert_eq!(cleanup.hard_purge_due_at(), Some(Millis::new(130)));
        assert_eq!(cleanup.next_deadline(), Some(Millis::new(65)));
    }

    #[test]
    fn render_cleanup_schedule_clamps_idle_target_to_small_retention_budget() {
        let cleanup = RenderCleanupState::scheduled(Millis::new(40), 25, 90, 1);

        assert_eq!(cleanup.max_kept_windows(), 1);
        assert_eq!(cleanup.idle_target_budget(), 1);
    }

    #[test]
    fn render_cleanup_continue_cooling_rearms_immediate_compaction_without_resetting_entry_time() {
        let cleanup = RenderCleanupState::scheduled(Millis::new(40), 25, 90, 12)
            .enter_cooling(Millis::new(65))
            .continue_cooling(Millis::new(81));

        assert_eq!(cleanup.thermal(), RenderThermalState::Cooling);
        assert_eq!(cleanup.next_compaction_due_at(), Some(Millis::new(81)));
        assert_eq!(cleanup.entered_cooling_at(), Some(Millis::new(65)));
        assert_eq!(cleanup.next_deadline(), Some(Millis::new(81)));
    }

    #[test]
    fn render_cleanup_converged_cold_preserves_idle_budget_for_diagnostics() {
        let cleanup = RenderCleanupState::scheduled(Millis::new(40), 25, 90, 12)
            .enter_cooling(Millis::new(65))
            .converge_to_cold();

        assert_eq!(cleanup.thermal(), RenderThermalState::Cold);
        assert_eq!(cleanup.max_kept_windows(), 12);
        assert_eq!(cleanup.idle_target_budget(), 2);
        assert_eq!(cleanup.max_prune_per_tick(), 12);
        assert_eq!(cleanup.next_compaction_due_at(), None);
        assert_eq!(cleanup.entered_cooling_at(), None);
        assert_eq!(cleanup.hard_purge_due_at(), None);
        assert_eq!(cleanup.next_deadline(), None);
    }
}
