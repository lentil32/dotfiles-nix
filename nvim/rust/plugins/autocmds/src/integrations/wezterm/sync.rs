use std::time::Duration;
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WeztermRuntimeMode {
    HealthyAsync,
    DegradedPolling,
}

impl WeztermRuntimeMode {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::HealthyAsync => "healthy_async",
            Self::DegradedPolling => "degraded_polling",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct WeztermSyncStats {
    pub(super) requested: u64,
    pub(super) enqueued: u64,
    pub(super) coalesced: u64,
    pub(super) executed: u64,
    pub(super) wakeup_failures: u64,
    pub(super) enqueue_failures: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct WeztermSyncSnapshot {
    pub(super) mode: WeztermRuntimeMode,
    pub(super) stats: WeztermSyncStats,
}

impl WeztermSyncSnapshot {
    pub(super) fn render(self) -> String {
        format!(
            "mode={} requested={} enqueued={} coalesced={} executed={} wakeup_failures={} enqueue_failures={}",
            self.mode.as_str(),
            self.stats.requested,
            self.stats.enqueued,
            self.stats.coalesced,
            self.stats.executed,
            self.stats.wakeup_failures,
            self.stats.enqueue_failures,
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct CompletionDrainBatchSize(usize);

impl CompletionDrainBatchSize {
    pub(super) const MIN: Self = Self(1);

    pub(super) const fn get(self) -> usize {
        self.0
    }

    pub(super) const fn try_new(value: usize) -> Option<Self> {
        if value == 0 { None } else { Some(Self(value)) }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct WeztermSyncPolicy {
    pub(super) completion_drain_batch_size: CompletionDrainBatchSize,
    pub(super) autocmd_debounce_window: Duration,
}

impl WeztermSyncPolicy {
    pub(super) const fn try_new(
        completion_drain_batch_size: usize,
        autocmd_debounce_window: Duration,
    ) -> Option<Self> {
        let Some(completion_drain_batch_size) =
            CompletionDrainBatchSize::try_new(completion_drain_batch_size)
        else {
            return None;
        };
        Some(Self {
            completion_drain_batch_size,
            autocmd_debounce_window,
        })
    }

    pub(super) fn default_policy() -> Self {
        let debounce = Duration::from_millis(super::WEZTERM_DEFAULT_SYNC_DEBOUNCE_WINDOW_MS);
        match Self::try_new(super::WEZTERM_DEFAULT_COMPLETION_DRAIN_BATCH_SIZE, debounce) {
            Some(policy) => policy,
            None => Self {
                completion_drain_batch_size: CompletionDrainBatchSize::MIN,
                autocmd_debounce_window: debounce,
            },
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct WeztermSyncGate {
    last_sync_started_at: Option<Instant>,
}

impl WeztermSyncGate {
    pub(super) const fn new() -> Self {
        Self {
            last_sync_started_at: None,
        }
    }

    pub(super) fn should_coalesce(&mut self, now: Instant, debounce_window: Duration) -> bool {
        let Some(last_sync_started_at) = self.last_sync_started_at else {
            self.last_sync_started_at = Some(now);
            return false;
        };
        if now.saturating_duration_since(last_sync_started_at) < debounce_window {
            return true;
        }
        self.last_sync_started_at = Some(now);
        false
    }
}

#[cfg(test)]
mod tests {
    use super::CompletionDrainBatchSize;
    use super::WeztermRuntimeMode;
    use super::WeztermSyncGate;
    use super::WeztermSyncPolicy;
    use super::WeztermSyncSnapshot;
    use super::WeztermSyncStats;
    use pretty_assertions::assert_eq;
    use std::time::Duration;
    use std::time::Instant;

    #[test]
    fn sync_snapshot_renders_healthy_state() {
        let snapshot = WeztermSyncSnapshot {
            mode: WeztermRuntimeMode::HealthyAsync,
            stats: WeztermSyncStats {
                requested: 2,
                enqueued: 1,
                coalesced: 0,
                executed: 3,
                wakeup_failures: 0,
                enqueue_failures: 0,
            },
        };

        insta::assert_snapshot!(snapshot.render());
    }

    #[test]
    fn sync_snapshot_renders_degraded_state() {
        let snapshot = WeztermSyncSnapshot {
            mode: WeztermRuntimeMode::DegradedPolling,
            stats: WeztermSyncStats {
                requested: 5,
                enqueued: 4,
                coalesced: 2,
                executed: 1,
                wakeup_failures: 1,
                enqueue_failures: 3,
            },
        };

        insta::assert_snapshot!(snapshot.render());
    }

    #[test]
    fn sync_gate_coalesces_requests_within_window() {
        let start = Instant::now();
        let mut gate = WeztermSyncGate::new();
        let debounce_window = WeztermSyncPolicy::default_policy().autocmd_debounce_window;
        assert!(!gate.should_coalesce(start, debounce_window));
        assert!(gate.should_coalesce(start + Duration::from_millis(10), debounce_window));
        assert!(!gate.should_coalesce(start + debounce_window, debounce_window));
    }

    #[test]
    fn sync_policy_rejects_zero_drain_batch_size() {
        assert_eq!(
            WeztermSyncPolicy::try_new(0, Duration::from_millis(1)),
            None
        );
        assert_eq!(CompletionDrainBatchSize::try_new(0), None);
    }
}
