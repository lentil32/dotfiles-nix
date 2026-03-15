use crate::core::types::{Millis, TimerGeneration, TimerId, TimerToken};

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
}

impl IngressPolicyState {
    pub(crate) const fn last_cursor_autocmd_at(self) -> Option<Millis> {
        self.last_cursor_autocmd_at
    }

    pub(crate) fn note_cursor_autocmd(self, observed_at: Millis) -> Self {
        let next_cursor_autocmd_at = match self.last_cursor_autocmd_at {
            Some(previous) if previous.value() > observed_at.value() => previous,
            _ => observed_at,
        };
        Self {
            last_cursor_autocmd_at: Some(next_cursor_autocmd_at),
        }
    }

    pub(crate) fn admits_key_fallback(self, observed_at: Millis, freshness_window_ms: u64) -> bool {
        let Some(last_cursor_autocmd_at) = self.last_cursor_autocmd_at else {
            return true;
        };
        observed_at
            .value()
            .saturating_sub(last_cursor_autocmd_at.value())
            > freshness_window_ms
    }
}

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub(crate) enum RenderCleanupState {
    #[default]
    Inactive,
    Armed {
        soft_due_at: Millis,
        hard_due_at: Millis,
        max_kept_windows: usize,
    },
    SoftCleared {
        hard_due_at: Millis,
        max_kept_windows: usize,
    },
}

impl RenderCleanupState {
    pub(crate) fn scheduled(
        observed_at: Millis,
        soft_delay_ms: u64,
        hard_delay_ms: u64,
        max_kept_windows: usize,
    ) -> Self {
        let soft_delay_ms = soft_delay_ms.max(1);
        let hard_delay_ms = hard_delay_ms.max(soft_delay_ms);
        Self::Armed {
            soft_due_at: Millis::new(observed_at.value().saturating_add(soft_delay_ms)),
            hard_due_at: Millis::new(observed_at.value().saturating_add(hard_delay_ms)),
            max_kept_windows,
        }
    }

    pub(crate) const fn next_deadline(self) -> Option<Millis> {
        match self {
            Self::Inactive => None,
            Self::Armed { soft_due_at, .. } => Some(soft_due_at),
            Self::SoftCleared { hard_due_at, .. } => Some(hard_due_at),
        }
    }

    pub(crate) fn soft_cleared(self) -> Self {
        match self {
            Self::Armed {
                hard_due_at,
                max_kept_windows,
                ..
            }
            | Self::SoftCleared {
                hard_due_at,
                max_kept_windows,
            } => Self::SoftCleared {
                hard_due_at,
                max_kept_windows,
            },
            Self::Inactive => Self::Inactive,
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
}
