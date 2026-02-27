use crate::core::runtime_reducer::RenderDecision;
use crate::core::state::{
    BackgroundProbeChunk, CoreState, InFlightProposal, ObservationBasis, ObservationRequest,
    ObservationSnapshot, ProbeKind,
};
use crate::core::types::{
    CursorPosition, DelayBudgetMs, Millis, ProbeRequestId, ProposalId, TimerGeneration, TimerId,
    TimerToken,
};
use crate::state::CursorLocation;
use crate::types::{Point, ScreenCell};

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(crate) enum TimerKind {
    Animation,
    Ingress,
    Recovery,
    Cleanup,
}

impl TimerKind {
    pub(crate) const fn timer_id(self) -> TimerId {
        match self {
            Self::Animation => TimerId::Animation,
            Self::Ingress => TimerId::Ingress,
            Self::Recovery => TimerId::Recovery,
            Self::Cleanup => TimerId::Cleanup,
        }
    }

    const fn fingerprint(self) -> u64 {
        match self {
            Self::Animation => 1_u64,
            Self::Ingress => 2_u64,
            Self::Recovery => 3_u64,
            Self::Cleanup => 4_u64,
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct ScheduleTimerEffect {
    pub(crate) kind: TimerKind,
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

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ObservationRuntimeContext {
    cursor_position_policy: CursorPositionReadPolicy,
    scroll_buffer_space: bool,
    tracked_location: Option<CursorLocation>,
    current_corners: [Point; 4],
}

impl ObservationRuntimeContext {
    pub(crate) const fn new(
        cursor_position_policy: CursorPositionReadPolicy,
        scroll_buffer_space: bool,
        tracked_location: Option<CursorLocation>,
        current_corners: [Point; 4],
    ) -> Self {
        Self {
            cursor_position_policy,
            scroll_buffer_space,
            tracked_location,
            current_corners,
        }
    }

    pub(crate) const fn cursor_position_policy(self) -> CursorPositionReadPolicy {
        self.cursor_position_policy
    }

    pub(crate) const fn scroll_buffer_space(self) -> bool {
        self.scroll_buffer_space
    }

    pub(crate) const fn tracked_location(self) -> Option<CursorLocation> {
        self.tracked_location
    }

    pub(crate) const fn current_corners(self) -> [Point; 4] {
        self.current_corners
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RequestProbeEffect {
    pub(crate) observation_basis: ObservationBasis,
    pub(crate) probe_request_id: ProbeRequestId,
    pub(crate) kind: ProbeKind,
    pub(crate) cursor_position_policy: CursorPositionReadPolicy,
    pub(crate) background_chunk: Option<BackgroundProbeChunk>,
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
    pub(crate) should_schedule_next_animation: bool,
    pub(crate) next_animation_at_ms: Option<Millis>,
    pub(crate) requested_at: Millis,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct IngressCursorPresentationRequest {
    mode_allowed: bool,
    outside_cmdline: bool,
    prepaint_cell: Option<ScreenCell>,
}

impl IngressCursorPresentationRequest {
    pub(crate) const fn new(
        mode_allowed: bool,
        outside_cmdline: bool,
        prepaint_cell: Option<ScreenCell>,
    ) -> Self {
        Self {
            mode_allowed,
            outside_cmdline,
            prepaint_cell,
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
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum IngressCursorPresentationEffect {
    HideCursor,
    HideCursorAndPrepaint { cell: ScreenCell, zindex: u32 },
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum RenderCleanupExecution {
    SoftClear { max_kept_windows: usize },
    HardPurge,
}

impl RenderCleanupExecution {
    const fn fingerprint(self) -> u64 {
        match self {
            Self::SoftClear { max_kept_windows } => {
                1_u64 ^ (max_kept_windows as u64).rotate_left(7)
            }
            Self::HardPurge => 2_u64,
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct ApplyRenderCleanupEffect {
    pub(crate) execution: RenderCleanupExecution,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum EventLoopMetricEffect {
    IngressCoalesced,
    StaleToken,
    ProbeRefreshRetried(ProbeKind),
    ProbeRefreshBudgetExhausted(ProbeKind),
}

impl EventLoopMetricEffect {
    const fn fingerprint(self) -> u64 {
        match self {
            Self::IngressCoalesced => 1_u64,
            Self::StaleToken => 2_u64,
            Self::ProbeRefreshRetried(kind) => 3_u64 ^ kind.fingerprint().rotate_left(7),
            Self::ProbeRefreshBudgetExhausted(kind) => 4_u64 ^ kind.fingerprint().rotate_left(7),
        }
    }
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

fn cursor_location_fingerprint(location: CursorLocation) -> u64 {
    u64::from_ne_bytes(location.window_handle.to_ne_bytes())
        ^ u64::from_ne_bytes(location.buffer_handle.to_ne_bytes()).rotate_left(7)
        ^ u64::from_ne_bytes(location.top_row.to_ne_bytes()).rotate_left(13)
        ^ u64::from_ne_bytes(location.line.to_ne_bytes()).rotate_left(19)
}

fn point_fingerprint(point: Point) -> u64 {
    point.row.to_bits() ^ point.col.to_bits().rotate_left(11)
}

fn screen_cell_fingerprint(cell: ScreenCell) -> u64 {
    u64::from_ne_bytes(cell.row().to_ne_bytes())
        ^ u64::from_ne_bytes(cell.col().to_ne_bytes()).rotate_left(7)
}

fn observation_context_fingerprint(context: ObservationRuntimeContext) -> u64 {
    let cursor_position_seed = if context.cursor_position_policy().smear_to_cmd() {
        1_u64
    } else {
        0_u64
    };
    let scroll_seed = if context.scroll_buffer_space() {
        1_u64
    } else {
        0_u64
    };
    let tracked_seed = context
        .tracked_location()
        .map_or(0_u64, cursor_location_fingerprint);
    let corner_seed = context
        .current_corners()
        .into_iter()
        .map(point_fingerprint)
        .fold(0_u64, u64::wrapping_add);

    cursor_position_seed
        ^ scroll_seed.rotate_left(5)
        ^ tracked_seed.rotate_left(11)
        ^ corner_seed.rotate_left(17)
}

fn cursor_color_witness_fingerprint(
    witness: Option<&crate::core::state::CursorColorProbeWitness>,
) -> u64 {
    let Some(witness) = witness else {
        return 0_u64;
    };

    witness.buffer_handle().unsigned_abs()
        ^ witness.changedtick().rotate_left(7)
        ^ witness
            .mode()
            .bytes()
            .fold(0_u64, |seed, byte| seed.rotate_left(5) ^ u64::from(byte))
            .rotate_left(13)
        ^ witness
            .cursor_position()
            .map_or(0_u64, CursorPosition::fingerprint)
            .rotate_left(19)
        ^ witness.colorscheme_generation().value().rotate_left(23)
}

impl Effect {
    pub(crate) fn fingerprint(&self) -> u64 {
        match self {
            Self::ScheduleTimer(payload) => {
                101_u64
                    ^ payload.kind.fingerprint()
                    ^ payload.token.fingerprint()
                    ^ payload.delay.value()
                    ^ payload.requested_at.value()
            }
            Self::RequestObservationBase(payload) => {
                let probe_seed = if payload.request.probes().cursor_color() {
                    1_u64
                } else {
                    0_u64
                } ^ if payload.request.probes().background() {
                    2_u64
                } else {
                    0_u64
                };
                105_u64
                    ^ payload.request.observation_id().value()
                    ^ payload.request.demand().seq().value()
                    ^ payload.request.demand().observed_at().value()
                    ^ probe_seed.rotate_left(11)
                    ^ observation_context_fingerprint(payload.context).rotate_left(17)
                    ^ payload
                        .request
                        .demand()
                        .requested_target()
                        .map_or(0_u64, CursorPosition::fingerprint)
            }
            Self::RequestProbe(payload) => {
                let basis = &payload.observation_basis;
                let cursor_seed = basis
                    .cursor_position()
                    .map_or(0_u64, CursorPosition::fingerprint);
                let viewport = basis.viewport();
                let background_chunk_seed = payload.background_chunk.map_or(0_u64, |chunk| {
                    u64::from(chunk.start_row().value())
                        ^ u64::from(chunk.row_count()).rotate_left(7)
                });
                let cursor_color_witness_seed =
                    cursor_color_witness_fingerprint(basis.cursor_color_witness());
                109_u64
                    ^ basis.observation_id().value()
                    ^ basis.observed_at().value()
                    ^ payload.probe_request_id.value().rotate_left(7)
                    ^ payload.kind.fingerprint().rotate_left(13)
                    ^ (if payload.cursor_position_policy.smear_to_cmd() {
                        1_u64
                    } else {
                        0_u64
                    })
                    .rotate_left(17)
                    ^ cursor_seed.rotate_left(19)
                    ^ u64::from(viewport.max_row.value()).rotate_left(23)
                    ^ u64::from(viewport.max_col.value()).rotate_left(29)
                    ^ background_chunk_seed.rotate_left(31)
                    ^ cursor_color_witness_seed.rotate_left(37)
            }
            Self::RequestRenderPlan(payload) => {
                111_u64
                    ^ payload.proposal_id.value()
                    ^ payload.planning_state.generation().value().rotate_left(7)
                    ^ payload
                        .observation
                        .basis()
                        .observation_id()
                        .value()
                        .rotate_left(13)
                    ^ payload.requested_at.value().rotate_left(19)
            }
            Self::ApplyProposal(payload) => {
                let proposal = &payload.proposal;
                let basis = proposal.patch().basis();
                let acknowledged_seed = basis.acknowledged().map_or(0_u64, |snapshot| {
                    snapshot.witness().scene_revision().value()
                        ^ snapshot.witness().observation_id().value()
                });
                let target_seed = basis.target().map_or(0_u64, |snapshot| {
                    snapshot.witness().scene_revision().value()
                        ^ snapshot.witness().observation_id().value()
                });
                let animation_seed = if proposal.should_schedule_next_animation() {
                    1_u64
                } else {
                    0_u64
                };
                113_u64
                    ^ proposal.proposal_id().value()
                    ^ acknowledged_seed
                    ^ target_seed
                    ^ animation_seed
                    ^ proposal.next_animation_at_ms().map_or(0_u64, Millis::value)
                    ^ payload.requested_at.value()
            }
            Self::ApplyRenderCleanup(payload) => {
                127_u64 ^ payload.execution.fingerprint().rotate_left(7)
            }
            Self::ApplyIngressCursorPresentation(payload) => match payload {
                IngressCursorPresentationEffect::HideCursor => 131_u64,
                IngressCursorPresentationEffect::HideCursorAndPrepaint { cell, zindex } => {
                    137_u64 ^ screen_cell_fingerprint(*cell) ^ u64::from(*zindex).rotate_left(11)
                }
            },
            Self::RecordEventLoopMetric(metric) => 139_u64 ^ metric.fingerprint().rotate_left(7),
            Self::RedrawCmdline => 149_u64,
        }
    }
}

pub(crate) fn phase4_effect_fingerprint_seed() -> u64 {
    let at = Millis::new(1);
    let schedule_kind = TimerKind::Animation;
    let schedule_token = TimerToken::new(TimerId::Animation, TimerGeneration::new(1));
    let ingress_kind = TimerKind::Ingress;
    let ingress_token = TimerToken::new(TimerId::Ingress, TimerGeneration::new(2));
    let cleanup_kind = TimerKind::Cleanup;
    let cleanup_token = TimerToken::new(TimerId::Cleanup, TimerGeneration::new(3));
    let ingress_delay = match DelayBudgetMs::try_new(9) {
        Ok(delay) => delay,
        Err(_) => DelayBudgetMs::DEFAULT_ANIMATION,
    };
    let cleanup_delay = match DelayBudgetMs::try_new(220) {
        Ok(delay) => delay,
        Err(_) => DelayBudgetMs::DEFAULT_ANIMATION,
    };
    let proposal_id = ProposalId::new(3);
    let request = ObservationRequest::new(
        crate::core::state::ExternalDemand::new(
            crate::core::types::IngressSeq::new(4),
            crate::core::state::ExternalDemandKind::ExternalCursor,
            at,
            None,
        ),
        crate::core::state::ProbeRequestSet::default(),
    );
    let observation = crate::core::state::ObservationSnapshot::new(
        request.clone(),
        crate::core::state::ObservationBasis::new(
            request.observation_id(),
            at,
            "n".to_string(),
            Some(CursorPosition {
                row: crate::core::types::CursorRow(1),
                col: crate::core::types::CursorCol(1),
            }),
            crate::state::CursorLocation::new(1, 1, 1, 1),
            crate::core::types::ViewportSnapshot::new(
                crate::core::types::CursorRow(40),
                crate::core::types::CursorCol(120),
            ),
        ),
        crate::core::state::ProbeSet::default(),
        crate::core::state::ObservationMotion::default(),
    );

    let effects = [
        Effect::ScheduleTimer(ScheduleTimerEffect {
            kind: schedule_kind,
            token: schedule_token,
            delay: DelayBudgetMs::DEFAULT_ANIMATION,
            requested_at: at,
        }),
        Effect::ScheduleTimer(ScheduleTimerEffect {
            kind: ingress_kind,
            token: ingress_token,
            delay: ingress_delay,
            requested_at: at,
        }),
        Effect::ScheduleTimer(ScheduleTimerEffect {
            kind: cleanup_kind,
            token: cleanup_token,
            delay: cleanup_delay,
            requested_at: at,
        }),
        Effect::RequestObservationBase(RequestObservationBaseEffect {
            request: request.clone(),
            context: ObservationRuntimeContext::new(
                CursorPositionReadPolicy::new(true),
                true,
                Some(crate::state::CursorLocation::new(10, 11, 12, 13)),
                [
                    Point { row: 1.0, col: 1.0 },
                    Point { row: 1.0, col: 2.0 },
                    Point { row: 2.0, col: 2.0 },
                    Point { row: 2.0, col: 1.0 },
                ],
            ),
        }),
        Effect::RequestProbe(RequestProbeEffect {
            observation_basis: crate::core::state::ObservationBasis::new(
                crate::core::types::ObservationId::from_ingress_seq(
                    crate::core::types::IngressSeq::new(4),
                ),
                at,
                "n".to_string(),
                Some(CursorPosition {
                    row: crate::core::types::CursorRow(1),
                    col: crate::core::types::CursorCol(1),
                }),
                crate::state::CursorLocation::new(1, 1, 1, 1),
                crate::core::types::ViewportSnapshot::new(
                    crate::core::types::CursorRow(40),
                    crate::core::types::CursorCol(120),
                ),
            ),
            probe_request_id: ProbeKind::CursorColor.request_id(
                crate::core::types::ObservationId::from_ingress_seq(
                    crate::core::types::IngressSeq::new(4),
                ),
            ),
            kind: ProbeKind::CursorColor,
            cursor_position_policy: CursorPositionReadPolicy::new(true),
            background_chunk: None,
        }),
        Effect::RequestRenderPlan(Box::new(RequestRenderPlanEffect {
            proposal_id,
            planning_state: crate::core::state::CoreState::default(),
            observation,
            render_decision: crate::core::runtime_reducer::RenderDecision {
                render_action: crate::core::runtime_reducer::RenderAction::Noop,
                render_cleanup_action: crate::core::runtime_reducer::RenderCleanupAction::NoAction,
                render_allocation_policy:
                    crate::core::runtime_reducer::RenderAllocationPolicy::ReuseOnly,
                render_side_effects: crate::core::runtime_reducer::RenderSideEffects::default(),
            },
            should_schedule_next_animation: false,
            next_animation_at_ms: None,
            requested_at: at,
        })),
        Effect::ApplyProposal(Box::new(ApplyProposalEffect {
            proposal: crate::core::state::InFlightProposal::new(
                proposal_id,
                crate::core::state::PatchBasis::new(None, None),
                crate::core::state::ScenePatch::derive(crate::core::state::PatchBasis::new(
                    None, None,
                )),
                crate::core::state::RealizationPlan::Noop,
                crate::core::runtime_reducer::RenderCleanupAction::NoAction,
                crate::core::runtime_reducer::RenderSideEffects::default(),
                false,
                None,
            ),
            requested_at: at,
        })),
        Effect::ApplyRenderCleanup(ApplyRenderCleanupEffect {
            execution: RenderCleanupExecution::SoftClear {
                max_kept_windows: 24,
            },
        }),
        Effect::ApplyIngressCursorPresentation(
            IngressCursorPresentationEffect::HideCursorAndPrepaint {
                cell: match ScreenCell::new(4, 5) {
                    Some(cell) => cell,
                    None => return 0_u64,
                },
                zindex: 200,
            },
        ),
        Effect::RedrawCmdline,
    ];

    effects
        .iter()
        .map(Effect::fingerprint)
        .fold(0_u64, u64::wrapping_add)
}
