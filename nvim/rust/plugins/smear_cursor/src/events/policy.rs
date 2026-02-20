use super::cursor::{current_buffer_buftype, current_buffer_filetype};
use super::event_loop::ExternalEventTimerKind;
use super::runtime::{cursor_callback_duration_estimate_ms, state_lock};
use crate::reducer::as_delay_ms;
use nvim_oxi::api::opts::OptionOpts;
use nvim_oxi::{Result, api};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BufferEventPolicy {
    Normal,
}

impl BufferEventPolicy {
    pub(super) fn from_buffer_metadata(
        _buftype: &str,
        _buflisted: bool,
        _line_count: i64,
        _callback_duration_estimate_ms: f64,
    ) -> Self {
        Self::Normal
    }

    pub(super) const fn settle_delay_floor_ms(self) -> u64 {
        0
    }

    pub(super) const fn animation_delay_floor_ms(self) -> u64 {
        0
    }

    pub(super) const fn should_use_debounced_external_settle(self) -> bool {
        true
    }

    pub(super) const fn use_key_fallback(self) -> bool {
        true
    }

    pub(super) const fn should_prepaint_cursor(self) -> bool {
        true
    }
}

pub(super) fn remaining_throttle_delay_ms(throttle_interval_ms: u64, elapsed_ms: f64) -> u64 {
    as_delay_ms((throttle_interval_ms as f64 - elapsed_ms).max(0.0))
}

pub(super) fn should_replace_external_timer_with_throttle(
    existing_kind: Option<ExternalEventTimerKind>,
) -> bool {
    matches!(existing_kind, Some(ExternalEventTimerKind::Settle))
}

pub(super) fn current_buffer_event_policy(buffer: &api::Buffer) -> Result<BufferEventPolicy> {
    let buftype = current_buffer_buftype(buffer)?;
    let opts = OptionOpts::builder().buf(buffer.clone()).build();
    let buflisted: bool = api::get_option_value("buflisted", &opts)?;
    let line_count_usize = buffer.line_count()?;
    let line_count = match i64::try_from(line_count_usize) {
        Ok(value) => value,
        Err(_) => i64::MAX,
    };
    let callback_duration_estimate_ms = cursor_callback_duration_estimate_ms();
    Ok(BufferEventPolicy::from_buffer_metadata(
        &buftype,
        buflisted,
        line_count,
        callback_duration_estimate_ms,
    ))
}

pub(super) fn skip_current_buffer_events(buffer: &api::Buffer) -> Result<bool> {
    let filetypes_disabled = {
        let state = state_lock();
        if state.is_delay_disabled() {
            return Ok(true);
        }
        state.config.filetypes_disabled.clone()
    };

    if filetypes_disabled.is_empty() {
        return Ok(false);
    }

    let filetype = current_buffer_filetype(buffer)?;
    Ok(filetypes_disabled.iter().any(|entry| entry == &filetype))
}
