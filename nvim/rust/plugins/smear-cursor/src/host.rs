//! Host-facing API facade.
//!
//! Runtime and shell modules use this facade instead of importing the external
//! Neovim API module directly. That keeps the external host boundary explicit
//! and gives architecture tests one import site to police.

use nvim_oxi::Array;
use nvim_oxi::Dictionary;
use nvim_oxi::Object;
use nvim_oxi::Result;

use crate::config::LogLevel;

mod buffer_metadata;
mod current_editor;
mod cursor_read;
mod cursor_visibility;
mod draw;
mod lifecycle;
mod scheduler;
mod tabpage;
mod window_surface;

pub(crate) use nvim_oxi::api;

#[cfg(test)]
pub(crate) use buffer_metadata::BufferMetadataCall;
pub(crate) use buffer_metadata::BufferMetadataPort;
pub(crate) use buffer_metadata::BufferMetadataSnapshot;
#[cfg(test)]
pub(crate) use buffer_metadata::FakeBufferMetadataPort;
#[cfg(test)]
pub(crate) use current_editor::CurrentEditorCall;
pub(crate) use current_editor::CurrentEditorPort;
#[cfg(test)]
pub(crate) use current_editor::FakeCurrentEditorPort;
#[cfg(test)]
pub(crate) use cursor_read::CursorReadCall;
pub(crate) use cursor_read::CursorReadPort;
#[cfg(test)]
pub(crate) use cursor_read::FakeCursorReadPort;
#[cfg(test)]
pub(crate) use cursor_visibility::CursorVisibilityCall;
pub(crate) use cursor_visibility::CursorVisibilityPort;
#[cfg(test)]
pub(crate) use cursor_visibility::FakeCursorVisibilityPort;
pub(crate) use cursor_visibility::HostCursorVisibility;
#[cfg(test)]
pub(crate) use draw::DrawResourceCall;
pub(crate) use draw::DrawResourcePort;
#[cfg(test)]
pub(crate) use draw::FakeDrawResourcePort;
pub(crate) use draw::FloatingWindowEnter;
#[cfg(test)]
pub(crate) use lifecycle::FakeLifecyclePort;
#[cfg(test)]
pub(crate) use lifecycle::LifecycleCall;
pub(crate) use lifecycle::LifecyclePort;
#[cfg(test)]
pub(crate) use scheduler::FakeSchedulerPort;
#[cfg(test)]
pub(crate) use scheduler::SchedulerCall;
pub(crate) use scheduler::SchedulerPort;
#[cfg(test)]
pub(crate) use tabpage::FakeTabPagePort;
pub(crate) use tabpage::HostTabSnapshot;
#[cfg(test)]
pub(crate) use tabpage::TabPageCall;
pub(crate) use tabpage::TabPagePort;
#[cfg(test)]
pub(crate) use window_surface::FakeWindowSurfacePort;
#[cfg(test)]
pub(crate) use window_surface::WindowSurfaceCall;
pub(crate) use window_surface::WindowSurfacePort;

pub(crate) const HOST_BRIDGE_REVISION_FUNCTION_NAME: &str =
    "nvimrs_smear_cursor#host_bridge#revision";
pub(crate) const DISPATCH_AUTOCMD_FUNCTION_NAME: &str =
    "nvimrs_smear_cursor#host_bridge#dispatch_autocmd";
#[cfg(test)]
pub(crate) const DISPATCH_TIMER_FUNCTION_NAME: &str =
    "nvimrs_smear_cursor#host_bridge#dispatch_timer";
pub(crate) const START_TIMER_ONCE_FUNCTION_NAME: &str =
    "nvimrs_smear_cursor#host_bridge#start_timer_once";
pub(crate) const STOP_TIMER_FUNCTION_NAME: &str = "nvimrs_smear_cursor#host_bridge#stop_timer";
pub(crate) const INSTALL_PROBE_HELPERS_FUNCTION_NAME: &str =
    "nvimrs_smear_cursor#host_bridge#install_probe_helpers";
pub(crate) const CURSOR_COLOR_AT_CURSOR_FUNCTION_NAME: &str =
    "nvimrs_smear_cursor#host_bridge#cursor_color_at_cursor";
pub(crate) const BACKGROUND_ALLOWED_MASK_FUNCTION_NAME: &str =
    "nvimrs_smear_cursor#host_bridge#background_allowed_mask";

#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(transparent)]
pub(crate) struct NamespaceId(u32);

