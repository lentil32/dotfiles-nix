use crate::core::effect::{IngressCursorPresentationRequest, TimerKind};
use crate::core::state::{
    ApplyFailureKind, BackgroundProbeBatch, BackgroundProbeChunk, CursorColorSample,
    ExternalDemandKind, ObservationBasis, ObservationMotion, ObservationRequest, PlannedRender,
    ProbeFailure, ProbeReuse, RealizationDivergence,
};
use crate::core::types::{
    CursorPosition, Millis, ObservationId, ProbeRequestId, ProposalId, TimerToken,
};

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct InitializeEvent {
    pub(crate) observed_at: Millis,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ExternalDemandQueuedEvent {
    pub(crate) kind: ExternalDemandKind,
    pub(crate) observed_at: Millis,
    pub(crate) requested_target: Option<CursorPosition>,
    pub(crate) ingress_cursor_presentation: Option<IngressCursorPresentationRequest>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct KeyFallbackQueuedEvent {
    pub(crate) observed_at: Millis,
    pub(crate) due_at: Millis,
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
        allowed_mask: Vec<bool>,
    },
    BackgroundFailed {
        observation_id: ObservationId,
        probe_request_id: ProbeRequestId,
        failure: ProbeFailure,
    },
}

#[derive(Debug, Clone, PartialEq)]
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
    pub(crate) planned_render: PlannedRender,
    pub(crate) observed_at: Millis,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RenderPlanFailedEvent {
    pub(crate) proposal_id: ProposalId,
    pub(crate) observed_at: Millis,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum RenderCleanupAppliedAction {
    SoftCleared,
    HardPurged,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RenderCleanupAppliedEvent {
    pub(crate) observed_at: Millis,
    pub(crate) action: RenderCleanupAppliedAction,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TimerFiredWithTokenEvent {
    pub(crate) kind: TimerKind,
    pub(crate) token: TimerToken,
    pub(crate) observed_at: Millis,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TimerLostWithTokenEvent {
    pub(crate) kind: TimerKind,
    pub(crate) token: TimerToken,
    pub(crate) observed_at: Millis,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum EffectFailureSource {
    PluginEntry,
    KeyListener,
    ScheduledCallback,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct EffectFailedEvent {
    pub(crate) proposal_id: Option<ProposalId>,
    pub(crate) observed_at: Millis,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Event {
    Initialize(InitializeEvent),
    ExternalDemandQueued(ExternalDemandQueuedEvent),
    KeyFallbackQueued(KeyFallbackQueuedEvent),
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
