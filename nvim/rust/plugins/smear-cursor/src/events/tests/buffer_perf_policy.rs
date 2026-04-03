use super::super::policy::BufferEventPolicy;
use super::super::policy::BufferEventPolicyInput;
use super::super::policy::BufferPerfClass;
use super::super::policy::BufferPerfSignals;
use super::super::policy::BufferPerfTelemetry;
use super::super::policy::IngressCursorPresentationContext;
use super::super::policy::IngressCursorPresentationPolicy;
use crate::config::BufferPerfMode;
use crate::test_support::proptest::pure_config;
use crate::types::ScreenCell;
use proptest::prelude::*;

const LINES_REASON_BIT: u8 = 1 << 0;
const SLOW_CALLBACK_REASON_BIT: u8 = 1 << 1;
const UNSUPPORTED_BUFTYPE_REASON_BIT: u8 = 1 << 2;
const DISABLED_FILETYPE_REASON_BIT: u8 = 1 << 3;
const EXTMARK_REASON_BIT: u8 = 1 << 4;
const CONCEAL_SCAN_REASON_BIT: u8 = 1 << 5;
const CONCEAL_RAW_REASON_BIT: u8 = 1 << 6;

#[derive(Clone, Copy, Debug)]
enum SupportedBufferCase {
    ListedNormal,
    UnlistedNormal,
    UnlistedTerminal,
}

impl SupportedBufferCase {
    const fn buftype(self) -> &'static str {
        match self {
            Self::ListedNormal | Self::UnlistedNormal => "",
            Self::UnlistedTerminal => "terminal",
        }
    }

    const fn buflisted(self) -> bool {
        match self {
            Self::ListedNormal => true,
            Self::UnlistedNormal | Self::UnlistedTerminal => false,
        }
    }

    const fn line_count_drives_policy(self) -> bool {
        !matches!(self, Self::UnlistedNormal)
    }
}

#[derive(Clone, Copy, Debug)]
enum SkipCase {
    UnsupportedBuftype,
    DisabledFiletype,
    Both,
}

impl SkipCase {
    const fn buftype(self) -> &'static str {
        match self {
            Self::UnsupportedBuftype | Self::Both => "quickfix",
            Self::DisabledFiletype => "",
        }
    }

    const fn buflisted(self) -> bool {
        !matches!(self, Self::UnsupportedBuftype | Self::Both)
    }

    const fn filetype_disabled(self) -> bool {
        matches!(self, Self::DisabledFiletype | Self::Both)
    }

    const fn includes_unsupported_buftype(self) -> bool {
        matches!(self, Self::UnsupportedBuftype | Self::Both)
    }
}

#[derive(Clone, Copy, Debug)]
enum PressureReason {
    Extmark,
    ConcealScan,
    ConcealRaw,
}

#[derive(Clone, Copy, Debug)]
enum HysteresisReason {
    LineCount,
    Callback,
    Extmark,
    ConcealScan,
    ConcealRaw,
}

fn supported_buffer_case_strategy() -> BoxedStrategy<SupportedBufferCase> {
    prop_oneof![
        Just(SupportedBufferCase::ListedNormal),
        Just(SupportedBufferCase::UnlistedNormal),
        Just(SupportedBufferCase::UnlistedTerminal),
    ]
    .boxed()
}

fn manual_buffer_perf_mode_strategy() -> BoxedStrategy<BufferPerfMode> {
    prop_oneof![
        Just(BufferPerfMode::Full),
        Just(BufferPerfMode::Fast),
        Just(BufferPerfMode::Off),
    ]
    .boxed()
}

fn skip_case_strategy() -> BoxedStrategy<SkipCase> {
    prop_oneof![
        Just(SkipCase::UnsupportedBuftype),
        Just(SkipCase::DisabledFiletype),
        Just(SkipCase::Both),
    ]
    .boxed()
}

fn hysteresis_reason_strategy() -> BoxedStrategy<HysteresisReason> {
    prop_oneof![
        Just(HysteresisReason::LineCount),
        Just(HysteresisReason::Callback),
        Just(HysteresisReason::Extmark),
        Just(HysteresisReason::ConcealScan),
        Just(HysteresisReason::ConcealRaw),
    ]
    .boxed()
}

