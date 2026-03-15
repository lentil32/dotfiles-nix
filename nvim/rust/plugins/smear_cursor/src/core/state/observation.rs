#![cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "phase-2 probe compatibility and fingerprint scaffolding is intentionally retained ahead of trace integration"
    )
)]

use super::{ExternalDemand, ExternalDemandKind};
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

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct ObservedTextRow {
    line: i64,
    text: String,
}

impl ObservedTextRow {
    pub(crate) fn new(line: i64, text: String) -> Self {
        Self { line, text }
    }

    pub(crate) const fn line(&self) -> i64 {
        self.line
    }

    pub(crate) fn text(&self) -> &str {
        &self.text
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct CursorTextContext {
    buffer_handle: i64,
    changedtick: u64,
    cursor_line: i64,
    nearby_rows: Vec<ObservedTextRow>,
    tracked_cursor_line: Option<i64>,
    tracked_nearby_rows: Option<Vec<ObservedTextRow>>,
}

impl CursorTextContext {
    pub(crate) fn new(
        buffer_handle: i64,
        changedtick: u64,
        cursor_line: i64,
        nearby_rows: Vec<ObservedTextRow>,
        tracked_cursor_line: Option<i64>,
        tracked_nearby_rows: Option<Vec<ObservedTextRow>>,
    ) -> Self {
        Self {
            buffer_handle,
            changedtick,
            cursor_line,
            nearby_rows,
            tracked_cursor_line,
            tracked_nearby_rows,
        }
    }

    pub(crate) const fn buffer_handle(&self) -> i64 {
        self.buffer_handle
    }

    pub(crate) const fn changedtick(&self) -> u64 {
        self.changedtick
    }

    pub(crate) const fn cursor_line(&self) -> i64 {
        self.cursor_line
    }

    pub(crate) fn nearby_rows(&self) -> &[ObservedTextRow] {
        &self.nearby_rows
    }

    pub(crate) const fn tracked_cursor_line(&self) -> Option<i64> {
        self.tracked_cursor_line
    }

    pub(crate) fn tracked_nearby_rows(&self) -> Option<&[ObservedTextRow]> {
        self.tracked_nearby_rows.as_deref()
    }
}

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub(crate) enum SemanticEvent {
    #[default]
    FrameCommitted,
    ModeChanged,
    CursorMovedWithoutTextMutation,
    TextMutatedAtCursorContext,
    ViewportOrWindowMoved,
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
    allowed_mask: BackgroundProbeMask,
}

#[derive(Debug, Clone, Eq, PartialEq)]
enum BackgroundProbeMask {
    Flat(Arc<[bool]>),
    Chunked {
        row_width: usize,
        row_start_offsets: Arc<[usize]>,
        chunks: Arc<[Arc<[bool]>]>,
    },
}

pub(crate) enum BackgroundProbeMaskIter<'a> {
    Flat(std::iter::Copied<std::slice::Iter<'a, bool>>),
    Chunked(BackgroundProbeChunkMaskIter<'a>),
}

pub(crate) struct BackgroundProbeChunkMaskIter<'a> {
    chunks: std::slice::Iter<'a, Arc<[bool]>>,
    current: Option<std::iter::Copied<std::slice::Iter<'a, bool>>>,
}

impl<'a> BackgroundProbeChunkMaskIter<'a> {
    fn new(chunks: &'a [Arc<[bool]>]) -> Self {
        Self {
            chunks: chunks.iter(),
            current: None,
        }
    }
}

impl Iterator for BackgroundProbeChunkMaskIter<'_> {
    type Item = bool;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(current) = &mut self.current
                && let Some(value) = current.next()
            {
                return Some(value);
            }

            let next_chunk = self.chunks.next()?;
            self.current = Some(next_chunk.iter().copied());
        }
    }
}

impl Iterator for BackgroundProbeMaskIter<'_> {
    type Item = bool;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Flat(iter) => iter.next(),
            Self::Chunked(iter) => iter.next(),
        }
    }
}

