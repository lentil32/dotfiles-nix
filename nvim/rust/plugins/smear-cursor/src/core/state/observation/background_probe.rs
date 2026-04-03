use crate::core::types::ViewportSnapshot;
use crate::types::RenderFrame;
use crate::types::ScreenCell;
use std::collections::BTreeSet;
use std::sync::Arc;

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct BackgroundProbePlan {
    cells: Arc<[ScreenCell]>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct BackgroundProbeChunk {
    cells: Arc<[ScreenCell]>,
    start_index: usize,
    end_index: usize,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct BackgroundProbeBatch {
    viewport: ViewportSnapshot,
    probed_cells: Arc<[ScreenCell]>,
    allowed_mask: BackgroundProbeChunkMask,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct BackgroundProbeChunkMask {
    cell_count: usize,
    packed: Arc<[u8]>,
}

pub(crate) struct BackgroundProbePackedMaskIter<'a> {
    packed: &'a [u8],
    cell_count: usize,
    next_index: usize,
}

impl BackgroundProbePlan {
    pub(crate) fn from_cells(mut cells: Vec<ScreenCell>) -> Self {
        cells.sort_unstable();
        cells.dedup();
        Self {
            cells: Arc::from(cells),
        }
    }

    pub(crate) fn from_render_frame(frame: &RenderFrame, viewport: ViewportSnapshot) -> Self {
        let target_cell = ScreenCell::from_rounded_point(frame.target)
            .filter(|cell| in_viewport(viewport, *cell));
        let mut cells = BTreeSet::new();

        for particle in frame.particles.iter() {
            if !particle.position.row.is_finite() || !particle.position.col.is_finite() {
                continue;
            }

            let row = particle.position.row.floor() as i64;
            let col = particle.position.col.floor() as i64;
            let Some(cell) = ScreenCell::new(row, col) else {
                continue;
            };
            if Some(cell) == target_cell || !in_viewport(viewport, cell) {
                continue;
            }

            cells.insert(cell);
        }

        Self::from_cells(cells.into_iter().collect())
    }

    pub(crate) fn len(&self) -> usize {
        self.cells.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    fn chunk(&self, start_index: usize) -> Option<BackgroundProbeChunk> {
        if start_index >= self.cells.len() {
            return None;
        }

        let end_index = start_index
            .saturating_add(MAX_BACKGROUND_PROBE_CELLS_PER_CHUNK)
            .min(self.cells.len());
        Some(BackgroundProbeChunk::new(
            Arc::clone(&self.cells),
            start_index,
            end_index,
        ))
    }
}

impl BackgroundProbeChunk {
    pub(crate) const fn new(
        cells: Arc<[ScreenCell]>,
        start_index: usize,
        end_index: usize,
    ) -> Self {
        Self {
            cells,
            start_index,
            end_index,
        }
    }

    pub(crate) const fn start_index(&self) -> usize {
        self.start_index
    }

    pub(crate) fn len(&self) -> usize {
        self.end_index.saturating_sub(self.start_index)
    }

    pub(crate) fn cells(&self) -> &[ScreenCell] {
        &self.cells[self.start_index..self.end_index]
    }
}

impl BackgroundProbeChunkMask {
    fn packed_len_for(cell_count: usize) -> usize {
        let whole_bytes = cell_count / 8;
        if cell_count.is_multiple_of(8) {
            whole_bytes
        } else {
            whole_bytes.saturating_add(1)
        }
    }

    fn trailing_byte_mask(cell_count: usize) -> u8 {
        match cell_count % 8 {
            0 => u8::MAX,
            bits => ((1_u16 << bits) - 1) as u8,
        }
    }

    fn all_disallowed(cell_count: usize) -> Self {
        Self {
            cell_count,
            packed: Arc::from(vec![0; Self::packed_len_for(cell_count)]),
        }
    }

    pub(crate) fn from_allowed_mask(allowed_mask: &[bool]) -> Self {
        let mut packed = vec![0_u8; Self::packed_len_for(allowed_mask.len())];
        for (index, allowed) in allowed_mask.iter().copied().enumerate() {
            if allowed {
                packed[index / 8] |= 1_u8 << (index % 8);
            }
        }

        Self {
            cell_count: allowed_mask.len(),
            packed: Arc::from(packed),
        }
    }

    pub(crate) fn from_packed_bytes(cell_count: usize, packed: Vec<u8>) -> Option<Self> {
        if packed.len() != Self::packed_len_for(cell_count) {
            return None;
        }

        let mut packed = packed;
        if let Some(last_byte) = packed.last_mut() {
            *last_byte &= Self::trailing_byte_mask(cell_count);
        }

        Some(Self {
            cell_count,
            packed: Arc::from(packed),
        })
    }

    pub(crate) fn len(&self) -> usize {
        self.cell_count
    }

    pub(crate) fn packed_len(&self) -> usize {
        self.packed.len()
    }

    fn get(&self, index: usize) -> Option<bool> {
        if index >= self.cell_count {
            return None;
        }

        let byte = *self.packed.get(index / 8)?;
        Some(byte & (1_u8 << (index % 8)) != 0)
    }

    pub(crate) fn iter(&self) -> BackgroundProbePackedMaskIter<'_> {
        BackgroundProbePackedMaskIter {
            packed: &self.packed,
            cell_count: self.cell_count,
            next_index: 0,
        }
    }

    fn concatenate(masks: &[BackgroundProbeChunkMask]) -> Self {
        let total_len = masks.iter().map(Self::len).sum();
        let mut allowed = Vec::with_capacity(total_len);
        for mask in masks {
            allowed.extend(mask.iter());
        }
        Self::from_allowed_mask(&allowed)
    }
}

impl Iterator for BackgroundProbePackedMaskIter<'_> {
    type Item = bool;

    fn next(&mut self) -> Option<Self::Item> {
        if self.next_index >= self.cell_count {
            return None;
        }

        let index = self.next_index;
        self.next_index = self.next_index.saturating_add(1);
        let byte = *self.packed.get(index / 8)?;
        Some(byte & (1_u8 << (index % 8)) != 0)
    }
}

impl BackgroundProbeBatch {
    #[cfg(test)]
    fn viewport_cell_len(viewport: ViewportSnapshot) -> usize {
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
            probed_cells: Arc::from(Vec::<ScreenCell>::new()),
            allowed_mask: BackgroundProbeChunkMask::all_disallowed(0),
        }
    }

    #[cfg(test)]
    pub(crate) fn from_allowed_mask(viewport: ViewportSnapshot, allowed_mask: Vec<bool>) -> Self {
        let expected_len = Self::viewport_cell_len(viewport);
        if allowed_mask.len() != expected_len {
            // Surprising: the shell returned a malformed background probe batch. Keep the
            // probe conservative instead of fusing partial screen state into semantic truth.
            return Self::empty(viewport);
        }

        let Some(probed_cells) = viewport_cells(viewport) else {
            return Self::empty(viewport);
        };

        Self::from_probed_cells(
            viewport,
            Arc::from(probed_cells),
            BackgroundProbeChunkMask::from_allowed_mask(&allowed_mask),
        )
    }

    fn from_probed_cells(
        viewport: ViewportSnapshot,
        probed_cells: Arc<[ScreenCell]>,
        allowed_mask: BackgroundProbeChunkMask,
    ) -> Self {
        if probed_cells.len() != allowed_mask.len()
            || !is_sorted_unique(&probed_cells)
            || probed_cells
                .iter()
                .copied()
                .any(|cell| !in_viewport(viewport, cell))
        {
            return Self::empty(viewport);
        }

        Self {
            viewport,
            probed_cells,
            allowed_mask,
        }
    }

    pub(crate) const fn viewport(&self) -> ViewportSnapshot {
        self.viewport
    }

    pub(crate) fn allowed_mask_len(&self) -> usize {
        self.probed_cells.len()
    }

    pub(crate) fn allows_particle(&self, cell: ScreenCell) -> bool {
        self.probed_cells
            .binary_search(&cell)
            .ok()
            .and_then(|index| self.allowed_mask.get(index))
            .unwrap_or(false)
    }
}

