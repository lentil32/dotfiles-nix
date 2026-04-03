mod telemetry;

use super::cursor::BufferMetadata;
use super::runtime::IngressReadSnapshot;
use crate::config::BufferPerfMode;
pub(super) use crate::core::state::BufferPerfClass;
#[cfg(test)]
use crate::types::ScreenCell;
use nvim_oxi::Result;
use nvim_oxi::api;
use std::collections::VecDeque;
pub(super) use telemetry::BufferPerfSignals;
pub(super) use telemetry::BufferPerfTelemetry;
pub(super) use telemetry::BufferPerfTelemetryCache;

// These thresholds stay intentionally conservative. Auto mode should only degrade after repeated
// buffer-local pressure, not after a single expensive probe edge.
const FAST_MOTION_LINE_COUNT_ENTER_THRESHOLD: usize = 20_000;
const FAST_MOTION_LINE_COUNT_EXIT_THRESHOLD: usize = 16_000;
const FAST_MOTION_CALLBACK_MS_ENTER_THRESHOLD: f64 = 8.0;
const FAST_MOTION_CALLBACK_MS_EXIT_THRESHOLD: f64 = 6.0;
const FAST_MOTION_EXTMARK_PRESSURE_ENTER_THRESHOLD: f64 = 2.0;
const FAST_MOTION_EXTMARK_PRESSURE_EXIT_THRESHOLD: f64 = 1.0;
const FAST_MOTION_CONCEAL_SCAN_PRESSURE_ENTER_THRESHOLD: f64 = 2.0;
const FAST_MOTION_CONCEAL_SCAN_PRESSURE_EXIT_THRESHOLD: f64 = 1.0;
const FAST_MOTION_CONCEAL_RAW_PRESSURE_ENTER_THRESHOLD: f64 = 2.0;
const FAST_MOTION_CONCEAL_RAW_PRESSURE_EXIT_THRESHOLD: f64 = 1.0;
const BUFFER_EVENT_POLICY_CACHE_CAPACITY: usize = 32;

fn threshold_active_usize(value: usize, previous_active: bool, enter: usize, exit: usize) -> bool {
    if previous_active {
        value >= exit
    } else {
        value >= enter
    }
}

fn threshold_active_f64(value: f64, previous_active: bool, enter: f64, exit: f64) -> bool {
    if previous_active {
        value >= exit
    } else {
        value >= enter
    }
}

fn unsupported_buftype(buftype: &str, buflisted: bool) -> bool {
    matches!(buftype, "help" | "nofile" | "prompt" | "quickfix")
        || (!buflisted && !matches!(buftype, "" | "acwrite" | "terminal"))
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum BufferPerfReason {
    LargeLineCount = 1 << 0,
    SlowCallback = 1 << 1,
    CursorColorExtmarkFallback = 1 << 4,
    ConcealFullScan = 1 << 5,
    ConcealRawScreenposFallback = 1 << 6,
    UnsupportedBuftype = 1 << 2,
    DisabledFiletype = 1 << 3,
}

impl BufferPerfReason {
    const fn diagnostic_name(self) -> &'static str {
        match self {
            Self::LargeLineCount => "lines",
            Self::SlowCallback => "slow_cb",
            Self::CursorColorExtmarkFallback => "extmark",
            Self::ConcealFullScan => "conceal_scan",
            Self::ConcealRawScreenposFallback => "conceal_raw",
            Self::UnsupportedBuftype => "buftype",
            Self::DisabledFiletype => "filetype",
        }
    }
}

#[derive(Debug, Default, Clone, Copy, Eq, PartialEq)]
struct BufferPerfReasons(u8);

impl BufferPerfReasons {
    fn insert(&mut self, reason: BufferPerfReason) {
        self.0 |= reason as u8;
    }

    const fn contains(self, reason: BufferPerfReason) -> bool {
        self.0 & reason as u8 != 0
    }

    const fn bits(self) -> u8 {
        self.0
    }

