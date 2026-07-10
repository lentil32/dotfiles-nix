use crate::core::types::Millis;

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub(crate) enum RenderThermalState {
    Hot,
    Cooling,
    #[default]
    Cold,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum RenderCleanupCompactionProgress {
    MadeProgress,
    NoProgress,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum RenderCleanupTimerRearm {
    Allowed,
    Quiesced,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct RenderCleanupRetryPolicy {
    initial_delay_ms: u64,
    max_delay_ms: u64,
}

impl RenderCleanupRetryPolicy {
    pub(crate) const fn new(initial_delay_ms: u64, max_delay_ms: u64) -> Self {
        let initial_delay_ms = if initial_delay_ms == 0 {
            1
        } else {
            initial_delay_ms
        };
        Self {
            initial_delay_ms,
            max_delay_ms: if max_delay_ms < initial_delay_ms {
                initial_delay_ms
            } else {
                max_delay_ms
            },
        }
    }

    fn delay_ms(self, attempt: u8) -> u64 {
        let shift = u32::from(attempt.saturating_sub(1)).min(u64::BITS - 1);
        self.initial_delay_ms
            .saturating_mul(1_u64 << shift)
            .min(self.max_delay_ms)
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct HotRenderCleanupState {
    next_compaction_due_at: Millis,
    hard_purge_due_at: Millis,
    timer_rearm: RenderCleanupTimerRearm,
}

impl HotRenderCleanupState {
    fn scheduled(observed_at: Millis, soft_delay_ms: u64, hard_delay_ms: u64) -> Self {
        let soft_delay_ms = soft_delay_ms.max(1);
        let hard_delay_ms = hard_delay_ms.max(soft_delay_ms);
        Self {
            next_compaction_due_at: Millis::new(observed_at.value().saturating_add(soft_delay_ms)),
            hard_purge_due_at: Millis::new(observed_at.value().saturating_add(hard_delay_ms)),
            timer_rearm: RenderCleanupTimerRearm::Allowed,
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
            compaction_attempted: false,
            retained_resource_retry_attempt: 0,
            timer_rearm: self.timer_rearm,
        }
    }

    fn enter_cooling_after_soft_clear(
        self,
        observed_at: Millis,
        hard_cleanup_delay_ms: u64,
    ) -> CoolingRenderCleanupState {
        self.enter_cooling(observed_at)
            .after_soft_clear(observed_at, hard_cleanup_delay_ms)
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct CoolingRenderCleanupState {
    entered_cooling_at: Millis,
    next_compaction_due_at: Millis,
    hard_purge_due_at: Millis,
    compaction_attempted: bool,
    retained_resource_retry_attempt: u8,
    timer_rearm: RenderCleanupTimerRearm,
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

    #[cfg(test)]
    const fn schedule_immediate_compaction(self, observed_at: Millis) -> Self {
        Self {
            next_compaction_due_at: observed_at,
            ..self
        }
    }

    fn after_soft_clear(self, observed_at: Millis, hard_cleanup_delay_ms: u64) -> Self {
        let hard_purge_due_at = if observed_at.value() >= self.hard_purge_due_at.value() {
            Millis::new(
                observed_at
                    .value()
                    .saturating_add(hard_cleanup_delay_ms.max(1)),
            )
        } else {
            self.hard_purge_due_at
        };
        Self {
            next_compaction_due_at: observed_at,
            hard_purge_due_at,
            compaction_attempted: false,
            retained_resource_retry_attempt: 0,
            ..self
        }
    }

    fn schedule_progress_compaction(
        self,
        observed_at: Millis,
        cadence_delay_ms: u64,
        hard_cleanup_delay_ms: u64,
    ) -> Self {
        // Keep disposal incremental while bounded compaction is succeeding. The hard purge remains
        // a fallback for stalled cleanup instead of becoming a bulk close at an old deadline.
        let progress_hard_purge_due_at = Millis::new(
            observed_at
                .value()
                .saturating_add(hard_cleanup_delay_ms.max(1)),
        );
        Self {
            next_compaction_due_at: Millis::new(
                observed_at.value().saturating_add(cadence_delay_ms.max(1)),
            ),
            hard_purge_due_at: if progress_hard_purge_due_at.value()
                > self.hard_purge_due_at.value()
            {
                progress_hard_purge_due_at
            } else {
                self.hard_purge_due_at
            },
            compaction_attempted: true,
            retained_resource_retry_attempt: 0,
            ..self
        }
    }

    const fn await_hard_purge(self) -> Self {
        Self {
            next_compaction_due_at: self.hard_purge_due_at,
            compaction_attempted: true,
            ..self
        }
    }

    fn retry_hard_purge(self, observed_at: Millis, retry_policy: RenderCleanupRetryPolicy) -> Self {
        let retained_resource_retry_attempt =
            self.retained_resource_retry_attempt.saturating_add(1);
        let retry_delay_ms = retry_policy.delay_ms(retained_resource_retry_attempt);
        let retry_due_at = Millis::new(observed_at.value().saturating_add(retry_delay_ms.max(1)));
        Self {
            next_compaction_due_at: retry_due_at,
            hard_purge_due_at: retry_due_at,
            compaction_attempted: true,
            retained_resource_retry_attempt,
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

    #[cfg(test)]
    const fn retained_resource_retry_attempt(self) -> u8 {
        self.retained_resource_retry_attempt
    }

    pub(crate) const fn has_compaction_attempted(self) -> bool {
        self.compaction_attempted
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

    pub(crate) const fn quiesce_timer_rearm(self) -> Self {
        match self {
            Self::Hot(schedule) => Self::Hot(HotRenderCleanupState {
                timer_rearm: RenderCleanupTimerRearm::Quiesced,
                ..schedule
            }),
            Self::Cooling(schedule) => Self::Cooling(CoolingRenderCleanupState {
                timer_rearm: RenderCleanupTimerRearm::Quiesced,
                ..schedule
            }),
            Self::Cold => Self::Cold,
        }
    }

    pub(crate) const fn revive_timer_rearm(self) -> Self {
        match self {
            Self::Hot(schedule) => Self::Hot(HotRenderCleanupState {
                timer_rearm: RenderCleanupTimerRearm::Allowed,
                ..schedule
            }),
            Self::Cooling(schedule) => Self::Cooling(CoolingRenderCleanupState {
                timer_rearm: RenderCleanupTimerRearm::Allowed,
                ..schedule
            }),
            Self::Cold => Self::Cold,
        }
    }

    pub(crate) const fn timer_rearm_is_quiesced(self) -> bool {
        match self {
            Self::Hot(schedule) => {
                matches!(schedule.timer_rearm, RenderCleanupTimerRearm::Quiesced)
            }
            Self::Cooling(schedule) => {
                matches!(schedule.timer_rearm, RenderCleanupTimerRearm::Quiesced)
            }
            Self::Cold => false,
        }
    }

    #[cfg(test)]
    pub(crate) fn enter_cooling(self, observed_at: Millis) -> Self {
        match self {
            Self::Hot(schedule) => Self::Cooling(schedule.enter_cooling(observed_at)),
            Self::Cooling(schedule) => {
                Self::Cooling(schedule.schedule_immediate_compaction(observed_at))
            }
            Self::Cold => Self::Cold,
        }
    }

    pub(crate) fn enter_cooling_after_soft_clear(
        self,
        observed_at: Millis,
        hard_cleanup_delay_ms: u64,
    ) -> Self {
        match self {
            Self::Hot(schedule) => Self::Cooling(
                schedule.enter_cooling_after_soft_clear(observed_at, hard_cleanup_delay_ms),
            ),
            Self::Cooling(schedule) => {
                Self::Cooling(schedule.after_soft_clear(observed_at, hard_cleanup_delay_ms))
            }
            Self::Cold => Self::Cold,
        }
    }

    pub(crate) fn continue_cooling_after_progress(
        self,
        observed_at: Millis,
        cadence_delay_ms: u64,
        hard_cleanup_delay_ms: u64,
    ) -> Self {
        match self {
            Self::Hot(schedule) => Self::Cooling(
                schedule
                    .enter_cooling(observed_at)
                    .schedule_progress_compaction(
                        observed_at,
                        cadence_delay_ms,
                        hard_cleanup_delay_ms,
                    ),
            ),
            Self::Cooling(schedule) => Self::Cooling(schedule.schedule_progress_compaction(
                observed_at,
                cadence_delay_ms,
                hard_cleanup_delay_ms,
            )),
            Self::Cold => Self::Cold,
        }
    }

    pub(crate) fn await_hard_purge_after_stalled_compaction(self, observed_at: Millis) -> Self {
        match self {
            Self::Hot(schedule) => {
                Self::Cooling(schedule.enter_cooling(observed_at).await_hard_purge())
            }
            Self::Cooling(schedule) => Self::Cooling(schedule.await_hard_purge()),
            Self::Cold => Self::Cold,
        }
    }

    pub(crate) fn retry_hard_purge_after_retained_resources(
        self,
        observed_at: Millis,
        retry_policy: RenderCleanupRetryPolicy,
    ) -> Self {
        match self {
            Self::Hot(schedule) => Self::Cooling(
                schedule
                    .enter_cooling(observed_at)
                    .retry_hard_purge(observed_at, retry_policy),
            ),
            Self::Cooling(schedule) => {
                Self::Cooling(schedule.retry_hard_purge(observed_at, retry_policy))
            }
            Self::Cold => {
                let cooling = CoolingRenderCleanupState {
                    entered_cooling_at: observed_at,
                    next_compaction_due_at: observed_at,
                    hard_purge_due_at: observed_at,
                    compaction_attempted: true,
                    retained_resource_retry_attempt: 0,
                    timer_rearm: RenderCleanupTimerRearm::Allowed,
                };
                Self::Cooling(cooling.retry_hard_purge(observed_at, retry_policy))
            }
        }
    }

    pub(crate) fn retry_hard_purge_after_observed_retained_resources(
        self,
        observed_at: Millis,
        retry_policy: RenderCleanupRetryPolicy,
    ) -> Self {
        match self {
            Self::Hot(schedule) => Self::Hot(schedule),
            Self::Cooling(schedule) => {
                Self::Cooling(schedule.retry_hard_purge(observed_at, retry_policy))
            }
            Self::Cold => {
                let cooling = CoolingRenderCleanupState {
                    entered_cooling_at: observed_at,
                    next_compaction_due_at: observed_at,
                    hard_purge_due_at: observed_at,
                    compaction_attempted: true,
                    retained_resource_retry_attempt: 0,
                    timer_rearm: RenderCleanupTimerRearm::Allowed,
                };
                Self::Cooling(cooling.retry_hard_purge(observed_at, retry_policy))
            }
        }
    }

    #[cfg(test)]
    pub(crate) const fn retained_resource_retry_attempt(self) -> u8 {
        match self {
            Self::Cooling(schedule) => schedule.retained_resource_retry_attempt(),
            Self::Hot(_) | Self::Cold => 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::proptest::stateful_config;
    use proptest::collection::vec;
    use proptest::prelude::*;

    #[derive(Debug, Clone, Copy)]
    enum CoolingTransitionInput {
        Enter {
            observed_at: Millis,
        },
        Progress {
            observed_at: Millis,
            cadence_delay_ms: u64,
            hard_cleanup_delay_ms: u64,
        },
        Stalled {
            observed_at: Millis,
        },
    }

    fn cooling_transition_input() -> impl Strategy<Value = CoolingTransitionInput> {
        prop_oneof![
            any::<u64>().prop_map(|observed_at| CoolingTransitionInput::Enter {
                observed_at: Millis::new(observed_at),
            }),
            (any::<u64>(), any::<u64>(), any::<u64>()).prop_map(
                |(observed_at, cadence_delay_ms, hard_cleanup_delay_ms)| {
                    CoolingTransitionInput::Progress {
                        observed_at: Millis::new(observed_at),
                        cadence_delay_ms,
                        hard_cleanup_delay_ms,
                    }
                }
            ),
            any::<u64>().prop_map(|observed_at| CoolingTransitionInput::Stalled {
                observed_at: Millis::new(observed_at),
            }),
        ]
    }

    proptest! {
        #![proptest_config(stateful_config())]

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
        fn prop_render_cleanup_rearming_preserves_entry_and_only_extends_hard_deadlines(
            observed_at in any::<u64>(),
            soft_delay_ms in any::<u64>(),
            hard_delay_ms in any::<u64>(),
            entered_cooling_at in any::<u64>(),
            rearm_sequence in vec(cooling_transition_input(), 0..=32),
        ) {
            let scheduled = RenderCleanupState::scheduled(
                Millis::new(observed_at),
                soft_delay_ms,
                hard_delay_ms,
            );
            let entered_cooling_at = Millis::new(entered_cooling_at);
            let mut cleanup = scheduled.enter_cooling(entered_cooling_at);
            let mut expected_next_compaction_due_at = entered_cooling_at;
            let mut expected_hard_purge_due_at = scheduled
                .hard_purge_due_at()
                .expect("scheduled cleanup should always arm a hard purge deadline");

            for transition in rearm_sequence {
                match transition {
                    CoolingTransitionInput::Enter { observed_at } => {
                        cleanup = cleanup.enter_cooling(observed_at);
                        expected_next_compaction_due_at = observed_at;
                    }
                    CoolingTransitionInput::Progress {
                        observed_at,
                        cadence_delay_ms,
                        hard_cleanup_delay_ms,
                    } => {
                        cleanup = cleanup.continue_cooling_after_progress(
                            observed_at,
                            cadence_delay_ms,
                            hard_cleanup_delay_ms,
                        );
                        expected_next_compaction_due_at = Millis::new(
                            observed_at
                                .value()
                                .saturating_add(cadence_delay_ms.max(1)),
                        );
                        let progress_hard_purge_due_at = Millis::new(
                            observed_at
                                .value()
                                .saturating_add(hard_cleanup_delay_ms.max(1)),
                        );
                        if progress_hard_purge_due_at.value()
                            > expected_hard_purge_due_at.value()
                        {
                            expected_hard_purge_due_at = progress_hard_purge_due_at;
                        }
                    }
                    CoolingTransitionInput::Stalled { observed_at } => {
                        cleanup = cleanup.await_hard_purge_after_stalled_compaction(observed_at);
                        expected_next_compaction_due_at = expected_hard_purge_due_at;
                    }
                }
            }

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
