use super::*;

proptest! {
    #![proptest_config(pure_config())]

    #[test]
    fn prop_probe_cache_state_boundary_events_invalidate_only_the_expected_partitions(
        color_sample in cursor_color_sample_strategy(),
        concealcursor in concealcursor_strategy(),
        regions in conceal_regions_strategy(),
        screen_cell in conceal_screen_cell_strategy(),
        current_col1 in any::<i64>(),
        delta in any::<i64>(),
        nearby_rows in observed_rows_strategy(2),
        tracked_nearby_rows in proptest::option::of(observed_rows_strategy(2)),
        event in boundary_event_strategy(),
    ) {
        let mut cache = ProbeCacheState::default();
        let color_witness = witness_with_cache_generation(11, 22, 14, "n", Some(cursor(7, 8)), 0, 0);
        let context_key = cursor_text_context_key(22, 14, 7, Some(5));
        let context = Some(cursor_text_context(22, 14, 7, nearby_rows, tracked_nearby_rows));
        let conceal_key = conceal_key(22, 14, 7, 2, &concealcursor);
        let screen_cell_key = ConcealScreenCellCacheKey::from_surface(
            &conceal_key,
            conceal_surface_snapshot(8, 22, 11, 0, 4, 2, 3, 40, 120),
            5,
        );
        let delta_key = ConcealDeltaCacheKey::from_surface(
            &conceal_key,
            conceal_surface_snapshot(8, 22, 11, 0, 4, 2, 3, 40, 120),
        );

        cache.store_cursor_color_sample(color_witness.clone(), color_sample);
        cache.store_cursor_text_context(context_key.clone(), context.clone());
        cache.store_conceal_regions(conceal_key.clone(), 18, Arc::clone(&regions));
        cache.store_conceal_screen_cell(screen_cell_key.clone(), screen_cell);
        cache.store_conceal_delta(delta_key.clone(), current_col1, delta);

        match event {
            BoundaryEvent::CursorColorObservationBoundary => {
                cache.note_cursor_color_observation_boundary();
            }
            BoundaryEvent::CursorColorColorschemeChange => {
                cache.note_cursor_color_colorscheme_change();
            }
            BoundaryEvent::ConcealReadBoundary => {
                cache.note_conceal_read_boundary();
            }
            BoundaryEvent::Reset => {
                cache.note_cursor_color_colorscheme_change();
                cache.reset();
            }
        }

        let queried_color_witness = match event {
            BoundaryEvent::CursorColorObservationBoundary => {
                witness_with_cache_generation(11, 22, 14, "n", Some(cursor(7, 8)), 0, 1)
            }
            BoundaryEvent::CursorColorColorschemeChange => {
                witness_with_cache_generation(11, 22, 14, "n", Some(cursor(7, 8)), 1, 1)
            }
            BoundaryEvent::ConcealReadBoundary | BoundaryEvent::Reset => color_witness,
        };
        let expect_color_miss = matches!(
            event,
            BoundaryEvent::CursorColorObservationBoundary
                | BoundaryEvent::CursorColorColorschemeChange
                | BoundaryEvent::Reset
        );
        let expect_conceal_miss =
            matches!(event, BoundaryEvent::ConcealReadBoundary | BoundaryEvent::Reset);
        let expect_context_miss = matches!(event, BoundaryEvent::Reset);
        let expected_colorscheme_generation = match event {
            BoundaryEvent::CursorColorColorschemeChange => Generation::new(1),
            BoundaryEvent::Reset => Generation::INITIAL,
            BoundaryEvent::CursorColorObservationBoundary | BoundaryEvent::ConcealReadBoundary => {
                Generation::INITIAL
            }
        };
        let expected_cache_generation = match event {
            BoundaryEvent::CursorColorObservationBoundary
            | BoundaryEvent::CursorColorColorschemeChange => Generation::new(1),
            BoundaryEvent::ConcealReadBoundary | BoundaryEvent::Reset => Generation::INITIAL,
        };

        prop_assert_eq!(cache.colorscheme_generation(), expected_colorscheme_generation);
        prop_assert_eq!(cache.cursor_color_cache_generation(), expected_cache_generation);
        prop_assert_eq!(
            cache.cached_cursor_color_sample(&queried_color_witness),
            if expect_color_miss {
                CursorColorCacheLookup::Miss
            } else {
                CursorColorCacheLookup::Hit(color_sample)
            },
        );
        prop_assert_eq!(
            cache.cached_cursor_text_context(&context_key),
            if expect_context_miss {
                CursorTextContextCacheLookup::Miss
            } else {
                CursorTextContextCacheLookup::Hit(context)
            },
        );
        prop_assert_eq!(
            cache.cached_conceal_regions(&conceal_key),
            if expect_conceal_miss {
                ConcealCacheLookup::Miss
            } else {
                ConcealCacheLookup::Hit(CachedConcealRegions::new(18, Arc::clone(&regions)))
            },
        );
        prop_assert_eq!(
            cache.cached_conceal_screen_cell(&screen_cell_key),
            if expect_conceal_miss {
                ConcealScreenCellCacheLookup::Miss
            } else {
                ConcealScreenCellCacheLookup::Hit(screen_cell)
            },
        );
        prop_assert_eq!(
            cache.cached_conceal_delta(&delta_key),
            if expect_conceal_miss {
                ConcealDeltaCacheLookup::Miss
            } else {
                ConcealDeltaCacheLookup::Hit(CachedConcealDelta::new(current_col1, delta))
            },
        );
    }
}