    fn diagnostic_summary(self) -> String {
        let reasons = [
            BufferPerfReason::LargeLineCount,
            BufferPerfReason::SlowCallback,
            BufferPerfReason::CursorColorExtmarkFallback,
            BufferPerfReason::ConcealFullScan,
            BufferPerfReason::ConcealRawScreenposFallback,
            BufferPerfReason::UnsupportedBuftype,
            BufferPerfReason::DisabledFiletype,
        ]
        .into_iter()
        .filter(|reason| self.bits() & *reason as u8 != 0)
        .map(BufferPerfReason::diagnostic_name)
        .collect::<Vec<_>>();
        if reasons.is_empty() {
            "none".to_string()
        } else {
            reasons.join(",")
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct BufferEventPolicy {
    perf_class: BufferPerfClass,
    reasons: BufferPerfReasons,
    line_count: usize,
    callback_duration_estimate_ms: f64,
}

#[derive(Debug, Clone, Copy)]
struct BufferEventPolicyInput<'a> {
    buftype: &'a str,
    buflisted: bool,
    perf_mode: BufferPerfMode,
    line_count: usize,
    callback_duration_estimate_ms: f64,
    signals: BufferPerfSignals,
    filetype_disabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct BufferEventPolicyCacheEntry {
    buffer_handle: i64,
    policy: BufferEventPolicy,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub(super) struct BufferEventPolicyCache {
    entries: VecDeque<BufferEventPolicyCacheEntry>,
}

impl BufferEventPolicyCache {
    pub(super) fn cached_policy(&self, buffer_handle: i64) -> Option<BufferEventPolicy> {
        self.entries
            .iter()
            .find(|entry| entry.buffer_handle == buffer_handle)
            .map(|entry| entry.policy)
    }

    pub(super) fn store_policy(&mut self, buffer_handle: i64, policy: BufferEventPolicy) {
        if let Some(existing_index) = self
            .entries
            .iter()
            .position(|entry| entry.buffer_handle == buffer_handle)
        {
            let _ = self.entries.remove(existing_index);
        }

        self.entries.push_front(BufferEventPolicyCacheEntry {
            buffer_handle,
            policy,
        });
        while self.entries.len() > BUFFER_EVENT_POLICY_CACHE_CAPACITY {
            let _ = self.entries.pop_back();
        }
    }

    pub(super) fn clear(&mut self) {
        self.entries.clear();
    }
}

impl BufferEventPolicy {
    fn previous_reason_active(previous: Option<Self>, reason: BufferPerfReason) -> bool {
        previous.is_some_and(|previous| {
            previous.reasons.contains(reason)
                && matches!(previous.perf_class, BufferPerfClass::FastMotion)
        })
    }

    fn from_input(input: BufferEventPolicyInput<'_>, previous: Option<Self>) -> Self {
        let previous_line_reason_active =
            Self::previous_reason_active(previous, BufferPerfReason::LargeLineCount);
        let previous_callback_reason_active =
            Self::previous_reason_active(previous, BufferPerfReason::SlowCallback);
        let previous_extmark_reason_active =
            Self::previous_reason_active(previous, BufferPerfReason::CursorColorExtmarkFallback);
        let previous_conceal_scan_reason_active =
            Self::previous_reason_active(previous, BufferPerfReason::ConcealFullScan);
        let previous_conceal_raw_reason_active =
            Self::previous_reason_active(previous, BufferPerfReason::ConcealRawScreenposFallback);
        let line_count_drives_policy = input.buflisted || input.buftype == "terminal";
        let line_count_fast_motion = line_count_drives_policy
            && threshold_active_usize(
                input.line_count,
                previous_line_reason_active,
                FAST_MOTION_LINE_COUNT_ENTER_THRESHOLD,
                FAST_MOTION_LINE_COUNT_EXIT_THRESHOLD,
            );
        let slow_callback = threshold_active_f64(
            input.callback_duration_estimate_ms,
            previous_callback_reason_active,
            FAST_MOTION_CALLBACK_MS_ENTER_THRESHOLD,
            FAST_MOTION_CALLBACK_MS_EXIT_THRESHOLD,
        );
        let extmark_pressure = threshold_active_f64(
            input.signals.cursor_color_extmark_fallback_pressure(),
            previous_extmark_reason_active,
            FAST_MOTION_EXTMARK_PRESSURE_ENTER_THRESHOLD,
            FAST_MOTION_EXTMARK_PRESSURE_EXIT_THRESHOLD,
        );
        let conceal_scan_pressure = threshold_active_f64(
            input.signals.conceal_full_scan_pressure(),
            previous_conceal_scan_reason_active,
            FAST_MOTION_CONCEAL_SCAN_PRESSURE_ENTER_THRESHOLD,
            FAST_MOTION_CONCEAL_SCAN_PRESSURE_EXIT_THRESHOLD,
        );
        let conceal_raw_pressure = threshold_active_f64(
            input.signals.conceal_raw_screenpos_fallback_pressure(),
            previous_conceal_raw_reason_active,
            FAST_MOTION_CONCEAL_RAW_PRESSURE_ENTER_THRESHOLD,
            FAST_MOTION_CONCEAL_RAW_PRESSURE_EXIT_THRESHOLD,
        );
        let unsupported_buftype = unsupported_buftype(input.buftype, input.buflisted);
        let mut reasons = BufferPerfReasons::default();
        if line_count_fast_motion {
            reasons.insert(BufferPerfReason::LargeLineCount);
        }
        if slow_callback {
            reasons.insert(BufferPerfReason::SlowCallback);
        }
        if extmark_pressure {
            reasons.insert(BufferPerfReason::CursorColorExtmarkFallback);
        }
        if conceal_scan_pressure {
            reasons.insert(BufferPerfReason::ConcealFullScan);
        }
        if conceal_raw_pressure {
            reasons.insert(BufferPerfReason::ConcealRawScreenposFallback);
        }
        if unsupported_buftype {
            reasons.insert(BufferPerfReason::UnsupportedBuftype);
        }
        if input.filetype_disabled {
            reasons.insert(BufferPerfReason::DisabledFiletype);
        }

        let perf_class = if input.filetype_disabled || unsupported_buftype {
            BufferPerfClass::Skip
        } else {
            match input.perf_mode.forced_perf_class() {
                Some(forced_perf_class) => forced_perf_class,
                None if line_count_fast_motion
                    || slow_callback
                    || extmark_pressure
                    || conceal_scan_pressure
                    || conceal_raw_pressure =>
                {
                    BufferPerfClass::FastMotion
                }
                None => BufferPerfClass::Full,
            }
        };

        Self {
            perf_class,
            reasons,
            line_count: input.line_count,
            callback_duration_estimate_ms: input.callback_duration_estimate_ms,
        }
    }

    fn from_snapshot(
        snapshot: &IngressReadSnapshot,
        metadata: &BufferMetadata,
        previous: Option<Self>,
        signals: BufferPerfSignals,
    ) -> Self {
        Self::from_input(
            BufferEventPolicyInput {
                buftype: metadata.buftype(),
                buflisted: metadata.buflisted(),
                perf_mode: snapshot.buffer_perf_mode(),
                line_count: metadata.line_count(),
                callback_duration_estimate_ms: snapshot.callback_duration_estimate_ms(),
                signals,
                filetype_disabled: snapshot.has_disabled_filetypes()
                    && snapshot.filetype_disabled(metadata.filetype()),
            },
            previous,
        )
    }

    pub(super) const fn should_skip(self) -> bool {
        matches!(self.perf_class, BufferPerfClass::Skip)
    }

    pub(super) const fn diagnostic_class_name(self) -> &'static str {
        self.perf_class.diagnostic_name()
    }

    pub(super) const fn core_perf_class(self) -> crate::core::state::BufferPerfClass {
        self.perf_class
    }

    pub(super) const fn diagnostic_effective_mode_name(
        self,
        configured_mode: BufferPerfMode,
    ) -> &'static str {
        match (configured_mode, self.perf_class) {
            (BufferPerfMode::Auto, BufferPerfClass::Full) => "auto_full",
            (BufferPerfMode::Auto, BufferPerfClass::FastMotion) => "auto_fast",
            (BufferPerfMode::Auto, BufferPerfClass::Skip) => "auto_skip",
            (BufferPerfMode::Full, BufferPerfClass::Full) => "full_full",
            (BufferPerfMode::Full, BufferPerfClass::FastMotion) => "full_fast",
            (BufferPerfMode::Full, BufferPerfClass::Skip) => "full_skip",
            (BufferPerfMode::Fast, BufferPerfClass::Full) => "fast_full",
            (BufferPerfMode::Fast, BufferPerfClass::FastMotion) => "fast_fast",
            (BufferPerfMode::Fast, BufferPerfClass::Skip) => "fast_skip",
            (BufferPerfMode::Off, BufferPerfClass::Full) => "off_full",
            (BufferPerfMode::Off, BufferPerfClass::FastMotion) => "off_fast",
            (BufferPerfMode::Off, BufferPerfClass::Skip) => "off_skip",
        }
    }

    pub(super) const fn observed_reason_bits(self) -> u8 {
        self.reasons.bits()
    }

    pub(super) const fn reason_bits(self) -> u8 {
        self.observed_reason_bits()
    }

    pub(super) fn diagnostic_observed_reason_summary(self) -> String {
        self.reasons.diagnostic_summary()
    }

    pub(super) fn diagnostic_reason_summary(self) -> String {
        self.diagnostic_observed_reason_summary()
    }

    pub(super) fn diagnostic_summary(self) -> String {
        let reasons = self.diagnostic_reason_summary();
        if reasons == "none" {
            self.diagnostic_class_name().to_string()
        } else {
            format!("{}:{reasons}", self.diagnostic_class_name())
        }
    }

    pub(super) const fn line_count(self) -> usize {
        self.line_count
    }

    pub(super) const fn callback_duration_estimate_ms(self) -> f64 {
        self.callback_duration_estimate_ms
    }

    pub(super) const fn perf_class(self) -> BufferPerfClass {
        self.perf_class
    }

    #[cfg(test)]
    pub(super) fn from_buffer_metadata(
        buftype: &str,
        buflisted: bool,
        line_count: usize,
        callback_duration_estimate_ms: f64,
    ) -> Self {
        Self::from_test_input_with_previous(
            None,
            buftype,
            buflisted,
            line_count,
            callback_duration_estimate_ms,
            false,
        )
    }

    #[cfg(test)]
    pub(super) fn from_test_input(
        buftype: &str,
        buflisted: bool,
        line_count: usize,
        callback_duration_estimate_ms: f64,
        filetype_disabled: bool,
    ) -> Self {
        Self::from_test_input_with_previous(
            None,
            buftype,
            buflisted,
            line_count,
            callback_duration_estimate_ms,
            filetype_disabled,
        )
    }

    #[cfg(test)]
    pub(super) fn from_test_input_with_previous(
        previous: Option<Self>,
        buftype: &str,
        buflisted: bool,
        line_count: usize,
        callback_duration_estimate_ms: f64,
        filetype_disabled: bool,
    ) -> Self {
        Self::from_input(
            BufferEventPolicyInput {
                buftype,
                buflisted,
                perf_mode: BufferPerfMode::Auto,
                line_count,
                callback_duration_estimate_ms,
                signals: BufferPerfSignals::default(),
                filetype_disabled,
            },
            previous,
        )
    }

    #[cfg(test)]
    pub(super) fn from_test_input_with_perf_mode(
        previous: Option<Self>,
        buftype: &str,
        buflisted: bool,
        perf_mode: BufferPerfMode,
        line_count: usize,
        callback_duration_estimate_ms: f64,
        signals: BufferPerfSignals,
        filetype_disabled: bool,
    ) -> Self {
        Self::from_input(
            BufferEventPolicyInput {
                buftype,
                buflisted,
                perf_mode,
                line_count,
                callback_duration_estimate_ms,
                signals,
                filetype_disabled,
            },
            previous,
        )
    }
}

pub(super) fn current_buffer_event_policy(
    snapshot: &IngressReadSnapshot,
    buffer: &api::Buffer,
    previous: Option<BufferEventPolicy>,
    telemetry: BufferPerfTelemetry,
    observed_at_ms: f64,
) -> Result<BufferEventPolicy> {
    let metadata = BufferMetadata::read(buffer)?;
    Ok(BufferEventPolicy::from_snapshot(
        snapshot,
        &metadata,
        previous,
        telemetry.signals_at(observed_at_ms),
    ))
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(super) struct IngressCursorPresentationContext {
    pub(super) enabled: bool,
    pub(super) animating: bool,
    pub(super) mode_allowed: bool,
    pub(super) hide_target_hack: bool,
    pub(super) outside_cmdline: bool,
    pub(super) prepaint_cell: Option<ScreenCell>,
    pub(super) windows_zindex: u32,
}

#[cfg(test)]
impl IngressCursorPresentationContext {
    pub(super) const fn new(
        enabled: bool,
        animating: bool,
        mode_allowed: bool,
        hide_target_hack: bool,
        outside_cmdline: bool,
        prepaint_cell: Option<ScreenCell>,
        windows_zindex: u32,
    ) -> Self {
        Self {
            enabled,
            animating,
            mode_allowed,
            hide_target_hack,
            outside_cmdline,
            prepaint_cell,
            windows_zindex,
        }
    }
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(super) enum IngressCursorPresentationPolicy {
    NoAction,
    HideCursor,
    HideCursorAndPrepaint { cell: ScreenCell, zindex: u32 },
}

#[cfg(test)]
impl BufferEventPolicy {
    pub(super) fn ingress_cursor_presentation_policy(
        self,
        context: IngressCursorPresentationContext,
    ) -> IngressCursorPresentationPolicy {
        if self.should_skip()
            || !context.enabled
            || context.animating
            || !context.mode_allowed
            || context.hide_target_hack
            || !context.outside_cmdline
        {
            return IngressCursorPresentationPolicy::NoAction;
        }

        context
            .prepaint_cell
            .map_or(IngressCursorPresentationPolicy::HideCursor, |cell| {
                IngressCursorPresentationPolicy::HideCursorAndPrepaint {
                    cell,
                    zindex: context.windows_zindex,
                }
            })
    }
}
