use super::snapshot::ObservationRequest;
use crate::core::types::ObservationId;
use crate::core::types::ProbeRequestId;

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub(crate) struct ProbeRequestSet {
    cursor_color: bool,
    background: bool,
}

impl ProbeRequestSet {
    pub(crate) const fn new(cursor_color: bool, background: bool) -> Self {
        Self {
            cursor_color,
            background,
        }
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
    Pending {
        request_id: ProbeRequestId,
    },
    Ready {
        request_id: ProbeRequestId,
        observed_from: ObservationId,
        reuse: ProbeReuse,
        value: T,
    },
    Failed {
        request_id: ProbeRequestId,
        failure: ProbeFailure,
    },
}

impl<T> ProbeState<T> {
    pub(crate) const fn pending(request_id: ProbeRequestId) -> Self {
        Self::Pending { request_id }
    }

    pub(crate) const fn ready(
        request_id: ProbeRequestId,
        observed_from: ObservationId,
        reuse: ProbeReuse,
        value: T,
    ) -> Self {
        Self::Ready {
            request_id,
            observed_from,
            reuse,
            value,
        }
    }

    pub(crate) const fn failed(request_id: ProbeRequestId, failure: ProbeFailure) -> Self {
        Self::Failed {
            request_id,
            failure,
        }
    }

    pub(crate) const fn request_id(&self) -> ProbeRequestId {
        match self {
            Self::Pending { request_id }
            | Self::Ready { request_id, .. }
            | Self::Failed { request_id, .. } => *request_id,
        }
    }

    pub(crate) fn value(&self) -> Option<&T> {
        match self {
            Self::Pending { .. } | Self::Failed { .. } => None,
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

    pub(crate) const fn pending(request_id: ProbeRequestId) -> Self {
        Self::Requested(ProbeState::pending(request_id))
    }

    pub(crate) const fn request_id(&self) -> Option<ProbeRequestId> {
        match self {
            Self::Unrequested => None,
            Self::Requested(state) => Some(state.request_id()),
        }
    }

    pub(crate) const fn is_pending(&self) -> bool {
        matches!(self, Self::Requested(ProbeState::Pending { .. }))
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
}

impl Default for ProbeSet {
    fn default() -> Self {
        Self {
            cursor_color: ProbeSlot::unrequested(),
        }
    }
}

impl ProbeSet {
    pub(crate) const fn cursor_color(&self) -> &ProbeSlot<Option<CursorColorSample>> {
        &self.cursor_color
    }

    pub(crate) fn from_request(request: &ObservationRequest) -> Self {
        let observation_id = request.observation_id();
        Self {
            cursor_color: if request.probes().cursor_color() {
                ProbeSlot::pending(ProbeKind::CursorColor.request_id(observation_id))
            } else {
                ProbeSlot::unrequested()
            },
        }
    }

    pub(super) fn set_cursor_color_state(
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
