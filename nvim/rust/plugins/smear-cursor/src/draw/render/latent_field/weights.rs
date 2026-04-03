use super::TailBand;
use super::TailBandProfile;
use super::store::DepositedSlice;
use super::store::WeightCurveKey;
use crate::types::BASE_TIME_INTERVAL;

pub(in super::super) const WEIGHT_Q16_SCALE: u64 = 65_536;
const DEFAULT_TAIL_DURATION_MS: f64 = 198.0;
const DURATION_SCALE_MIN: f64 = 0.40;
const DURATION_SCALE_MAX: f64 = 2.50;
const DURATION_SCALE_EXPONENT: f64 = 0.85;
const SHEATH_BASE_LIFETIME_MS: f64 = 40.0;
const CORE_BASE_LIFETIME_MS: f64 = 112.0;
const FILAMENT_BASE_LIFETIME_MS: f64 = 252.0;
const SHEATH_MIN_SUPPORT_STEPS: usize = 2;
const CORE_MIN_SUPPORT_STEPS: usize = 4;
const FILAMENT_MIN_SUPPORT_STEPS: usize = 7;
const SHEATH_WIDTH_SCALE: f64 = 1.18;
const CORE_WIDTH_SCALE: f64 = 0.58;
const FILAMENT_WIDTH_SCALE: f64 = 0.28;
const SHEATH_INTENSITY: f64 = 0.90;
const CORE_INTENSITY: f64 = 0.80;
const FILAMENT_INTENSITY: f64 = 0.78;
const TAIL_WEIGHT_EXPONENT: f64 = 0.90;
const COMBINED_HEAD_MIX: f64 = 0.20;
const COMBINED_TAIL_MIX: f64 = 0.80;
const RECENT_HEAD_MIX: f64 = 0.82;
const RECENT_TAIL_MIX: f64 = 0.18;

pub(in super::super) fn tail_support_steps(tail_duration_ms: f64, simulation_hz: f64) -> usize {
    let safe_duration_ms = if tail_duration_ms.is_finite() {
        tail_duration_ms.max(1.0)
    } else {
        180.0
    };
    let step_ms = simulation_step_ms(simulation_hz);

    ((safe_duration_ms / step_ms).round() as usize).max(1)
}

pub(in super::super) fn simulation_step_ms(simulation_hz: f64) -> f64 {
    let safe_hz = if simulation_hz.is_finite() {
        simulation_hz.max(1.0)
    } else {
        120.0
    };
    1000.0 / safe_hz
}

pub(in super::super) fn q16_from_non_negative(value: f64) -> u32 {
    if !value.is_finite() {
        return 0;
    }

    let scaled = (value.max(0.0) * f64::from(1_u32 << 16)).round();
    scaled.clamp(0.0, f64::from(u32::MAX)) as u32
}

fn reference_step_weight_q16(dt_ms_q16: u32) -> u64 {
    let reference_dt_q16 = u64::from(q16_from_non_negative(BASE_TIME_INTERVAL));
    if reference_dt_q16 == 0 {
        return WEIGHT_Q16_SCALE;
    }

    u64::from(dt_ms_q16)
        .saturating_mul(WEIGHT_Q16_SCALE)
        .saturating_div(reference_dt_q16)
}

fn scale_weight_by_step_dt(weight_q16: u64, dt_ms_q16: u32) -> u64 {
    let dt_weight_q16 = reference_step_weight_q16(dt_ms_q16);
    let scaled = u128::from(weight_q16)
        .saturating_mul(u128::from(dt_weight_q16))
        .saturating_div(u128::from(WEIGHT_Q16_SCALE));
    scaled.min(u128::from(u64::MAX)) as u64
}

pub(in super::super) fn intensity_q16(intensity: f64) -> u32 {
    if !intensity.is_finite() {
        return 0;
    }
    (intensity.clamp(0.0, 1.0) * WEIGHT_Q16_SCALE as f64).round() as u32
}

