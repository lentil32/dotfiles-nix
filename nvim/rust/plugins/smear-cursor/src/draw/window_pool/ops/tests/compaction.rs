#[derive(Clone, Copy, Debug)]
enum CompactionLifecycleSpec {
    AvailableVisible { last_used_epoch: u8 },
    AvailableHidden { last_used_epoch: u8 },
    InUse { epoch: u8 },
    Invalid,
}

#[derive(Debug)]
struct CompactionFixture {
    render_tabs: HashMap<TabHandle, TabWindows>,
    target_budget: usize,
    max_teardown_attempts_per_tick: usize,
}

fn compaction_window_lifecycle_spec() -> BoxedStrategy<CompactionLifecycleSpec> {
    prop_oneof![
        any::<u8>().prop_map(
            |last_used_epoch| CompactionLifecycleSpec::AvailableVisible { last_used_epoch }
        ),
        any::<u8>().prop_map(|last_used_epoch| CompactionLifecycleSpec::AvailableHidden {
            last_used_epoch,
        }),
        any::<u8>().prop_map(|epoch| CompactionLifecycleSpec::InUse { epoch }),
        Just(CompactionLifecycleSpec::Invalid),
    ]
    .boxed()
}

fn compaction_window(index: usize, lifecycle: CompactionLifecycleSpec) -> CachedRenderWindow {
    let offset = i32::try_from(index).unwrap_or(i32::MAX);
    let handles = WindowBufferHandle {
        window_id: 10_000_i32.saturating_add(offset),
        buffer_id: BufferHandle::from(20_000_i32.saturating_add(offset)),
    };
    let placement = WindowPlacement {
        row: i64::try_from(index).unwrap_or(i64::MAX),
        col: i64::try_from(index.saturating_mul(2)).unwrap_or(i64::MAX),
        width: 1,
        zindex: 80,
    };

    match lifecycle {
        CompactionLifecycleSpec::AvailableVisible { last_used_epoch } => CachedRenderWindow {
            handles,
            lifecycle: CachedWindowLifecycle::AvailableVisible {
                last_used_epoch: FrameEpoch(u64::from(last_used_epoch)),
            },
            placement: Some(placement),
        },
        CompactionLifecycleSpec::AvailableHidden { last_used_epoch } => CachedRenderWindow {
            handles,
            lifecycle: CachedWindowLifecycle::AvailableHidden {
                last_used_epoch: FrameEpoch(u64::from(last_used_epoch)),
            },
            placement: Some(placement),
        },
        CompactionLifecycleSpec::InUse { epoch } => CachedRenderWindow {
            handles,
            lifecycle: CachedWindowLifecycle::InUse {
                epoch: FrameEpoch(u64::from(epoch)),
            },
            placement: Some(placement),
        },
        CompactionLifecycleSpec::Invalid => CachedRenderWindow {
            handles,
            lifecycle: CachedWindowLifecycle::Invalid,
            placement: None,
        },
    }
}

fn compaction_tab_windows(
    tab_offset: usize,
    lifecycles: &[CompactionLifecycleSpec],
    cached_budget: usize,
) -> TabWindows {
    let base_index = tab_offset.saturating_mul(32);
    let windows = lifecycles
        .iter()
        .copied()
        .enumerate()
        .map(|(index, lifecycle)| compaction_window(base_index.saturating_add(index), lifecycle))
        .collect::<Vec<_>>();

    let mut tab_windows = TabWindows {
        windows,
        cached_budget,
        ..TabWindows::default()
    };
    tab_windows.seed_tracking_from_windows_for_test();
    tab_windows
}

fn compaction_fixture() -> BoxedStrategy<CompactionFixture> {
    vec(
        (
            0_usize..=128,
            vec(compaction_window_lifecycle_spec(), 0..=16),
        ),
        0..=6,
    )
    .prop_flat_map(|tab_specs| {
        let total_windows = tab_specs
            .iter()
            .map(|(_, lifecycles)| lifecycles.len())
            .sum::<usize>();
        let budget_limit = total_windows.saturating_add(8);

        (
            Just(tab_specs),
            0_usize..=budget_limit,
            0_usize..=budget_limit,
        )
    })
    .prop_map(|(tab_specs, target_budget, max_teardown_attempts_per_tick)| {
        let render_tabs = tab_specs
            .into_iter()
            .enumerate()
            .map(|(tab_offset, (cached_budget, lifecycles))| {
                let raw_tab_handle =
                    i32::try_from(tab_offset.saturating_add(1)).unwrap_or(i32::MAX);
                (
                    tab_handle(raw_tab_handle),
                    compaction_tab_windows(tab_offset, &lifecycles, cached_budget),
                )
            })
            .collect::<HashMap<_, _>>();

        CompactionFixture {
            render_tabs,
            target_budget,
            max_teardown_attempts_per_tick,
        }
    })
    .boxed()
}

