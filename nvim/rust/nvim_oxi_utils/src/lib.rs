//! Shared nvim-oxi helpers for plugin crates.

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

    fn from_payload(payload: Box<dyn Any + Send>) -> PanicInfo {
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
            Err(payload) => Err(from_payload(payload)),
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
    }

    /// Guard returned from `StateCell::lock` with poison information.
    #[derive(Debug)]
    pub struct StateGuard<'a, T> {
        guard: MutexGuard<'a, T>,
        poisoned: bool,
    }

    impl<'a, T> StateGuard<'a, T> {
        /// True when the mutex was poisoned before acquisition.
        pub fn poisoned(&self) -> bool {
            self.poisoned
        }
    }

    impl<'a, T> Deref for StateGuard<'a, T> {
        type Target = T;

        fn deref(&self) -> &Self::Target {
            &self.guard
        }
    }

    impl<'a, T> DerefMut for StateGuard<'a, T> {
        fn deref_mut(&mut self) -> &mut Self::Target {
            &mut self.guard
        }
    }
}

pub mod lua {
    use nvim_oxi::api;
    use nvim_oxi::conversion::FromObject;
    use nvim_oxi::{Array, Object, Result};

    /// Evaluate a Lua expression via `luaeval`, passing `arg` as `_A`.
    pub fn eval<T>(expr: &str, arg: Option<Object>) -> Result<T>
    where
        T: FromObject,
    {
        let mut args = Array::new();
        args.push(expr);
        if let Some(arg) = arg {
            args.push(arg);
        }
        api::call_function("luaeval", args).map_err(Into::into)
    }
}

pub mod notify {
    use nvim_oxi::Dictionary;
    use nvim_oxi::api::types::LogLevel;
    use nvim_oxi::api::{self, Error};

    fn format_message(context: &str, message: &str) -> String {
        if context.is_empty() {
            message.to_string()
        } else {
            format!("{context}: {message}")
        }
    }

    fn report_notify_error(context: &str, err: Error) {
        eprintln!("nvim-oxi notify failed ({}): {}", context, err);
    }

    /// Notify a warning in Neovim, falling back to stderr on failure.
    pub fn warn(context: &str, message: &str) {
        let text = format_message(context, message);
        if let Err(err) = api::notify(&text, LogLevel::Warn, &Dictionary::new()) {
            report_notify_error(context, err);
        }
    }

    /// Notify an error in Neovim, falling back to stderr on failure.
    pub fn error(context: &str, message: &str) {
        let text = format_message(context, message);
        if let Err(err) = api::notify(&text, LogLevel::Error, &Dictionary::new()) {
            report_notify_error(context, err);
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
            Self(buf.handle() as i64)
        }

        pub fn try_from_i64(handle: i64) -> Option<Self> {
            if handle <= 0 {
                return None;
            }
            i32::try_from(handle).ok().map(|_| Self(handle))
        }

        pub fn raw(self) -> i64 {
            self.0
        }

        pub fn to_buffer(self) -> Option<Buffer> {
            i32::try_from(self.0).ok().map(Buffer::from)
        }

        pub fn valid_buffer(self) -> Option<Buffer> {
            self.to_buffer().filter(|buf| buf.is_valid())
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct WinHandle(i64);

    impl WinHandle {
        pub fn from_window(win: &Window) -> Self {
            Self(win.handle() as i64)
        }

        pub fn try_from_i64(handle: i64) -> Option<Self> {
            if handle <= 0 {
                return None;
            }
            i32::try_from(handle).ok().map(|_| Self(handle))
        }

        pub fn raw(self) -> i64 {
            self.0
        }

        pub fn to_window(self) -> Option<Window> {
            i32::try_from(self.0).ok().map(Window::from)
        }

        pub fn valid_window(self) -> Option<Window> {
            self.to_window().filter(|win| win.is_valid())
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
        buffer_from_optional(handle).filter(|buf| buf.is_valid())
    }

    pub fn valid_window_optional(handle: Option<i64>) -> Option<Window> {
        window_from_optional(handle).filter(|win| win.is_valid())
    }
}

pub mod dict {
    use nvim_oxi::conversion::FromObject;
    use nvim_oxi::{Dictionary, Object, String as NvimString};

    pub fn get_i64(dict: &Dictionary, key: &str) -> Option<i64> {
        let key = NvimString::from(key);
        let obj = dict.get(&key)?.clone();
        i64::from_object(obj).ok()
    }

    pub fn get_string(dict: &Dictionary, key: &str) -> Option<String> {
        let key = NvimString::from(key);
        dict.get(&key)
            .and_then(|obj| NvimString::from_object(obj.clone()).ok())
            .map(|val| val.to_string_lossy().into_owned())
    }

    pub fn get_string_nonempty(dict: &Dictionary, key: &str) -> Option<String> {
        get_string(dict, key).filter(|val| !val.is_empty())
    }

    pub fn get_object(dict: &Dictionary, key: &str) -> Option<Object> {
        let key = NvimString::from(key);
        dict.get(&key).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::handles::{BufHandle, WinHandle};

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
    fn buf_handle_roundtrip_i32_max() {
        let value = i32::MAX as i64;
        let handle = BufHandle::try_from_i64(value).expect("i32 max should fit");
        assert_eq!(handle.raw(), value);
    }

    #[test]
    fn win_handle_roundtrip_i32_max() {
        let value = i32::MAX as i64;
        let handle = WinHandle::try_from_i64(value).expect("i32 max should fit");
        assert_eq!(handle.raw(), value);
    }
}