impl BackgroundProbeMask {
    fn len(&self) -> usize {
        match self {
            Self::Flat(mask) => mask.len(),
            Self::Chunked { chunks, .. } => chunks.iter().map(|chunk| chunk.len()).sum(),
        }
    }

    fn iter(&self) -> BackgroundProbeMaskIter<'_> {
        match self {
            Self::Flat(mask) => BackgroundProbeMaskIter::Flat(mask.iter().copied()),
            Self::Chunked { chunks, .. } => {
                BackgroundProbeMaskIter::Chunked(BackgroundProbeChunkMaskIter::new(chunks))
            }
        }
    }

    fn allows(&self, row_width: usize, row_index: usize, col_index: usize) -> Option<bool> {
        match self {
            Self::Flat(mask) => {
                let index = row_index.checked_mul(row_width)?.checked_add(col_index)?;
                mask.get(index).copied()
            }
            Self::Chunked {
                row_width: chunk_row_width,
                row_start_offsets,
                chunks,
            } => {
                let chunk_boundary = row_start_offsets.partition_point(|start| *start <= row_index);
                let chunk_index = chunk_boundary.checked_sub(1)?;
                let chunk_start_row = *row_start_offsets.get(chunk_index)?;
                let row_in_chunk = row_index.checked_sub(chunk_start_row)?;
                let index = row_in_chunk
                    .checked_mul(*chunk_row_width)?
                    .checked_add(col_index)?;
                chunks.get(chunk_index)?.get(index).copied()
            }
        }
    }
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
            allowed_mask: BackgroundProbeMask::Flat(Arc::from(vec![
                false;
                Self::mask_len(viewport)
            ])),
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
            allowed_mask: BackgroundProbeMask::Flat(Arc::from(allowed_mask)),
        }
    }

    fn from_chunk_masks(viewport: ViewportSnapshot, chunks: Vec<Arc<[bool]>>) -> Self {
        let row_width = usize::try_from(viewport.max_col.value()).unwrap_or(0);
        let expected_len = Self::mask_len(viewport);
        if row_width == 0 {
            return if expected_len == 0 {
                Self {
                    viewport,
                    allowed_mask: BackgroundProbeMask::Chunked {
                        row_width,
                        row_start_offsets: Arc::from(Vec::<usize>::new()),
                        chunks: Arc::from(chunks),
                    },
                }
            } else {
                Self::empty(viewport)
            };
        }

        let mut row_start_offsets = Vec::with_capacity(chunks.len());
        let mut row_count = 0usize;
        let mut total_len = 0usize;
        for chunk in &chunks {
            if chunk.len() % row_width != 0 {
                // Surprising: probe progress assembled a non-rectangular background chunk.
                // Keep the batch conservative instead of trusting malformed shell state.
                return Self::empty(viewport);
            }
            row_start_offsets.push(row_count);
            row_count = match row_count.checked_add(chunk.len() / row_width) {
                Some(next) => next,
                None => return Self::empty(viewport),
            };
            total_len = match total_len.checked_add(chunk.len()) {
                Some(next) => next,
                None => return Self::empty(viewport),
            };
        }

        if total_len != expected_len {
            // Surprising: chunked probe progress completed with a malformed total mask length.
            return Self::empty(viewport);
        }

        Self {
            viewport,
            allowed_mask: BackgroundProbeMask::Chunked {
                row_width,
                row_start_offsets: Arc::from(row_start_offsets),
                chunks: Arc::from(chunks),
            },
        }
    }

    pub(crate) const fn viewport(&self) -> ViewportSnapshot {
        self.viewport
    }

    pub(crate) fn allowed_mask_len(&self) -> usize {
        self.allowed_mask.len()
    }

    pub(crate) fn allowed_mask_iter(&self) -> BackgroundProbeMaskIter<'_> {
        self.allowed_mask.iter()
    }

    fn coordinates_for(&self, cell: ScreenCell) -> Option<(usize, usize)> {
        let row = u32::try_from(cell.row()).ok()?;
        let col = u32::try_from(cell.col()).ok()?;
        if row == 0
            || col == 0
            || row > self.viewport.max_row.value()
            || col > self.viewport.max_col.value()
        {
            return None;
        }

        let row_index = usize::try_from(row.saturating_sub(1)).ok()?;
        let col_index = usize::try_from(col.saturating_sub(1)).ok()?;
        Some((row_index, col_index))
    }

    pub(crate) fn allows_particle(&self, cell: ScreenCell) -> bool {
        self.coordinates_for(cell)
            .and_then(|(row_index, col_index)| {
                let row_width = usize::try_from(self.viewport.max_col.value()).ok()?;
                self.allowed_mask.allows(row_width, row_index, col_index)
            })
            .unwrap_or(false)
    }
}

