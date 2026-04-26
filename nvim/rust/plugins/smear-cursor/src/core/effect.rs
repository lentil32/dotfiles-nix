use crate::config::RuntimeConfig;
use crate::core::runtime_reducer::RenderDecision;
use crate::core::state::AnimationSchedule;
use crate::core::state::BackgroundProbeBatch;
use crate::core::state::BackgroundProbeChunk;
use crate::core::state::BufferPerfClass;
use crate::core::state::CursorColorProbeGenerations;
use crate::core::state::CursorColorProbeWitness;
use crate::core::state::CursorColorSample;
use crate::core::state::CursorTextContextBoundary;
use crate::core::state::ExternalDemandKind;
use crate::core::state::InFlightProposal;
use crate::core::state::ObservationBasis;
use crate::core::state::PendingObservation;
use crate::core::state::ProbeKind;
use crate::core::state::ProjectionHandle;
use crate::core::state::SceneState;
use crate::core::state::derive_cursor_color_probe_witness;
use crate::core::types::DelayBudgetMs;
use crate::core::types::Millis;
use crate::core::types::ObservationId;
use crate::core::types::ProbeRequestId;
use crate::core::types::ProposalId;
use crate::core::types::TimerToken;
use crate::host::BufferHandle;
use crate::position::BufferLine;
use crate::position::CursorObservation;
use crate::position::RenderPoint;
use crate::position::ScreenCell;
use crate::position::ViewportBounds;
use crate::position::WindowSurfaceSnapshot;
use crate::state::TrackedCursor;
use crate::types::CursorCellShape;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct ScheduleTimerEffect {
    pub(crate) token: TimerToken,
    pub(crate) delay: DelayBudgetMs,
    pub(crate) requested_at: Millis,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RequestObservationBaseEffect {
    pub(crate) request: PendingObservation,
    pub(crate) context: ObservationRuntimeContext,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct IngressObservationSurface {
    surface: WindowSurfaceSnapshot,
    cursor: Option<CursorObservation>,
    mode: String,
}

impl IngressObservationSurface {
    pub(crate) fn new(
        surface: WindowSurfaceSnapshot,
        cursor: Option<CursorObservation>,
        mode: String,
    ) -> Self {
        Self {
            surface,
            cursor,
            mode,
        }
    }

    pub(crate) const fn surface(&self) -> WindowSurfaceSnapshot {
        self.surface
    }

    pub(crate) const fn window_handle(&self) -> i64 {
        self.surface.id().window_handle()
    }

    pub(crate) const fn buffer_handle(&self) -> BufferHandle {
        self.surface.id().buffer_handle()
    }

    pub(crate) const fn cursor(&self) -> Option<CursorObservation> {
        self.cursor
    }

    pub(crate) fn mode(&self) -> &str {
        self.mode.as_str()
    }
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

/// Cursor-position freshness policy for observation reads.
///
/// Both modes return projected display-space cursor observations. The choice is
/// only whether the reader must finish exact projection now or may return a
/// deferred projected cell that still owes a follow-up exact refresh.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum CursorPositionProbeMode {
    Exact,
    DeferredAllowed,
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
pub(crate) enum RetainedCursorColorFallback {
    Unavailable,
    CompatibleSample,
}

impl RetainedCursorColorFallback {
    const fn reuse_mode(self) -> CursorColorReuseMode {
        match self {
            Self::Unavailable => CursorColorReuseMode::ExactOnly,
            Self::CompatibleSample => CursorColorReuseMode::CompatibleWithinLine,
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct ProbePolicy {
    cursor_position_mode: CursorPositionProbeMode,
    cursor_color_reuse_mode: CursorColorReuseMode,
    cursor_color_fallback_mode: CursorColorFallbackMode,
}

impl ProbePolicy {
    /// Reader strategy knobs for cursor position and color probes.
    ///
    /// The policy controls freshness, reuse, and fallback cost only. It never
    /// changes reducer-owned cursor coordinate space: any returned
    /// [`CursorObservation`] stays in projected display space.
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
            CursorPositionProbeMode::DeferredAllowed,
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
        retained_cursor_color_fallback: RetainedCursorColorFallback,
    ) -> Self {
        // A carried fallback sample is only safe for the same-line compatible
        // reuse path. Boundary refreshes still force an exact probe policy.
        let cursor_color_reuse_mode = match demand_kind {
            ExternalDemandKind::ExternalCursor => retained_cursor_color_fallback.reuse_mode(),
            ExternalDemandKind::ModeChanged
            | ExternalDemandKind::BufferEntered
            | ExternalDemandKind::BoundaryRefresh => CursorColorReuseMode::ExactOnly,
        };

        match demand_kind {
            ExternalDemandKind::ExternalCursor => match buffer_perf_class {
                BufferPerfClass::Full | BufferPerfClass::Skip => Self::from_modes(
                    CursorPositionProbeMode::Exact,
                    cursor_color_reuse_mode,
                    CursorColorFallbackMode::SyntaxThenExtmarks,
                ),
                // Fast motion still allows deferred cursor projection, but fresh cursor-color
                // samples must remain overlay-aware so semantic tokens and other extmarks do not
                // smear with stale syntax-only tint.
                BufferPerfClass::FastMotion => Self::from_modes(
                    CursorPositionProbeMode::DeferredAllowed,
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
            CursorPositionProbeMode::DeferredAllowed => ProbeQuality::FastMotion,
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
                CursorPositionProbeMode::DeferredAllowed,
                CursorColorReuseMode::ExactOnly,
                CursorColorFallbackMode::SyntaxThenExtmarks,
            ) => "deferred_extmarks",
            (
                CursorPositionProbeMode::DeferredAllowed,
                CursorColorReuseMode::CompatibleWithinLine,
                CursorColorFallbackMode::SyntaxThenExtmarks,
            ) => "deferred_compatible_extmarks",
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

    pub(crate) const fn allows_deferred_cursor_projection(self) -> bool {
        matches!(
            self.cursor_position_mode,
            CursorPositionProbeMode::DeferredAllowed
        )
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct TrackedBufferPosition {
    buffer_handle: BufferHandle,
    buffer_line: BufferLine,
}

impl TrackedBufferPosition {
    pub(crate) fn new(
        buffer_handle: impl Into<BufferHandle>,
        buffer_line: BufferLine,
    ) -> Option<Self> {
        let buffer_handle = buffer_handle.into();
        buffer_handle.is_valid().then_some(Self {
            buffer_handle,
            buffer_line,
        })
    }

    pub(crate) const fn buffer_handle(self) -> BufferHandle {
        self.buffer_handle
    }

    pub(crate) const fn buffer_line(self) -> BufferLine {
        self.buffer_line
    }
}

pub(crate) fn tracked_observation_inputs(
    tracked_cursor: Option<&TrackedCursor>,
) -> (Option<WindowSurfaceSnapshot>, Option<TrackedBufferPosition>) {
    let Some(tracked_cursor) = tracked_cursor else {
        return (None, None);
    };

    let tracked_surface = Some(tracked_cursor.surface());
    let tracked_buffer_position =
        TrackedBufferPosition::new(tracked_cursor.buffer_handle(), tracked_cursor.buffer_line());

    (tracked_surface, tracked_buffer_position)
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ObservationRuntimeContextArgs {
    pub(crate) cursor_position_policy: CursorPositionReadPolicy,
    pub(crate) scroll_buffer_space: bool,
    pub(crate) tracked_surface: Option<WindowSurfaceSnapshot>,
    pub(crate) tracked_buffer_position: Option<TrackedBufferPosition>,
    pub(crate) cursor_text_context_boundary: Option<CursorTextContextBoundary>,
    pub(crate) current_corners: [RenderPoint; 4],
    pub(crate) ingress_observation_surface: Option<IngressObservationSurface>,
    pub(crate) buffer_perf_class: BufferPerfClass,
    pub(crate) probe_policy: ProbePolicy,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ObservationRuntimeContext {
    cursor_position_policy: CursorPositionReadPolicy,
    scroll_buffer_space: bool,
    tracked_surface: Option<WindowSurfaceSnapshot>,
    tracked_buffer_position: Option<TrackedBufferPosition>,
    cursor_text_context_boundary: Option<CursorTextContextBoundary>,
    current_corners: [RenderPoint; 4],
    ingress_observation_surface: Option<IngressObservationSurface>,
    buffer_perf_class: BufferPerfClass,
    probe_policy: ProbePolicy,
}

impl ObservationRuntimeContext {
    pub(crate) fn new(args: ObservationRuntimeContextArgs) -> Self {
        let ObservationRuntimeContextArgs {
            cursor_position_policy,
            scroll_buffer_space,
            tracked_surface,
            tracked_buffer_position,
            cursor_text_context_boundary,
            current_corners,
            ingress_observation_surface,
            buffer_perf_class,
            probe_policy,
        } = args;
        Self {
            cursor_position_policy,
            scroll_buffer_space,
            tracked_surface,
            tracked_buffer_position,
            cursor_text_context_boundary,
            current_corners,
            ingress_observation_surface,
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

    pub(crate) const fn tracked_surface(&self) -> Option<WindowSurfaceSnapshot> {
        self.tracked_surface
    }

    pub(crate) const fn tracked_buffer_position(&self) -> Option<TrackedBufferPosition> {
        self.tracked_buffer_position
    }

    pub(crate) const fn cursor_text_context_boundary(&self) -> Option<CursorTextContextBoundary> {
        self.cursor_text_context_boundary
    }

    pub(crate) const fn current_corners(&self) -> [RenderPoint; 4] {
        self.current_corners
    }

    pub(crate) fn ingress_observation_surface(&self) -> Option<&IngressObservationSurface> {
        self.ingress_observation_surface.as_ref()
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
    pub(crate) observation_id: ObservationId,
    pub(crate) observation_basis: Box<ObservationBasis>,
    pub(crate) cursor_color_probe_generations: Option<CursorColorProbeGenerations>,
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

    pub(crate) fn cursor_color_probe_witness(&self) -> Option<CursorColorProbeWitness> {
        self.cursor_color_probe_generations.and_then(|generations| {
            derive_cursor_color_probe_witness(self.observation_basis.as_ref(), generations)
        })
    }

    pub(crate) const fn probe_request_id(&self) -> ProbeRequestId {
        self.kind.request_id(self.observation_id)
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
    pub(crate) buffer_handle: Option<BufferHandle>,
    pub(crate) requested_at: Millis,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct RenderPlanningObservation {
    observation_id: ObservationId,
    viewport: ViewportBounds,
    background_probe: Option<BackgroundProbeBatch>,
}

impl RenderPlanningObservation {
    pub(crate) const fn new(
        observation_id: ObservationId,
        viewport: ViewportBounds,
        background_probe: Option<BackgroundProbeBatch>,
    ) -> Self {
        Self {
            observation_id,
            viewport,
            background_probe,
        }
    }

    pub(crate) const fn observation_id(&self) -> ObservationId {
        self.observation_id
    }

    pub(crate) const fn viewport(&self) -> ViewportBounds {
        self.viewport
    }

    pub(crate) fn background_probe(&self) -> Option<&BackgroundProbeBatch> {
        self.background_probe.as_ref()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RenderPlanningContext {
    scene: SceneState,
    observation: Option<RenderPlanningObservation>,
    acknowledged_projection: Option<ProjectionHandle>,
    config: Arc<RuntimeConfig>,
}

impl RenderPlanningContext {
    pub(crate) fn new(
        scene: SceneState,
        observation: Option<RenderPlanningObservation>,
        acknowledged_projection: Option<ProjectionHandle>,
        config: Arc<RuntimeConfig>,
    ) -> Self {
        Self {
            scene,
            observation,
            acknowledged_projection,
            config,
        }
    }

    pub(crate) fn observation(&self) -> Option<&RenderPlanningObservation> {
        self.observation.as_ref()
    }

    pub(crate) fn into_parts(
        self,
    ) -> (
        SceneState,
        Option<RenderPlanningObservation>,
        Option<ProjectionHandle>,
        Arc<RuntimeConfig>,
    ) {
        (
            self.scene,
            self.observation,
            self.acknowledged_projection,
            self.config,
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RequestRenderPlanEffect {
    pub(crate) proposal_id: ProposalId,
    pub(crate) planning: RenderPlanningContext,
    pub(crate) render_decision: RenderDecision,
    pub(crate) animation_schedule: AnimationSchedule,
    pub(crate) requested_at: Millis,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum IngressCursorModeAdmission {
    Allowed,
    Blocked,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum IngressCursorCommandLineLocation {
    Outside,
    Inside,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct IngressCursorPresentationRequest {
    mode_admission: IngressCursorModeAdmission,
    command_line_location: IngressCursorCommandLineLocation,
    prepaint_cell: Option<ScreenCell>,
    prepaint_shape: CursorCellShape,
}

impl IngressCursorPresentationRequest {
    pub(crate) const fn new(
        mode_admission: IngressCursorModeAdmission,
        command_line_location: IngressCursorCommandLineLocation,
        prepaint_cell: Option<ScreenCell>,
        prepaint_shape: CursorCellShape,
    ) -> Self {
        Self {
            mode_admission,
            command_line_location,
            prepaint_cell,
            prepaint_shape,
        }
    }

    pub(crate) const fn mode_allowed(self) -> bool {
        matches!(self.mode_admission, IngressCursorModeAdmission::Allowed)
    }

    pub(crate) const fn outside_cmdline(self) -> bool {
        matches!(
            self.command_line_location,
            IngressCursorCommandLineLocation::Outside
        )
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

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum OrderedEffect {
    ScheduleTimer(ScheduleTimerEffect),
    RequestObservationBase(RequestObservationBaseEffect),
    RequestProbe(RequestProbeEffect),
    RequestRenderPlan(Box<RequestRenderPlanEffect>),
    ApplyProposal(Box<ApplyProposalEffect>),
    ApplyRenderCleanup(ApplyRenderCleanupEffect),
    ApplyIngressCursorPresentation(IngressCursorPresentationEffect),
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum ShellOnlyEffect {
    RecordEventLoopMetric(EventLoopMetricEffect),
    RedrawCmdline,
}

impl Effect {
    pub(crate) fn into_ordered_or_shell_only(self) -> Result<OrderedEffect, ShellOnlyEffect> {
        match self {
            Self::ScheduleTimer(payload) => Ok(OrderedEffect::ScheduleTimer(payload)),
            Self::RequestObservationBase(payload) => {
                Ok(OrderedEffect::RequestObservationBase(payload))
            }
            Self::RequestProbe(payload) => Ok(OrderedEffect::RequestProbe(payload)),
            Self::RequestRenderPlan(payload) => Ok(OrderedEffect::RequestRenderPlan(payload)),
            Self::ApplyProposal(payload) => Ok(OrderedEffect::ApplyProposal(payload)),
            Self::ApplyRenderCleanup(payload) => Ok(OrderedEffect::ApplyRenderCleanup(payload)),
            Self::ApplyIngressCursorPresentation(payload) => {
                Ok(OrderedEffect::ApplyIngressCursorPresentation(payload))
            }
            Self::RecordEventLoopMetric(metric) => {
                Err(ShellOnlyEffect::RecordEventLoopMetric(metric))
            }
            Self::RedrawCmdline => Err(ShellOnlyEffect::RedrawCmdline),
        }
    }
}

impl From<OrderedEffect> for Effect {
    fn from(effect: OrderedEffect) -> Self {
        match effect {
            OrderedEffect::ScheduleTimer(payload) => Self::ScheduleTimer(payload),
            OrderedEffect::RequestObservationBase(payload) => Self::RequestObservationBase(payload),
            OrderedEffect::RequestProbe(payload) => Self::RequestProbe(payload),
            OrderedEffect::RequestRenderPlan(payload) => Self::RequestRenderPlan(payload),
            OrderedEffect::ApplyProposal(payload) => Self::ApplyProposal(payload),
            OrderedEffect::ApplyRenderCleanup(payload) => Self::ApplyRenderCleanup(payload),
            OrderedEffect::ApplyIngressCursorPresentation(payload) => {
                Self::ApplyIngressCursorPresentation(payload)
            }
        }
    }
}

impl From<ShellOnlyEffect> for Effect {
    fn from(effect: ShellOnlyEffect) -> Self {
        match effect {
            ShellOnlyEffect::RecordEventLoopMetric(metric) => Self::RecordEventLoopMetric(metric),
            ShellOnlyEffect::RedrawCmdline => Self::RedrawCmdline,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::CursorColorFallbackMode;
    use super::CursorColorReuseMode;
    use super::CursorPositionProbeMode;
    use super::ProbePolicy;
    use super::ProbeQuality;
    use super::RetainedCursorColorFallback;
    use super::TrackedBufferPosition;
    use super::tracked_observation_inputs;
    use crate::core::state::BufferPerfClass;
    use crate::core::state::ExternalDemandKind;
    use crate::position::BufferLine;
    use crate::position::ScreenCell;
    use crate::position::SurfaceId;
    use crate::position::ViewportBounds;
    use crate::position::WindowSurfaceSnapshot;
    use crate::state::TrackedCursor;
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

    fn retained_cursor_color_fallback() -> BoxedStrategy<RetainedCursorColorFallback> {
        prop_oneof![
            Just(RetainedCursorColorFallback::Unavailable),
            Just(RetainedCursorColorFallback::CompatibleSample),
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
            CursorPositionProbeMode::DeferredAllowed
        } else {
            CursorPositionProbeMode::Exact
        }
    }

    const fn expected_reuse_mode(
        demand_kind: ExternalDemandKind,
        retained_cursor_color_fallback: RetainedCursorColorFallback,
    ) -> CursorColorReuseMode {
        match demand_kind {
            ExternalDemandKind::ExternalCursor => retained_cursor_color_fallback.reuse_mode(),
            ExternalDemandKind::ModeChanged
            | ExternalDemandKind::BufferEntered
            | ExternalDemandKind::BoundaryRefresh => CursorColorReuseMode::ExactOnly,
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
            (CursorPositionProbeMode::DeferredAllowed, CursorColorReuseMode::ExactOnly) => {
                "deferred_extmarks"
            }
            (
                CursorPositionProbeMode::DeferredAllowed,
                CursorColorReuseMode::CompatibleWithinLine,
            ) => "deferred_compatible_extmarks",
        }
    }

    #[test]
    fn tracked_observation_inputs_preserve_complete_surface_and_buffer_position() {
        let tracked_cursor = TrackedCursor::fixture(11, 17, 23, 29)
            .with_viewport_columns(5, 2)
            .with_window_origin(7, 13)
            .with_window_dimensions(80, 24);

        assert_eq!(
            tracked_observation_inputs(Some(&tracked_cursor)),
            (
                Some(WindowSurfaceSnapshot::new(
                    SurfaceId::new(11, 17).expect("positive handles"),
                    BufferLine::new(23).expect("positive top buffer line"),
                    5,
                    2,
                    ScreenCell::new(7, 13).expect("one-based window origin"),
                    ViewportBounds::new(24, 80).expect("positive window size"),
                )),
                Some(
                    TrackedBufferPosition::new(
                        17,
                        BufferLine::new(29).expect("positive buffer line"),
                    )
                    .expect("positive buffer handle"),
                ),
            ),
        );
    }

    #[test]
    fn tracked_observation_inputs_return_none_for_absent_tracking() {
        assert_eq!(tracked_observation_inputs(None), (None, None));
    }

    proptest! {
        #![proptest_config(pure_config())]

        #[test]
        fn prop_probe_policy_matches_demand_perf_class_and_retained_color_inputs(
            demand_kind in demand_kind(),
            buffer_perf_class in buffer_perf_class(),
            retained_cursor_color_fallback in retained_cursor_color_fallback(),
        ) {
            let policy = ProbePolicy::for_demand(
                demand_kind,
                buffer_perf_class,
                retained_cursor_color_fallback,
            );

            let expected_quality = expected_quality(demand_kind, buffer_perf_class);
            let expected_position_mode = expected_position_mode(demand_kind, buffer_perf_class);
            let expected_reuse_mode =
                expected_reuse_mode(demand_kind, retained_cursor_color_fallback);

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
