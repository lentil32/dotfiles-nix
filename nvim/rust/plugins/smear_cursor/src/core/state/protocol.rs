use super::{
    DemandQueue, EntropyState, ExternalDemand, InFlightProposal, IngressPolicyState,
    ObservationRequest, ObservationSnapshot, ProbeRefreshState, RealizationLedger,
    RecoveryPolicyState, RenderCleanupState, SceneState, TimerState,
};
use crate::core::types::{CursorPosition, Generation, IngressSeq, Lifecycle, ProposalId};

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct ProtocolSharedState {
    demand: DemandQueue,
    timers: TimerState,
    recovery_policy: RecoveryPolicyState,
    ingress_policy: IngressPolicyState,
    render_cleanup: RenderCleanupState,
}

impl ProtocolSharedState {
    pub(crate) const fn demand(&self) -> &DemandQueue {
        &self.demand
    }

    pub(crate) const fn timers(&self) -> TimerState {
        self.timers
    }

    pub(crate) const fn recovery_policy(&self) -> RecoveryPolicyState {
        self.recovery_policy
    }

    pub(crate) const fn ingress_policy(&self) -> IngressPolicyState {
        self.ingress_policy
    }

    pub(crate) const fn render_cleanup(&self) -> RenderCleanupState {
        self.render_cleanup
    }

    pub(crate) fn with_demand(mut self, demand: DemandQueue) -> Self {
        self.demand = demand;
        self
    }

    pub(crate) fn with_timers(mut self, timers: TimerState) -> Self {
        self.timers = timers;
        self
    }

    pub(crate) fn with_recovery_policy(mut self, recovery_policy: RecoveryPolicyState) -> Self {
        self.recovery_policy = recovery_policy;
        self
    }

    pub(crate) fn with_ingress_policy(mut self, ingress_policy: IngressPolicyState) -> Self {
        self.ingress_policy = ingress_policy;
        self
    }

