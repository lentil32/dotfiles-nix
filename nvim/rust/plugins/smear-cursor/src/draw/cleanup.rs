//! Render window cleanup, compaction, and hard-purge operations.

use super::context::take_prepaint_by_tab;
use super::context::take_render_tabs;
use super::context::with_render_tabs;
use super::prepaint::PrepaintOverlay;
use super::prepaint::clear_all_prepaint_tracked;
use super::window_pool;
use super::window_pool::CompactRenderWindowsSummary;
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
}

impl PurgeRenderWindowsSummary {
    pub(crate) fn had_visual_change(self) -> bool {
        (self.had_visible_render_windows_before_purge && self.purged_windows > 0)
            || (self.had_visible_prepaint_before_purge && self.cleared_prepaint_overlays > 0)
    }
}

fn evict_empty_render_tab_entries(render_tabs: &mut HashMap<i32, window_pool::TabWindows>) {
    render_tabs.retain(|_, tab_windows| {
        window_pool::tab_pool_snapshot_from_tab(tab_windows).total_windows > 0
    });
}

fn summarize_tracked_purge_state(
    render_tabs: &HashMap<i32, window_pool::TabWindows>,
    prepaint_by_tab: &HashMap<i32, PrepaintOverlay>,
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
    namespace_id: u32,
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

pub(crate) fn compact_render_windows(
    namespace_id: u32,
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

pub(crate) fn purge_render_windows(namespace_id: u32) -> PurgeRenderWindowsSummary {
    let mut render_tabs = take_render_tabs();
    let mut prepaint_by_tab = take_prepaint_by_tab();
    let summary = summarize_tracked_purge_state(&render_tabs, &prepaint_by_tab);
    let _ = clear_all_prepaint_tracked(&mut prepaint_by_tab, namespace_id);

    let mut tab_handles = render_tabs.keys().copied().collect::<Vec<_>>();
    tab_handles.sort_unstable();
    for tab_handle in tab_handles {
        if let Some(tab_windows) = render_tabs.get_mut(&tab_handle) {
            window_pool::purge_tab(tab_windows, namespace_id);
        }
    }
    // Hard purge is the terminal draw reset. Sweep any host objects that escaped tracking after a
    // failed mutation so disable/recovery never depends on bookkeeping remaining intact.
    let _ = window_pool::close_orphan_smear_windows(namespace_id);
    summary
}

pub(crate) fn recover_all_namespaces(namespace_id: u32) {
    let _ = purge_render_windows(namespace_id);
    let _ = super::apply::clear_namespace_all_buffers(namespace_id);
}

#[cfg(test)]
mod tests {
    use super::ClearActiveRenderWindowsSummary;
    use super::PurgeRenderWindowsSummary;
    use super::clear_active_render_windows;
    use super::evict_empty_render_tab_entries;
    use super::summarize_tracked_purge_state;
    use crate::draw::context::render_tab_handles_for_test;
    use crate::draw::context::restore_render_tabs;
    use crate::draw::context::take_render_tabs_for_test;
    use crate::draw::context::with_render_tab;
    use crate::draw::prepaint::PrepaintOverlay;
    use crate::draw::prepaint::PrepaintPlacement;
    use crate::draw::prepaint::insert_prepaint_overlay_for_test;
    use crate::draw::prepaint::prepaint_snapshot_for_test;
    use crate::draw::test_support::with_isolated_draw_context;
    use crate::draw::window_pool::WindowBufferHandle;
    use pretty_assertions::assert_eq;

    #[test]
    fn clear_active_render_windows_is_noop_without_tracked_state() {
        with_isolated_draw_context(|| {
            assert_eq!(
                clear_active_render_windows(99, 32),
                ClearActiveRenderWindowsSummary::default()
            );
        });
    }

    #[test]
    fn clear_active_render_windows_evicts_empty_tab_registry_entries() {
        with_isolated_draw_context(|| {
            with_render_tab(17, |tab_windows| tab_windows.cache_payload(77, 707));
            assert_eq!(render_tab_handles_for_test(), vec![17]);

            assert_eq!(
                clear_active_render_windows(99, 32),
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
            with_render_tab(7, |_| {});
            with_render_tab(9, |tab_windows| {
                tab_windows.push_test_visible_window(
                    WindowBufferHandle {
                        window_id: -9,
                        buffer_id: -19,
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
            assert_eq!(handles, vec![9]);
        });
    }

    #[test]
    fn tracked_purge_summary_reads_visible_render_and_prepaint_state() {
        with_isolated_draw_context(|| {
            with_render_tab(17, |tab_windows| {
                tab_windows.push_test_visible_window(
                    WindowBufferHandle {
                        window_id: -17,
                        buffer_id: -117,
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
                17,
                PrepaintOverlay {
                    window_id: -19,
                    buffer_id: -119,
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
                }
            );
            assert!(summary.had_visual_change());
        });
    }
}
