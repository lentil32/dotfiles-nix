use super::*;

#[test]
fn probe_cache_state_conceal_screen_cells_smoke_hits_exact_key_and_misses_after_view_change() {
    let mut cache = ProbeCacheState::default();
    let conceal_key = conceal_key(22, 14, 7, 2, "n");
    let base = ConcealScreenCellCacheKey::from_surface(
        &conceal_key,
        conceal_surface_snapshot((8, 22), 11, 0, 4, (2, 3), (40, 120)),
        5,
    );
    let shifted_view = ConcealScreenCellCacheKey::from_surface(
        &conceal_key,
        conceal_surface_snapshot((8, 22), 11, 1, 4, (2, 3), (40, 120)),
        5,
    );
    let cell = Some(ScreenCell::new(17, 23).expect("one-based conceal screen cell"));

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
