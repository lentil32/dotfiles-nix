use super::HighlightLevel;

pub(super) fn round_lua(value: f64) -> i64 {
    (value + 0.5).floor() as i64
}

pub(super) fn level_from_shade(shade: f64, color_levels: u32) -> Option<HighlightLevel> {
    if !shade.is_finite() || color_levels == 0 {
        return None;
    }

    let rounded = round_lua(shade * f64::from(color_levels));
    if rounded <= 0 {
        None
    } else {
        let clamped = rounded.min(i64::from(color_levels));
        let value = u32::try_from(clamped).ok()?;
        HighlightLevel::try_new(value)
    }
}
