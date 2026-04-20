use super::*;

#[test]
fn probe_cache_state_conceal_deltas_smoke_overwrite_latest_value_and_miss_after_view_change() {
    let mut cache = ProbeCacheState::default();
    let conceal_key = conceal_key(22, 14, 7, 2, "n");
    let base = ConcealDeltaCacheKey::from_surface(
        &conceal_key,
        conceal_surface_snapshot((8, 22), 11, 0, 4, (2, 3), (40, 120)),
    );
    let shifted_view = ConcealDeltaCacheKey::from_surface(
        &conceal_key,
        conceal_surface_snapshot((8, 22), 11, 0, 5, (2, 3), (40, 120)),
    );

    cache.store_conceal_delta(base.clone(), 12, -3);
    cache.store_conceal_delta(base.clone(), 13, 4);

    pretty_assertions::assert_eq!(
        cache.cached_conceal_delta(&base),
        ConcealDeltaCacheLookup::Hit(CachedConcealDelta::new(13, 4)),
    );
    pretty_assertions::assert_eq!(
        cache.cached_conceal_delta(&shifted_view),
        ConcealDeltaCacheLookup::Miss,
    );
}
