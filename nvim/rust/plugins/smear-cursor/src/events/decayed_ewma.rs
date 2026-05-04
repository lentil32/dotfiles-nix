const CALLBACK_DURATION_EWMA_ALPHA: f64 = 0.25;
// Slow callbacks are burst pressure, not a permanent buffer property. Decay lets auto mode recover
// after the burst while still reacting quickly to sustained expensive rendering.
const CALLBACK_DURATION_DECAY_HALF_LIFE_MS: f64 = 5_000.0;
const PRESSURE_SIGNAL_DECAY_HALF_LIFE_MS: f64 = 5_000.0;

const _: () = assert!(CALLBACK_DURATION_EWMA_ALPHA >= 0.0 && CALLBACK_DURATION_EWMA_ALPHA <= 1.0);
const _: () = assert!(
    CALLBACK_DURATION_DECAY_HALF_LIFE_MS > 0.0 && CALLBACK_DURATION_DECAY_HALF_LIFE_MS <= f64::MAX
);
const _: () = assert!(
    PRESSURE_SIGNAL_DECAY_HALF_LIFE_MS > 0.0 && PRESSURE_SIGNAL_DECAY_HALF_LIFE_MS <= f64::MAX
);

#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::events) struct NonNegativeFiniteMs(f64);

impl NonNegativeFiniteMs {
    pub(in crate::events) fn new(value_ms: f64) -> Option<Self> {
        value_ms.is_finite().then(|| Self(value_ms.max(0.0)))
    }

    pub(in crate::events) const fn get(self) -> f64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::events) struct PositiveFiniteMs(f64);

impl PositiveFiniteMs {
    pub(in crate::events) fn new(value_ms: f64) -> Option<Self> {
        (value_ms.is_finite() && value_ms > 0.0).then_some(Self(value_ms))
    }

    pub(in crate::events) const fn get(self) -> f64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::events) struct TelemetryInstantMs(f64);

impl TelemetryInstantMs {
    pub(in crate::events) const ZERO: Self = Self(0.0);

    pub(in crate::events) fn new(value_ms: f64) -> Option<Self> {
        value_ms.is_finite().then(|| Self(value_ms.max(0.0)))
    }

    pub(in crate::events) fn saturating_from(value_ms: f64) -> Self {
        Self::new(value_ms).unwrap_or(Self::ZERO)
    }

    pub(in crate::events) const fn get(self) -> f64 {
        self.0
    }

    pub(in crate::events) fn elapsed_since(self, earlier: Self) -> NonNegativeFiniteMs {
        NonNegativeFiniteMs((self.0 - earlier.0).max(0.0))
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::events) struct UnitInterval(f64);

impl UnitInterval {
    pub(in crate::events) fn new(value: f64) -> Option<Self> {
        (value.is_finite() && (0.0..=1.0).contains(&value)).then_some(Self(value))
    }

    const fn get(self) -> f64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::events) struct DecayKernel {
    half_life: PositiveFiniteMs,
}

impl DecayKernel {
    pub(in crate::events) const fn new(half_life: PositiveFiniteMs) -> Self {
        Self { half_life }
    }

    pub(in crate::events) fn decay_factor(
        self,
        recorded_at: TelemetryInstantMs,
        query_at: TelemetryInstantMs,
    ) -> f64 {
        let elapsed_ms = query_at.elapsed_since(recorded_at).get();
        f64::exp2(-elapsed_ms / self.half_life.get())
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::events) struct DecayedSample {
    value: f64,
    recorded_at: TelemetryInstantMs,
}

impl DecayedSample {
    pub(in crate::events) fn non_negative(
        value: f64,
        recorded_at: TelemetryInstantMs,
    ) -> Option<Self> {
        value.is_finite().then(|| Self {
            value: value.max(0.0),
            recorded_at,
        })
    }

    pub(in crate::events) fn value_at(
        self,
        kernel: DecayKernel,
        query_at: TelemetryInstantMs,
    ) -> f64 {
        self.value * kernel.decay_factor(self.recorded_at, query_at)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::events) struct DecayedEwma {
    sample: Option<DecayedSample>,
    alpha: UnitInterval,
    kernel: DecayKernel,
}

impl DecayedEwma {
    pub(in crate::events) fn callback_duration() -> Self {
        Self::new(
            UnitInterval(CALLBACK_DURATION_EWMA_ALPHA),
            PositiveFiniteMs(CALLBACK_DURATION_DECAY_HALF_LIFE_MS),
        )
    }

