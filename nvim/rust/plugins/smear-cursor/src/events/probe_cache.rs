use super::lru_cache::LruCache;
use crate::core::effect::ProbePolicy;
use crate::core::state::CursorColorProbeWitness;
use crate::core::state::CursorColorSample;
use crate::core::state::CursorTextContext;
use crate::core::state::ProbeReuse;
use crate::core::types::Generation;
use crate::position::ScreenCell;
use crate::position::WindowSurfaceSnapshot;
use std::sync::Arc;

const CURSOR_TEXT_CONTEXT_CACHE_CAPACITY: usize = 32;
const CONCEAL_REGION_CACHE_CAPACITY: usize = 32;
const CONCEAL_DELTA_CACHE_CAPACITY: usize = 32;
const CONCEAL_SCREEN_CELL_CACHE_CAPACITY: usize = 128;

mod cursor_color;

pub(super) use cursor_color::CachedCursorColorProbeSample;
#[cfg(test)]
pub(super) use cursor_color::CursorColorCacheLookup;
pub(super) use cursor_color::CursorColorProbeCache;

pub(super) type ConcealScreenCell = ScreenCell;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ConcealRegion {
    pub(crate) start_col1: i64,
    pub(crate) end_col1: i64,
    pub(crate) match_id: i64,
    pub(crate) replacement_width: i64,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub(crate) struct ConcealWindowState {
    conceallevel: i64,
    concealcursor: String,
}

impl ConcealWindowState {
    pub(crate) fn new(conceallevel: i64, concealcursor: impl Into<String>) -> Self {
        Self {
            conceallevel,
            concealcursor: concealcursor.into(),
        }
    }

    pub(super) const fn conceallevel(&self) -> i64 {
        self.conceallevel
    }

    pub(super) fn concealcursor(&self) -> &str {
        &self.concealcursor
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub(crate) struct ConcealCacheKey {
    buffer_handle: i64,
    text_revision: u64,
    line: usize,
    window_state: ConcealWindowState,
}

impl ConcealCacheKey {
    pub(crate) fn new(
        buffer_handle: i64,
        text_revision: u64,
        line: usize,
        window_state: ConcealWindowState,
    ) -> Self {
        Self {
            buffer_handle,
            text_revision,
            line,
            window_state,
        }
    }

    pub(super) const fn buffer_handle(&self) -> i64 {
        self.buffer_handle
    }

    pub(super) const fn text_revision(&self) -> u64 {
        self.text_revision
    }

    pub(super) const fn line(&self) -> usize {
        self.line
    }

    pub(super) fn window_state(&self) -> &ConcealWindowState {
        &self.window_state
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub(super) struct ConcealScreenCellCacheKey {
    window_handle: i64,
    buffer_handle: i64,
    text_revision: u64,
    line: usize,
    col1: i64,
    window_row: i64,
    window_col: i64,
    window_width: i64,
    window_height: i64,
    topline: i64,
    leftcol: i64,
    textoff: i64,
    window_state: ConcealWindowState,
}

impl ConcealScreenCellCacheKey {
    pub(super) fn from_surface(
        conceal_key: &ConcealCacheKey,
        surface_snapshot: WindowSurfaceSnapshot,
        col1: i64,
    ) -> Self {
        let surface_id = surface_snapshot.id();
        debug_assert_eq!(
            conceal_key.buffer_handle(),
            surface_id.buffer_handle(),
            "conceal cache keys must use a surface snapshot from the same buffer"
        );
        let (window_row, window_col, window_width, window_height, topline, leftcol, textoff) =
            surface_cache_fields(surface_snapshot);
        Self {
            window_handle: surface_id.window_handle(),
            buffer_handle: conceal_key.buffer_handle(),
            text_revision: conceal_key.text_revision(),
            line: conceal_key.line(),
            col1,
            window_row,
            window_col,
            window_width,
            window_height,
            topline,
            leftcol,
            textoff,
            window_state: conceal_key.window_state().clone(),
        }
    }

    pub(super) const fn buffer_handle(&self) -> i64 {
        self.buffer_handle
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub(super) struct ConcealDeltaCacheKey {
    window_handle: i64,
    buffer_handle: i64,
    text_revision: u64,
    line: usize,
    window_row: i64,
    window_col: i64,
    window_width: i64,
    window_height: i64,
    topline: i64,
    leftcol: i64,
    textoff: i64,
    window_state: ConcealWindowState,
}

impl ConcealDeltaCacheKey {
    pub(super) fn from_surface(
        conceal_key: &ConcealCacheKey,
        surface_snapshot: WindowSurfaceSnapshot,
    ) -> Self {
        let surface_id = surface_snapshot.id();
        debug_assert_eq!(
            conceal_key.buffer_handle(),
            surface_id.buffer_handle(),
            "conceal cache keys must use a surface snapshot from the same buffer"
        );
        let (window_row, window_col, window_width, window_height, topline, leftcol, textoff) =
            surface_cache_fields(surface_snapshot);
        Self {
            window_handle: surface_id.window_handle(),
            buffer_handle: conceal_key.buffer_handle(),
            text_revision: conceal_key.text_revision(),
            line: conceal_key.line(),
            window_row,
            window_col,
            window_width,
            window_height,
            topline,
            leftcol,
            textoff,
            window_state: conceal_key.window_state().clone(),
        }
    }

    pub(super) const fn buffer_handle(&self) -> i64 {
        self.buffer_handle
    }
}

fn surface_cache_fields(
    surface_snapshot: WindowSurfaceSnapshot,
) -> (i64, i64, i64, i64, i64, i64, i64) {
    (
        surface_snapshot.window_origin().row(),
        surface_snapshot.window_origin().col(),
        surface_snapshot.window_size().max_col(),
        surface_snapshot.window_size().max_row(),
        surface_snapshot.top_buffer_line().value(),
        i64::from(surface_snapshot.left_col0()),
        i64::from(surface_snapshot.text_offset0()),
    )
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
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

    pub(super) const fn buffer_handle(&self) -> i64 {
        self.buffer_handle
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

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(super) struct CachedConcealDelta {
    current_col1: i64,
    delta: i64,
}

impl CachedConcealDelta {
    pub(super) const fn new(current_col1: i64, delta: i64) -> Self {
        Self {
            current_col1,
            delta,
        }
    }

    pub(super) const fn current_col1(&self) -> i64 {
        self.current_col1
    }

    pub(super) const fn delta(&self) -> i64 {
        self.delta
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(super) enum ConcealDeltaCacheLookup {
    Miss,
    Hit(CachedConcealDelta),
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(super) enum CursorTextContextCacheLookup {
    Miss,
    Hit(Option<CursorTextContext>),
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(super) struct ProbeCacheState {
    colorscheme_generation: Generation,
    cursor_color_cache_generation: Generation,
    cursor_color: CursorColorProbeCache,
    cursor_text_context: LruCache<CursorTextContextCacheKey, Option<CursorTextContext>>,
    conceal_lines: LruCache<ConcealCacheKey, CachedConcealRegions>,
    conceal_deltas: LruCache<ConcealDeltaCacheKey, CachedConcealDelta>,
    conceal_screen_cells: LruCache<ConcealScreenCellCacheKey, Option<ConcealScreenCell>>,
}

impl Default for ProbeCacheState {
    fn default() -> Self {
        Self {
            colorscheme_generation: Generation::INITIAL,
            cursor_color_cache_generation: Generation::INITIAL,
            cursor_color: CursorColorProbeCache::default(),
            cursor_text_context: LruCache::new(CURSOR_TEXT_CONTEXT_CACHE_CAPACITY),
            conceal_lines: LruCache::new(CONCEAL_REGION_CACHE_CAPACITY),
            conceal_deltas: LruCache::new(CONCEAL_DELTA_CACHE_CAPACITY),
            conceal_screen_cells: LruCache::new(CONCEAL_SCREEN_CELL_CACHE_CAPACITY),
        }
    }
}

impl ProbeCacheState {
    pub(super) fn colorscheme_generation(&self) -> Generation {
        self.colorscheme_generation
    }

    pub(super) fn cursor_color_cache_generation(&self) -> Generation {
        self.cursor_color_cache_generation
    }

    #[cfg(test)]
    pub(super) fn cached_cursor_color_sample(
        &mut self,
        witness: &CursorColorProbeWitness,
    ) -> CursorColorCacheLookup {
        self.cursor_color.cached_sample(witness)
    }

    pub(super) fn cached_cursor_color_sample_for_probe(
        &mut self,
        witness: &CursorColorProbeWitness,
        probe_policy: ProbePolicy,
        reuse: ProbeReuse,
    ) -> Option<CachedCursorColorProbeSample> {
        self.cursor_color
            .cached_sample_for_probe(witness, probe_policy, reuse)
    }

    pub(super) fn store_cursor_color_sample(
        &mut self,
        witness: CursorColorProbeWitness,
        sample: Option<CursorColorSample>,
    ) {
        self.cursor_color.store_sample(witness, sample);
    }

    pub(super) fn note_cursor_color_observation_boundary(&mut self) {
        self.cursor_color_cache_generation = self.cursor_color_cache_generation.next();
        self.cursor_color.clear();
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

    pub(super) fn invalidate_buffer(&mut self, buffer_handle: i64) {
        self.cursor_text_context
            .remove_where(|key, _| key.buffer_handle() == buffer_handle);
        self.invalidate_conceal_buffer(buffer_handle);
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
        self.conceal_screen_cells.get_copy(key).map_or(
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

    pub(super) fn cached_conceal_delta(
        &mut self,
        key: &ConcealDeltaCacheKey,
    ) -> ConcealDeltaCacheLookup {
        self.conceal_deltas
            .get_copy(key)
            .map_or(ConcealDeltaCacheLookup::Miss, ConcealDeltaCacheLookup::Hit)
    }

    pub(super) fn store_conceal_delta(
        &mut self,
        key: ConcealDeltaCacheKey,
        current_col1: i64,
        delta: i64,
    ) {
        self.conceal_deltas
            .insert(key, CachedConcealDelta::new(current_col1, delta));
    }

    pub(super) fn invalidate_conceal_buffer(&mut self, buffer_handle: i64) {
        self.conceal_lines
            .remove_where(|key, _| key.buffer_handle() == buffer_handle);
        self.conceal_deltas
            .remove_where(|key, _| key.buffer_handle() == buffer_handle);
        self.conceal_screen_cells
            .remove_where(|key, _| key.buffer_handle() == buffer_handle);
    }

    pub(super) fn note_cursor_color_colorscheme_change(&mut self) {
        self.colorscheme_generation = self.colorscheme_generation.next();
        self.note_cursor_color_observation_boundary();
    }

    pub(super) fn note_conceal_read_boundary(&mut self) {
        self.conceal_lines.clear();
        self.conceal_deltas.clear();
        self.conceal_screen_cells.clear();
    }

    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests;
