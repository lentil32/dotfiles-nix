use super::{
    ENGINE_CONTEXT, LOG_LEVEL_DEBUG, LOG_LEVEL_ERROR, LOG_LEVEL_INFO, LOG_LEVEL_TRACE,
    LOG_LEVEL_WARN, LOG_SOURCE_NAME,
};
use nvim_oxi::{Array, Object, api};
use std::sync::atomic::Ordering;

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

fn notify_log(level: i64, message: &str) {
    if !should_log(level) {
        return;
    }

    let payload_message = format!("[{LOG_SOURCE_NAME}][{}] {message}", log_level_name(level));
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
