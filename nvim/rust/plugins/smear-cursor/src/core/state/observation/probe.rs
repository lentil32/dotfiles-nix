use super::background_probe::BackgroundProbeBatch;
use super::background_probe::BackgroundProbeChunk;
use super::background_probe::BackgroundProbeChunkMask;
use super::background_probe::BackgroundProbePlan;
use super::background_probe::BackgroundProbeProgress;
use super::background_probe::BackgroundProbeUpdate;
use crate::core::types::ObservationId;
use crate::core::types::ProbeRequestId;
use crate::core::types::ViewportSnapshot;

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub(crate) struct ProbeRequestSet {
    cursor_color: bool,
    background: bool,
}

impl ProbeRequestSet {
    pub(crate) const fn none() -> Self {
        Self {
            cursor_color: false,
            background: false,
        }
    }

    #[cfg(test)]
    pub(crate) const fn only(kind: ProbeKind) -> Self {
        Self::none().with_requested(kind)
    }

    pub(crate) const fn with_requested(mut self, kind: ProbeKind) -> Self {
        match kind {
            ProbeKind::CursorColor => {
                self.cursor_color = true;
            }
            ProbeKind::Background => {
                self.background = true;
            }
        }
        self
    }

    pub(crate) const fn cursor_color(self) -> bool {
        self.cursor_color
    }

