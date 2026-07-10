use super::BufferHandle;
use super::HostTabSnapshot;
use super::NamespaceId;
use super::NeovimHost;
use super::TabHandle;
use super::api;
use nvim_oxi::Result;
use nvimrs_nvim_oxi_utils::handles;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FloatingWindowEnter {
    DoNotEnter,
}

impl FloatingWindowEnter {
    const fn as_bool(self) -> bool {
        match self {
            Self::DoNotEnter => false,
        }
    }
}

pub(crate) trait DrawResourcePort: super::HostLoggingPort {
    fn eventignore(&self) -> Result<String>;
    fn set_eventignore(&self, value: &str) -> Result<()>;
    fn create_scratch_buffer(&self) -> Result<api::Buffer>;
    fn open_floating_window(
        &self,
        buffer: &api::Buffer,
        enter: FloatingWindowEnter,
        config: &api::types::WindowConfig,
    ) -> Result<api::Window>;
    fn set_window_config(
        &self,
        window: &mut api::Window,
        config: &api::types::WindowConfig,
    ) -> Result<()>;
    fn set_buffer_string_option(&self, buffer: &api::Buffer, name: &str, value: &str)
    -> Result<()>;
    fn set_buffer_bool_option(&self, buffer: &api::Buffer, name: &str, value: bool) -> Result<()>;
    fn set_window_string_option(
        &self,
        window: &api::Window,
        scope: api::opts::OptionScope,
        name: &str,
        value: &str,
    ) -> Result<()>;
    fn set_window_i64_option(
        &self,
        window: &api::Window,
        scope: api::opts::OptionScope,
        name: &str,
        value: i64,
    ) -> Result<()>;
    fn clear_buffer_namespace(
        &self,
        buffer: &mut api::Buffer,
        namespace_id: NamespaceId,
    ) -> Result<()>;
    fn delete_buffer_force(&self, buffer: api::Buffer) -> Result<()>;
    fn close_window_force(&self, window: api::Window) -> Result<()>;
    fn buffer_is_valid(&self, buffer: &api::Buffer) -> bool;
    fn window_is_valid(&self, window: &api::Window) -> bool;
    fn valid_buffer(&self, handle: BufferHandle) -> Option<api::Buffer>;
    fn valid_window_i32(&self, handle: i32) -> Option<api::Window>;
    fn window_from_handle_i32_unchecked(&self, handle: i32) -> api::Window;
    fn buffer_from_handle_unchecked(&self, handle: BufferHandle) -> Option<api::Buffer>;
    fn current_tab_snapshot(&self) -> HostTabSnapshot;
    fn set_buffer_extmark(
        &self,
        buffer: &mut api::Buffer,
        namespace_id: NamespaceId,
        line: usize,
        column: usize,
        opts: &api::opts::SetExtmarkOpts,
    ) -> Result<()>;
}

impl DrawResourcePort for NeovimHost {
    fn eventignore(&self) -> Result<String> {
        let opts = api::opts::OptionOpts::builder().build();
        Ok(api::get_option_value("eventignore", &opts)?)
    }

    fn set_eventignore(&self, value: &str) -> Result<()> {
        let opts = api::opts::OptionOpts::builder().build();
        api::set_option_value("eventignore", value, &opts)?;
        Ok(())
    }

    fn create_scratch_buffer(&self) -> Result<api::Buffer> {
        Ok(api::create_buf(
            /*is_listed*/ false, /*is_scratch*/ true,
        )?)
    }

    fn open_floating_window(
        &self,
        buffer: &api::Buffer,
        enter: FloatingWindowEnter,
        config: &api::types::WindowConfig,
    ) -> Result<api::Window> {
        Ok(api::open_win(buffer, enter.as_bool(), config)?)
    }

    fn set_window_config(
        &self,
        window: &mut api::Window,
        config: &api::types::WindowConfig,
    ) -> Result<()> {
        window.set_config(config)?;
        Ok(())
    }

