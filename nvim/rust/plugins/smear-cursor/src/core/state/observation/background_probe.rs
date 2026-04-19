use super::background_probe_cells::BackgroundProbeCellRange;
use super::background_probe_cells::BackgroundProbeCellView;
use crate::core::types::ViewportSnapshot;
use crate::types::RenderFrame;
use crate::types::ScreenCell;
use std::sync::Arc;

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct BackgroundProbePlan {
    cells: BackgroundProbeCellView,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct BackgroundProbeChunk {
    cells: BackgroundProbeCellView,
    start_index: usize,
    end_index: usize,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct BackgroundProbeBatch {
    probed_cells: BackgroundProbeCellView,
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

#[derive(Debug, Clone, Eq, PartialEq, Default)]
struct BackgroundProbePackedMaskBuilder {
    packed_mask_bytes: Vec<u8>,
    next_bit_offset: usize,
}

impl BackgroundProbePlan {
    #[cfg(test)]
    pub(crate) fn from_cells(cells: Vec<ScreenCell>) -> Self {
        Self {
            cells: BackgroundProbeCellView::from_cells(cells),
        }
    }

    pub(crate) fn from_render_frame(frame: &RenderFrame, viewport: ViewportSnapshot) -> Self {
        let target_cell = ScreenCell::from_rounded_point(frame.target)
            .filter(|cell| in_viewport(viewport, *cell));
        let source_cells = Arc::clone(&frame.particle_screen_cells);
        let mut visible_cell_count = 0_usize;
        let mut active_range_start = None;
        let mut ranges = Vec::new();

        for (source_index, screen_cell) in source_cells.iter().copied().enumerate() {
            let include_cell =
                Some(screen_cell) != target_cell && in_viewport(viewport, screen_cell);
            if include_cell && active_range_start.is_none() {
                active_range_start = Some((source_index, visible_cell_count));
            }
            if include_cell {
                visible_cell_count = visible_cell_count.saturating_add(1);
                continue;
            }

            if let Some((range_start, logical_start)) = active_range_start.take() {
                ranges.push(BackgroundProbeCellRange::new(
                    range_start,
                    logical_start,
                    source_index.saturating_sub(range_start),
                ));
            }
        }

        if let Some((range_start, logical_start)) = active_range_start {
            ranges.push(BackgroundProbeCellRange::new(
                range_start,
                logical_start,
                source_cells.len().saturating_sub(range_start),
            ));
        }

        Self {
            cells: BackgroundProbeCellView::from_source_ranges(source_cells, ranges),
        }
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
            self.cells.clone(),
            start_index,
            end_index,
        ))
    }

    #[cfg(test)]
    pub(crate) fn shares_source_with(&self, source_cells: &Arc<[ScreenCell]>) -> bool {
        self.cells.shares_source_with(source_cells)
    }
}

impl BackgroundProbeChunk {
    fn new(cells: BackgroundProbeCellView, start_index: usize, end_index: usize) -> Self {
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

    pub(crate) fn iter_cells(&self) -> impl Iterator<Item = ScreenCell> + '_ {
        self.cells.iter_range(self.start_index, self.end_index)
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

    fn packed_bytes(&self) -> &[u8] {
        &self.packed
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

    pub(crate) fn empty() -> Self {
        Self {
            probed_cells: BackgroundProbeCellView::default(),
            allowed_mask: BackgroundProbeChunkMask::all_disallowed(0),
        }
    }

    #[cfg(test)]
    pub(crate) fn from_allowed_mask(viewport: ViewportSnapshot, allowed_mask: Vec<bool>) -> Self {
        let expected_len = Self::viewport_cell_len(viewport);
        if allowed_mask.len() != expected_len {
            // Surprising: the shell returned a malformed background probe batch. Keep the
            // probe conservative instead of fusing partial screen state into semantic truth.
            return Self::empty();
        }

        let Some(probed_cells) = viewport_cells(viewport) else {
            return Self::empty();
        };

        Self::from_probed_cells(
            BackgroundProbeCellView::from_cells(probed_cells),
            BackgroundProbeChunkMask::from_allowed_mask(&allowed_mask),
        )
    }

    fn from_probed_cells(
        probed_cells: BackgroundProbeCellView,
        allowed_mask: BackgroundProbeChunkMask,
    ) -> Self {
        if probed_cells.len() != allowed_mask.len() || !probed_cells.is_sorted_unique() {
            return Self::empty();
        }

        Self {
            probed_cells,
            allowed_mask,
        }
    }

    fn from_packed_mask(probed_cells: BackgroundProbeCellView, packed_mask: Vec<u8>) -> Self {
        let Some(allowed_mask) =
            BackgroundProbeChunkMask::from_packed_bytes(probed_cells.len(), packed_mask)
        else {
            return Self::empty();
        };

        Self::from_probed_cells(probed_cells, allowed_mask)
    }

    pub(crate) fn is_valid_for_viewport(&self, viewport: ViewportSnapshot) -> bool {
        self.probed_cells.len() == self.allowed_mask.len()
            && self.probed_cells.is_sorted_unique()
            && self
                .probed_cells
                .iter()
                .all(|cell| in_viewport(viewport, cell))
    }

    pub(crate) fn allowed_mask_len(&self) -> usize {
        self.probed_cells.len()
    }

    pub(crate) fn allows_particle(&self, cell: ScreenCell) -> bool {
        self.probed_cells
            .position_of(cell)
            .and_then(|index| self.allowed_mask.get(index))
            .unwrap_or(false)
    }
}

impl BackgroundProbePackedMaskBuilder {
    fn new(cell_count: usize) -> Self {
        Self {
            packed_mask_bytes: vec![0; BackgroundProbeChunkMask::packed_len_for(cell_count)],
            next_bit_offset: 0,
        }
    }

    fn append_mask(&mut self, mask: &BackgroundProbeChunkMask) -> Option<()> {
        let end_bit_offset = self.next_bit_offset.checked_add(mask.len())?;
        let total_bits = self.packed_mask_bytes.len().checked_mul(8)?;
        if end_bit_offset > total_bits {
            return None;
        }

        let destination_byte_start = self.next_bit_offset / 8;
        let bit_shift = self.next_bit_offset % 8;
        if bit_shift == 0 {
            let destination_byte_end = destination_byte_start.checked_add(mask.packed_len())?;
            let destination = self
                .packed_mask_bytes
                .get_mut(destination_byte_start..destination_byte_end)?;
            destination.copy_from_slice(mask.packed_bytes());
        } else {
            let carry_shift = 8_usize.saturating_sub(bit_shift);
            for (offset, source_byte) in mask.packed_bytes().iter().copied().enumerate() {
                let destination_index = destination_byte_start.checked_add(offset)?;
                *self.packed_mask_bytes.get_mut(destination_index)? |= source_byte << bit_shift;
                let carry = source_byte >> carry_shift;
                if carry != 0 {
                    let next_destination_index = destination_index.checked_add(1)?;
                    *self.packed_mask_bytes.get_mut(next_destination_index)? |= carry;
                }
            }
        }

        self.next_bit_offset = end_bit_offset;
        Some(())
    }

    fn into_packed_mask_bytes(self) -> Vec<u8> {
        self.packed_mask_bytes
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
    plan: BackgroundProbePlan,
    next_cell_index: usize,
    packed_mask: BackgroundProbePackedMaskBuilder,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) enum BackgroundProbeUpdate {
    InProgress,
    Complete(BackgroundProbeBatch),
}

impl BackgroundProbeProgress {
    pub(crate) fn new(plan: BackgroundProbePlan) -> Self {
        let packed_mask = BackgroundProbePackedMaskBuilder::new(plan.len());
        Self {
            plan,
            next_cell_index: 0,
            packed_mask,
        }
    }

    pub(crate) fn next_chunk(&self) -> Option<BackgroundProbeChunk> {
        self.plan.chunk(self.next_cell_index)
    }

    pub(crate) fn apply_chunk(
        &mut self,
        chunk: &BackgroundProbeChunk,
        allowed_mask: &BackgroundProbeChunkMask,
    ) -> Option<BackgroundProbeUpdate> {
        let expected_chunk = self.next_chunk()?;
        if expected_chunk != *chunk || chunk.len() == 0 || allowed_mask.len() != chunk.len() {
            return None;
        }

        self.packed_mask.append_mask(allowed_mask)?;
        self.next_cell_index = chunk.end_index;
        if self.next_cell_index >= self.plan.len() {
            return Some(BackgroundProbeUpdate::Complete(
                BackgroundProbeBatch::from_packed_mask(
                    self.plan.cells.clone(),
                    std::mem::take(&mut self.packed_mask).into_packed_mask_bytes(),
                ),
            ));
        }

        Some(BackgroundProbeUpdate::InProgress)
    }
}
