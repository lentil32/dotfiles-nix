use super::*;
use pretty_assertions::assert_eq;

#[test]
fn buffer_invalidation_clears_cursor_text_and_conceal_entries_only_for_the_target_buffer() {
    let mut cache = ProbeCacheState::default();
    let context = Some(cursor_text_context(
        22,
        14,
        7,
        vec![observed_row("current")],
        Some(vec![observed_row("tracked")]),
    ));
    let target_context_key = cursor_text_context_key(22, 14, 7, Some(6));
    let other_context_key = cursor_text_context_key(29, 14, 7, Some(6));
    let target_conceal_key = conceal_key(22, 14, 7, 2, "n");
    let other_conceal_key = conceal_key(29, 14, 7, 2, "n");
    let regions: Arc<[ConcealRegion]> = vec![conceal_region(3, 4, 11, 1)].into();

    cache.store_cursor_text_context(target_context_key.clone(), context.clone());
    cache.store_cursor_text_context(other_context_key.clone(), context.clone());
    cache.store_conceal_regions(target_conceal_key.clone(), 18, Arc::clone(&regions));
    cache.store_conceal_regions(other_conceal_key.clone(), 18, Arc::clone(&regions));

    cache.invalidate_buffer(22);

    assert_eq!(
        cache.cached_cursor_text_context(&target_context_key),
        CursorTextContextCacheLookup::Miss,
    );
    assert_eq!(
        cache.cached_cursor_text_context(&other_context_key),
        CursorTextContextCacheLookup::Hit(context),
    );
    assert_eq!(
        cache.cached_conceal_regions(&target_conceal_key),
        ConcealCacheLookup::Miss,
    );
    assert_eq!(
        cache.cached_conceal_regions(&other_conceal_key),
        ConcealCacheLookup::Hit(CachedConcealRegions::new(18, regions)),
    );
}

#[test]
fn conceal_buffer_invalidation_keeps_cursor_text_context_entries() {
    let mut cache = ProbeCacheState::default();
    let context = Some(cursor_text_context(
        22,
        14,
        7,
        vec![observed_row("current")],
        None,
    ));
    let context_key = cursor_text_context_key(22, 14, 7, None);
    let conceal_key = conceal_key(22, 14, 7, 2, "n");
    let regions: Arc<[ConcealRegion]> = vec![conceal_region(3, 4, 11, 1)].into();

    cache.store_cursor_text_context(context_key.clone(), context.clone());
    cache.store_conceal_regions(conceal_key.clone(), 18, Arc::clone(&regions));

    cache.invalidate_conceal_buffer(22);

    assert_eq!(
        cache.cached_cursor_text_context(&context_key),
        CursorTextContextCacheLookup::Hit(context),
    );
    assert_eq!(
        cache.cached_conceal_regions(&conceal_key),
        ConcealCacheLookup::Miss
    );
}
