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
    delete_buffer_context: &'static str,
    close_window_context: &'static str,
}

pub(crate) struct AttachedFloatingWindow {
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
            delete_buffer_context,
            close_window_context,
        }
    }

    pub(crate) fn buffer(&self) -> &api::Buffer {
        match self.buffer.as_ref() {
            Some(buffer) => buffer,
            None => unreachable!("staged floating window lost its buffer before attachment"),
        }
    }

    pub(crate) fn attach_window(mut self, window: api::Window) -> AttachedFloatingWindow {
        let buffer = match self.buffer.take() {
            Some(buffer) => buffer,
            None => unreachable!("staged floating window lost its buffer before attachment"),
        };
        AttachedFloatingWindow {
            buffer: Some(buffer),
            window: Some(window),
            delete_buffer_context: self.delete_buffer_context,
            close_window_context: self.close_window_context,
        }
    }
}

impl AttachedFloatingWindow {
    pub(crate) fn buffer(&self) -> &api::Buffer {
        match self.buffer.as_ref() {
            Some(buffer) => buffer,
            None => unreachable!("attached floating window lost its buffer before commit"),
        }
    }

    pub(crate) fn window(&self) -> &api::Window {
        match self.window.as_ref() {
            Some(window) => window,
            None => unreachable!("attached floating window lost its window before commit"),
        }
    }

    pub(crate) fn into_window_and_buffer(mut self) -> (api::Window, api::Buffer) {
        let window = match self.window.take() {
            Some(window) => window,
            None => unreachable!("attached floating window lost its window before commit"),
        };
        let buffer = match self.buffer.take() {
            Some(buffer) => buffer,
            None => unreachable!("attached floating window lost its buffer before commit"),
        };
        (window, buffer)
    }
}

impl Drop for StagedFloatingWindow {
    fn drop(&mut self) {
        let Some(buffer) = self.buffer.take() else {
            return;
        };
        delete_floating_buffer(buffer, self.delete_buffer_context);
    }
}

impl Drop for AttachedFloatingWindow {
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
