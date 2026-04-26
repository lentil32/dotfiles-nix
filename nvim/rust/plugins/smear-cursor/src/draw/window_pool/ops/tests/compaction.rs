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
    max_prune_per_tick: usize,
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
    .prop_map(|(tab_specs, target_budget, max_prune_per_tick)| {
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
            max_prune_per_tick,
        }
    })
    .boxed()
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
            fixture.max_prune_per_tick,
        );

        prop_assert_eq!(
            global_compaction_prune_plan(
                &fixture.render_tabs,
                fixture.target_budget,
                fixture.max_prune_per_tick,
            ),
            expected,
        );
    }
}

fn global_compaction_prune_plan_sort_baseline(
    render_tabs: &HashMap<TabHandle, TabWindows>,
    target_budget: usize,
    max_prune_per_tick: usize,
) -> HashMap<TabHandle, Vec<usize>> {
    let total_windows = render_tabs
        .values()
        .map(|tab_windows| tab_windows.windows.len())
        .sum::<usize>();
    if total_windows <= target_budget || max_prune_per_tick == 0 {
        return HashMap::new();
    }

    let prune_goal = total_windows
        .saturating_sub(target_budget)
        .min(max_prune_per_tick);
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
