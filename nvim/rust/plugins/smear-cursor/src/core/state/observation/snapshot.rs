use super::background_probe::BackgroundProbeBatch;
use super::background_probe::BackgroundProbeChunk;
use super::background_probe::BackgroundProbeChunkMask;
use super::background_probe::BackgroundProbePlan;
use super::background_probe::BackgroundProbeProgress;
use super::cursor_context::CursorColorProbeWitness;
use super::cursor_context::CursorTextContextBoundary;
use super::cursor_context::CursorTextContextState;
use super::probe::CursorColorSample;
use super::probe::ProbeFailure;
use super::probe::ProbeKind;
use super::probe::ProbeRequestSet;
use super::probe::ProbeReuse;
use super::probe::ProbeSet;
use super::probe::ProbeSlot;
use super::probe::ProbeState;
use crate::core::runtime_reducer::ScrollShift;
use crate::core::state::ExternalDemand;
use crate::core::types::CursorPosition;
use crate::core::types::Millis;
use crate::core::types::ObservationId;
use crate::core::types::ProbeRequestId;
use crate::core::types::ViewportSnapshot;
use crate::state::CursorLocation;

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) enum BackgroundProbeState {
    Unrequested,
    Collecting {
        request_id: ProbeRequestId,
        progress: BackgroundProbeProgress,
    },
    Ready {
        request_id: ProbeRequestId,
        observed_from: ObservationId,
        reuse: ProbeReuse,
        batch: BackgroundProbeBatch,
    },
    Failed {
        request_id: ProbeRequestId,
        failure: ProbeFailure,
    },
}

impl BackgroundProbeState {
    const fn unrequested() -> Self {
        Self::Unrequested
    }

    fn from_plan(
        request_id: ProbeRequestId,
        observation_id: ObservationId,
        viewport: ViewportSnapshot,
        plan: BackgroundProbePlan,
    ) -> Self {
        if plan.is_empty() {
            return Self::Ready {
                request_id,
                observed_from: observation_id,
                reuse: ProbeReuse::Exact,
                batch: BackgroundProbeBatch::empty(viewport),
            };
        }

        Self::Collecting {
            request_id,
            progress: BackgroundProbeProgress::new(viewport, plan),
        }
    }

    pub(crate) const fn request_id(&self) -> Option<ProbeRequestId> {
        match self {
            Self::Unrequested => None,
            Self::Collecting { request_id, .. }
            | Self::Ready { request_id, .. }
            | Self::Failed { request_id, .. } => Some(*request_id),
        }
    }

    fn progress(&self) -> Option<&BackgroundProbeProgress> {
        match self {
            Self::Collecting { progress, .. } => Some(progress),
            Self::Unrequested | Self::Ready { .. } | Self::Failed { .. } => None,
        }
    }

    #[cfg(test)]
    fn progress_mut(&mut self) -> Option<&mut BackgroundProbeProgress> {
        match self {
            Self::Collecting { progress, .. } => Some(progress),
            Self::Unrequested | Self::Ready { .. } | Self::Failed { .. } => None,
        }
    }

    pub(crate) fn batch(&self) -> Option<&BackgroundProbeBatch> {
        match self {
            Self::Ready { batch, .. } => Some(batch),
            Self::Unrequested | Self::Collecting { .. } | Self::Failed { .. } => None,
        }
    }

    pub(crate) const fn reuse(&self) -> Option<ProbeReuse> {
        match self {
            Self::Ready { reuse, .. } => Some(*reuse),
            Self::Unrequested | Self::Collecting { .. } | Self::Failed { .. } => None,
        }
    }

    fn set_ready(
        &mut self,
        request_id: ProbeRequestId,
        observed_from: ObservationId,
        reuse: ProbeReuse,
        batch: BackgroundProbeBatch,
    ) -> bool {
        match self {
            Self::Collecting {
                request_id: current_request_id,
                ..
            } if *current_request_id == request_id => {
                *self = Self::Ready {
                    request_id,
                    observed_from,
                    reuse,
                    batch,
                };
                true
            }
            Self::Unrequested
            | Self::Collecting { .. }
            | Self::Ready { .. }
            | Self::Failed { .. } => false,
        }
    }

