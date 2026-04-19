use super::DemandQueue;
use super::EntropyState;
use super::ExternalDemand;
use super::InFlightProposal;
use super::IngressPolicyState;
use super::ObservationSnapshot;
use super::PendingObservation;
#[cfg(test)]
use super::ProbeKind;
use super::ProbeRefreshState;
use super::ProjectionState;
use super::RealizationLedger;
use super::RecoveryPolicyState;
use super::RenderCleanupState;
use super::SceneState;
use super::SemanticState;
use super::TimerState;
use crate::core::runtime_reducer::CursorTransition;
use crate::core::types::Generation;
use crate::core::types::IngressSeq;
use crate::core::types::Lifecycle;
use crate::core::types::ProposalId;
use crate::position::ScreenCell;
use std::rc::Rc;

#[derive(Debug, Clone, PartialEq)]
enum PreparedObservationRuntime {
    Preview(Box<crate::state::PreparedRuntimeMotion>),
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
            runtime: PreparedObservationRuntime::Preview(Box::new(prepared_motion)),
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
            runtime.apply_prepared_motion((**prepared_motion).clone());
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
        runtime.reclaim_preview_particles_scratch((*prepared_motion).into_particles());
    }

    pub(crate) fn apply_to_runtime_and_take_transition(
        self,
        runtime: &mut crate::state::RuntimeState,
    ) -> CursorTransition {
        if let PreparedObservationRuntime::Preview(prepared_motion) = self.runtime {
            runtime.apply_prepared_motion(*prepared_motion);
        }
        self.transition
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct ProtocolSharedState {
    // authoritative: protocol facts that survive phase transitions.
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
pub(crate) enum ProtocolPhaseKind {
    Idle,
    Primed,
    Collecting,
    Observing,
    Ready,
    Planning,
    Applying,
    Recovering,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ProtocolPhase {
    Idle,
    // initialization now owns only the protocol bootstrap edge; all planning reads
    // enter through the observation shell after the first external demand arrives.
    Primed,
    Collecting {
        // snapshot: last complete observation retained for reuse while collecting.
        retained: Option<Box<ObservationSnapshot>>,
        // authoritative: current under-construction observation inputs.
        pending: PendingObservation,
        // authoritative: observation-path retry state.
        probe_refresh: ProbeRefreshState,
    },
    Observing {
        // authoritative: active observation payload.
        active: Box<ObservationSnapshot>,
        // authoritative: observation-path retry state.
        probe_refresh: ProbeRefreshState,
        // cache: preview runtime transition derived from the active observation.
        prepared_plan: Option<Box<PreparedObservationPlan>>,
    },
    Ready {
        // authoritative: active observation payload.
        active: Box<ObservationSnapshot>,
    },
    Planning {
        // authoritative: active observation payload.
        active: Box<ObservationSnapshot>,
        // authoritative: in-flight proposal identity.
        proposal_id: ProposalId,
    },
    Applying {
        // authoritative: active observation payload.
        active: Box<ObservationSnapshot>,
        // authoritative: proposal being applied to the shell.
        proposal: Box<InFlightProposal>,
    },
    Recovering {
        // snapshot: last retained observation while recovery owns the phase.
        retained: Option<Box<ObservationSnapshot>>,
    },
}

impl ProtocolPhase {
    pub(crate) const fn kind(&self) -> ProtocolPhaseKind {
        match self {
            Self::Idle => ProtocolPhaseKind::Idle,
            Self::Primed => ProtocolPhaseKind::Primed,
            Self::Collecting { .. } => ProtocolPhaseKind::Collecting,
            Self::Observing { .. } => ProtocolPhaseKind::Observing,
            Self::Ready { .. } => ProtocolPhaseKind::Ready,
            Self::Planning { .. } => ProtocolPhaseKind::Planning,
            Self::Applying { .. } => ProtocolPhaseKind::Applying,
            Self::Recovering { .. } => ProtocolPhaseKind::Recovering,
        }
    }

    pub(crate) const fn lifecycle(&self) -> Lifecycle {
        match self.kind() {
            ProtocolPhaseKind::Idle => Lifecycle::Idle,
            ProtocolPhaseKind::Primed => Lifecycle::Primed,
            ProtocolPhaseKind::Collecting | ProtocolPhaseKind::Observing => Lifecycle::Observing,
            ProtocolPhaseKind::Ready => Lifecycle::Ready,
            ProtocolPhaseKind::Planning => Lifecycle::Planning,
            ProtocolPhaseKind::Applying => Lifecycle::Applying,
            ProtocolPhaseKind::Recovering => Lifecycle::Recovering,
        }
    }

    fn observation(&self) -> Option<&ObservationSnapshot> {
        match self {
            Self::Observing { active, .. }
            | Self::Ready { active }
            | Self::Planning { active, .. }
            | Self::Applying { active, .. } => Some(active.as_ref()),
            Self::Idle | Self::Primed | Self::Collecting { .. } | Self::Recovering { .. } => None,
        }
    }

    fn observation_mut(&mut self) -> Option<&mut ObservationSnapshot> {
        match self {
            Self::Observing { active, .. }
            | Self::Ready { active }
            | Self::Planning { active, .. }
            | Self::Applying { active, .. } => Some(active.as_mut()),
            Self::Idle | Self::Primed | Self::Collecting { .. } | Self::Recovering { .. } => None,
        }
    }

    fn phase_observation(&self) -> Option<&ObservationSnapshot> {
        match self {
            Self::Collecting { retained, .. } | Self::Recovering { retained } => {
                retained.as_deref()
            }
            Self::Observing { active, .. }
            | Self::Ready { active }
            | Self::Planning { active, .. }
            | Self::Applying { active, .. } => Some(active.as_ref()),
            Self::Idle | Self::Primed => None,
        }
    }

    fn retained_observation(&self) -> Option<&ObservationSnapshot> {
        match self {
            Self::Collecting { retained, .. } | Self::Recovering { retained } => {
                retained.as_deref()
            }
            Self::Idle
            | Self::Primed
            | Self::Observing { .. }
            | Self::Ready { .. }
            | Self::Planning { .. }
            | Self::Applying { .. } => None,
        }
    }

    fn take_retained_observation(&mut self) -> Option<ObservationSnapshot> {
        match self {
            Self::Collecting { retained, .. } | Self::Recovering { retained } => {
                retained.take().map(|observation| *observation)
            }
            Self::Idle
            | Self::Primed
            | Self::Observing { .. }
            | Self::Ready { .. }
            | Self::Planning { .. }
            | Self::Applying { .. } => None,
        }
    }

    fn restore_retained_observation(&mut self, observation: Option<ObservationSnapshot>) -> bool {
        match self {
            Self::Collecting { retained, .. } | Self::Recovering { retained } => {
                *retained = observation.map(Box::new);
                true
            }
            Self::Idle
            | Self::Primed
            | Self::Observing { .. }
            | Self::Ready { .. }
            | Self::Planning { .. }
            | Self::Applying { .. } => false,
        }
    }

    fn into_retained_observation(self) -> Option<Box<ObservationSnapshot>> {
        match self {
            Self::Collecting { retained, .. } | Self::Recovering { retained } => retained,
            Self::Observing { active, .. }
            | Self::Ready { active }
            | Self::Planning { active, .. }
            | Self::Applying { active, .. } => Some(active),
            Self::Idle | Self::Primed => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ProtocolState {
    shared: ProtocolSharedState,
    phase: ProtocolPhase,
}

impl Default for ProtocolState {
    fn default() -> Self {
        Self::idle()
    }
}

impl ProtocolState {
    fn assemble(shared: ProtocolSharedState, phase: ProtocolPhase) -> Self {
        let state = Self { shared, phase };
        state.debug_assert_invariants();
        state
    }

    fn idle() -> Self {
        Self::assemble(ProtocolSharedState::default(), ProtocolPhase::Idle)
    }

    fn primed(shared: ProtocolSharedState) -> Self {
        Self::assemble(shared, ProtocolPhase::Primed)
    }

    fn collecting(
        shared: ProtocolSharedState,
        retained: Option<Box<ObservationSnapshot>>,
        pending: PendingObservation,
        probe_refresh: ProbeRefreshState,
    ) -> Self {
        Self::assemble(
            shared,
            ProtocolPhase::Collecting {
                retained,
                pending,
                probe_refresh,
            },
        )
    }

    fn observing(
        shared: ProtocolSharedState,
        active: Box<ObservationSnapshot>,
        probe_refresh: ProbeRefreshState,
        prepared_plan: Option<Box<PreparedObservationPlan>>,
    ) -> Self {
        Self::assemble(
            shared,
            ProtocolPhase::Observing {
                active,
                probe_refresh,
                prepared_plan,
            },
        )
    }

    fn ready(shared: ProtocolSharedState, active: Box<ObservationSnapshot>) -> Self {
        Self::assemble(shared, ProtocolPhase::Ready { active })
    }

    fn planning(
        shared: ProtocolSharedState,
        active: Box<ObservationSnapshot>,
        proposal_id: ProposalId,
    ) -> Self {
        Self::assemble(
            shared,
            ProtocolPhase::Planning {
                active,
                proposal_id,
            },
        )
    }

    fn applying(
        shared: ProtocolSharedState,
        active: Box<ObservationSnapshot>,
        proposal: Box<InFlightProposal>,
    ) -> Self {
        Self::assemble(shared, ProtocolPhase::Applying { active, proposal })
    }

    fn recovering(shared: ProtocolSharedState, retained: Option<Box<ObservationSnapshot>>) -> Self {
        Self::assemble(shared, ProtocolPhase::Recovering { retained })
    }

    const fn shared(&self) -> &ProtocolSharedState {
        &self.shared
    }

    fn shared_mut(&mut self) -> &mut ProtocolSharedState {
        &mut self.shared
    }

    fn into_parts(self) -> (ProtocolSharedState, ProtocolPhase) {
        (self.shared, self.phase)
    }

    pub(crate) const fn phase_kind(&self) -> ProtocolPhaseKind {
        self.phase.kind()
    }

    pub(crate) const fn lifecycle(&self) -> Lifecycle {
        self.phase.lifecycle()
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
        self.phase.observation()
    }

    pub(crate) fn observation_mut(&mut self) -> Option<&mut ObservationSnapshot> {
        self.phase.observation_mut()
    }

    pub(crate) fn phase_observation(&self) -> Option<&ObservationSnapshot> {
        self.phase.phase_observation()
    }

    pub(crate) fn retained_observation(&self) -> Option<&ObservationSnapshot> {
        self.phase.retained_observation()
    }

    pub(crate) fn pending_observation(&self) -> Option<&PendingObservation> {
        match &self.phase {
            ProtocolPhase::Collecting { pending, .. } => Some(pending),
            ProtocolPhase::Idle
            | ProtocolPhase::Primed
            | ProtocolPhase::Observing { .. }
            | ProtocolPhase::Ready { .. }
            | ProtocolPhase::Planning { .. }
            | ProtocolPhase::Applying { .. }
            | ProtocolPhase::Recovering { .. } => None,
        }
    }

    fn take_retained_observation(&mut self) -> Option<ObservationSnapshot> {
        self.phase.take_retained_observation()
    }

    fn restore_retained_observation(&mut self, observation: Option<ObservationSnapshot>) -> bool {
        self.phase.restore_retained_observation(observation)
    }

    pub(crate) const fn probe_refresh_state(&self) -> Option<ProbeRefreshState> {
        match &self.phase {
            ProtocolPhase::Collecting { probe_refresh, .. }
            | ProtocolPhase::Observing { probe_refresh, .. } => Some(*probe_refresh),
            ProtocolPhase::Idle
            | ProtocolPhase::Primed
            | ProtocolPhase::Ready { .. }
            | ProtocolPhase::Planning { .. }
            | ProtocolPhase::Applying { .. }
            | ProtocolPhase::Recovering { .. } => None,
        }
    }

    fn set_probe_refresh_state(&mut self, probe_refresh: ProbeRefreshState) -> bool {
        match &mut self.phase {
            ProtocolPhase::Collecting {
                probe_refresh: current,
                ..
            }
            | ProtocolPhase::Observing {
                probe_refresh: current,
                ..
            } => {
                *current = probe_refresh;
                true
            }
            ProtocolPhase::Idle
            | ProtocolPhase::Primed
            | ProtocolPhase::Ready { .. }
            | ProtocolPhase::Planning { .. }
            | ProtocolPhase::Applying { .. }
            | ProtocolPhase::Recovering { .. } => false,
        }
    }

    #[cfg(test)]
    fn prepared_observation_plan(&self) -> Option<&PreparedObservationPlan> {
        match &self.phase {
            ProtocolPhase::Observing { prepared_plan, .. } => prepared_plan.as_deref(),
            ProtocolPhase::Idle
            | ProtocolPhase::Primed
            | ProtocolPhase::Collecting { .. }
            | ProtocolPhase::Ready { .. }
            | ProtocolPhase::Planning { .. }
            | ProtocolPhase::Applying { .. }
            | ProtocolPhase::Recovering { .. } => None,
        }
    }

    fn set_prepared_observation_plan(
        &mut self,
        prepared_plan: Option<PreparedObservationPlan>,
    ) -> bool {
        match &mut self.phase {
            ProtocolPhase::Observing {
                prepared_plan: current,
                ..
            } => {
                *current = prepared_plan.map(Box::new);
                true
            }
            ProtocolPhase::Idle
            | ProtocolPhase::Primed
            | ProtocolPhase::Collecting { .. }
            | ProtocolPhase::Ready { .. }
            | ProtocolPhase::Planning { .. }
            | ProtocolPhase::Applying { .. }
            | ProtocolPhase::Recovering { .. } => false,
        }
    }

    fn take_prepared_observation_plan(&mut self) -> Option<PreparedObservationPlan> {
        match &mut self.phase {
            ProtocolPhase::Observing { prepared_plan, .. } => {
                prepared_plan.take().map(|plan| *plan)
            }
            ProtocolPhase::Idle
            | ProtocolPhase::Primed
            | ProtocolPhase::Collecting { .. }
            | ProtocolPhase::Ready { .. }
            | ProtocolPhase::Planning { .. }
            | ProtocolPhase::Applying { .. }
            | ProtocolPhase::Recovering { .. } => None,
        }
    }

    pub(crate) fn pending_proposal(&self) -> Option<&InFlightProposal> {
        match &self.phase {
            ProtocolPhase::Applying { proposal, .. } => Some(proposal.as_ref()),
            ProtocolPhase::Idle
            | ProtocolPhase::Primed
            | ProtocolPhase::Collecting { .. }
            | ProtocolPhase::Observing { .. }
            | ProtocolPhase::Ready { .. }
            | ProtocolPhase::Planning { .. }
            | ProtocolPhase::Recovering { .. } => None,
        }
    }

    pub(crate) const fn pending_plan_proposal_id(&self) -> Option<ProposalId> {
        match &self.phase {
            ProtocolPhase::Planning { proposal_id, .. } => Some(*proposal_id),
            ProtocolPhase::Idle
            | ProtocolPhase::Primed
            | ProtocolPhase::Collecting { .. }
            | ProtocolPhase::Observing { .. }
            | ProtocolPhase::Ready { .. }
            | ProtocolPhase::Applying { .. }
            | ProtocolPhase::Recovering { .. } => None,
        }
    }

    #[cfg(debug_assertions)]
    fn debug_assert_invariants(&self) {
        match &self.phase {
            ProtocolPhase::Collecting { retained, .. } | ProtocolPhase::Recovering { retained } => {
                if let Some(observation) = retained.as_deref() {
                    observation.debug_assert_invariants();
                }
            }
            ProtocolPhase::Observing { active, .. }
            | ProtocolPhase::Ready { active }
            | ProtocolPhase::Planning { active, .. }
            | ProtocolPhase::Applying { active, .. } => active.debug_assert_invariants(),
            ProtocolPhase::Idle | ProtocolPhase::Primed => {}
        }
    }

    #[cfg(not(debug_assertions))]
    fn debug_assert_invariants(&self) {}
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct CoreStatePayload {
    // authoritative: reducer-owned subtrees and sequence allocators.
    pub(crate) entropy: EntropyState,
    pub(crate) latest_exact_cursor_cell: Option<ScreenCell>,
    pub(crate) motion: Rc<crate::state::RuntimeState>,
    pub(crate) semantics: Rc<SemanticState>,
    pub(crate) projection: Rc<ProjectionState>,
    pub(crate) realization: RealizationLedger,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CoreState {
    // `CoreState` is the reducer-owned semantic root. Shell code may cache
    // host-facing data around it, but not parallel reducer truth.
    // authoritative: lifecycle freshness, protocol workflow, and scene payload.
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

    fn commit_protocol(&mut self, protocol: ProtocolState, previous_lifecycle: Lifecycle) {
        if previous_lifecycle != protocol.lifecycle() {
            self.generation = self.next_generation();
        }
        self.protocol = protocol;
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

    pub(crate) const fn generation(&self) -> Generation {
        self.generation
    }

    #[cfg(debug_assertions)]
    pub(crate) fn debug_assert_invariants(&self) {
        self.runtime().debug_assert_invariants();
        self.projection_state().debug_assert_invariants();
        self.realization().debug_assert_invariants();
        self.protocol.debug_assert_invariants();
        if let Some(observation) = self.phase_observation() {
            match observation.basis().cursor().cell() {
                crate::position::ObservedCell::Exact(cell) => {
                    debug_assert_eq!(
                        self.latest_exact_cursor_cell(),
                        Some(cell),
                        "exact observations must refresh the latest exact cursor cell anchor",
                    );
                }
                crate::position::ObservedCell::Deferred(_)
                | crate::position::ObservedCell::Unavailable => {}
            }
        }
    }

    #[cfg(not(debug_assertions))]
    pub(crate) fn debug_assert_invariants(&self) {}

    pub(crate) const fn lifecycle(&self) -> Lifecycle {
        self.protocol.lifecycle()
    }

    pub(crate) const fn phase_kind(&self) -> ProtocolPhaseKind {
        self.protocol.phase_kind()
    }

    #[cfg(test)]
    pub(crate) const fn protocol(&self) -> &ProtocolState {
        &self.protocol
    }

    pub(crate) const fn needs_initialize(&self) -> bool {
        matches!(self.protocol.phase_kind(), ProtocolPhaseKind::Idle)
    }

    pub(crate) const fn latest_exact_cursor_cell(&self) -> Option<ScreenCell> {
        self.payload.latest_exact_cursor_cell
    }

    pub(crate) const fn fallback_cursor_cell(
        &self,
        observed_cursor_cell: Option<ScreenCell>,
    ) -> Option<ScreenCell> {
        match observed_cursor_cell {
            Some(cursor_cell) => Some(cursor_cell),
            None => self.latest_exact_cursor_cell(),
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

    pub(in crate::core::state) fn semantic_state(&self) -> &SemanticState {
        self.payload.semantics.as_ref()
    }

    pub(in crate::core::state) fn projection_state(&self) -> &ProjectionState {
        self.payload.projection.as_ref()
    }

    pub(crate) const fn demand_queue(&self) -> &DemandQueue {
        self.protocol.demand()
    }

    pub(crate) fn active_demand(&self) -> Option<&ExternalDemand> {
        self.pending_observation()
            .map(PendingObservation::demand)
            .or_else(|| self.observation().map(ObservationSnapshot::demand))
    }

    pub(crate) fn pending_observation(&self) -> Option<&PendingObservation> {
        self.protocol.pending_observation()
    }

    pub(crate) fn observation(&self) -> Option<&ObservationSnapshot> {
        self.protocol.observation()
    }

    pub(crate) fn observation_mut(&mut self) -> Option<&mut ObservationSnapshot> {
        self.protocol.observation_mut()
    }

    pub(crate) fn phase_observation(&self) -> Option<&ObservationSnapshot> {
        self.protocol.phase_observation()
    }

    pub(crate) fn retained_observation(&self) -> Option<&ObservationSnapshot> {
        self.protocol.retained_observation()
    }

    pub(crate) fn take_retained_observation(&mut self) -> Option<ObservationSnapshot> {
        self.protocol.take_retained_observation()
    }

    pub(crate) fn restore_retained_observation(
        &mut self,
        observation: Option<ObservationSnapshot>,
    ) -> bool {
        self.protocol.restore_retained_observation(observation)
    }

    #[cfg(test)]
    pub(crate) fn scene(&self) -> SceneState {
        self.shared_scene()
    }

    pub(crate) fn shared_scene(&self) -> SceneState {
        SceneState::from_parts(
            Rc::clone(&self.payload.semantics),
            Rc::clone(&self.payload.projection),
        )
    }

    pub(crate) const fn realization(&self) -> &RealizationLedger {
        &self.payload.realization
    }

    pub(crate) fn runtime(&self) -> &crate::state::RuntimeState {
        self.payload.motion.as_ref()
    }

    pub(crate) fn runtime_mut(&mut self) -> &mut crate::state::RuntimeState {
        Rc::make_mut(&mut self.payload.motion)
    }

    pub(crate) fn runtime_mut_with_observation(
        &mut self,
    ) -> Option<(&mut crate::state::RuntimeState, &ObservationSnapshot)> {
        let Self {
            protocol, payload, ..
        } = self;
        let observation = protocol.observation()?;
        Some((Rc::make_mut(&mut payload.motion), observation))
    }

    pub(crate) fn take_runtime(&mut self) -> crate::state::RuntimeState {
        std::mem::take(Rc::make_mut(&mut self.payload.motion))
    }

    pub(crate) fn pending_proposal(&self) -> Option<&InFlightProposal> {
        self.protocol.pending_proposal()
    }

    pub(crate) const fn pending_plan_proposal_id(&self) -> Option<ProposalId> {
        self.protocol.pending_plan_proposal_id()
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

    pub(crate) fn with_latest_exact_cursor_cell(
        mut self,
        latest_exact_cursor_cell: Option<ScreenCell>,
    ) -> Self {
        self.payload.latest_exact_cursor_cell = latest_exact_cursor_cell;
        self
    }

    pub(crate) fn with_realization(mut self, realization: RealizationLedger) -> Self {
        self.payload.realization = realization;
        self
    }

    pub(crate) fn with_runtime(mut self, runtime: crate::state::RuntimeState) -> Self {
        *Rc::make_mut(&mut self.payload.motion) = runtime;
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

    pub(crate) fn enter_observing_request(mut self, pending: PendingObservation) -> Option<Self> {
        self.recycle_staged_prepared_observation_plan();
        self.try_map_protocol(|protocol| {
            let (shared, phase) = protocol.into_parts();
            match phase {
                ProtocolPhase::Primed => Some(ProtocolState::collecting(
                    shared,
                    None,
                    pending,
                    ProbeRefreshState::default(),
                )),
                ProtocolPhase::Ready { active } => Some(ProtocolState::collecting(
                    shared,
                    Some(active),
                    pending,
                    ProbeRefreshState::default(),
                )),
                ProtocolPhase::Idle
                | ProtocolPhase::Collecting { .. }
                | ProtocolPhase::Observing { .. }
                | ProtocolPhase::Planning { .. }
                | ProtocolPhase::Applying { .. }
                | ProtocolPhase::Recovering { .. } => None,
            }
        })
    }

    #[cfg(test)]
    pub(crate) fn with_active_observation(self, observation: ObservationSnapshot) -> Option<Self> {
        let next_latest_exact_cursor_cell =
            self.fallback_cursor_cell(observation.exact_cursor_position());
        let mut state = self.with_latest_exact_cursor_cell(next_latest_exact_cursor_cell);
        state.activate_observation(observation).then_some(state)
    }

    #[cfg(test)]
    fn requested_probes_for_observation(
        observation: &ObservationSnapshot,
    ) -> super::ProbeRequestSet {
        let mut requested_probes = super::ProbeRequestSet::none();
        if observation.probes().cursor_color().is_requested() {
            requested_probes = requested_probes.with_requested(ProbeKind::CursorColor);
        }
        if observation.probes().background().is_requested() {
            requested_probes = requested_probes.with_requested(ProbeKind::Background);
        }
        requested_probes
    }

    #[cfg(test)]
    pub(crate) fn with_ready_observation(self, observation: ObservationSnapshot) -> Option<Self> {
        if self.phase_kind() != ProtocolPhaseKind::Primed {
            return None;
        }

        let pending = PendingObservation::new(
            observation.demand().clone(),
            Self::requested_probes_for_observation(&observation),
        );
        self.enter_observing_request(pending)?
            .with_active_observation(observation)?
            .with_completed_active_observation()
    }

    #[cfg(test)]
    pub(crate) fn with_completed_active_observation(self) -> Option<Self> {
        let mut state = self;
        state.complete_active_observation().then_some(state)
    }

    pub(crate) const fn probe_refresh_state(&self) -> Option<ProbeRefreshState> {
        self.protocol.probe_refresh_state()
    }

    pub(crate) fn activate_observation(&mut self, observation: ObservationSnapshot) -> bool {
        let previous_lifecycle = self.protocol.lifecycle();
        let protocol = std::mem::take(&mut self.protocol);
        match protocol {
            ProtocolState {
                shared,
                phase: ProtocolPhase::Collecting { probe_refresh, .. },
            } => {
                self.commit_protocol(
                    ProtocolState::observing(shared, Box::new(observation), probe_refresh, None),
                    previous_lifecycle,
                );
                true
            }
            protocol => {
                self.commit_protocol(protocol, previous_lifecycle);
                false
            }
        }
    }

    pub(crate) fn replace_active_observation_with_pending(
        &mut self,
        pending: PendingObservation,
    ) -> bool {
        self.recycle_staged_prepared_observation_plan();
        let previous_lifecycle = self.protocol.lifecycle();
        let protocol = std::mem::take(&mut self.protocol);
        match protocol {
            ProtocolState {
                shared,
                phase:
                    ProtocolPhase::Observing {
                        active,
                        probe_refresh,
                        ..
                    },
            } => {
                self.commit_protocol(
                    ProtocolState::collecting(shared, Some(active), pending, probe_refresh),
                    previous_lifecycle,
                );
                true
            }
            protocol => {
                self.commit_protocol(protocol, previous_lifecycle);
                false
            }
        }
    }

    pub(crate) fn complete_active_observation(&mut self) -> bool {
        self.recycle_staged_prepared_observation_plan();
        let previous_lifecycle = self.protocol.lifecycle();
        let protocol = std::mem::take(&mut self.protocol);
        match protocol {
            ProtocolState {
                shared,
                phase: ProtocolPhase::Observing { active, .. },
            } => {
                self.commit_protocol(ProtocolState::ready(shared, active), previous_lifecycle);
                true
            }
            protocol => {
                self.commit_protocol(protocol, previous_lifecycle);
                false
            }
        }
    }

    pub(crate) fn restore_retained_observation_to_ready(&mut self) -> bool {
        self.recycle_staged_prepared_observation_plan();
        let previous_lifecycle = self.protocol.lifecycle();
        let protocol = std::mem::take(&mut self.protocol);
        match protocol {
            ProtocolState {
                shared,
                phase:
                    ProtocolPhase::Recovering {
                        retained: Some(active),
                    },
            } => {
                self.commit_protocol(ProtocolState::ready(shared, active), previous_lifecycle);
                true
            }
            protocol => {
                self.commit_protocol(protocol, previous_lifecycle);
                false
            }
        }
    }

    pub(crate) fn enter_ready(&mut self, observation: ObservationSnapshot) -> bool {
        self.recycle_staged_prepared_observation_plan();
        let previous_lifecycle = self.protocol.lifecycle();
        let protocol = std::mem::take(&mut self.protocol);
        match protocol {
            ProtocolState {
                shared,
                phase: ProtocolPhase::Collecting { .. },
            } => {
                self.commit_protocol(
                    ProtocolState::ready(shared, Box::new(observation)),
                    previous_lifecycle,
                );
                true
            }
            protocol => {
                self.commit_protocol(protocol, previous_lifecycle);
                false
            }
        }
    }

    pub(crate) fn set_probe_refresh_state(&mut self, probe_refresh: ProbeRefreshState) -> bool {
        self.protocol.set_probe_refresh_state(probe_refresh)
    }

    #[cfg(test)]
    pub(crate) fn prepared_observation_plan(&self) -> Option<&PreparedObservationPlan> {
        self.protocol.prepared_observation_plan()
    }

    pub(crate) fn set_prepared_observation_plan(
        &mut self,
        prepared_plan: Option<PreparedObservationPlan>,
    ) -> bool {
        self.recycle_staged_prepared_observation_plan();
        self.protocol.set_prepared_observation_plan(prepared_plan)
    }

    pub(crate) fn take_prepared_observation_plan(&mut self) -> Option<PreparedObservationPlan> {
        self.protocol.take_prepared_observation_plan()
    }

    pub(crate) fn enter_planning(self, proposal_id: ProposalId) -> Option<Self> {
        self.try_map_protocol(|protocol| {
            let (shared, phase) = protocol.into_parts();
            match phase {
                ProtocolPhase::Ready { active } => {
                    Some(ProtocolState::planning(shared, active, proposal_id))
                }
                ProtocolPhase::Idle
                | ProtocolPhase::Primed
                | ProtocolPhase::Collecting { .. }
                | ProtocolPhase::Observing { .. }
                | ProtocolPhase::Planning { .. }
                | ProtocolPhase::Applying { .. }
                | ProtocolPhase::Recovering { .. } => None,
            }
        })
    }

    pub(crate) fn into_primed(mut self) -> Self {
        self.recycle_staged_prepared_observation_plan();
        self.map_protocol(|protocol| {
            let (shared, _) = protocol.into_parts();
            ProtocolState::primed(shared)
        })
    }

    #[cfg(test)]
    pub(crate) fn enter_applying(mut self, proposal: InFlightProposal) -> Option<Self> {
        self.recycle_staged_prepared_observation_plan();
        let proposal_id = proposal.proposal_id();
        let proposal = Box::new(proposal);
        self.try_map_protocol(|protocol| {
            let (shared, phase) = protocol.into_parts();
            match phase {
                ProtocolPhase::Planning {
                    active,
                    proposal_id: active_proposal_id,
                } if active_proposal_id == proposal_id => {
                    Some(ProtocolState::applying(shared, active, proposal))
                }
                ProtocolPhase::Idle
                | ProtocolPhase::Primed
                | ProtocolPhase::Collecting { .. }
                | ProtocolPhase::Observing { .. }
                | ProtocolPhase::Ready { .. }
                | ProtocolPhase::Planning { .. }
                | ProtocolPhase::Applying { .. }
                | ProtocolPhase::Recovering { .. } => None,
            }
        })
    }

    pub(crate) fn enter_recovering(mut self) -> Self {
        self.recycle_staged_prepared_observation_plan();
        self.map_protocol(|protocol| {
            let (shared, phase) = protocol.into_parts();
            ProtocolState::recovering(shared, phase.into_retained_observation())
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
        let previous_lifecycle = self.protocol.lifecycle();
        let protocol = std::mem::take(&mut self.protocol);
        match protocol {
            ProtocolState {
                shared,
                phase: ProtocolPhase::Applying { active, proposal },
            } if proposal.proposal_id() == proposal_id => {
                self.commit_protocol(ProtocolState::ready(shared, active), previous_lifecycle);
                Some(*proposal)
            }
            protocol => {
                self.commit_protocol(protocol, previous_lifecycle);
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
        let previous_lifecycle = self.protocol.lifecycle();
        let protocol = std::mem::take(&mut self.protocol);
        match protocol {
            ProtocolState {
                shared,
                phase:
                    ProtocolPhase::Planning {
                        active,
                        proposal_id: active_proposal_id,
                    },
            } if active_proposal_id == proposal_id => {
                self.commit_protocol(
                    ProtocolState::applying(shared, active, Box::new(proposal)),
                    previous_lifecycle,
                );
                scene_update.apply_to(
                    Rc::make_mut(&mut self.payload.semantics),
                    Rc::make_mut(&mut self.payload.projection),
                );
                true
            }
            protocol => {
                self.commit_protocol(protocol, previous_lifecycle);
                false
            }
        }
    }
}
