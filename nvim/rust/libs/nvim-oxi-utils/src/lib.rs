//! Shared nvim-oxi helpers for plugin crates.

pub mod decode;
mod error;
pub mod indexed_registry;

pub use error::{Error, Result};

pub mod guard {
    use std::any::Any;
    use std::panic::{AssertUnwindSafe, catch_unwind};

    /// Details about a captured panic payload.
    #[derive(Debug, Clone)]
    pub enum PanicInfo {
        /// A panic with a string message.
        Message(String),
        /// A panic without a string payload.
        Unknown,
    }

    impl PanicInfo {
        /// Render the panic payload as a human-readable message.
        pub fn render(&self) -> String {
            match self {
                Self::Message(msg) => msg.clone(),
                Self::Unknown => "panic payload: <non-string>".to_string(),
            }
        }
    }

    fn from_payload(payload: &(dyn Any + Send)) -> PanicInfo {
        if let Some(msg) = payload.downcast_ref::<&str>() {
            return PanicInfo::Message((*msg).to_string());
        }
        if let Some(msg) = payload.downcast_ref::<String>() {
            return PanicInfo::Message(msg.clone());
        }
        PanicInfo::Unknown
    }

    /// Execute `f`, returning a `PanicInfo` if a panic occurs.
    pub fn catch_unwind_result<F, R>(f: F) -> Result<R, PanicInfo>
    where
        F: FnOnce() -> R,
    {
        match catch_unwind(AssertUnwindSafe(f)) {
            Ok(value) => Ok(value),
            Err(payload) => Err(from_payload(payload.as_ref())),
        }
    }

    /// Execute `f`, returning `fallback` on panic and invoking `on_panic`.
    pub fn with_panic<F, R, G>(fallback: R, f: F, on_panic: G) -> R
    where
        F: FnOnce() -> R,
        G: FnOnce(PanicInfo),
    {
        match catch_unwind_result(f) {
            Ok(value) => value,
            Err(info) => {
                on_panic(info);
                fallback
            }
        }
    }
}

pub mod state {
    use std::ops::{Deref, DerefMut};
    use std::sync::{Mutex, MutexGuard};

    /// A mutex-backed state container that reports poisoning explicitly.
    #[derive(Debug)]
    pub struct StateCell<T> {
        inner: Mutex<T>,
    }

    impl<T> StateCell<T> {
        /// Create a new state cell with the provided value.
        pub const fn new(value: T) -> Self {
            Self {
                inner: Mutex::new(value),
            }
        }

        /// Lock the state. If the mutex is poisoned, returns the inner guard
        /// while marking it as poisoned for callers to report.
        pub fn lock(&self) -> StateGuard<'_, T> {
            match self.inner.lock() {
                Ok(guard) => StateGuard {
                    guard,
                    poisoned: false,
                },
                Err(poisoned) => StateGuard {
                    guard: poisoned.into_inner(),
                    poisoned: true,
                },
            }
        }

        /// Mark the mutex as healthy after explicit recovery from a poison event.
        pub fn clear_poison(&self) {
            self.inner.clear_poison();
        }

        /// Lock the state and recover poisoned state via `recover`.
        ///
        /// This keeps poison handling consistent across plugin crates while
        /// letting each caller define how state should be repaired.
        pub fn lock_recover<F>(&self, recover: F) -> StateGuard<'_, T>
        where
            F: FnOnce(&mut T),
        {
            let mut guard = self.lock();
            if guard.poisoned() {
                recover(&mut guard);
                self.clear_poison();
            }
            guard
        }
    }

    /// Guard returned from `StateCell::lock` with poison information.
    #[derive(Debug)]
    pub struct StateGuard<'a, T> {
        guard: MutexGuard<'a, T>,
        poisoned: bool,
    }

    impl<T> StateGuard<'_, T> {
        /// True when the mutex was poisoned before acquisition.
        pub const fn poisoned(&self) -> bool {
            self.poisoned
        }
    }

    impl<T> Deref for StateGuard<'_, T> {
        type Target = T;

        fn deref(&self) -> &Self::Target {
            &self.guard
        }
    }

    impl<T> DerefMut for StateGuard<'_, T> {
        fn deref_mut(&mut self) -> &mut Self::Target {
            &mut self.guard
        }
    }
}

pub mod state_machine {
    /// Marker type for machines that never emit effects.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct NoEffect;

    /// Marker type for machines that never emit commands.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct NoCommand;

