use super::*;

#[test]
fn probe_cache_state_conceal_regions_smoke_hits_exact_key_and_misses_after_line_change() {
    let mut cache = ProbeCacheState::default();
    let concealcursor = "n";
    let regions: Arc<[ConcealRegion]> =
        vec![conceal_region(3, 4, 11, 1), conceal_region(8, 9, 12, 2)].into();
    let base = conceal_key(22, 14, 7, 2, concealcursor);
    let shifted_line = conceal_key(22, 14, 8, 2, concealcursor);

    cache.store_conceal_regions(base.clone(), 18, Arc::clone(&regions));

    pretty_assertions::assert_eq!(
        cache.cached_conceal_regions(&base),
        ConcealCacheLookup::Hit(CachedConcealRegions::new(18, Arc::clone(&regions))),
    );
    pretty_assertions::assert_eq!(
        cache.cached_conceal_regions(&shifted_line),
        ConcealCacheLookup::Miss
    );
}
