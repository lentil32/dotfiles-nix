use crate::core::state::{CursorColorProbeWitness, CursorColorSample, CursorTextContext};
use crate::core::types::Generation;
use std::collections::VecDeque;
use std::sync::Arc;

const CURSOR_COLOR_CACHE_CAPACITY: usize = 16;
const CURSOR_TEXT_CONTEXT_CACHE_CAPACITY: usize = 32;
const CONCEAL_REGION_CACHE_CAPACITY: usize = 32;
const CONCEAL_SCREEN_CELL_CACHE_CAPACITY: usize = 128;

pub(super) type ConcealScreenCell = (i64, i64);

#[derive(Debug, Clone, Eq, PartialEq)]
struct LruCacheEntry<K, V> {
    key: K,
    value: V,
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct LruCache<K, V> {
    entries: VecDeque<LruCacheEntry<K, V>>,
    capacity: usize,
}

impl<K, V> LruCache<K, V> {
    fn new(capacity: usize) -> Self {
        debug_assert!(capacity > 0);
        Self {
            entries: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    fn clear(&mut self) {
        self.entries.clear();
    }
}

impl<K: Eq, V> LruCache<K, V> {
    fn take_entry(&mut self, key: &K) -> Option<LruCacheEntry<K, V>> {
        let existing_index = self.entries.iter().position(|entry| entry.key == *key)?;
        self.entries.remove(existing_index)
    }

    fn insert(&mut self, key: K, value: V) {
        let _ = self.take_entry(&key);
        self.entries.push_front(LruCacheEntry { key, value });
        while self.entries.len() > self.capacity {
            let _ = self.entries.pop_back();
        }
    }
}

impl<K: Eq, V: Clone> LruCache<K, V> {
    fn get_cloned(&mut self, key: &K) -> Option<V> {
        self.take_entry(key).map(|entry| {
            let value = entry.value.clone();
            self.entries.push_front(entry);
            value
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct ConcealRegion {
    pub(super) start_col1: i64,
    pub(super) end_col1: i64,
    pub(super) match_id: i64,
    pub(super) replacement_width: i64,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(super) struct ConcealCacheKey {
    buffer_handle: i64,
    changedtick: u64,
    line: usize,
}

impl ConcealCacheKey {
    pub(super) const fn new(buffer_handle: i64, changedtick: u64, line: usize) -> Self {
        Self {
            buffer_handle,
            changedtick,
            line,
        }
    }

    pub(super) const fn buffer_handle(&self) -> i64 {
        self.buffer_handle
    }

    pub(super) const fn changedtick(&self) -> u64 {
        self.changedtick
    }

    pub(super) const fn line(&self) -> usize {
        self.line
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(super) struct ConcealScreenCellCacheKey {
    window_handle: i64,
    buffer_handle: i64,
    changedtick: u64,
    line: usize,
    col1: i64,
    window_row: i64,
    window_col: i64,
    window_width: i64,
    window_height: i64,
    topline: i64,
    leftcol: i64,
    textoff: i64,
}

impl ConcealScreenCellCacheKey {
    #[allow(clippy::too_many_arguments)]
    pub(super) const fn new(
        window_handle: i64,
        buffer_handle: i64,
        changedtick: u64,
        line: usize,
        col1: i64,
        window_row: i64,
        window_col: i64,
        window_width: i64,
        window_height: i64,
        topline: i64,
        leftcol: i64,
        textoff: i64,
    ) -> Self {
        Self {
            window_handle,
            buffer_handle,
            changedtick,
            line,
            col1,
            window_row,
            window_col,
            window_width,
            window_height,
            topline,
            leftcol,
            textoff,
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(super) struct CursorTextContextCacheKey {
    buffer_handle: i64,
    changedtick: u64,
    cursor_line: i64,
    tracked_line: Option<i64>,
}

impl CursorTextContextCacheKey {
    pub(super) const fn new(
        buffer_handle: i64,
        changedtick: u64,
        cursor_line: i64,
        tracked_line: Option<i64>,
    ) -> Self {
        Self {
            buffer_handle,
            changedtick,
            cursor_line,
            tracked_line,
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(super) struct CachedConcealRegions {
    scanned_to_col1: i64,
    regions: Arc<[ConcealRegion]>,
}

impl CachedConcealRegions {
    pub(super) fn new(scanned_to_col1: i64, regions: Arc<[ConcealRegion]>) -> Self {
        Self {
            scanned_to_col1,
            regions,
        }
    }

    pub(super) const fn scanned_to_col1(&self) -> i64 {
        self.scanned_to_col1
    }

    pub(super) fn regions(&self) -> &Arc<[ConcealRegion]> {
        &self.regions
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(super) enum ConcealCacheLookup {
    Miss,
    Hit(CachedConcealRegions),
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(super) enum ConcealScreenCellCacheLookup {
    Miss,
    Hit(Option<ConcealScreenCell>),
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(super) enum CursorColorCacheLookup {
    Miss,
    Hit(Option<CursorColorSample>),
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(super) enum CursorTextContextCacheLookup {
    Miss,
    Hit(Option<CursorTextContext>),
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(super) struct ProbeCacheState {
    colorscheme_generation: Generation,
    cursor_color: LruCache<CursorColorProbeWitness, Option<CursorColorSample>>,
    cursor_text_context: LruCache<CursorTextContextCacheKey, Option<CursorTextContext>>,
    conceal_lines: LruCache<ConcealCacheKey, CachedConcealRegions>,
    conceal_screen_cells: LruCache<ConcealScreenCellCacheKey, Option<ConcealScreenCell>>,
}

impl Default for ProbeCacheState {
    fn default() -> Self {
        Self {
            colorscheme_generation: Generation::INITIAL,
            cursor_color: LruCache::new(CURSOR_COLOR_CACHE_CAPACITY),
            cursor_text_context: LruCache::new(CURSOR_TEXT_CONTEXT_CACHE_CAPACITY),
            conceal_lines: LruCache::new(CONCEAL_REGION_CACHE_CAPACITY),
            conceal_screen_cells: LruCache::new(CONCEAL_SCREEN_CELL_CACHE_CAPACITY),
        }
    }
}

impl ProbeCacheState {
    pub(super) fn colorscheme_generation(&self) -> Generation {
        self.colorscheme_generation
    }

    pub(super) fn cached_cursor_color_sample(
        &mut self,
        witness: &CursorColorProbeWitness,
    ) -> CursorColorCacheLookup {
        self.cursor_color
            .get_cloned(witness)
            .map_or(CursorColorCacheLookup::Miss, CursorColorCacheLookup::Hit)
    }

    pub(super) fn store_cursor_color_sample(
        &mut self,
        witness: CursorColorProbeWitness,
        sample: Option<CursorColorSample>,
    ) {
        self.cursor_color.insert(witness, sample);
    }

    pub(super) fn cached_cursor_text_context(
        &mut self,
        key: &CursorTextContextCacheKey,
    ) -> CursorTextContextCacheLookup {
        self.cursor_text_context.get_cloned(key).map_or(
            CursorTextContextCacheLookup::Miss,
            CursorTextContextCacheLookup::Hit,
        )
    }

    pub(super) fn store_cursor_text_context(
        &mut self,
        key: CursorTextContextCacheKey,
        context: Option<CursorTextContext>,
    ) {
        self.cursor_text_context.insert(key, context);
    }

    pub(super) fn cached_conceal_regions(&mut self, key: &ConcealCacheKey) -> ConcealCacheLookup {
        self.conceal_lines
            .get_cloned(key)
            .map_or(ConcealCacheLookup::Miss, ConcealCacheLookup::Hit)
    }

    pub(super) fn store_conceal_regions(
        &mut self,
        key: ConcealCacheKey,
        scanned_to_col1: i64,
        regions: Arc<[ConcealRegion]>,
    ) {
        self.conceal_lines
            .insert(key, CachedConcealRegions::new(scanned_to_col1, regions));
    }

    pub(super) fn cached_conceal_screen_cell(
        &mut self,
        key: &ConcealScreenCellCacheKey,
    ) -> ConcealScreenCellCacheLookup {
        self.conceal_screen_cells.get_cloned(key).map_or(
            ConcealScreenCellCacheLookup::Miss,
            ConcealScreenCellCacheLookup::Hit,
        )
    }

    pub(super) fn store_conceal_screen_cell(
        &mut self,
        key: ConcealScreenCellCacheKey,
        cell: Option<ConcealScreenCell>,
    ) {
        self.conceal_screen_cells.insert(key, cell);
    }

    pub(super) fn note_cursor_color_colorscheme_change(&mut self) {
        self.colorscheme_generation = self.colorscheme_generation.next();
        self.cursor_color.clear();
    }

    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CONCEAL_REGION_CACHE_CAPACITY, CONCEAL_SCREEN_CELL_CACHE_CAPACITY,
        CURSOR_COLOR_CACHE_CAPACITY, CURSOR_TEXT_CONTEXT_CACHE_CAPACITY, CachedConcealRegions,
        ConcealCacheKey, ConcealCacheLookup, ConcealRegion, ConcealScreenCellCacheKey,
        ConcealScreenCellCacheLookup, CursorColorCacheLookup, CursorTextContextCacheKey,
        CursorTextContextCacheLookup, ProbeCacheState,
    };
    use crate::core::state::{
        CursorColorProbeWitness, CursorColorSample, CursorTextContext, ObservedTextRow,
    };
    use crate::core::types::{CursorCol, CursorPosition, CursorRow, Generation};
    use std::sync::Arc;

    fn cursor(row: u32, col: u32) -> CursorPosition {
        CursorPosition {
            row: CursorRow(row),
            col: CursorCol(col),
        }
    }

    fn witness(
        buffer_handle: i64,
        changedtick: u64,
        mode: &str,
        cursor_position: Option<CursorPosition>,
        colorscheme_generation: u64,
    ) -> CursorColorProbeWitness {
        CursorColorProbeWitness::new(
            buffer_handle,
            changedtick,
            mode.to_string(),
            cursor_position,
            Generation::new(colorscheme_generation),
        )
    }

    fn conceal_key(buffer_handle: i64, changedtick: u64, line: usize) -> ConcealCacheKey {
        ConcealCacheKey::new(buffer_handle, changedtick, line)
    }

    fn cursor_text_context_key(
        buffer_handle: i64,
        changedtick: u64,
        cursor_line: i64,
        tracked_line: Option<i64>,
    ) -> CursorTextContextCacheKey {
        CursorTextContextCacheKey::new(buffer_handle, changedtick, cursor_line, tracked_line)
    }

    fn observed_row(line: i64, text: &str) -> ObservedTextRow {
        ObservedTextRow::new(line, text.to_string())
    }

    fn cursor_text_context(
        buffer_handle: i64,
        changedtick: u64,
        cursor_line: i64,
        nearby_rows: Vec<ObservedTextRow>,
        tracked_cursor_line: Option<i64>,
        tracked_nearby_rows: Option<Vec<ObservedTextRow>>,
    ) -> CursorTextContext {
        CursorTextContext::new(
            buffer_handle,
            changedtick,
            cursor_line,
            nearby_rows,
            tracked_cursor_line,
            tracked_nearby_rows,
        )
    }

    fn conceal_region(
        start_col1: i64,
        end_col1: i64,
        match_id: i64,
        replacement_width: i64,
    ) -> ConcealRegion {
        ConcealRegion {
            start_col1,
            end_col1,
            match_id,
            replacement_width,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn conceal_screen_cell_key(
        window_handle: i64,
        buffer_handle: i64,
        changedtick: u64,
        line: usize,
        col1: i64,
        window_row: i64,
        window_col: i64,
        window_width: i64,
        window_height: i64,
        topline: i64,
        leftcol: i64,
        textoff: i64,
    ) -> ConcealScreenCellCacheKey {
        ConcealScreenCellCacheKey::new(
            window_handle,
            buffer_handle,
            changedtick,
            line,
            col1,
            window_row,
            window_col,
            window_width,
            window_height,
            topline,
            leftcol,
            textoff,
        )
    }

    #[test]
    fn probe_cache_state_returns_cursor_color_entry_only_for_identical_witness() {
        let mut cache = ProbeCacheState::default();
        let key = witness(22, 14, "n", Some(cursor(7, 8)), 0);
        let sample = Some(CursorColorSample::new(0x00AB_CDEF));
        cache.store_cursor_color_sample(key.clone(), sample.clone());

        assert_eq!(
            cache.cached_cursor_color_sample(&key),
            CursorColorCacheLookup::Hit(sample),
        );

        let changedtick = witness(22, 15, "n", Some(cursor(7, 8)), 0);
        assert_eq!(
            cache.cached_cursor_color_sample(&changedtick),
            CursorColorCacheLookup::Miss,
        );

        let moved_cursor = witness(22, 14, "n", Some(cursor(7, 9)), 0);
        assert_eq!(
            cache.cached_cursor_color_sample(&moved_cursor),
            CursorColorCacheLookup::Miss,
        );
    }

    #[test]
    fn probe_cache_state_reuses_nearby_cursor_color_entries_without_blurring_the_key() {
        let mut cache = ProbeCacheState::default();
        let left = witness(22, 14, "n", Some(cursor(7, 8)), 0);
        let right = witness(22, 14, "n", Some(cursor(7, 9)), 0);
        let left_sample = Some(CursorColorSample::new(0x00AB_CDEF));
        let right_sample = Some(CursorColorSample::new(0x00FE_DCBA));

        cache.store_cursor_color_sample(left.clone(), left_sample.clone());
        cache.store_cursor_color_sample(right.clone(), right_sample.clone());

        assert_eq!(
            cache.cached_cursor_color_sample(&left),
            CursorColorCacheLookup::Hit(left_sample),
        );
        assert_eq!(
            cache.cached_cursor_color_sample(&right),
            CursorColorCacheLookup::Hit(right_sample),
        );
        assert_eq!(
            cache.cached_cursor_color_sample(&witness(22, 15, "n", Some(cursor(7, 8)), 0)),
            CursorColorCacheLookup::Miss,
        );
        assert_eq!(
            cache.cached_cursor_color_sample(&witness(22, 14, "i", Some(cursor(7, 8)), 0)),
            CursorColorCacheLookup::Miss,
        );
    }

    #[test]
    fn probe_cache_state_invalidates_cursor_color_entry_when_colorscheme_changes() {
        let mut cache = ProbeCacheState::default();
        cache.store_cursor_color_sample(
            witness(22, 14, "n", Some(cursor(7, 8)), 0),
            Some(CursorColorSample::new(0x00AB_CDEF)),
        );

        cache.note_cursor_color_colorscheme_change();

        assert_eq!(cache.colorscheme_generation(), Generation::new(1));
        assert_eq!(
            cache.cached_cursor_color_sample(&witness(22, 14, "n", Some(cursor(7, 8)), 1)),
            CursorColorCacheLookup::Miss,
        );
    }

    #[test]
    fn probe_cache_state_evicts_the_oldest_cursor_color_entry_when_capacity_is_exceeded() {
        let mut cache = ProbeCacheState::default();
        for col in 1..=CURSOR_COLOR_CACHE_CAPACITY as u32 {
            cache.store_cursor_color_sample(
                witness(22, 14, "n", Some(cursor(7, col)), 0),
                Some(CursorColorSample::new(col)),
            );
        }

        cache.store_cursor_color_sample(
            witness(22, 14, "n", Some(cursor(7, 99)), 0),
            Some(CursorColorSample::new(99)),
        );

        assert_eq!(
            cache.cached_cursor_color_sample(&witness(22, 14, "n", Some(cursor(7, 1)), 0)),
            CursorColorCacheLookup::Miss,
        );
        assert_eq!(
            cache.cached_cursor_color_sample(&witness(22, 14, "n", Some(cursor(7, 99)), 0)),
            CursorColorCacheLookup::Hit(Some(CursorColorSample::new(99))),
        );
    }

    #[test]
    fn probe_cache_state_promotes_cursor_color_hits_to_keep_recent_entries() {
        let mut cache = ProbeCacheState::default();
        for col in 1..=CURSOR_COLOR_CACHE_CAPACITY as u32 {
            cache.store_cursor_color_sample(
                witness(22, 14, "n", Some(cursor(7, col)), 0),
                Some(CursorColorSample::new(col)),
            );
        }

        assert_eq!(
            cache.cached_cursor_color_sample(&witness(22, 14, "n", Some(cursor(7, 1)), 0)),
            CursorColorCacheLookup::Hit(Some(CursorColorSample::new(1))),
        );

        cache.store_cursor_color_sample(
            witness(22, 14, "n", Some(cursor(7, 99)), 0),
            Some(CursorColorSample::new(99)),
        );

        assert_eq!(
            cache.cached_cursor_color_sample(&witness(22, 14, "n", Some(cursor(7, 1)), 0)),
            CursorColorCacheLookup::Hit(Some(CursorColorSample::new(1))),
        );
        assert_eq!(
            cache.cached_cursor_color_sample(&witness(22, 14, "n", Some(cursor(7, 2)), 0)),
            CursorColorCacheLookup::Miss,
        );
    }

    #[test]
    fn probe_cache_state_reuses_cursor_text_context_entry_only_for_identical_key() {
        let mut cache = ProbeCacheState::default();
        let key = cursor_text_context_key(22, 14, 7, Some(5));
        let context = Some(cursor_text_context(
            22,
            14,
            7,
            vec![observed_row(6, "before"), observed_row(7, "current")],
            Some(5),
            Some(vec![
                observed_row(4, "tracked before"),
                observed_row(5, "tracked"),
            ]),
        ));
        cache.store_cursor_text_context(key.clone(), context.clone());

        assert_eq!(
            cache.cached_cursor_text_context(&key),
            CursorTextContextCacheLookup::Hit(context),
        );
        assert_eq!(
            cache.cached_cursor_text_context(&cursor_text_context_key(22, 15, 7, Some(5))),
            CursorTextContextCacheLookup::Miss,
        );
        assert_eq!(
            cache.cached_cursor_text_context(&cursor_text_context_key(22, 14, 7, None)),
            CursorTextContextCacheLookup::Miss,
        );
    }

    #[test]
    fn probe_cache_state_evicts_oldest_cursor_text_context_entry_when_capacity_is_exceeded() {
        let mut cache = ProbeCacheState::default();
        for cursor_line in 1..=CURSOR_TEXT_CONTEXT_CACHE_CAPACITY as i64 {
            cache.store_cursor_text_context(
                cursor_text_context_key(22, 14, cursor_line, None),
                Some(cursor_text_context(
                    22,
                    14,
                    cursor_line,
                    vec![observed_row(cursor_line, "line")],
                    None,
                    None,
                )),
            );
        }

        cache.store_cursor_text_context(
            cursor_text_context_key(22, 14, 99, None),
            Some(cursor_text_context(
                22,
                14,
                99,
                vec![observed_row(99, "newest")],
                None,
                None,
            )),
        );

        assert_eq!(
            cache.cached_cursor_text_context(&cursor_text_context_key(22, 14, 1, None)),
            CursorTextContextCacheLookup::Miss,
        );
        assert_eq!(
            cache.cached_cursor_text_context(&cursor_text_context_key(22, 14, 99, None)),
            CursorTextContextCacheLookup::Hit(Some(cursor_text_context(
                22,
                14,
                99,
                vec![observed_row(99, "newest")],
                None,
                None,
            ))),
        );
    }

    #[test]
    fn probe_cache_state_returns_conceal_regions_only_for_identical_key() {
        let mut cache = ProbeCacheState::default();
        let key = conceal_key(22, 14, 7);
        let regions: Arc<[ConcealRegion]> =
            vec![conceal_region(2, 4, 11, 1), conceal_region(7, 7, 19, 0)].into();
        cache.store_conceal_regions(key.clone(), 18, Arc::clone(&regions));

        assert_eq!(
            cache.cached_conceal_regions(&key),
            ConcealCacheLookup::Hit(CachedConcealRegions::new(18, regions)),
        );

        assert_eq!(
            cache.cached_conceal_regions(&conceal_key(22, 15, 7)),
            ConcealCacheLookup::Miss,
        );
        assert_eq!(
            cache.cached_conceal_regions(&conceal_key(22, 14, 8)),
            ConcealCacheLookup::Miss,
        );
        assert_eq!(
            cache.cached_conceal_regions(&conceal_key(23, 14, 7)),
            ConcealCacheLookup::Miss,
        );
    }

    #[test]
    fn probe_cache_state_keeps_multiple_conceal_lines_hot() {
        let mut cache = ProbeCacheState::default();
        let first_key = conceal_key(22, 14, 7);
        let second_key = conceal_key(22, 14, 8);
        let first_regions: Arc<[ConcealRegion]> = vec![conceal_region(2, 4, 11, 1)].into();
        let second_regions: Arc<[ConcealRegion]> = vec![conceal_region(5, 6, 17, 0)].into();

        cache.store_conceal_regions(first_key.clone(), 12, Arc::clone(&first_regions));
        cache.store_conceal_regions(second_key.clone(), 9, Arc::clone(&second_regions));

        assert_eq!(
            cache.cached_conceal_regions(&first_key),
            ConcealCacheLookup::Hit(CachedConcealRegions::new(12, first_regions)),
        );
        assert_eq!(
            cache.cached_conceal_regions(&second_key),
            ConcealCacheLookup::Hit(CachedConcealRegions::new(9, second_regions)),
        );
    }

    #[test]
    fn probe_cache_state_evicts_the_oldest_conceal_entry_when_capacity_is_exceeded() {
        let mut cache = ProbeCacheState::default();
        for line in 1..=CONCEAL_REGION_CACHE_CAPACITY {
            cache.store_conceal_regions(
                conceal_key(22, 14, line),
                12,
                vec![conceal_region(2, 4, line as i64, 1)].into(),
            );
        }

        cache.store_conceal_regions(
            conceal_key(22, 14, 99),
            12,
            vec![conceal_region(8, 9, 99, 0)].into(),
        );

        assert_eq!(
            cache.cached_conceal_regions(&conceal_key(22, 14, 1)),
            ConcealCacheLookup::Miss,
        );
        assert_eq!(
            cache.cached_conceal_regions(&conceal_key(22, 14, 99)),
            ConcealCacheLookup::Hit(CachedConcealRegions::new(
                12,
                vec![conceal_region(8, 9, 99, 0)].into(),
            )),
        );
    }

    #[test]
    fn probe_cache_state_promotes_conceal_hits_to_keep_recent_entries() {
        let mut cache = ProbeCacheState::default();
        for line in 1..=CONCEAL_REGION_CACHE_CAPACITY {
            cache.store_conceal_regions(
                conceal_key(22, 14, line),
                12,
                vec![conceal_region(2, 4, line as i64, 1)].into(),
            );
        }

        assert_eq!(
            cache.cached_conceal_regions(&conceal_key(22, 14, 1)),
            ConcealCacheLookup::Hit(CachedConcealRegions::new(
                12,
                vec![conceal_region(2, 4, 1, 1)].into(),
            )),
        );

        cache.store_conceal_regions(
            conceal_key(22, 14, 99),
            12,
            vec![conceal_region(8, 9, 99, 0)].into(),
        );

        assert_eq!(
            cache.cached_conceal_regions(&conceal_key(22, 14, 1)),
            ConcealCacheLookup::Hit(CachedConcealRegions::new(
                12,
                vec![conceal_region(2, 4, 1, 1)].into(),
            )),
        );
        assert_eq!(
            cache.cached_conceal_regions(&conceal_key(22, 14, 2)),
            ConcealCacheLookup::Miss,
        );
    }

    #[test]
    fn probe_cache_state_returns_screen_cell_entries_only_for_identical_witness() {
        let mut cache = ProbeCacheState::default();
        let key = conceal_screen_cell_key(8, 22, 14, 7, 5, 2, 3, 120, 40, 11, 0, 4);
        cache.store_conceal_screen_cell(key.clone(), Some((19, 33)));

        assert_eq!(
            cache.cached_conceal_screen_cell(&key),
            ConcealScreenCellCacheLookup::Hit(Some((19, 33))),
        );
        assert_eq!(
            cache.cached_conceal_screen_cell(&conceal_screen_cell_key(
                8, 22, 14, 7, 5, 2, 3, 120, 40, 12, 0, 4
            )),
            ConcealScreenCellCacheLookup::Miss,
        );
        assert_eq!(
            cache.cached_conceal_screen_cell(&conceal_screen_cell_key(
                9, 22, 14, 7, 5, 2, 3, 120, 40, 11, 0, 4
            )),
            ConcealScreenCellCacheLookup::Miss,
        );
    }

    #[test]
    fn probe_cache_state_promotes_screen_cell_hits_to_keep_recent_entries() {
        let mut cache = ProbeCacheState::default();
        for col1 in 1..=CONCEAL_SCREEN_CELL_CACHE_CAPACITY as i64 {
            cache.store_conceal_screen_cell(
                conceal_screen_cell_key(8, 22, 14, 7, col1, 2, 3, 120, 40, 11, 0, 4),
                Some((19, col1)),
            );
        }

        assert_eq!(
            cache.cached_conceal_screen_cell(&conceal_screen_cell_key(
                8, 22, 14, 7, 1, 2, 3, 120, 40, 11, 0, 4
            )),
            ConcealScreenCellCacheLookup::Hit(Some((19, 1))),
        );

        cache.store_conceal_screen_cell(
            conceal_screen_cell_key(8, 22, 14, 7, 999, 2, 3, 120, 40, 11, 0, 4),
            Some((19, 999)),
        );

        assert_eq!(
            cache.cached_conceal_screen_cell(&conceal_screen_cell_key(
                8, 22, 14, 7, 1, 2, 3, 120, 40, 11, 0, 4
            )),
            ConcealScreenCellCacheLookup::Hit(Some((19, 1))),
        );
        assert_eq!(
            cache.cached_conceal_screen_cell(&conceal_screen_cell_key(
                8, 22, 14, 7, 2, 2, 3, 120, 40, 11, 0, 4
            )),
            ConcealScreenCellCacheLookup::Miss,
        );
    }

    #[test]
    fn probe_cache_state_reset_clears_generation_and_entries() {
        let mut cache = ProbeCacheState::default();
        cache.store_cursor_color_sample(
            witness(22, 14, "i", Some(cursor(5, 6)), 0),
            Some(CursorColorSample::new(0x00FE_DCBA)),
        );
        cache.store_conceal_regions(
            conceal_key(22, 14, 7),
            12,
            vec![conceal_region(2, 4, 11, 1)].into(),
        );
        cache.store_conceal_screen_cell(
            conceal_screen_cell_key(8, 22, 14, 7, 5, 2, 3, 120, 40, 11, 0, 4),
            Some((19, 33)),
        );
        cache.note_cursor_color_colorscheme_change();

        cache.reset();

        assert_eq!(cache.colorscheme_generation(), Generation::INITIAL);
        assert_eq!(
            cache.cached_cursor_color_sample(&witness(22, 14, "i", Some(cursor(5, 6)), 0)),
            CursorColorCacheLookup::Miss,
        );
        assert_eq!(
            cache.cached_conceal_regions(&conceal_key(22, 14, 7)),
            ConcealCacheLookup::Miss,
        );
        assert_eq!(
            cache.cached_conceal_screen_cell(&conceal_screen_cell_key(
                8, 22, 14, 7, 5, 2, 3, 120, 40, 11, 0, 4
            )),
            ConcealScreenCellCacheLookup::Miss,
        );
    }
}
