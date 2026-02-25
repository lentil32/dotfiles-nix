#[test]
fn clear_work_detection_ignores_hidden_windows_within_keep_budget() {
    let tabs = tabs_with(TabWindows {
        windows: vec![cached(1, 11, 1), cached(2, 12, 2)],
        cached_budget: 8,
        ..TabWindows::default()
    });
    assert!(!has_visible_windows(&tabs));
    assert!(!has_pending_clear_work(&tabs, 32));
}

#[test]
fn clear_work_detection_requires_clear_when_visible_window_exists() {
    let tabs = tabs_with(TabWindows {
        windows: vec![CachedRenderWindow {
            handles: WindowBufferHandle {
                window_id: 1,
                buffer_id: 11,
            },
            lifecycle: CachedWindowLifecycle::AvailableVisible {
                last_used_epoch: FrameEpoch(3),
            },
            placement: Some(WindowPlacement {
                row: 1,
                col: 1,
                width: 1,
                zindex: 80,
            }),
        }],
        ..TabWindows::default()
    });

    assert!(has_visible_windows(&tabs));
    assert!(has_pending_clear_work(&tabs, 32));
}

#[test]
fn clear_work_detection_requires_clear_when_cache_exceeds_budget() {
    let tabs = tabs_with(TabWindows {
        windows: vec![cached(1, 11, 1), cached(2, 12, 2), cached(3, 13, 3)],
        cached_budget: 1,
        ..TabWindows::default()
    });

    assert!(!has_visible_windows(&tabs));
    assert!(has_pending_clear_work(&tabs, 32));
}