    pub(crate) const fn background(self) -> bool {
        self.background
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum ProbeKind {
    CursorColor,
    Background,
}

impl ProbeKind {
    const fn ordinal(self) -> u64 {
        match self {
            Self::CursorColor => 1_u64,
            Self::Background => 2_u64,
        }
    }

    pub(crate) const fn request_id(self, observation_id: ObservationId) -> ProbeRequestId {
        ProbeRequestId::new(
            observation_id
                .value()
                .wrapping_mul(4)
                .wrapping_add(self.ordinal()),
        )
    }
}

pub(crate) const MAX_PROBE_REFRESH_RETRIES_PER_OBSERVATION: u8 = 2;

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub(crate) struct ProbeRefreshState {
    cursor_color_retries: u8,
    background_retries: u8,
}

impl ProbeRefreshState {
    pub(crate) const fn retry_count(self, kind: ProbeKind) -> u8 {
        match kind {
            ProbeKind::CursorColor => self.cursor_color_retries,
            ProbeKind::Background => self.background_retries,
        }
    }

    pub(crate) fn note_refresh_required(self, kind: ProbeKind) -> (Self, bool) {
        let next_retry_count = self.retry_count(kind).saturating_add(1);
        if next_retry_count > MAX_PROBE_REFRESH_RETRIES_PER_OBSERVATION {
            return (self, true);
        }

        let next_state = match kind {
            ProbeKind::CursorColor => Self {
                cursor_color_retries: next_retry_count,
                ..self
            },
            ProbeKind::Background => Self {
                background_retries: next_retry_count,
                ..self
            },
        };
        (next_state, false)
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum ProbeReuse {
    Exact,
    Compatible,
    RefreshRequired,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum ProbeFailure {
    ShellReadFailed,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct CursorColorSample(u32);

impl CursorColorSample {
    pub(crate) const fn new(value: u32) -> Self {
        Self(value)
    }

    pub(crate) const fn value(self) -> u32 {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ProbeState<T> {
    Pending,
    Ready { reuse: ProbeReuse, value: T },
    Failed { failure: ProbeFailure },
}

impl<T> ProbeState<T> {
    pub(crate) const fn pending() -> Self {
        Self::Pending
    }

    pub(crate) const fn ready(reuse: ProbeReuse, value: T) -> Self {
        Self::Ready { reuse, value }
    }

    pub(crate) const fn failed(failure: ProbeFailure) -> Self {
        Self::Failed { failure }
    }

    pub(crate) fn value(&self) -> Option<&T> {
        match self {
            Self::Pending | Self::Failed { .. } => None,
            Self::Ready { value, .. } => Some(value),
        }
    }

    pub(crate) const fn reuse(&self) -> Option<ProbeReuse> {
        match self {
            Self::Ready { reuse, .. } => Some(*reuse),
            Self::Pending { .. } | Self::Failed { .. } => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ProbeSlot<T> {
    Unrequested,
    Requested(ProbeState<T>),
}

impl<T> ProbeSlot<T> {
    pub(crate) const fn unrequested() -> Self {
        Self::Unrequested
    }

    pub(crate) const fn pending() -> Self {
        Self::Requested(ProbeState::pending())
    }

    pub(crate) const fn is_pending(&self) -> bool {
        matches!(self, Self::Requested(ProbeState::Pending))
    }

    pub(crate) const fn is_requested(&self) -> bool {
        !matches!(self, Self::Unrequested)
    }

    pub(crate) fn value(&self) -> Option<&T> {
        match self {
            Self::Unrequested => None,
            Self::Requested(state) => state.value(),
        }
    }

    pub(crate) const fn reuse(&self) -> Option<ProbeReuse> {
        match self {
            Self::Requested(state) => state.reuse(),
            Self::Unrequested => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ProbeSet {
    cursor_color: ProbeSlot<Option<CursorColorSample>>,
    background: BackgroundProbeState,
}

impl Default for ProbeSet {
    fn default() -> Self {
        Self {
            cursor_color: ProbeSlot::unrequested(),
            background: BackgroundProbeState::unrequested(),
        }
    }
}

impl ProbeSet {
    pub(crate) const fn new(
        cursor_color: ProbeSlot<Option<CursorColorSample>>,
        background: BackgroundProbeState,
    ) -> Self {
        Self {
            cursor_color,
            background,
        }
    }

    pub(crate) const fn cursor_color(&self) -> &ProbeSlot<Option<CursorColorSample>> {
        &self.cursor_color
    }

    pub(crate) const fn background(&self) -> &BackgroundProbeState {
        &self.background
    }

    pub(crate) fn background_mut(&mut self) -> &mut BackgroundProbeState {
        &mut self.background
    }

    pub(crate) fn set_cursor_color_state(
        &mut self,
        cursor_color: ProbeState<Option<CursorColorSample>>,
    ) -> bool {
        match &mut self.cursor_color {
            ProbeSlot::Unrequested => false,
            ProbeSlot::Requested(current) => {
                *current = cursor_color;
                true
            }
        }
    }

    pub(crate) fn sampled_cursor_color(&self) -> Option<u32> {
        self.cursor_color
            .value()
            .and_then(|sample| sample.as_ref())
            .copied()
            .map(CursorColorSample::value)
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) enum BackgroundProbeState {
    Unrequested,
    Collecting {
        progress: BackgroundProbeProgress,
    },
    Ready {
        reuse: ProbeReuse,
        batch: BackgroundProbeBatch,
    },
    Failed {
        failure: ProbeFailure,
    },
}

impl BackgroundProbeState {
    pub(crate) const fn unrequested() -> Self {
        Self::Unrequested
    }

    pub(crate) fn from_plan(plan: BackgroundProbePlan) -> Self {
        if plan.is_empty() {
            return Self::Ready {
                reuse: ProbeReuse::Exact,
                batch: BackgroundProbeBatch::empty(),
            };
        }

        Self::Collecting {
            progress: BackgroundProbeProgress::new(plan),
        }
    }

    pub(crate) const fn is_requested(&self) -> bool {
        !matches!(self, Self::Unrequested)
    }

    pub(crate) fn next_chunk(&self) -> Option<BackgroundProbeChunk> {
        match self {
            Self::Collecting { progress, .. } => progress.next_chunk(),
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

    fn set_ready(&mut self, reuse: ProbeReuse, batch: BackgroundProbeBatch) -> bool {
        match self {
            Self::Collecting { .. } => {
                *self = Self::Ready { reuse, batch };
                true
            }
            Self::Unrequested | Self::Ready { .. } | Self::Failed { .. } => false,
        }
    }

    pub(crate) fn accept_batch(
        &mut self,
        viewport: ViewportSnapshot,
        reuse: ProbeReuse,
        batch: BackgroundProbeBatch,
    ) -> bool {
        if !batch.is_valid_for_viewport(viewport) {
            return false;
        }

        self.set_ready(reuse, batch)
    }

    pub(crate) fn set_failed(&mut self, failure: ProbeFailure) -> bool {
        match self {
            Self::Collecting { .. } => {
                *self = Self::Failed { failure };
                true
            }
            Self::Unrequested | Self::Ready { .. } | Self::Failed { .. } => false,
        }
    }

    pub(crate) fn apply_chunk(
        &mut self,
        viewport: ViewportSnapshot,
        chunk: &BackgroundProbeChunk,
        allowed_mask: &BackgroundProbeChunkMask,
    ) -> bool {
        let update = match self {
            Self::Collecting { progress } => {
                let mut next_progress = progress.clone();
                next_progress
                    .apply_chunk(chunk, allowed_mask)
                    .map(|update| (next_progress, update))
            }
            Self::Unrequested | Self::Ready { .. } | Self::Failed { .. } => None,
        };

        match update {
            Some((next_progress, BackgroundProbeUpdate::InProgress)) => {
                *self = Self::Collecting {
                    progress: next_progress,
                };
                true
            }
            Some((_, BackgroundProbeUpdate::Complete(batch))) => {
                self.accept_batch(viewport, ProbeReuse::Exact, batch)
            }
            None => false,
        }
    }
}
