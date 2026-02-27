#[cfg(test)]
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum CleanupDirective {
    KeepWarm,
    SoftClear { max_kept_windows: usize },
    HardPurge,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy)]
pub(crate) struct CleanupPolicyInput {
    pub(crate) idle_ms: u64,
    pub(crate) soft_cleanup_delay_ms: u64,
    pub(crate) hard_cleanup_delay_ms: u64,
    pub(crate) pool_total_windows: usize,
    pub(crate) recent_frame_demand: usize,
    pub(crate) max_kept_windows: usize,
    pub(crate) callback_duration_estimate_ms: f64,
}

pub(crate) fn as_delay_ms(value: f64) -> u64 {
    let clamped = if value.is_finite() {
        value.max(0.0).floor()
    } else {
        0.0
    };
    if clamped > u64::MAX as f64 {
        u64::MAX
    } else {
        clamped as u64
    }
}

#[cfg(test)]
fn callback_penalty_ms(input: CleanupPolicyInput) -> u64 {
    as_delay_ms(input.callback_duration_estimate_ms * 2.0)
}

#[cfg(test)]
fn demand_penalty_ms(input: CleanupPolicyInput) -> u64 {
    u64::try_from(input.recent_frame_demand).unwrap_or(u64::MAX) / 4
}

#[cfg(test)]
pub(crate) fn keep_warm_until_ms(input: CleanupPolicyInput) -> u64 {
    input
        .soft_cleanup_delay_ms
        .saturating_add(callback_penalty_ms(input))
        .saturating_add(demand_penalty_ms(input))
}

#[cfg(test)]
pub(crate) fn decide_cleanup_directive(input: CleanupPolicyInput) -> CleanupDirective {
    if input.pool_total_windows == 0 {
        return CleanupDirective::KeepWarm;
    }

    let keep_warm_until_ms = keep_warm_until_ms(input);
    if input.idle_ms < keep_warm_until_ms {
        return CleanupDirective::KeepWarm;
    }

    if input.idle_ms < input.hard_cleanup_delay_ms {
        return CleanupDirective::SoftClear {
            max_kept_windows: input.max_kept_windows,
        };
    }

    CleanupDirective::HardPurge
}

#[cfg(test)]
pub(crate) fn next_cleanup_check_delay_ms(input: CleanupPolicyInput) -> Option<u64> {
    if input.pool_total_windows == 0 {
        return None;
    }

    match decide_cleanup_directive(input) {
        CleanupDirective::KeepWarm => Some(
            keep_warm_until_ms(input)
                .saturating_sub(input.idle_ms)
                .max(1),
        ),
        CleanupDirective::SoftClear { .. } => Some(
            input
                .hard_cleanup_delay_ms
                .saturating_sub(input.idle_ms)
                .max(1),
        ),
        CleanupDirective::HardPurge => None,
    }
}

pub(crate) fn render_cleanup_delay_ms(config: &RuntimeConfig) -> u64 {
    let baseline =
        as_delay_ms(config.time_interval + config.delay_event_to_smear + config.delay_after_key);
    baseline.max(MIN_RENDER_CLEANUP_DELAY_MS)
}

pub(crate) fn render_hard_cleanup_delay_ms(config: &RuntimeConfig) -> u64 {
    let soft_delay = render_cleanup_delay_ms(config);
    let scaled = soft_delay.saturating_mul(RENDER_HARD_PURGE_DELAY_MULTIPLIER);
    scaled.max(MIN_RENDER_HARD_PURGE_DELAY_MS)
}
use crate::config::RuntimeConfig;

pub(crate) const MIN_RENDER_CLEANUP_DELAY_MS: u64 = 200;
pub(crate) const MIN_RENDER_HARD_PURGE_DELAY_MS: u64 = 3_000;
pub(crate) const RENDER_HARD_PURGE_DELAY_MULTIPLIER: u64 = 8;
