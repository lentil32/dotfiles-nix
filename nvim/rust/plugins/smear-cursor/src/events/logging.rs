use super::ENGINE_CONTEXT;
use super::LOG_LEVEL_DEBUG;
use super::LOG_LEVEL_ERROR;
use super::LOG_LEVEL_INFO;
use super::LOG_LEVEL_TRACE;
use super::LOG_LEVEL_WARN;
use super::LOG_SOURCE_NAME;
use nvim_oxi::Array;
use nvim_oxi::Object;
use nvim_oxi::api;
use std::cell::RefCell;
use std::fs::File;
use std::fs::OpenOptions;
use std::io::BufWriter;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::sync::LazyLock;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

static LOG_FILE_PATH: LazyLock<Option<PathBuf>> =
    LazyLock::new(|| std::env::var_os("SMEAR_CURSOR_LOG_FILE").map(PathBuf::from));
// Keep file logging buffered unless a live-tailed session explicitly opts into per-line flushes.
static LOG_FILE_FLUSH_POLICY: LazyLock<LogFileFlushPolicy> = LazyLock::new(|| {
    std::env::var("SMEAR_CURSOR_LOG_FLUSH")
        .ok()
        .as_deref()
        .map_or(
            LogFileFlushPolicy::Buffered,
            LogFileFlushPolicy::from_env_value,
        )
});
static PERF_SLOW_CALLBACK_THRESHOLD_MS: LazyLock<Option<f64>> = LazyLock::new(|| {
    std::env::var("SMEAR_CURSOR_PERF_SLOW_MS")
        .ok()
        .and_then(|value| value.parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value >= 0.0)
});

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LogFileFlushPolicy {
    Buffered,
    Always,
}

impl LogFileFlushPolicy {
    fn from_env_value(value: &str) -> Self {
        let normalized = value.trim();
        if normalized == "1"
            || normalized.eq_ignore_ascii_case("true")
            || normalized.eq_ignore_ascii_case("always")
            || normalized.eq_ignore_ascii_case("line")
        {
            Self::Always
        } else {
            Self::Buffered
        }
    }

    fn should_flush(self) -> bool {
        matches!(self, Self::Always)
    }
}

struct LogFileWriter {
    file: BufWriter<File>,
}

impl LogFileWriter {
    fn open(path: &Path) -> std::io::Result<Self> {
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        Ok(Self {
            file: BufWriter::new(file),
        })
    }

    fn append_line(&mut self, level_name: &str, message: &str) -> std::io::Result<()> {
        let timestamp_ms = log_timestamp_ms();
        writeln!(
            self.file,
            "[{LOG_SOURCE_NAME}][{level_name}][{timestamp_ms}] {message}"
        )?;
        if LOG_FILE_FLUSH_POLICY.should_flush() {
            self.file.flush()?;
        }
        Ok(())
    }
}

thread_local! {
    static LOG_FILE_HANDLE: RefCell<Option<LogFileWriter>> = const { RefCell::new(None) };
}

pub(super) fn set_log_level(level: i64) {
    let normalized = if level < 0 { 0 } else { level };
    ENGINE_CONTEXT.with(|context| {
        context.log_level.set(normalized);
    });
}

fn should_log(level: i64) -> bool {
    ENGINE_CONTEXT.with(|context| context.log_level.get() <= level)
}

fn log_level_name(level: i64) -> &'static str {
    match level {
        LOG_LEVEL_TRACE => "TRACE",
        LOG_LEVEL_DEBUG => "DEBUG",
        LOG_LEVEL_WARN => "WARNING",
        LOG_LEVEL_ERROR => "ERROR",
        _ => "INFO",
    }
}

fn should_notify(level: i64) -> bool {
    level >= LOG_LEVEL_INFO
}

fn log_timestamp_ms() -> u128 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_millis(),
        Err(err) => err.duration().as_millis(),
    }
}

fn append_log_line(level_name: &str, message: &str) {
    let Some(path) = LOG_FILE_PATH.as_ref() else {
        return;
    };
    LOG_FILE_HANDLE.with(|file_handle| {
        // File logging is best-effort diagnostics. If a nested callback is already writing, skip
        // this line instead of panicking the plugin on a RefCell borrow failure.
        let Ok(mut file_guard) = file_handle.try_borrow_mut() else {
            return;
        };

        if file_guard.is_none() {
            match LogFileWriter::open(path) {
                Ok(file_writer) => {
                    *file_guard = Some(file_writer);
                }
                Err(err) => {
                    api::err_writeln(&format!(
                        "[{LOG_SOURCE_NAME}] failed to open log file {}: {err}",
                        path.display()
                    ));
                    return;
                }
            }
        }

        if let Some(file_writer) = file_guard.as_mut()
            && let Err(err) = file_writer.append_line(level_name, message)
        {
            api::err_writeln(&format!(
                "[{LOG_SOURCE_NAME}] failed to write log file: {err}"
            ));
            *file_guard = None;
        }
    });
}