impl NamespaceId {
    pub(crate) const fn new(value: u32) -> Self {
        Self(value)
    }

    pub(crate) const fn get(self) -> u32 {
        self.0
    }

    pub(crate) const fn is_global(self) -> bool {
        self.0 == 0
    }
}

impl std::fmt::Display for NamespaceId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(transparent)]
pub(crate) struct BufferHandle(i64);

impl BufferHandle {
    pub(crate) const fn new(value: i64) -> Option<Self> {
        if value > 0 { Some(Self(value)) } else { None }
    }

    pub(crate) fn from_buffer(buffer: &api::Buffer) -> Self {
        Self(i64::from(buffer.handle()))
    }

    #[cfg(test)]
    pub(crate) const fn from_raw_for_test(value: i64) -> Self {
        Self(value)
    }

    pub(crate) const fn get(self) -> i64 {
        self.0
    }

    pub(crate) const fn is_valid(self) -> bool {
        self.0 > 0
    }

    pub(crate) fn as_i32(self) -> Option<i32> {
        i32::try_from(self.0).ok()
    }
}

#[cfg(test)]
impl From<i64> for BufferHandle {
    fn from(value: i64) -> Self {
        Self::from_raw_for_test(value)
    }
}

#[cfg(test)]
impl From<i32> for BufferHandle {
    fn from(value: i32) -> Self {
        Self::from_raw_for_test(i64::from(value))
    }
}

#[cfg(test)]
impl PartialEq<i64> for BufferHandle {
    fn eq(&self, other: &i64) -> bool {
        self.0 == *other
    }
}

#[cfg(test)]
impl PartialEq<BufferHandle> for i64 {
    fn eq(&self, other: &BufferHandle) -> bool {
        *self == other.0
    }
}

impl std::fmt::Display for BufferHandle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(transparent)]
pub(crate) struct TabHandle(i32);

impl TabHandle {
    pub(crate) fn from_tabpage(tabpage: &api::TabPage) -> Self {
        Self(tabpage.handle())
    }

    #[cfg(test)]
    pub(crate) const fn from_raw_for_test(value: i32) -> Self {
        Self(value)
    }
}

#[cfg(test)]
impl From<i32> for TabHandle {
    fn from(value: i32) -> Self {
        Self::from_raw_for_test(value)
    }
}

impl std::fmt::Display for TabHandle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct EditorViewportOptions {
    lines: i64,
    cmdheight: i64,
    columns: i64,
}

impl EditorViewportOptions {
    pub(crate) const fn new(lines: i64, cmdheight: i64, columns: i64) -> Self {
        Self {
            lines,
            cmdheight,
            columns,
        }
    }

    pub(crate) const fn lines(self) -> i64 {
        self.lines
    }

    pub(crate) const fn cmdheight(self) -> i64 {
        self.cmdheight
    }

    pub(crate) const fn columns(self) -> i64 {
        self.columns
    }
}

