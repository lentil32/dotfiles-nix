use super::cursor::mode_string;
use super::event_loop::ExternalEventTimerKind;
use super::geometry::{current_cursor_snapshot, snapshots_match};
use super::handlers;
use super::logging::warn;
use super::policy::should_replace_external_timer_with_throttle;
use super::runtime::{
    bump_render_cleanup_generation, clear_animation_timer, clear_external_event_timer,
    clear_key_event_timer, clear_render_cleanup_timer, current_render_cleanup_generation,
    external_event_timer_kind, invalidate_render_cleanup, is_animation_timer_scheduled,
    set_animation_timer, set_external_event_timer, set_key_event_timer, set_render_cleanup_timer,
    state_lock,
};
use super::{AUTOCMD_GROUP_NAME, MIN_RENDER_CLEANUP_DELAY_MS};
use crate::config::RuntimeConfig;
use crate::draw::purge_render_windows;
use crate::reducer::{EventSource, RenderCleanupAction, as_delay_ms, external_settle_delay_ms};
use crate::state::CursorSnapshot;
use nvim_oxi::api::opts::CreateAugroupOpts;
use nvim_oxi::libuv::TimerHandle;
use nvim_oxi::{Result, api, schedule};
use std::time::Duration;

fn on_animation_tick() -> Result<()> {
    handlers::on_cursor_event_impl(EventSource::AnimationTick)
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum ExternalSettleAction {
    None,
    ClearPending,
    DispatchExternal,
    Reschedule(CursorSnapshot),
}

pub(super) fn decide_external_settle_action(
    mode: &str,
    smear_to_cmd: bool,
    expected_snapshot: Option<&CursorSnapshot>,
    current_snapshot: Option<&CursorSnapshot>,
) -> ExternalSettleAction {
    if mode == "c" && !smear_to_cmd {
        return ExternalSettleAction::ClearPending;
    }

    let Some(expected_snapshot) = expected_snapshot else {
        return ExternalSettleAction::None;
    };

    let Some(current_snapshot) = current_snapshot else {
        return ExternalSettleAction::ClearPending;
    };

    if snapshots_match(current_snapshot, expected_snapshot) {
        ExternalSettleAction::DispatchExternal
    } else {
        ExternalSettleAction::Reschedule(current_snapshot.clone())
    }
}

fn on_external_settle_tick() -> Result<()> {
    let mode = mode_string();
    let (smear_to_cmd, delay_ms, expected_snapshot) = {
        let state = state_lock();
        (
            state.config.smear_to_cmd,
            external_settle_delay_ms(state.config.delay_event_to_smear),
            state.pending_external_event_cloned(),
        )
    };
    let current_snapshot = if expected_snapshot.is_some() && (mode != "c" || smear_to_cmd) {
        current_cursor_snapshot(smear_to_cmd)?
    } else {
        None
    };
    let action = decide_external_settle_action(
        &mode,
        smear_to_cmd,
        expected_snapshot.as_ref(),
        current_snapshot.as_ref(),
    );

    match action {
        ExternalSettleAction::None => Ok(()),
        ExternalSettleAction::ClearPending => {
            let mut state = state_lock();
            state.clear_pending_external_event();
            Ok(())
        }
        ExternalSettleAction::DispatchExternal => {
            let mut state = state_lock();
            state.clear_pending_external_event();
            drop(state);
            handlers::on_cursor_event_impl(EventSource::External)
        }
        ExternalSettleAction::Reschedule(snapshot) => {
            let mut state = state_lock();
            state.set_pending_external_event(Some(snapshot));
            drop(state);
            schedule_external_event_timer(delay_ms);
            Ok(())
        }
    }
}

pub(super) fn schedule_animation_tick(delay_ms: u64) {
    let already_scheduled = is_animation_timer_scheduled();
    if already_scheduled {
        return;
    }

    let timeout = Duration::from_millis(delay_ms);
    match TimerHandle::once(timeout, || {
        schedule(|_| {
            clear_animation_timer();
            if let Err(err) = on_animation_tick() {
                warn(&format!("animation tick failed: {err}"));
            }
        });
    }) {
        Ok(handle) => {
            set_animation_timer(handle);
        }
        Err(err) => {
            warn(&format!("failed to schedule animation tick: {err}"));
        }
    }
}

pub(super) fn schedule_external_throttle_timer(delay_ms: u64) {
    let existing_kind = external_event_timer_kind();
    if existing_kind == Some(ExternalEventTimerKind::Throttle) {
        return;
    }
    if should_replace_external_timer_with_throttle(existing_kind) {
        clear_external_event_timer();
    }

    let timeout = Duration::from_millis(delay_ms);
    match TimerHandle::once(timeout, || {
        schedule(|_| {
            clear_external_event_timer();
            if let Err(err) = handlers::on_external_event_trigger() {
                warn(&format!("external throttle tick failed: {err}"));
            }
        });
    }) {
        Ok(handle) => {
            set_external_event_timer(handle, ExternalEventTimerKind::Throttle);
        }
        Err(err) => {
            warn(&format!("failed to schedule external throttle tick: {err}"));
        }
    }
}

pub(super) fn schedule_external_event_timer(delay_ms: u64) {
    clear_external_event_timer();

    let timeout = Duration::from_millis(delay_ms);
    match TimerHandle::once(timeout, || {
        schedule(|_| {
            clear_external_event_timer();
            if let Err(err) = on_external_settle_tick() {
                warn(&format!("external settle tick failed: {err}"));
            }
        });
    }) {
        Ok(handle) => {
            set_external_event_timer(handle, ExternalEventTimerKind::Settle);
        }
        Err(err) => {
            warn(&format!("failed to schedule external settle tick: {err}"));
        }
    }
}

pub(super) fn schedule_key_event_timer(delay_ms: u64) {
    clear_key_event_timer();

    if delay_ms == 0 {
        handlers::schedule_external_event_trigger();
        return;
    }

    let timeout = Duration::from_millis(delay_ms);
    match TimerHandle::once(timeout, || {
        schedule(|_| {
            clear_key_event_timer();
            handlers::schedule_external_event_trigger();
        });
    }) {
        Ok(handle) => {
            set_key_event_timer(handle);
        }
        Err(err) => {
            warn(&format!("failed to schedule key-event tick: {err}"));
        }
    }
}

pub(super) fn render_cleanup_delay_ms(config: &RuntimeConfig) -> u64 {
    let baseline =
        as_delay_ms(config.time_interval + config.delay_event_to_smear + config.delay_after_key);
    baseline.max(MIN_RENDER_CLEANUP_DELAY_MS)
}

fn schedule_render_cleanup_timer(namespace_id: u32, delay_ms: u64) {
    let generation = bump_render_cleanup_generation();
    clear_render_cleanup_timer();

    let timeout = Duration::from_millis(delay_ms);
    match TimerHandle::once(timeout, move || {
        schedule(move |_| {
            clear_render_cleanup_timer();

            if current_render_cleanup_generation() != generation {
                return;
            }
            purge_render_windows(namespace_id);
        });
    }) {
        Ok(handle) => {
            set_render_cleanup_timer(handle);
        }
        Err(err) => {
            warn(&format!("failed to schedule render cleanup: {err}"));
        }
    }
}

fn schedule_render_cleanup(namespace_id: u32) {
    let delay_ms = {
        let state = state_lock();
        render_cleanup_delay_ms(&state.config)
    };
    schedule_render_cleanup_timer(namespace_id, delay_ms);
}

pub(super) fn apply_render_cleanup_action(namespace_id: u32, action: RenderCleanupAction) {
    match action {
        RenderCleanupAction::None => {}
        RenderCleanupAction::Schedule => schedule_render_cleanup(namespace_id),
        RenderCleanupAction::Invalidate => invalidate_render_cleanup(),
    }
}

pub(super) fn set_on_key_listener(namespace_id: u32, enabled: bool) -> Result<()> {
    let command = if enabled {
        format!(
            "lua vim.on_key(function(_, _) require('rs_smear_cursor').on_key() end, {namespace_id})"
        )
    } else {
        format!("lua vim.on_key(nil, {namespace_id})")
    };
    api::command(&command)?;
    Ok(())
}

pub(super) fn clear_autocmd_group() {
    let opts = CreateAugroupOpts::builder().clear(true).build();
    if let Err(err) = api::create_augroup(AUTOCMD_GROUP_NAME, &opts) {
        warn(&format!("clear autocmd group failed: {err}"));
    }
}

pub(super) fn ensure_namespace_id() -> u32 {
    if let Some(namespace_id) = {
        let state = state_lock();
        state.namespace_id()
    } {
        return namespace_id;
    }

    let created = api::create_namespace("rs_smear_cursor");
    let mut state = state_lock();
    match state.namespace_id() {
        Some(existing) => existing,
        None => {
            state.set_namespace_id(created);
            created
        }
    }
}
