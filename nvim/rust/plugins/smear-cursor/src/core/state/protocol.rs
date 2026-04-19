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
use crate::core::runtime_reducer::CursorTransition;
use crate::core::types::CursorPosition;
use crate::core::types::Generation;
use crate::core::types::IngressSeq;
use crate::core::types::Lifecycle;
use crate::core::types::ProposalId;
use std::rc::Rc;

#[derive(Debug, Clone, PartialEq)]
enum PreparedObservationRuntime {
    Preview(crate::state::PreparedRuntimeMotion),
    Unchanged,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PreparedObservationPlan {
    runtime: PreparedObservationRuntime,
    transition: CursorTransition,
}

impl PreparedObservationPlan {
    pub(crate) fn new(
        prepared_motion: crate::state::PreparedRuntimeMotion,
        transition: CursorTransition,
    ) -> Self {
        Self {
            runtime: PreparedObservationRuntime::Preview(prepared_motion),
            transition,
        }
    }

    pub(crate) fn unchanged(transition: CursorTransition) -> Self {
        Self {
            runtime: PreparedObservationRuntime::Unchanged,
            transition,
        }
    }

    #[cfg(test)]
    pub(crate) fn apply_to_runtime(&self, runtime: &mut crate::state::RuntimeState) {
        if let PreparedObservationRuntime::Preview(prepared_motion) = &self.runtime {
            runtime.apply_prepared_motion(prepared_motion.clone());
        }
    }

    #[cfg(test)]
    pub(crate) fn prepared_particles_capacity(&self) -> usize {
        match &self.runtime {
            PreparedObservationRuntime::Preview(prepared_motion) => {
                prepared_motion.particles_capacity()
            }
            PreparedObservationRuntime::Unchanged => 0,
        }
    }

    #[cfg(test)]
    pub(crate) fn retains_preview_motion(&self) -> bool {
        matches!(&self.runtime, PreparedObservationRuntime::Preview(_))
    }

    pub(crate) fn transition(&self) -> &CursorTransition {
        &self.transition
    }

    pub(crate) fn reclaim_preview_particles_into(self, runtime: &mut crate::state::RuntimeState) {
        let PreparedObservationRuntime::Preview(prepared_motion) = self.runtime else {
            return;
        };
        runtime.reclaim_preview_particles_scratch(prepared_motion.into_particles());
    }

    pub(crate) fn apply_to_runtime_and_take_transition(
        self,
        runtime: &mut crate::state::RuntimeState,
    ) -> CursorTransition {
        if let PreparedObservationRuntime::Preview(prepared_motion) = self.runtime {
            runtime.apply_prepared_motion(prepared_motion);
        }
        self.transition
    }
}

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

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum ObservationSlotKind {
    Empty,
    Retained,
    Active,
}

#[derive(Debug, Clone, PartialEq, Default)]
enum ObservationSlot {
    #[default]
    Empty,
    Retained(ObservationSnapshot),
    Active(ObservationSnapshot),
}

impl ObservationSlot {
    const fn kind(&self) -> ObservationSlotKind {
        match self {
            Self::Empty => ObservationSlotKind::Empty,
            Self::Retained(_) => ObservationSlotKind::Retained,
            Self::Active(_) => ObservationSlotKind::Active,
        }
    }

    const fn active(&self) -> Option<&ObservationSnapshot> {
        match self {
            Self::Active(observation) => Some(observation),
            Self::Empty | Self::Retained(_) => None,
        }
    }

    fn active_mut(&mut self) -> Option<&mut ObservationSnapshot> {
        match self {
            Self::Active(observation) => Some(observation),
            Self::Empty | Self::Retained(_) => None,
        }
    }

    const fn retained(&self) -> Option<&ObservationSnapshot> {
        match self {
            Self::Retained(observation) | Self::Active(observation) => Some(observation),
            Self::Empty => None,
        }
    }

