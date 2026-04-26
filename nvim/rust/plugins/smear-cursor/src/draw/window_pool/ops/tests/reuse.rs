#[derive(Clone, Copy, Debug)]
enum ReuseLifecycleSpec {
    AvailableVisible { last_used_epoch: u8 },
    AvailableHidden { last_used_epoch: u8 },
    InUse { epoch: u8 },
    Invalid,
}

impl ReuseLifecycleSpec {
    fn is_visible_available(self) -> bool {
        matches!(self, Self::AvailableVisible { .. })
    }

    fn as_cached_window_lifecycle(self) -> CachedWindowLifecycle {
        match self {
            Self::AvailableVisible { last_used_epoch } => CachedWindowLifecycle::AvailableVisible {
                last_used_epoch: FrameEpoch(u64::from(last_used_epoch)),
            },
            Self::AvailableHidden { last_used_epoch } => CachedWindowLifecycle::AvailableHidden {
                last_used_epoch: FrameEpoch(u64::from(last_used_epoch)),
            },
            Self::InUse { epoch } => CachedWindowLifecycle::InUse {
                epoch: FrameEpoch(u64::from(epoch)),
            },
            Self::Invalid => CachedWindowLifecycle::Invalid,
        }
    }

    fn rollover(self, previous_epoch: FrameEpoch) -> Self {
        match self {
            Self::AvailableVisible { last_used_epoch } => {
                Self::AvailableVisible { last_used_epoch }
            }
            Self::AvailableHidden { last_used_epoch } => Self::AvailableHidden { last_used_epoch },
            Self::InUse { epoch } if u64::from(epoch) == previous_epoch.0 => {
                Self::AvailableVisible {
                    last_used_epoch: epoch,
                }
            }
            Self::InUse { epoch } => Self::AvailableVisible {
                last_used_epoch: epoch,
            },
            Self::Invalid => Self::Invalid,
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum ReuseAcquireCandidateSpec {
    MatchingValid,
    MatchingMissingWindow,
    MatchingMissingBuffer,
    Invalid,
}

fn reuse_lifecycle_spec() -> BoxedStrategy<ReuseLifecycleSpec> {
    prop_oneof![
        any::<u8>()
            .prop_map(|last_used_epoch| ReuseLifecycleSpec::AvailableVisible { last_used_epoch }),
        any::<u8>()
            .prop_map(|last_used_epoch| ReuseLifecycleSpec::AvailableHidden { last_used_epoch }),
        any::<u8>().prop_map(|epoch| ReuseLifecycleSpec::InUse { epoch }),
        Just(ReuseLifecycleSpec::Invalid),
    ]
    .boxed()
}

fn reuse_acquire_candidate_spec() -> BoxedStrategy<ReuseAcquireCandidateSpec> {
    prop_oneof![
        Just(ReuseAcquireCandidateSpec::MatchingValid),
        Just(ReuseAcquireCandidateSpec::MatchingMissingWindow),
        Just(ReuseAcquireCandidateSpec::MatchingMissingBuffer),
        Just(ReuseAcquireCandidateSpec::Invalid),
    ]
    .boxed()
}

fn reuse_remove_fixture() -> BoxedStrategy<(Vec<(ReuseLifecycleSpec, u8)>, usize)> {
    vec((reuse_lifecycle_spec(), 0_u8..=3), 1..=16)
        .prop_flat_map(|specs| {
            let len = specs.len();
            (Just(specs), 0_usize..len)
        })
        .boxed()
}

fn reuse_placement(key: u8) -> WindowPlacement {
    let row_offset = i64::from(key);
    WindowPlacement {
        row: 10_i64.saturating_add(row_offset),
        col: 20_i64.saturating_add(row_offset.saturating_mul(3)),
        width: u32::from(key % 3).saturating_add(1),
        zindex: 100_u32.saturating_add(u32::from(key)),
    }
}

fn reuse_cached_window(
    index: usize,
    lifecycle: ReuseLifecycleSpec,
    placement_key: u8,
) -> CachedRenderWindow {
    let offset = i32::try_from(index).unwrap_or(i32::MAX);
    CachedRenderWindow {
        handles: WindowBufferHandle {
            window_id: 1_000_i32.saturating_add(offset),
            buffer_id: BufferHandle::from(2_000_i32.saturating_add(offset)),
        },
        lifecycle: lifecycle.as_cached_window_lifecycle(),
        placement: (!matches!(lifecycle, ReuseLifecycleSpec::Invalid))
            .then_some(reuse_placement(placement_key)),
    }
}

fn reuse_matching_visible_window(index: usize, placement: WindowPlacement) -> CachedRenderWindow {
    let offset = i32::try_from(index).unwrap_or(i32::MAX);
    CachedRenderWindow {
        handles: WindowBufferHandle {
            window_id: 10_000_i32.saturating_add(offset),
            buffer_id: BufferHandle::from(20_000_i32.saturating_add(offset)),
        },
        lifecycle: CachedWindowLifecycle::AvailableVisible {
            last_used_epoch: FrameEpoch(1),
        },
        placement: Some(placement),
    }
}

fn reuse_acquire_candidate_window(
    index: usize,
    spec: ReuseAcquireCandidateSpec,
    placement: WindowPlacement,
) -> CachedRenderWindow {
    let offset = i32::try_from(index).unwrap_or(i32::MAX);
    match spec {
        ReuseAcquireCandidateSpec::MatchingValid => CachedRenderWindow {
            handles: WindowBufferHandle {
                window_id: 50_000_i32.saturating_add(offset),
                buffer_id: BufferHandle::from(60_000_i32.saturating_add(offset)),
            },
            lifecycle: CachedWindowLifecycle::AvailableVisible {
                last_used_epoch: FrameEpoch(1),
            },
            placement: Some(placement),
        },
        ReuseAcquireCandidateSpec::MatchingMissingWindow => CachedRenderWindow {
            handles: WindowBufferHandle {
                window_id: -50_000_i32.saturating_sub(offset),
                buffer_id: BufferHandle::from(60_000_i32.saturating_add(offset)),
            },
            lifecycle: CachedWindowLifecycle::AvailableVisible {
                last_used_epoch: FrameEpoch(1),
            },
            placement: Some(placement),
        },
        ReuseAcquireCandidateSpec::MatchingMissingBuffer => CachedRenderWindow {
            handles: WindowBufferHandle {
                window_id: 50_000_i32.saturating_add(offset),
                buffer_id: BufferHandle::from(-60_000_i32.saturating_sub(offset)),
            },
            lifecycle: CachedWindowLifecycle::AvailableVisible {
                last_used_epoch: FrameEpoch(1),
            },
            placement: Some(placement),
        },
        ReuseAcquireCandidateSpec::Invalid => CachedRenderWindow {
            handles: WindowBufferHandle {
                window_id: -70_000_i32.saturating_sub(offset),
                buffer_id: BufferHandle::from(-80_000_i32.saturating_sub(offset)),
            },
            lifecycle: CachedWindowLifecycle::Invalid,
            placement: None,
        },
    }
}

fn reuse_tab_windows_from_specs(
    specs: &[(ReuseLifecycleSpec, u8)],
    cached_budget: usize,
) -> TabWindows {
    let windows = specs
        .iter()
        .copied()
        .enumerate()
        .map(|(index, (lifecycle, placement_key))| {
            reuse_cached_window(index, lifecycle, placement_key)
        })
        .collect::<Vec<_>>();

    let mut tab_windows = TabWindows {
        windows,
        cached_budget,
        ..TabWindows::default()
    };
    tab_windows.seed_tracking_from_windows_for_test();
    tab_windows
}

fn reuse_window_signature(
    cached: CachedRenderWindow,
) -> (i32, BufferHandle, CachedWindowLifecycle, Option<WindowPlacement>) {
    (
        cached.handles.window_id,
        cached.handles.buffer_id,
        cached.lifecycle,
        cached.placement,
    )
}

fn reuse_window_signatures(
    windows: &[CachedRenderWindow],
) -> Vec<(i32, BufferHandle, CachedWindowLifecycle, Option<WindowPlacement>)> {
    windows
        .iter()
        .copied()
        .map(reuse_window_signature)
        .collect()
}

fn reuse_lru_prune_indices_baseline(
    windows: &[CachedRenderWindow],
    keep_count: usize,
) -> Vec<usize> {
    let mut available = windows
        .iter()
        .enumerate()
        .filter_map(|(index, cached)| cached.available_epoch().map(|epoch| (epoch, index)))
        .collect::<Vec<_>>();
    if available.len() <= keep_count {
        return Vec::new();
    }

    available.sort_unstable();
    let remove_count = available.len().saturating_sub(keep_count);
    let mut remove_indices = available
        .into_iter()
        .take(remove_count)
        .map(|(_, index)| index)
        .collect::<Vec<_>>();
    remove_indices.sort_unstable();
    remove_indices
}

fn reuse_single_acquire_reference(
    mut windows: Vec<CachedRenderWindow>,
    placement: WindowPlacement,
) -> (
    std::result::Result<(i32, ReuseFailureCounters), AcquireError>,
    Vec<CachedRenderWindow>,
) {
    let mut reuse_failures = ReuseFailureCounters::default();

    loop {
        let candidate_index = windows
            .iter()
            .enumerate()
            .rev()
            .find_map(|(index, cached)| {
                (cached.available_epoch().is_some() && cached.placement == Some(placement))
                    .then_some(index)
            });
        let Some(index) = candidate_index else {
            return (
                Err(AcquireError::Exhausted {
                    allocation_policy: AllocationPolicy::ReuseOnly,
                }),
                windows,
            );
        };

        let cached = windows[index];
        if cached.handles.window_id <= 0 {
            reuse_failures.missing_window = reuse_failures.missing_window.saturating_add(1);
            let _ = windows.swap_remove(index);
            continue;
        }
        if !cached.handles.buffer_id.is_valid() {
            reuse_failures.missing_buffer = reuse_failures.missing_buffer.saturating_add(1);
            let _ = windows.swap_remove(index);
            continue;
        }

        windows[index].lifecycle = CachedWindowLifecycle::InUse {
            epoch: FrameEpoch(0),
        };
        windows[index].placement = Some(placement);
        return (Ok((cached.handles.window_id, reuse_failures)), windows);
    }
}

proptest! {
    #![proptest_config(pure_config())]

    #[test]
    fn prop_rollover_in_use_windows_matches_reference_lifecycles_and_tracking(
        specs in vec((reuse_lifecycle_spec(), 0_u8..=3), 0..=16),
        previous_epoch in any::<u8>(),
    ) {
        let previous_epoch = FrameEpoch(u64::from(previous_epoch));
        let mut tab_windows = reuse_tab_windows_from_specs(&specs, ADAPTIVE_POOL_MIN_BUDGET);

        rollover_in_use_windows(&mut tab_windows, previous_epoch);

        let expected_lifecycles = specs
            .iter()
            .copied()
            .map(|(lifecycle, _)| lifecycle.rollover(previous_epoch).as_cached_window_lifecycle())
            .collect::<Vec<_>>();
        let actual_lifecycles = tab_windows
            .windows
            .iter()
            .map(|cached| cached.lifecycle)
            .collect::<Vec<_>>();

        prop_assert_eq!(actual_lifecycles, expected_lifecycles);
        tab_windows.assert_tracking_consistent();
    }

    #[test]
    fn prop_lru_prune_indices_matches_sort_baseline(
        specs in vec((reuse_lifecycle_spec(), 0_u8..=3), 0..=16),
        keep_count in 0_usize..=16,
    ) {
        let windows = specs
            .iter()
            .copied()
            .enumerate()
            .map(|(index, (lifecycle, placement_key))| {
                reuse_cached_window(index, lifecycle, placement_key)
            })
            .collect::<Vec<_>>();

        prop_assert_eq!(
            lru_prune_indices(&windows, keep_count),
            reuse_lru_prune_indices_baseline(&windows, keep_count),
        );
    }

    #[test]
    fn prop_swap_remove_window_matches_vector_swap_remove_and_preserves_tracking(
        (specs, remove_index) in reuse_remove_fixture(),
    ) {
        let mut tab_windows = reuse_tab_windows_from_specs(&specs, ADAPTIVE_POOL_MIN_BUDGET);
        let payloads = tab_windows
            .windows
            .iter()
            .copied()
            .map(|cached| {
                (
                    cached.handles.window_id,
                    u64::try_from(cached.handles.window_id).unwrap_or_default(),
                )
            })
            .collect::<Vec<_>>();
        for (window_id, payload_hash) in payloads {
            tab_windows.cache_payload(
                window_id,
                payload_hash,
            );
        }

        let mut expected_windows = tab_windows.windows.clone();
        let expected_removed = expected_windows.swap_remove(remove_index);
        let removed = tab_windows
            .swap_remove_window(remove_index)
            .expect("fixture index should be removable");

        prop_assert_eq!(
            reuse_window_signature(removed),
            reuse_window_signature(expected_removed),
        );
        prop_assert_eq!(
            reuse_window_signatures(&tab_windows.windows),
            reuse_window_signatures(&expected_windows),
        );
        prop_assert!(
            !tab_windows.cached_payload_matches(
                expected_removed.handles.window_id,
                u64::try_from(expected_removed.handles.window_id).unwrap_or_default(),
            )
        );
        for cached in expected_windows {
            prop_assert!(tab_windows.cached_payload_matches(
                cached.handles.window_id,
                u64::try_from(cached.handles.window_id).unwrap_or_default(),
            ));
        }
        tab_windows.assert_tracking_consistent();
    }

    #[test]
    fn prop_take_visible_available_indices_followed_by_hide_matches_lifecycle_truth(
        specs in vec((reuse_lifecycle_spec(), 0_u8..=3), 0..=16),
    ) {
        let mut tab_windows = reuse_tab_windows_from_specs(&specs, ADAPTIVE_POOL_MIN_BUDGET);
        let expected_hide_indices = specs
            .iter()
            .copied()
            .enumerate()
            .filter_map(|(index, (lifecycle, _))| lifecycle.is_visible_available().then_some(index))
            .collect::<Vec<_>>();

        let hide_indices = tab_windows.take_visible_available_indices_for_hide();
        prop_assert_eq!(hide_indices.as_slice(), expected_hide_indices.as_slice());

        for index in hide_indices {
            let previous_lifecycle = tab_windows.windows[index].lifecycle;
            let previous_placement = tab_windows.windows[index].placement;
            tab_windows.windows[index].mark_hidden();
            let next_lifecycle = tab_windows.windows[index].lifecycle;
            let next_placement = tab_windows.windows[index].placement;
            tab_windows.track_window_transition(
                index,
                previous_lifecycle,
                previous_placement,
                next_lifecycle,
                next_placement,
            );
        }

        let expected_lifecycles = specs
            .iter()
            .copied()
            .map(|(lifecycle, _)| match lifecycle {
                ReuseLifecycleSpec::AvailableVisible { last_used_epoch } => {
                    CachedWindowLifecycle::AvailableHidden {
                        last_used_epoch: FrameEpoch(u64::from(last_used_epoch)),
                    }
                }
                _ => lifecycle.as_cached_window_lifecycle(),
            })
            .collect::<Vec<_>>();
        let actual_lifecycles = tab_windows
            .windows
            .iter()
            .map(|cached| cached.lifecycle)
            .collect::<Vec<_>>();

        prop_assert_eq!(actual_lifecycles, expected_lifecycles);
        tab_windows.assert_tracking_consistent();
    }

    #[test]
    fn prop_changing_reuse_placement_only_tracks_the_new_key_after_next_frame(
        old_key in 0_u8..=3,
        new_key in 0_u8..=3,
    ) {
        let new_key = if new_key == old_key {
            new_key.saturating_add(1) % 4
        } else {
            new_key
        };
        let old_placement = reuse_placement(old_key);
        let new_placement = reuse_placement(new_key);
        let mut tab_windows = reuse_tab_windows_from_specs(
            &[(ReuseLifecycleSpec::AvailableVisible { last_used_epoch: 1 }, old_key)],
            ADAPTIVE_POOL_MIN_BUDGET,
        );

        let previous_lifecycle = tab_windows.windows[0].lifecycle;
        let previous_placement = tab_windows.windows[0].placement;
        prop_assert!(tab_windows.windows[0].mark_in_use(tab_windows.current_epoch));
        tab_windows.windows[0].set_placement(new_placement);
        let next_lifecycle = tab_windows.windows[0].lifecycle;
        let next_placement = tab_windows.windows[0].placement;
        tab_windows.track_window_transition(
            0,
            previous_lifecycle,
            previous_placement,
            next_lifecycle,
            next_placement,
        );

        prop_assert_eq!(
            available_window_index_for_placement(&tab_windows, old_placement),
            None,
        );

        begin_tab_frame(&mut tab_windows, 0);

        prop_assert_eq!(
            available_window_index_for_placement(&tab_windows, new_placement),
            Some(0),
        );
        prop_assert_eq!(
            available_window_index_for_placement(&tab_windows, old_placement),
            None,
        );
        tab_windows.assert_tracking_consistent();
    }

    #[test]
    fn prop_matching_visible_pool_supports_exact_reuse_capacity_then_exhausts(
        matching_visible_count in 0_usize..=8,
    ) {
        let placement = reuse_placement(0);
        let mut tab_windows = TabWindows {
            windows: (0..matching_visible_count)
                .map(|index| reuse_matching_visible_window(index, placement))
                .collect(),
            ..TabWindows::default()
        };
        tab_windows.seed_tracking_from_windows_for_test();
        let mut tabs = tabs_with(tab_windows);

        let acquired_window_ids = (0..matching_visible_count)
            .map(|_| {
                let acquired = acquire(
                    &mut tabs,
                    NamespaceId::new(/*value*/ 1),
                    tab_handle(1),
                    placement,
                    AllocationPolicy::ReuseOnly,
                )
                .expect("prepared matching pool should satisfy expected reuse demand");
                prop_assert_eq!(acquired.reuse_failures, ReuseFailureCounters::default());
                Ok::<i32, TestCaseError>(acquired.window_id)
            })
            .collect::<Result<Vec<_>, _>>()?;

        let expected_window_ids = (0..matching_visible_count)
            .rev()
            .map(|index| {
                let offset = i32::try_from(index).unwrap_or(i32::MAX);
                10_000_i32.saturating_add(offset)
            })
            .collect::<Vec<_>>();
        prop_assert_eq!(acquired_window_ids, expected_window_ids);
        let exhausted = acquire(
            &mut tabs,
            NamespaceId::new(/*value*/ 1),
            tab_handle(1),
            placement,
            AllocationPolicy::ReuseOnly,
        );
        prop_assert_eq!(
            exhausted.err(),
            Some(AcquireError::Exhausted {
                allocation_policy: AllocationPolicy::ReuseOnly,
            }),
        );
    }

    #[test]
    fn prop_single_reuse_acquire_matches_failure_cleanup_reference_model(
        specs in vec(reuse_acquire_candidate_spec(), 0..=12),
    ) {
        let placement = reuse_placement(2);
        let windows = specs
            .iter()
            .copied()
            .enumerate()
            .map(|(index, spec)| reuse_acquire_candidate_window(index, spec, placement))
            .collect::<Vec<_>>();
        let mut tab_windows = TabWindows {
            windows: windows.clone(),
            ..TabWindows::default()
        };
        tab_windows.seed_tracking_from_windows_for_test();

        let (expected_result, expected_windows) = reuse_single_acquire_reference(windows, placement);
        let actual_result = super::acquire_in_tab(
            &mut tab_windows,
            NamespaceId::new(/*value*/ 1),
            placement,
            AllocationPolicy::ReuseOnly,
        );

        match (actual_result, expected_result) {
            (Ok(actual), Ok((expected_window_id, expected_failures))) => {
                prop_assert_eq!(actual.window_id, expected_window_id);
                prop_assert_eq!(actual.reuse_failures, expected_failures);
            }
            (Err(actual), Err(expected)) => {
                prop_assert_eq!(actual, expected);
            }
            (actual, expected) => {
                prop_assert_eq!(format!("{actual:?}"), format!("{expected:?}"));
            }
        }

        prop_assert_eq!(
            reuse_window_signatures(&tab_windows.windows),
            reuse_window_signatures(&expected_windows),
        );
        tab_windows.assert_tracking_consistent();
    }

    #[test]
    fn prop_frame_capacity_target_matches_reference_formula(
        in_use_windows in 0_usize..=64,
        planned_windows in 0_usize..=64,
        max_kept_windows in 0_usize..=64,
        allocation_policy in prop_oneof![
            Just(AllocationPolicy::ReuseOnly),
            Just(AllocationPolicy::BootstrapIfPoolEmpty),
        ],
    ) {
        let required_capacity = in_use_windows.saturating_add(planned_windows);
        let warm_spare = match allocation_policy {
            AllocationPolicy::ReuseOnly => 1,
            AllocationPolicy::BootstrapIfPoolEmpty => 0,
        };
        let expected = if required_capacity == 0 {
            FrameCapacityTarget {
                requested_capacity: 0,
                target_capacity: 0,
            }
        } else {
            let requested_capacity = required_capacity.saturating_add(warm_spare);
            FrameCapacityTarget {
                requested_capacity,
                target_capacity: requested_capacity.min(max_kept_windows),
            }
        };

        prop_assert_eq!(
            frame_capacity_target(
                in_use_windows,
                planned_windows,
                max_kept_windows,
                allocation_policy,
            ),
            expected,
        );
    }
}