fn smoothstep01(value: f64) -> f64 {
    let x = value.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

fn band_intensity_factor(intensity_q16: u32) -> f64 {
    (f64::from(intensity_q16) / WEIGHT_Q16_SCALE as f64).max(0.0)
}

fn age_weights(age_steps: u64, support_steps: usize) -> Option<(f64, f64)> {
    let support_steps_u64 = u64::try_from(support_steps).unwrap_or(u64::MAX).max(1);
    if age_steps >= support_steps_u64 {
        return None;
    }

    let normalized_age = age_steps as f64 / support_steps_u64 as f64;
    let age = normalized_age.clamp(0.0, 1.0);
    let head_weight = (1.0 - age).clamp(0.0, 1.0);
    // Keep intensity decay smoother near support expiry to reduce one-frame aging pops.
    let age_smooth = smoothstep01(age);
    let tail_weight = (1.0 - age_smooth).powf(TAIL_WEIGHT_EXPONENT);
    Some((head_weight, tail_weight))
}

pub(in super::super) fn weight_q16_for_curve(key: WeightCurveKey, age_steps: u64) -> u64 {
    let Some((head_weight, tail_weight)) = age_weights(age_steps, key.support_steps) else {
        return 0;
    };

    let combined_weight = ((COMBINED_HEAD_MIX * head_weight + COMBINED_TAIL_MIX * tail_weight)
        * band_intensity_factor(key.intensity_q16))
    .clamp(0.0, 1.0);
    scale_weight_by_step_dt(
        (combined_weight * WEIGHT_Q16_SCALE as f64).round() as u64,
        key.dt_ms_q16,
    )
}

pub(in super::super) fn recent_weight_q16_for_curve(key: WeightCurveKey, age_steps: u64) -> u64 {
    let Some((head_weight, tail_weight)) = age_weights(age_steps, key.support_steps) else {
        return 0;
    };

    let recent_weight = ((RECENT_HEAD_MIX * head_weight + RECENT_TAIL_MIX * tail_weight)
        * band_intensity_factor(key.intensity_q16))
    .clamp(0.0, 1.0);
    scale_weight_by_step_dt(
        (recent_weight * WEIGHT_Q16_SCALE as f64).round() as u64,
        key.dt_ms_q16,
    )
}

#[cfg(test)]
pub(in super::super) fn slice_weight_q16(slice: &DepositedSlice, age_steps: u64) -> u64 {
    weight_q16_for_curve(WeightCurveKey::from_slice(slice), age_steps)
}

#[cfg(test)]
pub(in super::super) fn slice_recent_weight_q16(slice: &DepositedSlice, age_steps: u64) -> u64 {
    recent_weight_q16_for_curve(WeightCurveKey::from_slice(slice), age_steps)
}

pub(in super::super) fn comet_tail_profiles(tail_duration_ms: f64) -> [TailBandProfile; 3] {
    let duration_ratio = if tail_duration_ms.is_finite() {
        (tail_duration_ms / DEFAULT_TAIL_DURATION_MS).clamp(DURATION_SCALE_MIN, DURATION_SCALE_MAX)
    } else {
        1.0
    };
    let support_scale = duration_ratio.powf(DURATION_SCALE_EXPONENT);

    [
        TailBandProfile {
            band: TailBand::Sheath,
            width_scale: SHEATH_WIDTH_SCALE,
            lifetime_ms: SHEATH_BASE_LIFETIME_MS * support_scale,
            min_support_steps: SHEATH_MIN_SUPPORT_STEPS,
            intensity: SHEATH_INTENSITY,
        },
        TailBandProfile {
            band: TailBand::Core,
            width_scale: CORE_WIDTH_SCALE,
            lifetime_ms: CORE_BASE_LIFETIME_MS * support_scale,
            min_support_steps: CORE_MIN_SUPPORT_STEPS,
            intensity: CORE_INTENSITY,
        },
        TailBandProfile {
            band: TailBand::Filament,
            width_scale: FILAMENT_WIDTH_SCALE,
            lifetime_ms: FILAMENT_BASE_LIFETIME_MS * support_scale,
            min_support_steps: FILAMENT_MIN_SUPPORT_STEPS,
            intensity: FILAMENT_INTENSITY,
        },
    ]
}

pub(in super::super) fn max_comet_support_steps(
    tail_duration_ms: f64,
    simulation_hz: f64,
) -> usize {
    comet_tail_profiles(tail_duration_ms)
        .into_iter()
        .map(|profile| profile.support_steps(simulation_hz))
        .max()
        .unwrap_or(1)
}
