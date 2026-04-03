use super::*;

proptest! {
    #![proptest_config(pure_config())]

    #[test]
    fn prop_probe_cache_state_cursor_text_context_entries_hit_only_for_identical_key(
        buffer_handle in any::<i64>(),
        changedtick in any::<u64>(),
        cursor_line in any::<i64>(),
        tracked_line in proptest::option::of(any::<i64>()),
        nearby_rows in observed_rows_strategy(3),
        tracked_nearby_rows in proptest::option::of(observed_rows_strategy(3)),
        store_some in any::<bool>(),
        axis in cache_key_mutation_axis(CURSOR_TEXT_CONTEXT_AXIS_COUNT),
    ) {
        let mut cache = ProbeCacheState::default();
        let base = cursor_text_context_key(buffer_handle, changedtick, cursor_line, tracked_line);
        let context = store_some.then(|| {
            cursor_text_context(
                buffer_handle,
                changedtick,
                cursor_line,
                nearby_rows.clone(),
                tracked_nearby_rows.clone(),
            )
        });
        cache.store_cursor_text_context(base.clone(), context.clone());

        prop_assert_eq!(
            cache.cached_cursor_text_context(&base),
            CursorTextContextCacheLookup::Hit(context),
        );

        let mutated = match axis.index() {
            0 => cursor_text_context_key(
                buffer_handle.wrapping_add(1),
                changedtick,
                cursor_line,
                tracked_line,
            ),
            1 => cursor_text_context_key(
                buffer_handle,
                changedtick.wrapping_add(1),
                cursor_line,
                tracked_line,
            ),
            2 => cursor_text_context_key(
                buffer_handle,
                changedtick,
                cursor_line.wrapping_add(1),
                tracked_line,
            ),
            3 => cursor_text_context_key(
                buffer_handle,
                changedtick,
                cursor_line,
                tracked_line.map_or(Some(cursor_line.wrapping_add(1)), |_| None),
            ),
            _ => panic!("unexpected cursor text axis {}", axis.index()),
        };

        prop_assert_eq!(
            cache.cached_cursor_text_context(&mutated),
            CursorTextContextCacheLookup::Miss,
        );
    }
}
