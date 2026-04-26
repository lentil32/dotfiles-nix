use super::BufferHandle;
use super::NeovimHost;
use super::api;
use nvim_oxi::Array;
use nvim_oxi::Object;
use nvim_oxi::Result;

pub(crate) trait CursorReadPort {
    fn window_cursor(&self, window: &api::Window) -> Result<(usize, usize)>;
    fn window_buffer_handle(&self, window: &api::Window) -> Result<BufferHandle>;
    fn window_conceallevel(&self, window: &api::Window) -> Result<i64>;
    fn window_concealcursor(&self, window: &api::Window) -> Result<String>;
    fn screenpos(&self, window: &api::Window, line: usize, col1: i64) -> Result<Object>;
    fn synconcealed(&self, line: usize, col1: i64) -> Result<Object>;
    fn string_display_width(&self, text: &str) -> Result<Object>;
    fn command_type(&self) -> Result<Object>;
    fn command_screenpos(&self) -> Result<Object>;
    fn buffer_lines(
        &self,
        buffer: &api::Buffer,
        start_index: usize,
        end_index: usize,
    ) -> Result<Vec<String>>;
}

impl CursorReadPort for NeovimHost {
    fn window_cursor(&self, window: &api::Window) -> Result<(usize, usize)> {
        Ok(window.get_cursor()?)
    }

    fn window_buffer_handle(&self, window: &api::Window) -> Result<BufferHandle> {
        Ok(BufferHandle::from_buffer(&window.get_buf()?))
    }

    fn window_conceallevel(&self, window: &api::Window) -> Result<i64> {
        let opts = api::opts::OptionOpts::builder().win(window.clone()).build();
        Ok(api::get_option_value("conceallevel", &opts)?)
    }

    fn window_concealcursor(&self, window: &api::Window) -> Result<String> {
        let opts = api::opts::OptionOpts::builder().win(window.clone()).build();
        Ok(api::get_option_value("concealcursor", &opts)?)
    }

    fn screenpos(&self, window: &api::Window, line: usize, col1: i64) -> Result<Object> {
        let args = Array::from_iter([
            Object::from(window.handle()),
            Object::from(i64::try_from(line).unwrap_or(i64::MAX)),
            Object::from(col1),
        ]);
        Ok(api::call_function("screenpos", args)?)
    }

    fn synconcealed(&self, line: usize, col1: i64) -> Result<Object> {
        let line = i64::try_from(line).unwrap_or(i64::MAX);
        let args = Array::from_iter([Object::from(line), Object::from(col1)]);
        Ok(api::call_function("synconcealed", args)?)
    }

    fn string_display_width(&self, text: &str) -> Result<Object> {
        let args = Array::from_iter([Object::from(text)]);
        Ok(api::call_function("strdisplaywidth", args)?)
    }

    fn command_type(&self) -> Result<Object> {
        Ok(api::call_function("getcmdtype", Array::new())?)
    }

    fn command_screenpos(&self) -> Result<Object> {
        Ok(api::call_function("getcmdscreenpos", Array::new())?)
    }

    fn buffer_lines(
        &self,
        buffer: &api::Buffer,
        start_index: usize,
        end_index: usize,
    ) -> Result<Vec<String>> {
        Ok(buffer
            .get_lines(start_index..end_index, /*strict_indexing*/ false)?
            .map(|line| line.to_string_lossy().into_owned())
            .collect())
    }
}

#[cfg(test)]
#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) enum CursorReadCall {
    WindowCursor {
        window_handle: i32,
    },
    WindowBufferHandle {
        window_handle: i32,
    },
    WindowConceallevel {
        window_handle: i32,
    },
    WindowConcealcursor {
        window_handle: i32,
    },
    Screenpos {
        window_handle: i32,
        line: usize,
        col1: i64,
    },
    Synconcealed {
        line: usize,
        col1: i64,
    },
    StringDisplayWidth {
        text: String,
    },
    CommandType,
    CommandScreenpos,
    BufferLines {
        buffer_handle: BufferHandle,
        start_index: usize,
        end_index: usize,
    },
}

#[cfg(test)]
#[derive(Debug, Default)]
pub(crate) struct FakeCursorReadPort {
    calls: std::cell::RefCell<Vec<CursorReadCall>>,
    window_cursors: std::cell::RefCell<std::collections::HashMap<i32, (usize, usize)>>,
    window_buffer_handle_results:
        std::cell::RefCell<std::collections::VecDeque<Result<BufferHandle>>>,
    window_conceal_states: std::cell::RefCell<std::collections::HashMap<i32, (i64, String)>>,
    screenpos_results: std::cell::RefCell<std::collections::VecDeque<Result<Object>>>,
    synconcealed_results: std::cell::RefCell<std::collections::VecDeque<Result<Object>>>,
    string_display_width_results: std::cell::RefCell<std::collections::VecDeque<Result<Object>>>,
    command_type_results: std::cell::RefCell<std::collections::VecDeque<Result<Object>>>,
    command_screenpos_results: std::cell::RefCell<std::collections::VecDeque<Result<Object>>>,
    buffer_lines_results: std::cell::RefCell<std::collections::VecDeque<Result<Vec<String>>>>,
}