pub(crate) trait EditorViewportPort {
    fn editor_viewport_options(&self) -> Result<EditorViewportOptions>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CursorColorExtmarkFallback {
    SyntaxOnly,
    SyntaxThenExtmarks,
}

pub(crate) trait HostBridgePort {
    fn host_bridge_revision(&self) -> Result<i64>;
    fn start_timer_once(&self, host_callback_id: i64, timeout_ms: i64) -> Result<i64>;
    fn stop_timer(&self, timer_id: i64) -> Result<()>;
    fn install_probe_helpers(&self) -> Result<()>;
    fn cursor_color_at_cursor(
        &self,
        extmark_fallback: CursorColorExtmarkFallback,
    ) -> Result<Object>;
    fn background_allowed_mask(&self, request: Array) -> Result<Object>;
    fn create_namespace(&self, name: &str) -> NamespaceId;
}

pub(crate) trait HostLoggingPort {
    fn write_error(&self, message: &str);
    fn notify(&self, message: &str, level: LogLevel) -> Result<()>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HostFlushRedrawCapability {
    ApiAvailable,
    FallbackOnly,
}

pub(crate) trait RedrawCommandPort {
    fn probe_flush_redraw_capability(&self) -> Result<HostFlushRedrawCapability>;
    fn flush_redraw(&self) -> Result<()>;
    fn fallback_redraw(&self) -> Result<()>;
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum HighlightColorField {
    Foreground,
    Background,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct HighlightStyle<'a> {
    pub(crate) foreground: &'a str,
    pub(crate) background: &'a str,
    pub(crate) blend: u8,
    pub(crate) cterm_fg: Option<u16>,
    pub(crate) cterm_bg: Option<u16>,
}

pub(crate) trait HighlightPalettePort {
    fn highlight_color(&self, group: &str, field: HighlightColorField) -> Option<u32>;
    fn set_highlight(&self, group: &str, style: HighlightStyle<'_>) -> Result<()>;
    fn clear_highlight(&self, group: &str) -> Result<()>;
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct NeovimHost;

impl EditorViewportPort for NeovimHost {
    fn editor_viewport_options(&self) -> Result<EditorViewportOptions> {
        let opts = api::opts::OptionOpts::builder().build();
        let lines: i64 = api::get_option_value("lines", &opts)?;
        let cmdheight: i64 = api::get_option_value("cmdheight", &opts)?;
        let columns: i64 = api::get_option_value("columns", &opts)?;
        Ok(EditorViewportOptions::new(lines, cmdheight, columns))
    }
}

impl HostBridgePort for NeovimHost {
    fn host_bridge_revision(&self) -> Result<i64> {
        Ok(api::call_function(
            HOST_BRIDGE_REVISION_FUNCTION_NAME,
            Array::new(),
        )?)
    }

    fn start_timer_once(&self, host_callback_id: i64, timeout_ms: i64) -> Result<i64> {
        let args = Array::from_iter([Object::from(host_callback_id), Object::from(timeout_ms)]);
        Ok(api::call_function(START_TIMER_ONCE_FUNCTION_NAME, args)?)
    }

    fn stop_timer(&self, timer_id: i64) -> Result<()> {
        let args = Array::from_iter([Object::from(timer_id)]);
        let _: i64 = api::call_function(STOP_TIMER_FUNCTION_NAME, args)?;
        Ok(())
    }

    fn install_probe_helpers(&self) -> Result<()> {
        let _: i64 = api::call_function(INSTALL_PROBE_HELPERS_FUNCTION_NAME, Array::new())?;
        Ok(())
    }

    fn cursor_color_at_cursor(
        &self,
        extmark_fallback: CursorColorExtmarkFallback,
    ) -> Result<Object> {
        let allow_extmark_fallback = match extmark_fallback {
            CursorColorExtmarkFallback::SyntaxOnly => false,
            CursorColorExtmarkFallback::SyntaxThenExtmarks => true,
        };
        let args = Array::from_iter([Object::from(allow_extmark_fallback)]);
        Ok(api::call_function(
            CURSOR_COLOR_AT_CURSOR_FUNCTION_NAME,
            args,
        )?)
    }

    fn background_allowed_mask(&self, request: Array) -> Result<Object> {
        let args = Array::from_iter([Object::from(request)]);
        Ok(api::call_function(
            BACKGROUND_ALLOWED_MASK_FUNCTION_NAME,
            args,
        )?)
    }

    fn create_namespace(&self, name: &str) -> NamespaceId {
        NamespaceId::new(api::create_namespace(name))
    }
}

impl HostLoggingPort for NeovimHost {
    fn write_error(&self, message: &str) {
        api::err_writeln(message);
    }

    fn notify(&self, message: &str, level: LogLevel) -> Result<()> {
        let payload = Array::from_iter([Object::from(message), Object::from(level.as_vim_level())]);
        let args = Array::from_iter([
            Object::from("vim.notify(_A[1], _A[2])"),
            Object::from(payload),
        ]);
        let _: Object = api::call_function("luaeval", args)?;
        Ok(())
    }
}

impl RedrawCommandPort for NeovimHost {
    fn probe_flush_redraw_capability(&self) -> Result<HostFlushRedrawCapability> {
        let exists_result: i64 =
            api::call_function("exists", Array::from_iter([Object::from("*nvim__redraw")]))?;
        if exists_result > 0 {
            Ok(HostFlushRedrawCapability::ApiAvailable)
        } else {
            Ok(HostFlushRedrawCapability::FallbackOnly)
        }
    }

    fn flush_redraw(&self) -> Result<()> {
        let mut opts = Dictionary::new();
        opts.insert("cursor", true);
        opts.insert("valid", true);
        opts.insert("flush", true);
        let _: Object = api::call_function("nvim__redraw", Array::from_iter([Object::from(opts)]))?;
        Ok(())
    }

    fn fallback_redraw(&self) -> Result<()> {
        api::command("redraw!")?;
        Ok(())
    }
}

impl HighlightPalettePort for NeovimHost {
    fn highlight_color(&self, group: &str, field: HighlightColorField) -> Option<u32> {
        let opts = api::opts::GetHighlightOpts::builder()
            .name(group)
            .link(false)
            .create(false)
            .build();
        let infos = api::get_hl(0, &opts).ok()?;
        let api::types::GetHlInfos::Single(infos) = infos else {
            return None;
        };

        match field {
            HighlightColorField::Foreground => infos.foreground,
            HighlightColorField::Background => infos.background,
        }
    }

    fn set_highlight(&self, group: &str, style: HighlightStyle<'_>) -> Result<()> {
        if style.cterm_fg.is_none() && style.cterm_bg.is_none() {
            let opts = api::opts::SetHighlightOpts::builder()
                .foreground(style.foreground)
                .background(style.background)
                .blend(style.blend)
                .build();
            api::set_hl(0, group, &opts)?;
            return Ok(());
        }

        let mut highlight = Dictionary::new();
        highlight.insert("fg", style.foreground);
        highlight.insert("bg", style.background);
        highlight.insert("blend", i64::from(style.blend));
        if let Some(value) = style.cterm_fg {
            highlight.insert("ctermfg", i64::from(value));
        }
        if let Some(value) = style.cterm_bg {
            highlight.insert("ctermbg", i64::from(value));
        }

        let args = Array::from_iter([
            Object::from(0_i64),
            Object::from(group),
            Object::from(highlight),
        ]);
        let _: Object = api::call_function("nvim_set_hl", args)?;
        Ok(())
    }

    fn clear_highlight(&self, group: &str) -> Result<()> {
        let args = Array::from_iter([
            Object::from(0_i64),
            Object::from(group),
            Object::from(Dictionary::new()),
        ]);
        let _: Object = api::call_function("nvim_set_hl", args)?;
        Ok(())
    }
}

#[cfg(test)]
#[derive(Debug, Default)]
pub(crate) struct FakeEditorViewportPort {
    calls: std::cell::Cell<usize>,
    editor_viewport_options_results:
        std::cell::RefCell<std::collections::VecDeque<Result<EditorViewportOptions>>>,
}

#[cfg(test)]
impl FakeEditorViewportPort {
    pub(crate) fn push_editor_viewport_options(&self, options: EditorViewportOptions) {
        self.editor_viewport_options_results
            .borrow_mut()
            .push_back(Ok(options));
    }

    pub(crate) fn calls(&self) -> usize {
        self.calls.get()
    }
}

#[cfg(test)]
impl EditorViewportPort for FakeEditorViewportPort {
    fn editor_viewport_options(&self) -> Result<EditorViewportOptions> {
        self.calls.set(self.calls.get().saturating_add(1));
        pop_fake_response(
            &self.editor_viewport_options_results,
            "editor viewport options",
        )
    }
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum HostLoggingCall {
    WriteError { message: String },
    Notify { message: String, level: LogLevel },
}

#[cfg(test)]
#[derive(Debug, Default)]
pub(crate) struct FakeHostLoggingPort {
    calls: std::cell::RefCell<Vec<HostLoggingCall>>,
    notify_results: std::cell::RefCell<std::collections::VecDeque<Result<()>>>,
}

#[cfg(test)]
impl FakeHostLoggingPort {
    pub(crate) fn push_notify_error(&self, message: &str) {
        self.notify_results
            .borrow_mut()
            .push_back(Err(api::Error::Other(message.to_owned()).into()));
    }

    pub(crate) fn calls(&self) -> Vec<HostLoggingCall> {
        self.calls.borrow().clone()
    }

    fn record(&self, call: HostLoggingCall) {
        self.calls.borrow_mut().push(call);
    }
}

#[cfg(test)]
impl HostLoggingPort for FakeHostLoggingPort {
    fn write_error(&self, message: &str) {
        self.record(HostLoggingCall::WriteError {
            message: message.to_owned(),
        });
    }

    fn notify(&self, message: &str, level: LogLevel) -> Result<()> {
        self.record(HostLoggingCall::Notify {
            message: message.to_owned(),
            level,
        });
        self.notify_results
            .borrow_mut()
            .pop_front()
            .unwrap_or(Ok(()))
    }
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum RedrawCommandCall {
    ProbeFlushRedrawCapability,
    FlushRedraw,
    FallbackRedraw,
}

#[cfg(test)]
#[derive(Debug, Default)]
pub(crate) struct FakeRedrawCommandPort {
    calls: std::cell::RefCell<Vec<RedrawCommandCall>>,
    probe_results:
        std::cell::RefCell<std::collections::VecDeque<Result<HostFlushRedrawCapability>>>,
    flush_results: std::cell::RefCell<std::collections::VecDeque<Result<()>>>,
    fallback_results: std::cell::RefCell<std::collections::VecDeque<Result<()>>>,
}

#[cfg(test)]
impl FakeRedrawCommandPort {
    pub(crate) fn push_probe_flush_redraw_capability(&self, capability: HostFlushRedrawCapability) {
        self.probe_results.borrow_mut().push_back(Ok(capability));
    }

    pub(crate) fn push_probe_error(&self, message: &str) {
        self.probe_results
            .borrow_mut()
            .push_back(Err(api::Error::Other(message.to_owned()).into()));
    }

    pub(crate) fn push_flush_error(&self, message: &str) {
        self.flush_results
            .borrow_mut()
            .push_back(Err(api::Error::Other(message.to_owned()).into()));
    }

    pub(crate) fn push_fallback_error(&self, message: &str) {
        self.fallback_results
            .borrow_mut()
            .push_back(Err(api::Error::Other(message.to_owned()).into()));
    }

    pub(crate) fn calls(&self) -> Vec<RedrawCommandCall> {
        self.calls.borrow().clone()
    }

    fn record(&self, call: RedrawCommandCall) {
        self.calls.borrow_mut().push(call);
    }
}

#[cfg(test)]
impl RedrawCommandPort for FakeRedrawCommandPort {
    fn probe_flush_redraw_capability(&self) -> Result<HostFlushRedrawCapability> {
        self.record(RedrawCommandCall::ProbeFlushRedrawCapability);
        pop_fake_response(&self.probe_results, "flush redraw capability")
    }

    fn flush_redraw(&self) -> Result<()> {
        self.record(RedrawCommandCall::FlushRedraw);
        self.flush_results
            .borrow_mut()
            .pop_front()
            .unwrap_or(Ok(()))
    }

    fn fallback_redraw(&self) -> Result<()> {
        self.record(RedrawCommandCall::FallbackRedraw);
        self.fallback_results
            .borrow_mut()
            .pop_front()
            .unwrap_or(Ok(()))
    }
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum HighlightPaletteCall {
    HighlightColor {
        group: String,
        field: HighlightColorField,
    },
    SetHighlight {
        group: String,
        foreground: String,
        background: String,
        blend: u8,
        cterm_fg: Option<u16>,
        cterm_bg: Option<u16>,
    },
    ClearHighlight {
        group: String,
    },
}

#[cfg(test)]
#[derive(Debug, Default)]
pub(crate) struct FakeHighlightPalettePort {
    calls: std::cell::RefCell<Vec<HighlightPaletteCall>>,
    highlight_colors:
        std::cell::RefCell<std::collections::HashMap<(String, HighlightColorField), u32>>,
    set_highlight_results: std::cell::RefCell<std::collections::VecDeque<Result<()>>>,
    clear_highlight_results: std::cell::RefCell<std::collections::VecDeque<Result<()>>>,
}

#[cfg(test)]
impl FakeHighlightPalettePort {
    pub(crate) fn set_highlight_color(&self, group: &str, field: HighlightColorField, color: u32) {
        self.highlight_colors
            .borrow_mut()
            .insert((group.to_owned(), field), color);
    }

    pub(crate) fn push_set_highlight_error(&self, message: &str) {
        self.set_highlight_results
            .borrow_mut()
            .push_back(Err(api::Error::Other(message.to_owned()).into()));
    }

    pub(crate) fn push_clear_highlight_error(&self, message: &str) {
        self.clear_highlight_results
            .borrow_mut()
            .push_back(Err(api::Error::Other(message.to_owned()).into()));
    }

    pub(crate) fn calls(&self) -> Vec<HighlightPaletteCall> {
        self.calls.borrow().clone()
    }

    fn record(&self, call: HighlightPaletteCall) {
        self.calls.borrow_mut().push(call);
    }
}

#[cfg(test)]
impl HighlightPalettePort for FakeHighlightPalettePort {
    fn highlight_color(&self, group: &str, field: HighlightColorField) -> Option<u32> {
        self.record(HighlightPaletteCall::HighlightColor {
            group: group.to_owned(),
            field,
        });
        self.highlight_colors
            .borrow()
            .get(&(group.to_owned(), field))
            .copied()
    }

    fn set_highlight(&self, group: &str, style: HighlightStyle<'_>) -> Result<()> {
        self.record(HighlightPaletteCall::SetHighlight {
            group: group.to_owned(),
            foreground: style.foreground.to_owned(),
            background: style.background.to_owned(),
            blend: style.blend,
            cterm_fg: style.cterm_fg,
            cterm_bg: style.cterm_bg,
        });
        self.set_highlight_results
            .borrow_mut()
            .pop_front()
            .unwrap_or(Ok(()))
    }

    fn clear_highlight(&self, group: &str) -> Result<()> {
        self.record(HighlightPaletteCall::ClearHighlight {
            group: group.to_owned(),
        });
        self.clear_highlight_results
            .borrow_mut()
            .pop_front()
            .unwrap_or(Ok(()))
    }
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum HostBridgeCall {
    HostBridgeRevision,
    StartTimerOnce {
        host_callback_id: i64,
        timeout_ms: i64,
    },
    StopTimer {
        timer_id: i64,
    },
    InstallProbeHelpers,
    CursorColorAtCursor {
        extmark_fallback: CursorColorExtmarkFallback,
    },
    BackgroundAllowedMask {
        request: Array,
    },
    CreateNamespace {
        name: String,
    },
}

#[cfg(test)]
#[derive(Debug, Default)]
pub(crate) struct FakeHostBridgePort {
    calls: std::cell::RefCell<Vec<HostBridgeCall>>,
    host_bridge_revision_results: std::cell::RefCell<std::collections::VecDeque<Result<i64>>>,
    start_timer_once_results: std::cell::RefCell<std::collections::VecDeque<Result<i64>>>,
    namespace_id: std::cell::Cell<NamespaceId>,
}

#[cfg(test)]
impl FakeHostBridgePort {
    pub(crate) fn push_host_bridge_revision(&self, revision: i64) {
        self.host_bridge_revision_results
            .borrow_mut()
            .push_back(Ok(revision));
    }

    pub(crate) fn push_start_timer_once(&self, host_timer_id: i64) {
        self.start_timer_once_results
            .borrow_mut()
            .push_back(Ok(host_timer_id));
    }

    pub(crate) fn set_namespace_id(&self, namespace_id: NamespaceId) {
        self.namespace_id.set(namespace_id);
    }

    pub(crate) fn calls(&self) -> Vec<HostBridgeCall> {
        self.calls.borrow().clone()
    }

    fn record(&self, call: HostBridgeCall) {
        self.calls.borrow_mut().push(call);
    }
}

#[cfg(test)]
impl HostBridgePort for FakeHostBridgePort {
    fn host_bridge_revision(&self) -> Result<i64> {
        self.record(HostBridgeCall::HostBridgeRevision);
        pop_fake_response(&self.host_bridge_revision_results, "host bridge revision")
    }

    fn start_timer_once(&self, host_callback_id: i64, timeout_ms: i64) -> Result<i64> {
        self.record(HostBridgeCall::StartTimerOnce {
            host_callback_id,
            timeout_ms,
        });
        pop_fake_response(&self.start_timer_once_results, "start timer once")
    }

    fn stop_timer(&self, timer_id: i64) -> Result<()> {
        self.record(HostBridgeCall::StopTimer { timer_id });
        Ok(())
    }

    fn install_probe_helpers(&self) -> Result<()> {
        self.record(HostBridgeCall::InstallProbeHelpers);
        Ok(())
    }

    fn cursor_color_at_cursor(
        &self,
        extmark_fallback: CursorColorExtmarkFallback,
    ) -> Result<Object> {
        self.record(HostBridgeCall::CursorColorAtCursor { extmark_fallback });
        Ok(Object::nil())
    }

    fn background_allowed_mask(&self, request: Array) -> Result<Object> {
        self.record(HostBridgeCall::BackgroundAllowedMask { request });
        Ok(Object::nil())
    }

    fn create_namespace(&self, name: &str) -> NamespaceId {
        self.record(HostBridgeCall::CreateNamespace {
            name: name.to_owned(),
        });
        self.namespace_id.get()
    }
}

#[cfg(test)]
fn pop_fake_response<T>(
    responses: &std::cell::RefCell<std::collections::VecDeque<Result<T>>>,
    label: &str,
) -> Result<T> {
    responses.borrow_mut().pop_front().unwrap_or_else(|| {
        Err(api::Error::Other(format!("fake host bridge port missing {label} response")).into())
    })
}
