#[test]
fn global_compaction_prune_plan_selects_oldest_windows_across_tabs() {
    let mut tabs = HashMap::from([
        (
            1_i32,
            TabWindows {
                windows: vec![cached(1, 11, 1), cached(4, 14, 4)],
                ..TabWindows::default()
            },
        ),
        (
            2_i32,
            TabWindows {
                windows: vec![cached(2, 12, 2), cached(3, 13, 3)],
                ..TabWindows::default()
            },
        ),
    ]);
    for tab_windows in tabs.values_mut() {
        tab_windows.seed_tracking_from_windows_for_test();
    }

    let prune_plan = global_compaction_prune_plan(&tabs, 2, 4);

    assert_eq!(
        prune_plan,
        HashMap::from([(1_i32, vec![0_usize]), (2_i32, vec![0_usize])])
    );
}

#[test]
fn global_compaction_prune_plan_respects_max_prune_per_tick() {
    let mut tabs = HashMap::from([(
        1_i32,
        TabWindows {
            windows: vec![
                cached(1, 11, 1),
                cached(2, 12, 2),
                cached(3, 13, 3),
                cached(4, 14, 4),
            ],
            ..TabWindows::default()
        },
    )]);
    tabs.get_mut(&1)
        .expect("test tab should exist")
        .seed_tracking_from_windows_for_test();

    let prune_plan = global_compaction_prune_plan(&tabs, 1, 2);

    assert_eq!(prune_plan, HashMap::from([(1_i32, vec![0_usize, 1_usize])]));
}

#[test]
fn global_compaction_prune_plan_ignores_hot_cached_budget_when_targeting_idle_budget() {
    let mut tabs = HashMap::from([(
        1_i32,
        TabWindows {
            windows: vec![
                cached(1, 11, 1),
                cached(2, 12, 2),
                cached(3, 13, 3),
                cached(4, 14, 4),
            ],
            cached_budget: 64,
            ..TabWindows::default()
        },
    )]);
    tabs.get_mut(&1)
        .expect("test tab should exist")
        .seed_tracking_from_windows_for_test();

    let prune_plan = global_compaction_prune_plan(&tabs, 2, 4);

    assert_eq!(prune_plan, HashMap::from([(1_i32, vec![0_usize, 1_usize])]));
}

#[test]
fn global_compaction_prune_plan_returns_all_available_when_goal_covers_candidate_pool() {
    let placement = WindowPlacement {
        row: 0,
        col: 0,
        width: 1,
        zindex: 50,
    };
    let mut tabs = HashMap::from([(
        1_i32,
        TabWindows {
            windows: vec![
                cached(1, 11, 1),
                CachedRenderWindow::new_in_use(
                    WindowBufferHandle {
                        window_id: 2,
                        buffer_id: 12,
                    },
                    FrameEpoch(2),
                    placement,
                ),
                cached(3, 13, 3),
                CachedRenderWindow {
                    handles: WindowBufferHandle {
                        window_id: 4,
                        buffer_id: 14,
                    },
                    lifecycle: CachedWindowLifecycle::Invalid,
                    placement: None,
                },
            ],
            ..TabWindows::default()
        },
    )]);
    tabs.get_mut(&1)
        .expect("test tab should exist")
        .seed_tracking_from_windows_for_test();

    let prune_plan = global_compaction_prune_plan(&tabs, 0, 8);

    assert_eq!(prune_plan, HashMap::from([(1_i32, vec![0_usize, 2_usize])]));
}

#[test]
fn compaction_summary_only_converges_when_idle_budget_and_visibility_are_clear() {
    let summary = CompactRenderWindowsSummary {
        target_budget: 2,
        total_windows_before: 4,
        total_windows_after: 2,
        closed_visible_windows: 0,
        pruned_windows: 2,
        invalid_removed_windows: 0,
        has_visible_windows_after: false,
        has_pending_work_after: false,
    };

    assert!(summary.converged_to_idle());

    let still_pending = CompactRenderWindowsSummary {
        has_pending_work_after: true,
        ..summary
    };
    assert!(!still_pending.converged_to_idle());

    let still_visible = CompactRenderWindowsSummary {
        has_pending_work_after: false,
        has_visible_windows_after: true,
        ..summary
    };
    assert!(!still_visible.converged_to_idle());
}

fn global_compaction_prune_plan_sort_baseline(
    render_tabs: &HashMap<i32, TabWindows>,
    target_budget: usize,
    max_prune_per_tick: usize,
) -> HashMap<i32, Vec<usize>> {
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

    let mut plan = HashMap::<i32, Vec<usize>>::new();
    for (_, tab_handle, index) in candidates {
        plan.entry(tab_handle).or_default().push(index);
    }
    for indices in plan.values_mut() {
        indices.sort_unstable();
    }
    plan
}

fn benchmark_fixture_tabs(tab_count: i32, windows_per_tab: usize) -> HashMap<i32, TabWindows> {
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
            (tab_offset.saturating_add(1), tab_windows)
        })
        .collect()
}

fn run_compaction_planner_benchmark_case(
    case_name: &str,
    tabs: &HashMap<i32, TabWindows>,
    target_budget: usize,
    max_prune_per_tick: usize,
    iterations: usize,
) {
    let expected =
        global_compaction_prune_plan_sort_baseline(tabs, target_budget, max_prune_per_tick);
    assert_eq!(
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

#[test]
fn global_compaction_prune_plan_matches_sort_baseline_on_large_fixture() {
    let tabs = benchmark_fixture_tabs(16, 128);

    assert_eq!(
        global_compaction_prune_plan(&tabs, 2, 64),
        global_compaction_prune_plan_sort_baseline(&tabs, 2, 64)
    );
}

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
