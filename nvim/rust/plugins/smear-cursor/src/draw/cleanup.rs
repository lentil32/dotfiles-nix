//! Render window cleanup, compaction, and hard-purge operations.

use super::TrackedResourceCloseSummary;
use super::TrackedWindowBufferCloseOutcome;
use super::context::restore_prepaint_by_tab;
use super::context::restore_render_tabs;
use super::context::take_prepaint_by_tab;
use super::context::take_render_tabs;
use super::context::with_prepaint_by_tab;
use super::context::with_render_tabs;
use super::prepaint::PrepaintOverlay;
use super::prepaint::clear_all_prepaint_tracked_with_close_summary;
use super::prepaint::close_prepaint_overlay;
use super::prepaint::retained_prepaint_overlay_after_close;
use super::window_pool;
use super::window_pool::CompactRenderWindowsSummary;
use crate::host::NamespaceId;
use crate::host::TabHandle;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct ClearActiveRenderWindowsSummary {
    pub(crate) had_visible_windows_before_clear: bool,
    pub(crate) pruned_windows: usize,
    pub(crate) hidden_windows: usize,
    pub(crate) invalid_removed_windows: usize,
}

impl ClearActiveRenderWindowsSummary {
    pub(crate) fn had_visual_change(self) -> bool {
        self.had_visible_windows_before_clear
            && (self.pruned_windows > 0
                || self.hidden_windows > 0
                || self.invalid_removed_windows > 0)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct PurgeRenderWindowsSummary {
    pub(crate) had_visible_render_windows_before_purge: bool,
    pub(crate) had_visible_prepaint_before_purge: bool,
    pub(crate) purged_windows: usize,
    pub(crate) cleared_prepaint_overlays: usize,
    pub(crate) closed_visible_prepaint_overlays: usize,
    pub(crate) retained_render_windows: usize,
    pub(crate) retained_prepaint_overlays: usize,
    pub(crate) closed_orphan_windows: usize,
    pub(crate) retained_orphan_resources: usize,
}

impl PurgeRenderWindowsSummary {
    pub(crate) fn had_visual_change(self) -> bool {
        let closed_render_windows = self
            .purged_windows
            .saturating_sub(self.retained_render_windows);
        (self.had_visible_render_windows_before_purge && closed_render_windows > 0)
            || (self.had_visible_prepaint_before_purge && self.closed_visible_prepaint_overlays > 0)
            || self.closed_orphan_windows > 0
    }

    pub(crate) fn retained_resources(self) -> usize {
        self.retained_render_windows
            .saturating_add(self.retained_prepaint_overlays)
            .saturating_add(self.retained_orphan_resources)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct PruneClosedWindowResourcesSummary {
    pub(crate) pruned_render_windows: usize,
    pub(crate) cleared_prepaint_overlays: usize,
    pub(crate) retained_render_windows: usize,
    pub(crate) retained_prepaint_overlays: usize,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct PruneStaleTabResourcesSummary {
    pub(crate) pruned_render_tabs: usize,
    pub(crate) purged_render_windows: usize,
    pub(crate) cleared_prepaint_overlays: usize,
    pub(crate) retained_render_windows: usize,
    pub(crate) retained_prepaint_overlays: usize,
}

impl PruneClosedWindowResourcesSummary {
    pub(crate) fn retained_resources(self) -> usize {
        self.retained_render_windows
            .saturating_add(self.retained_prepaint_overlays)
    }
}

impl PruneStaleTabResourcesSummary {
    pub(crate) fn retained_resources(self) -> usize {
        self.retained_render_windows
            .saturating_add(self.retained_prepaint_overlays)
    }
}

fn evict_empty_render_tab_entries(render_tabs: &mut HashMap<TabHandle, window_pool::TabWindows>) {
    render_tabs.retain(|_, tab_windows| {
        window_pool::tab_pool_snapshot_from_tab(tab_windows).total_windows > 0
    });
}

fn summarize_tracked_purge_state(
    render_tabs: &HashMap<TabHandle, window_pool::TabWindows>,
    prepaint_by_tab: &HashMap<TabHandle, PrepaintOverlay>,
) -> PurgeRenderWindowsSummary {
    let mut summary = PurgeRenderWindowsSummary {
        had_visible_prepaint_before_purge: prepaint_by_tab
            .values()
            .any(|overlay| overlay.placement.is_some()),
        cleared_prepaint_overlays: prepaint_by_tab.len(),
        ..PurgeRenderWindowsSummary::default()
    };

    for tab_windows in render_tabs.values() {
        summary.had_visible_render_windows_before_purge = summary
            .had_visible_render_windows_before_purge
            || window_pool::tab_has_visible_windows(tab_windows);
        summary.purged_windows = summary
            .purged_windows
            .saturating_add(window_pool::tab_pool_snapshot_from_tab(tab_windows).total_windows);
    }

    summary
}

pub(crate) fn clear_active_render_windows(
    namespace_id: NamespaceId,
    max_kept_windows: usize,
) -> ClearActiveRenderWindowsSummary {
    with_render_tabs(|render_tabs| {
        let mut summary = ClearActiveRenderWindowsSummary::default();
        let mut tab_handles = render_tabs.keys().copied().collect::<Vec<_>>();
        tab_handles.sort_unstable();

        for tab_handle in tab_handles {
            let Some(tab_windows) = render_tabs.get_mut(&tab_handle) else {
                continue;
            };
            let tab_summary = {
                let had_visible_windows_before_clear =
                    window_pool::tab_has_visible_windows(tab_windows);
                if !window_pool::tab_has_pending_clear_work(tab_windows, max_kept_windows) {
                    ClearActiveRenderWindowsSummary {
                        had_visible_windows_before_clear,
                        ..ClearActiveRenderWindowsSummary::default()
                    }
                } else {
                    window_pool::begin_cleanup_frame(tab_windows);
                    let pruned_windows =
                        window_pool::prune_tab(tab_windows, namespace_id, max_kept_windows);
                    let closed_windows =
                        window_pool::close_shell_visible_tab(tab_windows, namespace_id);
                    let release_summary =
                        window_pool::release_unused_in_tab(tab_windows, namespace_id);
                    ClearActiveRenderWindowsSummary {
                        had_visible_windows_before_clear,
                        pruned_windows: pruned_windows.saturating_add(closed_windows),
                        hidden_windows: release_summary.hidden_windows,
                        invalid_removed_windows: release_summary.invalid_removed_windows,
                    }
                }
            };
            summary.had_visible_windows_before_clear = summary.had_visible_windows_before_clear
                || tab_summary.had_visible_windows_before_clear;
            summary.pruned_windows = summary
                .pruned_windows
                .saturating_add(tab_summary.pruned_windows);
            summary.hidden_windows = summary
                .hidden_windows
                .saturating_add(tab_summary.hidden_windows);
            summary.invalid_removed_windows = summary
                .invalid_removed_windows
                .saturating_add(tab_summary.invalid_removed_windows);
        }

        evict_empty_render_tab_entries(render_tabs);
        summary
    })
}

pub(crate) fn prune_closed_window_resources(
    namespace_id: NamespaceId,
    window_id: i32,
) -> PruneClosedWindowResourcesSummary {
    let (pruned_render_windows, retained_render_windows) = with_render_tabs(|render_tabs| {
        let mut close_summary = TrackedResourceCloseSummary::default();
        let mut tab_handles = render_tabs.keys().copied().collect::<Vec<_>>();
        tab_handles.sort_unstable();

        for tab_handle in tab_handles {
            let Some(tab_windows) = render_tabs.get_mut(&tab_handle) else {
                continue;
            };
            let tab_close_summary =
                window_pool::remove_window_in_tab(tab_windows, namespace_id, window_id);
            close_summary.closed_or_gone = close_summary
                .closed_or_gone
                .saturating_add(tab_close_summary.closed_or_gone);
            close_summary.retained = close_summary
                .retained
                .saturating_add(tab_close_summary.retained);
        }

        evict_empty_render_tab_entries(render_tabs);
        (close_summary.closed_or_gone, close_summary.retained)
    });

    let (cleared_prepaint_overlays, retained_prepaint_overlays) =
        with_prepaint_by_tab(|prepaint_by_tab| {
            let mut tab_handles = prepaint_by_tab
                .iter()
                .filter_map(|(tab_handle, overlay)| {
                    (overlay.window_id == window_id).then_some(*tab_handle)
                })
                .collect::<Vec<_>>();
            tab_handles.sort_unstable();

            let mut cleared_prepaint_overlays = 0_usize;
            let mut retained_prepaint_overlays = 0_usize;
            for tab_handle in tab_handles {
                let Some(overlay) = prepaint_by_tab.remove(&tab_handle) else {
                    continue;
                };
                let outcome = close_prepaint_overlay(namespace_id, overlay);
                if outcome.should_retain() {
                    retained_prepaint_overlays = retained_prepaint_overlays.saturating_add(1);
                    prepaint_by_tab.insert(
                        tab_handle,
                        retained_prepaint_overlay_after_close(overlay, outcome),
                    );
                } else {
                    cleared_prepaint_overlays = cleared_prepaint_overlays.saturating_add(1);
                }
            }
            (cleared_prepaint_overlays, retained_prepaint_overlays)
        });

    PruneClosedWindowResourcesSummary {
        pruned_render_windows,
        cleared_prepaint_overlays,
        retained_render_windows,
        retained_prepaint_overlays,
    }
}

fn sorted_unique_tab_handles(tab_handles: &[TabHandle]) -> Vec<TabHandle> {
    let mut tab_handles = tab_handles.to_vec();
    tab_handles.sort_unstable();
    tab_handles.dedup();
    tab_handles
}

pub(crate) fn prune_stale_tab_resources(
    namespace_id: NamespaceId,
    stale_tab_handles: &[TabHandle],
) -> PruneStaleTabResourcesSummary {
    let stale_tab_handles = sorted_unique_tab_handles(stale_tab_handles);
    let (pruned_render_tabs, purged_render_windows, retained_render_windows) =
        with_render_tabs(|render_tabs| {
            let mut pruned_render_tabs = 0_usize;
            let mut purged_render_windows = 0_usize;
            let mut retained_render_windows = 0_usize;

            for tab_handle in stale_tab_handles.iter() {
                let Some(mut tab_windows) = render_tabs.remove(tab_handle) else {
                    continue;
                };
                let close_summary = window_pool::purge_tab(&mut tab_windows, namespace_id);
                purged_render_windows =
                    purged_render_windows.saturating_add(close_summary.closed_or_gone);
                retained_render_windows =
                    retained_render_windows.saturating_add(close_summary.retained);
                if window_pool::tab_pool_snapshot_from_tab(&tab_windows).total_windows > 0 {
                    render_tabs.insert(*tab_handle, tab_windows);
                } else {
                    pruned_render_tabs = pruned_render_tabs.saturating_add(1);
                }
            }

            (
                pruned_render_tabs,
                purged_render_windows,
                retained_render_windows,
            )
        });

    let (cleared_prepaint_overlays, retained_prepaint_overlays) =
        with_prepaint_by_tab(|prepaint_by_tab| {
            let mut cleared_prepaint_overlays = 0_usize;
            let mut retained_prepaint_overlays = 0_usize;

            for tab_handle in stale_tab_handles {
                let Some(overlay) = prepaint_by_tab.remove(&tab_handle) else {
                    continue;
                };
                let outcome = close_prepaint_overlay(namespace_id, overlay);
                if outcome.should_retain() {
                    retained_prepaint_overlays = retained_prepaint_overlays.saturating_add(1);
                    prepaint_by_tab.insert(
                        tab_handle,
                        retained_prepaint_overlay_after_close(overlay, outcome),
                    );
                } else {
                    cleared_prepaint_overlays = cleared_prepaint_overlays.saturating_add(1);
                }
            }

            (cleared_prepaint_overlays, retained_prepaint_overlays)
        });

    PruneStaleTabResourcesSummary {
        pruned_render_tabs,
        purged_render_windows,
        cleared_prepaint_overlays,
        retained_render_windows,
        retained_prepaint_overlays,
    }
}

pub(crate) fn compact_render_windows(
    namespace_id: NamespaceId,
    target_budget: usize,
    max_prune_per_tick: usize,
) -> CompactRenderWindowsSummary {
    with_render_tabs(|render_tabs| {
        let summary = window_pool::compact_tabs_to_budget(
            render_tabs,
            namespace_id,
            target_budget,
            max_prune_per_tick,
        );
        evict_empty_render_tab_entries(render_tabs);
        summary
    })
}

fn purge_tracked_resources_with_closers<P, C>(
    render_tabs: &mut HashMap<TabHandle, window_pool::TabWindows>,
    prepaint_by_tab: &mut HashMap<TabHandle, PrepaintOverlay>,
    namespace_id: NamespaceId,
    purge_tab: &mut P,
    close_prepaint_overlay: &mut C,
) -> PurgeRenderWindowsSummary
where
    P: FnMut(&mut window_pool::TabWindows, NamespaceId) -> TrackedResourceCloseSummary,
    C: FnMut(NamespaceId, PrepaintOverlay) -> TrackedWindowBufferCloseOutcome,
{
    let mut summary = summarize_tracked_purge_state(render_tabs, prepaint_by_tab);
    let (prepaint_summary, _) = clear_all_prepaint_tracked_with_close_summary(
        prepaint_by_tab,
        namespace_id,
        close_prepaint_overlay,
    );
    summary.closed_visible_prepaint_overlays = prepaint_summary.closed_visible_prepaint_overlays;
    summary.retained_prepaint_overlays = prepaint_summary.retained_prepaint_overlays;

    let mut tab_handles = render_tabs.keys().copied().collect::<Vec<_>>();
    tab_handles.sort_unstable();
    for tab_handle in tab_handles {
        if let Some(tab_windows) = render_tabs.get_mut(&tab_handle) {
            let render_close_summary = purge_tab(tab_windows, namespace_id);
            summary.retained_render_windows = summary
                .retained_render_windows
                .saturating_add(render_close_summary.retained);
        }
    }
    evict_empty_render_tab_entries(render_tabs);
    summary
}

fn purge_tracked_resources(
    render_tabs: &mut HashMap<TabHandle, window_pool::TabWindows>,
    prepaint_by_tab: &mut HashMap<TabHandle, PrepaintOverlay>,
    namespace_id: NamespaceId,
) -> PurgeRenderWindowsSummary {
    let mut purge_tab = |tab_windows: &mut window_pool::TabWindows, namespace_id| {
        window_pool::purge_tab(tab_windows, namespace_id)
    };
    let mut close_prepaint_overlay = close_prepaint_overlay;
    purge_tracked_resources_with_closers(
        render_tabs,
        prepaint_by_tab,
        namespace_id,
        &mut purge_tab,
        &mut close_prepaint_overlay,
    )
}

pub(crate) fn purge_render_windows(namespace_id: NamespaceId) -> PurgeRenderWindowsSummary {
    let mut render_tabs = take_render_tabs();
    let mut prepaint_by_tab = take_prepaint_by_tab();
    let mut summary = purge_tracked_resources(&mut render_tabs, &mut prepaint_by_tab, namespace_id);
    restore_render_tabs(render_tabs);
    restore_prepaint_by_tab(prepaint_by_tab);
    // Hard purge is the terminal draw reset. Sweep any host objects that escaped tracking after a
    // failed mutation so disable/recovery never depends on bookkeeping remaining intact.
    let orphan_summary = window_pool::close_orphan_smear_resources(namespace_id);
    summary.closed_orphan_windows = orphan_summary.closed_windows;
    summary.retained_orphan_resources = orphan_summary.retained_resources;
    summary
}

pub(crate) fn recover_all_namespaces(namespace_id: NamespaceId) {
    let _ = purge_render_windows(namespace_id);
    let _ = super::apply::clear_namespace_all_buffers(namespace_id);
}

#[cfg(test)]
mod tests {
    use super::ClearActiveRenderWindowsSummary;
    use super::PruneClosedWindowResourcesSummary;
    use super::PruneStaleTabResourcesSummary;
    use super::PurgeRenderWindowsSummary;
    use super::clear_active_render_windows;
    use super::evict_empty_render_tab_entries;
    use super::prune_closed_window_resources;
    use super::prune_stale_tab_resources;
    use super::purge_tracked_resources_with_closers;
    use super::summarize_tracked_purge_state;
    use crate::draw::TrackedResourceCloseOutcome;
    use crate::draw::TrackedResourceCloseSummary;
    use crate::draw::TrackedWindowBufferCloseOutcome;
    use crate::draw::context::render_tab_handles_for_test;
    use crate::draw::context::restore_render_tabs;
    use crate::draw::context::take_render_tabs_for_test;
    use crate::draw::context::with_render_tab;
    use crate::draw::prepaint::PrepaintOverlay;
    use crate::draw::prepaint::PrepaintPlacement;
    use crate::draw::prepaint::insert_prepaint_overlay_for_test;
    use crate::draw::prepaint::prepaint_snapshot_for_test;
    use crate::draw::test_support::with_isolated_draw_context;
    use crate::draw::window_pool;
    use crate::draw::window_pool::TabPoolSnapshot;
    use crate::draw::window_pool::WindowBufferHandle;
    use crate::draw::window_pool::WindowPlacement;
    use crate::host::BufferHandle;
    use crate::host::NamespaceId;
    use crate::host::TabHandle;
    use pretty_assertions::assert_eq;
    use std::collections::HashMap;

    fn tab_handle(value: i32) -> TabHandle {
        TabHandle::from_raw_for_test(value)
    }

    #[test]
    fn clear_active_render_windows_evicts_empty_tab_registry_entries() {
        with_isolated_draw_context(|| {
            with_render_tab(tab_handle(17), |tab_windows| {
                tab_windows.cache_payload(77, 707)
            });
            assert_eq!(render_tab_handles_for_test(), vec![tab_handle(17)]);

            assert_eq!(
                clear_active_render_windows(
                    NamespaceId::new(/*value*/ 99),
                    /*max_kept_windows*/ 32,
                ),
                ClearActiveRenderWindowsSummary::default()
            );
            assert!(
                render_tab_handles_for_test().is_empty(),
                "soft cleanup should evict empty tab bookkeeping instead of retaining dead metadata"
            );
        });
    }

    #[test]
    fn empty_render_tab_eviction_preserves_tabs_with_retained_windows() {
        with_isolated_draw_context(|| {
            with_render_tab(tab_handle(7), |_| {});
            with_render_tab(tab_handle(9), |tab_windows| {
                tab_windows.push_test_visible_window(
                    WindowBufferHandle {
                        window_id: -9,
                        buffer_id: BufferHandle::from_raw_for_test(/*value*/ -19),
                    },
                    crate::draw::window_pool::WindowPlacement {
                        row: 2,
                        col: 3,
                        width: 1,
                        zindex: 80,
                    },
                    2,
                );
            });

            let mut render_tabs = take_render_tabs_for_test()
                .into_iter()
                .collect::<std::collections::HashMap<_, _>>();

            evict_empty_render_tab_entries(&mut render_tabs);

            let mut handles = render_tabs.keys().copied().collect::<Vec<_>>();
            handles.sort_unstable();
            assert_eq!(handles, vec![tab_handle(9)]);
        });
    }

    #[test]
    fn closed_window_pruning_removes_only_exact_render_and_prepaint_resources() {
        with_isolated_draw_context(|| {
            let placement = crate::draw::window_pool::WindowPlacement {
                row: 2,
                col: 3,
                width: 1,
                zindex: 80,
            };
            with_render_tab(tab_handle(17), |tab_windows| {
                tab_windows.push_test_visible_window(
                    WindowBufferHandle {
                        window_id: -41,
                        buffer_id: BufferHandle::from_raw_for_test(/*value*/ -141),
                    },
                    placement,
                    1,
                );
                tab_windows.push_test_visible_window(
                    WindowBufferHandle {
                        window_id: -42,
                        buffer_id: BufferHandle::from_raw_for_test(/*value*/ -142),
                    },
                    placement,
                    2,
                );
                tab_windows.cache_payload(-41, 410);
                tab_windows.cache_payload(-42, 420);
            });
            insert_prepaint_overlay_for_test(
                tab_handle(17),
                PrepaintOverlay {
                    window_id: -42,
                    buffer_id: BufferHandle::from_raw_for_test(/*value*/ -242),
                    placement: None,
                },
            );
            insert_prepaint_overlay_for_test(
                tab_handle(29),
                PrepaintOverlay {
                    window_id: -99,
                    buffer_id: BufferHandle::from_raw_for_test(/*value*/ -299),
                    placement: None,
                },
            );

            let summary = prune_closed_window_resources(
                NamespaceId::new(/*value*/ 99),
                /*window_id*/ -42,
            );

            assert_eq!(
                summary,
                PruneClosedWindowResourcesSummary {
                    pruned_render_windows: 1,
                    cleared_prepaint_overlays: 1,
                    retained_render_windows: 0,
                    retained_prepaint_overlays: 0,
                }
            );
            assert!(with_render_tab(tab_handle(17), |tab_windows| tab_windows
                .cached_payload_matches(-41, 410)));
            assert!(!with_render_tab(tab_handle(17), |tab_windows| tab_windows
                .cached_payload_matches(-42, 420)));
            assert_eq!(
                prepaint_snapshot_for_test()
                    .keys()
                    .copied()
                    .collect::<Vec<_>>(),
                vec![tab_handle(29)]
            );
        });
    }

    #[test]
    fn stale_tab_pruning_purges_render_and_prepaint_state_for_missing_tab_handles() {
        with_isolated_draw_context(|| {
            let placement = crate::draw::window_pool::WindowPlacement {
                row: 3,
                col: 4,
                width: 2,
                zindex: 120,
            };
            with_render_tab(tab_handle(11), |tab_windows| {
                tab_windows.push_test_visible_window(
                    WindowBufferHandle {
                        window_id: -11,
                        buffer_id: BufferHandle::from_raw_for_test(/*value*/ -111),
                    },
                    placement,
                    1,
                );
            });
            with_render_tab(tab_handle(22), |tab_windows| {
                tab_windows.push_test_visible_window(
                    WindowBufferHandle {
                        window_id: -22,
                        buffer_id: BufferHandle::from_raw_for_test(/*value*/ -122),
                    },
                    placement,
                    2,
                );
            });
            insert_prepaint_overlay_for_test(
                tab_handle(22),
                PrepaintOverlay {
                    window_id: -122,
                    buffer_id: BufferHandle::from_raw_for_test(/*value*/ -222),
                    placement: None,
                },
            );
            insert_prepaint_overlay_for_test(
                tab_handle(33),
                PrepaintOverlay {
                    window_id: -133,
                    buffer_id: BufferHandle::from_raw_for_test(/*value*/ -233),
                    placement: None,
                },
            );

            let summary = prune_stale_tab_resources(
                NamespaceId::new(/*value*/ 99),
                &[tab_handle(22), tab_handle(22), tab_handle(44)],
            );

            assert_eq!(
                summary,
                PruneStaleTabResourcesSummary {
                    pruned_render_tabs: 1,
                    purged_render_windows: 1,
                    cleared_prepaint_overlays: 1,
                    retained_render_windows: 0,
                    retained_prepaint_overlays: 0,
                }
            );
            assert_eq!(render_tab_handles_for_test(), vec![tab_handle(11)]);
            assert_eq!(
                prepaint_snapshot_for_test()
                    .keys()
                    .copied()
                    .collect::<Vec<_>>(),
                vec![tab_handle(33)]
            );
        });
    }

    #[test]
    fn tracked_purge_retains_failed_render_and_prepaint_resources_for_retry() {
        let retained_render_handles = WindowBufferHandle {
            window_id: 17,
            buffer_id: BufferHandle::from_raw_for_test(/*value*/ 117),
        };
        let released_render_handles = WindowBufferHandle {
            window_id: 18,
            buffer_id: BufferHandle::from_raw_for_test(/*value*/ 118),
        };
        let placement = WindowPlacement {
            row: 3,
            col: 4,
            width: 2,
            zindex: 120,
        };
        let mut tab_windows = window_pool::TabWindows::default();
        tab_windows.push_test_visible_window(retained_render_handles, placement, 1);
        tab_windows.push_test_visible_window(released_render_handles, placement, 2);
        let mut render_tabs = HashMap::from([(tab_handle(7), tab_windows)]);

        let retained_prepaint_overlay = PrepaintOverlay {
            window_id: 27,
            buffer_id: BufferHandle::from_raw_for_test(/*value*/ 127),
            placement: None,
        };
        let released_prepaint_overlay = PrepaintOverlay {
            window_id: 28,
            buffer_id: BufferHandle::from_raw_for_test(/*value*/ 128),
            placement: None,
        };
        let mut prepaint_by_tab = HashMap::from([
            (tab_handle(7), retained_prepaint_overlay),
            (tab_handle(8), released_prepaint_overlay),
        ]);

        let mut close_render_window = |_: NamespaceId, handles: WindowBufferHandle| {
            if handles == retained_render_handles {
                TrackedResourceCloseOutcome::Retained
            } else {
                TrackedResourceCloseOutcome::ClosedOrGone
            }
        };
        let mut purge_tab = |tab_windows: &mut window_pool::TabWindows, namespace_id| {
            window_pool::purge_tab_with_closer(tab_windows, namespace_id, &mut close_render_window)
        };
        let mut close_prepaint_overlay = |_: NamespaceId, overlay: PrepaintOverlay| {
            if overlay == retained_prepaint_overlay {
                TrackedWindowBufferCloseOutcome::new(
                    TrackedResourceCloseOutcome::Retained,
                    TrackedResourceCloseOutcome::Retained,
                )
            } else {
                TrackedWindowBufferCloseOutcome::closed_or_gone()
            }
        };

        let summary = purge_tracked_resources_with_closers(
            &mut render_tabs,
            &mut prepaint_by_tab,
            NamespaceId::new(/*value*/ 99),
            &mut purge_tab,
            &mut close_prepaint_overlay,
        );

        assert_eq!(
            summary,
            PurgeRenderWindowsSummary {
                had_visible_render_windows_before_purge: true,
                had_visible_prepaint_before_purge: false,
                purged_windows: 2,
                cleared_prepaint_overlays: 2,
                closed_visible_prepaint_overlays: 0,
                retained_render_windows: 1,
                retained_prepaint_overlays: 1,
                closed_orphan_windows: 0,
                retained_orphan_resources: 0,
            }
        );
        assert_eq!(
            render_tabs
                .get(&tab_handle(7))
                .map(window_pool::tab_pool_snapshot_from_tab),
            Some(TabPoolSnapshot {
                total_windows: 1,
                available_windows: 0,
                in_use_windows: 0,
                cached_budget: crate::draw::window_pool::ADAPTIVE_POOL_MIN_BUDGET,
                last_frame_demand: 0,
                peak_total_windows: 1,
                peak_frame_demand: 0,
                peak_requested_capacity: 0,
                capacity_cap_hits: 0,
            })
        );
        assert_eq!(
            prepaint_by_tab,
            HashMap::from([(tab_handle(7), retained_prepaint_overlay)])
        );
    }

    #[test]
    fn tracked_purge_reports_visual_change_when_visible_prepaint_window_closes_but_buffer_retains()
    {
        let retained_prepaint_overlay = PrepaintOverlay {
            window_id: 27,
            buffer_id: BufferHandle::from_raw_for_test(/*value*/ 127),
            placement: Some(PrepaintPlacement {
                cell: crate::position::ScreenCell::new(3, 4)
                    .expect("test prepaint cell should be in bounds"),
                zindex: 120,
            }),
        };
        let mut render_tabs = HashMap::new();
        let mut prepaint_by_tab = HashMap::from([(tab_handle(7), retained_prepaint_overlay)]);
        let mut purge_tab =
            |_: &mut window_pool::TabWindows, _: NamespaceId| -> TrackedResourceCloseSummary {
                TrackedResourceCloseSummary::default()
            };
        let mut close_prepaint_overlay = |_: NamespaceId, overlay: PrepaintOverlay| {
            assert_eq!(overlay, retained_prepaint_overlay);
            TrackedWindowBufferCloseOutcome::new(
                TrackedResourceCloseOutcome::ClosedOrGone,
                TrackedResourceCloseOutcome::Retained,
            )
        };

        let summary = purge_tracked_resources_with_closers(
            &mut render_tabs,
            &mut prepaint_by_tab,
            NamespaceId::new(/*value*/ 99),
            &mut purge_tab,
            &mut close_prepaint_overlay,
        );

        assert_eq!(
            summary,
            PurgeRenderWindowsSummary {
                had_visible_render_windows_before_purge: false,
                had_visible_prepaint_before_purge: true,
                purged_windows: 0,
                cleared_prepaint_overlays: 1,
                closed_visible_prepaint_overlays: 1,
                retained_render_windows: 0,
                retained_prepaint_overlays: 1,
                closed_orphan_windows: 0,
                retained_orphan_resources: 0,
            }
        );
        assert!(summary.had_visual_change());
        assert_eq!(
            prepaint_by_tab,
            HashMap::from([(
                tab_handle(7),
                PrepaintOverlay {
                    placement: None,
                    ..retained_prepaint_overlay
                }
            )])
        );
    }

    #[test]
    fn tracked_purge_summary_reads_visible_render_and_prepaint_state() {
        with_isolated_draw_context(|| {
            with_render_tab(tab_handle(17), |tab_windows| {
                tab_windows.push_test_visible_window(
                    WindowBufferHandle {
                        window_id: -17,
                        buffer_id: BufferHandle::from_raw_for_test(/*value*/ -117),
                    },
                    crate::draw::window_pool::WindowPlacement {
                        row: 3,
                        col: 4,
                        width: 2,
                        zindex: 120,
                    },
                    7,
                );
            });
            insert_prepaint_overlay_for_test(
                tab_handle(17),
                PrepaintOverlay {
                    window_id: -19,
                    buffer_id: BufferHandle::from_raw_for_test(/*value*/ -119),
                    placement: Some(PrepaintPlacement {
                        cell: crate::position::ScreenCell::new(3, 4)
                            .expect("test prepaint cell should be in bounds"),
                        zindex: 120,
                    }),
                },
            );

            let render_tabs = take_render_tabs_for_test()
                .into_iter()
                .collect::<std::collections::HashMap<_, _>>();
            let prepaint_by_tab = prepaint_snapshot_for_test();
            let summary = summarize_tracked_purge_state(&render_tabs, &prepaint_by_tab);
            restore_render_tabs(render_tabs);

            assert_eq!(
                summary,
                PurgeRenderWindowsSummary {
                    had_visible_render_windows_before_purge: true,
                    had_visible_prepaint_before_purge: true,
                    purged_windows: 1,
                    cleared_prepaint_overlays: 1,
                    closed_visible_prepaint_overlays: 0,
                    retained_render_windows: 0,
                    retained_prepaint_overlays: 0,
                    closed_orphan_windows: 0,
                    retained_orphan_resources: 0,
                }
            );
            assert!(summary.had_visual_change());
        });
    }

    #[test]
    fn hard_purge_summary_accounts_orphan_resource_results() {
        let summary = PurgeRenderWindowsSummary {
            closed_orphan_windows: 1,
            retained_orphan_resources: 2,
            ..PurgeRenderWindowsSummary::default()
        };

        assert!(summary.had_visual_change());
        assert_eq!(summary.retained_resources(), 2);
    }
}
