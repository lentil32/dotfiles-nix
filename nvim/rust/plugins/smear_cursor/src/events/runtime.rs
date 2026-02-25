use super::event_loop::{self, ExternalEventTimerKind};
use super::logging::{set_log_level, warn};
use super::{ENGINE_CONTEXT, EngineState, RuntimeStateGuard};
use crate::config::RuntimeConfig;
use crate::draw::clear_all_namespaces;
use crate::types::DEFAULT_RNG_STATE;
use nvim_oxi::libuv::TimerHandle;
use std::time::{SystemTime, UNIX_EPOCH};

pub(super) fn clear_animation_timer() {
    event_loop::clear_animation_timer();
}

pub(super) fn set_animation_timer(handle: TimerHandle, generation: u64) {
    event_loop::set_animation_timer(handle, generation);
}

pub(super) fn is_animation_timer_scheduled() -> bool {
    event_loop::is_animation_timer_scheduled()
}

pub(super) fn animation_timer_generation() -> Option<u64> {
    event_loop::animation_timer_generation()
}

pub(super) fn clear_external_event_timer() {
    event_loop::clear_external_event_timer();
}

pub(super) fn external_event_timer_kind() -> Option<ExternalEventTimerKind> {
    event_loop::external_event_timer_kind()
}

pub(super) fn set_external_event_timer(handle: TimerHandle, kind: ExternalEventTimerKind) {
    event_loop::set_external_event_timer(handle, kind);
}

pub(super) fn clear_key_event_timer() {
    event_loop::clear_key_event_timer();
}

pub(super) fn set_key_event_timer(handle: TimerHandle) {
    event_loop::set_key_event_timer(handle);
}

pub(super) fn clear_render_cleanup_timer() {
    event_loop::clear_render_cleanup_timer();
}

pub(super) fn set_render_cleanup_timer(handle: TimerHandle) {
    event_loop::set_render_cleanup_timer(handle);
}

pub(super) fn bump_render_cleanup_generation() -> u64 {
    let mut state = engine_lock();
    state.bump_render_cleanup_generation()
}

pub(super) fn current_render_cleanup_generation() -> u64 {
    let state = engine_lock();
    state.current_render_cleanup_generation()
}

pub(super) fn bump_render_generation() -> u64 {
    let mut state = engine_lock();
    state.bump_render_generation()
}

pub(super) fn current_render_generation() -> u64 {
    let state = engine_lock();
    state.current_render_generation()
}

pub(super) fn invalidate_render_cleanup() {
    bump_render_cleanup_generation();
    clear_render_cleanup_timer();
}

pub(super) fn clear_external_trigger_pending() {
    event_loop::clear_external_trigger_pending();
}

pub(super) fn clear_cmdline_redraw_pending() {
    event_loop::clear_cmdline_redraw_pending();
}

pub(super) fn mark_external_trigger_pending_if_idle() -> bool {
    event_loop::mark_external_trigger_pending_if_idle()
}

pub(super) fn complete_external_trigger_dispatch() -> bool {
    event_loop::complete_external_trigger_dispatch()
}

pub(super) fn mark_cmdline_redraw_pending_if_idle() -> bool {
    event_loop::mark_cmdline_redraw_pending_if_idle()
}

pub(super) fn note_autocmd_event_now() {
    event_loop::note_autocmd_event(now_ms());
}

pub(super) fn note_external_dispatch_now() {
    event_loop::note_external_dispatch(now_ms());
}

pub(super) fn clear_autocmd_event_timestamp() {
    event_loop::clear_autocmd_event_timestamp();
}

pub(super) fn clear_external_dispatch_timestamp() {
    event_loop::clear_external_dispatch_timestamp();
}

pub(super) fn elapsed_ms_since_last_autocmd_event(now_ms: f64) -> f64 {
    event_loop::elapsed_ms_since_last_autocmd_event(now_ms)
}

pub(super) fn elapsed_ms_since_last_external_dispatch(now_ms: f64) -> f64 {
    event_loop::elapsed_ms_since_last_external_dispatch(now_ms)
}

pub(super) fn record_cursor_callback_duration(duration_ms: f64) {
    event_loop::record_cursor_callback_duration(duration_ms);
}

pub(super) fn clear_cursor_callback_duration_estimate() {
    event_loop::clear_cursor_callback_duration_estimate();
}

pub(super) fn cursor_callback_duration_estimate_ms() -> f64 {
    event_loop::cursor_callback_duration_estimate_ms()
}

pub(super) fn reset_transient_event_state() {
    clear_animation_timer();
    clear_external_event_timer();
    clear_key_event_timer();
    clear_external_trigger_pending();
    clear_cmdline_redraw_pending();
    clear_autocmd_event_timestamp();
    clear_external_dispatch_timestamp();
    clear_cursor_callback_duration_estimate();
    invalidate_render_cleanup();
    let _ = bump_render_generation();
}

pub(super) fn reset_transient_event_state_without_generation_bump() {
    clear_animation_timer();
    clear_external_event_timer();
    clear_key_event_timer();
    clear_external_trigger_pending();
    clear_cmdline_redraw_pending();
    clear_autocmd_event_timestamp();
    clear_external_dispatch_timestamp();
    clear_cursor_callback_duration_estimate();
    clear_render_cleanup_timer();
}

pub(super) fn engine_lock() -> std::sync::MutexGuard<'static, EngineState> {
    loop {
        match ENGINE_CONTEXT.state.lock() {
            Ok(guard) => return guard,
            Err(poisoned) => {
                let mut guard = poisoned.into_inner();
                let namespace_id = guard.runtime.namespace_id();
                *guard = EngineState::default();
                drop(guard);

                ENGINE_CONTEXT.state.clear_poison();
                set_log_level(RuntimeConfig::default().logging_level);
                warn("state mutex poisoned; resetting runtime state");
                if let Some(namespace_id) = namespace_id {
                    clear_all_namespaces(namespace_id);
                }
                reset_transient_event_state_without_generation_bump();
            }
        }
    }
}

pub(super) fn state_lock() -> RuntimeStateGuard {
    RuntimeStateGuard(engine_lock())
}

pub(super) fn is_enabled() -> bool {
    let state = state_lock();
    state.is_enabled()
}

pub(super) fn now_ms() -> f64 {
    let duration = match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration,
        Err(err) => err.duration(),
    };
    duration.as_secs_f64() * 1000.0
}

pub(super) fn seed_from_clock() -> u32 {
    let duration = match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration,
        Err(err) => err.duration(),
    };

    let mut seed = (duration.as_nanos() & u128::from(u32::MAX)) as u32;
    if seed == 0 {
        seed = DEFAULT_RNG_STATE;
    }
    seed
}
