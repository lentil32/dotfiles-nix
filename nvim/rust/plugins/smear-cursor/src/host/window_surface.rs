use super::BufferHandle;
use super::NeovimHost;
use super::api;
use nvim_oxi::Array;
use nvim_oxi::Object;
use nvim_oxi::Result;

pub(crate) trait WindowSurfacePort {
    fn getwininfo(&self, window: &api::Window) -> Result<Object>;
    fn window_buffer_handle(&self, window: &api::Window) -> Result<BufferHandle>;
    fn window_text_height_rows(
        &self,
        window: &api::Window,
        start_row: usize,
        end_row: usize,
    ) -> i64;
}

impl WindowSurfacePort for NeovimHost {
    fn getwininfo(&self, window: &api::Window) -> Result<Object> {
        let args = Array::from_iter([Object::from(window.handle())]);
        Ok(api::call_function("getwininfo", args)?)
    }

    fn window_buffer_handle(&self, window: &api::Window) -> Result<BufferHandle> {
        Ok(BufferHandle::from_buffer(&window.get_buf()?))
    }

    fn window_text_height_rows(
        &self,
        window: &api::Window,
        start_row: usize,
        end_row: usize,
    ) -> i64 {
        let opts = api::opts::WinTextHeightOpts::builder()
            .start_row(start_row)
            .end_row(end_row)
            .build();
        window
            .text_height(&opts)
            .map_or(0, |height| i64::from(height.all).saturating_sub(1))
    }
}

#[cfg(test)]
#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) enum WindowSurfaceCall {
    Getwininfo {
        window_handle: i32,
    },
    WindowBufferHandle {
        window_handle: i32,
    },
    WindowTextHeightRows {
        window_handle: i32,
        start_row: usize,
        end_row: usize,
    },
}

#[cfg(test)]
#[derive(Debug, Default)]
pub(crate) struct FakeWindowSurfacePort {
    calls: std::cell::RefCell<Vec<WindowSurfaceCall>>,
    getwininfo_results: std::cell::RefCell<std::collections::VecDeque<Result<Object>>>,
    window_buffer_handle_results:
        std::cell::RefCell<std::collections::VecDeque<Result<BufferHandle>>>,
    window_text_height_rows_results: std::cell::RefCell<std::collections::VecDeque<i64>>,
}

#[cfg(test)]
impl FakeWindowSurfacePort {
    pub(crate) fn push_getwininfo(&self, result: Object) {
        self.getwininfo_results.borrow_mut().push_back(Ok(result));
    }

    pub(crate) fn push_window_buffer_handle(&self, handle: impl Into<BufferHandle>) {
        self.window_buffer_handle_results
            .borrow_mut()
            .push_back(Ok(handle.into()));
    }

    pub(crate) fn push_window_text_height_rows(&self, rows: i64) {
        self.window_text_height_rows_results
            .borrow_mut()
            .push_back(rows);
    }

    pub(crate) fn calls(&self) -> Vec<WindowSurfaceCall> {
        self.calls.borrow().clone()
    }

    fn record(&self, call: WindowSurfaceCall) {
        self.calls.borrow_mut().push(call);
    }
}

#[cfg(test)]
impl WindowSurfacePort for FakeWindowSurfacePort {
    fn getwininfo(&self, window: &api::Window) -> Result<Object> {
        self.record(WindowSurfaceCall::Getwininfo {
            window_handle: window.handle(),
        });
        super::pop_fake_response(&self.getwininfo_results, "getwininfo")
    }

    fn window_buffer_handle(&self, window: &api::Window) -> Result<BufferHandle> {
        self.record(WindowSurfaceCall::WindowBufferHandle {
            window_handle: window.handle(),
        });
        super::pop_fake_response(&self.window_buffer_handle_results, "window buffer handle")
    }

    fn window_text_height_rows(
        &self,
        window: &api::Window,
        start_row: usize,
        end_row: usize,
    ) -> i64 {
        self.record(WindowSurfaceCall::WindowTextHeightRows {
            window_handle: window.handle(),
            start_row,
            end_row,
        });
        self.window_text_height_rows_results
            .borrow_mut()
            .pop_front()
            .unwrap_or_default()
    }
}
