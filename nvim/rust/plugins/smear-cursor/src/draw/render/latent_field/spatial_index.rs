//! Row-major indexes over cell maps used by the planner's bounded queries.
//!
//! # Safety Contract
//!
//! `BorrowedCellRows` stores `NonNull<T>` pointers into values owned by an
//! external `BTreeMap<(i64, i64), T>`. That is sound because
//! `BorrowedCellRows::build()` ties three borrows to the same lifetime:
//!
//! - the immutable borrow of the source map
//! - the mutable borrow of the scratch buffers that store row metadata
//! - the returned borrowed row index
//!
//! Callers must treat the index as a view into both inputs. While a
//! `BorrowedCellRows<'_, T>` is alive, the source map must stay immutably
//! borrowed and the scratch buffers must not be rebuilt or otherwise reused.
//! The unsafe dereference in `for_each_in_bounds()` relies on that contract.

use super::CellRect;
use std::collections::BTreeMap;
use std::marker::PhantomData;
use std::ptr::NonNull;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(in super::super) struct CellRows<T> {
    rows: BTreeMap<i64, BTreeMap<i64, T>>,
}

impl<T> CellRows<T> {
    pub(in super::super) fn len(&self) -> usize {
        self.rows.values().map(BTreeMap::len).sum()
    }

    pub(in super::super) fn clear(&mut self) {
        self.rows.clear();
    }

    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "the row-oriented insert helper is landed ahead of the planner fast path"
        )
    )]
    pub(in super::super) fn insert(&mut self, coord: (i64, i64), value: T) -> Option<T> {
        let (row, col) = coord;
        self.rows.entry(row).or_default().insert(col, value)
    }

    pub(in super::super) fn entry_mut(&mut self, coord: (i64, i64)) -> &mut T
    where
        T: Default,
    {
        let (row, col) = coord;
        self.rows.entry(row).or_default().entry(col).or_default()
    }

    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "read-only probes will use direct row lookups once the local-query path moves over"
        )
    )]
    pub(in super::super) fn get(&self, coord: (i64, i64)) -> Option<&T> {
        self.rows.get(&coord.0)?.get(&coord.1)
    }

    pub(in super::super) fn get_mut(&mut self, coord: (i64, i64)) -> Option<&mut T> {
        self.rows.get_mut(&coord.0)?.get_mut(&coord.1)
    }

    pub(in super::super) fn remove(&mut self, coord: (i64, i64)) -> Option<T> {
        let (row, col) = coord;
        let removed = self.rows.get_mut(&row).and_then(|cols| cols.remove(&col));
        let remove_row = self.rows.get(&row).is_some_and(BTreeMap::is_empty);
        if remove_row {
            let _ = self.rows.remove(&row);
        }
        removed
    }

    pub(in super::super) fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub(in super::super) fn iter(&self) -> impl Iterator<Item = ((i64, i64), &T)> {
        self.rows
            .iter()
            .flat_map(|(&row, cols)| cols.iter().map(move |(&col, value)| ((row, col), value)))
    }

    pub(in super::super) fn iter_in_bounds(
        &self,
        bounds: CellRect,
    ) -> impl Iterator<Item = ((i64, i64), &T)> {
        self.rows
            .range(bounds.min_row..=bounds.max_row)
            .flat_map(move |(&row, cols)| {
                cols.range(bounds.min_col..=bounds.max_col)
                    .map(move |(&col, value)| ((row, col), value))
            })
    }

    pub(in super::super) fn for_each(&self, mut visit: impl FnMut((i64, i64), &T)) {
        for (coord, value) in self.iter() {
            visit(coord, value);
        }
    }

    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "the row-bounded iterator lands before the planner switches to bounded field compilation"
        )
    )]
    pub(in super::super) fn for_each_in_bounds(
        &self,
        bounds: CellRect,
        mut visit: impl FnMut((i64, i64), &T),
    ) -> CellRowQueryStats {
        let mut stats = CellRowQueryStats::default();
        for (&row, cols) in self.rows.range(bounds.min_row..=bounds.max_row) {
            stats.bucket_maps_scanned = stats.bucket_maps_scanned.saturating_add(1);
            stats.bucket_cells_scanned = stats.bucket_cells_scanned.saturating_add(cols.len());
            for (&col, value) in cols.range(bounds.min_col..=bounds.max_col) {
                stats.local_query_cells = stats.local_query_cells.saturating_add(1);
                visit((row, col), value);
            }
        }
        stats
    }
}