fn pressure_signal(kind: PressureReason, active: bool) -> BufferPerfSignals {
    let mut telemetry = BufferPerfTelemetry::default();
    if active {
        for _ in 0..2 {
            match kind {
                PressureReason::Extmark => {
                    telemetry.record_cursor_color_extmark_fallback(1_000.0);
                }
                PressureReason::ConcealScan => {
                    telemetry.record_conceal_full_scan(1_000.0);
                }
                PressureReason::ConcealRaw => {
                    telemetry.record_conceal_raw_screenpos_fallback(1_000.0);
                }
            }
        }
    }
    telemetry.signals_at(1_000.0)
}

fn pressure_signals(
    extmark_active: bool,
    conceal_scan_active: bool,
    conceal_raw_active: bool,
) -> BufferPerfSignals {
    let mut telemetry = BufferPerfTelemetry::default();
    if extmark_active {
        for _ in 0..2 {
            telemetry.record_cursor_color_extmark_fallback(1_000.0);
        }
    }
    if conceal_scan_active {
        for _ in 0..2 {
            telemetry.record_conceal_full_scan(1_000.0);
        }
    }
    if conceal_raw_active {
        for _ in 0..2 {
            telemetry.record_conceal_raw_screenpos_fallback(1_000.0);
        }
    }
    telemetry.signals_at(1_000.0)
}

fn reason_names(
    line_count: bool,
    slow_callback: bool,
    extmark: bool,
    conceal_scan: bool,
    conceal_raw: bool,
    unsupported_buftype: bool,
    filetype_disabled: bool,
) -> Vec<&'static str> {
    let mut reasons = Vec::new();
    if line_count {
        reasons.push("lines");
    }
    if slow_callback {
        reasons.push("slow_cb");
    }
    if extmark {
        reasons.push("extmark");
    }
    if conceal_scan {
        reasons.push("conceal_scan");
    }
    if conceal_raw {
        reasons.push("conceal_raw");
    }
    if unsupported_buftype {
        reasons.push("buftype");
    }
    if filetype_disabled {
        reasons.push("filetype");
    }
    reasons
}

