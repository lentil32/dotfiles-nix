use super::LOG_SOURCE_NAME;
use super::RealCursorVisibility;
use super::runtime::clear_real_cursor_visibility;
use super::runtime::note_real_cursor_visibility;
use super::runtime::real_cursor_visibility_matches;
use super::runtime::set_runtime_log_level;
use super::runtime::should_runtime_log;
use super::runtime::with_runtime_log_file_handle;
use crate::config::LogLevel;
use crate::host::CursorVisibilityPort;
use crate::host::HostCursorVisibility;
use crate::host::HostLoggingPort;
use crate::host::NeovimHost;
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
// Keep file logging buffered unless a live-tailed session explicitly opts into always-flush mode.
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
        if normalized.eq_ignore_ascii_case("always") {
            Self::Always
        } else {
            Self::Buffered
        }
    }

    fn should_flush(self) -> bool {
        matches!(self, Self::Always)
    }
}

#[derive(Debug)]
pub(super) struct LogFileWriter {
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

pub(super) fn set_log_level(level: LogLevel) {
    set_runtime_log_level(level);
}

fn should_log(level: LogLevel) -> bool {
    should_runtime_log(level)
}

fn should_notify(level: LogLevel) -> bool {
    level.should_notify()
}

fn log_timestamp_ms() -> u128 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_millis(),
        Err(err) => err.duration().as_millis(),
    }
}

fn append_log_line_with(host: &impl HostLoggingPort, level_name: &str, message: &str) {
    let Some(path) = LOG_FILE_PATH.as_ref() else {
        return;
    };
    with_runtime_log_file_handle(|file_handle| {
        if file_handle.is_none() {
            match LogFileWriter::open(path) {
                Ok(file_writer) => {
                    *file_handle = Some(file_writer);
                }
                Err(err) => {
                    host.write_error(&format!(
                        "[{LOG_SOURCE_NAME}] failed to open log file {}: {err}",
                        path.display()
                    ));
                    return;
                }
            }
        }

        if let Some(file_writer) = file_handle.as_mut()
            && let Err(err) = file_writer.append_line(level_name, message)
        {
            host.write_error(&format!(
                "[{LOG_SOURCE_NAME}] failed to write log file: {err}"
            ));
            *file_handle = None;
        }
    });
}

fn notify_log(level: LogLevel, message: &str) {
    notify_log_with(&NeovimHost, level, message);
}

fn notify_log_with(host: &impl HostLoggingPort, level: LogLevel, message: &str) {
    if !should_log(level) {
        return;
    }

    append_log_line_with(host, level.name(), message);
    if !should_notify(level) {
        return;
    }

    notify_host_with(host, level, message);
}

fn notify_host_with(host: &impl HostLoggingPort, level: LogLevel, message: &str) {
    let level_name = level.name();
    let payload_message = format!("[{LOG_SOURCE_NAME}][{level_name}] {message}");
    if let Err(err) = host.notify(&payload_message, level) {
        host.write_error(&format!("[{LOG_SOURCE_NAME}] vim.notify failed: {err}"));
    }
}

pub(crate) fn warn(message: &str) {
    notify_log(LogLevel::Warn, message);
}

pub(super) fn trace_lazy(message: impl FnOnce() -> String) {
    if !should_log(LogLevel::Trace) {
        return;
    }
    notify_log(LogLevel::Trace, &message());
}

pub(super) fn debug(message: &str) {
    notify_log(LogLevel::Debug, message);
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
    append_log_line_with(&NeovimHost, "PERF", &message);
}

pub(super) fn ensure_hideable_guicursor() {
    ensure_hideable_guicursor_with(&NeovimHost);
}

