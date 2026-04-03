use super::*;

#[test]
fn probe_cache_state_conceal_screen_cells_smoke_hits_exact_key_and_misses_after_view_change() {
    let mut cache = ProbeCacheState::default();
    let state = conceal_window_state(2, "n");
    let base =
        ConcealScreenCellCacheKey::new(8, 22, 14, 7, 5, 2, 3, 120, 40, 11, 0, 4, state.clone());
    let shifted_view =
        ConcealScreenCellCacheKey::new(8, 22, 14, 7, 5, 2, 3, 120, 40, 11, 1, 4, state);
    let cell = Some((17, 23));

    cache.store_conceal_screen_cell(base.clone(), cell);

    pretty_assertions::assert_eq!(
        cache.cached_conceal_screen_cell(&base),
        ConcealScreenCellCacheLookup::Hit(cell),
    );
    pretty_assertions::assert_eq!(
        cache.cached_conceal_screen_cell(&shifted_view),
        ConcealScreenCellCacheLookup::Miss,
    );
}