#[cfg(test)]
fn viewport_cells(viewport: ViewportSnapshot) -> Option<Vec<ScreenCell>> {
    let max_row = i64::from(viewport.max_row.value());
    let max_col = i64::from(viewport.max_col.value());
    let capacity = BackgroundProbeBatch::viewport_cell_len(viewport);
    let mut cells = Vec::with_capacity(capacity);
    for row in 1..=max_row {
        for col in 1..=max_col {
            cells.push(ScreenCell::new(row, col)?);
        }
    }
    Some(cells)
}

fn is_sorted_unique(cells: &[ScreenCell]) -> bool {
    cells.windows(2).all(|pair| pair[0] < pair[1])
}

fn in_viewport(viewport: ViewportSnapshot, cell: ScreenCell) -> bool {
    let Ok(row) = u32::try_from(cell.row()) else {
        return false;
    };
    let Ok(col) = u32::try_from(cell.col()) else {
        return false;
    };
    row >= 1 && row <= viewport.max_row.value() && col >= 1 && col <= viewport.max_col.value()
}

const MAX_BACKGROUND_PROBE_CELLS_PER_CHUNK: usize = 2048;

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct BackgroundProbeProgress {
    viewport: ViewportSnapshot,
    plan: BackgroundProbePlan,
    next_cell_index: usize,
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
    chunk: BackgroundProbeChunk,
    mask: BackgroundProbeChunkMask,
}

