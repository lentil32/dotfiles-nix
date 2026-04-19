use super::CursorParseError;
use super::CursorResult;
use super::cursor_parse_error;
use super::screenpos::buffer_column_to_col1;
use super::screenpos::current_window_option;
use super::screenpos::parse_screenpos_cell;
use super::screenpos::screenpos_for_buffer_column;
use crate::events::logging::warn;
use crate::events::probe_cache::ConcealCacheKey;
use crate::events::probe_cache::ConcealCacheLookup;
use crate::events::probe_cache::ConcealDeltaCacheLookup;
use crate::events::probe_cache::ConcealRegion;
use crate::events::probe_cache::ConcealScreenCellCacheKey;
use crate::events::probe_cache::ConcealScreenCellCacheLookup;
use crate::events::probe_cache::ConcealWindowState;
use crate::events::runtime::buffer_text_revision;
use crate::events::runtime::cached_conceal_delta;
use crate::events::runtime::cached_conceal_regions;
use crate::events::runtime::cached_conceal_screen_cell;
use crate::events::runtime::note_conceal_read_boundary;
use crate::events::runtime::reclaim_conceal_regions_scratch;
use crate::events::runtime::record_conceal_full_scan;
use crate::events::runtime::record_conceal_region_cache_hit;
use crate::events::runtime::record_conceal_region_cache_miss;
use crate::events::runtime::record_conceal_screen_cell_cache_hit;
use crate::events::runtime::record_conceal_screen_cell_cache_miss;
use crate::events::runtime::store_conceal_delta;
use crate::events::runtime::store_conceal_regions;
use crate::events::runtime::store_conceal_screen_cell;
use crate::events::runtime::take_conceal_regions_scratch;
use crate::lua::i64_from_object_typed;
use crate::lua::string_from_object_typed;
use crate::position::ObservedCell;
use crate::position::ScreenCell;
use crate::position::WindowSurfaceSnapshot;
use nvim_oxi::Array;
use nvim_oxi::Object;
use nvim_oxi::api;
use nvim_oxi::conversion::FromObject;
use nvimrs_nvim_utils::mode::is_cmdline_mode;
use nvimrs_nvim_utils::mode::is_insert_like_mode;
use nvimrs_nvim_utils::mode::is_replace_like_mode;
use nvimrs_nvim_utils::mode::is_terminal_like_mode;
use nvimrs_nvim_utils::mode::is_visual_like_mode;
use std::sync::Arc;

fn conceal_screen_cell_cache_key(
    surface_snapshot: WindowSurfaceSnapshot,
    conceal_key: &ConcealCacheKey,
    col1: i64,
) -> ConcealScreenCellCacheKey {
    ConcealScreenCellCacheKey::from_surface(conceal_key, surface_snapshot, col1)
}