    pub(in crate::events) const fn new(alpha: UnitInterval, half_life: PositiveFiniteMs) -> Self {
        Self {
            sample: None,
            alpha,
            kernel: DecayKernel::new(half_life),
        }
    }

    pub(in crate::events) fn record_at(
        &mut self,
        duration: NonNegativeFiniteMs,
        observed_at: TelemetryInstantMs,
    ) {
        let observed_ms = duration.get();
        let estimate_ms = match self.value_at(observed_at) {
            Some(previous_estimate_ms) => {
                previous_estimate_ms + self.alpha.get() * (observed_ms - previous_estimate_ms)
            }
            None => observed_ms,
        };
        self.sample = DecayedSample::non_negative(estimate_ms, observed_at);
    }

    pub(in crate::events) fn value_at(&self, query_at: TelemetryInstantMs) -> Option<f64> {
        self.sample
            .map(|sample| sample.value_at(self.kernel, query_at))
    }

    pub(in crate::events) fn value_at_ms(&self, query_at_ms: f64) -> Option<f64> {
        let query_at = TelemetryInstantMs::new(query_at_ms)
            .or_else(|| self.sample.map(|sample| sample.recorded_at))?;
        self.value_at(query_at)
    }

    pub(in crate::events) fn clear(&mut self) {
        self.sample = None;
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::events) struct DecayedCounter {
    sample: Option<DecayedSample>,
    kernel: DecayKernel,
}

impl DecayedCounter {
    pub(in crate::events) const fn new(half_life: PositiveFiniteMs) -> Self {
        Self {
            sample: None,
            kernel: DecayKernel::new(half_life),
        }
    }

    pub(in crate::events) const fn pressure_signal() -> Self {
        Self::new(PositiveFiniteMs(PRESSURE_SIGNAL_DECAY_HALF_LIFE_MS))
    }

    pub(in crate::events) fn record_at(&mut self, observed_at: TelemetryInstantMs) {
        let score = self.value_at(observed_at) + 1.0;
        self.sample = DecayedSample::non_negative(score, observed_at);
    }

    pub(in crate::events) fn value_at(&self, query_at: TelemetryInstantMs) -> f64 {
        self.sample
            .map(|sample| sample.value_at(self.kernel, query_at))
            .unwrap_or(0.0)
    }

    pub(in crate::events) fn value_at_ms(&self, query_at_ms: f64) -> f64 {
        match TelemetryInstantMs::new(query_at_ms) {
            Some(query_at) => self.value_at(query_at),
            None => self.sample.map(|sample| sample.value).unwrap_or(0.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::DecayedEwma;
    use super::NonNegativeFiniteMs;
    use super::PositiveFiniteMs;
    use super::TelemetryInstantMs;
    use super::UnitInterval;

    fn duration(value_ms: f64) -> NonNegativeFiniteMs {
        NonNegativeFiniteMs::new(value_ms).expect("test duration must be finite")
    }

    fn instant(value_ms: f64) -> TelemetryInstantMs {
        TelemetryInstantMs::new(value_ms).expect("test instant must be finite")
    }

    #[test]
    fn ewma_distinguishes_no_sample_from_zero_ms_sample() {
        let mut ewma = DecayedEwma::new(
            UnitInterval::new(0.25).expect("test alpha must be valid"),
            PositiveFiniteMs::new(5_000.0).expect("test half-life must be valid"),
        );

        assert_eq!(ewma.value_at(instant(0.0)), None);

        ewma.record_at(duration(0.0), instant(100.0));
        assert_eq!(ewma.value_at(instant(100.0)), Some(0.0));

        ewma.record_at(duration(16.0), instant(100.0));
        assert_eq!(ewma.value_at(instant(100.0)), Some(4.0));
    }

    #[test]
    fn ewma_rejects_invalid_decay_parameters_at_the_boundary() {
        assert_eq!(UnitInterval::new(f64::NAN), None);
        assert_eq!(UnitInterval::new(-0.1), None);
        assert_eq!(UnitInterval::new(1.1), None);
        assert_eq!(PositiveFiniteMs::new(0.0), None);
        assert_eq!(PositiveFiniteMs::new(f64::INFINITY), None);
    }
}
