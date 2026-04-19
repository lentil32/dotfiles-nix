use super::super::lru_cache::LruCache;

const BUFFER_PERF_TELEMETRY_CACHE_CAPACITY: usize = 32;
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
    conceal_deferred_projection_pressure: f64,
}

impl BufferPerfSignals {
    pub(in crate::events) const fn cursor_color_extmark_fallback_pressure(self) -> f64 {
        self.cursor_color_extmark_fallback_pressure
    }

    pub(in crate::events) const fn conceal_full_scan_pressure(self) -> f64 {
        self.conceal_full_scan_pressure
    }

    pub(in crate::events) const fn conceal_deferred_projection_pressure(self) -> f64 {
        self.conceal_deferred_projection_pressure
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub(in crate::events) struct BufferPerfTelemetry {
    callback_duration_estimate_ms: f64,
    cursor_color_extmark_fallback_pressure: DecayingPressureSignal,
    conceal_full_scan_pressure: DecayingPressureSignal,
    conceal_deferred_projection_pressure: DecayingPressureSignal,
}

impl BufferPerfTelemetry {
    pub(in crate::events) fn record_callback_duration(&mut self, duration_ms: f64) {
        if let Some(next_estimate_ms) = super::super::update_callback_duration_ewma(
            self.callback_duration_estimate_ms,
            duration_ms,
        ) {
            self.callback_duration_estimate_ms = next_estimate_ms;
        }
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

    pub(in crate::events) fn record_conceal_deferred_projection(&mut self, observed_at_ms: f64) {
        self.conceal_deferred_projection_pressure
            .record_event(observed_at_ms);
    }

    pub(in crate::events) fn signals_at(self, observed_at_ms: f64) -> BufferPerfSignals {
        BufferPerfSignals {
            cursor_color_extmark_fallback_pressure: self
                .cursor_color_extmark_fallback_pressure
                .value_at(observed_at_ms),
            conceal_full_scan_pressure: self.conceal_full_scan_pressure.value_at(observed_at_ms),
            conceal_deferred_projection_pressure: self
                .conceal_deferred_projection_pressure
                .value_at(observed_at_ms),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(in crate::events) struct BufferPerfTelemetryCache {
    entries: LruCache<i64, BufferPerfTelemetry>,
}

impl Default for BufferPerfTelemetryCache {
    fn default() -> Self {
        Self {
            entries: LruCache::new(BUFFER_PERF_TELEMETRY_CACHE_CAPACITY),
        }
    }
}

impl BufferPerfTelemetryCache {
    pub(in crate::events) fn telemetry(&self, buffer_handle: i64) -> Option<BufferPerfTelemetry> {
        self.entries.peek_copy(&buffer_handle)
    }

    pub(in crate::events) fn invalidate_buffer(&mut self, buffer_handle: i64) {
        let _ = self.entries.remove(&buffer_handle);
    }

    fn update_telemetry_entry(
        &mut self,
        buffer_handle: i64,
        update: impl FnOnce(&mut BufferPerfTelemetry),
    ) -> BufferPerfTelemetry {
        let mut telemetry = self.telemetry(buffer_handle).unwrap_or_default();
        update(&mut telemetry);
        self.store_telemetry(buffer_handle, telemetry);
        telemetry
    }

    pub(in crate::events) fn record_callback_duration(
        &mut self,
        buffer_handle: i64,
        duration_ms: f64,
    ) -> BufferPerfTelemetry {
        self.update_telemetry_entry(buffer_handle, |telemetry| {
            telemetry.record_callback_duration(duration_ms);
        })
    }

    pub(in crate::events) fn clear(&mut self) {
        self.entries.clear();
    }

    pub(in crate::events) fn record_cursor_color_extmark_fallback(
        &mut self,
        buffer_handle: i64,
        observed_at_ms: f64,
    ) -> BufferPerfTelemetry {
        self.update_telemetry_entry(buffer_handle, |telemetry| {
            telemetry.record_cursor_color_extmark_fallback(observed_at_ms);
        })
    }

    pub(in crate::events) fn record_conceal_full_scan(
        &mut self,
        buffer_handle: i64,
        observed_at_ms: f64,
    ) -> BufferPerfTelemetry {
        self.update_telemetry_entry(buffer_handle, |telemetry| {
            telemetry.record_conceal_full_scan(observed_at_ms);
        })
    }

    pub(in crate::events) fn record_conceal_deferred_projection(
        &mut self,
        buffer_handle: i64,
        observed_at_ms: f64,
    ) -> BufferPerfTelemetry {
        self.update_telemetry_entry(buffer_handle, |telemetry| {
            telemetry.record_conceal_deferred_projection(observed_at_ms);
        })
    }

    fn store_telemetry(&mut self, buffer_handle: i64, telemetry: BufferPerfTelemetry) {
        self.entries.insert(buffer_handle, telemetry);
    }
}

#[cfg(test)]
mod tests {
    use super::BufferPerfSignals;
    use super::BufferPerfTelemetry;
    use super::BufferPerfTelemetryCache;
    use crate::test_support::proptest::pure_config;
    use proptest::collection::vec;
    use proptest::prelude::*;
    use std::collections::BTreeMap;

    const CALLBACK_DURATION_EWMA_ALPHA: f64 = 0.25;

    #[derive(Clone, Copy, Debug)]
    enum FloatCase {
        Finite(f64),
        Nan,
        PositiveInfinity,
        NegativeInfinity,
    }

    impl FloatCase {
        fn value(self) -> f64 {
            match self {
                Self::Finite(value) => value,
                Self::Nan => f64::NAN,
                Self::PositiveInfinity => f64::INFINITY,
                Self::NegativeInfinity => f64::NEG_INFINITY,
            }
        }
    }

    fn duration_case() -> BoxedStrategy<FloatCase> {
        prop_oneof![
            (-32.0_f64..64.0_f64).prop_map(FloatCase::Finite),
            Just(FloatCase::Nan),
            Just(FloatCase::PositiveInfinity),
            Just(FloatCase::NegativeInfinity),
        ]
        .boxed()
    }

    fn timestamp_case() -> BoxedStrategy<FloatCase> {
        prop_oneof![
            (-5_000.0_f64..20_000.0_f64).prop_map(FloatCase::Finite),
            Just(FloatCase::Nan),
            Just(FloatCase::PositiveInfinity),
            Just(FloatCase::NegativeInfinity),
        ]
        .boxed()
    }

    #[derive(Clone, Copy, Debug)]
    enum CallbackOp {
        Record {
            buffer_handle: i64,
            duration_ms: FloatCase,
        },
    }

    fn callback_op() -> BoxedStrategy<CallbackOp> {
        (0_i64..4_i64, duration_case())
            .prop_map(|(buffer_handle, duration_ms)| CallbackOp::Record {
                buffer_handle,
                duration_ms,
            })
            .boxed()
    }

    fn update_callback_estimate(previous_estimate_ms: f64, duration_ms: f64) -> f64 {
        if !duration_ms.is_finite() {
            return previous_estimate_ms;
        }

        let observed_ms = duration_ms.max(0.0);
        if previous_estimate_ms <= 0.0 {
            observed_ms
        } else {
            previous_estimate_ms
                + CALLBACK_DURATION_EWMA_ALPHA * (observed_ms - previous_estimate_ms)
        }
    }

    #[derive(Clone, Copy, Debug)]
    enum PressureSignalKind {
        CursorColorExtmarkFallback,
        ConcealFullScan,
        ConcealDeferredProjection,
    }

    impl PressureSignalKind {
        fn record(self, telemetry: &mut BufferPerfTelemetry, observed_at_ms: f64) {
            match self {
                Self::CursorColorExtmarkFallback => {
                    telemetry.record_cursor_color_extmark_fallback(observed_at_ms);
                }
                Self::ConcealFullScan => telemetry.record_conceal_full_scan(observed_at_ms),
                Self::ConcealDeferredProjection => {
                    telemetry.record_conceal_deferred_projection(observed_at_ms);
                }
            }
        }
    }

    fn pressure_signal_kind() -> BoxedStrategy<PressureSignalKind> {
        prop_oneof![
            Just(PressureSignalKind::CursorColorExtmarkFallback),
            Just(PressureSignalKind::ConcealFullScan),
            Just(PressureSignalKind::ConcealDeferredProjection),
        ]
        .boxed()
    }

    #[derive(Clone, Copy, Debug, Default, PartialEq)]
    struct PressureSignalModel {
        score: f64,
        last_event_at_ms: f64,
    }

    impl PressureSignalModel {
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
            self.score * 0.5_f64.powf(elapsed_ms / super::PRESSURE_SIGNAL_HALF_LIFE_MS)
        }
    }

    #[derive(Clone, Copy, Debug, Default, PartialEq)]
    struct BufferPerfTelemetryModel {
        cursor_color_extmark_fallback_pressure: PressureSignalModel,
        conceal_full_scan_pressure: PressureSignalModel,
        conceal_deferred_projection_pressure: PressureSignalModel,
    }

    impl BufferPerfTelemetryModel {
        fn record(&mut self, kind: PressureSignalKind, observed_at_ms: f64) {
            match kind {
                PressureSignalKind::CursorColorExtmarkFallback => self
                    .cursor_color_extmark_fallback_pressure
                    .record_event(observed_at_ms),
                PressureSignalKind::ConcealFullScan => {
                    self.conceal_full_scan_pressure.record_event(observed_at_ms);
                }
                PressureSignalKind::ConcealDeferredProjection => self
                    .conceal_deferred_projection_pressure
                    .record_event(observed_at_ms),
            }
        }

        fn signals_at(self, observed_at_ms: f64) -> BufferPerfSignals {
            BufferPerfSignals {
                cursor_color_extmark_fallback_pressure: self
                    .cursor_color_extmark_fallback_pressure
                    .value_at(observed_at_ms),
                conceal_full_scan_pressure: self
                    .conceal_full_scan_pressure
                    .value_at(observed_at_ms),
                conceal_deferred_projection_pressure: self
                    .conceal_deferred_projection_pressure
                    .value_at(observed_at_ms),
            }
        }
    }

    #[derive(Clone, Copy, Debug)]
    enum PressureOp {
        Record {
            buffer_handle: i64,
            signal_kind: PressureSignalKind,
            observed_at_ms: FloatCase,
            query_at_ms: FloatCase,
        },
    }

    fn pressure_op() -> BoxedStrategy<PressureOp> {
        (
            0_i64..4_i64,
            pressure_signal_kind(),
            timestamp_case(),
            timestamp_case(),
        )
            .prop_map(
                |(buffer_handle, signal_kind, observed_at_ms, query_at_ms)| PressureOp::Record {
                    buffer_handle,
                    signal_kind,
                    observed_at_ms,
                    query_at_ms,
                },
            )
            .boxed()
    }

    proptest! {
        #![proptest_config(pure_config())]

        #[test]
        fn prop_callback_duration_ewma_cache_matches_per_buffer_reference_model(
            operations in vec(callback_op(), 1..64),
        ) {
            let mut cache = BufferPerfTelemetryCache::default();
            let mut model = BTreeMap::<i64, f64>::new();

            for operation in operations {
                match operation {
                    CallbackOp::Record {
                        buffer_handle,
                        duration_ms,
                    } => {
                        let duration_ms = duration_ms.value();
                        let estimate = model.entry(buffer_handle).or_insert(0.0);
                        *estimate = update_callback_estimate(*estimate, duration_ms);

                        let actual = cache
                            .record_callback_duration(buffer_handle, duration_ms)
                            .callback_duration_estimate_ms();
                        prop_assert_eq!(actual, *estimate);
                    }
                }

                for buffer_handle in 0_i64..4_i64 {
                    prop_assert_eq!(
                        cache
                            .telemetry(buffer_handle)
                            .map(BufferPerfTelemetry::callback_duration_estimate_ms),
                        model.get(&buffer_handle).copied(),
                    );
                }
            }
        }

        #[test]
        fn prop_pressure_signal_cache_matches_per_buffer_reference_model(
            operations in vec(pressure_op(), 1..64),
        ) {
            let mut cache = BufferPerfTelemetryCache::default();
            let mut model = BTreeMap::<i64, BufferPerfTelemetryModel>::new();

            for operation in operations {
                match operation {
                    PressureOp::Record {
                        buffer_handle,
                        signal_kind,
                        observed_at_ms,
                        query_at_ms,
                    } => {
                        let observed_at_ms = observed_at_ms.value();
                        let query_at_ms = query_at_ms.value();
                        model
                            .entry(buffer_handle)
                            .or_default()
                            .record(signal_kind, observed_at_ms);

                        let actual = match signal_kind {
                            PressureSignalKind::CursorColorExtmarkFallback => {
                                cache
                                    .record_cursor_color_extmark_fallback(
                                        buffer_handle,
                                        observed_at_ms,
                                    )
                                    .signals_at(query_at_ms)
                            }
                            PressureSignalKind::ConcealFullScan => {
                                cache
                                    .record_conceal_full_scan(buffer_handle, observed_at_ms)
                                    .signals_at(query_at_ms)
                            }
                            PressureSignalKind::ConcealDeferredProjection => {
                                cache
                                    .record_conceal_deferred_projection(
                                        buffer_handle,
                                        observed_at_ms,
                                    )
                                    .signals_at(query_at_ms)
                            }
                        };
                        prop_assert_eq!(
                            actual,
                            model
                                .get(&buffer_handle)
                                .copied()
                                .expect("the recorded buffer must exist in the reference model")
                                .signals_at(query_at_ms),
                        );

                        for compare_buffer in 0_i64..4_i64 {
                            prop_assert_eq!(
                                cache
                                    .telemetry(compare_buffer)
                                    .map(|telemetry| telemetry.signals_at(query_at_ms)),
                                model
                                    .get(&compare_buffer)
                                    .copied()
                                    .map(|telemetry| telemetry.signals_at(query_at_ms)),
                            );
                        }
                    }
                }
            }
        }

        #[test]
        fn prop_pressure_signal_decay_is_monotone_for_each_signal_kind(
            signal_kind in pressure_signal_kind(),
            first_observed_at_ms in -2_000.0_f64..2_000.0_f64,
            second_gap_ms in 0.0_f64..5_000.0_f64,
            sample_gap_ms in 0.0_f64..10_000.0_f64,
            later_gap_ms in 0.0_f64..10_000.0_f64,
        ) {
            let mut telemetry = BufferPerfTelemetry::default();
            let second_observed_at_ms = first_observed_at_ms + second_gap_ms;
            signal_kind.record(&mut telemetry, first_observed_at_ms);
            signal_kind.record(&mut telemetry, second_observed_at_ms);

            let sample_at_ms = second_observed_at_ms.max(0.0) + sample_gap_ms;
            let later_at_ms = sample_at_ms + later_gap_ms;
            let current = telemetry.signals_at(sample_at_ms);
            let later = telemetry.signals_at(later_at_ms);

            match signal_kind {
                PressureSignalKind::CursorColorExtmarkFallback => {
                    prop_assert!(
                        later.cursor_color_extmark_fallback_pressure()
                            <= current.cursor_color_extmark_fallback_pressure()
                    );
                }
                PressureSignalKind::ConcealFullScan => {
                    prop_assert!(
                        later.conceal_full_scan_pressure()
                            <= current.conceal_full_scan_pressure()
                    );
                }
                PressureSignalKind::ConcealDeferredProjection => {
                    prop_assert!(
                        later.conceal_deferred_projection_pressure()
                            <= current.conceal_deferred_projection_pressure()
                    );
                }
            }
        }
    }
}
