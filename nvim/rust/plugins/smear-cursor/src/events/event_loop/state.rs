use super::RuntimeBehaviorMetrics;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::events) struct EventLoopDiagnostics {
    pub(in crate::events) metrics: RuntimeBehaviorMetrics,
    pub(in crate::events) last_autocmd_event_ms: f64,
    pub(in crate::events) last_observation_request_ms: f64,
    pub(in crate::events) callback_duration_ewma_ms: f64,
}

pub(in crate::events) struct EventLoopState {
    last_autocmd_event_ms: f64,
    last_observation_request_ms: f64,
    callback_duration_ewma_ms: f64,
    runtime_metrics: RuntimeBehaviorMetrics,
}

impl EventLoopState {
    pub(in crate::events) const fn new() -> Self {
        Self {
            last_autocmd_event_ms: 0.0,
            last_observation_request_ms: 0.0,
            callback_duration_ewma_ms: 0.0,
            runtime_metrics: RuntimeBehaviorMetrics::new(),
        }
    }

    pub(in crate::events) fn note_autocmd_event(&mut self, now_ms: f64) {
        self.last_autocmd_event_ms = now_ms;
    }

    pub(in crate::events) fn clear_autocmd_event_timestamp(&mut self) {
        self.last_autocmd_event_ms = 0.0;
    }

    #[cfg(test)]
    pub(in crate::events) fn elapsed_ms_since_last_autocmd_event(&self, now_ms: f64) -> f64 {
        let last = self.last_autocmd_event_ms;
        if last <= 0.0 {
            f64::INFINITY
        } else {
            (now_ms - last).max(0.0)
        }
    }

    pub(in crate::events) fn note_observation_request(&mut self, now_ms: f64) {
        self.last_observation_request_ms = now_ms;
    }

    pub(in crate::events) fn clear_observation_request_timestamp(&mut self) {
        self.last_observation_request_ms = 0.0;
    }

    pub(in crate::events) fn record_cursor_callback_duration(&mut self, duration_ms: f64) {
        if let Some(next_estimate_ms) =
            super::super::update_callback_duration_ewma(self.callback_duration_ewma_ms, duration_ms)
        {
            self.callback_duration_ewma_ms = next_estimate_ms;
        }
    }

    pub(in crate::events) fn clear_cursor_callback_duration_estimate(&mut self) {
        self.callback_duration_ewma_ms = 0.0;
    }

    pub(in crate::events) fn cursor_callback_duration_estimate_ms(&self) -> f64 {
        self.callback_duration_ewma_ms.max(0.0)
    }

    pub(in crate::events) fn runtime_metrics_mut(&mut self) -> &mut RuntimeBehaviorMetrics {
        &mut self.runtime_metrics
    }

    pub(in crate::events) fn runtime_metrics(&self) -> RuntimeBehaviorMetrics {
        self.runtime_metrics
    }

    pub(in crate::events) fn diagnostics_snapshot(&self) -> EventLoopDiagnostics {
        EventLoopDiagnostics {
            metrics: self.runtime_metrics(),
            last_autocmd_event_ms: self.last_autocmd_event_ms,
            last_observation_request_ms: self.last_observation_request_ms,
            callback_duration_ewma_ms: self.callback_duration_ewma_ms.max(0.0),
        }
    }
}