    fn set_buffer_string_option(
        &self,
        buffer: &api::Buffer,
        name: &str,
        value: &str,
    ) -> Result<()> {
        let opts = api::opts::OptionOpts::builder().buf(buffer.clone()).build();
        api::set_option_value(name, value, &opts)?;
        Ok(())
    }

    fn set_buffer_bool_option(&self, buffer: &api::Buffer, name: &str, value: bool) -> Result<()> {
        let opts = api::opts::OptionOpts::builder().buf(buffer.clone()).build();
        api::set_option_value(name, value, &opts)?;
        Ok(())
    }

    fn set_window_string_option(
        &self,
        window: &api::Window,
        scope: api::opts::OptionScope,
        name: &str,
        value: &str,
    ) -> Result<()> {
        let opts = api::opts::OptionOpts::builder()
            .scope(scope)
            .win(window.clone())
            .build();
        api::set_option_value(name, value, &opts)?;
        Ok(())
    }

    fn set_window_i64_option(
        &self,
        window: &api::Window,
        scope: api::opts::OptionScope,
        name: &str,
        value: i64,
    ) -> Result<()> {
        let opts = api::opts::OptionOpts::builder()
            .scope(scope)
            .win(window.clone())
            .build();
        api::set_option_value(name, value, &opts)?;
        Ok(())
    }

    fn clear_buffer_namespace(
        &self,
        buffer: &mut api::Buffer,
        namespace_id: NamespaceId,
    ) -> Result<()> {
        buffer
            .clear_namespace(namespace_id.get(), 0..)
            .map_err(nvim_oxi::Error::Api)
    }

    fn delete_buffer_force(&self, buffer: api::Buffer) -> Result<()> {
        let opts = api::opts::BufDeleteOpts::builder().force(true).build();
        buffer.delete(&opts)?;
        Ok(())
    }

    fn close_window_force(&self, window: api::Window) -> Result<()> {
        window.close(/*force*/ true)?;
        Ok(())
    }

    fn buffer_is_valid(&self, buffer: &api::Buffer) -> bool {
        buffer.is_valid()
    }

    fn window_is_valid(&self, window: &api::Window) -> bool {
        window.is_valid()
    }

    fn valid_buffer(&self, handle: BufferHandle) -> Option<api::Buffer> {
        if !handle.is_valid() {
            return None;
        }
        handles::valid_buffer(handle.get())
    }

    fn valid_window_i32(&self, handle: i32) -> Option<api::Window> {
        if handle <= 0 {
            return None;
        }
        handles::valid_window(i64::from(handle))
    }

    fn window_from_handle_i32_unchecked(&self, handle: i32) -> api::Window {
        api::Window::from(handle)
    }

    fn buffer_from_handle_unchecked(&self, handle: BufferHandle) -> Option<api::Buffer> {
        handle.as_i32().map(api::Buffer::from)
    }

    fn current_tab_snapshot(&self) -> HostTabSnapshot {
        let tabpage = api::get_current_tabpage();
        HostTabSnapshot {
            tab_handle: TabHandle::from_tabpage(&tabpage),
            tab_number: tabpage.get_number().ok(),
        }
    }

