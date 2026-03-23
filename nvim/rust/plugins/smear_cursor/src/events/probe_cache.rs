use crate::core::state::{CursorColorProbeWitness, CursorColorSample};
use crate::core::types::Generation;
use std::collections::VecDeque;
use std::sync::Arc;

const CURSOR_COLOR_CACHE_CAPACITY: usize = 4;

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
pub(super) enum CursorColorCacheLookup {
    Miss,
    Hit(Option<CursorColorSample>),
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct CursorColorProbeCacheEntry {
    witness: CursorColorProbeWitness,
    sample: Option<CursorColorSample>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct ConcealRegionCacheEntry {
    key: ConcealCacheKey,
    cached: CachedConcealRegions,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(super) struct ProbeCacheState {
    colorscheme_generation: Generation,
    cursor_color: VecDeque<CursorColorProbeCacheEntry>,
    conceal_line: Option<ConcealRegionCacheEntry>,
}

impl Default for ProbeCacheState {
    fn default() -> Self {
        Self {
            colorscheme_generation: Generation::INITIAL,
            cursor_color: VecDeque::with_capacity(CURSOR_COLOR_CACHE_CAPACITY),
            conceal_line: None,
        }
    }
}

impl ProbeCacheState {
    pub(super) fn colorscheme_generation(&self) -> Generation {
        self.colorscheme_generation
    }

    pub(super) fn cached_cursor_color_sample(
        &self,
        witness: &CursorColorProbeWitness,
    ) -> CursorColorCacheLookup {
        self.cursor_color
            .iter()
            .filter(|entry| entry.witness == *witness)
            .next()
            .map_or(CursorColorCacheLookup::Miss, |entry| {
                CursorColorCacheLookup::Hit(entry.sample.clone())
            })
    }

    pub(super) fn store_cursor_color_sample(
        &mut self,
        witness: CursorColorProbeWitness,
        sample: Option<CursorColorSample>,
    ) {
        if let Some(existing_index) = self
            .cursor_color
            .iter()
            .position(|entry| entry.witness == witness)
        {
            let _ = self.cursor_color.remove(existing_index);
        }

        self.cursor_color
            .push_front(CursorColorProbeCacheEntry { witness, sample });
        while self.cursor_color.len() > CURSOR_COLOR_CACHE_CAPACITY {
            let _ = self.cursor_color.pop_back();
        }
    }

    pub(super) fn cached_conceal_regions(&self, key: &ConcealCacheKey) -> ConcealCacheLookup {
        self.conceal_line
            .as_ref()
            .filter(|entry| entry.key == *key)
            .map_or(ConcealCacheLookup::Miss, |entry| {
                ConcealCacheLookup::Hit(entry.cached.clone())
            })
    }

    pub(super) fn store_conceal_regions(
        &mut self,
        key: ConcealCacheKey,
        scanned_to_col1: i64,
        regions: Arc<[ConcealRegion]>,
    ) {
        self.conceal_line = Some(ConcealRegionCacheEntry {
            key,
            cached: CachedConcealRegions::new(scanned_to_col1, regions),
        });
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
        CachedConcealRegions, ConcealCacheKey, ConcealCacheLookup, ConcealRegion,
        CURSOR_COLOR_CACHE_CAPACITY, CursorColorCacheLookup, ProbeCacheState,
    };
    use crate::core::state::{CursorColorProbeWitness, CursorColorSample};
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

    #[test]
    fn probe_cache_state_returns_cursor_color_entry_only_for_identical_witness() {
        let mut cache = ProbeCacheState::default();
        let key = witness(22, 14, "n", Some(cursor(7, 8)), 0);
        let sample = Some(CursorColorSample::new("#abcdef".to_string()));
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
        let left_sample = Some(CursorColorSample::new("#abcdef".to_string()));
        let right_sample = Some(CursorColorSample::new("#fedcba".to_string()));

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
            Some(CursorColorSample::new("#abcdef".to_string())),
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
                Some(CursorColorSample::new(format!("#{col:06x}"))),
            );
        }

        cache.store_cursor_color_sample(
            witness(22, 14, "n", Some(cursor(7, 99)), 0),
            Some(CursorColorSample::new("#999999".to_string())),
        );

        assert_eq!(
            cache.cached_cursor_color_sample(&witness(22, 14, "n", Some(cursor(7, 1)), 0)),
            CursorColorCacheLookup::Miss,
        );
        assert_eq!(
            cache.cached_cursor_color_sample(&witness(22, 14, "n", Some(cursor(7, 99)), 0)),
            CursorColorCacheLookup::Hit(Some(CursorColorSample::new("#999999".to_string()))),
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
    fn probe_cache_state_reset_clears_generation_and_entries() {
        let mut cache = ProbeCacheState::default();
        cache.store_cursor_color_sample(
            witness(22, 14, "i", Some(cursor(5, 6)), 0),
            Some(CursorColorSample::new("#fedcba".to_string())),
        );
        cache.store_conceal_regions(
            conceal_key(22, 14, 7),
            12,
            vec![conceal_region(2, 4, 11, 1)].into(),
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
    }
}