fn conceal_delta_cache_key(
    surface_snapshot: WindowSurfaceSnapshot,
    conceal_key: &ConcealCacheKey,
) -> crate::events::probe_cache::ConcealDeltaCacheKey {
    crate::events::probe_cache::ConcealDeltaCacheKey::from_surface(conceal_key, surface_snapshot)
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
struct WrappedScreenCellLayout {
    text_start_col: i64,
    text_width: i64,
}

impl WrappedScreenCellLayout {
    fn from_surface(surface_snapshot: WindowSurfaceSnapshot) -> Option<Self> {
        let window_col = surface_snapshot.window_origin().col();
        let text_offset0 = i64::from(surface_snapshot.text_offset0());
        let text_width = surface_snapshot
            .window_size()
            .max_col()
            .saturating_sub(text_offset0);
        if window_col <= 0 || text_width <= 0 {
            return None;
        }

        Some(Self {
            text_start_col: window_col.saturating_add(text_offset0),
            text_width,
        })
    }

    fn wrapped_cell_delta(self, start: ScreenCell, end: ScreenCell) -> Option<i64> {
        if start.row() == end.row() {
            return Some(end.col().saturating_sub(start.col()));
        }
        if end.row() < start.row() || !self.contains(start.col()) || !self.contains(end.col()) {
            return None;
        }

        let middle_rows = end.row().saturating_sub(start.row()).saturating_sub(1);
        let tail_width = self
            .text_end_col()
            .saturating_sub(start.col())
            .saturating_add(1);
        let head_width = end.col().saturating_sub(self.text_start_col);
        Some(
            tail_width
                .saturating_add(middle_rows.saturating_mul(self.text_width))
                .saturating_add(head_width),
        )
    }

    fn shift_left(self, cell: ScreenCell, mut delta: i64) -> Option<ScreenCell> {
        let mut row = cell.row();
        let mut col = cell.col();
        while delta > 0 {
            if row <= 0 || !self.contains(col) {
                return None;
            }

            let cells_to_row_start = col.saturating_sub(self.text_start_col);
            if delta <= cells_to_row_start {
                return ScreenCell::new(row, col.saturating_sub(delta));
            }

            delta = delta.saturating_sub(cells_to_row_start.saturating_add(1));
            row = row.saturating_sub(1);
            col = self.text_end_col();
        }

        ScreenCell::new(row, col)
    }

    fn contains(self, col: i64) -> bool {
        col >= self.text_start_col && col <= self.text_end_col()
    }

    fn text_end_col(self) -> i64 {
        self.text_start_col
            .saturating_add(self.text_width)
            .saturating_sub(1)
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum CachedConcealDriftHint {
    NoDrift,
    Drifted,
    Unknown,
}

fn replacement_display_width(replacement: &str) -> CursorResult<i64> {
    if replacement.is_empty() {
        return Ok(0);
    }

    let args = Array::from_iter([Object::from(replacement)]);
    let width = api::call_function("strdisplaywidth", args)?;
    i64_from_object_typed("strdisplaywidth", width)
        .map_err(|source| cursor_parse_error("strdisplaywidth", source))
}

fn parse_synconcealed(value: Object) -> CursorResult<Option<(String, i64)>> {
    let [concealed, replacement, match_id]: [Object; 3] = Vec::<Object>::from_object(value)
        .map_err(|_| CursorParseError::SynconcealedInvalidList)?
        .try_into()
        .map_err(|_| CursorParseError::SynconcealedUnexpectedLen)?;

    let concealed = i64_from_object_typed("synconcealed[0]", concealed)
        .map_err(|source| cursor_parse_error("synconcealed[0]", source))?;
    if concealed == 0 {
        return Ok(None);
    }

    let replacement = string_from_object_typed("synconcealed[1]", replacement)
        .map_err(|source| cursor_parse_error("synconcealed[1]", source))?;
    let match_id = i64_from_object_typed("synconcealed[2]", match_id)
        .map_err(|source| cursor_parse_error("synconcealed[2]", source))?;
    Ok(Some((replacement, match_id)))
}

pub(super) fn merge_conceal_region(
    regions: &mut Vec<ConcealRegion>,
    col1: i64,
    match_id: i64,
    replacement_width: i64,
) {
    if let Some(last) = regions.last_mut()
        && last.match_id == match_id
        && last.replacement_width == replacement_width
        && last.end_col1.saturating_add(1) == col1
    {
        last.end_col1 = col1;
        return;
    }

    regions.push(ConcealRegion {
        start_col1: col1,
        end_col1: col1,
        match_id,
        replacement_width,
    });
}

fn extend_concealed_regions(
    line: usize,
    start_col1: i64,
    end_col1: i64,
    regions: &mut Vec<ConcealRegion>,
) -> CursorResult<()> {
    if start_col1 > end_col1 {
        return Ok(());
    }

    let line = i64::try_from(line).unwrap_or(i64::MAX);
    for col1 in start_col1..=end_col1 {
        let args = Array::from_iter([Object::from(line), Object::from(col1)]);
        let concealed = parse_synconcealed(api::call_function("synconcealed", args)?)?;
        let Some((replacement, match_id)) = concealed else {
            continue;
        };

        let replacement_width = replacement_display_width(&replacement)?;
        merge_conceal_region(regions, col1, match_id, replacement_width);
    }
    Ok(())
}

fn window_buffer_handle(window: &api::Window) -> CursorResult<i64> {
    Ok(i64::from(window.get_buf()?.handle()))
}

fn concealcursor_mode_key(mode: &str) -> Option<char> {
    if is_cmdline_mode(mode) {
        Some('c')
    } else if is_insert_like_mode(mode) || is_replace_like_mode(mode) {
        Some('i')
    } else if is_visual_like_mode(mode) {
        Some('v')
    } else if is_terminal_like_mode(mode) {
        None
    } else {
        Some('n')
    }
}

pub(super) fn concealcursor_allows_mode(concealcursor: &str, mode: &str) -> bool {
    concealcursor_mode_key(mode).is_some_and(|mode_key| concealcursor.contains(mode_key))
}

fn conceal_window_state_allows_mode(window_state: &ConcealWindowState, mode: &str) -> bool {
    if window_state.conceallevel() <= 0 {
        return false;
    }

    concealcursor_allows_mode(window_state.concealcursor(), mode)
}

fn capture_conceal_window_state(window: &api::Window) -> CursorResult<ConcealWindowState> {
    let conceallevel: i64 = current_window_option(window, "conceallevel")?;
    let concealcursor: String = current_window_option(window, "concealcursor")?;
    Ok(ConcealWindowState::new(conceallevel, concealcursor))
}

fn conceal_cache_key(
    window: &api::Window,
    line: usize,
    window_state: ConcealWindowState,
) -> CursorResult<ConcealCacheKey> {
    let buffer_handle = window_buffer_handle(window)?;
    let text_revision = buffer_text_revision(buffer_handle)
        .map_err(nvim_oxi::Error::from)?
        .value();
    Ok(ConcealCacheKey::new(
        buffer_handle,
        text_revision,
        line,
        window_state,
    ))
}

fn cached_concealed_regions_for_cursor(
    key: ConcealCacheKey,
    column: usize,
) -> CursorResult<Arc<[ConcealRegion]>> {
    let required_col1 = i64::try_from(column).unwrap_or(i64::MAX);
    let cached = match cached_conceal_regions(&key) {
        Ok(ConcealCacheLookup::Hit(cached)) => Some(cached),
        Ok(ConcealCacheLookup::Miss) => None,
        Err(err) => {
            warn(&format!("conceal cache read failed: {err}"));
            None
        }
    };

    if let Some(cached) = cached.as_ref()
        && cached.scanned_to_col1() >= required_col1
    {
        record_conceal_region_cache_hit();
        return Ok(Arc::clone(cached.regions()));
    }
    record_conceal_region_cache_miss();

    let mut regions = match take_conceal_regions_scratch() {
        Ok(mut scratch) => {
            if let Some(cached) = cached.as_ref() {
                scratch.extend_from_slice(cached.regions());
            }
            scratch
        }
        Err(err) => {
            warn(&format!("conceal region scratch unavailable: {err}"));
            cached
                .as_ref()
                .map_or_else(Vec::new, |cached| cached.regions().to_vec())
        }
    };
    let scan_start_col1 = cached
        .as_ref()
        .map_or(1, |cached| cached.scanned_to_col1().saturating_add(1));
    let result = (|| -> CursorResult<Arc<[ConcealRegion]>> {
        record_conceal_full_scan(key.buffer_handle());
        extend_concealed_regions(key.line(), scan_start_col1, required_col1, &mut regions)?;

        let regions: Arc<[ConcealRegion]> = Arc::from(regions.as_slice());
        if let Err(err) = store_conceal_regions(key, required_col1, Arc::clone(&regions)) {
            warn(&format!("conceal cache write failed: {err}"));
        }
        Ok(regions)
    })();
    if let Err(err) = reclaim_conceal_regions_scratch(regions) {
        warn(&format!("conceal region scratch reclaim failed: {err}"));
    }
    result
}

fn cached_concealed_regions_hint_for_cursor(
    key: &ConcealCacheKey,
    column: usize,
) -> Option<Arc<[ConcealRegion]>> {
    let required_col1 = i64::try_from(column).unwrap_or(i64::MAX);
    let cached = match cached_conceal_regions(key) {
        Ok(ConcealCacheLookup::Hit(cached)) => cached,
        Ok(ConcealCacheLookup::Miss) => return None,
        Err(err) => {
            warn(&format!("conceal cache read failed: {err}"));
            return None;
        }
    };
    if cached.scanned_to_col1() < required_col1 {
        return None;
    }

    Some(Arc::clone(cached.regions()))
}

fn screen_cell_for_buffer_column(
    window: &api::Window,
    line: usize,
    col1: i64,
) -> CursorResult<Option<ScreenCell>> {
    parse_screenpos_cell(screenpos_for_buffer_column(window, line, col1)?)
}

fn cached_screen_cell_for_buffer_column(
    window: &api::Window,
    conceal_key: &ConcealCacheKey,
    surface_snapshot: WindowSurfaceSnapshot,
    col1: i64,
) -> CursorResult<Option<ScreenCell>> {
    let cache_key = conceal_screen_cell_cache_key(surface_snapshot, conceal_key, col1);
    match cached_conceal_screen_cell(&cache_key) {
        Ok(ConcealScreenCellCacheLookup::Hit(cell)) => {
            record_conceal_screen_cell_cache_hit();
            return Ok(cell);
        }
        Ok(ConcealScreenCellCacheLookup::Miss) => {
            record_conceal_screen_cell_cache_miss();
        }
        Err(err) => {
            record_conceal_screen_cell_cache_miss();
            warn(&format!("conceal screen cell cache read failed: {err}"));
        }
    }

    let cell = screen_cell_for_buffer_column(window, conceal_key.line(), col1)?;
    if let Err(err) = store_conceal_screen_cell(cache_key, cell) {
        warn(&format!("conceal screen cell cache write failed: {err}"));
    }
    Ok(cell)
}

fn cached_screen_cell_for_buffer_column_hint(
    conceal_key: &ConcealCacheKey,
    surface_snapshot: WindowSurfaceSnapshot,
    col1: i64,
) -> Option<Option<ScreenCell>> {
    let cache_key = conceal_screen_cell_cache_key(surface_snapshot, conceal_key, col1);
    match cached_conceal_screen_cell(&cache_key) {
        Ok(ConcealScreenCellCacheLookup::Hit(cell)) => Some(cell),
        Ok(ConcealScreenCellCacheLookup::Miss) => None,
        Err(err) => {
            warn(&format!("conceal screen cell cache read failed: {err}"));
            None
        }
    }
}

pub(super) fn apply_conceal_delta(
    raw_cell: ScreenCell,
    conceal_delta: i64,
    surface_snapshot: Option<WindowSurfaceSnapshot>,
) -> ScreenCell {
    surface_snapshot
        .and_then(WrappedScreenCellLayout::from_surface)
        .and_then(|layout| layout.shift_left(raw_cell, conceal_delta))
        .or_else(|| {
            ScreenCell::new(
                raw_cell.row(),
                raw_cell.col().saturating_sub(conceal_delta).max(1),
            )
        })
        .unwrap_or(raw_cell)
}

fn cached_conceal_drift_hint_from_regions_and_delta(
    current_col1: i64,
    regions: &[ConcealRegion],
    cached_delta: Option<i64>,
) -> CachedConcealDriftHint {
    if regions.is_empty()
        || !regions
            .iter()
            .any(|region| region.start_col1 < current_col1)
    {
        return CachedConcealDriftHint::NoDrift;
    }

    match cached_delta {
        Some(delta) if delta > 0 => CachedConcealDriftHint::Drifted,
        Some(_) => CachedConcealDriftHint::NoDrift,
        None => CachedConcealDriftHint::Unknown,
    }
}

fn cached_conceal_delta_hint(
    conceal_key: &ConcealCacheKey,
    current_col1: i64,
    raw_cell: ScreenCell,
    regions: &[ConcealRegion],
    surface_snapshot: Option<WindowSurfaceSnapshot>,
) -> CursorResult<Option<i64>> {
    let Some(surface_snapshot) = surface_snapshot else {
        return Ok(None);
    };
    let wrapped_layout = WrappedScreenCellLayout::from_surface(surface_snapshot);
    let cache_key = conceal_delta_cache_key(surface_snapshot, conceal_key);
    match cached_conceal_delta(&cache_key) {
        Ok(ConcealDeltaCacheLookup::Hit(cached)) if cached.current_col1() == current_col1 => {
            return Ok(Some(cached.delta()));
        }
        Ok(ConcealDeltaCacheLookup::Hit(_) | ConcealDeltaCacheLookup::Miss) => {}
        Err(err) => warn(&format!("conceal delta cache read failed: {err}")),
    }

    let mut cache_complete = true;
    let Some(conceal_delta) =
        conceal_delta_for_regions(current_col1, raw_cell, regions, wrapped_layout, |col1| {
            Ok(
                match cached_screen_cell_for_buffer_column_hint(conceal_key, surface_snapshot, col1)
                {
                    Some(cell) => cell,
                    None => {
                        cache_complete = false;
                        None
                    }
                },
            )
        })?
    else {
        return Ok(None);
    };
    if !cache_complete {
        return Ok(None);
    }

    Ok(Some(conceal_delta))
}

pub(super) fn observed_cell_for_raw_screenpos(
    window: &api::Window,
    line: usize,
    column: usize,
    mode: &str,
    raw_cell: ScreenCell,
    surface_snapshot: Option<WindowSurfaceSnapshot>,
) -> CursorResult<ObservedCell> {
    let window_state = capture_conceal_window_state(window)?;
    if column == 0 || !conceal_window_state_allows_mode(&window_state, mode) {
        return Ok(ObservedCell::Exact(raw_cell));
    }

    // This is a hint-only fast path. Exact conceal resolution clears cross-read cache state before
    // re-sampling, but motion reads can reuse prior hints to avoid deferring every sample.
    let conceal_key = conceal_cache_key(window, line, window_state)?;
    let Some(regions) = cached_concealed_regions_hint_for_cursor(&conceal_key, column) else {
        return Ok(ObservedCell::Deferred(raw_cell));
    };
    let current_col1 = buffer_column_to_col1(column);
    if matches!(
        cached_conceal_drift_hint_from_regions_and_delta(current_col1, regions.as_ref(), None),
        CachedConcealDriftHint::NoDrift
    ) {
        return Ok(ObservedCell::Exact(raw_cell));
    }
    let cached_delta = cached_conceal_delta_hint(
        &conceal_key,
        current_col1,
        raw_cell,
        regions.as_ref(),
        surface_snapshot,
    )?;

    Ok(
        match cached_conceal_drift_hint_from_regions_and_delta(
            current_col1,
            regions.as_ref(),
            cached_delta,
        ) {
            CachedConcealDriftHint::NoDrift => ObservedCell::Exact(raw_cell),
            CachedConcealDriftHint::Drifted | CachedConcealDriftHint::Unknown => {
                ObservedCell::Deferred(raw_cell)
            }
        },
    )
}

fn conceal_delta_for_regions(
    current_col1: i64,
    raw_cell: ScreenCell,
    regions: &[ConcealRegion],
    wrapped_layout: Option<WrappedScreenCellLayout>,
    mut screen_cell_for_col1: impl FnMut(i64) -> CursorResult<Option<ScreenCell>>,
) -> CursorResult<Option<i64>> {
    let mut conceal_delta = 0_i64;
    for region in regions
        .iter()
        .take_while(|region| region.start_col1 < current_col1)
    {
        let start = screen_cell_for_col1(region.start_col1)?;
        let effective_end_col1 = region.end_col1.min(current_col1.saturating_sub(1));
        let next_col1 = effective_end_col1.saturating_add(1);
        let end = if next_col1 == current_col1 {
            Some(raw_cell)
        } else {
            screen_cell_for_col1(next_col1)?
        };

        let (Some(start), Some(end)) = (start, end) else {
            continue;
        };
        let raw_width = if start.row() == end.row() {
            end.col().saturating_sub(start.col())
        } else {
            let Some(wrapped_layout) = wrapped_layout else {
                return Ok(None);
            };
            let Some(raw_width) = wrapped_layout.wrapped_cell_delta(start, end) else {
                return Ok(None);
            };
            raw_width
        };
        conceal_delta =
            conceal_delta.saturating_add(raw_width.saturating_sub(region.replacement_width).max(0));
    }

    Ok(Some(conceal_delta))
}

pub(super) fn resolve_buffer_cursor_position(
    window: &api::Window,
    line: usize,
    column: usize,
    mode: &str,
    raw_cell: ScreenCell,
    surface_snapshot: Option<WindowSurfaceSnapshot>,
) -> CursorResult<ScreenCell> {
    if column == 0 {
        return Ok(raw_cell);
    }

    let window_state = capture_conceal_window_state(window)?;
    if !conceal_window_state_allows_mode(&window_state, mode) {
        return Ok(raw_cell);
    }

    // Fast-motion reads may reuse conceal hints across reads to avoid deferring every motion
    // sample in conceal-enabled windows. Exact resolution is the authoritative path, so clear any
    // cross-read conceal cache state here before re-sampling syntax/extmark-driven conceal.
    if let Err(err) = note_conceal_read_boundary() {
        warn(&format!("conceal cache boundary update failed: {err}"));
    }

    let conceal_key = conceal_cache_key(window, line, window_state)?;
    let regions = cached_concealed_regions_for_cursor(conceal_key.clone(), column)?;
    if regions.is_empty() {
        return Ok(raw_cell);
    }

    let current_col1 = buffer_column_to_col1(column);
    let conceal_delta_cache_key =
        surface_snapshot.map(|snapshot| conceal_delta_cache_key(snapshot, &conceal_key));
    if let Some(cache_key) = conceal_delta_cache_key.as_ref() {
        match cached_conceal_delta(cache_key) {
            Ok(ConcealDeltaCacheLookup::Hit(cached)) if cached.current_col1() == current_col1 => {
                return Ok(apply_conceal_delta(
                    raw_cell,
                    cached.delta(),
                    surface_snapshot,
                ));
            }
            Ok(ConcealDeltaCacheLookup::Hit(_) | ConcealDeltaCacheLookup::Miss) => {}
            Err(err) => warn(&format!("conceal delta cache read failed: {err}")),
        }
    }
    let Some(conceal_delta) = conceal_delta_for_regions(
        current_col1,
        raw_cell,
        regions.as_ref(),
        surface_snapshot.and_then(WrappedScreenCellLayout::from_surface),
        |col1| {
            if let Some(surface_snapshot) = surface_snapshot {
                cached_screen_cell_for_buffer_column(window, &conceal_key, surface_snapshot, col1)
            } else {
                screen_cell_for_buffer_column(window, line, col1)
            }
        },
    )?
    else {
        // If the observation-time surface snapshot is unavailable, keep the raw sample and let
        // the next exact settle-time pass re-sync from a fresh window view instead of freezing a
        // guessed wrapped position.
        return Ok(raw_cell);
    };
    if let Some(cache_key) = conceal_delta_cache_key
        && let Err(err) = store_conceal_delta(cache_key, current_col1, conceal_delta)
    {
        warn(&format!("conceal delta cache write failed: {err}"));
    }

    Ok(apply_conceal_delta(
        raw_cell,
        conceal_delta,
        surface_snapshot,
    ))
}

#[cfg(test)]
#[path = "conceal_tests.rs"]
mod tests;
