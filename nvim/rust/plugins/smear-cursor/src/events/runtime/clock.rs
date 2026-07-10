//! Process-lifetime logical time that stays aligned with host timers across system suspend.
//!
//! On Apple platforms Rust's monotonic clock can exclude sleep while libuv timer deadlines include
//! it. Normal samples therefore follow `Instant`, but a large wall/monotonic skew advances by the
//! wall delta once so the existing animation-discontinuity recovery sees the missed interval.
//! Negative wall-clock skew is carried forward as debt, preventing a later clock correction from
//! being mistaken for suspend.

use crate::core::types::Millis;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::sync::PoisonError;
use std::time::Duration;
use std::time::Instant;
use std::time::SystemTime;

const LOGICAL_CLOCK_OFFSET_MS: f64 = 1.0;
const SUSPEND_SKEW_THRESHOLD: Duration = Duration::from_secs(/*secs*/ 1);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WallClockDelta {
    Forward(Duration),
    Regressed(Duration),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ClockDeltas {
    monotonic: Duration,
    wall: WallClockDelta,
}

#[derive(Debug, Default)]
struct LogicalClock {
    elapsed: Duration,
    wall_skew_debt: Duration,
}

impl LogicalClock {
    fn advance(&mut self, deltas: ClockDeltas) -> f64 {
        let elapsed = match deltas.wall {
            WallClockDelta::Forward(wall) if wall < deltas.monotonic => {
                self.wall_skew_debt = self.wall_skew_debt.saturating_add(deltas.monotonic - wall);
                deltas.monotonic
            }
            WallClockDelta::Forward(wall) => {
                let forward_skew = wall - deltas.monotonic;
                let unexplained_skew = forward_skew.saturating_sub(self.wall_skew_debt);
                self.wall_skew_debt = self.wall_skew_debt.saturating_sub(forward_skew);
                if unexplained_skew >= SUSPEND_SKEW_THRESHOLD {
                    deltas.monotonic.saturating_add(unexplained_skew)
                } else {
                    deltas.monotonic
                }
            }
            WallClockDelta::Regressed(regression) => {
                self.wall_skew_debt = self
                    .wall_skew_debt
                    .saturating_add(deltas.monotonic)
                    .saturating_add(regression);
                deltas.monotonic
            }
        };
        self.elapsed = self.elapsed.saturating_add(elapsed);
        self.elapsed.as_secs_f64() * 1000.0 + LOGICAL_CLOCK_OFFSET_MS
    }
}

#[derive(Debug)]
struct RuntimeClock {
    last_monotonic: Instant,
    last_wall: SystemTime,
    logical: LogicalClock,
}

impl RuntimeClock {
    fn new() -> Self {
        Self {
            last_monotonic: Instant::now(),
            last_wall: SystemTime::now(),
            logical: LogicalClock::default(),
        }
    }

    fn sample_now(&mut self) -> f64 {
        let monotonic = Instant::now();
        let wall = SystemTime::now();
        let deltas = ClockDeltas {
            monotonic: monotonic.saturating_duration_since(self.last_monotonic),
            wall: match wall.duration_since(self.last_wall) {
                Ok(delta) => WallClockDelta::Forward(delta),
                Err(error) => WallClockDelta::Regressed(error.duration()),
            },
        };
        self.last_monotonic = monotonic;
        self.last_wall = wall;
        self.logical.advance(deltas)
    }
}

pub(crate) fn to_core_millis(value_ms: f64) -> Millis {
    if !value_ms.is_finite() || value_ms <= 0.0 {
        return Millis::new(/*value*/ 0);
    }
    let Ok(duration) = Duration::try_from_secs_f64(value_ms / 1000.0) else {
        return Millis::new(u64::MAX);
    };
    Millis::new(u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
}

pub(crate) fn duration_to_micros(duration: Duration) -> u64 {
    u64::try_from(duration.as_micros()).unwrap_or(u64::MAX)
}

pub(crate) fn now_ms() -> f64 {
    static CLOCK: OnceLock<Mutex<RuntimeClock>> = OnceLock::new();
    let clock = CLOCK.get_or_init(|| Mutex::new(RuntimeClock::new()));
    let mut clock = clock.lock().unwrap_or_else(PoisonError::into_inner);
    clock.sample_now()
}

#[cfg(test)]
mod tests {
    use super::ClockDeltas;
    use super::LogicalClock;
    use super::WallClockDelta;
    use pretty_assertions::assert_eq;
    use std::time::Duration;

    #[test]
    fn normal_progress_uses_monotonic_deltas() {
        let mut clock = LogicalClock::default();

        let observed = [
            clock.advance(ClockDeltas {
                monotonic: Duration::from_secs(/*secs*/ 1),
                wall: WallClockDelta::Forward(Duration::from_millis(/*millis*/ 1_500)),
            }),
            clock.advance(ClockDeltas {
                monotonic: Duration::from_secs(/*secs*/ 2),
                wall: WallClockDelta::Forward(Duration::from_secs(/*secs*/ 2)),
            }),
        ];

        assert_eq!(observed, [1_001.0, 3_001.0]);
    }

    #[test]
    fn multi_hour_suspend_uses_wall_delta_then_returns_to_monotonic_progress() {
        let mut clock = LogicalClock::default();

        let observed = [
            clock.advance(ClockDeltas {
                monotonic: Duration::from_secs(/*secs*/ 1),
                wall: WallClockDelta::Forward(Duration::from_secs(/*secs*/ 1)),
            }),
            clock.advance(ClockDeltas {
                monotonic: Duration::from_secs(/*secs*/ 2),
                wall: WallClockDelta::Forward(Duration::from_secs(/*secs*/ 4 * 60 * 60 + 2)),
            }),
            clock.advance(ClockDeltas {
                monotonic: Duration::from_secs(/*secs*/ 3),
                wall: WallClockDelta::Forward(Duration::from_secs(/*secs*/ 3)),
            }),
        ];

        assert_eq!(observed, [1_001.0, 14_403_001.0, 14_406_001.0]);
    }

    #[test]
    fn wall_clock_rollback_falls_back_to_monotonic_progress() {
        let mut clock = LogicalClock::default();

        let observed = [
            clock.advance(ClockDeltas {
                monotonic: Duration::from_secs(/*secs*/ 2),
                wall: WallClockDelta::Forward(Duration::from_secs(/*secs*/ 2)),
            }),
            clock.advance(ClockDeltas {
                monotonic: Duration::from_secs(/*secs*/ 3),
                wall: WallClockDelta::Regressed(Duration::from_secs(/*secs*/ 7)),
            }),
        ];

        assert_eq!(observed, [2_001.0, 5_001.0]);
    }

    #[test]
    fn forward_correction_after_rollback_does_not_jump_logical_time() {
        let mut clock = LogicalClock::default();

        let observed = [
            clock.advance(ClockDeltas {
                monotonic: Duration::from_secs(/*secs*/ 2),
                wall: WallClockDelta::Forward(Duration::from_secs(/*secs*/ 2)),
            }),
            clock.advance(ClockDeltas {
                monotonic: Duration::from_secs(/*secs*/ 3),
                wall: WallClockDelta::Regressed(Duration::from_secs(/*secs*/ 7)),
            }),
            clock.advance(ClockDeltas {
                monotonic: Duration::from_secs(/*secs*/ 1),
                wall: WallClockDelta::Forward(Duration::from_secs(/*secs*/ 11)),
            }),
            clock.advance(ClockDeltas {
                monotonic: Duration::from_secs(/*secs*/ 2),
                wall: WallClockDelta::Forward(Duration::from_secs(/*secs*/ 2)),
            }),
        ];

        assert_eq!(observed, [2_001.0, 5_001.0, 6_001.0, 8_001.0]);
    }

    #[test]
    fn genuine_suspend_during_rollback_correction_uses_remaining_wall_skew() {
        let mut clock = LogicalClock::default();

        let observed = [
            clock.advance(ClockDeltas {
                monotonic: Duration::from_secs(/*secs*/ 1),
                wall: WallClockDelta::Forward(Duration::from_secs(/*secs*/ 1)),
            }),
            clock.advance(ClockDeltas {
                monotonic: Duration::from_secs(/*secs*/ 2),
                wall: WallClockDelta::Regressed(Duration::from_secs(/*secs*/ 8)),
            }),
            clock.advance(ClockDeltas {
                monotonic: Duration::from_secs(/*secs*/ 1),
                wall: WallClockDelta::Forward(Duration::from_secs(/*secs*/ 4 * 60 * 60 + 11)),
            }),
        ];

        assert_eq!(observed, [1_001.0, 3_001.0, 14_404_001.0]);
    }
}
