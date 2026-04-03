use super::CursorParseError;
use super::CursorResult;
use super::ScreenCell;
use super::ScreenPoint;
use super::cursor_parse_error;
use super::screenpos::buffer_column_to_col1;
use super::screenpos::current_window_option;
use super::screenpos::parse_screenpos_cell;
use super::screenpos::required_dictionary_i64_field;
use super::screenpos::screen_cell_to_point;
use super::screenpos::screenpos_for_buffer_column;
use crate::core::state::CursorPositionSync;
use crate::events::logging::warn;
use crate::events::probe_cache::ConcealCacheKey;
use crate::events::probe_cache::ConcealCacheLookup;
use crate::events::probe_cache::ConcealDeltaCacheLookup;
use crate::events::probe_cache::ConcealRegion;
use crate::events::probe_cache::ConcealScreenCellCacheKey;
use crate::events::probe_cache::ConcealScreenCellCacheLookup;
use crate::events::runtime::cached_conceal_delta;
use crate::events::runtime::cached_conceal_regions;
use crate::events::runtime::cached_conceal_screen_cell;
use crate::events::runtime::record_conceal_full_scan;
use crate::events::runtime::record_conceal_region_cache_hit;
use crate::events::runtime::record_conceal_region_cache_miss;
use crate::events::runtime::record_conceal_screen_cell_cache_hit;
use crate::events::runtime::record_conceal_screen_cell_cache_miss;
use crate::events::runtime::store_conceal_delta;
use crate::events::runtime::store_conceal_regions;
use crate::events::runtime::store_conceal_screen_cell;
use crate::lua::i64_from_object_typed;
use crate::lua::string_from_object_typed;
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

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(super) struct ConcealScreenCellView {
    pub(super) window_row: i64,
    pub(super) window_col: i64,
    pub(super) window_width: i64,
    pub(super) window_height: i64,
    pub(super) topline: i64,
    pub(super) leftcol: i64,
    pub(super) textoff: i64,
}

impl ConcealScreenCellView {
    fn capture(window: &api::Window) -> CursorResult<Self> {
        let args = Array::from_iter([Object::from(window.handle())]);
        let [entry]: [Object; 1] =
            Vec::<Object>::from_object(api::call_function("getwininfo", args)?)
                .map_err(|_| CursorParseError::GetwininfoInvalidList)?
                .try_into()
                .map_err(|_| CursorParseError::GetwininfoUnexpectedLen)?;
        let dict = nvim_oxi::Dictionary::from_object(entry)
            .map_err(|_| CursorParseError::GetwininfoDictionary)?;

        Ok(Self {
            window_row: required_dictionary_i64_field(&dict, "getwininfo", "winrow")?,
            window_col: required_dictionary_i64_field(&dict, "getwininfo", "wincol")?,
            window_width: required_dictionary_i64_field(&dict, "getwininfo", "width")?,
            window_height: required_dictionary_i64_field(&dict, "getwininfo", "height")?,
            topline: required_dictionary_i64_field(&dict, "getwininfo", "topline")?,
            leftcol: required_dictionary_i64_field(&dict, "getwininfo", "leftcol")?,
            textoff: required_dictionary_i64_field(&dict, "getwininfo", "textoff")?,
        })
    }

    pub(super) fn cache_key(
        self,
        window_handle: i64,
        conceal_key: &ConcealCacheKey,
        col1: i64,
    ) -> ConcealScreenCellCacheKey {
        ConcealScreenCellCacheKey::new(
            window_handle,
            conceal_key.buffer_handle(),
            conceal_key.changedtick(),
            conceal_key.line(),
            col1,
            self.window_row,
            self.window_col,
            self.window_width,
            self.window_height,
            self.topline,
            self.leftcol,
            self.textoff,
        )
    }

