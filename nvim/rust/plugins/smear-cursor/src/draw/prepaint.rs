//! Cursor prepaint overlay lifecycle and bookkeeping.

use super::constants::PREPAINT_BUFFER_FILETYPE;
use super::constants::PREPAINT_BUFFER_TYPE;
use super::constants::PREPAINT_EXTMARK_ID;
use super::constants::PREPAINT_HIGHLIGHT_GROUP;
use super::context::with_prepaint_by_tab;
use super::floating_windows::FloatingWindowPlacement;
use super::floating_windows::FloatingWindowVisibility;
use super::floating_windows::clear_namespace_and_hide_floating_window_with;
use super::floating_windows::close_floating_window_with;
use super::floating_windows::delete_floating_buffer_with;
use super::floating_windows::initialize_floating_buffer_options_with;
use super::floating_windows::initialize_floating_window_options_with;
use super::floating_windows::open_floating_window_config;
use super::floating_windows::reconfigure_floating_window_config;
use super::floating_windows::set_existing_floating_window_config_with;
use super::resource_close::TrackedResourceCloseOutcome;
use super::resource_close::TrackedWindowBufferCloseOutcome;
use super::resource_close::TrackedWindowBufferCloseSummary;
use super::resource_guard::PrepaintOverlaySlot;
use super::resource_guard::StagedFloatingWindow;
use crate::host::BufferHandle;
use crate::host::DrawResourcePort;
use crate::host::FloatingWindowEnter;
use crate::host::NamespaceId;
use crate::host::NeovimHost;
use crate::host::TabHandle;
use crate::host::api;
use crate::host::api::opts::OptionScope;
use crate::host::api::opts::SetExtmarkOpts;
use crate::host::api::types::ExtmarkVirtTextPosition;
use crate::host::api::types::WindowRelativeTo;
use crate::host::api::types::WindowStyle;
use crate::position::ScreenCell;
use crate::types::CursorCellShape;
use nvim_oxi::Result;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct ClearPrepaintOverlaysSummary {
    pub(crate) had_visible_prepaint_before_clear: bool,
    pub(crate) cleared_prepaint_overlays: usize,
    pub(crate) closed_visible_prepaint_overlays: usize,
    pub(crate) retained_prepaint_overlays: usize,
}

impl ClearPrepaintOverlaysSummary {
    pub(crate) fn had_visual_change(self) -> bool {
        self.closed_visible_prepaint_overlays > 0
    }

