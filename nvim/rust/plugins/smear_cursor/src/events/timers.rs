use super::cursor::mode_string;
use super::event_loop::ExternalEventTimerKind;
use super::geometry::{current_cursor_snapshot, snapshots_match};
use super::handlers;
use super::logging::warn;
use super::policy::should_replace_external_timer_with_throttle;
use super::runtime::{
    animation_timer_generation, bump_render_cleanup_generation, clear_animation_timer,
    clear_external_event_timer, clear_key_event_timer, clear_render_cleanup_timer,
    current_render_cleanup_generation, current_render_generation,
    cursor_callback_duration_estimate_ms, external_event_timer_kind, invalidate_render_cleanup,
    is_animation_timer_scheduled, set_animation_timer, set_external_event_timer,
    set_key_event_timer, set_render_cleanup_timer, state_lock,
};
use super::{
    AUTOCMD_GROUP_NAME, MIN_RENDER_CLEANUP_DELAY_MS, MIN_RENDER_HARD_PURGE_DELAY_MS,
    RENDER_HARD_PURGE_DELAY_MULTIPLIER,
};
use crate::config::RuntimeConfig;
use crate::draw::{clear_active_render_windows, global_pool_snapshot, purge_render_windows};
use crate::reducer::{
    CleanupDirective, CleanupPolicyInput, EventSource, RenderCleanupAction, as_delay_ms,
    decide_cleanup_directive, external_settle_delay_ms,
};
use crate::state::CursorSnapshot;
use nvim_oxi::api::opts::CreateAugroupOpts;
use nvim_oxi::libuv::TimerHandle;
use nvim_oxi::{Result, api, schedule};
use nvim_utils::mode::is_cmdline_mode;
use std::time::Duration;

// Timer scheduling uses one shared primitive so all timers follow the same clear/schedule policy.

fn on_animation_tick() -> Result<()> {
    handlers::on_cursor_event_impl(EventSource::AnimationTick)
}

fn schedule_once(
    delay_ms: u64,
    tick: impl FnOnce() + 'static,
    on_scheduled: impl FnOnce(TimerHandle),
    schedule_error_context: &str,
) {
    let timeout = Duration::from_millis(delay_ms);
    match TimerHandle::once(timeout, move || {
        schedule(move |_| {
            tick();
        });
    }) {
        Ok(handle) => on_scheduled(handle),
        Err(err) => {
            warn(&format!(
                "failed to schedule {schedule_error_context}: {err}"
            ));
        }
    }
}