    fn set_buffer_extmark(
        &self,
        buffer: &mut api::Buffer,
        namespace_id: NamespaceId,
        line: usize,
        column: usize,
        opts: &api::opts::SetExtmarkOpts,
    ) -> Result<()> {
        buffer
            .set_extmark(namespace_id.get(), line, column, opts)
            .map(|_| ())
            .map_err(nvim_oxi::Error::Api)
    }
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum DrawResourceCall {
    WriteError {
        message: String,
    },
    Eventignore,
    SetEventignore {
        value: String,
    },
    CreateScratchBuffer,
    OpenFloatingWindow {
        buffer: BufferHandle,
        enter: FloatingWindowEnter,
    },
    SetWindowConfig {
        window_id: i32,
    },
    SetBufferStringOption {
        buffer: BufferHandle,
        name: String,
        value: String,
    },
    SetBufferBoolOption {
        buffer: BufferHandle,
        name: String,
        value: bool,
    },
    SetWindowStringOption {
        window_id: i32,
        name: String,
        value: String,
    },
    SetWindowI64Option {
        window_id: i32,
        name: String,
        value: i64,
    },
    ClearBufferNamespace {
        buffer: BufferHandle,
        namespace_id: NamespaceId,
    },
    DeleteBufferForce {
        buffer: BufferHandle,
    },
    CloseWindowForce {
        window_id: i32,
    },
    CurrentTabSnapshot,
    SetBufferExtmark {
        buffer: BufferHandle,
        namespace_id: NamespaceId,
        line: usize,
        column: usize,
    },
}

#[cfg(test)]
#[derive(Debug)]
pub(crate) struct FakeDrawResourcePort {
    calls: std::cell::RefCell<Vec<DrawResourceCall>>,
    eventignore_results: std::cell::RefCell<std::collections::VecDeque<Result<String>>>,
    current_tab_snapshot: std::cell::Cell<HostTabSnapshot>,
    delete_buffer_failures: std::cell::Cell<usize>,
    close_window_failures: std::cell::Cell<usize>,
}

#[cfg(test)]
impl Default for FakeDrawResourcePort {
    fn default() -> Self {
        Self {
            calls: std::cell::RefCell::new(Vec::new()),
            eventignore_results: std::cell::RefCell::new(std::collections::VecDeque::new()),
            current_tab_snapshot: std::cell::Cell::new(HostTabSnapshot {
                tab_handle: TabHandle::from_raw_for_test(/*value*/ 1),
                tab_number: Some(1),
            }),
            delete_buffer_failures: std::cell::Cell::new(0),
            close_window_failures: std::cell::Cell::new(0),
        }
    }
}

#[cfg(test)]
impl FakeDrawResourcePort {
    pub(crate) fn push_eventignore(&self, value: &str) {
        self.eventignore_results
            .borrow_mut()
            .push_back(Ok(value.to_owned()));
    }

    pub(crate) fn calls(&self) -> Vec<DrawResourceCall> {
        self.calls.borrow().clone()
    }

    pub(crate) fn fail_next_buffer_delete(&self) {
        self.delete_buffer_failures
            .set(self.delete_buffer_failures.get().saturating_add(1));
    }

    pub(crate) fn fail_next_window_close(&self) {
        self.close_window_failures
            .set(self.close_window_failures.get().saturating_add(1));
    }

    fn record(&self, call: DrawResourceCall) {
        self.calls.borrow_mut().push(call);
    }
}

#[cfg(test)]
impl super::HostLoggingPort for FakeDrawResourcePort {
    fn write_error(&self, message: &str) {
        self.record(DrawResourceCall::WriteError {
            message: message.to_owned(),
        });
    }

    fn notify(&self, _message: &str, _level: crate::config::LogLevel) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
impl DrawResourcePort for FakeDrawResourcePort {
    fn eventignore(&self) -> Result<String> {
        self.record(DrawResourceCall::Eventignore);
        super::pop_fake_response(&self.eventignore_results, "eventignore")
    }

    fn set_eventignore(&self, value: &str) -> Result<()> {
        self.record(DrawResourceCall::SetEventignore {
            value: value.to_owned(),
        });
        Ok(())
    }

    fn create_scratch_buffer(&self) -> Result<api::Buffer> {
        self.record(DrawResourceCall::CreateScratchBuffer);
        Ok(api::Buffer::from(1))
    }

    fn open_floating_window(
        &self,
        buffer: &api::Buffer,
        enter: FloatingWindowEnter,
        _config: &api::types::WindowConfig,
    ) -> Result<api::Window> {
        self.record(DrawResourceCall::OpenFloatingWindow {
            buffer: BufferHandle::from_buffer(buffer),
            enter,
        });
        Ok(api::Window::from(1))
    }

    fn set_window_config(
        &self,
        window: &mut api::Window,
        _config: &api::types::WindowConfig,
    ) -> Result<()> {
        self.record(DrawResourceCall::SetWindowConfig {
            window_id: window.handle(),
        });
        Ok(())
    }