    /// Reducer output: effect list to execute and an optional command to dispatch.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Transition<E, C> {
        pub effects: Vec<E>,
        pub command: Option<C>,
    }

    impl<E, C> Default for Transition<E, C> {
        fn default() -> Self {
            Self {
                effects: Vec::new(),
                command: None,
            }
        }
    }

    impl<E, C> Transition<E, C> {
        pub fn with_effect(effect: E) -> Self {
            Self {
                effects: vec![effect],
                command: None,
            }
        }

        pub fn with_effects(effects: Vec<E>) -> Self {
            Self {
                effects,
                command: None,
            }
        }

        pub fn with_command(command: C) -> Self {
            Self {
                effects: Vec::new(),
                command: Some(command),
            }
        }

        pub fn push_effect(&mut self, effect: E) {
            self.effects.push(effect);
        }

        pub fn set_command(&mut self, command: C) {
            self.command = Some(command);
        }

        pub fn is_empty(&self) -> bool {
            self.effects.is_empty() && self.command.is_none()
        }
    }

    /// Generic reducer contract for state machines that emit effects and commands.
    pub trait Machine {
        type Event;
        type Effect;
        type Command;

        fn reduce(&mut self, event: Self::Event) -> Transition<Self::Effect, Self::Command>;
    }

    /// Apply one event to a machine.
    pub fn apply_event<M>(machine: &mut M, event: M::Event) -> Transition<M::Effect, M::Command>
    where
        M: Machine,
    {
        machine.reduce(event)
    }
}

pub mod lua {
    use nvim_oxi::Result;
    use nvim_oxi::mlua;

    /// Return the Neovim Lua state handle.
    pub fn state() -> mlua::Lua {
        nvim_oxi::mlua::lua()
    }

    /// Require a Lua module, returning it as a table.
    pub fn require_table(lua: &mlua::Lua, name: &str) -> Result<mlua::Table> {
        let require: mlua::Function = lua.globals().get("require")?;
        require.call(name).map_err(Into::into)
    }

    /// Try to require a Lua module, returning None on failure.
    pub fn try_require_table(lua: &mlua::Lua, name: &str) -> Option<mlua::Table> {
        let require: mlua::Function = lua.globals().get("require").ok()?;
        require.call(name).ok()
    }

    /// Call a function from a Lua table.
    pub fn call_table_function<A, R>(table: &mlua::Table, name: &str, args: A) -> Result<R>
    where
        A: mlua::IntoLuaMulti,
        R: mlua::FromLuaMulti,
    {
        let fun: mlua::Function = table.get(name)?;
        fun.call(args).map_err(Into::into)
    }
}

pub mod notify {
    use nvim_oxi::Error;
    use nvim_oxi::api;
    use nvim_oxi::api::opts::EchoOpts;

    fn format_message(context: &str, message: &str) -> String {
        if context.is_empty() {
            message.to_string()
        } else {
            format!("{context}: {message}")
        }
    }

    fn report_echo_error(context: &str, err: &Error) {
        api::err_writeln(&format!("nvim-oxi echo failed ({context}): {err}"));
    }

    fn echo_message(
        context: &str,
        message: &str,
        hl_group: Option<&str>,
        err: bool,
    ) -> nvim_oxi::Result<()> {
        let text = format_message(context, message);
        let mut opts = EchoOpts::builder();
        if err {
            opts.err(true);
        }
        api::echo([(text.as_str(), hl_group)], true, &opts.build()).map_err(Into::into)
    }

    /// Notify an info message in Neovim, falling back to stderr on failure.
    pub fn info(context: &str, message: &str) {
        if let Err(err) = echo_message(context, message, None, false) {
            report_echo_error(context, &err);
        }
    }

    /// Notify a warning in Neovim, falling back to stderr on failure.
    pub fn warn(context: &str, message: &str) {
        if let Err(err) = echo_message(context, message, Some("WarningMsg"), false) {
            report_echo_error(context, &err);
        }
    }

    /// Notify an error in Neovim, falling back to stderr on failure.
    pub fn error(context: &str, message: &str) {
        if let Err(err) = echo_message(context, message, Some("ErrorMsg"), true) {
            report_echo_error(context, &err);
        }
    }
}

pub mod handles {
    use nvim_oxi::api;
    use nvim_oxi::api::{Buffer, Window};

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct BufHandle(i64);

    impl BufHandle {
        pub fn from_buffer(buf: &Buffer) -> Self {
            Self(i64::from(buf.handle()))
        }

        pub fn try_from_i64(handle: i64) -> Option<Self> {
            if handle <= 0 {
                return None;
            }
            i32::try_from(handle).ok().map(|_| Self(handle))
        }

        pub const fn raw(self) -> i64 {
            self.0
        }

        pub fn to_buffer(self) -> Option<Buffer> {
            i32::try_from(self.0).ok().map(Buffer::from)
        }