#[cfg(test)]
impl FakeCursorReadPort {
    pub(crate) fn set_window_cursor(&self, window_handle: i32, line: usize, column: usize) {
        self.window_cursors
            .borrow_mut()
            .insert(window_handle, (line, column));
    }

    pub(crate) fn push_window_buffer_handle(&self, handle: impl Into<BufferHandle>) {
        self.window_buffer_handle_results
            .borrow_mut()
            .push_back(Ok(handle.into()));
    }

    pub(crate) fn set_window_conceal_state(
        &self,
        window_handle: i32,
        conceallevel: i64,
        concealcursor: &str,
    ) {
        self.window_conceal_states
            .borrow_mut()
            .insert(window_handle, (conceallevel, concealcursor.to_owned()));
    }

    pub(crate) fn push_screenpos(&self, screenpos: Object) {
        self.screenpos_results.borrow_mut().push_back(Ok(screenpos));
    }

    pub(crate) fn push_synconcealed(&self, synconcealed: Object) {
        self.synconcealed_results
            .borrow_mut()
            .push_back(Ok(synconcealed));
    }

    pub(crate) fn push_string_display_width(&self, width: Object) {
        self.string_display_width_results
            .borrow_mut()
            .push_back(Ok(width));
    }

    pub(crate) fn push_command_type(&self, command_type: Object) {
        self.command_type_results
            .borrow_mut()
            .push_back(Ok(command_type));
    }

    pub(crate) fn push_command_screenpos(&self, screenpos: Object) {
        self.command_screenpos_results
            .borrow_mut()
            .push_back(Ok(screenpos));
    }

    pub(crate) fn push_buffer_lines<I, S>(&self, lines: I)
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.buffer_lines_results
            .borrow_mut()
            .push_back(Ok(lines.into_iter().map(Into::into).collect()));
    }

    pub(crate) fn calls(&self) -> Vec<CursorReadCall> {
        self.calls.borrow().clone()
    }

    fn record(&self, call: CursorReadCall) {
        self.calls.borrow_mut().push(call);
    }
}

#[cfg(test)]
impl CursorReadPort for FakeCursorReadPort {
    fn window_cursor(&self, window: &api::Window) -> Result<(usize, usize)> {
        let handle = window.handle();
        self.record(CursorReadCall::WindowCursor {
            window_handle: handle,
        });
        Ok(self
            .window_cursors
            .borrow()
            .get(&handle)
            .copied()
            .unwrap_or((1, 0)))
    }

    fn window_buffer_handle(&self, window: &api::Window) -> Result<BufferHandle> {
        self.record(CursorReadCall::WindowBufferHandle {
            window_handle: window.handle(),
        });
        super::pop_fake_response(&self.window_buffer_handle_results, "window buffer handle")
    }

    fn window_conceallevel(&self, window: &api::Window) -> Result<i64> {
        let handle = window.handle();
        self.record(CursorReadCall::WindowConceallevel {
            window_handle: handle,
        });
        Ok(self
            .window_conceal_states
            .borrow()
            .get(&handle)
            .map(|(conceallevel, _concealcursor)| *conceallevel)
            .unwrap_or_default())
    }

    fn window_concealcursor(&self, window: &api::Window) -> Result<String> {
        let handle = window.handle();
        self.record(CursorReadCall::WindowConcealcursor {
            window_handle: handle,
        });
        Ok(self
            .window_conceal_states
            .borrow()
            .get(&handle)
            .map(|(_conceallevel, concealcursor)| concealcursor.clone())
            .unwrap_or_default())
    }

    fn screenpos(&self, window: &api::Window, line: usize, col1: i64) -> Result<Object> {
        self.record(CursorReadCall::Screenpos {
            window_handle: window.handle(),
            line,
            col1,
        });
        super::pop_fake_response(&self.screenpos_results, "screenpos")
    }

    fn synconcealed(&self, line: usize, col1: i64) -> Result<Object> {
        self.record(CursorReadCall::Synconcealed { line, col1 });
        super::pop_fake_response(&self.synconcealed_results, "synconcealed")
    }

    fn string_display_width(&self, text: &str) -> Result<Object> {
        self.record(CursorReadCall::StringDisplayWidth {
            text: text.to_owned(),
        });
        super::pop_fake_response(&self.string_display_width_results, "strdisplaywidth")
    }

    fn command_type(&self) -> Result<Object> {
        self.record(CursorReadCall::CommandType);
        super::pop_fake_response(&self.command_type_results, "getcmdtype")
    }

    fn command_screenpos(&self) -> Result<Object> {
        self.record(CursorReadCall::CommandScreenpos);
        super::pop_fake_response(&self.command_screenpos_results, "getcmdscreenpos")
    }

    fn buffer_lines(
        &self,
        buffer: &api::Buffer,
        start_index: usize,
        end_index: usize,
    ) -> Result<Vec<String>> {
        self.record(CursorReadCall::BufferLines {
            buffer_handle: BufferHandle::from_buffer(buffer),
            start_index,
            end_index,
        });
        super::pop_fake_response(&self.buffer_lines_results, "buffer lines")
    }
}