    fn set_buffer_string_option(
        &self,
        buffer: &api::Buffer,
        name: &str,
        value: &str,
    ) -> Result<()> {
        self.record(DrawResourceCall::SetBufferStringOption {
            buffer: BufferHandle::from_buffer(buffer),
            name: name.to_owned(),
            value: value.to_owned(),
        });
        Ok(())
    }

    fn set_buffer_bool_option(&self, buffer: &api::Buffer, name: &str, value: bool) -> Result<()> {
        self.record(DrawResourceCall::SetBufferBoolOption {
            buffer: BufferHandle::from_buffer(buffer),
            name: name.to_owned(),
            value,
        });
        Ok(())
    }

    fn set_window_string_option(
        &self,
        window: &api::Window,
        _scope: api::opts::OptionScope,
        name: &str,
        value: &str,
    ) -> Result<()> {
        self.record(DrawResourceCall::SetWindowStringOption {
            window_id: window.handle(),
            name: name.to_owned(),
            value: value.to_owned(),
        });
        Ok(())
    }

    fn set_window_i64_option(
        &self,
        window: &api::Window,
        _scope: api::opts::OptionScope,
        name: &str,
        value: i64,
    ) -> Result<()> {
        self.record(DrawResourceCall::SetWindowI64Option {
            window_id: window.handle(),
            name: name.to_owned(),
            value,
        });
        Ok(())
    }

    fn clear_buffer_namespace(
        &self,
        buffer: &mut api::Buffer,
        namespace_id: NamespaceId,
    ) -> Result<()> {
        self.record(DrawResourceCall::ClearBufferNamespace {
            buffer: BufferHandle::from_buffer(buffer),
            namespace_id,
        });
        Ok(())
    }

    fn delete_buffer_force(&self, buffer: api::Buffer) -> Result<()> {
        self.record(DrawResourceCall::DeleteBufferForce {
            buffer: BufferHandle::from_buffer(&buffer),
        });
        if self.delete_buffer_failures.get() > 0 {
            self.delete_buffer_failures
                .set(self.delete_buffer_failures.get().saturating_sub(1));
            return Err(api::Error::Other("injected buffer delete failure".to_owned()).into());
        }
        Ok(())
    }

    fn close_window_force(&self, window: api::Window) -> Result<()> {
        self.record(DrawResourceCall::CloseWindowForce {
            window_id: window.handle(),
        });
        if self.close_window_failures.get() > 0 {
            self.close_window_failures
                .set(self.close_window_failures.get().saturating_sub(1));
            return Err(api::Error::Other("injected window close failure".to_owned()).into());
        }
        Ok(())
    }

    fn buffer_is_valid(&self, buffer: &api::Buffer) -> bool {
        BufferHandle::from_buffer(buffer).is_valid()
    }

    fn window_is_valid(&self, window: &api::Window) -> bool {
        window.handle() > 0
    }

    fn valid_buffer(&self, handle: BufferHandle) -> Option<api::Buffer> {
        handle
            .as_i32()
            .filter(|handle| *handle > 0)
            .map(api::Buffer::from)
    }

    fn valid_window_i32(&self, handle: i32) -> Option<api::Window> {
        (handle > 0).then(|| api::Window::from(handle))
    }

    fn window_from_handle_i32_unchecked(&self, handle: i32) -> api::Window {
        api::Window::from(handle)
    }

    fn buffer_from_handle_unchecked(&self, handle: BufferHandle) -> Option<api::Buffer> {
        handle.as_i32().map(api::Buffer::from)
    }

    fn current_tab_snapshot(&self) -> HostTabSnapshot {
        self.record(DrawResourceCall::CurrentTabSnapshot);
        self.current_tab_snapshot.get()
    }

    fn set_buffer_extmark(
        &self,
        buffer: &mut api::Buffer,
        namespace_id: NamespaceId,
        line: usize,
        column: usize,
        _opts: &api::opts::SetExtmarkOpts,
    ) -> Result<()> {
        self.record(DrawResourceCall::SetBufferExtmark {
            buffer: BufferHandle::from_buffer(buffer),
            namespace_id,
            line,
            column,
        });
        Ok(())
    }
}
