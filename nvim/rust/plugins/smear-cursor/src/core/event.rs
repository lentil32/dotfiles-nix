use crate::core::effect::IngressCursorPresentationRequest;
use crate::core::state::ApplyFailureKind;
use crate::core::state::BackgroundProbeBatch;
use crate::core::state::BackgroundProbeChunk;
use crate::core::state::BackgroundProbeChunkMask;
use crate::core::state::BufferPerfClass;
use crate::core::state::CursorColorSample;
use crate::core::state::ExternalDemandKind;
use crate::core::state::ObservationBasis;
use crate::core::state::ObservationMotion;
use crate::core::state::ObservationRequest;
use crate::core::state::PlannedRender;
use crate::core::state::ProbeFailure;
use crate::core::state::ProbeReuse;
use crate::core::state::RealizationDivergence;
use crate::core::types::CursorPosition;
use crate::core::types::Millis;
use crate::core::types::ObservationId;
use crate::core::types::ProbeRequestId;
use crate::core::types::ProposalId;
use crate::core::types::TimerToken;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct InitializeEvent {
    pub(crate) observed_at: Millis,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct ExternalDemandQueuedEvent {
    pub(crate) kind: ExternalDemandKind,
    pub(crate) observed_at: Millis,
    pub(crate) requested_target: Option<CursorPosition>,
    pub(crate) buffer_perf_class: BufferPerfClass,
    pub(crate) ingress_cursor_presentation: Option<IngressCursorPresentationRequest>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ObservationBaseCollectedEvent {
    pub(crate) request: ObservationRequest,
    pub(crate) basis: ObservationBasis,
    pub(crate) motion: ObservationMotion,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ProbeReportedEvent {
    CursorColorReady {
        observation_id: ObservationId,
        probe_request_id: ProbeRequestId,
        reuse: ProbeReuse,
        sample: Option<CursorColorSample>,
    },
    CursorColorFailed {
        observation_id: ObservationId,
        probe_request_id: ProbeRequestId,
        failure: ProbeFailure,
    },
    BackgroundReady {
        observation_id: ObservationId,
        probe_request_id: ProbeRequestId,
        reuse: ProbeReuse,
        batch: BackgroundProbeBatch,
    },
    BackgroundChunkReady {
        observation_id: ObservationId,
        probe_request_id: ProbeRequestId,
        chunk: BackgroundProbeChunk,
        allowed_mask: BackgroundProbeChunkMask,
    },
    BackgroundFailed {
        observation_id: ObservationId,
        probe_request_id: ProbeRequestId,
        failure: ProbeFailure,
    },
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum ApplyReport {
    AppliedFully {
        proposal_id: ProposalId,
        observed_at: Millis,
        visual_change: bool,
    },
    AppliedDegraded {
        proposal_id: ProposalId,
        divergence: RealizationDivergence,
        observed_at: Millis,
        visual_change: bool,
    },
    ApplyFailed {
        proposal_id: ProposalId,
        reason: ApplyFailureKind,
        divergence: RealizationDivergence,
        observed_at: Millis,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RenderPlanComputedEvent {
    pub(crate) proposal_id: ProposalId,
    pub(crate) planned_render: Box<PlannedRender>,
    pub(crate) observed_at: Millis,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct RenderPlanFailedEvent {
    pub(crate) proposal_id: ProposalId,
    pub(crate) observed_at: Millis,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum RenderCleanupAppliedAction {
    SoftCleared,
    CompactedToBudget { converged_to_idle: bool },
    HardPurged,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct RenderCleanupAppliedEvent {
    pub(crate) observed_at: Millis,
    pub(crate) action: RenderCleanupAppliedAction,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct TimerFiredWithTokenEvent {
    pub(crate) token: TimerToken,
    pub(crate) observed_at: Millis,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct TimerLostWithTokenEvent {
    pub(crate) token: TimerToken,
    pub(crate) observed_at: Millis,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum EffectFailureSource {
    PluginEntry,
    ScheduledCallback,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct EffectFailedEvent {
    pub(crate) proposal_id: Option<ProposalId>,
    pub(crate) observed_at: Millis,
}

#[expect(
    clippy::large_enum_variant,
    reason = "Reducer events stay fully typed values; boxing the largest payload would spread allocation through the state machine hot path."
)]
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Event {
    Initialize(InitializeEvent),
    ExternalDemandQueued(ExternalDemandQueuedEvent),
    ObservationBaseCollected(ObservationBaseCollectedEvent),
    ProbeReported(ProbeReportedEvent),
    RenderPlanComputed(RenderPlanComputedEvent),
    RenderPlanFailed(RenderPlanFailedEvent),
    ApplyReported(ApplyReport),
    RenderCleanupApplied(RenderCleanupAppliedEvent),
    TimerFiredWithToken(TimerFiredWithTokenEvent),
    TimerLostWithToken(TimerLostWithTokenEvent),
    EffectFailed(EffectFailedEvent),
}
