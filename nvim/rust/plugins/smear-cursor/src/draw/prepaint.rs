//! Cursor prepaint overlay lifecycle and bookkeeping.

use super::constants::PREPAINT_BUFFER_FILETYPE;
use super::constants::PREPAINT_BUFFER_TYPE;
use super::constants::PREPAINT_EXTMARK_ID;
use super::constants::PREPAINT_HIGHLIGHT_GROUP;
use super::context::with_prepaint_by_tab;
use super::floating_windows::FloatingWindowPlacement;
use super::floating_windows::clear_namespace_and_hide_floating_window;
use super::floating_windows::delete_floating_buffer;
use super::floating_windows::initialize_floating_buffer_options;
use super::floating_windows::initialize_floating_window_options;
use super::floating_windows::open_floating_window_config;
use super::floating_windows::reconfigure_floating_window_config;
use super::floating_windows::set_existing_floating_window_config;
use super::resource_guard::PrepaintOverlaySlot;
use super::resource_guard::StagedFloatingWindow;
use crate::position::ScreenCell;
use crate::types::CursorCellShape;
use nvim_oxi::Result;
use nvim_oxi::api;
use nvim_oxi::api::opts::OptionScope;
use nvim_oxi::api::opts::SetExtmarkOpts;
use nvim_oxi::api::types::ExtmarkVirtTextPosition;
use nvim_oxi::api::types::WindowRelativeTo;
use nvim_oxi::api::types::WindowStyle;
use nvimrs_nvim_oxi_utils::handles;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct ClearPrepaintOverlaysSummary {
    pub(crate) had_visible_prepaint_before_clear: bool,
    pub(crate) cleared_prepaint_overlays: usize,
}

