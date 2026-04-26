use crate::core::effect::CursorColorFallbackMode;
use crate::core::effect::CursorColorReuseMode;
use crate::core::effect::CursorPositionProbeMode;
use crate::core::effect::ProbePolicy;
use crate::core::effect::ProbeQuality;
use pretty_assertions::assert_eq;

pub(crate) fn assert_tracking_consistent<T>(bookkeeping: &T, lifecycle_truth: &T)
where
    T: PartialEq + std::fmt::Debug,
{
    assert_eq!(
        bookkeeping, lifecycle_truth,
        "render window bookkeeping drifted from lifecycle truth"
    );
}

pub(crate) fn assert_probe_policy_shape(
    policy: ProbePolicy,
    quality: ProbeQuality,
    cursor_position_mode: CursorPositionProbeMode,
    cursor_color_reuse_mode: CursorColorReuseMode,
    cursor_color_fallback_mode: CursorColorFallbackMode,
) {
    assert_eq!(policy.quality(), quality);
    assert_eq!(policy.cursor_position_mode(), cursor_position_mode);
    assert_eq!(policy.cursor_color_reuse_mode(), cursor_color_reuse_mode);
    assert_eq!(
        policy.cursor_color_fallback_mode(),
        cursor_color_fallback_mode
    );
    assert_eq!(
        policy.allows_compatible_cursor_color_reuse(),
        matches!(
            cursor_color_reuse_mode,
            CursorColorReuseMode::CompatibleWithinLine
        )
    );
    assert_eq!(
        policy.allows_cursor_color_extmark_fallback(),
        matches!(
            cursor_color_fallback_mode,
            CursorColorFallbackMode::SyntaxThenExtmarks
        )
    );
    assert_eq!(
        policy.allows_deferred_cursor_projection(),
        matches!(
            cursor_position_mode,
            CursorPositionProbeMode::DeferredAllowed
        )
    );
}
