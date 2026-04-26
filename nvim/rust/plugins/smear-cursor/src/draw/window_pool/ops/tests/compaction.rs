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

    #[test]
    fn prop_compaction_summary_converged_to_idle_matches_budget_visibility_and_pending_work(
        target_budget in 0_usize..=64,
        total_windows_after in 0_usize..=64,
        closed_visible_windows in 0_usize..=64,
        pruned_windows in 0_usize..=64,
        invalid_removed_windows in 0_usize..=64,
        has_visible_windows_after in any::<bool>(),
        has_pending_work_after in any::<bool>(),
    ) {
        let summary = CompactRenderWindowsSummary {
            target_budget,
            total_windows_after,
            closed_visible_windows,
            pruned_windows,
            invalid_removed_windows,
            has_visible_windows_after,
            has_pending_work_after,
        };

        prop_assert_eq!(
            summary.converged_to_idle(),
            total_windows_after <= target_budget
                && !has_visible_windows_after
                && !has_pending_work_after,
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

fn benchmark_fixture_tabs(tab_count: i32, windows_per_tab: usize) -> HashMap<TabHandle, TabWindows> {
    let mut next_epoch = 1_u64;
    let mut next_window_id = 1_i32;

    (0..tab_count)
        .map(|tab_offset| {
            let windows = (0..windows_per_tab)
                .map(|_| {
                    let cached = cached(
                        next_window_id,
                        next_window_id.saturating_add(10_000),
                        next_epoch,
                    );
                    next_epoch = next_epoch.saturating_add(1);
                    next_window_id = next_window_id.saturating_add(1);
                    cached
                })
                .collect::<Vec<_>>();
            let mut tab_windows = TabWindows {
                windows,
                ..TabWindows::default()
            };
            tab_windows.seed_tracking_from_windows_for_test();
            (tab_handle(tab_offset.saturating_add(1)), tab_windows)
        })
        .collect()
}

fn run_compaction_planner_benchmark_case(
    case_name: &str,
    tabs: &HashMap<TabHandle, TabWindows>,
    target_budget: usize,
    max_prune_per_tick: usize,
    iterations: usize,
) {
    let expected =
        global_compaction_prune_plan_sort_baseline(tabs, target_budget, max_prune_per_tick);
    pretty_assertions::assert_eq!(
        global_compaction_prune_plan(tabs, target_budget, max_prune_per_tick),
        expected,
        "planner under test must match sort baseline for benchmark case {case_name}"
    );

    for implementation in ["current", "sort_baseline"] {
        for _ in 0..32 {
            let plan = match implementation {
                "current" => global_compaction_prune_plan(
                    std::hint::black_box(tabs),
                    std::hint::black_box(target_budget),
                    std::hint::black_box(max_prune_per_tick),
                ),
                "sort_baseline" => global_compaction_prune_plan_sort_baseline(
                    std::hint::black_box(tabs),
                    std::hint::black_box(target_budget),
                    std::hint::black_box(max_prune_per_tick),
                ),
                _ => unreachable!("unknown benchmark implementation"),
            };
            std::hint::black_box(plan);
        }

        let started_at = std::time::Instant::now();
        let mut checksum = 0_usize;
        for _ in 0..iterations {
            let plan = match implementation {
                "current" => global_compaction_prune_plan(
                    std::hint::black_box(tabs),
                    std::hint::black_box(target_budget),
                    std::hint::black_box(max_prune_per_tick),
                ),
                "sort_baseline" => global_compaction_prune_plan_sort_baseline(
                    std::hint::black_box(tabs),
                    std::hint::black_box(target_budget),
                    std::hint::black_box(max_prune_per_tick),
                ),
                _ => unreachable!("unknown benchmark implementation"),
            };
            checksum = checksum.saturating_add(plan.values().map(Vec::len).sum::<usize>());
            std::hint::black_box(plan);
        }

        let elapsed = started_at.elapsed();
        let nanos_per_iteration = elapsed.as_nanos() / (iterations.max(1) as u128);
        println!(
            "benchmark=global_compaction_prune_plan case={case_name} impl={implementation} total_windows={} target_budget={} max_prune_per_tick={} iterations={iterations} total_ms={} ns_per_iter={} checksum={checksum}",
            tabs.values()
                .map(|tab_windows| tab_windows.windows.len())
                .sum::<usize>(),
            target_budget,
            max_prune_per_tick,
            elapsed.as_millis(),
            nanos_per_iteration,
        );
    }
}

mod benchmarks {
    use super::*;

    #[test]
    #[ignore = "benchmark: run with cargo test -p nvimrs-smear-cursor benchmark_global_compaction_prune_plan --release -- --ignored --nocapture"]
    fn benchmark_global_compaction_prune_plan_cooling_tail() {
        let tabs = benchmark_fixture_tabs(32, 256);
        run_compaction_planner_benchmark_case("cooling_tail", &tabs, 2, 64, 250);
    }

    #[test]
    #[ignore = "benchmark: run with cargo test -p nvimrs-smear-cursor benchmark_global_compaction_prune_plan --release -- --ignored --nocapture"]
    fn benchmark_global_compaction_prune_plan_wide_prune_goal() {
        let tabs = benchmark_fixture_tabs(32, 256);
        run_compaction_planner_benchmark_case("wide_prune_goal", &tabs, 2, 256, 200);
    }
}