    pub(super) fn delta_cache_key(
        self,
        window_handle: i64,
        conceal_key: &ConcealCacheKey,
    ) -> crate::events::probe_cache::ConcealDeltaCacheKey {
        crate::events::probe_cache::ConcealDeltaCacheKey::new(
            window_handle,
            conceal_key.buffer_handle(),
            conceal_key.changedtick(),
            conceal_key.line(),
            self.window_row,
            self.window_col,
            self.window_width,
            self.window_height,
            self.topline,
            self.leftcol,
            self.textoff,
        )
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

fn current_buffer_changedtick(buffer_handle: i64) -> CursorResult<u64> {
    let args = Array::from_iter([Object::from(buffer_handle), Object::from("changedtick")]);
    let value = api::call_function("getbufvar", args)?;
    let changedtick = i64_from_object_typed("getbufvar(changedtick)", value)
        .map_err(|source| cursor_parse_error("getbufvar(changedtick)", source))?;
    if changedtick < 0 {
        return Err(
            nvim_oxi::api::Error::Other("conceal changedtick must be non-negative".into()).into(),
        );
    }

    Ok(changedtick as u64)
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

fn conceal_can_affect_cursor_line(window: &api::Window, mode: &str) -> CursorResult<bool> {
    let conceallevel: i64 = current_window_option(window, "conceallevel")?;
    if conceallevel <= 0 {
        return Ok(false);
    }

    let concealcursor: String = current_window_option(window, "concealcursor")?;
    Ok(concealcursor_allows_mode(&concealcursor, mode))
}

fn conceal_cache_key(window: &api::Window, line: usize) -> CursorResult<ConcealCacheKey> {
    let buffer_handle = window_buffer_handle(window)?;
    let changedtick = current_buffer_changedtick(buffer_handle)?;
    Ok(ConcealCacheKey::new(buffer_handle, changedtick, line))
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

    let mut regions = cached
        .as_ref()
        .map_or_else(Vec::new, |cached| cached.regions().to_vec());
    let scan_start_col1 = cached
        .as_ref()
        .map_or(1, |cached| cached.scanned_to_col1().saturating_add(1));
    record_conceal_full_scan();
    extend_concealed_regions(key.line(), scan_start_col1, required_col1, &mut regions)?;

    let regions: Arc<[ConcealRegion]> = regions.into();
    if let Err(err) = store_conceal_regions(key, required_col1, Arc::clone(&regions)) {
        warn(&format!("conceal cache write failed: {err}"));
    }
    Ok(regions)
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
    view: ConcealScreenCellView,
    col1: i64,
) -> CursorResult<Option<ScreenCell>> {
    let window_handle = i64::from(window.handle());
    let cache_key = view.cache_key(window_handle, conceal_key, col1);
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
    window: &api::Window,
    conceal_key: &ConcealCacheKey,
    view: ConcealScreenCellView,
    col1: i64,
) -> Option<Option<ScreenCell>> {
    let window_handle = i64::from(window.handle());
    let cache_key = view.cache_key(window_handle, conceal_key, col1);
    match cached_conceal_screen_cell(&cache_key) {
        Ok(ConcealScreenCellCacheLookup::Hit(cell)) => Some(cell),
        Ok(ConcealScreenCellCacheLookup::Miss) => None,
        Err(err) => {
            warn(&format!("conceal screen cell cache read failed: {err}"));
            None
        }
    }
}

pub(super) fn apply_conceal_delta(raw_cell: ScreenCell, conceal_delta: i64) -> ScreenPoint {
    (
        raw_cell.0 as f64,
        raw_cell.1.saturating_sub(conceal_delta).max(1) as f64,
    )
}

fn cached_conceal_drift_hint_from_regions_and_delta(
    current_col1: i64,
    regions: &[ConcealRegion],
    cached_delta: Option<Option<i64>>,
) -> CachedConcealDriftHint {
    if regions.is_empty()
        || !regions
            .iter()
            .any(|region| region.start_col1 < current_col1)
    {
        return CachedConcealDriftHint::NoDrift;
    }

    match cached_delta {
        Some(Some(delta)) if delta > 0 => CachedConcealDriftHint::Drifted,
        Some(Some(_)) | Some(None) => CachedConcealDriftHint::NoDrift,
        None => CachedConcealDriftHint::Unknown,
    }
}

fn cached_conceal_delta_hint(
    window: &api::Window,
    conceal_key: &ConcealCacheKey,
    current_col1: i64,
    raw_cell: ScreenCell,
    regions: &[ConcealRegion],
) -> CursorResult<Option<Option<i64>>> {
    let screen_cell_view = match ConcealScreenCellView::capture(window) {
        Ok(screen_cell_view) => screen_cell_view,
        Err(err) => {
            warn(&format!("conceal screen cell view capture failed: {err}"));
            return Ok(None);
        }
    };
    let cache_key = screen_cell_view.delta_cache_key(i64::from(window.handle()), conceal_key);
    match cached_conceal_delta(&cache_key) {
        Ok(ConcealDeltaCacheLookup::Hit(cached)) if cached.current_col1() == current_col1 => {
            return Ok(Some(Some(cached.delta())));
        }
        Ok(ConcealDeltaCacheLookup::Hit(_) | ConcealDeltaCacheLookup::Miss) => {}
        Err(err) => warn(&format!("conceal delta cache read failed: {err}")),
    }

    let mut cache_complete = true;
    let conceal_delta = conceal_delta_for_regions(current_col1, raw_cell, regions, |col1| {
        Ok(
            match cached_screen_cell_for_buffer_column_hint(
                window,
                conceal_key,
                screen_cell_view,
                col1,
            ) {
                Some(cell) => cell,
                None => {
                    cache_complete = false;
                    None
                }
            },
        )
    })?;
    if !cache_complete {
        return Ok(None);
    }

    Ok(Some(conceal_delta))
}

pub(super) fn cursor_position_sync_for_raw_screenpos(
    window: &api::Window,
    line: usize,
    column: usize,
    mode: &str,
    raw_cell: ScreenCell,
) -> CursorResult<CursorPositionSync> {
    if column == 0 || !conceal_can_affect_cursor_line(window, mode)? {
        return Ok(CursorPositionSync::Exact);
    }

    let conceal_key = conceal_cache_key(window, line)?;
    let Some(regions) = cached_concealed_regions_hint_for_cursor(&conceal_key, column) else {
        return Ok(CursorPositionSync::ConcealDeferred);
    };
    let current_col1 = buffer_column_to_col1(column);
    if matches!(
        cached_conceal_drift_hint_from_regions_and_delta(current_col1, regions.as_ref(), None),
        CachedConcealDriftHint::NoDrift
    ) {
        return Ok(CursorPositionSync::Exact);
    }
    let cached_delta = cached_conceal_delta_hint(
        window,
        &conceal_key,
        current_col1,
        raw_cell,
        regions.as_ref(),
    )?;

    Ok(
        match cached_conceal_drift_hint_from_regions_and_delta(
            current_col1,
            regions.as_ref(),
            cached_delta,
        ) {
            CachedConcealDriftHint::NoDrift => CursorPositionSync::Exact,
            CachedConcealDriftHint::Drifted | CachedConcealDriftHint::Unknown => {
                CursorPositionSync::ConcealDeferred
            }
        },
    )
}

pub(super) fn conceal_delta_for_regions(
    current_col1: i64,
    raw_cell: ScreenCell,
    regions: &[ConcealRegion],
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

        let (Some((start_row, start_col)), Some((end_row, end_col))) = (start, end) else {
            continue;
        };
        if start_row != end_row {
            return Ok(None);
        }

        let raw_width = end_col.saturating_sub(start_col);
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
) -> CursorResult<ScreenPoint> {
    if column == 0 {
        return Ok(screen_cell_to_point(raw_cell));
    }

    if !conceal_can_affect_cursor_line(window, mode)? {
        return Ok(screen_cell_to_point(raw_cell));
    }

    let conceal_key = conceal_cache_key(window, line)?;
    let regions = cached_concealed_regions_for_cursor(conceal_key.clone(), column)?;
    if regions.is_empty() {
        return Ok(screen_cell_to_point(raw_cell));
    }

    let screen_cell_view = match ConcealScreenCellView::capture(window) {
        Ok(screen_cell_view) => Some(screen_cell_view),
        Err(err) => {
            warn(&format!("conceal screen cell view capture failed: {err}"));
            None
        }
    };
    let current_col1 = buffer_column_to_col1(column);
    let conceal_delta_cache_key = screen_cell_view.map(|screen_cell_view| {
        screen_cell_view.delta_cache_key(i64::from(window.handle()), &conceal_key)
    });
    if let Some(cache_key) = conceal_delta_cache_key.as_ref() {
        match cached_conceal_delta(cache_key) {
            Ok(ConcealDeltaCacheLookup::Hit(cached)) if cached.current_col1() == current_col1 => {
                return Ok(apply_conceal_delta(raw_cell, cached.delta()));
            }
            Ok(ConcealDeltaCacheLookup::Hit(_) | ConcealDeltaCacheLookup::Miss) => {}
            Err(err) => warn(&format!("conceal delta cache read failed: {err}")),
        }
    }
    let Some(conceal_delta) =
        conceal_delta_for_regions(current_col1, raw_cell, regions.as_ref(), |col1| {
            if let Some(screen_cell_view) = screen_cell_view {
                cached_screen_cell_for_buffer_column(window, &conceal_key, screen_cell_view, col1)
            } else {
                screen_cell_for_buffer_column(window, line, col1)
            }
        })?
    else {
        // this conceal correction is proven for same-row drift. If a concealed region crosses a
        // soft-wrap boundary, keep the shell-authoritative raw screenpos until we model wrapped
        // line offsets explicitly.
        return Ok(screen_cell_to_point(raw_cell));
    };
    if let Some(cache_key) = conceal_delta_cache_key
        && let Err(err) = store_conceal_delta(cache_key, current_col1, conceal_delta)
    {
        warn(&format!("conceal delta cache write failed: {err}"));
    }

    Ok(apply_conceal_delta(raw_cell, conceal_delta))
}

#[cfg(test)]
mod tests {
    use super::CachedConcealDriftHint;
    use super::ConcealScreenCellView;
    use super::apply_conceal_delta;
    use super::cached_conceal_drift_hint_from_regions_and_delta;
    use super::conceal_delta_for_regions;
    use super::concealcursor_allows_mode;
    use super::merge_conceal_region;
    use crate::events::probe_cache::ConcealCacheKey;
    use crate::events::probe_cache::ConcealRegion;
    use pretty_assertions::assert_eq;

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

    fn conceal_screen_cell_view(
        window_row: i64,
        window_col: i64,
        window_width: i64,
        window_height: i64,
        topline: i64,
        leftcol: i64,
        textoff: i64,
    ) -> ConcealScreenCellView {
        ConcealScreenCellView {
            window_row,
            window_col,
            window_width,
            window_height,
            topline,
            leftcol,
            textoff,
        }
    }

    #[test]
    fn apply_conceal_delta_moves_cursor_left_without_changing_row() {
        let adjusted = apply_conceal_delta((2, 38), 5);

        assert_eq!(adjusted, (2.0, 33.0));
    }

    #[test]
    fn cached_conceal_drift_hint_ignores_regions_at_or_after_cursor() {
        let regions = [conceal_region(8, 9, 17, 0)];

        let hint = cached_conceal_drift_hint_from_regions_and_delta(8, &regions, None);

        assert_eq!(hint, CachedConcealDriftHint::NoDrift);
    }

    #[test]
    fn cached_conceal_drift_hint_marks_unknown_when_prior_regions_lack_cached_delta() {
        let regions = [conceal_region(3, 4, 17, 1)];

        let hint = cached_conceal_drift_hint_from_regions_and_delta(6, &regions, None);

        assert_eq!(hint, CachedConcealDriftHint::Unknown);
    }

    #[test]
    fn cached_conceal_drift_hint_uses_cached_delta_when_available() {
        let regions = [conceal_region(3, 4, 17, 1)];

        let drifted = cached_conceal_drift_hint_from_regions_and_delta(6, &regions, Some(Some(2)));
        let exact = cached_conceal_drift_hint_from_regions_and_delta(6, &regions, Some(Some(0)));
        let wrapped_raw = cached_conceal_drift_hint_from_regions_and_delta(6, &regions, Some(None));

        assert_eq!(drifted, CachedConcealDriftHint::Drifted);
        assert_eq!(exact, CachedConcealDriftHint::NoDrift);
        assert_eq!(wrapped_raw, CachedConcealDriftHint::NoDrift);
    }

    #[test]
    fn merge_conceal_region_merges_adjacent_cells_with_same_match_and_width() {
        let mut regions = Vec::new();

        merge_conceal_region(&mut regions, 3, 17, 1);
        merge_conceal_region(&mut regions, 4, 17, 1);
        merge_conceal_region(&mut regions, 7, 18, 0);

        assert_eq!(
            regions,
            vec![conceal_region(3, 4, 17, 1), conceal_region(7, 7, 18, 0)],
        );
    }

    #[test]
    fn conceal_screen_cell_cache_key_tracks_window_view_state() {
        let conceal_key = ConcealCacheKey::new(22, 14, 7);
        let base = conceal_screen_cell_view(2, 3, 120, 40, 11, 0, 4);

        let moved_view = conceal_screen_cell_view(2, 3, 120, 40, 12, 0, 4);
        let changed_textoff = conceal_screen_cell_view(2, 3, 120, 40, 11, 0, 5);

        assert_ne!(
            base.cache_key(8, &conceal_key, 5),
            moved_view.cache_key(8, &conceal_key, 5),
        );
        assert_ne!(
            base.cache_key(8, &conceal_key, 5),
            changed_textoff.cache_key(8, &conceal_key, 5),
        );
    }

    #[test]
    fn conceal_delta_cache_key_tracks_window_view_state() {
        let conceal_key = ConcealCacheKey::new(22, 14, 7);
        let base = conceal_screen_cell_view(2, 3, 120, 40, 11, 0, 4);

        let moved_view = conceal_screen_cell_view(2, 3, 120, 40, 12, 0, 4);
        let changed_textoff = conceal_screen_cell_view(2, 3, 120, 40, 11, 0, 5);

        assert_ne!(
            base.delta_cache_key(8, &conceal_key),
            moved_view.delta_cache_key(8, &conceal_key),
        );
        assert_ne!(
            base.delta_cache_key(8, &conceal_key),
            changed_textoff.delta_cache_key(8, &conceal_key),
        );
    }

    #[test]
    fn concealcursor_allows_expected_mode_families() {
        assert!(concealcursor_allows_mode("nvc", "n"));
        assert!(concealcursor_allows_mode("i", "R"));
        assert!(concealcursor_allows_mode("v", "V"));
        assert!(!concealcursor_allows_mode("", "n"));
        assert!(!concealcursor_allows_mode("n", "c"));
        assert!(!concealcursor_allows_mode("n", "t"));
    }

    #[test]
    fn conceal_delta_for_regions_accumulates_same_row_drift() {
        let regions = vec![conceal_region(2, 3, 11, 1), conceal_region(5, 5, 12, 0)];
        let delta = conceal_delta_for_regions(6, (4, 10), &regions, |col1| {
            Ok(match col1 {
                2 => Some((4, 4)),
                4 => Some((4, 8)),
                5 => Some((4, 9)),
                _ => None,
            })
        })
        .expect("same-row conceal delta should parse");

        assert_eq!(delta, Some(4));
    }

    #[test]
    fn conceal_delta_for_regions_returns_none_when_region_wraps_rows() {
        let regions = vec![conceal_region(2, 3, 11, 1)];
        let delta = conceal_delta_for_regions(4, (5, 2), &regions, |col1| {
            Ok(match col1 {
                2 => Some((4, 80)),
                _ => None,
            })
        })
        .expect("wrapped conceal region should parse");

        assert_eq!(delta, None);
    }
}