fn schedule_external_timer(
    delay_ms: u64,
    kind: ExternalEventTimerKind,
    tick: impl FnOnce() -> Result<()> + 'static,
    tick_error_context: &'static str,
    schedule_error_context: &'static str,
) {
    schedule_once(
        delay_ms,
        move || {
            clear_external_event_timer();
            if let Err(err) = tick() {
                warn(&format!("{tick_error_context}: {err}"));
            }
        },
        |handle| set_external_event_timer(handle, kind),
        schedule_error_context,
    );
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum ExternalSettleAction {
    NoAction,
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
    if is_cmdline_mode(mode) && !smear_to_cmd {
        return ExternalSettleAction::ClearPending;
    }

    let Some(expected_snapshot) = expected_snapshot else {
        return ExternalSettleAction::NoAction;
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
    let current_snapshot =
        if expected_snapshot.is_some() && (!is_cmdline_mode(&mode) || smear_to_cmd) {
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
        ExternalSettleAction::NoAction => Ok(()),
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

pub(super) fn schedule_animation_tick(delay_ms: u64, generation: u64) {
    let existing_generation = animation_timer_generation();
    if existing_generation == Some(generation) {
        return;
    }
    if is_animation_timer_scheduled() {
        clear_animation_timer();
    }

    schedule_once(
        delay_ms,
        move || {
            clear_animation_timer();
            if current_render_generation() != generation {
                return;
            }
            if let Err(err) = on_animation_tick() {
                warn(&format!("animation tick failed: {err}"));
            }
        },
        |handle| set_animation_timer(handle, generation),
        "animation tick",
    );
}

pub(super) fn schedule_external_throttle_timer(delay_ms: u64) {
    let existing_kind = external_event_timer_kind();
    if existing_kind == Some(ExternalEventTimerKind::Throttle) {
        return;
    }
    if should_replace_external_timer_with_throttle(existing_kind) {
        clear_external_event_timer();
    }

    schedule_external_timer(
        delay_ms,
        ExternalEventTimerKind::Throttle,
        handlers::on_external_event_trigger,
        "external throttle tick failed",
        "external throttle tick",
    );
}

pub(super) fn schedule_external_event_timer(delay_ms: u64) {
    clear_external_event_timer();
    schedule_external_timer(
        delay_ms,
        ExternalEventTimerKind::Settle,
        on_external_settle_tick,
        "external settle tick failed",
        "external settle tick",
    );
}

pub(super) fn schedule_key_event_timer(delay_ms: u64) {
    clear_key_event_timer();

    if delay_ms == 0 {
        handlers::schedule_external_event_trigger();
        return;
    }

    schedule_once(
        delay_ms,
        || {
            clear_key_event_timer();
            handlers::schedule_external_event_trigger();
        },
        set_key_event_timer,
        "key-event tick",
    );
}

pub(super) fn render_cleanup_delay_ms(config: &RuntimeConfig) -> u64 {
    let baseline =
        as_delay_ms(config.time_interval + config.delay_event_to_smear + config.delay_after_key);
    baseline.max(MIN_RENDER_CLEANUP_DELAY_MS)
}

pub(super) fn render_hard_cleanup_delay_ms(config: &RuntimeConfig) -> u64 {
    let soft_delay = render_cleanup_delay_ms(config);
    let scaled = soft_delay.saturating_mul(RENDER_HARD_PURGE_DELAY_MULTIPLIER);
    scaled.max(MIN_RENDER_HARD_PURGE_DELAY_MS)
}

fn apply_cleanup_directive(namespace_id: u32, directive: CleanupDirective) {
    match directive {
        CleanupDirective::KeepWarm => {}
        CleanupDirective::SoftClear { max_kept_windows } => {
            let _ = clear_active_render_windows(namespace_id, max_kept_windows);
        }
        CleanupDirective::HardPurge => purge_render_windows(namespace_id),
    }
}

fn schedule_render_cleanup_timer(
    namespace_id: u32,
    generation: u64,
    delay_ms: u64,
    total_idle_ms: u64,
    soft_delay_ms: u64,
    hard_delay_ms: u64,
    max_kept_windows: usize,
) {
    clear_render_cleanup_timer();

    schedule_once(
        delay_ms,
        move || {
            clear_render_cleanup_timer();

            if current_render_cleanup_generation() != generation {
                return;
            }

            let pool_snapshot = global_pool_snapshot();
            let directive = decide_cleanup_directive(CleanupPolicyInput {
                idle_ms: total_idle_ms,
                soft_cleanup_delay_ms: soft_delay_ms,
                hard_cleanup_delay_ms: hard_delay_ms,
                pool_total_windows: pool_snapshot.total_windows,
                recent_frame_demand: pool_snapshot.recent_frame_demand,
                max_kept_windows,
                callback_duration_estimate_ms: cursor_callback_duration_estimate_ms(),
            });
            apply_cleanup_directive(namespace_id, directive);
        },
        set_render_cleanup_timer,
        "render cleanup",
    );
}

fn schedule_render_cleanup(namespace_id: u32) {
    let (soft_delay_ms, hard_delay_ms, max_kept_windows) = {
        let state = state_lock();
        (
            render_cleanup_delay_ms(&state.config),
            render_hard_cleanup_delay_ms(&state.config),
            state.config.max_kept_windows,
        )
    };
    let generation = bump_render_cleanup_generation();
    schedule_render_cleanup_timer(
        namespace_id,
        generation,
        hard_delay_ms,
        hard_delay_ms,
        soft_delay_ms,
        hard_delay_ms,
        max_kept_windows,
    );
}

pub(super) fn apply_render_cleanup_action(namespace_id: u32, action: RenderCleanupAction) {
    match action {
        RenderCleanupAction::NoAction => {}
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
        Option::Some(existing) => existing,
        Option::None => {
            state.set_namespace_id(created);
            created
        }
    }
}
