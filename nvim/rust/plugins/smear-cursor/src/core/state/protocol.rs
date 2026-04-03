use super::DemandQueue;
use super::EntropyState;
use super::ExternalDemand;
use super::InFlightProposal;
use super::IngressPolicyState;
use super::ObservationRequest;
use super::ObservationSnapshot;
use super::ProbeRefreshState;
use super::RealizationLedger;
use super::RecoveryPolicyState;
use super::RenderCleanupState;
use super::SceneState;
use super::TimerState;
use crate::core::types::CursorPosition;
use crate::core::types::Generation;
use crate::core::types::IngressSeq;
use crate::core::types::Lifecycle;
use crate::core::types::ProposalId;

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
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ProtocolState {
    Idle {
        shared: ProtocolSharedState,
    },
    // initialization now owns only the protocol bootstrap edge; all planning reads
    // enter through the observation shell after the first external demand arrives.
    Primed {
        shared: ProtocolSharedState,
    },
    ObservingRequest {
        shared: ProtocolSharedState,
        request: ObservationRequest,
        retained_observation: Option<ObservationSnapshot>,
        probe_refresh: ProbeRefreshState,
    },
    ObservingActive {
        shared: ProtocolSharedState,
        request: ObservationRequest,
        observation: ObservationSnapshot,
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
            | Self::ObservingRequest { shared, .. }
            | Self::ObservingActive { shared, .. }
            | Self::Ready { shared, .. }
            | Self::Planning { shared, .. }
            | Self::Applying { shared, .. }
            | Self::Recovering { shared, .. } => shared,
        }
    }

    fn shared_mut(&mut self) -> &mut ProtocolSharedState {
        match self {
            Self::Idle { shared }
            | Self::Primed { shared }
            | Self::ObservingRequest { shared, .. }
            | Self::ObservingActive { shared, .. }
            | Self::Ready { shared, .. }
            | Self::Planning { shared, .. }
            | Self::Applying { shared, .. }
            | Self::Recovering { shared, .. } => shared,
        }
    }

    fn into_shared(self) -> ProtocolSharedState {
        match self {
            Self::Idle { shared }
            | Self::Primed { shared }
            | Self::ObservingRequest { shared, .. }
            | Self::ObservingActive { shared, .. }
            | Self::Ready { shared, .. }
            | Self::Planning { shared, .. }
            | Self::Applying { shared, .. }
            | Self::Recovering { shared, .. } => shared,
        }
    }

    fn into_shared_and_retained_observation(
        self,
    ) -> (ProtocolSharedState, Option<ObservationSnapshot>) {
        match self {
            Self::Idle { shared } | Self::Primed { shared } => (shared, None),
            Self::ObservingRequest {
                shared,
                retained_observation,
                ..
            } => (shared, retained_observation),
            Self::ObservingActive {
                shared,
                observation,
                ..
            } => (shared, Some(observation)),
            Self::Recovering {
                shared,
                observation,
            } => (shared, observation),
            Self::Ready {
                shared,
                observation,
            }
            | Self::Planning {
                shared,
                observation,
                ..
            }
            | Self::Applying {
                shared,
                observation,
                ..
            } => (shared, Some(observation)),
        }
    }

    pub(crate) const fn lifecycle(&self) -> Lifecycle {
        match self {
            Self::Idle { .. } => Lifecycle::Idle,
            Self::Primed { .. } => Lifecycle::Primed,
            Self::ObservingRequest { .. } | Self::ObservingActive { .. } => Lifecycle::Observing,
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
            Self::ObservingRequest { .. } | Self::Recovering { .. } => None,
            Self::ObservingActive { observation, .. }
            | Self::Ready { observation, .. }
            | Self::Planning { observation, .. }
            | Self::Applying { observation, .. } => Some(observation),
        }
    }

    pub(crate) fn retained_observation(&self) -> Option<&ObservationSnapshot> {
        match self {
            Self::Idle { .. } | Self::Primed { .. } => None,
            Self::ObservingRequest {
                retained_observation,
                ..
            } => retained_observation.as_ref(),
            Self::ObservingActive { observation, .. }
            | Self::Ready { observation, .. }
            | Self::Planning { observation, .. }
            | Self::Applying { observation, .. } => Some(observation),
            Self::Recovering { observation, .. } => observation.as_ref(),
        }
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
    fn map_protocol<F>(mut self, map: F) -> Self
    where
        F: FnOnce(ProtocolState) -> ProtocolState,
    {
        let previous_lifecycle = self.protocol.lifecycle();
        let protocol = map(std::mem::take(&mut self.protocol));
        if previous_lifecycle != protocol.lifecycle() {
            self.generation = self.next_generation();
        }
        self.protocol = protocol;
        self
    }

    fn try_map_protocol<F>(mut self, map: F) -> Option<Self>
    where
        F: FnOnce(ProtocolState) -> Option<ProtocolState>,
    {
        let previous_lifecycle = self.protocol.lifecycle();
        let protocol = map(std::mem::take(&mut self.protocol))?;
        if previous_lifecycle != protocol.lifecycle() {
            self.generation = self.next_generation();
        }
        self.protocol = protocol;
        Some(self)
    }

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
            ProtocolState::ObservingRequest { request, .. }
            | ProtocolState::ObservingActive { request, .. } => Some(request.demand()),
            _ => None,
        }
    }

    pub(crate) fn active_observation_request(&self) -> Option<&ObservationRequest> {
        match &self.protocol {
            ProtocolState::ObservingRequest { request, .. }
            | ProtocolState::ObservingActive { request, .. } => Some(request),
            _ => None,
        }
    }

    pub(crate) fn observation(&self) -> Option<&ObservationSnapshot> {
        self.protocol.observation()
    }

    pub(crate) fn retained_observation(&self) -> Option<&ObservationSnapshot> {
        self.protocol.retained_observation()
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

    pub(crate) fn runtime_mut(&mut self) -> &mut crate::state::RuntimeState {
        self.payload.scene.motion_mut()
    }

    pub(crate) fn take_runtime(&mut self) -> crate::state::RuntimeState {
        self.payload.scene.take_motion()
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

    pub(crate) fn with_timers(mut self, timers: TimerState) -> Self {
        self.protocol.shared_mut().timers = timers;
        self
    }

    pub(crate) fn with_recovery_policy(mut self, recovery_policy: RecoveryPolicyState) -> Self {
        self.protocol.shared_mut().recovery_policy = recovery_policy;
        self
    }

    pub(crate) fn with_ingress_policy(mut self, ingress_policy: IngressPolicyState) -> Self {
        self.protocol.shared_mut().ingress_policy = ingress_policy;
        self
    }

    pub(crate) fn with_render_cleanup(mut self, render_cleanup: RenderCleanupState) -> Self {
        self.protocol.shared_mut().render_cleanup = render_cleanup;
        self
    }

    pub(crate) fn with_entropy(mut self, entropy: EntropyState) -> Self {
        self.payload.entropy = entropy;
        self
    }

    pub(crate) fn with_last_cursor(mut self, last_cursor: Option<CursorPosition>) -> Self {
        self.payload.last_cursor = last_cursor;
        self
    }

    #[cfg(test)]
    pub(crate) fn with_scene(mut self, scene: SceneState) -> Self {
        self.payload.scene = scene;
        self
    }

    pub(crate) fn with_realization(mut self, realization: RealizationLedger) -> Self {
        self.payload.realization = realization;
        self
    }

    pub(crate) fn with_runtime(mut self, runtime: crate::state::RuntimeState) -> Self {
        let scene = std::mem::take(&mut self.payload.scene);
        self.payload.scene = scene.with_motion(runtime);
        self
    }

    #[cfg(test)]
    pub(crate) fn with_demand_queue(mut self, demand: DemandQueue) -> Self {
        self.protocol.shared_mut().demand = demand;
        self
    }

    pub(crate) fn map_demand_queue<R>(
        mut self,
        map: impl FnOnce(DemandQueue) -> (DemandQueue, R),
    ) -> (Self, R) {
        // reducers that already own `CoreState` should move the demand queue through the
        // transition edge instead of cloning protocol-shared ingress state.
        let demand = std::mem::take(&mut self.protocol.shared_mut().demand);
        let (demand, result) = map(demand);
        self.protocol.shared_mut().demand = demand;
        (self, result)
    }

    pub(crate) fn initialize(self) -> Self {
        self.map_protocol(|protocol| ProtocolState::Primed {
            shared: protocol.into_shared(),
        })
    }

    pub(crate) fn into_observing(self, request: ObservationRequest) -> Self {
        self.map_protocol(|protocol| {
            let (shared, retained_observation) = protocol.into_shared_and_retained_observation();
            ProtocolState::ObservingRequest {
                shared,
                request,
                retained_observation,
                probe_refresh: ProbeRefreshState::default(),
            }
        })
    }

    #[cfg(test)]
    pub(crate) fn with_active_observation(
        self,
        observation: Option<ObservationSnapshot>,
    ) -> Option<Self> {
        let mut state = self;
        state.set_active_observation(observation).then_some(state)
    }

    pub(crate) const fn probe_refresh_state(&self) -> Option<ProbeRefreshState> {
        match self.protocol() {
            ProtocolState::ObservingRequest { probe_refresh, .. }
            | ProtocolState::ObservingActive { probe_refresh, .. } => Some(*probe_refresh),
            _ => None,
        }
    }

    pub(crate) fn set_active_observation(
        &mut self,
        observation: Option<ObservationSnapshot>,
    ) -> bool {
        let protocol = std::mem::take(&mut self.protocol);
        match protocol {
            ProtocolState::ObservingRequest {
                shared,
                request,
                probe_refresh,
                ..
            } => {
                self.protocol = match observation {
                    Some(observation) => ProtocolState::ObservingActive {
                        shared,
                        request,
                        observation,
                        probe_refresh,
                    },
                    None => ProtocolState::ObservingRequest {
                        shared,
                        request,
                        retained_observation: None,
                        probe_refresh,
                    },
                };
                true
            }
            ProtocolState::ObservingActive {
                shared,
                request,
                probe_refresh,
                ..
            } => {
                self.protocol = match observation {
                    Some(observation) => ProtocolState::ObservingActive {
                        shared,
                        request,
                        observation,
                        probe_refresh,
                    },
                    None => ProtocolState::ObservingRequest {
                        shared,
                        request,
                        retained_observation: None,
                        probe_refresh,
                    },
                };
                true
            }
            protocol => {
                self.protocol = protocol;
                false
            }
        }
    }

    pub(crate) fn set_probe_refresh_state(&mut self, probe_refresh: ProbeRefreshState) -> bool {
        match &mut self.protocol {
            ProtocolState::ObservingRequest {
                probe_refresh: current,
                ..
            }
            | ProtocolState::ObservingActive {
                probe_refresh: current,
                ..
            } => {
                *current = probe_refresh;
                true
            }
            _ => false,
        }
    }

    pub(crate) fn into_ready_with_observation(self, observation: ObservationSnapshot) -> Self {
        self.map_protocol(|protocol| ProtocolState::Ready {
            shared: protocol.into_shared(),
            observation,
        })
    }

    pub(crate) fn into_planning(self, proposal_id: ProposalId) -> Option<Self> {
        self.try_map_protocol(|protocol| {
            let ProtocolState::Ready {
                shared,
                observation,
            } = protocol
            else {
                return None;
            };
            Some(ProtocolState::Planning {
                shared,
                observation,
                proposal_id,
            })
        })
    }

    pub(crate) fn into_primed(self) -> Self {
        self.map_protocol(|protocol| ProtocolState::Primed {
            shared: protocol.into_shared(),
        })
    }

    #[cfg(test)]
    pub(crate) fn into_applying(self, proposal: InFlightProposal) -> Option<Self> {
        self.try_map_protocol(|protocol| match protocol {
            ProtocolState::ObservingActive {
                shared,
                observation,
                ..
            }
            | ProtocolState::Ready {
                shared,
                observation,
            }
            | ProtocolState::Planning {
                shared,
                observation,
                ..
            }
            | ProtocolState::Recovering {
                shared,
                observation: Some(observation),
            } => Some(ProtocolState::Applying {
                shared,
                observation,
                proposal: Box::new(proposal),
            }),
            ProtocolState::Idle { .. }
            | ProtocolState::Primed { .. }
            | ProtocolState::ObservingRequest { .. }
            | ProtocolState::Applying { .. }
            | ProtocolState::Recovering {
                observation: None, ..
            } => None,
        })
    }

    pub(crate) fn into_recovering(self) -> Self {
        self.map_protocol(|protocol| {
            let (shared, observation) = protocol.into_shared_and_retained_observation();
            ProtocolState::Recovering {
                shared,
                observation,
            }
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

    #[cfg(test)]
    pub(crate) fn clear_pending_for(
        self,
        proposal_id: ProposalId,
    ) -> Option<(Self, InFlightProposal)> {
        let mut state = self;
        let pending = state.take_pending_proposal(proposal_id)?;
        Some((state, pending))
    }

    pub(crate) fn take_pending_proposal(
        &mut self,
        proposal_id: ProposalId,
    ) -> Option<InFlightProposal> {
        let protocol = std::mem::take(&mut self.protocol);
        match protocol {
            ProtocolState::Applying {
                shared,
                observation,
                proposal,
            } if proposal.proposal_id() == proposal_id => {
                self.protocol = ProtocolState::Ready {
                    shared,
                    observation,
                };
                Some(*proposal)
            }
            protocol => {
                self.protocol = protocol;
                None
            }
        }
    }

    pub(crate) fn accept_planned_render_mut(
        &mut self,
        planned_render: crate::core::state::PlannedRender,
    ) -> bool {
        let proposal_id = planned_render.proposal_id();
        let (next_scene, proposal) = planned_render.into_parts();
        let protocol = std::mem::take(&mut self.protocol);
        match protocol {
            ProtocolState::Planning {
                shared,
                observation,
                proposal_id: active_proposal_id,
            } if active_proposal_id == proposal_id => {
                self.protocol = ProtocolState::Applying {
                    shared,
                    observation,
                    proposal: Box::new(proposal),
                };
                self.payload.scene = next_scene;
                true
            }
            protocol => {
                self.protocol = protocol;
                false
            }
        }
    }
}