        pub fn valid_buffer(self) -> Option<Buffer> {
            self.to_buffer().filter(Buffer::is_valid)
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct WinHandle(i64);

    impl WinHandle {
        pub fn from_window(win: &Window) -> Self {
            Self(i64::from(win.handle()))
        }

        pub fn try_from_i64(handle: i64) -> Option<Self> {
            if handle <= 0 {
                return None;
            }
            i32::try_from(handle).ok().map(|_| Self(handle))
        }

        pub const fn raw(self) -> i64 {
            self.0
        }

        pub fn to_window(self) -> Option<Window> {
            i32::try_from(self.0).ok().map(Window::from)
        }

        pub fn valid_window(self) -> Option<Window> {
            self.to_window().filter(Window::is_valid)
        }
    }

    pub fn buffer_from_i64(handle: i64) -> Option<Buffer> {
        BufHandle::try_from_i64(handle).and_then(BufHandle::to_buffer)
    }

    pub fn window_from_i64(handle: i64) -> Option<Window> {
        WinHandle::try_from_i64(handle).and_then(WinHandle::to_window)
    }

    pub fn buffer_from_optional(handle: Option<i64>) -> Option<Buffer> {
        let handle = handle?;
        if handle == 0 {
            return Some(api::get_current_buf());
        }
        if handle < 0 {
            return None;
        }
        buffer_from_i64(handle)
    }

    pub fn window_from_optional(handle: Option<i64>) -> Option<Window> {
        let handle = handle?;
        if handle == 0 {
            return Some(api::get_current_win());
        }
        if handle < 0 {
            return None;
        }
        window_from_i64(handle)
    }

    pub fn valid_buffer(handle: i64) -> Option<Buffer> {
        BufHandle::try_from_i64(handle).and_then(BufHandle::valid_buffer)
    }

    pub fn valid_window(handle: i64) -> Option<Window> {
        WinHandle::try_from_i64(handle).and_then(WinHandle::valid_window)
    }

    pub fn valid_buffer_optional(handle: Option<i64>) -> Option<Buffer> {
        buffer_from_optional(handle).filter(Buffer::is_valid)
    }

    pub fn valid_window_optional(handle: Option<i64>) -> Option<Window> {
        window_from_optional(handle).filter(Window::is_valid)
    }
}

pub mod dict {
    use crate::decode;
    use crate::{Error, Result};
    use nvim_oxi::conversion::FromObject;
    use nvim_oxi::{Dictionary, Object, String as NvimString};

    pub fn get_i64(dict: &Dictionary, key: &str) -> Option<i64> {
        let key = NvimString::from(key);
        let obj = dict.get(&key)?.clone();
        i64::from_object(obj).ok()
    }

    pub fn require_i64(dict: &Dictionary, key: &str) -> Result<i64> {
        decode::require_i64(decode::get_object(dict, key), key)
    }

    pub fn get_string(dict: &Dictionary, key: &str) -> Option<String> {
        let key = NvimString::from(key);
        dict.get(&key)
            .and_then(|obj| NvimString::from_object(obj.clone()).ok())
            .map(|val| val.to_string_lossy().into_owned())
    }

    pub fn require_string(dict: &Dictionary, key: &str) -> Result<String> {
        decode::require_string(decode::get_object(dict, key), key)
    }

    pub fn require_string_nonempty(dict: &Dictionary, key: &str) -> Result<String> {
        let value = require_string(dict, key)?;
        if value.is_empty() {
            return Err(Error::invalid_value(key, "non-empty string"));
        }
        Ok(value)
    }

    pub fn get_string_nonempty(dict: &Dictionary, key: &str) -> Option<String> {
        get_string(dict, key).filter(|val| !val.is_empty())
    }

    pub fn get_object(dict: &Dictionary, key: &str) -> Option<Object> {
        decode::get_object(dict, key)
    }
}

#[cfg(test)]
mod tests {
    use super::handles::{BufHandle, WinHandle, buffer_from_i64, window_from_i64};

    #[test]
    fn buf_handle_rejects_non_positive() {
        assert!(BufHandle::try_from_i64(0).is_none());
        assert!(BufHandle::try_from_i64(-1).is_none());
        assert!(BufHandle::try_from_i64(i64::MIN).is_none());
    }

    #[test]
    fn win_handle_rejects_non_positive() {
        assert!(WinHandle::try_from_i64(0).is_none());
        assert!(WinHandle::try_from_i64(-1).is_none());
        assert!(WinHandle::try_from_i64(i64::MIN).is_none());
    }

    #[test]
    fn buf_handle_rejects_overflow_i32_max_plus_one() {
        let overflow = i64::from(i32::MAX) + 1;
        assert!(BufHandle::try_from_i64(overflow).is_none());
        assert!(BufHandle::try_from_i64(i64::MAX).is_none());
        assert!(buffer_from_i64(overflow).is_none());
        assert!(buffer_from_i64(i64::MAX).is_none());
    }

    #[test]
    fn win_handle_rejects_overflow_i32_max_plus_one() {
        let overflow = i64::from(i32::MAX) + 1;
        assert!(WinHandle::try_from_i64(overflow).is_none());
        assert!(WinHandle::try_from_i64(i64::MAX).is_none());
        assert!(window_from_i64(overflow).is_none());
        assert!(window_from_i64(i64::MAX).is_none());
    }
}