const MAX_BACKGROUND_PROBE_CELLS_PER_EDGE: u32 = 2048;

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct BackgroundProbeProgress {
    viewport: ViewportSnapshot,
    next_row: CursorRow,
    sampled_chunk_tail: Option<Arc<BackgroundProbeChunkNode>>,
    sampled_chunk_count: usize,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) enum BackgroundProbeUpdate {
    InProgress(BackgroundProbeProgress),
    Complete(BackgroundProbeBatch),
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct BackgroundProbeChunkNode {
    previous: Option<Arc<BackgroundProbeChunkNode>>,
    mask: Arc<[bool]>,
}

pub(crate) struct BackgroundProbeChunks<'a> {
    nodes: Vec<&'a BackgroundProbeChunkNode>,
}

impl<'a> Iterator for BackgroundProbeChunks<'a> {
    type Item = &'a Arc<[bool]>;

    fn next(&mut self) -> Option<Self::Item> {
        self.nodes.pop().map(|node| &node.mask)
    }
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
            sampled_chunk_tail: None,
            sampled_chunk_count: 0,
        }
    }

    pub(crate) const fn viewport(&self) -> ViewportSnapshot {
        self.viewport
    }

    pub(crate) const fn next_row(&self) -> CursorRow {
        self.next_row
    }

    pub(crate) fn sampled_chunks(&self) -> BackgroundProbeChunks<'_> {
        let mut nodes = Vec::with_capacity(self.sampled_chunk_count);
        let mut current = self.sampled_chunk_tail.as_deref();
        while let Some(node) = current {
            nodes.push(node);
            current = node.previous.as_deref();
        }

        BackgroundProbeChunks { nodes }
    }

    fn collect_sampled_chunks(
        tail: Option<&BackgroundProbeChunkNode>,
        chunk_count: usize,
    ) -> Vec<Arc<[bool]>> {
        let mut chunks = Vec::with_capacity(chunk_count);
        let mut current = tail;
        while let Some(node) = current {
            chunks.push(Arc::clone(&node.mask));
            current = node.previous.as_deref();
        }
        chunks.reverse();
        chunks
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
        if end_row > Self::row_count(self.viewport) {
            return None;
        }

        let sampled_chunk_tail = Some(Arc::new(BackgroundProbeChunkNode {
            previous: self.sampled_chunk_tail.clone(),
            mask: Arc::<[bool]>::from(allowed_mask),
        }));
        let sampled_chunk_count = self.sampled_chunk_count.saturating_add(1);

        let next_row_value = chunk.start_row().value().saturating_add(chunk.row_count());
        if next_row_value > self.viewport.max_row.value() {
            return Some(BackgroundProbeUpdate::Complete(
                BackgroundProbeBatch::from_chunk_masks(
                    self.viewport,
                    Self::collect_sampled_chunks(
                        sampled_chunk_tail.as_deref(),
                        sampled_chunk_count,
                    ),
                ),
            ));
        }

        Some(BackgroundProbeUpdate::InProgress(Self {
            viewport: self.viewport,
            next_row: CursorRow(next_row_value),
            sampled_chunk_tail,
            sampled_chunk_count,
        }))
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

    fn with_state(self, state: ProbeState<T>) -> Option<Self> {
        match self {
            Self::Unrequested => None,
            Self::Requested(_) => Some(Self::Requested(state)),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ProbeSet {
    cursor_color: ProbeSlot<Option<CursorColorSample>>,
    background: ProbeSlot<BackgroundProbeBatch>,
}

impl Default for ProbeSet {
    fn default() -> Self {
        Self {
            cursor_color: ProbeSlot::unrequested(),
            background: ProbeSlot::unrequested(),
        }
    }
}

impl ProbeSet {
    pub(crate) const fn cursor_color(&self) -> &ProbeSlot<Option<CursorColorSample>> {
        &self.cursor_color
    }

    pub(crate) const fn background(&self) -> &ProbeSlot<BackgroundProbeBatch> {
        &self.background
    }

    pub(crate) fn from_request(request: &ObservationRequest) -> Self {
        let observation_id = request.observation_id();
        Self {
            cursor_color: if request.probes().cursor_color() {
                ProbeSlot::pending(ProbeKind::CursorColor.request_id(observation_id))
            } else {
                ProbeSlot::unrequested()
            },
            background: if request.probes().background() {
                ProbeSlot::pending(ProbeKind::Background.request_id(observation_id))
            } else {
                ProbeSlot::unrequested()
            },
        }
    }

    fn with_cursor_color_state(
        mut self,
        cursor_color: ProbeState<Option<CursorColorSample>>,
    ) -> Option<Self> {
        self.cursor_color = self.cursor_color.with_state(cursor_color)?;
        Some(self)
    }

    fn with_background_state(
        mut self,
        background: ProbeState<BackgroundProbeBatch>,
    ) -> Option<Self> {
        self.background = self.background.with_state(background)?;
        Some(self)
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

#[derive(Debug, Clone, PartialEq)]
enum BackgroundProbeProgressState {
    Unrequested,
    Pending(BackgroundProbeProgress),
    Complete,
}

impl BackgroundProbeProgressState {
    fn from_request(request: &ObservationRequest, viewport: ViewportSnapshot) -> Self {
        if request.probes().background() {
            Self::Pending(BackgroundProbeProgress::new(viewport))
        } else {
            Self::Unrequested
        }
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
            background_progress: BackgroundProbeProgressState::from_request(
                &request,
                basis.viewport(),
            ),
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

fn cursor_motion_detected(previous: &ObservationBasis, current: &ObservationBasis) -> bool {
    previous.cursor_position() != current.cursor_position()
        || previous.cursor_location().line != current.cursor_location().line
}

fn viewport_or_window_moved(previous: &ObservationBasis, current: &ObservationBasis) -> bool {
    let previous_location = previous.cursor_location();
    let current_location = current.cursor_location();
    previous_location.window_handle != current_location.window_handle
        || previous_location.buffer_handle != current_location.buffer_handle
        || previous_location.top_row != current_location.top_row
        || previous.viewport() != current.viewport()
}

fn text_mutated_at_cursor_context(
    previous: Option<&CursorTextContext>,
    current: Option<&CursorTextContext>,
) -> bool {
    let (Some(previous), Some(current)) = (previous, current) else {
        return false;
    };
    if previous.buffer_handle() != current.buffer_handle()
        || previous.changedtick() == current.changedtick()
    {
        return false;
    }

    // Surprising: line numbers drift after insertions and deletions above the cursor, so text
    // mutation detection compares the last committed footprint against a footprint sampled around
    // the runtime's previously tracked cursor line instead of trusting absolute line numbers.
    current.tracked_nearby_rows().is_some_and(|tracked_rows| {
        rows_differ_by_relative_offset(previous.nearby_rows(), tracked_rows)
    }) || (previous.cursor_line() == current.cursor_line()
        && rows_differ_by_relative_offset(previous.nearby_rows(), current.nearby_rows()))
}

fn rows_differ_by_relative_offset(
    previous_rows: &[ObservedTextRow],
    current_rows: &[ObservedTextRow],
) -> bool {
    previous_rows.len() != current_rows.len()
        || previous_rows
            .iter()
            .zip(current_rows)
            .any(|(previous_row, current_row)| previous_row.text() != current_row.text())
}

pub(crate) fn classify_semantic_event(
    previous: Option<&ObservationSnapshot>,
    current: &ObservationSnapshot,
) -> SemanticEvent {
    let Some(previous) = previous else {
        return SemanticEvent::FrameCommitted;
    };

    let previous_basis = previous.basis();
    let current_basis = current.basis();
    if current.request().demand().kind() == ExternalDemandKind::ModeChanged
        || previous_basis.mode() != current_basis.mode()
    {
        return SemanticEvent::ModeChanged;
    }
    if text_mutated_at_cursor_context(
        previous_basis.cursor_text_context(),
        current_basis.cursor_text_context(),
    ) {
        return SemanticEvent::TextMutatedAtCursorContext;
    }
    if viewport_or_window_moved(previous_basis, current_basis) {
        return SemanticEvent::ViewportOrWindowMoved;
    }
    if cursor_motion_detected(previous_basis, current_basis) {
        return SemanticEvent::CursorMovedWithoutTextMutation;
    }

    SemanticEvent::FrameCommitted
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::state::{ExternalDemand, ExternalDemandKind};
    use crate::core::types::{CursorCol, IngressSeq};

    fn observation_request(probes: ProbeRequestSet) -> ObservationRequest {
        ObservationRequest::new(
            ExternalDemand::new(
                IngressSeq::new(1),
                ExternalDemandKind::ExternalCursor,
                Millis::new(10),
                None,
            ),
            probes,
        )
    }

    fn observation_basis(
        request: &ObservationRequest,
        viewport: ViewportSnapshot,
    ) -> ObservationBasis {
        ObservationBasis::new(
            request.observation_id(),
            Millis::new(10),
            "n".to_string(),
            Some(CursorPosition {
                row: CursorRow(4),
                col: CursorCol(5),
            }),
            CursorLocation::new(1, 1, 1, 1),
            viewport,
        )
    }

    fn observed_rows(rows: &[(i64, &str)]) -> Vec<ObservedTextRow> {
        rows.iter()
            .map(|(row, text)| ObservedTextRow::new(*row, (*text).to_string()))
            .collect()
    }

    fn text_context(
        changedtick: u64,
        line: i64,
        rows: &[(i64, &str)],
        tracked_line: Option<i64>,
        tracked_rows: Option<&[(i64, &str)]>,
    ) -> CursorTextContext {
        CursorTextContext::new(
            99,
            changedtick,
            line,
            observed_rows(rows),
            tracked_line,
            tracked_rows.map(observed_rows),
        )
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "The assertion helper keeps previous and current text contexts explicit at the call site."
    )]
    fn assert_text_mutation_classification(
        previous_position: CursorPosition,
        previous_line: i64,
        previous_rows: &[(i64, &str)],
        current_position: CursorPosition,
        current_line: i64,
        current_rows: &[(i64, &str)],
        current_tracked_line: Option<i64>,
        current_tracked_rows: Option<&[(i64, &str)]>,
    ) {
        let request = observation_request(ProbeRequestSet::default());
        let viewport = ViewportSnapshot::new(CursorRow(40), CursorCol(120));
        let previous = ObservationSnapshot::new(
            request.clone(),
            ObservationBasis::new(
                ObservationId::from_ingress_seq(IngressSeq::new(1)),
                Millis::new(10),
                "n".to_string(),
                Some(previous_position),
                CursorLocation::new(1, 1, 1, previous_line),
                viewport,
            )
            .with_cursor_text_context(Some(text_context(
                4,
                previous_line,
                previous_rows,
                None,
                None,
            ))),
            ObservationMotion::default(),
        );
        let current = ObservationSnapshot::new(
            request,
            ObservationBasis::new(
                ObservationId::from_ingress_seq(IngressSeq::new(1)),
                Millis::new(11),
                "n".to_string(),
                Some(current_position),
                CursorLocation::new(1, 1, 1, current_line),
                viewport,
            )
            .with_cursor_text_context(Some(text_context(
                5,
                current_line,
                current_rows,
                current_tracked_line,
                current_tracked_rows,
            ))),
            ObservationMotion::default(),
        );

        assert_eq!(
            classify_semantic_event(Some(&previous), &current),
            SemanticEvent::TextMutatedAtCursorContext
        );
    }

    #[test]
    fn unrequested_probe_slots_reject_probe_population() {
        let request = observation_request(ProbeRequestSet::default());
        let viewport = ViewportSnapshot::new(CursorRow(8), CursorCol(16));
        let snapshot = ObservationSnapshot::new(
            request.clone(),
            observation_basis(&request, viewport),
            ObservationMotion::default(),
        );

        assert!(matches!(
            snapshot.probes().cursor_color(),
            ProbeSlot::Unrequested
        ));
        assert!(matches!(
            snapshot.probes().background(),
            ProbeSlot::Unrequested
        ));
        assert!(snapshot.background_progress().is_none());
        assert!(
            snapshot
                .clone()
                .with_cursor_color_probe(ProbeState::ready(
                    ProbeKind::CursorColor.request_id(request.observation_id()),
                    request.observation_id(),
                    ProbeReuse::Exact,
                    Some(CursorColorSample::new("#abcdef".to_string())),
                ))
                .is_none()
        );
        assert!(
            snapshot
                .clone()
                .with_background_progress(BackgroundProbeProgress::new(viewport))
                .is_none()
        );
        assert!(
            snapshot
                .with_background_probe(ProbeState::failed(
                    ProbeKind::Background.request_id(request.observation_id()),
                    ProbeFailure::ShellReadFailed,
                ))
                .is_none()
        );
    }

    #[test]
    fn requested_background_probe_tracks_progress_until_completion() {
        let request = observation_request(ProbeRequestSet::new(false, true));
        let viewport = ViewportSnapshot::new(CursorRow(600), CursorCol(4));
        let mut snapshot = ObservationSnapshot::new(
            request.clone(),
            observation_basis(&request, viewport),
            ObservationMotion::default(),
        );
        let probe_request_id = ProbeKind::Background.request_id(request.observation_id());
        let mut saw_in_progress = false;

        loop {
            let progress = snapshot
                .background_progress()
                .expect("requested background probe should own chunk progress");
            let chunk = progress.next_chunk().expect("remaining background chunk");
            let width = usize::try_from(viewport.max_col.value()).expect("viewport width");
            let row_count = usize::try_from(chunk.row_count()).expect("chunk rows");
            let allowed_mask = vec![true; width * row_count];

            match progress
                .apply_chunk(chunk, &allowed_mask)
                .expect("chunk should match the active progress cursor")
            {
                BackgroundProbeUpdate::InProgress(next_progress) => {
                    saw_in_progress = true;
                    snapshot = snapshot
                        .with_background_progress(next_progress)
                        .expect("requested background probe should keep progress");
                }
                BackgroundProbeUpdate::Complete(batch) => {
                    snapshot = snapshot
                        .with_background_probe(ProbeState::ready(
                            probe_request_id,
                            request.observation_id(),
                            ProbeReuse::Exact,
                            batch,
                        ))
                        .expect("requested background probe should complete");
                    break;
                }
            }
        }

        assert!(
            saw_in_progress,
            "viewport should require multiple background chunks"
        );
        assert!(snapshot.background_progress().is_none());
        assert!(matches!(
            snapshot.probes().background(),
            ProbeSlot::Requested(ProbeState::Ready { .. })
        ));
        assert!(snapshot.background_probe().is_some());
    }

    #[test]
    fn semantic_classifier_detects_text_mutation_before_motion_only() {
        assert_text_mutation_classification(
            CursorPosition {
                row: CursorRow(4),
                col: CursorCol(5),
            },
            7,
            &[(6, "before"), (7, "alpha"), (8, "after")],
            CursorPosition {
                row: CursorRow(4),
                col: CursorCol(6),
            },
            8,
            &[(7, "alph"), (8, "a"), (9, "after")],
            Some(7),
            Some(&[(6, "before"), (7, "alph"), (8, "a")]),
        );
    }

    #[test]
    fn semantic_classifier_detects_insert_char_as_text_mutation() {
        assert_text_mutation_classification(
            CursorPosition {
                row: CursorRow(5),
                col: CursorCol(5),
            },
            9,
            &[(8, "header"), (9, "alpha"), (10, "tail")],
            CursorPosition {
                row: CursorRow(5),
                col: CursorCol(6),
            },
            9,
            &[(8, "header"), (9, "alphax"), (10, "tail")],
            Some(9),
            Some(&[(8, "header"), (9, "alphax"), (10, "tail")]),
        );
    }

    #[test]
    fn semantic_classifier_detects_backspace_as_text_mutation() {
        assert_text_mutation_classification(
            CursorPosition {
                row: CursorRow(5),
                col: CursorCol(6),
            },
            9,
            &[(8, "header"), (9, "alphax"), (10, "tail")],
            CursorPosition {
                row: CursorRow(5),
                col: CursorCol(5),
            },
            9,
            &[(8, "header"), (9, "alpha"), (10, "tail")],
            Some(9),
            Some(&[(8, "header"), (9, "alpha"), (10, "tail")]),
        );
    }

    #[test]
    fn semantic_classifier_detects_delete_as_text_mutation() {
        assert_text_mutation_classification(
            CursorPosition {
                row: CursorRow(5),
                col: CursorCol(5),
            },
            9,
            &[(8, "header"), (9, "alpha"), (10, "tail")],
            CursorPosition {
                row: CursorRow(5),
                col: CursorCol(5),
            },
            9,
            &[(8, "header"), (9, "alha"), (10, "tail")],
            Some(9),
            Some(&[(8, "header"), (9, "alha"), (10, "tail")]),
        );
    }

    #[test]
    fn semantic_classifier_detects_paste_as_text_mutation() {
        assert_text_mutation_classification(
            CursorPosition {
                row: CursorRow(5),
                col: CursorCol(5),
            },
            9,
            &[(8, "header"), (9, "alpha"), (10, "tail")],
            CursorPosition {
                row: CursorRow(6),
                col: CursorCol(3),
            },
            10,
            &[(9, "alpha pasted"), (10, "block"), (11, "tail")],
            Some(9),
            Some(&[(8, "header"), (9, "alpha pasted"), (10, "block")]),
        );
    }

    #[test]
    fn semantic_classifier_detects_ime_commit_as_text_mutation() {
        assert_text_mutation_classification(
            CursorPosition {
                row: CursorRow(5),
                col: CursorCol(5),
            },
            9,
            &[(8, "header"), (9, "ka"), (10, "tail")],
            CursorPosition {
                row: CursorRow(5),
                col: CursorCol(7),
            },
            9,
            &[(8, "header"), (9, "kana"), (10, "tail")],
            Some(9),
            Some(&[(8, "header"), (9, "kana"), (10, "tail")]),
        );
    }

    #[test]
    fn semantic_classifier_detects_motion_without_text_mutation() {
        let request = observation_request(ProbeRequestSet::default());
        let viewport = ViewportSnapshot::new(CursorRow(40), CursorCol(120));
        let previous = ObservationSnapshot::new(
            request.clone(),
            observation_basis(&request, viewport).with_cursor_text_context(Some(text_context(
                8,
                7,
                &[(6, "before"), (7, "alpha"), (8, "after")],
                None,
                None,
            ))),
            ObservationMotion::default(),
        );
        let current = ObservationSnapshot::new(
            request,
            ObservationBasis::new(
                ObservationId::from_ingress_seq(IngressSeq::new(1)),
                Millis::new(11),
                "n".to_string(),
                Some(CursorPosition {
                    row: CursorRow(5),
                    col: CursorCol(5),
                }),
                CursorLocation::new(1, 1, 1, 8),
                viewport,
            )
            .with_cursor_text_context(Some(text_context(
                8,
                8,
                &[(7, "alpha"), (8, "after"), (9, "tail")],
                Some(7),
                Some(&[(6, "before"), (7, "alpha"), (8, "after")]),
            ))),
            ObservationMotion::default(),
        );

        assert_eq!(
            classify_semantic_event(Some(&previous), &current),
            SemanticEvent::CursorMovedWithoutTextMutation
        );
    }

    #[test]
    fn semantic_classifier_detects_viewport_or_window_motion() {
        let request = observation_request(ProbeRequestSet::default());
        let viewport = ViewportSnapshot::new(CursorRow(40), CursorCol(120));
        let previous = ObservationSnapshot::new(
            request.clone(),
            observation_basis(&request, viewport),
            ObservationMotion::default(),
        );
        let current = ObservationSnapshot::new(
            request,
            ObservationBasis::new(
                ObservationId::from_ingress_seq(IngressSeq::new(1)),
                Millis::new(11),
                "n".to_string(),
                Some(CursorPosition {
                    row: CursorRow(4),
                    col: CursorCol(5),
                }),
                CursorLocation::new(2, 1, 3, 1),
                viewport,
            ),
            ObservationMotion::default(),
        );

        assert_eq!(
            classify_semantic_event(Some(&previous), &current),
            SemanticEvent::ViewportOrWindowMoved
        );
    }

    #[test]
    fn semantic_classifier_detects_mode_change() {
        let previous_request = observation_request(ProbeRequestSet::default());
        let current_request = ObservationRequest::new(
            ExternalDemand::new(
                IngressSeq::new(1),
                ExternalDemandKind::ModeChanged,
                Millis::new(10),
                None,
            ),
            ProbeRequestSet::default(),
        );
        let viewport = ViewportSnapshot::new(CursorRow(40), CursorCol(120));
        let previous = ObservationSnapshot::new(
            previous_request.clone(),
            observation_basis(&previous_request, viewport),
            ObservationMotion::default(),
        );
        let current = ObservationSnapshot::new(
            current_request.clone(),
            ObservationBasis::new(
                current_request.observation_id(),
                Millis::new(11),
                "i".to_string(),
                Some(CursorPosition {
                    row: CursorRow(4),
                    col: CursorCol(5),
                }),
                CursorLocation::new(1, 1, 1, 1),
                viewport,
            ),
            ObservationMotion::default(),
        );

        assert_eq!(
            classify_semantic_event(Some(&previous), &current),
            SemanticEvent::ModeChanged
        );
    }

    #[test]
    fn semantic_classifier_detects_multiline_paste_without_absolute_line_overlap() {
        assert_text_mutation_classification(
            CursorPosition {
                row: CursorRow(5),
                col: CursorCol(5),
            },
            9,
            &[(8, "header"), (9, "alpha"), (10, "tail")],
            CursorPosition {
                row: CursorRow(10),
                col: CursorCol(3),
            },
            14,
            &[(13, "inserted two"), (14, "inserted three"), (15, "tail")],
            Some(9),
            Some(&[(8, "header"), (9, "alpha pasted"), (10, "inserted one")]),
        );
    }

    #[test]
    fn semantic_classifier_prioritizes_text_mutation_before_viewport_motion() {
        let request = observation_request(ProbeRequestSet::default());
        let viewport = ViewportSnapshot::new(CursorRow(40), CursorCol(120));
        let previous = ObservationSnapshot::new(
            request.clone(),
            ObservationBasis::new(
                request.observation_id(),
                Millis::new(10),
                "n".to_string(),
                Some(CursorPosition {
                    row: CursorRow(5),
                    col: CursorCol(5),
                }),
                CursorLocation::new(1, 1, 1, 9),
                viewport,
            )
            .with_cursor_text_context(Some(text_context(
                10,
                9,
                &[(8, "header"), (9, "alpha"), (10, "tail")],
                None,
                None,
            ))),
            ObservationMotion::default(),
        );
        let current = ObservationSnapshot::new(
            request,
            ObservationBasis::new(
                ObservationId::from_ingress_seq(IngressSeq::new(1)),
                Millis::new(11),
                "n".to_string(),
                Some(CursorPosition {
                    row: CursorRow(6),
                    col: CursorCol(3),
                }),
                CursorLocation::new(1, 1, 4, 10),
                viewport,
            )
            .with_cursor_text_context(Some(text_context(
                11,
                10,
                &[(9, "alpha pasted"), (10, "block"), (11, "tail")],
                Some(9),
                Some(&[(8, "header"), (9, "alpha pasted"), (10, "block")]),
            ))),
            ObservationMotion::default(),
        );

        assert_eq!(
            classify_semantic_event(Some(&previous), &current),
            SemanticEvent::TextMutatedAtCursorContext
        );
    }
}
