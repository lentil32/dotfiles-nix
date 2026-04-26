use super::BufferHandle;
use super::NeovimHost;
use super::api;

pub(crate) trait CurrentEditorPort {
    fn current_mode(&self) -> String;
    fn current_window(&self) -> api::Window;
    fn current_buffer(&self) -> api::Buffer;
    fn window_is_valid(&self, window: &api::Window) -> bool;
    fn buffer_is_valid(&self, buffer: &api::Buffer) -> bool;
    fn valid_window_from_handle(&self, handle: i64) -> Option<api::Window>;
    fn valid_buffer_from_handle(&self, handle: BufferHandle) -> Option<api::Buffer>;
}

impl CurrentEditorPort for NeovimHost {
    fn current_mode(&self) -> String {
        api::get_mode().mode.to_string_lossy().into_owned()
    }

    fn current_window(&self) -> api::Window {
        api::get_current_win()
    }

    fn current_buffer(&self) -> api::Buffer {
        api::get_current_buf()
    }

    fn window_is_valid(&self, window: &api::Window) -> bool {
        window.is_valid()
    }

    fn buffer_is_valid(&self, buffer: &api::Buffer) -> bool {
        buffer.is_valid()
    }

    fn valid_window_from_handle(&self, handle: i64) -> Option<api::Window> {
        let handle = i32::try_from(handle).ok()?;
        let window = api::Window::from(handle);
        self.window_is_valid(&window).then_some(window)
    }

    fn valid_buffer_from_handle(&self, handle: BufferHandle) -> Option<api::Buffer> {
        let handle = handle.as_i32()?;
        let buffer = api::Buffer::from(handle);
        self.buffer_is_valid(&buffer).then_some(buffer)
    }
}

#[cfg(test)]
#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) enum CurrentEditorCall {
    CurrentMode,
    CurrentWindow,
    CurrentBuffer,
    WindowIsValid { window_handle: i32 },
    BufferIsValid { buffer_handle: i32 },
    ValidWindowFromHandle { window_handle: i64 },
    ValidBufferFromHandle { buffer_handle: BufferHandle },
}

#[cfg(test)]
#[derive(Debug)]
pub(crate) struct FakeCurrentEditorPort {
    calls: std::cell::RefCell<Vec<CurrentEditorCall>>,
    mode: std::cell::RefCell<String>,
    current_window_handle: std::cell::Cell<i32>,
    current_buffer_handle: std::cell::Cell<i32>,
    window_validity: std::cell::RefCell<std::collections::HashMap<i32, bool>>,
    buffer_validity: std::cell::RefCell<std::collections::HashMap<i32, bool>>,
}

#[cfg(test)]
impl Default for FakeCurrentEditorPort {
    fn default() -> Self {
        Self {
            calls: std::cell::RefCell::new(Vec::new()),
            mode: std::cell::RefCell::new("n".to_string()),
            current_window_handle: std::cell::Cell::new(1),
            current_buffer_handle: std::cell::Cell::new(1),
            window_validity: std::cell::RefCell::new(std::collections::HashMap::new()),
            buffer_validity: std::cell::RefCell::new(std::collections::HashMap::new()),
        }
    }
}

#[cfg(test)]
impl FakeCurrentEditorPort {
    pub(crate) fn set_current_mode(&self, mode: &str) {
        *self.mode.borrow_mut() = mode.to_string();
    }

    pub(crate) fn set_current_window_handle(&self, handle: i32) {
        self.current_window_handle.set(handle);
    }

    pub(crate) fn set_current_buffer_handle(&self, handle: i32) {
        self.current_buffer_handle.set(handle);
    }

    pub(crate) fn set_window_validity(&self, handle: i32, valid: bool) {
        self.window_validity.borrow_mut().insert(handle, valid);
    }

    pub(crate) fn set_buffer_validity(&self, handle: i32, valid: bool) {
        self.buffer_validity.borrow_mut().insert(handle, valid);
    }

    pub(crate) fn calls(&self) -> Vec<CurrentEditorCall> {
        self.calls.borrow().clone()
    }

    fn record(&self, call: CurrentEditorCall) {
        self.calls.borrow_mut().push(call);
    }

    fn window_validity(&self, handle: i32) -> bool {
        self.window_validity
            .borrow()
            .get(&handle)
            .copied()
            .unwrap_or(handle > 0)
    }

    fn buffer_validity(&self, handle: i32) -> bool {
        self.buffer_validity
            .borrow()
            .get(&handle)
            .copied()
            .unwrap_or(handle > 0)
    }
}

#[cfg(test)]
impl CurrentEditorPort for FakeCurrentEditorPort {
    fn current_mode(&self) -> String {
        self.record(CurrentEditorCall::CurrentMode);
        self.mode.borrow().clone()
    }

    fn current_window(&self) -> api::Window {
        self.record(CurrentEditorCall::CurrentWindow);
        api::Window::from(self.current_window_handle.get())
    }

    fn current_buffer(&self) -> api::Buffer {
        self.record(CurrentEditorCall::CurrentBuffer);
        api::Buffer::from(self.current_buffer_handle.get())
    }

    fn window_is_valid(&self, window: &api::Window) -> bool {
        let handle = window.handle();
        self.record(CurrentEditorCall::WindowIsValid {
            window_handle: handle,
        });
        self.window_validity(handle)
    }

    fn buffer_is_valid(&self, buffer: &api::Buffer) -> bool {
        let handle = buffer.handle();
        self.record(CurrentEditorCall::BufferIsValid {
            buffer_handle: handle,
        });
        self.buffer_validity(handle)
    }

    fn valid_window_from_handle(&self, handle: i64) -> Option<api::Window> {
        self.record(CurrentEditorCall::ValidWindowFromHandle {
            window_handle: handle,
        });
        let handle = i32::try_from(handle).ok()?;
        self.window_validity(handle)
            .then(|| api::Window::from(handle))
    }

    fn valid_buffer_from_handle(&self, handle: BufferHandle) -> Option<api::Buffer> {
        self.record(CurrentEditorCall::ValidBufferFromHandle {
            buffer_handle: handle,
        });
        let handle = handle.as_i32()?;
        self.buffer_validity(handle)
            .then(|| api::Buffer::from(handle))
    }
}
