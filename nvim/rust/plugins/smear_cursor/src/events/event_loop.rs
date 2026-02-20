use nvim_oxi::libuv::TimerHandle;
use std::cell::RefCell;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ExternalEventTimerKind {
    Settle,
    Throttle,
}

pub(super) struct EventLoopState {
    animation_timer: Option<TimerHandle>,
    external_event_timer: Option<(TimerHandle, ExternalEventTimerKind)>,
    key_event_timer: Option<TimerHandle>,
    render_cleanup_timer: Option<TimerHandle>,
    external_trigger_pending: bool,
    last_autocmd_event_ms: f64,
    last_external_dispatch_ms: f64,
    callback_duration_ewma_ms: f64,
}

impl EventLoopState {
    const CALLBACK_DURATION_EWMA_ALPHA: f64 = 0.25;

    pub(super) const fn new() -> Self {
        Self {
            animation_timer: None,
            external_event_timer: None,
            key_event_timer: None,
            render_cleanup_timer: None,
            external_trigger_pending: false,
            last_autocmd_event_ms: 0.0,
            last_external_dispatch_ms: 0.0,
            callback_duration_ewma_ms: 0.0,
        }
    }

    pub(super) fn is_animation_timer_scheduled(&self) -> bool {
        self.animation_timer.is_some()
    }

    pub(super) fn clear_animation_timer(&mut self) {
        let _ = self.animation_timer.take();
    }

    pub(super) fn set_animation_timer(&mut self, handle: TimerHandle) {
        self.animation_timer = Some(handle);
    }

    pub(super) fn clear_external_event_timer(&mut self) {
        let _ = self.external_event_timer.take();
    }

    pub(super) fn external_event_timer_kind(&self) -> Option<ExternalEventTimerKind> {
        self.external_event_timer.as_ref().map(|(_, kind)| *kind)
    }

    pub(super) fn set_external_event_timer(
        &mut self,
        handle: TimerHandle,
        kind: ExternalEventTimerKind,
    ) {
        self.external_event_timer = Some((handle, kind));
    }

    pub(super) fn clear_key_event_timer(&mut self) {
        let _ = self.key_event_timer.take();
    }

    pub(super) fn set_key_event_timer(&mut self, handle: TimerHandle) {
        self.key_event_timer = Some(handle);
    }

    pub(super) fn clear_render_cleanup_timer(&mut self) {
        let _ = self.render_cleanup_timer.take();
    }

    pub(super) fn set_render_cleanup_timer(&mut self, handle: TimerHandle) {
        self.render_cleanup_timer = Some(handle);
    }

    pub(super) fn clear_external_trigger_pending(&mut self) {
        self.external_trigger_pending = false;
    }

    pub(super) fn mark_external_trigger_pending_if_idle(&mut self) -> bool {
        if self.external_trigger_pending {
            false
        } else {
            self.external_trigger_pending = true;
            true
        }
    }

    pub(super) fn note_autocmd_event(&mut self, now_ms: f64) {
        self.last_autocmd_event_ms = now_ms;
    }

    pub(super) fn clear_autocmd_event_timestamp(&mut self) {
        self.last_autocmd_event_ms = 0.0;
    }

    pub(super) fn elapsed_ms_since_last_autocmd_event(&self, now_ms: f64) -> f64 {
        let last = self.last_autocmd_event_ms;
        if last <= 0.0 {
            f64::INFINITY
        } else {
            (now_ms - last).max(0.0)
        }
    }

    pub(super) fn note_external_dispatch(&mut self, now_ms: f64) {
        self.last_external_dispatch_ms = now_ms;
    }

    pub(super) fn clear_external_dispatch_timestamp(&mut self) {
        self.last_external_dispatch_ms = 0.0;
    }

    pub(super) fn elapsed_ms_since_last_external_dispatch(&self, now_ms: f64) -> f64 {
        let last = self.last_external_dispatch_ms;
        if last <= 0.0 {
            f64::INFINITY
        } else {
            (now_ms - last).max(0.0)
        }
    }

    pub(super) fn record_cursor_callback_duration(&mut self, duration_ms: f64) {
        if !duration_ms.is_finite() {
            return;
        }

        let observed = duration_ms.max(0.0);
        let previous = self.callback_duration_ewma_ms;
        self.callback_duration_ewma_ms = if previous <= 0.0 {
            observed
        } else {
            previous + Self::CALLBACK_DURATION_EWMA_ALPHA * (observed - previous)
        };
    }

