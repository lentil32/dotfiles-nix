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
