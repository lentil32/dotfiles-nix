use std::collections::VecDeque;

const BUFFER_PERF_TELEMETRY_CACHE_CAPACITY: usize = 32;
const CALLBACK_DURATION_EWMA_ALPHA: f64 = 0.25;
const PRESSURE_SIGNAL_HALF_LIFE_MS: f64 = 5_000.0;

#[derive(Debug, Default, Clone, Copy, PartialEq)]
struct DecayingPressureSignal {
    score: f64,
    last_event_at_ms: f64,
}

impl DecayingPressureSignal {
    fn record_event(&mut self, observed_at_ms: f64) {
        if !observed_at_ms.is_finite() {
            return;
        }

        let observed_at_ms = observed_at_ms.max(0.0);
        self.score = self.value_at(observed_at_ms) + 1.0;
        self.last_event_at_ms = observed_at_ms;
    }

    fn value_at(self, observed_at_ms: f64) -> f64 {
        if self.score <= 0.0 {
            return 0.0;
        }

        if !observed_at_ms.is_finite() {
            return self.score.max(0.0);
        }

        let elapsed_ms = (observed_at_ms.max(0.0) - self.last_event_at_ms.max(0.0)).max(0.0);
        self.score * 0.5_f64.powf(elapsed_ms / PRESSURE_SIGNAL_HALF_LIFE_MS)
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub(in crate::events) struct BufferPerfSignals {
    cursor_color_extmark_fallback_pressure: f64,
    conceal_full_scan_pressure: f64,
    conceal_raw_screenpos_fallback_pressure: f64,
}

impl BufferPerfSignals {
    pub(in crate::events) const fn cursor_color_extmark_fallback_pressure(self) -> f64 {
        self.cursor_color_extmark_fallback_pressure
    }

    pub(in crate::events) const fn conceal_full_scan_pressure(self) -> f64 {
        self.conceal_full_scan_pressure
    }

    pub(in crate::events) const fn conceal_raw_screenpos_fallback_pressure(self) -> f64 {
        self.conceal_raw_screenpos_fallback_pressure
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub(in crate::events) struct BufferPerfTelemetry {
    callback_duration_estimate_ms: f64,
    cursor_color_extmark_fallback_pressure: DecayingPressureSignal,
    conceal_full_scan_pressure: DecayingPressureSignal,
    conceal_raw_screenpos_fallback_pressure: DecayingPressureSignal,
}

impl BufferPerfTelemetry {
    pub(in crate::events) fn record_callback_duration(&mut self, duration_ms: f64) {
        if !duration_ms.is_finite() {
            return;
        }

        let observed = duration_ms.max(0.0);
        let previous = self.callback_duration_estimate_ms;
        self.callback_duration_estimate_ms = if previous <= 0.0 {
            observed
        } else {
            previous + CALLBACK_DURATION_EWMA_ALPHA * (observed - previous)
        };
    }

    pub(in crate::events) const fn callback_duration_estimate_ms(self) -> f64 {
        self.callback_duration_estimate_ms
    }

    pub(in crate::events) fn record_cursor_color_extmark_fallback(&mut self, observed_at_ms: f64) {
        self.cursor_color_extmark_fallback_pressure
            .record_event(observed_at_ms);
    }

    pub(in crate::events) fn record_conceal_full_scan(&mut self, observed_at_ms: f64) {
        self.conceal_full_scan_pressure.record_event(observed_at_ms);
    }

    pub(in crate::events) fn record_conceal_raw_screenpos_fallback(&mut self, observed_at_ms: f64) {
        self.conceal_raw_screenpos_fallback_pressure
            .record_event(observed_at_ms);
    }

    pub(in crate::events) fn signals_at(self, observed_at_ms: f64) -> BufferPerfSignals {
        BufferPerfSignals {
            cursor_color_extmark_fallback_pressure: self
                .cursor_color_extmark_fallback_pressure
                .value_at(observed_at_ms),
            conceal_full_scan_pressure: self.conceal_full_scan_pressure.value_at(observed_at_ms),
            conceal_raw_screenpos_fallback_pressure: self
                .conceal_raw_screenpos_fallback_pressure
                .value_at(observed_at_ms),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct BufferPerfTelemetryCacheEntry {
    buffer_handle: i64,
    telemetry: BufferPerfTelemetry,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub(in crate::events) struct BufferPerfTelemetryCache {
    entries: VecDeque<BufferPerfTelemetryCacheEntry>,
}

impl BufferPerfTelemetryCache {
    pub(in crate::events) fn telemetry(&self, buffer_handle: i64) -> Option<BufferPerfTelemetry> {
        self.entries
            .iter()
            .find(|entry| entry.buffer_handle == buffer_handle)
            .map(|entry| entry.telemetry)
    }

    pub(in crate::events) fn record_callback_duration(
        &mut self,
        buffer_handle: i64,
        duration_ms: f64,
    ) -> BufferPerfTelemetry {
        let mut telemetry = self.telemetry(buffer_handle).unwrap_or_default();
        telemetry.record_callback_duration(duration_ms);
        self.store_telemetry(buffer_handle, telemetry);
        telemetry
    }

    pub(in crate::events) fn clear(&mut self) {
        self.entries.clear();
    }

    pub(in crate::events) fn record_cursor_color_extmark_fallback(
        &mut self,
        buffer_handle: i64,
        observed_at_ms: f64,
    ) -> BufferPerfTelemetry {
        let mut telemetry = self.telemetry(buffer_handle).unwrap_or_default();
        telemetry.record_cursor_color_extmark_fallback(observed_at_ms);
        self.store_telemetry(buffer_handle, telemetry);
        telemetry
    }

    pub(in crate::events) fn record_conceal_full_scan(
        &mut self,
        buffer_handle: i64,
        observed_at_ms: f64,
    ) -> BufferPerfTelemetry {
        let mut telemetry = self.telemetry(buffer_handle).unwrap_or_default();
        telemetry.record_conceal_full_scan(observed_at_ms);
        self.store_telemetry(buffer_handle, telemetry);
        telemetry
    }

    pub(in crate::events) fn record_conceal_raw_screenpos_fallback(
        &mut self,
        buffer_handle: i64,
        observed_at_ms: f64,
    ) -> BufferPerfTelemetry {
        let mut telemetry = self.telemetry(buffer_handle).unwrap_or_default();
        telemetry.record_conceal_raw_screenpos_fallback(observed_at_ms);
        self.store_telemetry(buffer_handle, telemetry);
        telemetry
    }

    fn store_telemetry(&mut self, buffer_handle: i64, telemetry: BufferPerfTelemetry) {
        if let Some(existing_index) = self
            .entries
            .iter()
            .position(|entry| entry.buffer_handle == buffer_handle)
        {
            let _ = self.entries.remove(existing_index);
        }

        self.entries.push_front(BufferPerfTelemetryCacheEntry {
            buffer_handle,
            telemetry,
        });
        while self.entries.len() > BUFFER_PERF_TELEMETRY_CACHE_CAPACITY {
            let _ = self.entries.pop_back();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::BufferPerfSignals;
    use super::BufferPerfTelemetry;
    use super::BufferPerfTelemetryCache;
    use pretty_assertions::assert_eq;

    #[test]
    fn callback_duration_ewma_tracks_each_buffer_independently() {
        let mut cache = BufferPerfTelemetryCache::default();

        assert_eq!(
            cache
                .record_callback_duration(11, 16.0)
                .callback_duration_estimate_ms(),
            16.0
        );
        assert_eq!(
            cache
                .record_callback_duration(22, 4.0)
                .callback_duration_estimate_ms(),
            4.0
        );
        assert_eq!(
            cache
                .record_callback_duration(11, 8.0)
                .callback_duration_estimate_ms(),
            14.0
        );
        assert_eq!(
            cache
                .telemetry(11)
                .expect("buffer 11 telemetry should exist")
                .callback_duration_estimate_ms(),
            14.0,
        );
        assert_eq!(
            cache
                .telemetry(22)
                .expect("buffer 22 telemetry should exist")
                .callback_duration_estimate_ms(),
            4.0,
        );
    }

    #[test]
    fn callback_duration_ewma_ignores_non_finite_values() {
        let mut telemetry = BufferPerfTelemetry::default();

        telemetry.record_callback_duration(8.0);
        telemetry.record_callback_duration(f64::NAN);
        telemetry.record_callback_duration(f64::INFINITY);

        assert_eq!(telemetry.callback_duration_estimate_ms(), 8.0);
    }

    #[test]
    fn pressure_signals_track_each_buffer_independently() {
        let mut cache = BufferPerfTelemetryCache::default();

        assert_eq!(
            cache
                .record_cursor_color_extmark_fallback(11, 1_000.0)
                .signals_at(1_000.0),
            BufferPerfSignals {
                cursor_color_extmark_fallback_pressure: 1.0,
                conceal_full_scan_pressure: 0.0,
                conceal_raw_screenpos_fallback_pressure: 0.0,
            },
        );
        assert_eq!(
            cache
                .record_conceal_full_scan(22, 1_000.0)
                .signals_at(1_000.0),
            BufferPerfSignals {
                cursor_color_extmark_fallback_pressure: 0.0,
                conceal_full_scan_pressure: 1.0,
                conceal_raw_screenpos_fallback_pressure: 0.0,
            },
        );
        assert_eq!(
            cache
                .telemetry(11)
                .expect("buffer 11 telemetry should exist")
                .signals_at(1_000.0),
            BufferPerfSignals {
                cursor_color_extmark_fallback_pressure: 1.0,
                conceal_full_scan_pressure: 0.0,
                conceal_raw_screenpos_fallback_pressure: 0.0,
            },
        );
        assert_eq!(
            cache
                .telemetry(22)
                .expect("buffer 22 telemetry should exist")
                .signals_at(1_000.0),
            BufferPerfSignals {
                cursor_color_extmark_fallback_pressure: 0.0,
                conceal_full_scan_pressure: 1.0,
                conceal_raw_screenpos_fallback_pressure: 0.0,
            },
        );
    }

    #[test]
    fn pressure_signals_decay_after_quiet_periods() {
        let mut telemetry = BufferPerfTelemetry::default();

        telemetry.record_conceal_full_scan(1_000.0);
        telemetry.record_conceal_full_scan(1_000.0);

        assert_eq!(
            telemetry.signals_at(6_000.0),
            BufferPerfSignals {
                cursor_color_extmark_fallback_pressure: 0.0,
                conceal_full_scan_pressure: 1.0,
                conceal_raw_screenpos_fallback_pressure: 0.0,
            },
        );
    }

    #[test]
    fn pressure_signals_ignore_non_finite_timestamps() {
        let mut telemetry = BufferPerfTelemetry::default();

        telemetry.record_cursor_color_extmark_fallback(1_000.0);
        telemetry.record_cursor_color_extmark_fallback(f64::NAN);
        telemetry.record_cursor_color_extmark_fallback(f64::INFINITY);

        assert_eq!(
            telemetry.signals_at(1_000.0),
            BufferPerfSignals {
                cursor_color_extmark_fallback_pressure: 1.0,
                conceal_full_scan_pressure: 0.0,
                conceal_raw_screenpos_fallback_pressure: 0.0,
            },
        );
    }
}
