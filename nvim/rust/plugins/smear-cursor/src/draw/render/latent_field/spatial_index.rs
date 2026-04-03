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

    #[cfg(test)]
    pub(in super::super) fn get_mut(&mut self, coord: (i64, i64)) -> Option<&mut T> {
        self.rows.get_mut(&coord.0)?.get_mut(&coord.1)
    }

    #[cfg(test)]
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
    use crate::test_support::proptest::pure_config;
    use proptest::collection::vec;
    use proptest::prelude::*;
    use std::collections::BTreeMap;

    type Coord = (i64, i64);
    type CellMap = BTreeMap<Coord, u16>;

    fn coord() -> impl Strategy<Value = Coord> {
        (-8_i64..=8_i64, -8_i64..=8_i64)
    }

    fn cell_entries(max_len: usize) -> BoxedStrategy<Vec<(Coord, u16)>> {
        vec((coord(), any::<u16>()), 0..=max_len).boxed()
    }

    fn cell_rect() -> BoxedStrategy<CellRect> {
        (
            -9_i64..=9_i64,
            -9_i64..=9_i64,
            -9_i64..=9_i64,
            -9_i64..=9_i64,
        )
            .prop_map(|(row_a, row_b, col_a, col_b)| {
                CellRect::new(
                    row_a.min(row_b),
                    row_a.max(row_b),
                    col_a.min(col_b),
                    col_a.max(col_b),
                )
            })
            .boxed()
    }

    fn cell_map(entries: &[(Coord, u16)]) -> CellMap {
        entries.iter().copied().collect()
    }

    fn cell_rows(entries: &[(Coord, u16)]) -> CellRows<u16> {
        let mut rows = CellRows::default();
        for &(coord, value) in entries {
            let _ = rows.insert(coord, value);
        }
        rows
    }

    fn expected_entries(cells: &CellMap) -> Vec<(Coord, u16)> {
        cells
            .iter()
            .map(|(&coord, &value)| (coord, value))
            .collect()
    }

    fn expected_query(cells: &CellMap, bounds: CellRect) -> (Vec<(Coord, u16)>, CellRowQueryStats) {
        let mut rows = BTreeMap::<i64, BTreeMap<i64, u16>>::new();
        for (&(row, col), &value) in cells {
            let _ = rows.entry(row).or_default().insert(col, value);
        }

        let mut visited = Vec::new();
        let mut stats = CellRowQueryStats::default();
        for (&row, cols) in rows.range(bounds.min_row..=bounds.max_row) {
            stats.bucket_maps_scanned = stats.bucket_maps_scanned.saturating_add(1);
            stats.bucket_cells_scanned = stats.bucket_cells_scanned.saturating_add(cols.len());
            for (&col, &value) in cols.range(bounds.min_col..=bounds.max_col) {
                stats.local_query_cells = stats.local_query_cells.saturating_add(1);
                visited.push(((row, col), value));
            }
        }

        (visited, stats)
    }

    fn query_cell_rows(
        rows: &CellRows<u16>,
        bounds: CellRect,
    ) -> (Vec<(Coord, u16)>, CellRowQueryStats) {
        let mut visited = Vec::new();
        let stats = rows.for_each_in_bounds(bounds, |coord, value| visited.push((coord, *value)));
        (visited, stats)
    }

    fn query_borrowed_cell_rows(
        cells: &CellMap,
        scratch: &mut BorrowedCellRowsScratch<u16>,
        bounds: CellRect,
    ) -> (Vec<(Coord, u16)>, CellRowQueryStats, usize, usize) {
        let (visited, stats) = {
            let rows = BorrowedCellRows::build(cells, scratch);
            let mut visited = Vec::new();
            let stats =
                rows.for_each_in_bounds(bounds, |coord, value| visited.push((coord, *value)));
            (visited, stats)
        };

        (
            visited,
            stats,
            scratch.row_capacity(),
            scratch.entry_capacity(),
        )
    }

    proptest! {
        #![proptest_config(pure_config())]

        #[test]
        fn prop_cell_rows_preserve_deterministic_row_major_iteration_order(
            entries in cell_entries(48),
        ) {
            let expected = cell_map(&entries);
            let rows = cell_rows(&entries);

            prop_assert_eq!(rows.len(), expected.len());
            prop_assert_eq!(rows.is_empty(), expected.is_empty());
            prop_assert_eq!(rows.iter().map(|(coord, value)| (coord, *value)).collect::<Vec<_>>(), expected_entries(&expected));

            let mut visited = Vec::new();
            rows.for_each(|coord, value| visited.push((coord, *value)));
            prop_assert_eq!(visited, expected_entries(&expected));
        }

        #[test]
        fn prop_cell_rows_remove_matches_btreemap_and_drops_empty_rows(
            entries in cell_entries(48),
            removals in vec(coord(), 0..=64),
        ) {
            let mut rows = cell_rows(&entries);
            let mut expected = cell_map(&entries);

            for coord in removals {
                prop_assert_eq!(rows.remove(coord), expected.remove(&coord));
                prop_assert_eq!(rows.get_mut(coord).map(|value| *value), expected.get(&coord).copied());
                prop_assert_eq!(rows.len(), expected.len());
                prop_assert_eq!(rows.is_empty(), expected.is_empty());
                prop_assert_eq!(
                    rows.iter().map(|(entry_coord, value)| (entry_coord, *value)).collect::<Vec<_>>(),
                    expected_entries(&expected),
                );
            }
        }

        #[test]
        fn prop_rectangle_queries_match_reference_for_owned_and_borrowed_indexes(
            entries in cell_entries(48),
            bounds in cell_rect(),
        ) {
            let cells = cell_map(&entries);
            let rows = cell_rows(&entries);
            let (expected_visited, expected_stats) = expected_query(&cells, bounds);
            let iter_in_bounds = rows
                .iter_in_bounds(bounds)
                .map(|(coord, value)| (coord, *value))
                .collect::<Vec<_>>();
            let (owned_visited, owned_stats) = query_cell_rows(&rows, bounds);

            prop_assert_eq!(&iter_in_bounds, &expected_visited);
            prop_assert_eq!(&owned_visited, &expected_visited);
            prop_assert_eq!(&owned_stats, &expected_stats);

            let mut scratch = BorrowedCellRowsScratch::default();
            let (borrowed_visited, borrowed_stats, _, _) =
                query_borrowed_cell_rows(&cells, &mut scratch, bounds);
            let borrowed_is_empty = {
                let borrowed = BorrowedCellRows::build(&cells, &mut scratch);
                borrowed.is_empty()
            };

            prop_assert_eq!(borrowed_is_empty, cells.is_empty());
            prop_assert_eq!(&borrowed_visited, &expected_visited);
            prop_assert_eq!(&borrowed_stats, &expected_stats);
        }

        #[test]
        fn prop_borrowed_cell_rows_scratch_reuse_does_not_drift_output(
            first_entries in cell_entries(48),
            second_entries in cell_entries(48),
            first_bounds in cell_rect(),
            second_bounds in cell_rect(),
        ) {
            let first_cells = cell_map(&first_entries);
            let second_cells = cell_map(&second_entries);
            let expected_first = expected_query(&first_cells, first_bounds);
            let expected_second = expected_query(&second_cells, second_bounds);
            let mut scratch = BorrowedCellRowsScratch::default();

            let first =
                query_borrowed_cell_rows(&first_cells, &mut scratch, first_bounds);
            let repeated =
                query_borrowed_cell_rows(&first_cells, &mut scratch, first_bounds);
            let rebuilt =
                query_borrowed_cell_rows(&second_cells, &mut scratch, second_bounds);

            prop_assert_eq!(&first.0, &expected_first.0);
            prop_assert_eq!(&first.1, &expected_first.1);
            prop_assert_eq!(&repeated.0, &first.0);
            prop_assert_eq!(&repeated.1, &first.1);
            prop_assert_eq!(repeated.2, first.2);
            prop_assert_eq!(repeated.3, first.3);
            prop_assert_eq!(&rebuilt.0, &expected_second.0);
            prop_assert_eq!(&rebuilt.1, &expected_second.1);
        }
    }
}
