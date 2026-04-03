use crate::core::runtime_reducer::RenderDecision;
use crate::core::state::AnimationSchedule;
use crate::core::state::BackgroundProbeChunk;
use crate::core::state::BufferPerfClass;
use crate::core::state::CoreState;
use crate::core::state::CursorColorProbeWitness;
use crate::core::state::CursorColorSample;
use crate::core::state::ExternalDemandKind;
use crate::core::state::InFlightProposal;
use crate::core::state::ObservationBasis;
use crate::core::state::ObservationRequest;
use crate::core::state::ObservationSnapshot;
use crate::core::state::ProbeKind;
use crate::core::types::DelayBudgetMs;
use crate::core::types::Millis;
use crate::core::types::ProbeRequestId;
use crate::core::types::ProposalId;
use crate::core::types::TimerId;
use crate::core::types::TimerToken;
use crate::state::CursorLocation;
use crate::types::CursorCellShape;
use crate::types::Point;
use crate::types::ScreenCell;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(crate) enum TimerKind {
    Animation,
    Ingress,
    Recovery,
    Cleanup,
}

impl TimerKind {
    pub(crate) const fn from_timer_id(timer_id: TimerId) -> Self {
        match timer_id {
            TimerId::Animation => Self::Animation,
            TimerId::Ingress => Self::Ingress,
            TimerId::Recovery => Self::Recovery,
            TimerId::Cleanup => Self::Cleanup,
        }
    }

