use super::background_probe::BackgroundProbeBatch;
use super::background_probe::BackgroundProbePlan;
use super::background_probe::BackgroundProbeProgress;
use super::cursor_context::CursorColorProbeWitness;
use super::cursor_context::CursorTextContext;
use super::probe::CursorColorSample;
use super::probe::ProbeKind;
use super::probe::ProbeRequestSet;
use super::probe::ProbeReuse;
use super::probe::ProbeSet;
use super::probe::ProbeState;
use crate::core::runtime_reducer::ScrollShift;
use crate::core::state::ExternalDemand;
use crate::core::types::CursorPosition;
use crate::core::types::Millis;
use crate::core::types::ObservationId;
use crate::core::types::ViewportSnapshot;
use crate::state::CursorLocation;

#[derive(Debug, Clone, PartialEq)]
enum BackgroundProbeProgressState {
    Unrequested,
    Pending(BackgroundProbeProgress),
    Complete,
}

impl BackgroundProbeProgressState {
    const fn unrequested() -> Self {
        Self::Unrequested
    }

    fn progress(&self) -> Option<&BackgroundProbeProgress> {
        match self {
            Self::Pending(progress) => Some(progress),
            Self::Unrequested | Self::Complete => None,
        }
    }

    fn with_progress(self, progress: BackgroundProbeProgress) -> Option<Self> {
        match self {
            Self::Pending(_) => Some(Self::Pending(progress)),
            Self::Unrequested | Self::Complete => None,
        }
    }

    fn complete(self) -> Option<Self> {
        match self {
            Self::Unrequested => None,
            Self::Pending(_) | Self::Complete => Some(Self::Complete),
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
    cursor_text_context: Option<CursorTextContext>,
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
            cursor_text_context: None,
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

    pub(crate) fn cursor_text_context(&self) -> Option<&CursorTextContext> {
        self.cursor_text_context.as_ref()
    }

    pub(crate) fn with_cursor_color_witness(
        mut self,
        cursor_color_witness: Option<CursorColorProbeWitness>,
    ) -> Self {
        self.cursor_color_witness = cursor_color_witness;
        self
    }

    pub(crate) fn with_cursor_text_context(
        mut self,
        cursor_text_context: Option<CursorTextContext>,
    ) -> Self {
        self.cursor_text_context = cursor_text_context;
        self
    }
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
    background_progress: BackgroundProbeProgressState,
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
            background_progress: BackgroundProbeProgressState::unrequested(),
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

    pub(crate) fn background_progress(&self) -> Option<&BackgroundProbeProgress> {
        self.background_progress.progress()
    }

    pub(crate) const fn motion(&self) -> ObservationMotion {
        self.motion
    }

    pub(crate) fn with_cursor_color_probe(
        mut self,
        cursor_color: ProbeState<Option<CursorColorSample>>,
    ) -> Option<Self> {
        self.probes = self.probes.with_cursor_color_state(cursor_color)?;
        Some(self)
    }

    pub(crate) fn with_background_progress(
        mut self,
        background_progress: BackgroundProbeProgress,
    ) -> Option<Self> {
        self.background_progress = self
            .background_progress
            .with_progress(background_progress)?;
        Some(self)
    }

    pub(crate) fn with_background_probe(
        mut self,
        background: ProbeState<BackgroundProbeBatch>,
    ) -> Option<Self> {
        self.probes = self.probes.with_background_state(background)?;
        self.background_progress = self.background_progress.complete()?;
        Some(self)
    }

    pub(crate) fn with_background_probe_plan(mut self, plan: BackgroundProbePlan) -> Self {
        if !self.request.probes().background() {
            return self;
        }

        let request_id = ProbeKind::Background.request_id(self.request.observation_id());
        if plan.is_empty() {
            self.probes = self.probes.with_background_ready(
                request_id,
                self.request.observation_id(),
                ProbeReuse::Exact,
                BackgroundProbeBatch::empty(self.basis.viewport()),
            );
            self.background_progress = BackgroundProbeProgressState::Complete;
            return self;
        }

        self.probes = self.probes.with_background_pending(request_id);
        self.background_progress = BackgroundProbeProgressState::Pending(
            BackgroundProbeProgress::new(self.basis.viewport(), plan),
        );
        self
    }

    pub(crate) fn cursor_color(&self) -> Option<u32> {
        self.probes.sampled_cursor_color()
    }

    pub(crate) fn background_probe(&self) -> Option<&BackgroundProbeBatch> {
        self.probes.sampled_background()
    }

    pub(crate) const fn scroll_shift(&self) -> Option<ScrollShift> {
        self.motion().scroll_shift()
    }

    pub(crate) const fn requires_exact_cursor_color_refresh(&self) -> bool {
        matches!(
            self.probes.cursor_color().reuse(),
            Some(ProbeReuse::Compatible)
        )
    }

    pub(crate) const fn requires_exact_cursor_position_refresh(&self) -> bool {
        self.motion.requires_exact_cursor_position_refresh()
    }
}