    pub(super) fn clear_cursor_callback_duration_estimate(&mut self) {
        self.callback_duration_ewma_ms = 0.0;
    }

    pub(super) fn cursor_callback_duration_estimate_ms(&self) -> f64 {
        self.callback_duration_ewma_ms.max(0.0)
    }
}

thread_local! {
    static EVENT_LOOP_STATE: RefCell<EventLoopState> = const { RefCell::new(EventLoopState::new()) };
}

fn with_event_loop_state<R>(mutator: impl FnOnce(&mut EventLoopState) -> R) -> R {
    EVENT_LOOP_STATE.with(|state| {
        let mut state = state.borrow_mut();
        mutator(&mut state)
    })
}

fn read_event_loop_state<R>(reader: impl FnOnce(&EventLoopState) -> R) -> R {
    EVENT_LOOP_STATE.with(|state| {
        let state = state.borrow();
        reader(&state)
    })
}

pub(super) fn clear_animation_timer() {
    with_event_loop_state(EventLoopState::clear_animation_timer);
}

pub(super) fn set_animation_timer(handle: TimerHandle) {
    with_event_loop_state(|state| state.set_animation_timer(handle));
}

pub(super) fn is_animation_timer_scheduled() -> bool {
    read_event_loop_state(EventLoopState::is_animation_timer_scheduled)
}

pub(super) fn clear_external_event_timer() {
    with_event_loop_state(EventLoopState::clear_external_event_timer);
}

pub(super) fn external_event_timer_kind() -> Option<ExternalEventTimerKind> {
    read_event_loop_state(EventLoopState::external_event_timer_kind)
}

pub(super) fn set_external_event_timer(handle: TimerHandle, kind: ExternalEventTimerKind) {
    with_event_loop_state(|state| state.set_external_event_timer(handle, kind));
}

pub(super) fn clear_key_event_timer() {
    with_event_loop_state(EventLoopState::clear_key_event_timer);
}

pub(super) fn set_key_event_timer(handle: TimerHandle) {
    with_event_loop_state(|state| state.set_key_event_timer(handle));
}

pub(super) fn clear_render_cleanup_timer() {
    with_event_loop_state(EventLoopState::clear_render_cleanup_timer);
}

pub(super) fn set_render_cleanup_timer(handle: TimerHandle) {
    with_event_loop_state(|state| state.set_render_cleanup_timer(handle));
}

pub(super) fn clear_external_trigger_pending() {
    with_event_loop_state(EventLoopState::clear_external_trigger_pending);
}

pub(super) fn mark_external_trigger_pending_if_idle() -> bool {
    with_event_loop_state(EventLoopState::mark_external_trigger_pending_if_idle)
}

pub(super) fn note_autocmd_event(now_ms: f64) {
    with_event_loop_state(|state| state.note_autocmd_event(now_ms));
}

pub(super) fn clear_autocmd_event_timestamp() {
    with_event_loop_state(EventLoopState::clear_autocmd_event_timestamp);
}

pub(super) fn elapsed_ms_since_last_autocmd_event(now_ms: f64) -> f64 {
    read_event_loop_state(|state| state.elapsed_ms_since_last_autocmd_event(now_ms))
}

pub(super) fn note_external_dispatch(now_ms: f64) {
    with_event_loop_state(|state| state.note_external_dispatch(now_ms));
}

pub(super) fn clear_external_dispatch_timestamp() {
    with_event_loop_state(EventLoopState::clear_external_dispatch_timestamp);
}

pub(super) fn elapsed_ms_since_last_external_dispatch(now_ms: f64) -> f64 {
    read_event_loop_state(|state| state.elapsed_ms_since_last_external_dispatch(now_ms))
}

pub(super) fn record_cursor_callback_duration(duration_ms: f64) {
    with_event_loop_state(|state| state.record_cursor_callback_duration(duration_ms));
}

pub(super) fn clear_cursor_callback_duration_estimate() {
    with_event_loop_state(EventLoopState::clear_cursor_callback_duration_estimate);
}

pub(super) fn cursor_callback_duration_estimate_ms() -> f64 {
    read_event_loop_state(EventLoopState::cursor_callback_duration_estimate_ms)
}