fn expected_reason_bits(reasons: &[&'static str]) -> u8 {
    reasons.iter().fold(0_u8, |bits, reason| {
        bits | match *reason {
            "lines" => LINES_REASON_BIT,
            "slow_cb" => SLOW_CALLBACK_REASON_BIT,
            "extmark" => EXTMARK_REASON_BIT,
            "conceal_scan" => CONCEAL_SCAN_REASON_BIT,
            "conceal_raw" => CONCEAL_RAW_REASON_BIT,
            "buftype" => UNSUPPORTED_BUFTYPE_REASON_BIT,
            "filetype" => DISABLED_FILETYPE_REASON_BIT,
            _ => 0,
        }
    })
}

fn expected_reason_summary(reasons: &[&'static str]) -> String {
    if reasons.is_empty() {
        "none".to_string()
    } else {
        reasons.join(",")
    }
}

const fn diagnostic_class_name(perf_class: BufferPerfClass) -> &'static str {
    match perf_class {
        BufferPerfClass::Full => "full",
        BufferPerfClass::FastMotion => "fast",
        BufferPerfClass::Skip => "skip",
    }
}

fn expected_diagnostic_summary(perf_class: BufferPerfClass, reasons: &[&'static str]) -> String {
    let summary = expected_reason_summary(reasons);
    if summary == "none" {
        diagnostic_class_name(perf_class).to_string()
    } else {
        format!("{}:{summary}", diagnostic_class_name(perf_class))
    }
}

fn supported_policy_input(
    buffer_case: SupportedBufferCase,
    perf_mode: BufferPerfMode,
    line_count: usize,
    callback_duration_estimate_ms: f64,
    signals: BufferPerfSignals,
    filetype_disabled: bool,
) -> BufferEventPolicyInput<'static> {
    BufferEventPolicyInput {
        buftype: buffer_case.buftype(),
        buflisted: buffer_case.buflisted(),
        perf_mode,
        line_count,
        callback_duration_estimate_ms,
        signals,
        filetype_disabled,
    }
}

fn previous_policy(reason: HysteresisReason) -> BufferEventPolicy {
    match reason {
        HysteresisReason::LineCount => {
            BufferEventPolicy::from_buffer_metadata("", true, 20_000, 0.0)
        }
        HysteresisReason::Callback => BufferEventPolicy::from_buffer_metadata("", true, 1, 16.0),
        HysteresisReason::Extmark => BufferEventPolicy::from_test_input_with_perf_mode(
            None,
            supported_policy_input(
                SupportedBufferCase::ListedNormal,
                BufferPerfMode::Auto,
                1,
                0.0,
                pressure_signal(PressureReason::Extmark, true),
                false,
            ),
        ),
        HysteresisReason::ConcealScan => BufferEventPolicy::from_test_input_with_perf_mode(
            None,
            supported_policy_input(
                SupportedBufferCase::ListedNormal,
                BufferPerfMode::Auto,
                1,
                0.0,
                pressure_signal(PressureReason::ConcealScan, true),
                false,
            ),
        ),
        HysteresisReason::ConcealRaw => BufferEventPolicy::from_test_input_with_perf_mode(
            None,
            supported_policy_input(
                SupportedBufferCase::ListedNormal,
                BufferPerfMode::Auto,
                1,
                0.0,
                pressure_signal(PressureReason::ConcealRaw, true),
                false,
            ),
        ),
    }
}

fn hysteresis_policies(reason: HysteresisReason) -> (BufferEventPolicy, BufferEventPolicy) {
    let previous = previous_policy(reason);
    match reason {
        HysteresisReason::LineCount => {
            let held = BufferEventPolicy::from_test_input_with_previous(
                Some(previous),
                "",
                true,
                16_000,
                0.0,
                false,
            );
            let released = BufferEventPolicy::from_test_input_with_previous(
                Some(held),
                "",
                true,
                15_999,
                0.0,
                false,
            );
            (held, released)
        }
        HysteresisReason::Callback => {
            let held = BufferEventPolicy::from_test_input_with_previous(
                Some(previous),
                "",
                true,
                1,
                6.0,
                false,
            );
            let released = BufferEventPolicy::from_test_input_with_previous(
                Some(held),
                "",
                true,
                1,
                5.9,
                false,
            );
            (held, released)
        }
        HysteresisReason::Extmark => {
            let held = BufferEventPolicy::from_test_input_with_perf_mode(
                Some(previous),
                supported_policy_input(
                    SupportedBufferCase::ListedNormal,
                    BufferPerfMode::Auto,
                    1,
                    0.0,
                    {
                        let mut telemetry = BufferPerfTelemetry::default();
                        telemetry.record_cursor_color_extmark_fallback(1_000.0);
                        telemetry.record_cursor_color_extmark_fallback(1_000.0);
                        telemetry.signals_at(6_000.0)
                    },
                    false,
                ),
            );
            let released = BufferEventPolicy::from_test_input_with_perf_mode(
                Some(held),
                supported_policy_input(
                    SupportedBufferCase::ListedNormal,
                    BufferPerfMode::Auto,
                    1,
                    0.0,
                    {
                        let mut telemetry = BufferPerfTelemetry::default();
                        telemetry.record_cursor_color_extmark_fallback(1_000.0);
                        telemetry.record_cursor_color_extmark_fallback(1_000.0);
                        telemetry.signals_at(6_100.0)
                    },
                    false,
                ),
            );
            (held, released)
        }
        HysteresisReason::ConcealScan => {
            let held = BufferEventPolicy::from_test_input_with_perf_mode(
                Some(previous),
                supported_policy_input(
                    SupportedBufferCase::ListedNormal,
                    BufferPerfMode::Auto,
                    1,
                    0.0,
                    {
                        let mut telemetry = BufferPerfTelemetry::default();
                        telemetry.record_conceal_full_scan(1_000.0);
                        telemetry.record_conceal_full_scan(1_000.0);
                        telemetry.signals_at(6_000.0)
                    },
                    false,
                ),
            );
            let released = BufferEventPolicy::from_test_input_with_perf_mode(
                Some(held),
                supported_policy_input(
                    SupportedBufferCase::ListedNormal,
                    BufferPerfMode::Auto,
                    1,
                    0.0,
                    {
                        let mut telemetry = BufferPerfTelemetry::default();
                        telemetry.record_conceal_full_scan(1_000.0);
                        telemetry.record_conceal_full_scan(1_000.0);
                        telemetry.signals_at(6_100.0)
                    },
                    false,
                ),
            );
            (held, released)
        }
        HysteresisReason::ConcealRaw => {
            let held = BufferEventPolicy::from_test_input_with_perf_mode(
                Some(previous),
                supported_policy_input(
                    SupportedBufferCase::ListedNormal,
                    BufferPerfMode::Auto,
                    1,
                    0.0,
                    {
                        let mut telemetry = BufferPerfTelemetry::default();
                        telemetry.record_conceal_raw_screenpos_fallback(1_000.0);
                        telemetry.record_conceal_raw_screenpos_fallback(1_000.0);
                        telemetry.signals_at(6_000.0)
                    },
                    false,
                ),
            );
            let released = BufferEventPolicy::from_test_input_with_perf_mode(
                Some(held),
                supported_policy_input(
                    SupportedBufferCase::ListedNormal,
                    BufferPerfMode::Auto,
                    1,
                    0.0,
                    {
                        let mut telemetry = BufferPerfTelemetry::default();
                        telemetry.record_conceal_raw_screenpos_fallback(1_000.0);
                        telemetry.record_conceal_raw_screenpos_fallback(1_000.0);
                        telemetry.signals_at(6_100.0)
                    },
                    false,
                ),
            );
            (held, released)
        }
    }
}

fn hysteresis_reason_bit(reason: HysteresisReason) -> u8 {
    match reason {
        HysteresisReason::LineCount => LINES_REASON_BIT,
        HysteresisReason::Callback => SLOW_CALLBACK_REASON_BIT,
        HysteresisReason::Extmark => EXTMARK_REASON_BIT,
        HysteresisReason::ConcealScan => CONCEAL_SCAN_REASON_BIT,
        HysteresisReason::ConcealRaw => CONCEAL_RAW_REASON_BIT,
    }
}

proptest! {
    #![proptest_config(pure_config())]

    #[test]
    fn prop_ingress_cursor_presentation_matches_runtime_eligibility(
        enabled in any::<bool>(),
        animating in any::<bool>(),
        mode_allowed in any::<bool>(),
        hide_target_hack in any::<bool>(),
        outside_cmdline in any::<bool>(),
        with_prepaint_cell in any::<bool>(),
        row in 1_i64..64_i64,
        col in 1_i64..256_i64,
        windows_zindex in 1_u32..500_u32,
    ) {
        let policy = BufferEventPolicy::from_buffer_metadata("", true, 1, 0.0);
        let prepaint_cell = with_prepaint_cell
            .then(|| ScreenCell::new(row, col).expect("generated screen cell should be valid"));
        let context = IngressCursorPresentationContext::new(
            enabled,
            animating,
            mode_allowed,
            hide_target_hack,
            outside_cmdline,
            prepaint_cell,
            windows_zindex,
        );

        let expected = if enabled
            && !animating
            && mode_allowed
            && !hide_target_hack
            && outside_cmdline
        {
            prepaint_cell.map_or(
                IngressCursorPresentationPolicy::HideCursor,
                |cell| IngressCursorPresentationPolicy::HideCursorAndPrepaint {
                    cell,
                    zindex: windows_zindex,
                },
            )
        } else {
            IngressCursorPresentationPolicy::NoAction
        };

        prop_assert_eq!(policy.ingress_cursor_presentation_policy(context), expected);
    }

    #[test]
    fn prop_auto_policy_matches_reason_activation_and_canonical_summary_order(
        buffer_case in supported_buffer_case_strategy(),
        high_line_count in any::<bool>(),
        slow_callback in any::<bool>(),
        extmark_pressure in any::<bool>(),
        conceal_scan_pressure in any::<bool>(),
        conceal_raw_pressure in any::<bool>(),
        filetype_disabled in any::<bool>(),
    ) {
        let line_count = if high_line_count { 80_000 } else { 1 };
        let callback_duration_estimate_ms = if slow_callback { 16.0 } else { 0.0 };
        let signals = pressure_signals(
            extmark_pressure,
            conceal_scan_pressure,
            conceal_raw_pressure,
        );
        let policy = BufferEventPolicy::from_test_input_with_perf_mode(
            None,
            supported_policy_input(
                buffer_case,
                BufferPerfMode::Auto,
                line_count,
                callback_duration_estimate_ms,
                signals,
                filetype_disabled,
            ),
        );

        let line_count_active = high_line_count && buffer_case.line_count_drives_policy();
        let reasons = reason_names(
            line_count_active,
            slow_callback,
            extmark_pressure,
            conceal_scan_pressure,
            conceal_raw_pressure,
            false,
            filetype_disabled,
        );
        let expected_perf_class = if filetype_disabled {
            BufferPerfClass::Skip
        } else if reasons.is_empty() {
            BufferPerfClass::Full
        } else {
            BufferPerfClass::FastMotion
        };

        prop_assert_eq!(policy.perf_class(), expected_perf_class);
        prop_assert_eq!(policy.observed_reason_bits(), expected_reason_bits(&reasons));
        prop_assert_eq!(
            policy.diagnostic_observed_reason_summary(),
            expected_reason_summary(&reasons)
        );
        prop_assert_eq!(
            policy.diagnostic_summary(),
            expected_diagnostic_summary(expected_perf_class, &reasons)
        );
    }

    #[test]
    fn prop_auto_policy_is_monotone_across_non_decreasing_inputs(
        buffer_case in supported_buffer_case_strategy(),
        low_line_count_active in any::<bool>(),
        raise_line_count in any::<bool>(),
        low_slow_callback in any::<bool>(),
        raise_callback in any::<bool>(),
        low_extmark_pressure in any::<bool>(),
        raise_extmark_pressure in any::<bool>(),
        low_conceal_scan_pressure in any::<bool>(),
        raise_conceal_scan_pressure in any::<bool>(),
        low_conceal_raw_pressure in any::<bool>(),
        raise_conceal_raw_pressure in any::<bool>(),
    ) {
        let high_line_count_active = low_line_count_active || raise_line_count;
        let high_slow_callback = low_slow_callback || raise_callback;
        let high_extmark_pressure = low_extmark_pressure || raise_extmark_pressure;
        let high_conceal_scan_pressure =
            low_conceal_scan_pressure || raise_conceal_scan_pressure;
        let high_conceal_raw_pressure =
            low_conceal_raw_pressure || raise_conceal_raw_pressure;
        let low_policy = BufferEventPolicy::from_test_input_with_perf_mode(
            None,
            supported_policy_input(
                buffer_case,
                BufferPerfMode::Auto,
                if low_line_count_active { 80_000 } else { 1 },
                if low_slow_callback { 16.0 } else { 0.0 },
                pressure_signals(
                    low_extmark_pressure,
                    low_conceal_scan_pressure,
                    low_conceal_raw_pressure,
                ),
                false,
            ),
        );
        let high_policy = BufferEventPolicy::from_test_input_with_perf_mode(
            None,
            supported_policy_input(
                buffer_case,
                BufferPerfMode::Auto,
                if high_line_count_active { 80_000 } else { 1 },
                if high_slow_callback { 16.0 } else { 0.0 },
                pressure_signals(
                    high_extmark_pressure,
                    high_conceal_scan_pressure,
                    high_conceal_raw_pressure,
                ),
                false,
            ),
        );

        prop_assert_eq!(
            low_policy.observed_reason_bits() & !high_policy.observed_reason_bits(),
            0
        );
        if matches!(low_policy.perf_class(), BufferPerfClass::FastMotion) {
            prop_assert_eq!(high_policy.perf_class(), BufferPerfClass::FastMotion);
        }
    }

    #[test]
    fn prop_manual_modes_force_supported_buffers_to_the_configured_perf_class(
        buffer_case in supported_buffer_case_strategy(),
        perf_mode in manual_buffer_perf_mode_strategy(),
        high_line_count in any::<bool>(),
        slow_callback in any::<bool>(),
        extmark_pressure in any::<bool>(),
        conceal_scan_pressure in any::<bool>(),
        conceal_raw_pressure in any::<bool>(),
    ) {
        let policy = BufferEventPolicy::from_test_input_with_perf_mode(
            None,
            supported_policy_input(
                buffer_case,
                perf_mode,
                if high_line_count { 80_000 } else { 1 },
                if slow_callback { 16.0 } else { 0.0 },
                pressure_signals(
                    extmark_pressure,
                    conceal_scan_pressure,
                    conceal_raw_pressure,
                ),
                false,
            ),
        );
        let reasons = reason_names(
            high_line_count && buffer_case.line_count_drives_policy(),
            slow_callback,
            extmark_pressure,
            conceal_scan_pressure,
            conceal_raw_pressure,
            false,
            false,
        );
        let expected_perf_class = perf_mode
            .forced_perf_class()
            .expect("manual buffer perf modes should always force a class");

        prop_assert_eq!(policy.perf_class(), expected_perf_class);
        prop_assert_eq!(policy.observed_reason_bits(), expected_reason_bits(&reasons));
        prop_assert_eq!(
            policy.diagnostic_summary(),
            expected_diagnostic_summary(expected_perf_class, &reasons)
        );
        prop_assert_eq!(
            policy.diagnostic_effective_mode_name(perf_mode),
            match (perf_mode, expected_perf_class) {
                (BufferPerfMode::Full, BufferPerfClass::Full) => "full_full",
                (BufferPerfMode::Fast, BufferPerfClass::FastMotion) => "fast_fast",
                (BufferPerfMode::Off, BufferPerfClass::Skip) => "off_skip",
                _ => unreachable!("manual modes do not map to other forced perf classes"),
            }
        );
    }

    #[test]
    fn prop_hard_skip_precedence_overrides_manual_modes_and_auto_reasons(
        skip_case in skip_case_strategy(),
        perf_mode in prop_oneof![
            Just(BufferPerfMode::Auto),
            Just(BufferPerfMode::Full),
            Just(BufferPerfMode::Fast),
            Just(BufferPerfMode::Off),
        ],
        high_line_count in any::<bool>(),
        slow_callback in any::<bool>(),
        extmark_pressure in any::<bool>(),
        conceal_scan_pressure in any::<bool>(),
        conceal_raw_pressure in any::<bool>(),
    ) {
        let policy = BufferEventPolicy::from_test_input_with_perf_mode(
            None,
            BufferEventPolicyInput {
                buftype: skip_case.buftype(),
                buflisted: skip_case.buflisted(),
                perf_mode,
                line_count: if high_line_count { 80_000 } else { 1 },
                callback_duration_estimate_ms: if slow_callback { 16.0 } else { 0.0 },
                signals: pressure_signals(
                    extmark_pressure,
                    conceal_scan_pressure,
                    conceal_raw_pressure,
                ),
                filetype_disabled: skip_case.filetype_disabled(),
            },
        );
        let reasons = reason_names(
            high_line_count && (skip_case.buflisted() || skip_case.buftype() == "terminal"),
            slow_callback,
            extmark_pressure,
            conceal_scan_pressure,
            conceal_raw_pressure,
            skip_case.includes_unsupported_buftype(),
            skip_case.filetype_disabled(),
        );

        prop_assert_eq!(policy.perf_class(), BufferPerfClass::Skip);
        prop_assert_eq!(policy.observed_reason_bits(), expected_reason_bits(&reasons));
        prop_assert_eq!(
            policy.diagnostic_observed_reason_summary(),
            expected_reason_summary(&reasons)
        );
        prop_assert_eq!(
            policy.diagnostic_summary(),
            expected_diagnostic_summary(BufferPerfClass::Skip, &reasons)
        );
    }

    #[test]
    fn prop_reason_hysteresis_holds_at_exit_threshold_and_releases_below_it(
        reason in hysteresis_reason_strategy()
    ) {
        let (held, released) = hysteresis_policies(reason);
        let reason_bit = hysteresis_reason_bit(reason);

        prop_assert_eq!(held.perf_class(), BufferPerfClass::FastMotion);
        prop_assert_eq!(held.observed_reason_bits(), reason_bit);
        prop_assert_eq!(released.perf_class(), BufferPerfClass::Full);
        prop_assert_eq!(released.observed_reason_bits(), 0);
    }
}