#[test]
fn compaction_bounds_all_teardown_attempts_across_visible_invalid_and_reusable_windows() {
    let lifecycles = [
        CompactionLifecycleSpec::AvailableVisible { last_used_epoch: 1 },
        CompactionLifecycleSpec::AvailableVisible { last_used_epoch: 2 },
        CompactionLifecycleSpec::Invalid,
        CompactionLifecycleSpec::Invalid,
        CompactionLifecycleSpec::Invalid,
        CompactionLifecycleSpec::AvailableHidden { last_used_epoch: 3 },
        CompactionLifecycleSpec::AvailableHidden { last_used_epoch: 4 },
        CompactionLifecycleSpec::AvailableHidden { last_used_epoch: 5 },
    ];
    let mut render_tabs = HashMap::from([(
        tab_handle(/*value*/ 1),
        compaction_tab_windows(/*tab_offset*/ 0, &lifecycles, /*cached_budget*/ 8),
    )]);
    let mut close_attempts = 0_usize;
    let mut close_cached = |tab_windows: &mut TabWindows, _namespace_id, index| {
        close_attempts = close_attempts.saturating_add(1);
        let _ = tab_windows.swap_remove_window(index);
        TrackedResourceCloseOutcome::ClosedOrGone
    };

    let summary = compact_tabs_to_budget_with_closer(
        &mut render_tabs,
        NamespaceId::new(/*value*/ 7),
        /*target_budget*/ 0,
        /*max_teardown_attempts_per_tick*/ 4,
        &mut close_cached,
    );

    assert_eq!(
        (close_attempts, summary),
        (
            4,
            CompactRenderWindowsSummary {
                target_budget: 0,
                total_windows_after: 4,
                closed_visible_windows: 2,
                pruned_windows: 0,
                invalid_removed_windows: 2,
                cleared_prepaint_overlays: 0,
                cleared_quarantined_resources: 0,
                teardown_attempts: 4,
                has_visible_windows_after: false,
                has_pending_work_after: true,
                potential_visual_change: true,
                close_stalled: false,
            },
        ),
    );
}

#[test]
fn retained_visible_close_is_not_retried_as_invalid_in_the_same_compaction_tick() {
    let lifecycles = [
        CompactionLifecycleSpec::AvailableVisible { last_used_epoch: 1 },
        CompactionLifecycleSpec::Invalid,
        CompactionLifecycleSpec::AvailableHidden { last_used_epoch: 2 },
    ];
    let mut render_tabs = HashMap::from([(
        tab_handle(/*value*/ 1),
        compaction_tab_windows(/*tab_offset*/ 0, &lifecycles, /*cached_budget*/ 3),
    )]);
    let mut attempted_handles = Vec::new();
    let mut retain_close = |tab_windows: &mut TabWindows, _namespace_id, index| {
        let Some(mut cached) = tab_windows.swap_remove_window(index) else {
            return TrackedResourceCloseOutcome::ClosedOrGone;
        };
        attempted_handles.push(cached.handles);
        cached.mark_invalid();
        tab_windows.push_cached_window(cached);
        TrackedResourceCloseOutcome::Retained
    };

    let summary = compact_tabs_to_budget_with_closer(
        &mut render_tabs,
        NamespaceId::new(/*value*/ 7),
        /*target_budget*/ 0,
        /*max_teardown_attempts_per_tick*/ 8,
        &mut retain_close,
    );

    assert_eq!(
        (attempted_handles.len(), summary),
        (
            1,
            CompactRenderWindowsSummary {
                target_budget: 0,
                total_windows_after: 3,
                closed_visible_windows: 0,
                pruned_windows: 0,
                invalid_removed_windows: 0,
                cleared_prepaint_overlays: 0,
                cleared_quarantined_resources: 0,
                teardown_attempts: 1,
                has_visible_windows_after: false,
                has_pending_work_after: true,
                potential_visual_change: true,
                close_stalled: true,
            },
        ),
    );
}

proptest! {
    #![proptest_config(pure_config())]

    #[test]
    fn prop_global_compaction_prune_plan_matches_sort_baseline_on_arbitrary_fixtures(
        fixture in compaction_fixture(),
    ) {
        let expected = global_compaction_prune_plan_sort_baseline(
            &fixture.render_tabs,
            fixture.target_budget,
            fixture.max_teardown_attempts_per_tick,
        );

        prop_assert_eq!(
            global_compaction_prune_plan(
                &fixture.render_tabs,
                fixture.target_budget,
                fixture.max_teardown_attempts_per_tick,
            ),
            expected,
        );
    }
}

fn global_compaction_prune_plan_sort_baseline(
    render_tabs: &HashMap<TabHandle, TabWindows>,
    target_budget: usize,
    max_teardown_attempts_per_tick: usize,
) -> HashMap<TabHandle, Vec<usize>> {
    let total_windows = render_tabs
        .values()
        .map(|tab_windows| tab_windows.windows.len())
        .sum::<usize>();
    if total_windows <= target_budget || max_teardown_attempts_per_tick == 0 {
        return HashMap::new();
    }

    let prune_goal = total_windows
        .saturating_sub(target_budget)
        .min(max_teardown_attempts_per_tick);
    let mut candidates = render_tabs
        .iter()
        .flat_map(|(tab_handle, tab_windows)| {
            tab_windows
                .windows
                .iter()
                .enumerate()
                .filter_map(|(index, cached)| {
                    cached
                        .available_epoch()
                        .map(|epoch| (epoch, *tab_handle, index))
                })
        })
        .collect::<Vec<_>>();
    candidates.sort_unstable();
    candidates.truncate(prune_goal.min(candidates.len()));

    let mut plan = HashMap::<TabHandle, Vec<usize>>::new();
    for (_, tab_handle, index) in candidates {
        plan.entry(tab_handle).or_default().push(index);
    }
    for indices in plan.values_mut() {
        indices.sort_unstable();
    }
    plan
}