#[derive(Clone, Copy, Debug)]
struct IndexedRow {
    row: i64,
    start: usize,
    end: usize,
}

#[derive(Clone, Copy, Debug)]
struct IndexedRowEntry<T> {
    col: i64,
    value: NonNull<T>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(in super::super) struct CellRowQueryStats {
    pub(in super::super) bucket_maps_scanned: usize,
    pub(in super::super) bucket_cells_scanned: usize,
    pub(in super::super) local_query_cells: usize,
}

#[derive(Debug, Default)]
pub(in super::super) struct BorrowedCellRowsScratch<T> {
    rows: Vec<IndexedRow>,
    entries: Vec<IndexedRowEntry<T>>,
}

/// Borrowed row-major index over an immutable cell map.
///
/// The index is valid only for the shared lifetime of the source map and the
/// scratch buffers populated during `build()`. See the module-level safety
/// contract above.
pub(in super::super) struct BorrowedCellRows<'a, T> {
    rows: &'a [IndexedRow],
    entries: &'a [IndexedRowEntry<T>],
    _marker: PhantomData<&'a T>,
}

impl<T> BorrowedCellRowsScratch<T> {
    #[cfg(test)]
    pub(in super::super) fn row_capacity(&self) -> usize {
        self.rows.capacity()
    }

    #[cfg(test)]
    pub(in super::super) fn entry_capacity(&self) -> usize {
        self.entries.capacity()
    }

    fn build<'a>(&'a mut self, cells: &'a BTreeMap<(i64, i64), T>) -> BorrowedCellRows<'a, T> {
        self.rows.clear();
        self.entries.clear();
        self.entries
            .reserve(cells.len().saturating_sub(self.entries.capacity()));

        let mut active_row = None;
        let mut row_start = 0_usize;
        for (&(row, col), value) in cells {
            if active_row != Some(row)
                && let Some(previous_row) = active_row.replace(row)
            {
                self.rows.push(IndexedRow {
                    row: previous_row,
                    start: row_start,
                    end: self.entries.len(),
                });
                row_start = self.entries.len();
            }
            self.entries.push(IndexedRowEntry {
                col,
                value: NonNull::from(value),
            });
        }

        if let Some(row) = active_row {
            self.rows.push(IndexedRow {
                row,
                start: row_start,
                end: self.entries.len(),
            });
        }

        BorrowedCellRows {
            rows: &self.rows,
            entries: &self.entries,
            _marker: PhantomData,
        }
    }
}

impl<'a, T> BorrowedCellRows<'a, T> {
    pub(in super::super) fn build(
        cells: &'a BTreeMap<(i64, i64), T>,
        scratch: &'a mut BorrowedCellRowsScratch<T>,
    ) -> Self {
        scratch.build(cells)
    }