fn notify_log(level: i64, message: &str) {
    if !should_log(level) {
        return;
    }

    let level_name = log_level_name(level);
    append_log_line(level_name, message);
    if !should_notify(level) {
        return;
    }

    let payload_message = format!("[{LOG_SOURCE_NAME}][{level_name}] {message}");
    let payload = Array::from_iter([Object::from(payload_message), Object::from(level)]);
    let args = Array::from_iter([
        Object::from("vim.notify(_A[1], _A[2])"),
        Object::from(payload),
    ]);
    if let Err(err) = api::call_function::<_, Object>("luaeval", args) {
        api::err_writeln(&format!("[{LOG_SOURCE_NAME}] vim.notify failed: {err}"));
    }
}

pub(crate) fn warn(message: &str) {
    notify_log(LOG_LEVEL_WARN, message);
}

pub(super) fn trace_lazy(message: impl FnOnce() -> String) {
    if !should_log(LOG_LEVEL_TRACE) {
        return;
    }
    notify_log(LOG_LEVEL_TRACE, &message());
}

pub(super) fn debug(message: &str) {
    notify_log(LOG_LEVEL_DEBUG, message);
}

pub(super) fn should_log_slow_callback(callback_duration_ms: f64) -> bool {
    PERF_SLOW_CALLBACK_THRESHOLD_MS.is_some_and(|threshold_ms| callback_duration_ms >= threshold_ms)
}

pub(super) fn log_slow_callback(
    source: &str,
    mode: &str,
    callback_duration_ms: f64,
    callback_duration_estimate_ms: f64,
    details: &str,
) {
    if !should_log_slow_callback(callback_duration_ms) {
        return;
    }

    let message = format!(
        "slow-callback source={source} mode={mode} callback_ms={callback_duration_ms:.3} estimate_ms={callback_duration_estimate_ms:.3} {details}",
    );
    append_log_line("PERF", &message);
}

pub(super) fn ensure_hideable_guicursor() {
    let opts = nvim_oxi::api::opts::OptionOpts::builder().build();
    let Ok(mut guicursor) = api::get_option_value::<String>("guicursor", &opts) else {
        return;
    };
    if guicursor
        .split(',')
        .any(|entry| entry.trim() == "a:SmearCursorHideable")
    {
        return;
    }
    if !guicursor.is_empty() {
        guicursor.push(',');
    }
    guicursor.push_str("a:SmearCursorHideable");
    if let Err(err) = api::set_option_value("guicursor", guicursor, &opts) {
        warn(&format!("set guicursor failed: {err}"));
    }
}

pub(super) fn hide_real_cursor() {
    let opts = nvim_oxi::api::opts::SetHighlightOpts::builder()
        .foreground("white")
        .blend(100)
        .build();
    if let Err(err) = api::set_hl(0, "SmearCursorHideable", &opts) {
        warn(&format!("set highlight failed: {err}"));
    }
}

pub(super) fn unhide_real_cursor() {
    let opts = nvim_oxi::api::opts::SetHighlightOpts::builder()
        .foreground("none")
        .blend(0)
        .build();
    if let Err(err) = api::set_hl(0, "SmearCursorHideable", &opts) {
        warn(&format!("restore highlight failed: {err}"));
    }
}

#[cfg(test)]
mod tests {
    use super::LogFileFlushPolicy;
    use super::should_notify;

    #[test]
    fn log_file_flush_policy_accepts_explicit_per_line_aliases() {
        assert_eq!(
            LogFileFlushPolicy::from_env_value("always"),
            LogFileFlushPolicy::Always
        );
        assert_eq!(
            LogFileFlushPolicy::from_env_value(" LINE "),
            LogFileFlushPolicy::Always
        );
        assert_eq!(
            LogFileFlushPolicy::from_env_value("true"),
            LogFileFlushPolicy::Always
        );
        assert_eq!(
            LogFileFlushPolicy::from_env_value("1"),
            LogFileFlushPolicy::Always
        );
    }

    #[test]
    fn log_file_flush_policy_defaults_to_buffered_without_opt_in() {
        assert_eq!(
            LogFileFlushPolicy::from_env_value("buffered"),
            LogFileFlushPolicy::Buffered
        );
        assert_eq!(
            LogFileFlushPolicy::from_env_value("false"),
            LogFileFlushPolicy::Buffered
        );
        assert_eq!(
            LogFileFlushPolicy::from_env_value("unexpected"),
            LogFileFlushPolicy::Buffered
        );
        assert!(!LogFileFlushPolicy::Buffered.should_flush());
        assert!(LogFileFlushPolicy::Always.should_flush());
    }

    #[test]
    fn trace_and_debug_logs_are_file_only() {
        assert!(!should_notify(super::LOG_LEVEL_TRACE));
        assert!(!should_notify(super::LOG_LEVEL_DEBUG));
    }

    #[test]
    fn info_and_above_still_notify() {
        assert!(should_notify(super::LOG_LEVEL_INFO));
        assert!(should_notify(super::LOG_LEVEL_WARN));
        assert!(should_notify(super::LOG_LEVEL_ERROR));
    }
}
