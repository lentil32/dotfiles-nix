use crate::core::state::ProbeKind;
use std::cell::RefCell;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct DurationTelemetry {
    pub(super) samples: u64,
    pub(super) total_micros: u64,
    pub(super) max_micros: u64,
}

impl DurationTelemetry {
    fn record_micros(&mut self, duration_micros: u64) {
        self.samples = self.samples.saturating_add(1);
        self.total_micros = self.total_micros.saturating_add(duration_micros);
        self.max_micros = self.max_micros.max(duration_micros);
    }

    pub(super) fn mean_ms(self) -> f64 {
        if self.samples == 0 {
            return 0.0;
        }
        (self.total_micros as f64 / self.samples as f64) / 1000.0
    }

    pub(super) fn max_ms(self) -> f64 {
        self.max_micros as f64 / 1000.0
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct DepthTelemetry {
    pub(super) samples: u64,
    pub(super) total_depth: u64,
    pub(super) max_depth: u64,
}

impl DepthTelemetry {
    fn record_depth(&mut self, depth: usize) {
        let depth = u64::try_from(depth).unwrap_or(u64::MAX);
        self.samples = self.samples.saturating_add(1);
        self.total_depth = self.total_depth.saturating_add(depth);
        self.max_depth = self.max_depth.max(depth);
    }

    pub(super) fn mean_depth(self) -> f64 {
        if self.samples == 0 {
            return 0.0;
        }
        self.total_depth as f64 / self.samples as f64
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct RuntimeBehaviorMetrics {
    pub(super) ingress_received: u64,
    pub(super) ingress_coalesced: u64,
    pub(super) ingress_dropped: u64,
    pub(super) ingress_applied: u64,
    pub(super) observation_requests_executed: u64,
    pub(super) degraded_draw_applications: u64,
    pub(super) stale_token_events: u64,
    pub(super) timer_schedule: DurationTelemetry,
    pub(super) timer_fire: DurationTelemetry,
    pub(super) scheduled_queue_depth: DepthTelemetry,
    pub(super) cursor_color_probe: ProbeTelemetry,
    pub(super) background_probe: ProbeTelemetry,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct ProbeTelemetry {
    pub(super) duration: DurationTelemetry,
    pub(super) refresh_retries: u64,
    pub(super) refresh_budget_exhausted: u64,
}

impl ProbeTelemetry {
    const fn new() -> Self {
        Self {
            duration: DurationTelemetry {
                samples: 0,
                total_micros: 0,
                max_micros: 0,
            },
            refresh_retries: 0,
            refresh_budget_exhausted: 0,
        }
    }
}

impl RuntimeBehaviorMetrics {
    fn probe_telemetry_mut(&mut self, kind: ProbeKind) -> &mut ProbeTelemetry {
        match kind {
            ProbeKind::CursorColor => &mut self.cursor_color_probe,
            ProbeKind::Background => &mut self.background_probe,
        }
    }

    fn record_ingress_received(&mut self) {
        self.ingress_received = self.ingress_received.saturating_add(1);
    }

    fn record_ingress_coalesced(&mut self) {
        self.ingress_coalesced = self.ingress_coalesced.saturating_add(1);
    }

    fn record_ingress_dropped(&mut self) {
        self.ingress_dropped = self.ingress_dropped.saturating_add(1);
    }

    fn record_ingress_applied(&mut self) {
        self.ingress_applied = self.ingress_applied.saturating_add(1);
    }

    fn record_observation_request_executed(&mut self) {
        self.observation_requests_executed = self.observation_requests_executed.saturating_add(1);
    }

    fn record_degraded_draw_application(&mut self) {
        self.degraded_draw_applications = self.degraded_draw_applications.saturating_add(1);
    }

    fn record_stale_token_event(&mut self) {
        self.stale_token_events = self.stale_token_events.saturating_add(1);
    }

    fn record_timer_schedule_duration(&mut self, duration_micros: u64) {
        self.timer_schedule.record_micros(duration_micros);
    }

    fn record_timer_fire_duration(&mut self, duration_micros: u64) {
        self.timer_fire.record_micros(duration_micros);
    }

    fn record_scheduled_queue_depth(&mut self, depth: usize) {
        self.scheduled_queue_depth.record_depth(depth);
    }

    fn record_probe_duration(&mut self, kind: ProbeKind, duration_micros: u64) {
        self.probe_telemetry_mut(kind)
            .duration
            .record_micros(duration_micros);
    }

    fn record_probe_refresh_retried(&mut self, kind: ProbeKind) {
        let telemetry = self.probe_telemetry_mut(kind);
        telemetry.refresh_retries = telemetry.refresh_retries.saturating_add(1);
    }

    fn record_probe_refresh_budget_exhausted(&mut self, kind: ProbeKind) {
        let telemetry = self.probe_telemetry_mut(kind);
        telemetry.refresh_budget_exhausted = telemetry.refresh_budget_exhausted.saturating_add(1);
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct EventLoopDiagnostics {
    pub(super) metrics: RuntimeBehaviorMetrics,
    pub(super) last_autocmd_event_ms: f64,
    pub(super) last_observation_request_ms: f64,
    pub(super) callback_duration_ewma_ms: f64,
}

pub(super) struct EventLoopState {
    last_autocmd_event_ms: f64,
    last_observation_request_ms: f64,
    callback_duration_ewma_ms: f64,
    runtime_metrics: RuntimeBehaviorMetrics,
}

impl EventLoopState {
    const CALLBACK_DURATION_EWMA_ALPHA: f64 = 0.25;

    pub(super) const fn new() -> Self {
        Self {
            last_autocmd_event_ms: 0.0,
            last_observation_request_ms: 0.0,
            callback_duration_ewma_ms: 0.0,
            runtime_metrics: RuntimeBehaviorMetrics {
                ingress_received: 0,
                ingress_coalesced: 0,
                ingress_dropped: 0,
                ingress_applied: 0,
                observation_requests_executed: 0,
                degraded_draw_applications: 0,
                stale_token_events: 0,
                timer_schedule: DurationTelemetry {
                    samples: 0,
                    total_micros: 0,
                    max_micros: 0,
                },
                timer_fire: DurationTelemetry {
                    samples: 0,
                    total_micros: 0,
                    max_micros: 0,
                },
                scheduled_queue_depth: DepthTelemetry {
                    samples: 0,
                    total_depth: 0,
                    max_depth: 0,
                },
                cursor_color_probe: ProbeTelemetry::new(),
                background_probe: ProbeTelemetry::new(),
            },
        }
    }

    pub(super) fn note_autocmd_event(&mut self, now_ms: f64) {
        self.last_autocmd_event_ms = now_ms;
    }

    pub(super) fn clear_autocmd_event_timestamp(&mut self) {
        self.last_autocmd_event_ms = 0.0;
    }

    #[cfg(test)]
    pub(super) fn elapsed_ms_since_last_autocmd_event(&self, now_ms: f64) -> f64 {
        let last = self.last_autocmd_event_ms;
        if last <= 0.0 {
            f64::INFINITY
        } else {
            (now_ms - last).max(0.0)
        }
    }

    pub(super) fn note_observation_request(&mut self, now_ms: f64) {
        self.last_observation_request_ms = now_ms;
    }

    pub(super) fn clear_observation_request_timestamp(&mut self) {
        self.last_observation_request_ms = 0.0;
    }

    pub(super) fn record_cursor_callback_duration(&mut self, duration_ms: f64) {
        if !duration_ms.is_finite() {
            return;
        }

        let observed = duration_ms.max(0.0);
        let previous = self.callback_duration_ewma_ms;
        self.callback_duration_ewma_ms = if previous <= 0.0 {
            observed
        } else {
            previous + Self::CALLBACK_DURATION_EWMA_ALPHA * (observed - previous)
        };
    }

    pub(super) fn clear_cursor_callback_duration_estimate(&mut self) {
        self.callback_duration_ewma_ms = 0.0;
    }

    pub(super) fn cursor_callback_duration_estimate_ms(&self) -> f64 {
        self.callback_duration_ewma_ms.max(0.0)
    }

    pub(super) fn record_ingress_received(&mut self) {
        self.runtime_metrics.record_ingress_received();
    }

    pub(super) fn record_ingress_coalesced(&mut self) {
        self.runtime_metrics.record_ingress_coalesced();
    }

    pub(super) fn record_ingress_dropped(&mut self) {
        self.runtime_metrics.record_ingress_dropped();
    }

    pub(super) fn record_ingress_applied(&mut self) {
        self.runtime_metrics.record_ingress_applied();
    }

    pub(super) fn record_observation_request_executed(&mut self) {
        self.runtime_metrics.record_observation_request_executed();
    }

    pub(super) fn record_degraded_draw_application(&mut self) {
        self.runtime_metrics.record_degraded_draw_application();
    }

    pub(super) fn record_stale_token_event(&mut self) {
        self.runtime_metrics.record_stale_token_event();
    }

    pub(super) fn record_timer_schedule_duration(&mut self, duration_micros: u64) {
        self.runtime_metrics
            .record_timer_schedule_duration(duration_micros);
    }

    pub(super) fn record_timer_fire_duration(&mut self, duration_micros: u64) {
        self.runtime_metrics
            .record_timer_fire_duration(duration_micros);
    }

    pub(super) fn record_scheduled_queue_depth(&mut self, depth: usize) {
        self.runtime_metrics.record_scheduled_queue_depth(depth);
    }

    pub(super) fn record_probe_duration(&mut self, kind: ProbeKind, duration_micros: u64) {
        self.runtime_metrics
            .record_probe_duration(kind, duration_micros);
    }

    pub(super) fn record_probe_refresh_retried(&mut self, kind: ProbeKind) {
        self.runtime_metrics.record_probe_refresh_retried(kind);
    }

    pub(super) fn record_probe_refresh_budget_exhausted(&mut self, kind: ProbeKind) {
        self.runtime_metrics
            .record_probe_refresh_budget_exhausted(kind);
    }

    pub(super) fn runtime_metrics(&self) -> RuntimeBehaviorMetrics {
        self.runtime_metrics
    }

    pub(super) fn diagnostics_snapshot(&self) -> EventLoopDiagnostics {
        EventLoopDiagnostics {
            metrics: self.runtime_metrics(),
            last_autocmd_event_ms: self.last_autocmd_event_ms,
            last_observation_request_ms: self.last_observation_request_ms,
            callback_duration_ewma_ms: self.callback_duration_ewma_ms.max(0.0),
        }
    }
}

thread_local! {
    static EVENT_LOOP_STATE: RefCell<EventLoopState> = const { RefCell::new(EventLoopState::new()) };
}

fn with_event_loop_state<R>(mutator: impl FnOnce(&mut EventLoopState) -> R) -> R {
    EVENT_LOOP_STATE.with(|state| {
        let mut state = state.borrow_mut();
        mutator(&mut state)
    })
}

fn read_event_loop_state<R>(reader: impl FnOnce(&EventLoopState) -> R) -> R {
    EVENT_LOOP_STATE.with(|state| {
        let state = state.borrow();
        reader(&state)
    })
}

pub(super) fn note_autocmd_event(now_ms: f64) {
    with_event_loop_state(|state| state.note_autocmd_event(now_ms));
}

pub(super) fn clear_autocmd_event_timestamp() {
    with_event_loop_state(EventLoopState::clear_autocmd_event_timestamp);
}

pub(super) fn note_observation_request(now_ms: f64) {
    with_event_loop_state(|state| state.note_observation_request(now_ms));
}

pub(super) fn clear_observation_request_timestamp() {
    with_event_loop_state(EventLoopState::clear_observation_request_timestamp);
}

pub(super) fn record_cursor_callback_duration(duration_ms: f64) {
    with_event_loop_state(|state| state.record_cursor_callback_duration(duration_ms));
}

pub(super) fn clear_cursor_callback_duration_estimate() {
    with_event_loop_state(EventLoopState::clear_cursor_callback_duration_estimate);
}

pub(super) fn cursor_callback_duration_estimate_ms() -> f64 {
    read_event_loop_state(EventLoopState::cursor_callback_duration_estimate_ms)
}

pub(super) fn record_ingress_received() {
    with_event_loop_state(EventLoopState::record_ingress_received);
}

pub(super) fn record_ingress_coalesced() {
    with_event_loop_state(EventLoopState::record_ingress_coalesced);
}

pub(super) fn record_ingress_dropped() {
    with_event_loop_state(EventLoopState::record_ingress_dropped);
}

pub(super) fn record_ingress_applied() {
    with_event_loop_state(EventLoopState::record_ingress_applied);
}

pub(super) fn record_observation_request_executed() {
    with_event_loop_state(EventLoopState::record_observation_request_executed);
}

pub(super) fn record_degraded_draw_application() {
    with_event_loop_state(EventLoopState::record_degraded_draw_application);
}

pub(super) fn record_stale_token_event() {
    with_event_loop_state(EventLoopState::record_stale_token_event);
}

pub(super) fn record_timer_schedule_duration(duration_micros: u64) {
    with_event_loop_state(|state| state.record_timer_schedule_duration(duration_micros));
}

pub(super) fn record_timer_fire_duration(duration_micros: u64) {
    with_event_loop_state(|state| state.record_timer_fire_duration(duration_micros));
}

pub(super) fn record_scheduled_queue_depth(depth: usize) {
    with_event_loop_state(|state| state.record_scheduled_queue_depth(depth));
}

pub(super) fn record_probe_duration(kind: ProbeKind, duration_micros: u64) {
    with_event_loop_state(|state| state.record_probe_duration(kind, duration_micros));
}

pub(super) fn record_probe_refresh_retried(kind: ProbeKind) {
    with_event_loop_state(|state| state.record_probe_refresh_retried(kind));
}

pub(super) fn record_probe_refresh_budget_exhausted(kind: ProbeKind) {
    with_event_loop_state(|state| state.record_probe_refresh_budget_exhausted(kind));
}

pub(super) fn diagnostics_snapshot() -> EventLoopDiagnostics {
    read_event_loop_state(EventLoopState::diagnostics_snapshot)
}

#[cfg(test)]
mod tests {
    use super::{
        EventLoopState, diagnostics_snapshot, record_probe_duration,
        record_probe_refresh_budget_exhausted, record_probe_refresh_retried,
        record_scheduled_queue_depth, record_timer_fire_duration, record_timer_schedule_duration,
        with_event_loop_state,
    };
    use crate::core::state::ProbeKind;

    fn reset_event_loop_state() {
        with_event_loop_state(|state| *state = EventLoopState::new());
    }

    #[test]
    fn probe_telemetry_records_duration_and_retry_counts_per_probe_kind() {
        reset_event_loop_state();

        record_probe_duration(ProbeKind::CursorColor, 2_500);
        record_probe_refresh_retried(ProbeKind::CursorColor);
        record_probe_refresh_budget_exhausted(ProbeKind::CursorColor);
        record_probe_duration(ProbeKind::Background, 5_000);
        record_probe_refresh_retried(ProbeKind::Background);

        let diagnostics = diagnostics_snapshot();

        assert_eq!(diagnostics.metrics.cursor_color_probe.duration.samples, 1);
        assert_eq!(
            diagnostics.metrics.cursor_color_probe.duration.max_micros,
            2_500
        );
        assert_eq!(diagnostics.metrics.cursor_color_probe.refresh_retries, 1);
        assert_eq!(
            diagnostics
                .metrics
                .cursor_color_probe
                .refresh_budget_exhausted,
            1
        );
        assert_eq!(diagnostics.metrics.background_probe.duration.samples, 1);
        assert_eq!(
            diagnostics.metrics.background_probe.duration.max_micros,
            5_000
        );
        assert_eq!(diagnostics.metrics.background_probe.refresh_retries, 1);
        assert_eq!(
            diagnostics
                .metrics
                .background_probe
                .refresh_budget_exhausted,
            0
        );
    }

    #[test]
    fn timer_and_queue_telemetry_record_duration_and_depth_samples() {
        reset_event_loop_state();

        record_timer_schedule_duration(400);
        record_timer_schedule_duration(1_600);
        record_timer_fire_duration(750);
        record_scheduled_queue_depth(1);
        record_scheduled_queue_depth(3);

        let diagnostics = diagnostics_snapshot();

        assert_eq!(diagnostics.metrics.timer_schedule.samples, 2);
        assert_eq!(diagnostics.metrics.timer_schedule.total_micros, 2_000);
        assert_eq!(diagnostics.metrics.timer_schedule.max_micros, 1_600);
        assert_eq!(diagnostics.metrics.timer_fire.samples, 1);
        assert_eq!(diagnostics.metrics.timer_fire.total_micros, 750);
        assert_eq!(diagnostics.metrics.timer_fire.max_micros, 750);
        assert_eq!(diagnostics.metrics.scheduled_queue_depth.samples, 2);
        assert_eq!(diagnostics.metrics.scheduled_queue_depth.total_depth, 4);
        assert_eq!(diagnostics.metrics.scheduled_queue_depth.max_depth, 3);
    }
}