    pub(in super::super) fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub(in super::super) fn for_each_in_bounds(
        &self,
        bounds: CellRect,
        mut visit: impl FnMut((i64, i64), &'a T),
    ) -> CellRowQueryStats {
        let row_start = self
            .rows
            .partition_point(|bucket| bucket.row < bounds.min_row);
        let row_end = self
            .rows
            .partition_point(|bucket| bucket.row <= bounds.max_row);
        let mut stats = CellRowQueryStats::default();
        for bucket in &self.rows[row_start..row_end] {
            stats.bucket_maps_scanned = stats.bucket_maps_scanned.saturating_add(1);
            let row_entries = &self.entries[bucket.start..bucket.end];
            stats.bucket_cells_scanned =
                stats.bucket_cells_scanned.saturating_add(row_entries.len());
            let col_start = row_entries.partition_point(|entry| entry.col < bounds.min_col);
            let col_end = row_entries.partition_point(|entry| entry.col <= bounds.max_col);
            stats.local_query_cells = stats
                .local_query_cells
                .saturating_add(col_end.saturating_sub(col_start));
            for entry in &row_entries[col_start..col_end] {
                // SAFETY: see the module-level safety contract. `build()` snapshots pointers from
                // `cells` while borrowing both `cells` and `scratch` for `'a`, so callers cannot
                // mutate/drop the source map or reuse the scratch buffers until this index drops.
                let value = unsafe { entry.value.as_ref() };
                visit((bucket.row, entry.col), value);
            }
        }
        stats
    }
}

#[cfg(test)]
mod tests {
    use super::BorrowedCellRows;
    use super::BorrowedCellRowsScratch;
    use super::CellRect;
    use super::CellRowQueryStats;
    use super::CellRows;
    use pretty_assertions::assert_eq;
    use std::collections::BTreeMap;

    #[test]
    fn cell_rows_preserve_deterministic_row_major_iteration_order() {
        let entries = [
            ((12_i64, 4_i64), 124_u32),
            ((10_i64, 9_i64), 109_u32),
            ((10_i64, 3_i64), 103_u32),
            ((11_i64, 2_i64), 112_u32),
        ];
        let expected = entries.into_iter().collect::<BTreeMap<_, _>>();
        let mut rows = CellRows::default();

        for (coord, value) in entries {
            let _ = rows.insert(coord, value);
        }

        let iterated = rows
            .iter()
            .map(|(coord, value)| (coord, *value))
            .collect::<Vec<_>>();
        let expected = expected.into_iter().collect::<Vec<_>>();

        assert_eq!(iterated, expected);
    }

    #[test]
    fn cell_rows_remove_matches_btreemap_and_drops_empty_rows() {
        let entries = [
            ((10_i64, 1_i64), 101_u32),
            ((10_i64, 4_i64), 104_u32),
            ((11_i64, 2_i64), 112_u32),
        ];
        let mut rows = CellRows::default();
        let mut expected = entries.into_iter().collect::<BTreeMap<_, _>>();

        for (coord, value) in entries {
            let _ = rows.insert(coord, value);
        }

        assert_eq!(rows.remove((10, 1)), expected.remove(&(10, 1)));
        assert_eq!(
            rows.iter()
                .map(|(coord, value)| (coord, *value))
                .collect::<Vec<_>>(),
            expected
                .iter()
                .map(|(&coord, &value)| (coord, value))
                .collect::<Vec<_>>()
        );

        assert_eq!(rows.remove((10, 4)), expected.remove(&(10, 4)));
        assert_eq!(rows.get((10, 4)), None);
        assert_eq!(
            rows.iter()
                .map(|(coord, value)| (coord, *value))
                .collect::<Vec<_>>(),
            expected
                .iter()
                .map(|(&coord, &value)| (coord, value))
                .collect::<Vec<_>>()
        );

        assert_eq!(rows.remove((11, 2)), expected.remove(&(11, 2)));
        assert!(rows.is_empty());
    }

    #[test]
    fn cell_rows_for_each_in_bounds_visits_only_requested_rectangle() {
        let mut rows = CellRows::default();
        for row in 8_i64..=12_i64 {
            for col in 98_i64..=102_i64 {
                let _ = rows.insert((row, col), row * 100 + col);
            }
        }

        let bounds = CellRect::new(9, 10, 99, 101);
        let mut visited = Vec::new();
        rows.for_each_in_bounds(bounds, |coord, value| visited.push((coord, *value)));

        let expected = (9_i64..=10_i64)
            .flat_map(|row| (99_i64..=101_i64).map(move |col| ((row, col), row * 100 + col)))
            .collect::<Vec<_>>();

        assert_eq!(visited, expected);
    }