impl ClearPrepaintOverlaysSummary {
    pub(crate) fn had_visual_change(self) -> bool {
        self.had_visible_prepaint_before_clear && self.cleared_prepaint_overlays > 0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct PrepaintPlacement {
    pub(super) cell: ScreenCell,
    pub(super) zindex: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct PrepaintOverlay {
    pub(super) window_id: i32,
    pub(super) buffer_id: i32,
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

pub(super) fn close_prepaint_overlay(namespace_id: u32, overlay: PrepaintOverlay) {
    let mut buffer = handles::valid_buffer(i64::from(overlay.buffer_id));
    if let Some(existing_buffer) = buffer.as_mut()
        && let Err(err) = existing_buffer.clear_namespace(namespace_id, 0..)
    {
        super::context::log_draw_error("clear prepaint namespace", &err);
    }
    if let Some(window) = handles::valid_window(i64::from(overlay.window_id))
        && let Err(err) = window.close(true)
    {
        super::context::log_draw_error("close prepaint overlay window", &err);
    }
    if let Some(buffer) = buffer.take() {
        delete_floating_buffer(buffer, "delete prepaint overlay buffer");
    }
}

pub(super) fn valid_prepaint_handles(
    overlay: PrepaintOverlay,
) -> Option<(api::Window, api::Buffer)> {
    let window = handles::valid_window(i64::from(overlay.window_id))?;
    let buffer = handles::valid_buffer(i64::from(overlay.buffer_id))?;
    Some((window, buffer))
}

fn create_prepaint_overlay(
    placement: PrepaintPlacement,
) -> Result<(PrepaintOverlay, api::Window, api::Buffer)> {
    let staged = StagedFloatingWindow::new(
        api::create_buf(false, true)?,
        "delete staged prepaint buffer",
        "close staged prepaint window",
    );
    initialize_floating_buffer_options(
        staged.buffer(),
        PREPAINT_BUFFER_TYPE,
        PREPAINT_BUFFER_FILETYPE,
    )?;
    let config = open_floating_window_config(
        floating_window_placement(placement),
        false,
        WindowRelativeTo::Editor,
        WindowStyle::Minimal,
    );
    let window = api::open_win(staged.buffer(), false, &config)?;
    let attached = staged.attach_window(window);
    initialize_floating_window_options(attached.window(), OptionScope::Local)?;
    let (window, buffer) = attached.into_window_and_buffer();

    let overlay = PrepaintOverlay {
        window_id: window.handle(),
        buffer_id: buffer.handle(),
        placement: Some(placement),
    };
    Ok((overlay, window, buffer))
}

fn hide_prepaint_overlay(namespace_id: u32, overlay: &mut PrepaintOverlay) -> bool {
    if overlay.placement.is_none() {
        return true;
    }

    let Some(mut buffer) = handles::valid_buffer(i64::from(overlay.buffer_id)) else {
        return false;
    };
    let Some(mut window) = handles::valid_window(i64::from(overlay.window_id)) else {
        return false;
    };

    if clear_namespace_and_hide_floating_window(
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

pub(crate) fn prepaint_cursor_cell(
    namespace_id: u32,
    cell: ScreenCell,
    shape: CursorCellShape,
    zindex: u32,
) {
    if namespace_id == 0 {
        return;
    }

    let tab_handle = super::apply::current_tab_handle();
    let requested_placement = PrepaintPlacement { cell, zindex };

    with_prepaint_by_tab(|prepaint_by_tab| {
        let mut overlay_slot = PrepaintOverlaySlot::detach(prepaint_by_tab, tab_handle);
        let mut handles_pair = overlay_slot.valid_handles();

        if handles_pair.is_none() {
            overlay_slot.close_overlay(namespace_id);
            match create_prepaint_overlay(requested_placement) {
                Ok((created_overlay, window, buffer)) => {
                    overlay_slot.replace(created_overlay);
                    handles_pair = Some((window, buffer));
                }
                Err(err) => {
                    // Prepaint is non-critical: keep cursor callback non-fatal.
                    super::context::log_draw_error("create prepaint overlay", &err);
                    return;
                }
            }
        }

        let Some((mut window, mut buffer)) = handles_pair else {
            return;
        };

        if overlay_slot
            .overlay()
            .is_some_and(|entry| entry.placement != Some(requested_placement))
            && let Err(err) = set_existing_floating_window_config(
                &mut window,
                reconfigure_floating_window_config(
                    floating_window_placement(requested_placement),
                    false,
                    WindowRelativeTo::Editor,
                    WindowStyle::Minimal,
                ),
            )
        {
            super::context::log_draw_error("reconfigure prepaint overlay window", &err);

            overlay_slot.close_overlay(namespace_id);
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
                    return;
                }
            }
        }

        let extmark_opts = SetExtmarkOpts::builder()
            .id(PREPAINT_EXTMARK_ID)
            .virt_text([(shape.glyph(), PREPAINT_HIGHLIGHT_GROUP)])
            .virt_text_pos(ExtmarkVirtTextPosition::Overlay)
            .virt_text_win_col(0)
            .build();
        if let Err(err) = buffer.set_extmark(namespace_id, 0, 0, &extmark_opts) {
            super::context::log_draw_error("set prepaint overlay payload", &err);
            overlay_slot.close_overlay(namespace_id);
            return;
        }

        overlay_slot.set_placement(requested_placement);
    });
}

pub(crate) fn clear_prepaint_for_current_tab(namespace_id: u32) {
    if namespace_id == 0 {
        return;
    }

    let tab_handle = super::apply::current_tab_handle();
    with_prepaint_by_tab(|prepaint_by_tab| {
        let mut overlay_slot = PrepaintOverlaySlot::detach(prepaint_by_tab, tab_handle);
        let hide_succeeded = match overlay_slot.overlay_mut() {
            Some(entry) => hide_prepaint_overlay(namespace_id, entry),
            None => return,
        };
        if hide_succeeded {
            return;
        }
        overlay_slot.close_overlay(namespace_id);
    });
}

pub(super) fn clear_all_prepaint_tracked(
    prepaint_by_tab: &mut HashMap<i32, PrepaintOverlay>,
    namespace_id: u32,
) -> ClearPrepaintOverlaysSummary {
    let prepaint_by_tab = std::mem::take(prepaint_by_tab);
    let summary = ClearPrepaintOverlaysSummary {
        had_visible_prepaint_before_clear: prepaint_by_tab
            .values()
            .any(|overlay| overlay.placement.is_some()),
        cleared_prepaint_overlays: prepaint_by_tab.len(),
    };
    for overlay in prepaint_by_tab.values().copied() {
        close_prepaint_overlay(namespace_id, overlay);
    }
    summary
}

pub(crate) fn clear_all_prepaint_overlays(namespace_id: u32) -> ClearPrepaintOverlaysSummary {
    if namespace_id == 0 {
        return ClearPrepaintOverlaysSummary::default();
    }

    with_prepaint_by_tab(|prepaint_by_tab| {
        clear_all_prepaint_tracked(prepaint_by_tab, namespace_id)
    })
}

#[cfg(test)]
pub(super) fn insert_prepaint_overlay_for_test(tab_handle: i32, overlay: PrepaintOverlay) {
    with_prepaint_by_tab(|prepaint_by_tab| {
        prepaint_by_tab.insert(tab_handle, overlay);
    });
}

#[cfg(test)]
pub(super) fn prepaint_count_for_test() -> usize {
    with_prepaint_by_tab(|prepaint_by_tab| prepaint_by_tab.len())
}

#[cfg(test)]
pub(super) fn prepaint_snapshot_for_test() -> HashMap<i32, PrepaintOverlay> {
    with_prepaint_by_tab(|prepaint_by_tab| prepaint_by_tab.clone())
}

#[cfg(test)]
mod tests {
    use super::ClearPrepaintOverlaysSummary;
    use super::PrepaintOverlay;
    use super::PrepaintPlacement;
    use super::clear_all_prepaint_overlays;
    use super::insert_prepaint_overlay_for_test;
    use super::prepaint_count_for_test;
    use super::prepaint_snapshot_for_test;
    use crate::draw::context::with_render_tab;
    use crate::draw::test_support::with_isolated_draw_context;
    use crate::types::CursorCellShape;
    use pretty_assertions::assert_eq;
    use std::collections::HashMap;

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
            with_render_tab(17, |tab_windows| tab_windows.cache_payload(77, 707));
            insert_prepaint_overlay_for_test(
                17,
                PrepaintOverlay {
                    window_id: -1,
                    buffer_id: -1,
                    placement: None,
                },
            );
            insert_prepaint_overlay_for_test(
                23,
                PrepaintOverlay {
                    window_id: -2,
                    buffer_id: -2,
                    placement: None,
                },
            );
            let summary = clear_all_prepaint_overlays(99);

            assert_eq!(
                summary,
                ClearPrepaintOverlaysSummary {
                    had_visible_prepaint_before_clear: false,
                    cleared_prepaint_overlays: 2,
                }
            );
            assert_eq!(prepaint_count_for_test(), 0);

            assert!(with_render_tab(17, |tab_windows| tab_windows
                .cached_payload_matches(77, 707)));
        });
    }

    #[test]
    fn clearing_all_prepaint_overlays_reports_visible_overlay_changes() {
        with_isolated_draw_context(|| {
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

            let summary = clear_all_prepaint_overlays(99);

            assert_eq!(
                summary,
                ClearPrepaintOverlaysSummary {
                    had_visible_prepaint_before_clear: true,
                    cleared_prepaint_overlays: 1,
                }
            );
            assert!(summary.had_visual_change());
            assert_eq!(prepaint_count_for_test(), 0);
        });
    }

    #[test]
    fn detached_prepaint_slot_restores_overlay_when_transaction_aborts() {
        with_isolated_draw_context(|| {
            let overlay = PrepaintOverlay {
                window_id: -19,
                buffer_id: -119,
                placement: None,
            };
            insert_prepaint_overlay_for_test(17, overlay);

            crate::draw::context::with_prepaint_by_tab(|prepaint_by_tab| {
                let _slot =
                    crate::draw::resource_guard::PrepaintOverlaySlot::detach(prepaint_by_tab, 17);
            });

            assert_eq!(prepaint_snapshot_for_test(), HashMap::from([(17, overlay)]));
        });
    }

    #[test]
    fn detached_prepaint_slot_drops_tracking_after_overlay_close() {
        with_isolated_draw_context(|| {
            insert_prepaint_overlay_for_test(
                17,
                PrepaintOverlay {
                    window_id: -19,
                    buffer_id: -119,
                    placement: None,
                },
            );

            crate::draw::context::with_prepaint_by_tab(|prepaint_by_tab| {
                let mut slot =
                    crate::draw::resource_guard::PrepaintOverlaySlot::detach(prepaint_by_tab, 17);
                slot.close_overlay(99);
            });

            assert_eq!(prepaint_snapshot_for_test(), HashMap::new());
        });
    }
}
