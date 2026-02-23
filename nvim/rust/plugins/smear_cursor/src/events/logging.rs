use super::{
    ENGINE_CONTEXT, LOG_LEVEL_DEBUG, LOG_LEVEL_ERROR, LOG_LEVEL_INFO, LOG_LEVEL_TRACE,
    LOG_LEVEL_WARN, LOG_SOURCE_NAME,
};
use nvim_oxi::{Array, Object, api};
use std::sync::atomic::Ordering;
use std::sync::{LazyLock, Mutex};
use std::{
    fs::{File, OpenOptions},
    io::Write,
    path::PathBuf,
};

static LOG_FILE_PATH: LazyLock<Option<PathBuf>> =
    LazyLock::new(|| std::env::var_os("SMEAR_CURSOR_LOG_FILE").map(PathBuf::from));
static LOG_FILE_HANDLE: LazyLock<Mutex<Option<File>>> = LazyLock::new(|| Mutex::new(None));
static PERF_SLOW_CALLBACK_THRESHOLD_MS: LazyLock<Option<f64>> = LazyLock::new(|| {
    std::env::var("SMEAR_CURSOR_PERF_SLOW_MS")
        .ok()
        .and_then(|value| value.parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value >= 0.0)
});

pub(super) fn set_log_level(level: i64) {
    let normalized = if level < 0 { 0 } else { level };
    ENGINE_CONTEXT
        .log_level
        .store(normalized, Ordering::Relaxed);
}

fn should_log(level: i64) -> bool {
    ENGINE_CONTEXT.log_level.load(Ordering::Relaxed) <= level
}

fn log_level_name(level: i64) -> &'static str {
    match level {
        LOG_LEVEL_TRACE => "TRACE",
        LOG_LEVEL_DEBUG => "DEBUG",
        LOG_LEVEL_INFO => "INFO",
        LOG_LEVEL_WARN => "WARNING",
        LOG_LEVEL_ERROR => "ERROR",
        _ => "INFO",
    }
}

fn append_log_line(level_name: &str, message: &str) {
    let Some(path) = LOG_FILE_PATH.as_ref() else {
        return;
    };
    let mut file_guard = match LOG_FILE_HANDLE.lock() {
        Ok(guard) => guard,
        Err(_) => return,
    };

    if file_guard.is_none() {
        match OpenOptions::new().create(true).append(true).open(path) {
            Ok(file) => {
                *file_guard = Some(file);
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

    if let Some(file) = file_guard.as_mut()
        && let Err(err) = writeln!(file, "[{LOG_SOURCE_NAME}][{level_name}] {message}")
    {
        api::err_writeln(&format!(
            "[{LOG_SOURCE_NAME}] failed to write log file: {err}"
        ));
        *file_guard = None;
    }
}

fn notify_log(level: i64, message: &str) {
    if !should_log(level) {
        return;
    }

    let level_name = log_level_name(level);
    append_log_line(level_name, message);
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

pub(super) fn warn(message: &str) {
    notify_log(LOG_LEVEL_WARN, message);
}

pub(super) fn debug(message: &str) {
    notify_log(LOG_LEVEL_DEBUG, message);
}

pub(super) fn log_slow_callback(
    source: &str,
    mode: &str,
    callback_duration_ms: f64,
    callback_duration_estimate_ms: f64,
    details: &str,
) {
    let Some(threshold_ms) = *PERF_SLOW_CALLBACK_THRESHOLD_MS else {
        return;
    };
    if callback_duration_ms < threshold_ms {
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