    pub(crate) fn retained_resources(self) -> usize {
        self.retained_prepaint_overlays
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct PrepaintPlacement {
    pub(super) cell: ScreenCell,
    pub(super) zindex: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PrepaintOverlay {
    pub(super) window_id: i32,
    pub(super) buffer_id: BufferHandle,
    pub(super) placement: Option<PrepaintPlacement>,
}

fn floating_window_placement(placement: PrepaintPlacement) -> FloatingWindowPlacement {
    FloatingWindowPlacement {
        row: placement.cell.row(),
        col: placement.cell.col(),
        width: 1,
        zindex: placement.zindex,
    }
}

pub(super) fn close_prepaint_overlay(
    namespace_id: NamespaceId,
    overlay: PrepaintOverlay,
) -> TrackedWindowBufferCloseOutcome {
    let host = NeovimHost;
    let mut buffer = host.valid_buffer(overlay.buffer_id);
    if let Some(existing_buffer) = buffer.as_mut()
        && let Err(err) = host.clear_buffer_namespace(existing_buffer, namespace_id)
    {
        super::context::log_draw_error("clear prepaint namespace", &err);
    }
    let window = if let Some(window) = host.valid_window_i32(overlay.window_id) {
        close_floating_window_with(&host, window, "close prepaint overlay window")
    } else {
        TrackedResourceCloseOutcome::ClosedOrGone
    };
    let buffer = if let Some(buffer) = buffer.take() {
        delete_floating_buffer_with(&host, buffer, "delete prepaint overlay buffer")
    } else {
        TrackedResourceCloseOutcome::ClosedOrGone
    };
    TrackedWindowBufferCloseOutcome::new(window, buffer)
}

pub(super) fn valid_prepaint_handles(
    overlay: PrepaintOverlay,
) -> Option<(api::Window, api::Buffer)> {
    let host = NeovimHost;
    let window = host.valid_window_i32(overlay.window_id)?;
    let buffer = host.valid_buffer(overlay.buffer_id)?;
    Some((window, buffer))
}

pub(super) fn retained_prepaint_overlay_after_close(
    overlay: PrepaintOverlay,
    outcome: TrackedWindowBufferCloseOutcome,
) -> PrepaintOverlay {
    if outcome.window_closed_or_gone() {
        PrepaintOverlay {
            placement: None,
            ..overlay
        }
    } else {
        overlay
    }
}

fn create_prepaint_overlay(
    placement: PrepaintPlacement,
) -> Result<(PrepaintOverlay, api::Window, api::Buffer)> {
    let host = NeovimHost;
    let staged = StagedFloatingWindow::new(
        host.create_scratch_buffer()?,
        "delete staged prepaint buffer",
        "close staged prepaint window",
    );
    initialize_floating_buffer_options_with(
        &host,
        staged.buffer(),
        PREPAINT_BUFFER_TYPE,
        PREPAINT_BUFFER_FILETYPE,
    )?;
    let config = open_floating_window_config(
        floating_window_placement(placement),
        FloatingWindowVisibility::Visible,
        WindowRelativeTo::Editor,
        WindowStyle::Minimal,
    );
    let window =
        host.open_floating_window(staged.buffer(), FloatingWindowEnter::DoNotEnter, &config)?;
    let attached = staged.attach_window(window);
    initialize_floating_window_options_with(&host, attached.window(), OptionScope::Local)?;
    let (window, buffer) = attached.into_window_and_buffer();

    let overlay = PrepaintOverlay {
        window_id: window.handle(),
        buffer_id: BufferHandle::from_buffer(&buffer),
        placement: Some(placement),
    };
    Ok((overlay, window, buffer))
}

fn hide_prepaint_overlay(namespace_id: NamespaceId, overlay: &mut PrepaintOverlay) -> bool {
    if overlay.placement.is_none() {
        return true;
    }

    let host = NeovimHost;
    let Some(mut buffer) = host.valid_buffer(overlay.buffer_id) else {
        return false;
    };
    let Some(mut window) = host.valid_window_i32(overlay.window_id) else {
        return false;
    };

    if clear_namespace_and_hide_floating_window_with(
        &host,
        namespace_id,
        &mut buffer,
        &mut window,
        "clear prepaint overlay namespace before hide",
        "hide prepaint overlay window",
    )
    .is_err()
    {
        return false;
    }
    overlay.placement = None;
    true
}

fn close_prepaint_slot_and_record(
    overlay_slot: &mut PrepaintOverlaySlot<'_>,
    namespace_id: NamespaceId,
    close_summary: &mut TrackedWindowBufferCloseSummary,
) -> TrackedWindowBufferCloseOutcome {
    let had_overlay = overlay_slot.overlay().is_some();
    let outcome = overlay_slot.close_overlay(namespace_id);
    if had_overlay {
        close_summary.record(outcome);
    }
    outcome
}

pub(crate) fn prepaint_cursor_cell(
    namespace_id: NamespaceId,
    cell: ScreenCell,
    shape: CursorCellShape,
    zindex: u32,
) -> TrackedWindowBufferCloseSummary {
    if namespace_id.is_global() {
        return TrackedWindowBufferCloseSummary::default();
    }

    let host = NeovimHost;
    let tab_handle = super::apply::current_tab_handle();
    let requested_placement = PrepaintPlacement { cell, zindex };

    with_prepaint_by_tab(|prepaint_by_tab| {
        let mut close_summary = TrackedWindowBufferCloseSummary::default();
        let mut overlay_slot = PrepaintOverlaySlot::detach(prepaint_by_tab, tab_handle);
        let mut handles_pair = overlay_slot.valid_handles();

        if handles_pair.is_none() {
            let outcome =
                close_prepaint_slot_and_record(&mut overlay_slot, namespace_id, &mut close_summary);
            if outcome.should_retain() {
                return close_summary;
            }
            match create_prepaint_overlay(requested_placement) {
                Ok((created_overlay, window, buffer)) => {
                    overlay_slot.replace(created_overlay);
                    handles_pair = Some((window, buffer));
                }
                Err(err) => {
                    // Prepaint is non-critical: keep cursor callback non-fatal.
                    super::context::log_draw_error("create prepaint overlay", &err);
                    return close_summary;
                }
            }
        }

        let Some((mut window, mut buffer)) = handles_pair else {
            return close_summary;
        };

        if overlay_slot
            .overlay()
            .is_some_and(|entry| entry.placement != Some(requested_placement))
            && let Err(err) = set_existing_floating_window_config_with(
                &host,
                &mut window,
                reconfigure_floating_window_config(
                    floating_window_placement(requested_placement),
                    FloatingWindowVisibility::Visible,
                    WindowRelativeTo::Editor,
                    WindowStyle::Minimal,
                ),
            )
        {
            super::context::log_draw_error("reconfigure prepaint overlay window", &err);

            let outcome =
                close_prepaint_slot_and_record(&mut overlay_slot, namespace_id, &mut close_summary);
            if outcome.should_retain() {
                return close_summary;
            }
            match create_prepaint_overlay(requested_placement) {
                Ok((created_overlay, _recreated_window, recreated_buffer)) => {
                    overlay_slot.replace(created_overlay);
                    buffer = recreated_buffer;
                }
                Err(recreate_err) => {
                    super::context::log_draw_error(
                        "recreate prepaint overlay after reconfigure failure",
                        &recreate_err,
                    );
                    return close_summary;
                }
            }
        }

        let extmark_opts = SetExtmarkOpts::builder()
            .id(PREPAINT_EXTMARK_ID)
            .virt_text([(shape.glyph(), PREPAINT_HIGHLIGHT_GROUP)])
            .virt_text_pos(ExtmarkVirtTextPosition::Overlay)
            .virt_text_win_col(0)
            .build();
        if let Err(err) = host.set_buffer_extmark(&mut buffer, namespace_id, 0, 0, &extmark_opts) {
            super::context::log_draw_error("set prepaint overlay payload", &err);
            close_prepaint_slot_and_record(&mut overlay_slot, namespace_id, &mut close_summary);
            return close_summary;
        }

        overlay_slot.set_placement(requested_placement);
        close_summary
    })
}

pub(crate) fn clear_prepaint_for_current_tab(
    namespace_id: NamespaceId,
) -> TrackedWindowBufferCloseSummary {
    if namespace_id.is_global() {
        return TrackedWindowBufferCloseSummary::default();
    }

    let tab_handle = super::apply::current_tab_handle();
    with_prepaint_by_tab(|prepaint_by_tab| {
        let mut close_summary = TrackedWindowBufferCloseSummary::default();
        let mut overlay_slot = PrepaintOverlaySlot::detach(prepaint_by_tab, tab_handle);
        let hide_succeeded = match overlay_slot.overlay_mut() {
            Some(entry) => hide_prepaint_overlay(namespace_id, entry),
            None => return close_summary,
        };
        if hide_succeeded {
            return close_summary;
        }
        close_prepaint_slot_and_record(&mut overlay_slot, namespace_id, &mut close_summary);
        close_summary
    })
}

pub(super) fn clear_all_prepaint_tracked(
    prepaint_by_tab: &mut HashMap<TabHandle, PrepaintOverlay>,
    namespace_id: NamespaceId,
) -> ClearPrepaintOverlaysSummary {
    let mut close_overlay = close_prepaint_overlay;
    clear_all_prepaint_tracked_with_closer(prepaint_by_tab, namespace_id, &mut close_overlay)
}

pub(super) fn clear_all_prepaint_tracked_with_closer<C>(
    prepaint_by_tab: &mut HashMap<TabHandle, PrepaintOverlay>,
    namespace_id: NamespaceId,
    close_overlay: &mut C,
) -> ClearPrepaintOverlaysSummary
where
    C: FnMut(NamespaceId, PrepaintOverlay) -> TrackedWindowBufferCloseOutcome,
{
    clear_all_prepaint_tracked_with_close_summary(prepaint_by_tab, namespace_id, close_overlay).0
}

pub(super) fn clear_all_prepaint_tracked_with_close_summary<C>(
    prepaint_by_tab: &mut HashMap<TabHandle, PrepaintOverlay>,
    namespace_id: NamespaceId,
    close_overlay: &mut C,
) -> (
    ClearPrepaintOverlaysSummary,
    TrackedWindowBufferCloseSummary,
)
where
    C: FnMut(NamespaceId, PrepaintOverlay) -> TrackedWindowBufferCloseOutcome,
{
    let drained_prepaint_by_tab = std::mem::take(prepaint_by_tab);
    let mut summary = ClearPrepaintOverlaysSummary {
        had_visible_prepaint_before_clear: drained_prepaint_by_tab
            .values()
            .any(|overlay| overlay.placement.is_some()),
        cleared_prepaint_overlays: drained_prepaint_by_tab.len(),
        closed_visible_prepaint_overlays: 0,
        retained_prepaint_overlays: 0,
    };
    let mut close_summary = TrackedWindowBufferCloseSummary::default();
    let mut entries = drained_prepaint_by_tab.into_iter().collect::<Vec<_>>();
    entries.sort_unstable_by_key(|(tab_handle, _)| *tab_handle);
    for (tab_handle, overlay) in entries {
        let was_visible = overlay.placement.is_some();
        let outcome = close_overlay(namespace_id, overlay);
        close_summary.record(outcome);
        if was_visible && outcome.window_closed_or_gone() {
            summary.closed_visible_prepaint_overlays =
                summary.closed_visible_prepaint_overlays.saturating_add(1);
        }
        if outcome.should_retain() {
            summary.retained_prepaint_overlays =
                summary.retained_prepaint_overlays.saturating_add(1);
            prepaint_by_tab.insert(
                tab_handle,
                retained_prepaint_overlay_after_close(overlay, outcome),
            );
        }
    }
    (summary, close_summary)
}

pub(crate) fn clear_all_prepaint_overlays(
    namespace_id: NamespaceId,
) -> ClearPrepaintOverlaysSummary {
    if namespace_id.is_global() {
        return ClearPrepaintOverlaysSummary::default();
    }

    with_prepaint_by_tab(|prepaint_by_tab| {
        clear_all_prepaint_tracked(prepaint_by_tab, namespace_id)
    })
}

#[cfg(test)]
pub(super) fn insert_prepaint_overlay_for_test(tab_handle: TabHandle, overlay: PrepaintOverlay) {
    with_prepaint_by_tab(|prepaint_by_tab| {
        prepaint_by_tab.insert(tab_handle, overlay);
    });
}

#[cfg(test)]
pub(super) fn prepaint_count_for_test() -> usize {
    with_prepaint_by_tab(|prepaint_by_tab| prepaint_by_tab.len())
}

#[cfg(test)]
pub(super) fn prepaint_snapshot_for_test() -> HashMap<TabHandle, PrepaintOverlay> {
    with_prepaint_by_tab(|prepaint_by_tab| prepaint_by_tab.clone())
}

#[cfg(test)]
mod tests {
    use super::ClearPrepaintOverlaysSummary;
    use super::PrepaintOverlay;
    use super::PrepaintPlacement;
    use super::clear_all_prepaint_overlays;
    use super::clear_all_prepaint_tracked_with_close_summary;
    use super::clear_all_prepaint_tracked_with_closer;
    use super::insert_prepaint_overlay_for_test;
    use super::prepaint_count_for_test;
    use super::prepaint_snapshot_for_test;
    use crate::draw::TrackedResourceCloseOutcome;
    use crate::draw::TrackedWindowBufferCloseOutcome;
    use crate::draw::TrackedWindowBufferCloseSummary;
    use crate::draw::context::with_render_tab;
    use crate::draw::test_support::with_isolated_draw_context;
    use crate::host::BufferHandle;
    use crate::host::NamespaceId;
    use crate::host::TabHandle;
    use crate::types::CursorCellShape;
    use pretty_assertions::assert_eq;
    use std::collections::HashMap;

    fn tab_handle(value: i32) -> TabHandle {
        TabHandle::from_raw_for_test(value)
    }

    #[test]
    fn cursor_shape_prepaint_glyphs_match_cursor_geometry() {
        insta::assert_snapshot!(
            [
                CursorCellShape::Block,
                CursorCellShape::VerticalBar,
                CursorCellShape::HorizontalBar,
            ]
            .into_iter()
            .map(|shape| format!("{shape:?}={}", shape.glyph()))
            .collect::<Vec<_>>()
            .join("\n"),
            @r"
        Block=█
        VerticalBar=▏
        HorizontalBar=▁
        "
        );
    }

    #[test]
    fn clearing_all_prepaint_overlays_does_not_touch_render_tab_state() {
        with_isolated_draw_context(|| {
            with_render_tab(tab_handle(17), |tab_windows| {
                tab_windows.cache_payload(77, 707)
            });
            insert_prepaint_overlay_for_test(
                tab_handle(17),
                PrepaintOverlay {
                    window_id: -1,
                    buffer_id: BufferHandle::from_raw_for_test(/*value*/ -1),
                    placement: None,
                },
            );
            insert_prepaint_overlay_for_test(
                tab_handle(23),
                PrepaintOverlay {
                    window_id: -2,
                    buffer_id: BufferHandle::from_raw_for_test(/*value*/ -2),
                    placement: None,
                },
            );
            let summary = clear_all_prepaint_overlays(NamespaceId::new(/*value*/ 99));

            assert_eq!(
                summary,
                ClearPrepaintOverlaysSummary {
                    had_visible_prepaint_before_clear: false,
                    cleared_prepaint_overlays: 2,
                    closed_visible_prepaint_overlays: 0,
                    retained_prepaint_overlays: 0,
                }
            );
            assert_eq!(prepaint_count_for_test(), 0);

            assert!(with_render_tab(tab_handle(17), |tab_windows| tab_windows
                .cached_payload_matches(77, 707)));
        });
    }

    #[test]
    fn clearing_all_prepaint_overlays_reports_visible_overlay_changes() {
        with_isolated_draw_context(|| {
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

            let summary = clear_all_prepaint_overlays(NamespaceId::new(/*value*/ 99));

            assert_eq!(
                summary,
                ClearPrepaintOverlaysSummary {
                    had_visible_prepaint_before_clear: true,
                    cleared_prepaint_overlays: 1,
                    closed_visible_prepaint_overlays: 1,
                    retained_prepaint_overlays: 0,
                }
            );
            assert!(summary.had_visual_change());
            assert_eq!(prepaint_count_for_test(), 0);
        });
    }

    #[test]
    fn clearing_all_prepaint_tracked_retains_failed_overlay_close_for_retry() {
        let retained_overlay = PrepaintOverlay {
            window_id: 19,
            buffer_id: BufferHandle::from_raw_for_test(/*value*/ 119),
            placement: Some(PrepaintPlacement {
                cell: crate::position::ScreenCell::new(3, 4)
                    .expect("test prepaint cell should be in bounds"),
                zindex: 120,
            }),
        };
        let released_overlay = PrepaintOverlay {
            window_id: 29,
            buffer_id: BufferHandle::from_raw_for_test(/*value*/ 129),
            placement: None,
        };
        let mut prepaint_by_tab = HashMap::from([
            (tab_handle(17), retained_overlay),
            (tab_handle(23), released_overlay),
        ]);
        let mut close_overlay = |_: NamespaceId, overlay: PrepaintOverlay| {
            if overlay == retained_overlay {
                TrackedWindowBufferCloseOutcome::new(
                    TrackedResourceCloseOutcome::Retained,
                    TrackedResourceCloseOutcome::Retained,
                )
            } else {
                TrackedWindowBufferCloseOutcome::closed_or_gone()
            }
        };

        let summary = clear_all_prepaint_tracked_with_closer(
            &mut prepaint_by_tab,
            NamespaceId::new(/*value*/ 99),
            &mut close_overlay,
        );

        assert_eq!(
            summary,
            ClearPrepaintOverlaysSummary {
                had_visible_prepaint_before_clear: true,
                cleared_prepaint_overlays: 2,
                closed_visible_prepaint_overlays: 0,
                retained_prepaint_overlays: 1,
            }
        );
        assert!(!summary.had_visual_change());
        assert_eq!(
            prepaint_by_tab,
            HashMap::from([(tab_handle(17), retained_overlay)])
        );
    }

    #[test]
    fn clearing_all_prepaint_tracked_reports_visual_change_when_window_closes_but_buffer_retains() {
        let retained_overlay = PrepaintOverlay {
            window_id: 19,
            buffer_id: BufferHandle::from_raw_for_test(/*value*/ 119),
            placement: Some(PrepaintPlacement {
                cell: crate::position::ScreenCell::new(3, 4)
                    .expect("test prepaint cell should be in bounds"),
                zindex: 120,
            }),
        };
        let mut prepaint_by_tab = HashMap::from([(tab_handle(17), retained_overlay)]);
        let mut close_overlay = |_: NamespaceId, overlay: PrepaintOverlay| {
            assert_eq!(overlay, retained_overlay);
            TrackedWindowBufferCloseOutcome::new(
                TrackedResourceCloseOutcome::ClosedOrGone,
                TrackedResourceCloseOutcome::Retained,
            )
        };

        let (summary, close_summary) = clear_all_prepaint_tracked_with_close_summary(
            &mut prepaint_by_tab,
            NamespaceId::new(/*value*/ 99),
            &mut close_overlay,
        );

        assert_eq!(
            summary,
            ClearPrepaintOverlaysSummary {
                had_visible_prepaint_before_clear: true,
                cleared_prepaint_overlays: 1,
                closed_visible_prepaint_overlays: 1,
                retained_prepaint_overlays: 1,
            }
        );
        assert_eq!(
            close_summary,
            TrackedWindowBufferCloseSummary {
                window_closed_or_gone: 1,
                window_retained: 0,
                buffer_closed_or_gone: 0,
                buffer_retained: 1,
            }
        );
        assert!(summary.had_visual_change());
        assert_eq!(
            prepaint_by_tab,
            HashMap::from([(
                tab_handle(17),
                PrepaintOverlay {
                    placement: None,
                    ..retained_overlay
                }
            )])
        );
    }

    #[test]
    fn detached_prepaint_slot_restores_overlay_when_transaction_aborts() {
        with_isolated_draw_context(|| {
            let overlay = PrepaintOverlay {
                window_id: -19,
                buffer_id: BufferHandle::from_raw_for_test(/*value*/ -119),
                placement: None,
            };
            insert_prepaint_overlay_for_test(tab_handle(17), overlay);

            crate::draw::context::with_prepaint_by_tab(|prepaint_by_tab| {
                let _slot = crate::draw::resource_guard::PrepaintOverlaySlot::detach(
                    prepaint_by_tab,
                    tab_handle(17),
                );
            });

            assert_eq!(
                prepaint_snapshot_for_test(),
                HashMap::from([(tab_handle(17), overlay)])
            );
        });
    }

    #[test]
    fn detached_prepaint_slot_drops_tracking_after_overlay_close() {
        with_isolated_draw_context(|| {
            insert_prepaint_overlay_for_test(
                tab_handle(17),
                PrepaintOverlay {
                    window_id: -19,
                    buffer_id: BufferHandle::from_raw_for_test(/*value*/ -119),
                    placement: None,
                },
            );

            crate::draw::context::with_prepaint_by_tab(|prepaint_by_tab| {
                let mut slot = crate::draw::resource_guard::PrepaintOverlaySlot::detach(
                    prepaint_by_tab,
                    tab_handle(17),
                );
                assert_eq!(
                    slot.close_overlay(NamespaceId::new(/*value*/ 99))
                        .aggregate(),
                    TrackedResourceCloseOutcome::ClosedOrGone
                );
            });

            assert_eq!(prepaint_snapshot_for_test(), HashMap::new());
        });
    }
}