    fn set_failed(&mut self, request_id: ProbeRequestId, failure: ProbeFailure) -> bool {
        match self {
            Self::Collecting {
                request_id: current_request_id,
                ..
            } if *current_request_id == request_id => {
                *self = Self::Failed {
                    request_id,
                    failure,
                };
                true
            }
            Self::Unrequested
            | Self::Collecting { .. }
            | Self::Ready { .. }
            | Self::Failed { .. } => false,
        }
    }

    fn apply_chunk(
        &mut self,
        request_id: ProbeRequestId,
        observed_from: ObservationId,
        chunk: &BackgroundProbeChunk,
        allowed_mask: &BackgroundProbeChunkMask,
    ) -> bool {
        let update = match self {
            Self::Collecting {
                request_id: current_request_id,
                progress,
            } if *current_request_id == request_id => progress.apply_chunk(chunk, allowed_mask),
            Self::Unrequested
            | Self::Collecting { .. }
            | Self::Ready { .. }
            | Self::Failed { .. } => None,
        };
        match update {
            Some(super::background_probe::BackgroundProbeUpdate::InProgress) => true,
            Some(super::background_probe::BackgroundProbeUpdate::Complete(batch)) => {
                *self = Self::Ready {
                    request_id,
                    observed_from,
                    reuse: ProbeReuse::Exact,
                    batch,
                };
                true
            }
            None => false,
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct ObservationRequest {
    observation_id: ObservationId,
    demand: ExternalDemand,
    probes: ProbeRequestSet,
}

impl ObservationRequest {
    pub(crate) fn new(demand: ExternalDemand, probes: ProbeRequestSet) -> Self {
        Self {
            observation_id: ObservationId::from_ingress_seq(demand.seq()),
            demand,
            probes,
        }
    }

    pub(crate) const fn observation_id(&self) -> ObservationId {
        self.observation_id
    }

    pub(crate) const fn demand(&self) -> &ExternalDemand {
        &self.demand
    }

    pub(crate) const fn probes(&self) -> ProbeRequestSet {
        self.probes
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ObservationBasis {
    observation_id: ObservationId,
    observed_at: Millis,
    mode: String,
    cursor_position: Option<CursorPosition>,
    cursor_location: CursorLocation,
    viewport: ViewportSnapshot,
    cursor_color_witness: Option<CursorColorProbeWitness>,
    cursor_text_context_state: CursorTextContextState,
}

impl ObservationBasis {
    pub(crate) fn new(
        observation_id: ObservationId,
        observed_at: Millis,
        mode: String,
        cursor_position: Option<CursorPosition>,
        cursor_location: CursorLocation,
        viewport: ViewportSnapshot,
    ) -> Self {
        Self {
            observation_id,
            observed_at,
            mode,
            cursor_position,
            cursor_location,
            viewport,
            cursor_color_witness: None,
            cursor_text_context_state: CursorTextContextState::Unavailable,
        }
    }

    pub(crate) const fn observation_id(&self) -> ObservationId {
        self.observation_id
    }

    pub(crate) const fn observed_at(&self) -> Millis {
        self.observed_at
    }

    pub(crate) fn mode(&self) -> &str {
        &self.mode
    }

    pub(crate) const fn cursor_position(&self) -> Option<CursorPosition> {
        self.cursor_position
    }

    pub(crate) fn cursor_location(&self) -> CursorLocation {
        self.cursor_location.clone()
    }

    pub(crate) const fn viewport(&self) -> ViewportSnapshot {
        self.viewport
    }

    pub(crate) fn cursor_color_witness(&self) -> Option<&CursorColorProbeWitness> {
        self.cursor_color_witness.as_ref()
    }

    pub(crate) const fn cursor_text_context_boundary(&self) -> Option<CursorTextContextBoundary> {
        self.cursor_text_context_state.boundary()
    }

    pub(crate) const fn cursor_text_context_state(&self) -> &CursorTextContextState {
        &self.cursor_text_context_state
    }

    pub(crate) fn with_cursor_color_witness(
        mut self,
        cursor_color_witness: Option<CursorColorProbeWitness>,
    ) -> Self {
        self.cursor_color_witness = cursor_color_witness;
        self
    }

    pub(crate) fn with_cursor_text_context_state(
        mut self,
        cursor_text_context_state: CursorTextContextState,
    ) -> Self {
        self.cursor_text_context_state = cursor_text_context_state;
        self
    }

    #[cfg(debug_assertions)]
    fn debug_assert_invariants(&self) {}

    #[cfg(not(debug_assertions))]
    fn debug_assert_invariants(&self) {}
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct ObservationMotion {
    scroll_shift: Option<ScrollShift>,
    cursor_position_sync: CursorPositionSync,
}

impl ObservationMotion {
    pub(crate) const fn new(scroll_shift: Option<ScrollShift>) -> Self {
        Self {
            scroll_shift,
            cursor_position_sync: CursorPositionSync::Exact,
        }
    }

    pub(crate) const fn scroll_shift(&self) -> Option<ScrollShift> {
        self.scroll_shift
    }

    pub(crate) const fn requires_exact_cursor_position_refresh(&self) -> bool {
        matches!(
            self.cursor_position_sync,
            CursorPositionSync::ConcealDeferred
        )
    }

    pub(crate) const fn exact_cursor_position(
        &self,
        cursor_position: Option<CursorPosition>,
    ) -> Option<CursorPosition> {
        if self.requires_exact_cursor_position_refresh() {
            None
        } else {
            cursor_position
        }
    }

    pub(crate) const fn with_cursor_position_sync(
        self,
        cursor_position_sync: CursorPositionSync,
    ) -> Self {
        Self {
            cursor_position_sync,
            ..self
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub(crate) enum CursorPositionSync {
    #[default]
    Exact,
    ConcealDeferred,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ObservationSnapshot {
    request: ObservationRequest,
    basis: ObservationBasis,
    probes: ProbeSet,
    background_probe: BackgroundProbeState,
    motion: ObservationMotion,
}

impl ObservationSnapshot {
    pub(crate) fn new(
        request: ObservationRequest,
        basis: ObservationBasis,
        motion: ObservationMotion,
    ) -> Self {
        Self {
            probes: ProbeSet::from_request(&request),
            background_probe: BackgroundProbeState::unrequested(),
            request,
            basis,
            motion,
        }
    }

    pub(crate) const fn request(&self) -> &ObservationRequest {
        &self.request
    }

    pub(crate) const fn basis(&self) -> &ObservationBasis {
        &self.basis
    }

    pub(crate) const fn probes(&self) -> &ProbeSet {
        &self.probes
    }

    pub(crate) const fn background_probe_state(&self) -> &BackgroundProbeState {
        &self.background_probe
    }

    pub(crate) const fn background_probe_request_id(&self) -> Option<ProbeRequestId> {
        self.background_probe.request_id()
    }

    pub(crate) fn background_progress(&self) -> Option<&BackgroundProbeProgress> {
        self.background_probe.progress()
    }

    #[cfg(test)]
    pub(crate) fn background_progress_mut(&mut self) -> Option<&mut BackgroundProbeProgress> {
        self.background_probe.progress_mut()
    }

    pub(crate) const fn motion(&self) -> ObservationMotion {
        self.motion
    }

    #[cfg(debug_assertions)]
    pub(crate) fn debug_assert_invariants(&self) {
        self.basis.debug_assert_invariants();
        match (
            self.request.probes().cursor_color(),
            self.probes.cursor_color(),
        ) {
            (true, ProbeSlot::Requested(_)) | (false, ProbeSlot::Unrequested) => {}
            (true, ProbeSlot::Unrequested) => {
                debug_assert!(
                    false,
                    "requested cursor color probes must keep a requested slot"
                );
            }
            (false, ProbeSlot::Requested(_)) => {
                debug_assert!(
                    false,
                    "unrequested cursor color probes must not retain requested state"
                );
            }
        }
        if !self.request.probes().background() {
            debug_assert!(
                matches!(self.background_probe, BackgroundProbeState::Unrequested),
                "unrequested background probes must not retain lifecycle state"
            );
            return;
        }

        let expected_request_id = ProbeKind::Background.request_id(self.request.observation_id());
        if let Some(request_id) = self.background_probe.request_id() {
            debug_assert_eq!(
                request_id, expected_request_id,
                "background probe state must keep the observation-owned request id"
            );
        }

        if let BackgroundProbeState::Ready { batch, .. } = &self.background_probe {
            debug_assert_eq!(
                batch.viewport(),
                self.basis.viewport(),
                "background probe batches must match the observation viewport"
            );
        }
    }

    #[cfg(not(debug_assertions))]
    pub(crate) fn debug_assert_invariants(&self) {}

    #[cfg(test)]
    pub(crate) fn with_cursor_color_probe(
        mut self,
        cursor_color: ProbeState<Option<CursorColorSample>>,
    ) -> Option<Self> {
        self.set_cursor_color_probe(cursor_color).then_some(self)
    }

    pub(crate) fn set_cursor_color_probe(
        &mut self,
        cursor_color: ProbeState<Option<CursorColorSample>>,
    ) -> bool {
        self.probes.set_cursor_color_state(cursor_color)
    }

    #[cfg(test)]
    pub(crate) fn with_background_probe_ready(
        mut self,
        request_id: ProbeRequestId,
        observed_from: ObservationId,
        reuse: ProbeReuse,
        batch: BackgroundProbeBatch,
    ) -> Option<Self> {
        self.set_background_probe_ready(request_id, observed_from, reuse, batch)
            .then_some(self)
    }

    pub(crate) fn set_background_probe_ready(
        &mut self,
        request_id: ProbeRequestId,
        observed_from: ObservationId,
        reuse: ProbeReuse,
        batch: BackgroundProbeBatch,
    ) -> bool {
        self.background_probe
            .set_ready(request_id, observed_from, reuse, batch)
    }

    #[cfg(test)]
    pub(crate) fn with_background_probe_failed(
        mut self,
        request_id: ProbeRequestId,
        failure: ProbeFailure,
    ) -> Option<Self> {
        self.set_background_probe_failed(request_id, failure)
            .then_some(self)
    }

    pub(crate) fn set_background_probe_failed(
        &mut self,
        request_id: ProbeRequestId,
        failure: ProbeFailure,
    ) -> bool {
        self.background_probe.set_failed(request_id, failure)
    }

    pub(crate) fn apply_background_probe_chunk(
        &mut self,
        request_id: ProbeRequestId,
        chunk: &BackgroundProbeChunk,
        allowed_mask: &BackgroundProbeChunkMask,
    ) -> bool {
        self.background_probe.apply_chunk(
            request_id,
            self.request.observation_id(),
            chunk,
            allowed_mask,
        )
    }

    pub(crate) fn with_background_probe_plan(mut self, plan: BackgroundProbePlan) -> Self {
        if !self.request.probes().background() {
            return self;
        }

        let request_id = ProbeKind::Background.request_id(self.request.observation_id());
        self.background_probe = BackgroundProbeState::from_plan(
            request_id,
            self.request.observation_id(),
            self.basis.viewport(),
            plan,
        );
        self
    }

    pub(crate) fn cursor_color(&self) -> Option<u32> {
        self.probes.sampled_cursor_color()
    }

    pub(crate) fn background_probe(&self) -> Option<&BackgroundProbeBatch> {
        self.background_probe.batch()
    }

    pub(crate) const fn scroll_shift(&self) -> Option<ScrollShift> {
        self.motion().scroll_shift()
    }

    pub(crate) const fn exact_cursor_position(&self) -> Option<CursorPosition> {
        self.motion
            .exact_cursor_position(self.basis.cursor_position())
    }

    pub(crate) const fn requires_exact_cursor_color_refresh(&self) -> bool {
        matches!(
            self.probes.cursor_color().reuse(),
            Some(ProbeReuse::Compatible)
        )
    }

    pub(crate) const fn background_probe_reuse(&self) -> Option<ProbeReuse> {
        self.background_probe.reuse()
    }

    pub(crate) const fn requires_exact_cursor_position_refresh(&self) -> bool {
        self.motion.requires_exact_cursor_position_refresh()
    }
}