    #[test]
    fn borrowed_cell_rows_for_each_in_bounds_visits_only_requested_rectangle() {
        let cells = (8_i64..=12_i64)
            .flat_map(|row| (98_i64..=102_i64).map(move |col| ((row, col), row * 100 + col)))
            .collect::<BTreeMap<_, _>>();
        let mut scratch = BorrowedCellRowsScratch::default();
        let rows = BorrowedCellRows::build(&cells, &mut scratch);
        let mut visited = Vec::new();

        let stats = rows.for_each_in_bounds(CellRect::new(9, 10, 99, 101), |coord, value| {
            visited.push((coord, *value))
        });

        let expected = (9_i64..=10_i64)
            .flat_map(|row| (99_i64..=101_i64).map(move |col| ((row, col), row * 100 + col)))
            .collect::<Vec<_>>();

        assert_eq!(visited, expected);
        assert_eq!(
            stats,
            CellRowQueryStats {
                bucket_maps_scanned: 2,
                bucket_cells_scanned: 10,
                local_query_cells: 6,
            }
        );
    }

    #[test]
    fn borrowed_cell_rows_scratch_reuses_index_storage() {
        let cells = BTreeMap::from([
            ((10_i64, 8_i64), 108_u32),
            ((10_i64, 9_i64), 109_u32),
            ((11_i64, 7_i64), 117_u32),
        ]);
        let mut scratch = BorrowedCellRowsScratch::default();

        let first_visited = {
            let first = BorrowedCellRows::build(&cells, &mut scratch);
            let mut visited = Vec::new();
            let _ = first.for_each_in_bounds(CellRect::new(10, 11, 7, 9), |coord, value| {
                visited.push((coord, *value))
            });
            visited
        };
        let first_rows_capacity = scratch.row_capacity();
        let first_entries_capacity = scratch.entry_capacity();

        let second_visited = {
            let second = BorrowedCellRows::build(&cells, &mut scratch);
            let mut visited = Vec::new();
            let _ = second.for_each_in_bounds(CellRect::new(10, 11, 7, 9), |coord, value| {
                visited.push((coord, *value))
            });
            visited
        };

        assert!(first_rows_capacity > 0);
        assert!(first_entries_capacity > 0);
        assert_eq!(scratch.row_capacity(), first_rows_capacity);
        assert_eq!(scratch.entry_capacity(), first_entries_capacity);
        assert_eq!(second_visited, first_visited);
    }

    #[test]
    fn borrowed_cell_rows_rebuilds_from_new_source_after_borrow_scope_ends() {
        let mut scratch = BorrowedCellRowsScratch::default();

        let first_visited = {
            let first_cells =
                BTreeMap::from([((10_i64, 8_i64), 108_u32), ((11_i64, 7_i64), 117_u32)]);
            let rows = BorrowedCellRows::build(&first_cells, &mut scratch);
            let mut visited = Vec::new();
            let _ = rows.for_each_in_bounds(CellRect::new(10, 11, 7, 8), |coord, value| {
                visited.push((coord, *value))
            });
            visited
        };

        let second_visited = {
            // The first source map and borrowed index both drop at the end of the block above, so
            // the same scratch storage can be reused for a different map without stale pointers.
            let second_cells = BTreeMap::from([
                ((9_i64, 9_i64), 99_u32),
                ((10_i64, 8_i64), 1008_u32),
                ((12_i64, 6_i64), 1206_u32),
            ]);
            let rows = BorrowedCellRows::build(&second_cells, &mut scratch);
            let mut visited = Vec::new();
            let _ = rows.for_each_in_bounds(CellRect::new(9, 12, 6, 9), |coord, value| {
                visited.push((coord, *value))
            });
            visited
        };

        assert_eq!(
            first_visited,
            vec![((10_i64, 8_i64), 108_u32), ((11_i64, 7_i64), 117_u32)]
        );
        assert_eq!(
            second_visited,
            vec![
                ((9_i64, 9_i64), 99_u32),
                ((10_i64, 8_i64), 1008_u32),
                ((12_i64, 6_i64), 1206_u32),
            ]
        );
    }
}