impl BackgroundProbeProgress {
    pub(crate) fn new(viewport: ViewportSnapshot, plan: BackgroundProbePlan) -> Self {
        Self {
            viewport,
            plan,
            next_cell_index: 0,
            sampled_chunk_tail: None,
            sampled_chunk_count: 0,
        }
    }

    #[cfg(test)]
    pub(crate) const fn next_cell_index(&self) -> usize {
        self.next_cell_index
    }

    fn collect_sampled_chunks(
        tail: Option<&BackgroundProbeChunkNode>,
        chunk_count: usize,
    ) -> Vec<(BackgroundProbeChunk, BackgroundProbeChunkMask)> {
        let mut chunks = Vec::with_capacity(chunk_count);
        let mut current = tail;
        while let Some(node) = current {
            chunks.push((node.chunk.clone(), node.mask.clone()));
            current = node.previous.as_deref();
        }
        chunks.reverse();
        chunks
    }

    pub(crate) fn next_chunk(&self) -> Option<BackgroundProbeChunk> {
        self.plan.chunk(self.next_cell_index)
    }

    pub(crate) fn apply_chunk(
        &self,
        chunk: &BackgroundProbeChunk,
        allowed_mask: &BackgroundProbeChunkMask,
    ) -> Option<BackgroundProbeUpdate> {
        let expected_chunk = self.next_chunk()?;
        if expected_chunk != *chunk || chunk.len() == 0 || allowed_mask.len() != chunk.len() {
            return None;
        }

        let sampled_chunk_tail = Some(Arc::new(BackgroundProbeChunkNode {
            previous: self.sampled_chunk_tail.clone(),
            chunk: chunk.clone(),
            mask: allowed_mask.clone(),
        }));
        let sampled_chunk_count = self.sampled_chunk_count.saturating_add(1);
        let next_cell_index = chunk.end_index;
        if next_cell_index >= self.plan.len() {
            let masks =
                Self::collect_sampled_chunks(sampled_chunk_tail.as_deref(), sampled_chunk_count)
                    .into_iter()
                    .map(|(_, mask)| mask)
                    .collect::<Vec<_>>();
            return Some(BackgroundProbeUpdate::Complete(
                BackgroundProbeBatch::from_probed_cells(
                    self.viewport,
                    Arc::clone(&self.plan.cells),
                    BackgroundProbeChunkMask::concatenate(&masks),
                ),
            ));
        }

        Some(BackgroundProbeUpdate::InProgress(Self {
            viewport: self.viewport,
            plan: self.plan.clone(),
            next_cell_index,
            sampled_chunk_tail,
            sampled_chunk_count,
        }))
    }
}
