use super::PrepaintOverlay;
use super::PrepaintPlacement;
use super::close_prepaint_overlay;
use super::delete_floating_buffer;
use super::log_draw_error;
use super::valid_prepaint_handles;
use nvim_oxi::api;
use std::collections::HashMap;

pub(crate) struct StagedFloatingWindow {
    buffer: Option<api::Buffer>,
    window: Option<api::Window>,
    delete_buffer_context: &'static str,
    close_window_context: &'static str,
}

impl StagedFloatingWindow {
    pub(crate) fn new(
        buffer: api::Buffer,
        delete_buffer_context: &'static str,
        close_window_context: &'static str,
    ) -> Self {
        Self {
            buffer: Some(buffer),
            window: None,
            delete_buffer_context,
            close_window_context,
        }
    }

    pub(crate) fn attach_window(&mut self, window: api::Window) {
        debug_assert!(
            self.window.is_none(),
            "staged floating window already owns a window"
        );
        self.window = Some(window);
    }

    pub(crate) fn buffer(&self) -> &api::Buffer {
        self.buffer
            .as_ref()
            .expect("staged floating window should always own a buffer before commit")
    }

    pub(crate) fn window(&self) -> &api::Window {
        self.window
            .as_ref()
            .expect("staged floating window should own a window after attachment")
    }

    pub(crate) fn into_window_and_buffer(mut self) -> (api::Window, api::Buffer) {
        let window = self
            .window
            .take()
            .expect("staged floating window should own a window before commit");
        let buffer = self
            .buffer
            .take()
            .expect("staged floating window should own a buffer before commit");
        (window, buffer)
    }
}

impl Drop for StagedFloatingWindow {
    fn drop(&mut self) {
        if let Some(window) = self.window.take()
            && let Err(err) = window.close(true)
        {
            log_draw_error(self.close_window_context, &err);
        }

        let Some(buffer) = self.buffer.take() else {
            return;
        };
        delete_floating_buffer(buffer, self.delete_buffer_context);
    }
}

pub(super) struct PrepaintOverlaySlot<'a> {
    prepaint_by_tab: &'a mut HashMap<i32, PrepaintOverlay>,
    tab_handle: i32,
    overlay: Option<PrepaintOverlay>,
}

impl<'a> PrepaintOverlaySlot<'a> {
    pub(super) fn detach(
        prepaint_by_tab: &'a mut HashMap<i32, PrepaintOverlay>,
        tab_handle: i32,
    ) -> Self {
        Self {
            overlay: prepaint_by_tab.remove(&tab_handle),
            prepaint_by_tab,
            tab_handle,
        }
    }

    pub(super) fn overlay(&self) -> Option<PrepaintOverlay> {
        self.overlay
    }

    pub(super) fn overlay_mut(&mut self) -> Option<&mut PrepaintOverlay> {
        self.overlay.as_mut()
    }

    pub(super) fn valid_handles(&self) -> Option<(api::Window, api::Buffer)> {
        self.overlay.and_then(valid_prepaint_handles)
    }

    pub(super) fn replace(&mut self, overlay: PrepaintOverlay) {
        self.overlay = Some(overlay);
    }

    pub(super) fn set_placement(&mut self, placement: PrepaintPlacement) {
        if let Some(overlay) = self.overlay.as_mut() {
            overlay.placement = Some(placement);
        }
    }

    pub(super) fn close_overlay(&mut self, namespace_id: u32) {
        if let Some(overlay) = self.overlay.take() {
            close_prepaint_overlay(namespace_id, overlay);
        }
    }
}

impl Drop for PrepaintOverlaySlot<'_> {
    fn drop(&mut self) {
        if let Some(overlay) = self.overlay.take() {
            self.prepaint_by_tab.insert(self.tab_handle, overlay);
        }
    }
}
