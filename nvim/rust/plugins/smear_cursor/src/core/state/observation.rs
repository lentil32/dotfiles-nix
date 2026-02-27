use super::ExternalDemand;
use crate::core::runtime_reducer::ScrollShift;
use crate::core::types::{
    CursorPosition, CursorRow, Generation, Millis, ObservationId, ProbeRequestId, ViewportSnapshot,
};
use crate::state::CursorLocation;
use crate::types::ScreenCell;
use std::sync::Arc;

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

    pub(crate) const fn fingerprint(self) -> u64 {
        self.ordinal()
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

impl ProbeReuse {
    pub(crate) const fn fingerprint(self) -> u64 {
        match self {
            Self::Exact => 1_u64,
            Self::Compatible => 2_u64,
            Self::RefreshRequired => 3_u64,
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum ProbeFailure {
    ShellReadFailed,
}

impl ProbeFailure {
    pub(crate) const fn fingerprint(self) -> u64 {
        match self {
            Self::ShellReadFailed => 1_u64,
        }
    }
}

pub(crate) fn phase2_probe_fingerprint_seed() -> u64 {
    let reuse_seed = [
        ProbeReuse::Exact,
        ProbeReuse::Compatible,
        ProbeReuse::RefreshRequired,
    ]
    .iter()
    .copied()
    .map(ProbeReuse::fingerprint)
    .fold(0_u64, u64::wrapping_add);
    reuse_seed ^ ProbeFailure::ShellReadFailed.fingerprint().rotate_left(7)
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct CursorColorSample(String);

impl CursorColorSample {
    pub(crate) fn new(value: String) -> Self {
        Self(value)
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct CursorColorProbeWitness {
    buffer_handle: i64,
    changedtick: u64,
    mode: String,
    cursor_position: Option<CursorPosition>,
    colorscheme_generation: Generation,
}

impl CursorColorProbeWitness {
    pub(crate) fn new(
        buffer_handle: i64,
        changedtick: u64,
        mode: String,
        cursor_position: Option<CursorPosition>,
        colorscheme_generation: Generation,
    ) -> Self {
        Self {
            buffer_handle,
            changedtick,
            mode,
            cursor_position,
            colorscheme_generation,
        }
    }

    pub(crate) const fn buffer_handle(&self) -> i64 {
        self.buffer_handle
    }

    pub(crate) const fn changedtick(&self) -> u64 {
        self.changedtick
    }

    pub(crate) fn mode(&self) -> &str {
        &self.mode
    }

    pub(crate) const fn cursor_position(&self) -> Option<CursorPosition> {
        self.cursor_position
    }

    pub(crate) const fn colorscheme_generation(&self) -> Generation {
        self.colorscheme_generation
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct BackgroundProbeChunk {
    start_row: CursorRow,
    row_count: u32,
}

impl BackgroundProbeChunk {
    pub(crate) const fn new(start_row: CursorRow, row_count: u32) -> Self {
        Self {
            start_row,
            row_count,
        }
    }

    pub(crate) const fn start_row(self) -> CursorRow {
        self.start_row
    }

    pub(crate) const fn row_count(self) -> u32 {
        self.row_count
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct BackgroundProbeBatch {
    viewport: ViewportSnapshot,
    allowed_mask: Arc<[bool]>,
}

impl BackgroundProbeBatch {
    fn mask_len(viewport: ViewportSnapshot) -> usize {
        usize::try_from(viewport.max_row.value())
            .ok()
            .and_then(|rows| {
                usize::try_from(viewport.max_col.value())
                    .ok()
                    .and_then(|cols| rows.checked_mul(cols))
            })
            .unwrap_or(0)
    }

    pub(crate) fn empty(viewport: ViewportSnapshot) -> Self {
        Self {
            viewport,
            allowed_mask: Arc::from(vec![false; Self::mask_len(viewport)]),
        }
    }

    pub(crate) fn from_allowed_mask(viewport: ViewportSnapshot, allowed_mask: Vec<bool>) -> Self {
        let expected_len = Self::mask_len(viewport);
        if allowed_mask.len() != expected_len {
            // Surprising: the shell returned a malformed background probe batch. Keep the
            // probe conservative instead of fusing partial screen state into semantic truth.
            return Self::empty(viewport);
        }

        Self {
            viewport,
            allowed_mask: Arc::from(allowed_mask),
        }
    }

    pub(crate) const fn viewport(&self) -> ViewportSnapshot {
        self.viewport
    }

    pub(crate) fn allowed_mask(&self) -> &[bool] {
        self.allowed_mask.as_ref()
    }

    fn index_for(&self, cell: ScreenCell) -> Option<usize> {
        let row = u32::try_from(cell.row()).ok()?;
        let col = u32::try_from(cell.col()).ok()?;
        if row == 0
            || col == 0
            || row > self.viewport.max_row.value()
            || col > self.viewport.max_col.value()
        {
            return None;
        }

        let width = usize::try_from(self.viewport.max_col.value()).ok()?;
        let row_index = usize::try_from(row.saturating_sub(1)).ok()?;
        let col_index = usize::try_from(col.saturating_sub(1)).ok()?;
        row_index.checked_mul(width)?.checked_add(col_index)
    }

    pub(crate) fn allows_particle(&self, cell: ScreenCell) -> bool {
        self.index_for(cell)
            .and_then(|index| self.allowed_mask.get(index).copied())
            .unwrap_or(false)
    }
}

const MAX_BACKGROUND_PROBE_CELLS_PER_EDGE: u32 = 2048;

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct BackgroundProbeProgress {
    viewport: ViewportSnapshot,
    next_row: CursorRow,
    sampled_rows: Vec<Option<Arc<[bool]>>>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) enum BackgroundProbeUpdate {
    InProgress(BackgroundProbeProgress),
    Complete(BackgroundProbeBatch),
}

impl BackgroundProbeProgress {
    fn row_width(viewport: ViewportSnapshot) -> usize {
        usize::try_from(viewport.max_col.value()).unwrap_or(0)
    }

    fn row_count(viewport: ViewportSnapshot) -> usize {
        usize::try_from(viewport.max_row.value()).unwrap_or(0)
    }

    pub(crate) fn new(viewport: ViewportSnapshot) -> Self {
        Self {
            viewport,
            next_row: CursorRow(1),
            sampled_rows: vec![None; Self::row_count(viewport)],
        }
    }

    pub(crate) const fn viewport(&self) -> ViewportSnapshot {
        self.viewport
    }

    pub(crate) const fn next_row(&self) -> CursorRow {
        self.next_row
    }

    pub(crate) fn sampled_rows(&self) -> &[Option<Arc<[bool]>>] {
        self.sampled_rows.as_slice()
    }

    pub(crate) fn next_chunk(&self) -> Option<BackgroundProbeChunk> {
        if self.next_row.value() == 0 || self.next_row.value() > self.viewport.max_row.value() {
            return None;
        }

        let width = self.viewport.max_col.value().max(1);
        let rows_per_chunk = (MAX_BACKGROUND_PROBE_CELLS_PER_EDGE / width).max(1);
        let remaining_rows = self
            .viewport
            .max_row
            .value()
            .saturating_sub(self.next_row.value())
            .saturating_add(1);
        Some(BackgroundProbeChunk::new(
            self.next_row,
            remaining_rows.min(rows_per_chunk),
        ))
    }

    pub(crate) fn apply_chunk(
        &self,
        chunk: BackgroundProbeChunk,
        allowed_mask: &[bool],
    ) -> Option<BackgroundProbeUpdate> {
        if chunk.row_count() == 0 || chunk.start_row() != self.next_row {
            return None;
        }

        let width = Self::row_width(self.viewport);
        let row_count = usize::try_from(chunk.row_count()).ok()?;
        let expected_len = width.checked_mul(row_count)?;
        if allowed_mask.len() != expected_len {
            return None;
        }

        let start_row = usize::try_from(chunk.start_row().value().saturating_sub(1)).ok()?;
        let end_row = start_row.checked_add(row_count)?;
        if end_row > self.sampled_rows.len() {
            return None;
        }

        let mut sampled_rows = self.sampled_rows.clone();
        for (row_offset, row_index) in (start_row..end_row).enumerate() {
            let row_start = row_offset.checked_mul(width)?;
            let row_end = row_start.checked_add(width)?;
            sampled_rows[row_index] = Some(Arc::from(allowed_mask[row_start..row_end].to_vec()));
        }

        let next_row_value = chunk.start_row().value().saturating_add(chunk.row_count());
        if next_row_value > self.viewport.max_row.value() {
            let mut flattened = Vec::with_capacity(width.checked_mul(sampled_rows.len())?);
            for row in &sampled_rows {
                let row = row.as_ref()?;
                flattened.extend(row.iter().copied());
            }
            return Some(BackgroundProbeUpdate::Complete(
                BackgroundProbeBatch::from_allowed_mask(self.viewport, flattened),
            ));
        }

        Some(BackgroundProbeUpdate::InProgress(Self {
            viewport: self.viewport,
            next_row: CursorRow(next_row_value),
            sampled_rows,
        }))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ProbeState<T> {
    Missing,
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

    pub(crate) const fn request_id(&self) -> Option<ProbeRequestId> {
        match self {
            Self::Missing => None,
            Self::Pending { request_id }
            | Self::Ready { request_id, .. }
            | Self::Failed { request_id, .. } => Some(*request_id),
        }
    }

    pub(crate) const fn is_pending(&self) -> bool {
        matches!(self, Self::Pending { .. })
    }

    pub(crate) fn value(&self) -> Option<&T> {
        match self {
            Self::Missing | Self::Pending { .. } | Self::Failed { .. } => None,
            Self::Ready { value, .. } => Some(value),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ProbeSet {
    cursor_color: ProbeState<Option<CursorColorSample>>,
    background: ProbeState<BackgroundProbeBatch>,
}

impl Default for ProbeSet {
    fn default() -> Self {
        Self {
            cursor_color: ProbeState::Missing,
            background: ProbeState::Missing,
        }
    }
}

impl ProbeSet {
    pub(crate) const fn cursor_color(&self) -> &ProbeState<Option<CursorColorSample>> {
        &self.cursor_color
    }

    pub(crate) const fn background(&self) -> &ProbeState<BackgroundProbeBatch> {
        &self.background
    }

    pub(crate) fn from_request(request: &ObservationRequest) -> Self {
        let observation_id = request.observation_id();
        Self {
            cursor_color: if request.probes().cursor_color() {
                ProbeState::pending(ProbeKind::CursorColor.request_id(observation_id))
            } else {
                ProbeState::Missing
            },
            background: if request.probes().background() {
                ProbeState::pending(ProbeKind::Background.request_id(observation_id))
            } else {
                ProbeState::Missing
            },
        }
    }

    pub(crate) fn with_cursor_color(
        mut self,
        cursor_color: ProbeState<Option<CursorColorSample>>,
    ) -> Self {
        self.cursor_color = cursor_color;
        self
    }

    pub(crate) fn with_background(mut self, background: ProbeState<BackgroundProbeBatch>) -> Self {
        self.background = background;
        self
    }

    pub(crate) fn sampled_cursor_color(&self) -> Option<&str> {
        self.cursor_color
            .value()
            .and_then(|sample| sample.as_ref())
            .map(CursorColorSample::as_str)
    }

    pub(crate) fn sampled_background(&self) -> Option<&BackgroundProbeBatch> {
        self.background.value()
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

    pub(crate) const fn cursor_location(&self) -> CursorLocation {
        self.cursor_location
    }

    pub(crate) const fn viewport(&self) -> ViewportSnapshot {
        self.viewport
    }

    pub(crate) fn cursor_color_witness(&self) -> Option<&CursorColorProbeWitness> {
        self.cursor_color_witness.as_ref()
    }

    pub(crate) fn with_cursor_color_witness(
        mut self,
        cursor_color_witness: Option<CursorColorProbeWitness>,
    ) -> Self {
        self.cursor_color_witness = cursor_color_witness;
        self
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct ObservationMotion {
    scroll_shift: Option<ScrollShift>,
}

impl ObservationMotion {
    pub(crate) const fn new(scroll_shift: Option<ScrollShift>) -> Self {
        Self { scroll_shift }
    }

    pub(crate) const fn scroll_shift(&self) -> Option<ScrollShift> {
        self.scroll_shift
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ObservationSnapshot {
    request: ObservationRequest,
    basis: ObservationBasis,
    probes: ProbeSet,
    background_progress: Option<BackgroundProbeProgress>,
    motion: ObservationMotion,
}

impl ObservationSnapshot {
    pub(crate) fn new(
        request: ObservationRequest,
        basis: ObservationBasis,
        probes: ProbeSet,
        motion: ObservationMotion,
    ) -> Self {
        Self {
            background_progress: request
                .probes()
                .background()
                .then(|| BackgroundProbeProgress::new(basis.viewport())),
            request,
            basis,
            probes,
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
        self.background_progress.as_ref()
    }

    pub(crate) const fn motion(&self) -> ObservationMotion {
        self.motion
    }

    pub(crate) fn with_probes(mut self, probes: ProbeSet) -> Self {
        self.probes = probes;
        self
    }

    pub(crate) fn with_background_progress(
        mut self,
        background_progress: Option<BackgroundProbeProgress>,
    ) -> Self {
        self.background_progress = background_progress;
        self
    }

    pub(crate) fn cursor_color(&self) -> Option<&str> {
        self.probes.sampled_cursor_color()
    }

    pub(crate) fn background_probe(&self) -> Option<&BackgroundProbeBatch> {
        self.probes.sampled_background()
    }

    pub(crate) const fn scroll_shift(&self) -> Option<ScrollShift> {
        self.motion().scroll_shift()
    }
}