    pub(crate) fn with_render_cleanup(mut self, render_cleanup: RenderCleanupState) -> Self {
        self.render_cleanup = render_cleanup;
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ProtocolState {
    Idle {
        shared: ProtocolSharedState,
    },
    // Comment: initialization now owns only the protocol bootstrap edge; all planning reads
    // enter through the observation shell after the first external demand arrives.
    Primed {
        shared: ProtocolSharedState,
    },
    Observing {
        shared: ProtocolSharedState,
        request: ObservationRequest,
        observation: Option<ObservationSnapshot>,
        probe_refresh: ProbeRefreshState,
    },
    Ready {
        shared: ProtocolSharedState,
        observation: ObservationSnapshot,
    },
    Planning {
        shared: ProtocolSharedState,
        observation: ObservationSnapshot,
        proposal_id: ProposalId,
    },
    Applying {
        shared: ProtocolSharedState,
        observation: ObservationSnapshot,
        proposal: Box<InFlightProposal>,
    },
    Recovering {
        shared: ProtocolSharedState,
        observation: Option<ObservationSnapshot>,
    },
}

impl Default for ProtocolState {
    fn default() -> Self {
        Self::Idle {
            shared: ProtocolSharedState::default(),
        }
    }
}

impl ProtocolState {
    const fn shared(&self) -> &ProtocolSharedState {
        match self {
            Self::Idle { shared }
            | Self::Primed { shared }
            | Self::Observing { shared, .. }
            | Self::Ready { shared, .. }
            | Self::Planning { shared, .. }
            | Self::Applying { shared, .. }
            | Self::Recovering { shared, .. } => shared,
        }
    }

    fn with_shared(self, shared: ProtocolSharedState) -> Self {
        match self {
            Self::Idle { .. } => Self::Idle { shared },
            Self::Primed { .. } => Self::Primed { shared },
            Self::Observing {
                request,
                observation,
                probe_refresh,
                ..
            } => Self::Observing {
                shared,
                request,
                observation,
                probe_refresh,
            },
            Self::Ready { observation, .. } => Self::Ready {
                shared,
                observation,
            },
            Self::Planning {
                observation,
                proposal_id,
                ..
            } => Self::Planning {
                shared,
                observation,
                proposal_id,
            },
            Self::Applying {
                observation,
                proposal,
                ..
            } => Self::Applying {
                shared,
                observation,
                proposal,
            },
            Self::Recovering { observation, .. } => Self::Recovering {
                shared,
                observation,
            },
        }
    }

    pub(crate) const fn lifecycle(&self) -> Lifecycle {
        match self {
            Self::Idle { .. } => Lifecycle::Idle,
            Self::Primed { .. } => Lifecycle::Primed,
            Self::Observing { .. } => Lifecycle::Observing,
            Self::Ready { .. } => Lifecycle::Ready,
            Self::Planning { .. } => Lifecycle::Planning,
            Self::Applying { .. } => Lifecycle::Applying,
            Self::Recovering { .. } => Lifecycle::Recovering,
        }
    }

    pub(crate) const fn demand(&self) -> &DemandQueue {
        self.shared().demand()
    }

    pub(crate) const fn timers(&self) -> TimerState {
        self.shared().timers()
    }

    pub(crate) const fn recovery_policy(&self) -> RecoveryPolicyState {
        self.shared().recovery_policy()
    }

    pub(crate) const fn ingress_policy(&self) -> IngressPolicyState {
        self.shared().ingress_policy()
    }

    pub(crate) const fn render_cleanup(&self) -> RenderCleanupState {
        self.shared().render_cleanup()
    }

    pub(crate) fn observation(&self) -> Option<&ObservationSnapshot> {
        match self {
            Self::Idle { .. } | Self::Primed { .. } => None,
            Self::Observing { observation, .. } | Self::Recovering { observation, .. } => {
                observation.as_ref()
            }
            Self::Ready { observation, .. }
            | Self::Planning { observation, .. }
            | Self::Applying { observation, .. } => Some(observation),
        }
    }

    pub(crate) fn with_demand(self, next_demand: DemandQueue) -> Self {
        let shared = self.shared().clone().with_demand(next_demand);
        self.with_shared(shared)
    }

    pub(crate) fn with_timers(self, timers: TimerState) -> Self {
        let shared = self.shared().clone().with_timers(timers);
        self.with_shared(shared)
    }

    pub(crate) fn with_recovery_policy(self, recovery_policy: RecoveryPolicyState) -> Self {
        let shared = self.shared().clone().with_recovery_policy(recovery_policy);
        self.with_shared(shared)
    }

    pub(crate) fn with_ingress_policy(self, ingress_policy: IngressPolicyState) -> Self {
        let shared = self.shared().clone().with_ingress_policy(ingress_policy);
        self.with_shared(shared)
    }

    pub(crate) fn with_render_cleanup(self, render_cleanup: RenderCleanupState) -> Self {
        let shared = self.shared().clone().with_render_cleanup(render_cleanup);
        self.with_shared(shared)
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct CoreStatePayload {
    pub(crate) entropy: EntropyState,
    pub(crate) last_cursor: Option<CursorPosition>,
    pub(crate) scene: SceneState,
    pub(crate) realization: RealizationLedger,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CoreState {
    generation: Generation,
    protocol: ProtocolState,
    payload: CoreStatePayload,
}

impl Default for CoreState {
    fn default() -> Self {
        Self {
            generation: Generation::INITIAL,
            protocol: ProtocolState::default(),
            payload: CoreStatePayload::default(),
        }
    }
}

impl CoreState {
    pub(crate) const fn generation(&self) -> Generation {
        self.generation
    }

    pub(crate) fn next_generation(&self) -> Generation {
        self.generation.next()
    }

    pub(crate) const fn lifecycle(&self) -> Lifecycle {
        self.protocol.lifecycle()
    }

    pub(crate) const fn protocol(&self) -> &ProtocolState {
        &self.protocol
    }

    pub(crate) const fn needs_initialize(&self) -> bool {
        matches!(self.protocol, ProtocolState::Idle { .. })
    }

    pub(crate) const fn last_cursor(&self) -> Option<CursorPosition> {
        self.payload.last_cursor
    }

    pub(crate) const fn timers(&self) -> TimerState {
        self.protocol.timers()
    }

    pub(crate) const fn recovery_policy(&self) -> RecoveryPolicyState {
        self.protocol.recovery_policy()
    }

    pub(crate) const fn ingress_policy(&self) -> IngressPolicyState {
        self.protocol.ingress_policy()
    }

    pub(crate) const fn render_cleanup(&self) -> RenderCleanupState {
        self.protocol.render_cleanup()
    }

    pub(crate) const fn entropy(&self) -> EntropyState {
        self.payload.entropy
    }

    pub(crate) const fn demand_queue(&self) -> &DemandQueue {
        self.protocol.demand()
    }

    pub(crate) fn active_demand(&self) -> Option<&ExternalDemand> {
        match &self.protocol {
            ProtocolState::Observing { request, .. } => Some(request.demand()),
            _ => None,
        }
    }

    pub(crate) fn active_observation_request(&self) -> Option<&ObservationRequest> {
        match &self.protocol {
            ProtocolState::Observing { request, .. } => Some(request),
            _ => None,
        }
    }

    pub(crate) fn observation(&self) -> Option<&ObservationSnapshot> {
        self.protocol.observation()
    }

    pub(crate) const fn scene(&self) -> &SceneState {
        &self.payload.scene
    }

    pub(crate) const fn realization(&self) -> &RealizationLedger {
        &self.payload.realization
    }

    pub(crate) const fn runtime(&self) -> &crate::state::RuntimeState {
        self.payload.scene.motion()
    }

    pub(crate) fn pending_proposal(&self) -> Option<&InFlightProposal> {
        match &self.protocol {
            ProtocolState::Applying { proposal, .. } => Some(proposal.as_ref()),
            _ => None,
        }
    }

    pub(crate) const fn pending_plan_proposal_id(&self) -> Option<ProposalId> {
        match &self.protocol {
            ProtocolState::Planning { proposal_id, .. } => Some(*proposal_id),
            _ => None,
        }
    }

    pub(crate) fn with_timers(self, timers: TimerState) -> Self {
        let protocol = self.protocol.clone().with_timers(timers);
        self.with_protocol(protocol)
    }

    pub(crate) fn with_recovery_policy(self, recovery_policy: RecoveryPolicyState) -> Self {
        let protocol = self.protocol.clone().with_recovery_policy(recovery_policy);
        self.with_protocol(protocol)
    }

    pub(crate) fn with_ingress_policy(self, ingress_policy: IngressPolicyState) -> Self {
        let protocol = self.protocol.clone().with_ingress_policy(ingress_policy);
        self.with_protocol(protocol)
    }

    pub(crate) fn with_render_cleanup(self, render_cleanup: RenderCleanupState) -> Self {
        let protocol = self.protocol.clone().with_render_cleanup(render_cleanup);
        self.with_protocol(protocol)
    }

    pub(crate) fn with_entropy(mut self, entropy: EntropyState) -> Self {
        self.payload.entropy = entropy;
        self
    }

    pub(crate) fn with_last_cursor(mut self, last_cursor: Option<CursorPosition>) -> Self {
        self.payload.last_cursor = last_cursor;
        self
    }

    pub(crate) fn with_scene(mut self, scene: SceneState) -> Self {
        self.payload.scene = scene;
        self
    }

    pub(crate) fn with_realization(mut self, realization: RealizationLedger) -> Self {
        self.payload.realization = realization;
        self
    }

    pub(crate) fn with_runtime(mut self, runtime: crate::state::RuntimeState) -> Self {
        self.payload.scene = self.payload.scene.clone().with_motion(runtime);
        self
    }

    pub(crate) fn with_protocol(mut self, protocol: ProtocolState) -> Self {
        if self.protocol.lifecycle() != protocol.lifecycle() {
            self.generation = self.next_generation();
        }
        self.protocol = protocol;
        self
    }

    pub(crate) fn with_demand_queue(self, demand: DemandQueue) -> Self {
        let protocol = self.protocol.clone().with_demand(demand);
        self.with_protocol(protocol)
    }

    pub(crate) fn initialize(self) -> Self {
        let shared = self.protocol.shared().clone();
        self.with_protocol(ProtocolState::Primed { shared })
    }

    pub(crate) fn into_observing(
        self,
        request: ObservationRequest,
        remaining: DemandQueue,
    ) -> Self {
        let observation = self.observation().cloned();
        let shared = self.protocol.shared().clone().with_demand(remaining);
        self.with_protocol(ProtocolState::Observing {
            shared,
            request,
            observation,
            probe_refresh: ProbeRefreshState::default(),
        })
    }

    pub(crate) fn with_active_observation(
        self,
        observation: Option<ObservationSnapshot>,
    ) -> Option<Self> {
        let probe_refresh = self.probe_refresh_state()?;
        let ProtocolState::Observing {
            shared, request, ..
        } = self.protocol().clone()
        else {
            return None;
        };
        Some(self.with_protocol(ProtocolState::Observing {
            shared,
            request,
            observation,
            probe_refresh,
        }))
    }

    pub(crate) const fn probe_refresh_state(&self) -> Option<ProbeRefreshState> {
        match self.protocol() {
            ProtocolState::Observing { probe_refresh, .. } => Some(*probe_refresh),
            _ => None,
        }
    }

    pub(crate) fn with_probe_refresh_state(self, probe_refresh: ProbeRefreshState) -> Option<Self> {
        let ProtocolState::Observing {
            shared,
            request,
            observation,
            ..
        } = self.protocol().clone()
        else {
            return None;
        };
        Some(self.with_protocol(ProtocolState::Observing {
            shared,
            request,
            observation,
            probe_refresh,
        }))
    }

    pub(crate) fn into_ready_with_observation(self, observation: ObservationSnapshot) -> Self {
        let shared = self.protocol.shared().clone();
        self.with_protocol(ProtocolState::Ready {
            shared,
            observation,
        })
    }

    pub(crate) fn into_planning(self, proposal_id: ProposalId) -> Option<Self> {
        let observation = self.observation()?.clone();
        let shared = self.protocol.shared().clone();
        Some(self.with_protocol(ProtocolState::Planning {
            shared,
            observation,
            proposal_id,
        }))
    }

    pub(crate) fn into_primed(self) -> Self {
        let shared = self.protocol.shared().clone();
        self.with_protocol(ProtocolState::Primed { shared })
    }

    pub(crate) fn into_applying(self, proposal: InFlightProposal) -> Option<Self> {
        let observation = self.observation()?.clone();
        let shared = self.protocol.shared().clone();
        Some(self.with_protocol(ProtocolState::Applying {
            shared,
            observation,
            proposal: Box::new(proposal),
        }))
    }

    pub(crate) fn into_recovering(self) -> Self {
        let observation = self.observation().cloned();
        let shared = self.protocol.shared().clone();
        self.with_protocol(ProtocolState::Recovering {
            shared,
            observation,
        })
    }

    pub(crate) fn allocate_ingress_seq(self) -> (Self, IngressSeq) {
        let (entropy, seq) = self.entropy().allocate_ingress_seq();
        (self.with_entropy(entropy), seq)
    }

    pub(crate) fn allocate_proposal_id(self) -> (Self, ProposalId) {
        let (entropy, proposal_id) = self.entropy().allocate_proposal_id();
        (self.with_entropy(entropy), proposal_id)
    }

    pub(crate) fn clear_pending_for(
        self,
        proposal_id: ProposalId,
    ) -> Option<(Self, InFlightProposal)> {
        let pending = self.pending_proposal()?.clone();
        if pending.proposal_id() != proposal_id {
            return None;
        }

        let observation = self.observation()?.clone();
        let shared = self.protocol.shared().clone();
        let next_state = self.with_protocol(ProtocolState::Ready {
            shared,
            observation,
        });
        Some((next_state, pending))
    }

    pub(crate) fn accept_planned_render(
        self,
        planned_render: crate::core::state::PlannedRender,
    ) -> Option<Self> {
        let proposal_id = planned_render.proposal_id();
        let (next_scene, proposal) = planned_render.into_parts();
        let ProtocolState::Planning {
            shared,
            observation,
            proposal_id: active_proposal_id,
        } = self.protocol().clone()
        else {
            return None;
        };
        if active_proposal_id != proposal_id {
            return None;
        }

        Some(
            self.with_scene(next_scene)
                .with_protocol(ProtocolState::Applying {
                    shared,
                    observation,
                    proposal: Box::new(proposal),
                }),
        )
    }
}
