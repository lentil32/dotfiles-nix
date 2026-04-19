use crate::position::ScreenCell;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(super) struct BackgroundProbeCellRange {
    source_start: usize,
    logical_start: usize,
    len: usize,
}

#[derive(Debug, Clone, Eq, PartialEq, Default)]
pub(super) struct BackgroundProbeCellView {
    source_cells: Arc<[ScreenCell]>,
    ranges: Arc<[BackgroundProbeCellRange]>,
    cell_count: usize,
}

pub(super) struct BackgroundProbeCellIter<'a> {
    source_cells: &'a [ScreenCell],
    ranges: &'a [BackgroundProbeCellRange],
    range_index: usize,
    next_source_index: usize,
    range_end: usize,
    remaining: usize,
}

impl BackgroundProbeCellRange {
    pub(super) const fn new(source_start: usize, logical_start: usize, len: usize) -> Self {
        Self {
            source_start,
            logical_start,
            len,
        }
    }

    const fn source_end(self) -> usize {
        self.source_start + self.len
    }

    const fn logical_end(self) -> usize {
        self.logical_start + self.len
    }
}

impl BackgroundProbeCellView {
    #[cfg(test)]
    pub(super) fn from_cells(mut cells: Vec<ScreenCell>) -> Self {
        cells.sort_unstable();
        cells.dedup();

        let cell_count = cells.len();
        let source_cells = Arc::from(cells);
        let ranges = if cell_count == 0 {
            Arc::default()
        } else {
            Arc::from(vec![BackgroundProbeCellRange::new(0, 0, cell_count)])
        };

        Self {
            source_cells,
            ranges,
            cell_count,
        }
    }

    pub(super) fn from_source_ranges(
        source_cells: Arc<[ScreenCell]>,
        ranges: Vec<BackgroundProbeCellRange>,
    ) -> Self {
        let cell_count = ranges
            .last()
            .copied()
            .map(BackgroundProbeCellRange::logical_end)
            .unwrap_or(0);

        Self {
            source_cells,
            ranges: Arc::from(ranges),
            cell_count,
        }
    }

    pub(super) const fn len(&self) -> usize {
        self.cell_count
    }

    pub(super) const fn is_empty(&self) -> bool {
        self.cell_count == 0
    }

    pub(super) fn iter(&self) -> BackgroundProbeCellIter<'_> {
        self.iter_range(0, self.cell_count)
    }

    pub(super) fn iter_range(
        &self,
        start_index: usize,
        end_index: usize,
    ) -> BackgroundProbeCellIter<'_> {
        if start_index >= end_index || end_index > self.cell_count {
            return BackgroundProbeCellIter::empty(&self.source_cells, &self.ranges);
        }

        let Some((range_index, offset)) = self.locate_range(start_index) else {
            return BackgroundProbeCellIter::empty(&self.source_cells, &self.ranges);
        };
        let range = self.ranges[range_index];

        BackgroundProbeCellIter {
            source_cells: &self.source_cells,
            ranges: &self.ranges,
            range_index,
            next_source_index: range.source_start + offset,
            range_end: range.source_end(),
            remaining: end_index.saturating_sub(start_index),
        }
    }

    pub(super) fn position_of(&self, cell: ScreenCell) -> Option<usize> {
        let mut left = 0_usize;
        let mut right = self.ranges.len();
        while left < right {
            let mid = left + (right - left) / 2;
            let range = self.ranges[mid];
            let slice = self
                .source_cells
                .get(range.source_start..range.source_end())?;
            let first = *slice.first()?;
            let last = *slice.last()?;

            if cell < first {
                right = mid;
                continue;
            }
            if cell > last {
                left = mid.saturating_add(1);
                continue;
            }

            return slice
                .binary_search(&cell)
                .ok()
                .map(|offset| range.logical_start + offset);
        }

        None
    }

    pub(super) fn is_sorted_unique(&self) -> bool {
        let mut iter = self.iter();
        let Some(mut previous) = iter.next() else {
            return true;
        };
        for cell in iter {
            if previous >= cell {
                return false;
            }
            previous = cell;
        }
        true
    }

    #[cfg(test)]
    pub(super) fn shares_source_with(&self, source_cells: &Arc<[ScreenCell]>) -> bool {
        Arc::ptr_eq(&self.source_cells, source_cells)
    }

    fn locate_range(&self, logical_index: usize) -> Option<(usize, usize)> {
        let mut left = 0_usize;
        let mut right = self.ranges.len();
        while left < right {
            let mid = left + (right - left) / 2;
            let range = self.ranges[mid];

            if logical_index < range.logical_start {
                right = mid;
                continue;
            }
            if logical_index >= range.logical_end() {
                left = mid.saturating_add(1);
                continue;
            }

            return Some((mid, logical_index - range.logical_start));
        }

        None
    }
}

impl<'a> BackgroundProbeCellIter<'a> {
    const fn empty(source_cells: &'a [ScreenCell], ranges: &'a [BackgroundProbeCellRange]) -> Self {
        Self {
            source_cells,
            ranges,
            range_index: 0,
            next_source_index: 0,
            range_end: 0,
            remaining: 0,
        }
    }
}

impl Iterator for BackgroundProbeCellIter<'_> {
    type Item = ScreenCell;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }

        let cell = *self.source_cells.get(self.next_source_index)?;
        self.next_source_index = self.next_source_index.saturating_add(1);
        self.remaining = self.remaining.saturating_sub(1);
        if self.next_source_index >= self.range_end && self.remaining > 0 {
            self.range_index = self.range_index.saturating_add(1);
            let range = *self.ranges.get(self.range_index)?;
            self.next_source_index = range.source_start;
            self.range_end = range.source_end();
        }

        Some(cell)
    }
}
