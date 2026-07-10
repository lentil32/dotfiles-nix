//! Bounded disposal of inactive-tab prepaint overlays during render Cooling.

use super::context::with_prepaint_by_tab;
use super::floating_windows::EventIgnoreGuard;
use super::prepaint::PrepaintOverlay;
use super::prepaint::close_prepaint_overlay_with_autocmds_suppressed;
use super::prepaint::retained_prepaint_overlay_after_close;
use super::resource_close::TrackedWindowBufferCloseOutcome;
use crate::host::NamespaceId;
use crate::host::NeovimHost;
use crate::host::TabHandle;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct CompactPrepaintOverlaysSummary {
    pub(super) attempted_overlays: usize,
    pub(super) cleared_overlays: usize,
    pub(super) retained_overlays: usize,
    pub(super) potential_visual_change: bool,
    pub(super) has_pending_work_after: bool,
}

fn compact_prepaint_tracked_with_closer<C>(
    prepaint_by_tab: &mut HashMap<TabHandle, PrepaintOverlay>,
    namespace_id: NamespaceId,
    max_attempts: usize,
    close_overlay: &mut C,
) -> CompactPrepaintOverlaysSummary
where
    C: FnMut(NamespaceId, PrepaintOverlay) -> TrackedWindowBufferCloseOutcome,
{
    let mut summary = CompactPrepaintOverlaysSummary::default();
    let mut tab_handles = prepaint_by_tab.keys().copied().collect::<Vec<_>>();
    tab_handles.sort_unstable();
    tab_handles.truncate(max_attempts);

    for tab_handle in tab_handles {
        let Some(overlay) = prepaint_by_tab.remove(&tab_handle) else {
            continue;
        };
        let was_visible = overlay.placement.is_some();
        let outcome = close_overlay(namespace_id, overlay);
        summary.attempted_overlays = summary.attempted_overlays.saturating_add(1);
        summary.potential_visual_change |= was_visible;
        if outcome.should_retain() {
            summary.retained_overlays = summary.retained_overlays.saturating_add(1);
            prepaint_by_tab.insert(
                tab_handle,
                retained_prepaint_overlay_after_close(overlay, outcome),
            );
            break;
        }
        summary.cleared_overlays = summary.cleared_overlays.saturating_add(1);
    }

    summary.has_pending_work_after = !prepaint_by_tab.is_empty();
    summary
}

pub(super) fn compact_prepaint_overlays(
    namespace_id: NamespaceId,
    max_attempts: usize,
) -> CompactPrepaintOverlaysSummary {
    if namespace_id.is_global() || max_attempts == 0 {
        return with_prepaint_by_tab(|prepaint_by_tab| CompactPrepaintOverlaysSummary {
            has_pending_work_after: !prepaint_by_tab.is_empty(),
            ..CompactPrepaintOverlaysSummary::default()
        });
    }

    let host = NeovimHost;
    with_prepaint_by_tab(|prepaint_by_tab| {
        let should_attempt_close = !prepaint_by_tab.is_empty();
        let _event_ignore = should_attempt_close.then(|| EventIgnoreGuard::set_all_with(&host));
        let mut close_overlay = |namespace_id, overlay| {
            close_prepaint_overlay_with_autocmds_suppressed(&host, namespace_id, overlay)
        };
        compact_prepaint_tracked_with_closer(
            prepaint_by_tab,
            namespace_id,
            max_attempts,
            &mut close_overlay,
        )
    })
}

#[cfg(test)]
mod tests {
    use super::CompactPrepaintOverlaysSummary;
    use super::compact_prepaint_tracked_with_closer;
    use crate::draw::TrackedResourceCloseOutcome;
    use crate::draw::TrackedWindowBufferCloseOutcome;
    use crate::draw::prepaint::PrepaintOverlay;
    use crate::draw::prepaint::PrepaintPlacement;
    use crate::host::BufferHandle;
    use crate::host::NamespaceId;
    use crate::host::TabHandle;
    use pretty_assertions::assert_eq;
    use std::collections::HashMap;

    fn tab_handle(value: i32) -> TabHandle {
        TabHandle::from_raw_for_test(value)
    }

    #[test]
    fn cooling_prepaint_compaction_is_bounded_and_retains_the_unprocessed_tail() {
        let overlays = (1..=5)
            .map(|raw| {
                (
                    tab_handle(raw),
                    PrepaintOverlay {
                        window_id: raw,
                        buffer_id: BufferHandle::from_raw_for_test(i64::from(raw + 100)),
                        placement: None,
                    },
                )
            })
            .collect::<HashMap<_, _>>();
        let mut prepaint_by_tab = overlays;
        let mut attempted_tabs = Vec::new();
        let mut close_overlay = |_: NamespaceId, overlay: PrepaintOverlay| {
            attempted_tabs.push(overlay.window_id);
            TrackedWindowBufferCloseOutcome::closed_or_gone()
        };

        let summary = compact_prepaint_tracked_with_closer(
            &mut prepaint_by_tab,
            NamespaceId::new(/*value*/ 99),
            /*max_attempts*/ 2,
            &mut close_overlay,
        );

        assert_eq!(
            (summary, attempted_tabs, prepaint_by_tab.len()),
            (
                CompactPrepaintOverlaysSummary {
                    attempted_overlays: 2,
                    cleared_overlays: 2,
                    retained_overlays: 0,
                    potential_visual_change: false,
                    has_pending_work_after: true,
                },
                vec![1, 2],
                3,
            ),
        );
    }

    #[test]
    fn cooling_prepaint_compaction_stops_after_retained_partial_close() {
        let visible = PrepaintOverlay {
            window_id: 17,
            buffer_id: BufferHandle::from_raw_for_test(/*value*/ 117),
            placement: Some(PrepaintPlacement {
                cell: crate::position::ScreenCell::new(3, 4)
                    .expect("test prepaint cell should be in bounds"),
                zindex: 120,
            }),
        };
        let hidden = PrepaintOverlay {
            window_id: 23,
            buffer_id: BufferHandle::from_raw_for_test(/*value*/ 123),
            placement: None,
        };
        let mut prepaint_by_tab =
            HashMap::from([(tab_handle(17), visible), (tab_handle(23), hidden)]);
        let mut close_calls = 0_usize;
        let mut close_overlay = |_: NamespaceId, overlay: PrepaintOverlay| {
            close_calls = close_calls.saturating_add(1);
            assert_eq!(overlay, visible);
            TrackedWindowBufferCloseOutcome::new(
                TrackedResourceCloseOutcome::ClosedOrGone,
                TrackedResourceCloseOutcome::Retained,
            )
        };

        let summary = compact_prepaint_tracked_with_closer(
            &mut prepaint_by_tab,
            NamespaceId::new(/*value*/ 99),
            /*max_attempts*/ 8,
            &mut close_overlay,
        );

        assert_eq!(
            (summary, close_calls, prepaint_by_tab),
            (
                CompactPrepaintOverlaysSummary {
                    attempted_overlays: 1,
                    cleared_overlays: 0,
                    retained_overlays: 1,
                    potential_visual_change: true,
                    has_pending_work_after: true,
                },
                1,
                HashMap::from([
                    (
                        tab_handle(17),
                        PrepaintOverlay {
                            placement: None,
                            ..visible
                        },
                    ),
                    (tab_handle(23), hidden),
                ]),
            ),
        );
    }
}