    fn take_retained(&mut self) -> Option<ObservationSnapshot> {
        match std::mem::replace(self, Self::Empty) {
            Self::Retained(observation) => Some(observation),
            slot @ (Self::Empty | Self::Active(_)) => {
                *self = slot;
                None
            }
        }
    }

    fn set_retained(&mut self, observation: Option<ObservationSnapshot>) {
        *self = observation.map_or(Self::Empty, Self::Retained);
    }

    fn into_retained(self) -> Self {
        match self {
            Self::Active(observation) | Self::Retained(observation) => Self::Retained(observation),
            Self::Empty => Self::Empty,
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum ProtocolWorkflowKind {
    Idle,
    Primed,
    ObservingRequest,
    ObservingActive,
    Ready,
    Planning,
    Applying,
    Recovering,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ProtocolWorkflow {
    Idle,
    // initialization now owns only the protocol bootstrap edge; all planning reads
    // enter through the observation shell after the first external demand arrives.
    Primed,
    ObservingRequest {
        request: ObservationRequest,
        probe_refresh: ProbeRefreshState,
    },
    ObservingActive {
        request: ObservationRequest,
        probe_refresh: ProbeRefreshState,
        prepared_plan: Option<Box<PreparedObservationPlan>>,
    },
    Ready,
    Planning {
        proposal_id: ProposalId,
    },
    Applying {
        proposal: Box<InFlightProposal>,
    },
    Recovering,
}

impl ProtocolWorkflow {
    pub(crate) const fn kind(&self) -> ProtocolWorkflowKind {
        match self {
            Self::Idle => ProtocolWorkflowKind::Idle,
            Self::Primed => ProtocolWorkflowKind::Primed,
            Self::ObservingRequest { .. } => ProtocolWorkflowKind::ObservingRequest,
            Self::ObservingActive { .. } => ProtocolWorkflowKind::ObservingActive,
            Self::Ready => ProtocolWorkflowKind::Ready,
            Self::Planning { .. } => ProtocolWorkflowKind::Planning,
            Self::Applying { .. } => ProtocolWorkflowKind::Applying,
            Self::Recovering => ProtocolWorkflowKind::Recovering,
        }
    }

    pub(crate) const fn lifecycle(&self) -> Lifecycle {
        match self.kind() {
            ProtocolWorkflowKind::Idle => Lifecycle::Idle,
            ProtocolWorkflowKind::Primed => Lifecycle::Primed,
            ProtocolWorkflowKind::ObservingRequest | ProtocolWorkflowKind::ObservingActive => {
                Lifecycle::Observing
            }
            ProtocolWorkflowKind::Ready => Lifecycle::Ready,
            ProtocolWorkflowKind::Planning => Lifecycle::Planning,
            ProtocolWorkflowKind::Applying => Lifecycle::Applying,
            ProtocolWorkflowKind::Recovering => Lifecycle::Recovering,
        }
    }
}

const fn workflow_allows_observation_slot(
    workflow: ProtocolWorkflowKind,
    observation: ObservationSlotKind,
) -> bool {
    match workflow {
        ProtocolWorkflowKind::Idle | ProtocolWorkflowKind::Primed => {
            matches!(observation, ObservationSlotKind::Empty)
        }
        ProtocolWorkflowKind::ObservingRequest | ProtocolWorkflowKind::Recovering => {
            matches!(
                observation,
                ObservationSlotKind::Empty | ObservationSlotKind::Retained
            )
        }
        ProtocolWorkflowKind::ObservingActive
        | ProtocolWorkflowKind::Ready
        | ProtocolWorkflowKind::Planning
        | ProtocolWorkflowKind::Applying => matches!(observation, ObservationSlotKind::Active),
    }
}

#[cfg(test)]
pub(crate) const fn workflow_allows_observation_slot_for_tests(
    workflow: ProtocolWorkflowKind,
    observation: ObservationSlotKind,
) -> bool {
    workflow_allows_observation_slot(workflow, observation)
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ProtocolState {
    shared: ProtocolSharedState,
    observation: ObservationSlot,
    workflow: ProtocolWorkflow,
}

impl Default for ProtocolState {
    fn default() -> Self {
        Self::idle()
    }
}

impl ProtocolState {
    // Keep every protocol workflow transition behind named assembly helpers so the
    // workflow/slot matrix is expressed once instead of via repeated field surgery.
    fn assemble(
        shared: ProtocolSharedState,
        observation: ObservationSlot,
        workflow: ProtocolWorkflow,
    ) -> Self {
        let state = Self {
            shared,
            observation,
            workflow,
        };
        state.debug_assert_invariants();
        state
    }

    fn idle() -> Self {
        Self::assemble(
            ProtocolSharedState::default(),
            ObservationSlot::Empty,
            ProtocolWorkflow::Idle,
        )
    }

    fn primed(shared: ProtocolSharedState) -> Self {
        Self::assemble(shared, ObservationSlot::Empty, ProtocolWorkflow::Primed)
    }

    fn observing_request(
        shared: ProtocolSharedState,
        observation: ObservationSlot,
        request: ObservationRequest,
        probe_refresh: ProbeRefreshState,
    ) -> Self {
        Self::assemble(
            shared,
            observation,
            ProtocolWorkflow::ObservingRequest {
                request,
                probe_refresh,
            },
        )
    }

    fn observing_active(
        shared: ProtocolSharedState,
        observation: ObservationSnapshot,
        request: ObservationRequest,
        probe_refresh: ProbeRefreshState,
        prepared_plan: Option<Box<PreparedObservationPlan>>,
    ) -> Self {
        Self::assemble(
            shared,
            ObservationSlot::Active(observation),
            ProtocolWorkflow::ObservingActive {
                request,
                probe_refresh,
                prepared_plan,
            },
        )
    }

    fn ready(shared: ProtocolSharedState, observation: ObservationSnapshot) -> Self {
        Self::assemble(
            shared,
            ObservationSlot::Active(observation),
            ProtocolWorkflow::Ready,
        )
    }

    fn planning(
        shared: ProtocolSharedState,
        observation: ObservationSnapshot,
        proposal_id: ProposalId,
    ) -> Self {
        Self::assemble(
            shared,
            ObservationSlot::Active(observation),
            ProtocolWorkflow::Planning { proposal_id },
        )
    }

    fn applying(
        shared: ProtocolSharedState,
        observation: ObservationSnapshot,
        proposal: Box<InFlightProposal>,
    ) -> Self {
        Self::assemble(
            shared,
            ObservationSlot::Active(observation),
            ProtocolWorkflow::Applying { proposal },
        )
    }

    fn recovering(shared: ProtocolSharedState, observation: ObservationSlot) -> Self {
        Self::assemble(shared, observation, ProtocolWorkflow::Recovering)
    }

    const fn shared(&self) -> &ProtocolSharedState {
        &self.shared
    }

    fn shared_mut(&mut self) -> &mut ProtocolSharedState {
        &mut self.shared
    }

    fn into_parts(self) -> (ProtocolSharedState, ObservationSlot, ProtocolWorkflow) {
        (self.shared, self.observation, self.workflow)
    }

    pub(crate) const fn workflow_kind(&self) -> ProtocolWorkflowKind {
        self.workflow.kind()
    }

    #[cfg(test)]
    pub(crate) const fn observation_slot_kind(&self) -> ObservationSlotKind {
        self.observation.kind()
    }

    pub(crate) const fn lifecycle(&self) -> Lifecycle {
        self.workflow.lifecycle()
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
        self.observation.active()
    }

    pub(crate) fn observation_mut(&mut self) -> Option<&mut ObservationSnapshot> {
        self.observation.active_mut()
    }

    pub(crate) fn retained_observation(&self) -> Option<&ObservationSnapshot> {
        self.observation.retained()
    }

    #[cfg(debug_assertions)]
    fn debug_assert_invariants(&self) {
        let workflow_kind = self.workflow.kind();
        let observation_kind = self.observation.kind();
        debug_assert!(
            workflow_allows_observation_slot(workflow_kind, observation_kind),
            "workflow {workflow_kind:?} does not allow observation slot {observation_kind:?}"
        );

        match (workflow_kind, observation_kind) {
            (ProtocolWorkflowKind::Idle, ObservationSlotKind::Empty)
            | (ProtocolWorkflowKind::Primed, ObservationSlotKind::Empty)
            | (ProtocolWorkflowKind::ObservingRequest, ObservationSlotKind::Empty)
            | (ProtocolWorkflowKind::Recovering, ObservationSlotKind::Empty) => {}
            (ProtocolWorkflowKind::ObservingRequest, ObservationSlotKind::Retained)
            | (ProtocolWorkflowKind::Recovering, ObservationSlotKind::Retained) => {
                let Some(observation) = self.retained_observation() else {
                    unreachable!("retained slot should contain an observation");
                };
                observation.debug_assert_invariants();
            }
            (ProtocolWorkflowKind::ObservingActive, ObservationSlotKind::Active) => {
                let Some(observation) = self.observation() else {
                    unreachable!("active slot should expose an observation");
                };
                observation.debug_assert_invariants();
                let ProtocolWorkflow::ObservingActive { request, .. } = &self.workflow else {
                    unreachable!("observing-active kind must come from observing-active workflow");
                };
                debug_assert_eq!(
                    observation.request().observation_id(),
                    request.observation_id(),
                    "active observation must stay paired with its observation request"
                );
            }
            (ProtocolWorkflowKind::Ready, ObservationSlotKind::Active)
            | (ProtocolWorkflowKind::Planning, ObservationSlotKind::Active)
            | (ProtocolWorkflowKind::Applying, ObservationSlotKind::Active) => {
                let Some(observation) = self.observation() else {
                    unreachable!("active slot should expose an observation");
                };
                observation.debug_assert_invariants();
            }
            _ => unreachable!("invalid workflow/slot combination should be rejected above"),
        }
    }

    #[cfg(not(debug_assertions))]
    fn debug_assert_invariants(&self) {}
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct CoreStatePayload {
    pub(crate) entropy: EntropyState,
    pub(crate) latest_exact_cursor_position: Option<CursorPosition>,
    pub(crate) scene: Rc<SceneState>,
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
    fn recycle_staged_prepared_observation_plan(&mut self) {
        let Some(prepared_plan) = self.take_prepared_observation_plan() else {
            return;
        };
        prepared_plan.reclaim_preview_particles_into(self.runtime_mut());
    }

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

    pub(crate) fn next_generation(&self) -> Generation {
        self.generation.next()
    }

    #[cfg(debug_assertions)]
    pub(crate) fn debug_assert_invariants(&self) {
        self.runtime().debug_assert_invariants();
        self.protocol.debug_assert_invariants();
    }

    #[cfg(not(debug_assertions))]
    pub(crate) fn debug_assert_invariants(&self) {}

    pub(crate) const fn lifecycle(&self) -> Lifecycle {
        self.protocol.lifecycle()
    }

    pub(crate) const fn protocol(&self) -> &ProtocolState {
        &self.protocol
    }

    pub(crate) const fn needs_initialize(&self) -> bool {
        matches!(self.protocol.workflow_kind(), ProtocolWorkflowKind::Idle)
    }

    pub(crate) const fn latest_exact_cursor_position(&self) -> Option<CursorPosition> {
        self.payload.latest_exact_cursor_position
    }

    pub(crate) const fn fallback_cursor_position(
        &self,
        observed_cursor_position: Option<CursorPosition>,
    ) -> Option<CursorPosition> {
        match observed_cursor_position {
            Some(cursor_position) => Some(cursor_position),
            None => self.latest_exact_cursor_position(),
        }
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
        match &self.protocol.workflow {
            ProtocolWorkflow::ObservingRequest { request, .. }
            | ProtocolWorkflow::ObservingActive { request, .. } => Some(request.demand()),
            ProtocolWorkflow::Idle
            | ProtocolWorkflow::Primed
            | ProtocolWorkflow::Ready
            | ProtocolWorkflow::Planning { .. }
            | ProtocolWorkflow::Applying { .. }
            | ProtocolWorkflow::Recovering => None,
        }
    }

    pub(crate) fn active_observation_request(&self) -> Option<&ObservationRequest> {
        match &self.protocol.workflow {
            ProtocolWorkflow::ObservingRequest { request, .. }
            | ProtocolWorkflow::ObservingActive { request, .. } => Some(request),
            ProtocolWorkflow::Idle
            | ProtocolWorkflow::Primed
            | ProtocolWorkflow::Ready
            | ProtocolWorkflow::Planning { .. }
            | ProtocolWorkflow::Applying { .. }
            | ProtocolWorkflow::Recovering => None,
        }
    }

    pub(crate) fn observation(&self) -> Option<&ObservationSnapshot> {
        self.protocol.observation()
    }

    pub(crate) fn observation_mut(&mut self) -> Option<&mut ObservationSnapshot> {
        self.protocol.observation_mut()
    }

    pub(crate) fn retained_observation(&self) -> Option<&ObservationSnapshot> {
        self.protocol.retained_observation()
    }

    pub(crate) fn take_retained_observation(&mut self) -> Option<ObservationSnapshot> {
        match self.protocol.workflow_kind() {
            ProtocolWorkflowKind::ObservingRequest | ProtocolWorkflowKind::Recovering => {
                self.protocol.observation.take_retained()
            }
            ProtocolWorkflowKind::Idle
            | ProtocolWorkflowKind::Primed
            | ProtocolWorkflowKind::ObservingActive
            | ProtocolWorkflowKind::Ready
            | ProtocolWorkflowKind::Planning
            | ProtocolWorkflowKind::Applying => None,
        }
    }

    pub(crate) fn restore_retained_observation(
        &mut self,
        observation: Option<ObservationSnapshot>,
    ) -> bool {
        match self.protocol.workflow_kind() {
            ProtocolWorkflowKind::ObservingRequest | ProtocolWorkflowKind::Recovering => {
                self.protocol.observation.set_retained(observation);
                true
            }
            ProtocolWorkflowKind::Idle
            | ProtocolWorkflowKind::Primed
            | ProtocolWorkflowKind::ObservingActive
            | ProtocolWorkflowKind::Ready
            | ProtocolWorkflowKind::Planning
            | ProtocolWorkflowKind::Applying => false,
        }
    }

    #[cfg(test)]
    pub(crate) fn scene(&self) -> &SceneState {
        self.payload.scene.as_ref()
    }

    pub(crate) fn shared_scene(&self) -> Rc<SceneState> {
        Rc::clone(&self.payload.scene)
    }

    pub(crate) const fn realization(&self) -> &RealizationLedger {
        &self.payload.realization
    }

    pub(crate) fn runtime(&self) -> &crate::state::RuntimeState {
        self.payload.scene.motion()
    }

    pub(crate) fn runtime_mut(&mut self) -> &mut crate::state::RuntimeState {
        Rc::make_mut(&mut self.payload.scene).motion_mut()
    }

    pub(crate) fn runtime_mut_with_observation(
        &mut self,
    ) -> Option<(&mut crate::state::RuntimeState, &ObservationSnapshot)> {
        let Self {
            protocol, payload, ..
        } = self;
        let observation = protocol.observation()?;
        Some((Rc::make_mut(&mut payload.scene).motion_mut(), observation))
    }

    pub(crate) fn take_runtime(&mut self) -> crate::state::RuntimeState {
        Rc::make_mut(&mut self.payload.scene).take_motion()
    }

    pub(crate) fn pending_proposal(&self) -> Option<&InFlightProposal> {
        match &self.protocol.workflow {
            ProtocolWorkflow::Applying { proposal } => Some(proposal.as_ref()),
            ProtocolWorkflow::Idle
            | ProtocolWorkflow::Primed
            | ProtocolWorkflow::ObservingRequest { .. }
            | ProtocolWorkflow::ObservingActive { .. }
            | ProtocolWorkflow::Ready
            | ProtocolWorkflow::Planning { .. }
            | ProtocolWorkflow::Recovering => None,
        }
    }

    pub(crate) const fn pending_plan_proposal_id(&self) -> Option<ProposalId> {
        match &self.protocol.workflow {
            ProtocolWorkflow::Planning { proposal_id } => Some(*proposal_id),
            ProtocolWorkflow::Idle
            | ProtocolWorkflow::Primed
            | ProtocolWorkflow::ObservingRequest { .. }
            | ProtocolWorkflow::ObservingActive { .. }
            | ProtocolWorkflow::Ready
            | ProtocolWorkflow::Applying { .. }
            | ProtocolWorkflow::Recovering => None,
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

    pub(crate) fn with_latest_exact_cursor_position(
        mut self,
        latest_exact_cursor_position: Option<CursorPosition>,
    ) -> Self {
        self.payload.latest_exact_cursor_position = latest_exact_cursor_position;
        self
    }

    pub(crate) fn with_realization(mut self, realization: RealizationLedger) -> Self {
        self.payload.realization = realization;
        self
    }

    pub(crate) fn with_runtime(mut self, runtime: crate::state::RuntimeState) -> Self {
        *Rc::make_mut(&mut self.payload.scene).motion_mut() = runtime;
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

    pub(crate) fn enter_observing_request(mut self, request: ObservationRequest) -> Self {
        self.recycle_staged_prepared_observation_plan();
        self.map_protocol(|protocol| {
            let (shared, observation, _) = protocol.into_parts();
            ProtocolState::observing_request(
                shared,
                observation.into_retained(),
                request,
                ProbeRefreshState::default(),
            )
        })
    }

    #[cfg(test)]
    pub(crate) fn with_active_observation(
        self,
        observation: Option<ObservationSnapshot>,
    ) -> Option<Self> {
        let mut state = self;
        match observation {
            Some(observation) => state.activate_observation(observation).then_some(state),
            None => state.clear_active_observation().then_some(state),
        }
    }

    pub(crate) const fn probe_refresh_state(&self) -> Option<ProbeRefreshState> {
        match &self.protocol.workflow {
            ProtocolWorkflow::ObservingRequest { probe_refresh, .. }
            | ProtocolWorkflow::ObservingActive { probe_refresh, .. } => Some(*probe_refresh),
            ProtocolWorkflow::Idle
            | ProtocolWorkflow::Primed
            | ProtocolWorkflow::Ready
            | ProtocolWorkflow::Planning { .. }
            | ProtocolWorkflow::Applying { .. }
            | ProtocolWorkflow::Recovering => None,
        }
    }

    pub(crate) fn activate_observation(&mut self, observation: ObservationSnapshot) -> bool {
        let protocol = std::mem::take(&mut self.protocol);
        match protocol {
            ProtocolState {
                shared,
                workflow:
                    ProtocolWorkflow::ObservingRequest {
                        request,
                        probe_refresh,
                    },
                ..
            } => {
                self.protocol = ProtocolState::observing_active(
                    shared,
                    observation,
                    request,
                    probe_refresh,
                    None,
                );
                true
            }
            ProtocolState {
                shared,
                workflow:
                    ProtocolWorkflow::ObservingActive {
                        request,
                        probe_refresh,
                        prepared_plan,
                    },
                ..
            } => {
                self.protocol = ProtocolState::observing_active(
                    shared,
                    observation,
                    request,
                    probe_refresh,
                    prepared_plan,
                );
                true
            }
            protocol => {
                self.protocol = protocol;
                false
            }
        }
    }

    pub(crate) fn clear_active_observation(&mut self) -> bool {
        self.recycle_staged_prepared_observation_plan();
        let protocol = std::mem::take(&mut self.protocol);
        match protocol {
            ProtocolState {
                shared,
                observation: ObservationSlot::Active(_),
                workflow:
                    ProtocolWorkflow::ObservingActive {
                        request,
                        probe_refresh,
                        ..
                    },
            } => {
                self.protocol = ProtocolState::observing_request(
                    shared,
                    ObservationSlot::Empty,
                    request,
                    probe_refresh,
                );
                true
            }
            protocol => {
                self.protocol = protocol;
                false
            }
        }
    }

    pub(crate) fn take_active_observation_for_completion(&mut self) -> Option<ObservationSnapshot> {
        self.recycle_staged_prepared_observation_plan();
        let protocol = std::mem::take(&mut self.protocol);
        match protocol {
            ProtocolState {
                shared,
                observation: ObservationSlot::Active(observation),
                workflow:
                    ProtocolWorkflow::ObservingActive {
                        request,
                        probe_refresh,
                        ..
                    },
            } => {
                self.protocol = ProtocolState::observing_request(
                    shared,
                    ObservationSlot::Empty,
                    request,
                    probe_refresh,
                );
                Some(observation)
            }
            protocol => {
                self.protocol = protocol;
                None
            }
        }
    }

    pub(crate) fn set_probe_refresh_state(&mut self, probe_refresh: ProbeRefreshState) -> bool {
        match &mut self.protocol.workflow {
            ProtocolWorkflow::ObservingRequest {
                probe_refresh: current,
                ..
            }
            | ProtocolWorkflow::ObservingActive {
                probe_refresh: current,
                ..
            } => {
                *current = probe_refresh;
                true
            }
            ProtocolWorkflow::Idle
            | ProtocolWorkflow::Primed
            | ProtocolWorkflow::Ready
            | ProtocolWorkflow::Planning { .. }
            | ProtocolWorkflow::Applying { .. }
            | ProtocolWorkflow::Recovering => false,
        }
    }

    #[cfg(test)]
    pub(crate) fn prepared_observation_plan(&self) -> Option<&PreparedObservationPlan> {
        match &self.protocol.workflow {
            ProtocolWorkflow::ObservingActive { prepared_plan, .. } => prepared_plan.as_deref(),
            ProtocolWorkflow::Idle
            | ProtocolWorkflow::Primed
            | ProtocolWorkflow::ObservingRequest { .. }
            | ProtocolWorkflow::Ready
            | ProtocolWorkflow::Planning { .. }
            | ProtocolWorkflow::Applying { .. }
            | ProtocolWorkflow::Recovering => None,
        }
    }

    pub(crate) fn set_prepared_observation_plan(
        &mut self,
        prepared_plan: Option<PreparedObservationPlan>,
    ) -> bool {
        self.recycle_staged_prepared_observation_plan();
        match &mut self.protocol.workflow {
            ProtocolWorkflow::ObservingActive {
                prepared_plan: current,
                ..
            } => {
                *current = prepared_plan.map(Box::new);
                true
            }
            ProtocolWorkflow::Idle
            | ProtocolWorkflow::Primed
            | ProtocolWorkflow::ObservingRequest { .. }
            | ProtocolWorkflow::Ready
            | ProtocolWorkflow::Planning { .. }
            | ProtocolWorkflow::Applying { .. }
            | ProtocolWorkflow::Recovering => false,
        }
    }

    pub(crate) fn take_prepared_observation_plan(&mut self) -> Option<PreparedObservationPlan> {
        match &mut self.protocol.workflow {
            ProtocolWorkflow::ObservingActive { prepared_plan, .. } => {
                prepared_plan.take().map(|plan| *plan)
            }
            ProtocolWorkflow::Idle
            | ProtocolWorkflow::Primed
            | ProtocolWorkflow::ObservingRequest { .. }
            | ProtocolWorkflow::Ready
            | ProtocolWorkflow::Planning { .. }
            | ProtocolWorkflow::Applying { .. }
            | ProtocolWorkflow::Recovering => None,
        }
    }

    pub(crate) fn enter_ready(mut self, observation: ObservationSnapshot) -> Self {
        self.recycle_staged_prepared_observation_plan();
        self.map_protocol(|protocol| {
            let (shared, _, _) = protocol.into_parts();
            ProtocolState::ready(shared, observation)
        })
    }

    pub(crate) fn enter_planning(self, proposal_id: ProposalId) -> Option<Self> {
        self.try_map_protocol(|protocol| {
            let (shared, observation, workflow) = protocol.into_parts();
            match (workflow, observation) {
                (ProtocolWorkflow::Ready, ObservationSlot::Active(observation)) => {
                    Some(ProtocolState::planning(shared, observation, proposal_id))
                }
                (ProtocolWorkflow::Idle, _)
                | (ProtocolWorkflow::Primed, _)
                | (ProtocolWorkflow::ObservingRequest { .. }, _)
                | (ProtocolWorkflow::ObservingActive { .. }, _)
                | (
                    ProtocolWorkflow::Ready,
                    ObservationSlot::Empty | ObservationSlot::Retained(_),
                )
                | (ProtocolWorkflow::Planning { .. }, _)
                | (ProtocolWorkflow::Applying { .. }, _)
                | (ProtocolWorkflow::Recovering, _) => None,
            }
        })
    }

    pub(crate) fn into_primed(mut self) -> Self {
        self.recycle_staged_prepared_observation_plan();
        self.map_protocol(|protocol| {
            let (shared, _, _) = protocol.into_parts();
            ProtocolState::primed(shared)
        })
    }

    #[cfg(test)]
    pub(crate) fn enter_applying(mut self, proposal: InFlightProposal) -> Option<Self> {
        self.recycle_staged_prepared_observation_plan();
        let proposal = Box::new(proposal);
        self.try_map_protocol(|protocol| {
            let ProtocolState {
                shared,
                observation,
                workflow,
            } = protocol;
            let workflow_kind = workflow.kind();
            let observation = match (workflow_kind, observation) {
                (
                    ProtocolWorkflowKind::ObservingActive
                    | ProtocolWorkflowKind::Ready
                    | ProtocolWorkflowKind::Planning,
                    ObservationSlot::Active(observation),
                )
                | (ProtocolWorkflowKind::Recovering, ObservationSlot::Retained(observation)) => {
                    observation
                }
                (ProtocolWorkflowKind::Idle, _)
                | (ProtocolWorkflowKind::Primed, _)
                | (ProtocolWorkflowKind::ObservingRequest, _)
                | (
                    ProtocolWorkflowKind::ObservingActive
                    | ProtocolWorkflowKind::Ready
                    | ProtocolWorkflowKind::Planning,
                    ObservationSlot::Empty | ObservationSlot::Retained(_),
                )
                | (ProtocolWorkflowKind::Applying, _)
                | (
                    ProtocolWorkflowKind::Recovering,
                    ObservationSlot::Empty | ObservationSlot::Active(_),
                ) => return None,
            };
            Some(ProtocolState::applying(shared, observation, proposal))
        })
    }

    pub(crate) fn enter_recovering(mut self) -> Self {
        self.recycle_staged_prepared_observation_plan();
        self.map_protocol(|protocol| {
            let (shared, observation, _) = protocol.into_parts();
            ProtocolState::recovering(shared, observation.into_retained())
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
            ProtocolState {
                shared,
                observation: ObservationSlot::Active(observation),
                workflow: ProtocolWorkflow::Applying { proposal },
            } if proposal.proposal_id() == proposal_id => {
                self.protocol = ProtocolState::ready(shared, observation);
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
        let (scene_update, proposal) = planned_render.into_parts();
        let protocol = std::mem::take(&mut self.protocol);
        match protocol {
            ProtocolState {
                shared,
                observation: ObservationSlot::Active(observation),
                workflow:
                    ProtocolWorkflow::Planning {
                        proposal_id: active_proposal_id,
                    },
            } if active_proposal_id == proposal_id => {
                self.protocol = ProtocolState::applying(shared, observation, Box::new(proposal));
                Rc::make_mut(&mut self.payload.scene).apply_planned_update(scene_update);
                true
            }
            protocol => {
                self.protocol = protocol;
                false
            }
        }
    }
}
