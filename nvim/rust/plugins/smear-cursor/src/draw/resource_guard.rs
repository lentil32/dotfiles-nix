use super::floating_windows::close_floating_window;
use super::floating_windows::delete_floating_buffer;
use super::prepaint::PrepaintOverlay;
use super::prepaint::PrepaintPlacement;
use super::prepaint::close_prepaint_overlay;
use super::prepaint::retained_prepaint_overlay_after_close;
use super::prepaint::valid_prepaint_handles;
use super::resource_close::TrackedWindowBufferCloseOutcome;
use crate::host::NamespaceId;
use crate::host::TabHandle;
use crate::host::api;
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
        if let Some(window) = self.window.take() {
            let _ = close_floating_window(window, self.close_window_context);
        }

        let Some(buffer) = self.buffer.take() else {
            return;
        };
        delete_floating_buffer(buffer, self.delete_buffer_context);
    }
}

pub(super) struct PrepaintOverlaySlot<'a> {
    prepaint_by_tab: &'a mut HashMap<TabHandle, PrepaintOverlay>,
    tab_handle: TabHandle,
    overlay: Option<PrepaintOverlay>,
}

impl<'a> PrepaintOverlaySlot<'a> {
    pub(super) fn detach(
        prepaint_by_tab: &'a mut HashMap<TabHandle, PrepaintOverlay>,
        tab_handle: TabHandle,
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

    pub(super) fn close_overlay(
        &mut self,
        namespace_id: NamespaceId,
    ) -> TrackedWindowBufferCloseOutcome {
        let mut close_overlay = close_prepaint_overlay;
        self.close_overlay_with_closer(namespace_id, &mut close_overlay)
    }

    fn close_overlay_with_closer<C>(
        &mut self,
        namespace_id: NamespaceId,
        close_overlay: &mut C,
    ) -> TrackedWindowBufferCloseOutcome
    where
        C: FnMut(NamespaceId, PrepaintOverlay) -> TrackedWindowBufferCloseOutcome,
    {
        let Some(overlay) = self.overlay.take() else {
            return TrackedWindowBufferCloseOutcome::closed_or_gone();
        };
        let outcome = close_overlay(namespace_id, overlay);
        if outcome.should_retain() {
            self.overlay = Some(retained_prepaint_overlay_after_close(overlay, outcome));
        }
        outcome
    }
}

impl Drop for PrepaintOverlaySlot<'_> {
    fn drop(&mut self) {
        if let Some(overlay) = self.overlay.take() {
            self.prepaint_by_tab.insert(self.tab_handle, overlay);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::PrepaintOverlaySlot;
    use crate::draw::TrackedResourceCloseOutcome;
    use crate::draw::TrackedWindowBufferCloseOutcome;
    use crate::draw::context::with_prepaint_by_tab;
    use crate::draw::prepaint::PrepaintOverlay;
    use crate::draw::prepaint::insert_prepaint_overlay_for_test;
    use crate::draw::prepaint::prepaint_snapshot_for_test;
    use crate::draw::test_support::with_isolated_draw_context;
    use crate::host::BufferHandle;
    use crate::host::NamespaceId;
    use crate::host::TabHandle;
    use pretty_assertions::assert_eq;
    use std::collections::HashMap;

    fn tab_handle(value: i32) -> TabHandle {
        TabHandle::from_raw_for_test(value)
    }

    #[test]
    fn prepaint_slot_restores_overlay_when_injected_close_retains() {
        with_isolated_draw_context(|| {
            let overlay = PrepaintOverlay {
                window_id: 19,
                buffer_id: BufferHandle::from_raw_for_test(/*value*/ 119),
                placement: None,
            };
            let mut close_calls = 0_usize;
            insert_prepaint_overlay_for_test(tab_handle(17), overlay);

            with_prepaint_by_tab(|prepaint_by_tab| {
                let mut slot = PrepaintOverlaySlot::detach(prepaint_by_tab, tab_handle(17));
                let mut close_overlay = |namespace_id, closed_overlay| {
                    assert_eq!(namespace_id, NamespaceId::new(/*value*/ 99));
                    assert_eq!(closed_overlay, overlay);
                    close_calls = close_calls.saturating_add(1);
                    TrackedWindowBufferCloseOutcome::new(
                        TrackedResourceCloseOutcome::Retained,
                        TrackedResourceCloseOutcome::Retained,
                    )
                };

                assert_eq!(
                    slot.close_overlay_with_closer(
                        NamespaceId::new(/*value*/ 99),
                        &mut close_overlay
                    )
                    .aggregate(),
                    TrackedResourceCloseOutcome::Retained
                );
            });

            assert_eq!(close_calls, 1);
            assert_eq!(
                prepaint_snapshot_for_test(),
                HashMap::from([(tab_handle(17), overlay)])
            );
        });
    }
}