fn ensure_hideable_guicursor_with(host: &impl CursorVisibilityPort) {
    let Ok(mut guicursor) = host.guicursor() else {
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
    if let Err(err) = host.set_guicursor(&guicursor) {
        warn(&format!("set guicursor failed: {err}"));
    }
}

fn should_skip_real_cursor_visibility_update(visibility: RealCursorVisibility) -> bool {
    real_cursor_visibility_matches(visibility).unwrap_or(false)
}

fn apply_real_cursor_visibility(visibility: RealCursorVisibility) {
    apply_real_cursor_visibility_with(&NeovimHost, visibility);
}

fn apply_real_cursor_visibility_with(
    host: &impl CursorVisibilityPort,
    visibility: RealCursorVisibility,
) {
    if should_skip_real_cursor_visibility_update(visibility) {
        return;
    }

    let host_visibility = match visibility {
        RealCursorVisibility::Hidden => HostCursorVisibility::Hidden,
        RealCursorVisibility::Visible => HostCursorVisibility::Visible,
    };
    let error_message = match visibility {
        RealCursorVisibility::Hidden => "set highlight failed",
        RealCursorVisibility::Visible => "restore highlight failed",
    };

    if let Err(err) = host.set_cursor_highlight_visibility(host_visibility) {
        warn(&format!("{error_message}: {err}"));
        return;
    }

    let _ = note_real_cursor_visibility(visibility);
}

pub(super) fn invalidate_real_cursor_visibility() {
    let _ = clear_real_cursor_visibility();
}

pub(super) fn hide_real_cursor() {
    apply_real_cursor_visibility(RealCursorVisibility::Hidden);
}

pub(super) fn unhide_real_cursor() {
    apply_real_cursor_visibility(RealCursorVisibility::Visible);
}

#[cfg(test)]
mod tests {
    use super::LogFileFlushPolicy;
    use super::RealCursorVisibility;
    use super::apply_real_cursor_visibility_with;
    use super::ensure_hideable_guicursor_with;
    use super::notify_host_with;
    use super::should_notify;
    use super::should_skip_real_cursor_visibility_update;
    use crate::config::LogLevel;
    use crate::events::runtime::clear_real_cursor_visibility;
    use crate::events::runtime::note_real_cursor_visibility;
    use crate::host::CursorVisibilityCall;
    use crate::host::FakeCursorVisibilityPort;
    use crate::host::FakeHostLoggingPort;
    use crate::host::HostCursorVisibility;
    use crate::host::HostLoggingCall;
    use pretty_assertions::assert_eq;

    #[test]
    fn log_file_flush_policy_accepts_only_the_canonical_opt_in() {
        assert_eq!(
            LogFileFlushPolicy::from_env_value("always"),
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
        assert!(!should_notify(LogLevel::Trace));
        assert!(!should_notify(LogLevel::Debug));
    }

    #[test]
    fn info_and_above_still_notify() {
        assert!(should_notify(LogLevel::Info));
        assert!(should_notify(LogLevel::Warn));
        assert!(should_notify(LogLevel::Error));
    }

    #[test]
    fn warning_notifications_route_through_host_logging_port() {
        let host = FakeHostLoggingPort::default();

        notify_host_with(&host, LogLevel::Warn, "low contrast cursor");

        assert_eq!(
            host.calls(),
            vec![HostLoggingCall::Notify {
                message: "[smear_cursor][WARNING] low contrast cursor".to_string(),
                level: LogLevel::Warn,
            }]
        );
    }

    #[test]
    fn notification_failure_routes_diagnostic_error_through_host_logging_port() {
        let host = FakeHostLoggingPort::default();
        host.push_notify_error("notify unavailable");

        notify_host_with(&host, LogLevel::Warn, "low contrast cursor");

        assert_eq!(
            host.calls(),
            vec![
                HostLoggingCall::Notify {
                    message: "[smear_cursor][WARNING] low contrast cursor".to_string(),
                    level: LogLevel::Warn,
                },
                HostLoggingCall::WriteError {
                    message: "[smear_cursor] vim.notify failed: notify unavailable".to_string(),
                },
            ]
        );
    }

    #[test]
    fn ensure_hideable_guicursor_routes_option_reads_and_writes_through_cursor_port() {
        let host = FakeCursorVisibilityPort::default();
        host.set_initial_guicursor("n-v-c:block");

        ensure_hideable_guicursor_with(&host);

        assert_eq!(
            host.calls(),
            vec![
                CursorVisibilityCall::Guicursor,
                CursorVisibilityCall::SetGuicursor {
                    value: "n-v-c:block,a:SmearCursorHideable".to_string(),
                },
            ]
        );
    }

    #[test]
    fn real_cursor_visibility_routes_highlight_writes_through_cursor_port() {
        clear_real_cursor_visibility().expect("shell state access should succeed");
        let host = FakeCursorVisibilityPort::default();

        apply_real_cursor_visibility_with(&host, RealCursorVisibility::Hidden);
        apply_real_cursor_visibility_with(&host, RealCursorVisibility::Hidden);
        apply_real_cursor_visibility_with(&host, RealCursorVisibility::Visible);

        assert_eq!(
            host.calls(),
            vec![
                CursorVisibilityCall::SetCursorHighlightVisibility {
                    visibility: HostCursorVisibility::Hidden,
                },
                CursorVisibilityCall::SetCursorHighlightVisibility {
                    visibility: HostCursorVisibility::Visible,
                },
            ]
        );
    }

    #[test]
    fn real_cursor_visibility_skip_only_matches_the_cached_state() {
        clear_real_cursor_visibility().expect("shell state access should succeed");
        assert!(!should_skip_real_cursor_visibility_update(
            RealCursorVisibility::Hidden
        ));
        assert!(!should_skip_real_cursor_visibility_update(
            RealCursorVisibility::Visible
        ));

        note_real_cursor_visibility(RealCursorVisibility::Hidden)
            .expect("shell state access should succeed");
        assert!(should_skip_real_cursor_visibility_update(
            RealCursorVisibility::Hidden
        ));
        assert!(!should_skip_real_cursor_visibility_update(
            RealCursorVisibility::Visible
        ));
    }
}