    pub(crate) const fn timer_id(self) -> TimerId {
        match self {
            Self::Animation => TimerId::Animation,
            Self::Ingress => TimerId::Ingress,
            Self::Recovery => TimerId::Recovery,
            Self::Cleanup => TimerId::Cleanup,
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct ScheduleTimerEffect {
    pub(crate) token: TimerToken,
    pub(crate) delay: DelayBudgetMs,
    pub(crate) requested_at: Millis,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RequestObservationBaseEffect {
    pub(crate) request: ObservationRequest,
    pub(crate) context: ObservationRuntimeContext,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct CursorPositionReadPolicy {
    smear_to_cmd: bool,
}

impl CursorPositionReadPolicy {
    pub(crate) const fn new(smear_to_cmd: bool) -> Self {
        Self { smear_to_cmd }
    }

    pub(crate) const fn smear_to_cmd(self) -> bool {
        self.smear_to_cmd
    }
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum ProbeQuality {
    Exact,
    FastMotion,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum CursorPositionProbeMode {
    Exact,
    RawDuringMotion,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum CursorColorReuseMode {
    ExactOnly,
    // Reuse only when the probe witness still matches on buffer, changedtick,
    // mode, colorscheme generation, and line. Column drift is allowed.
    CompatibleWithinLine,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum CursorColorFallbackMode {
    SyntaxThenExtmarks,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct ProbePolicy {
    cursor_position_mode: CursorPositionProbeMode,
    cursor_color_reuse_mode: CursorColorReuseMode,
    cursor_color_fallback_mode: CursorColorFallbackMode,
}

impl ProbePolicy {
    #[cfg(test)]
    pub(crate) const fn new(quality: ProbeQuality) -> Self {
        match quality {
            ProbeQuality::Exact => Self::exact(),
            ProbeQuality::FastMotion => Self::fast_motion(),
        }
    }

    pub(crate) const fn exact() -> Self {
        Self::from_modes(
            CursorPositionProbeMode::Exact,
            CursorColorReuseMode::ExactOnly,
            CursorColorFallbackMode::SyntaxThenExtmarks,
        )
    }

    #[cfg(test)]
    pub(crate) const fn fast_motion() -> Self {
        Self::from_modes(
            CursorPositionProbeMode::RawDuringMotion,
            CursorColorReuseMode::CompatibleWithinLine,
            CursorColorFallbackMode::SyntaxThenExtmarks,
        )
    }

    pub(crate) const fn from_modes(
        cursor_position_mode: CursorPositionProbeMode,
        cursor_color_reuse_mode: CursorColorReuseMode,
        cursor_color_fallback_mode: CursorColorFallbackMode,
    ) -> Self {
        Self {
            cursor_position_mode,
            cursor_color_reuse_mode,
            cursor_color_fallback_mode,
        }
    }

    pub(crate) const fn for_demand(
        demand_kind: ExternalDemandKind,
        buffer_perf_class: BufferPerfClass,
        has_cursor_color_fallback_sample: bool,
    ) -> Self {
        // A carried fallback sample is only safe for the same-line compatible
        // reuse path. Boundary refreshes still force an exact probe policy.
        let cursor_color_reuse_mode = if has_cursor_color_fallback_sample {
            CursorColorReuseMode::CompatibleWithinLine
        } else {
            CursorColorReuseMode::ExactOnly
        };

        match demand_kind {
            ExternalDemandKind::ExternalCursor => match buffer_perf_class {
                BufferPerfClass::Full | BufferPerfClass::Skip => Self::from_modes(
                    CursorPositionProbeMode::Exact,
                    cursor_color_reuse_mode,
                    CursorColorFallbackMode::SyntaxThenExtmarks,
                ),
                // Fast motion still uses the raw screen-position path, but fresh cursor-color
                // samples must remain overlay-aware so semantic tokens and other extmarks do not
                // smear with stale syntax-only tint.
                BufferPerfClass::FastMotion => Self::from_modes(
                    CursorPositionProbeMode::RawDuringMotion,
                    cursor_color_reuse_mode,
                    CursorColorFallbackMode::SyntaxThenExtmarks,
                ),
            },
            ExternalDemandKind::ModeChanged
            | ExternalDemandKind::BufferEntered
            | ExternalDemandKind::BoundaryRefresh => Self::exact(),
        }
    }

    #[cfg(test)]
    pub(crate) const fn quality(self) -> ProbeQuality {
        match self.cursor_position_mode {
            CursorPositionProbeMode::Exact => ProbeQuality::Exact,
            CursorPositionProbeMode::RawDuringMotion => ProbeQuality::FastMotion,
        }
    }

    #[cfg(test)]
    pub(crate) const fn cursor_position_mode(self) -> CursorPositionProbeMode {
        self.cursor_position_mode
    }

    #[cfg(test)]
    pub(crate) const fn cursor_color_reuse_mode(self) -> CursorColorReuseMode {
        self.cursor_color_reuse_mode
    }

    #[cfg(test)]
    pub(crate) const fn cursor_color_fallback_mode(self) -> CursorColorFallbackMode {
        self.cursor_color_fallback_mode
    }

    pub(crate) const fn diagnostic_name(self) -> &'static str {
        match (
            self.cursor_position_mode,
            self.cursor_color_reuse_mode,
            self.cursor_color_fallback_mode,
        ) {
            (
                CursorPositionProbeMode::Exact,
                CursorColorReuseMode::ExactOnly,
                CursorColorFallbackMode::SyntaxThenExtmarks,
            ) => "exact",
            (
                CursorPositionProbeMode::Exact,
                CursorColorReuseMode::CompatibleWithinLine,
                CursorColorFallbackMode::SyntaxThenExtmarks,
            ) => "exact_compatible",
            (
                CursorPositionProbeMode::RawDuringMotion,
                CursorColorReuseMode::ExactOnly,
                CursorColorFallbackMode::SyntaxThenExtmarks,
            ) => "raw_extmarks",
            (
                CursorPositionProbeMode::RawDuringMotion,
                CursorColorReuseMode::CompatibleWithinLine,
                CursorColorFallbackMode::SyntaxThenExtmarks,
            ) => "raw_compatible_extmarks",
        }
    }

    pub(crate) const fn allows_compatible_cursor_color_reuse(self) -> bool {
        matches!(
            self.cursor_color_reuse_mode,
            CursorColorReuseMode::CompatibleWithinLine
        )
    }

    pub(crate) const fn allows_cursor_color_extmark_fallback(self) -> bool {
        matches!(
            self.cursor_color_fallback_mode,
            CursorColorFallbackMode::SyntaxThenExtmarks
        )
    }

    pub(crate) const fn uses_raw_screenpos_fallback(self) -> bool {
        matches!(
            self.cursor_position_mode,
            CursorPositionProbeMode::RawDuringMotion
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ObservationRuntimeContext {
    cursor_position_policy: CursorPositionReadPolicy,
    scroll_buffer_space: bool,
    tracked_location: Option<CursorLocation>,
    current_corners: [Point; 4],
    buffer_perf_class: BufferPerfClass,
    probe_policy: ProbePolicy,
}

impl ObservationRuntimeContext {
    pub(crate) const fn new(
        cursor_position_policy: CursorPositionReadPolicy,
        scroll_buffer_space: bool,
        tracked_location: Option<CursorLocation>,
        current_corners: [Point; 4],
        buffer_perf_class: BufferPerfClass,
        probe_policy: ProbePolicy,
    ) -> Self {
        Self {
            cursor_position_policy,
            scroll_buffer_space,
            tracked_location,
            current_corners,
            buffer_perf_class,
            probe_policy,
        }
    }

    pub(crate) const fn cursor_position_policy(&self) -> CursorPositionReadPolicy {
        self.cursor_position_policy
    }

    pub(crate) const fn scroll_buffer_space(&self) -> bool {
        self.scroll_buffer_space
    }

    pub(crate) fn tracked_location(&self) -> Option<CursorLocation> {
        self.tracked_location.clone()
    }

    pub(crate) const fn current_corners(&self) -> [Point; 4] {
        self.current_corners
    }

    #[cfg(test)]
    pub(crate) const fn buffer_perf_class(&self) -> BufferPerfClass {
        self.buffer_perf_class
    }

    pub(crate) const fn probe_policy(&self) -> ProbePolicy {
        self.probe_policy
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RequestProbeEffect {
    pub(crate) observation_basis: Box<ObservationBasis>,
    pub(crate) probe_request_id: ProbeRequestId,
    pub(crate) kind: ProbeKind,
    pub(crate) cursor_position_policy: CursorPositionReadPolicy,
    pub(crate) buffer_perf_class: BufferPerfClass,
    pub(crate) probe_policy: ProbePolicy,
    pub(crate) background_chunk: Option<BackgroundProbeChunk>,
    pub(crate) cursor_color_fallback: Option<CursorColorFallback>,
}

impl RequestProbeEffect {
    pub(crate) const fn probe_policy(&self) -> ProbePolicy {
        self.probe_policy
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct CursorColorFallback {
    sample: CursorColorSample,
    witness: CursorColorProbeWitness,
}

impl CursorColorFallback {
    pub(crate) fn new(sample: CursorColorSample, witness: CursorColorProbeWitness) -> Self {
        Self { sample, witness }
    }

    pub(crate) fn sample(&self) -> CursorColorSample {
        self.sample
    }

    pub(crate) fn witness(&self) -> &CursorColorProbeWitness {
        &self.witness
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ApplyProposalEffect {
    pub(crate) proposal: InFlightProposal,
    pub(crate) requested_at: Millis,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RequestRenderPlanEffect {
    pub(crate) proposal_id: ProposalId,
    pub(crate) planning_state: CoreState,
    pub(crate) observation: ObservationSnapshot,
    pub(crate) render_decision: RenderDecision,
    pub(crate) animation_schedule: AnimationSchedule,
    pub(crate) requested_at: Millis,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct IngressCursorPresentationRequest {
    mode_allowed: bool,
    outside_cmdline: bool,
    prepaint_cell: Option<ScreenCell>,
    prepaint_shape: CursorCellShape,
}

impl IngressCursorPresentationRequest {
    pub(crate) const fn new(
        mode_allowed: bool,
        outside_cmdline: bool,
        prepaint_cell: Option<ScreenCell>,
        prepaint_shape: CursorCellShape,
    ) -> Self {
        Self {
            mode_allowed,
            outside_cmdline,
            prepaint_cell,
            prepaint_shape,
        }
    }

    pub(crate) const fn mode_allowed(self) -> bool {
        self.mode_allowed
    }

    pub(crate) const fn outside_cmdline(self) -> bool {
        self.outside_cmdline
    }

    pub(crate) const fn prepaint_cell(self) -> Option<ScreenCell> {
        self.prepaint_cell
    }

    pub(crate) const fn prepaint_shape(self) -> CursorCellShape {
        self.prepaint_shape
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum IngressCursorPresentationEffect {
    HideCursor,
    HideCursorAndPrepaint {
        cell: ScreenCell,
        shape: CursorCellShape,
        zindex: u32,
    },
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum RenderCleanupExecution {
    SoftClear {
        max_kept_windows: usize,
    },
    CompactToBudget {
        target_budget: usize,
        max_prune_per_tick: usize,
    },
    HardPurge,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct ApplyRenderCleanupEffect {
    pub(crate) execution: RenderCleanupExecution,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum EventLoopMetricEffect {
    IngressCoalesced,
    DelayedIngressPendingUpdated,
    CleanupConvergedToCold {
        started_at: Millis,
        converged_at: Millis,
    },
    StaleToken,
    ProbeRefreshRetried(ProbeKind),
    ProbeRefreshBudgetExhausted(ProbeKind),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Effect {
    ScheduleTimer(ScheduleTimerEffect),
    RequestObservationBase(RequestObservationBaseEffect),
    RequestProbe(RequestProbeEffect),
    RequestRenderPlan(Box<RequestRenderPlanEffect>),
    ApplyProposal(Box<ApplyProposalEffect>),
    ApplyRenderCleanup(ApplyRenderCleanupEffect),
    ApplyIngressCursorPresentation(IngressCursorPresentationEffect),
    RecordEventLoopMetric(EventLoopMetricEffect),
    RedrawCmdline,
}

#[cfg(test)]
mod tests {
    use super::CursorColorFallbackMode;
    use super::CursorColorReuseMode;
    use super::CursorPositionProbeMode;
    use super::ProbePolicy;
    use super::ProbeQuality;
    use crate::core::state::BufferPerfClass;
    use crate::core::state::ExternalDemandKind;
    use crate::test_support::assertions::assert_probe_policy_shape;
    use crate::test_support::proptest::pure_config;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;

    fn demand_kind() -> BoxedStrategy<ExternalDemandKind> {
        prop_oneof![
            Just(ExternalDemandKind::ExternalCursor),
            Just(ExternalDemandKind::ModeChanged),
            Just(ExternalDemandKind::BufferEntered),
            Just(ExternalDemandKind::BoundaryRefresh),
        ]
        .boxed()
    }

    fn buffer_perf_class() -> BoxedStrategy<BufferPerfClass> {
        prop_oneof![
            Just(BufferPerfClass::Full),
            Just(BufferPerfClass::FastMotion),
            Just(BufferPerfClass::Skip),
        ]
        .boxed()
    }

    const fn expected_quality(
        demand_kind: ExternalDemandKind,
        buffer_perf_class: BufferPerfClass,
    ) -> ProbeQuality {
        if matches!(demand_kind, ExternalDemandKind::ExternalCursor)
            && matches!(buffer_perf_class, BufferPerfClass::FastMotion)
        {
            ProbeQuality::FastMotion
        } else {
            ProbeQuality::Exact
        }
    }

    const fn expected_position_mode(
        demand_kind: ExternalDemandKind,
        buffer_perf_class: BufferPerfClass,
    ) -> CursorPositionProbeMode {
        if matches!(demand_kind, ExternalDemandKind::ExternalCursor)
            && matches!(buffer_perf_class, BufferPerfClass::FastMotion)
        {
            CursorPositionProbeMode::RawDuringMotion
        } else {
            CursorPositionProbeMode::Exact
        }
    }

    const fn expected_reuse_mode(
        demand_kind: ExternalDemandKind,
        has_cursor_color_fallback_sample: bool,
    ) -> CursorColorReuseMode {
        if matches!(demand_kind, ExternalDemandKind::ExternalCursor)
            && has_cursor_color_fallback_sample
        {
            CursorColorReuseMode::CompatibleWithinLine
        } else {
            CursorColorReuseMode::ExactOnly
        }
    }

    const fn expected_diagnostic_name(
        cursor_position_mode: CursorPositionProbeMode,
        cursor_color_reuse_mode: CursorColorReuseMode,
    ) -> &'static str {
        match (cursor_position_mode, cursor_color_reuse_mode) {
            (CursorPositionProbeMode::Exact, CursorColorReuseMode::ExactOnly) => "exact",
            (CursorPositionProbeMode::Exact, CursorColorReuseMode::CompatibleWithinLine) => {
                "exact_compatible"
            }
            (CursorPositionProbeMode::RawDuringMotion, CursorColorReuseMode::ExactOnly) => {
                "raw_extmarks"
            }
            (
                CursorPositionProbeMode::RawDuringMotion,
                CursorColorReuseMode::CompatibleWithinLine,
            ) => "raw_compatible_extmarks",
        }
    }

    proptest! {
        #![proptest_config(pure_config())]

        #[test]
        fn prop_probe_policy_matches_demand_perf_class_and_retained_color_inputs(
            demand_kind in demand_kind(),
            buffer_perf_class in buffer_perf_class(),
            has_cursor_color_fallback_sample in any::<bool>(),
        ) {
            let policy = ProbePolicy::for_demand(
                demand_kind,
                buffer_perf_class,
                has_cursor_color_fallback_sample,
            );

            let expected_quality = expected_quality(demand_kind, buffer_perf_class);
            let expected_position_mode = expected_position_mode(demand_kind, buffer_perf_class);
            let expected_reuse_mode =
                expected_reuse_mode(demand_kind, has_cursor_color_fallback_sample);

            assert_probe_policy_shape(
                policy,
                expected_quality,
                expected_position_mode,
                expected_reuse_mode,
                CursorColorFallbackMode::SyntaxThenExtmarks,
            );
            assert_eq!(
                policy.diagnostic_name(),
                expected_diagnostic_name(expected_position_mode, expected_reuse_mode),
            );
        }
    }
}
