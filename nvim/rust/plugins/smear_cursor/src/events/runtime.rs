use super::event_loop;
use super::host_bridge::{InstalledHostBridge, installed_host_bridge};
use super::logging::{set_log_level, trace_lazy, warn};
use super::probe_cache::CursorColorCacheLookup;
use super::timers::{NvimTimerId, start_timer_once, stop_timer};
use super::trace::{timer_kind_name, timer_token_summary};
use super::{ENGINE_CONTEXT, EngineContext, EngineState, HostBridgeState};
use crate::config::RuntimeConfig;
use crate::core::effect::{Effect, EventLoopMetricEffect, TimerKind};
use crate::core::event::{
    EffectFailedEvent, EffectFailureSource, Event, TimerFiredWithTokenEvent,
    TimerLostWithTokenEvent,
};
use crate::core::runtime_reducer::as_delay_ms;
use crate::core::state::{CoreState, CursorColorProbeWitness, CursorColorSample, ProbeKind};
use crate::core::types::{DelayBudgetMs, Generation, Millis, TimerId, TimerToken};
use crate::draw::recover_all_namespaces;
use crate::types::Point;
use nvim_oxi::Result;
use nvim_utils::mode::{
    is_cmdline_mode, is_insert_like_mode, is_replace_like_mode, is_terminal_like_mode,
};
use std::cell::RefCell;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
struct CoreTimerHandle {
    shell_timer_id: NvimTimerId,
    token: TimerToken,
}

#[derive(Default)]
struct CoreTimerHandles {
    animation: Option<CoreTimerHandle>,
    ingress: Option<CoreTimerHandle>,
    recovery: Option<CoreTimerHandle>,
    cleanup: Option<CoreTimerHandle>,
}

impl CoreTimerHandles {
    fn slot_mut(&mut self, timer_id: TimerId) -> &mut Option<CoreTimerHandle> {
        match timer_id {
            TimerId::Animation => &mut self.animation,
            TimerId::Ingress => &mut self.ingress,
            TimerId::Recovery => &mut self.recovery,
            TimerId::Cleanup => &mut self.cleanup,
        }
    }

    fn replace(&mut self, handle: CoreTimerHandle) -> Option<CoreTimerHandle> {
        self.slot_mut(handle.token.id()).replace(handle)
    }

    fn clear(&mut self, timer_id: TimerId) -> Option<CoreTimerHandle> {
        self.slot_mut(timer_id).take()
    }

    fn clear_all(&mut self) -> [Option<CoreTimerHandle>; 4] {
        [
            self.animation.take(),
            self.ingress.take(),
            self.recovery.take(),
            self.cleanup.take(),
        ]
    }

    fn take_by_shell_timer_id(&mut self, shell_timer_id: NvimTimerId) -> Option<CoreTimerHandle> {
        for slot in [
            &mut self.animation,
            &mut self.ingress,
            &mut self.recovery,
            &mut self.cleanup,
        ] {
            if slot.is_some_and(|handle| handle.shell_timer_id == shell_timer_id) {
                return slot.take();
            }
        }

        None
    }
}

thread_local! {
    static CORE_TIMER_HANDLES: RefCell<CoreTimerHandles> = const { RefCell::new(CoreTimerHandles {
        animation: None,
        ingress: None,
        recovery: None,
        cleanup: None,
    }) };
}

fn with_core_timer_handles<R>(f: impl FnOnce(&mut CoreTimerHandles) -> R) -> R {
    CORE_TIMER_HANDLES.with(|handles| {
        let mut handles = handles.borrow_mut();
        f(&mut handles)
    })
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
struct IngressModePolicySnapshot(u8);

impl IngressModePolicySnapshot {
    const INSERT: u8 = 1 << 0;
    const REPLACE: u8 = 1 << 1;
    const TERMINAL: u8 = 1 << 2;
    const CMDLINE: u8 = 1 << 3;

    fn from_runtime_config(config: &RuntimeConfig) -> Self {
        Self::from_mode_flags([
            config.smear_insert_mode,
            config.smear_replace_mode,
            config.smear_terminal_mode,
            config.smear_to_cmd,
        ])
    }

    const fn from_mode_flags(mode_flags: [bool; 4]) -> Self {
        let mut encoded = 0;
        if mode_flags[0] {
            encoded |= Self::INSERT;
        }
        if mode_flags[1] {
            encoded |= Self::REPLACE;
        }
        if mode_flags[2] {
            encoded |= Self::TERMINAL;
        }
        if mode_flags[3] {
            encoded |= Self::CMDLINE;
        }
        Self(encoded)
    }

    const fn allows(self, flag: u8) -> bool {
        self.0 & flag != 0
    }

    fn mode_allowed(self, mode: &str) -> bool {
        if is_insert_like_mode(mode) {
            self.allows(Self::INSERT)
        } else if is_replace_like_mode(mode) {
            self.allows(Self::REPLACE)
        } else if is_terminal_like_mode(mode) {
            self.allows(Self::TERMINAL)
        } else if is_cmdline_mode(mode) {
            self.allows(Self::CMDLINE)
        } else {
            true
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct IngressReadSnapshot {
    enabled: bool,
    needs_initialize: bool,
    key_delay_ms: u64,
    current_corners: [Point; 4],
    mode_policy: IngressModePolicySnapshot,
    filetypes_disabled: Arc<[String]>,
}

impl IngressReadSnapshot {
    fn capture() -> Self {
        read_engine_state(|state| {
            let runtime = state.core_state.runtime();
            let config = &runtime.config;

            Self {
                enabled: runtime.is_enabled(),
                needs_initialize: state.core_state.needs_initialize(),
                key_delay_ms: as_delay_ms(config.delay_after_key),
                current_corners: runtime.current_corners(),
                mode_policy: IngressModePolicySnapshot::from_runtime_config(config),
                filetypes_disabled: Arc::clone(&config.filetypes_disabled),
            }
        })
    }

    pub(super) const fn enabled(&self) -> bool {
        self.enabled
    }

    pub(super) const fn needs_initialize(&self) -> bool {
        self.needs_initialize
    }

    pub(super) const fn key_delay_ms(&self) -> u64 {
        self.key_delay_ms
    }

    pub(super) const fn current_corners(&self) -> [Point; 4] {
        self.current_corners
    }

    pub(super) fn mode_allowed(&self, mode: &str) -> bool {
        self.mode_policy.mode_allowed(mode)
    }

    pub(super) fn has_disabled_filetypes(&self) -> bool {
        !self.filetypes_disabled.is_empty()
    }

    pub(super) fn filetype_disabled(&self, filetype: &str) -> bool {
        self.filetypes_disabled
            .iter()
            .any(|entry| entry == filetype)
    }

    #[cfg(test)]
    pub(super) fn new_for_test(
        enabled: bool,
        needs_initialize: bool,
        key_delay_ms: u64,
        current_corners: [Point; 4],
        mode_policy: (bool, bool, bool, bool),
        filetypes_disabled: Vec<String>,
    ) -> Self {
        Self {
            enabled,
            needs_initialize,
            key_delay_ms,
            current_corners,
            mode_policy: IngressModePolicySnapshot::from_mode_flags([
                mode_policy.0,
                mode_policy.1,
                mode_policy.2,
                mode_policy.3,
            ]),
            filetypes_disabled: Arc::from(filetypes_disabled),
        }
    }
}

fn set_core_timer_handle(handle: CoreTimerHandle) {
    with_core_timer_handles(|handles| {
        let _ = handles.replace(handle);
    });
}

fn stop_core_timer_handle(handle: CoreTimerHandle, context: &'static str) {
    let kind = TimerKind::from_timer_id(handle.token.id());
    trace_lazy(|| {
        format!(
            "timer_stop context={} kind={} token={} shell_timer_id={}",
            context,
            timer_kind_name(kind),
            timer_token_summary(handle.token),
            handle.shell_timer_id.get(),
        )
    });
    if let Err(err) = stop_timer(handle.shell_timer_id) {
        warn(&format!(
            "failed to stop core timer (context={context}, kind={:?}, token={:?}): {err}",
            kind, handle.token
        ));
    }
}

fn clear_core_timer_handle(timer_id: TimerId) {
    let previous = with_core_timer_handles(|handles| handles.clear(timer_id));
    if let Some(handle) = previous {
        stop_core_timer_handle(handle, "clear");
    }
}

fn clear_all_core_timer_handles() {
    let drained = with_core_timer_handles(CoreTimerHandles::clear_all);
    for handle in drained.into_iter().flatten() {
        stop_core_timer_handle(handle, "reset");
    }
}

fn take_core_timer_handle_by_shell_timer_id(
    shell_timer_id: NvimTimerId,
) -> Option<CoreTimerHandle> {
    with_core_timer_handles(|handles| handles.take_by_shell_timer_id(shell_timer_id))
}

pub(super) fn note_autocmd_event_now() {
    event_loop::note_autocmd_event(now_ms());
}

pub(super) fn note_observation_request_now() {
    event_loop::note_observation_request(now_ms());
}

pub(super) fn clear_autocmd_event_timestamp() {
    event_loop::clear_autocmd_event_timestamp();
}

pub(super) fn clear_observation_request_timestamp() {
    event_loop::clear_observation_request_timestamp();
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

pub(super) fn record_ingress_received() {
    event_loop::record_ingress_received();
}

pub(super) fn record_ingress_coalesced() {
    event_loop::record_ingress_coalesced();
}

pub(super) fn record_ingress_dropped() {
    event_loop::record_ingress_dropped();
}

pub(super) fn record_ingress_applied() {
    event_loop::record_ingress_applied();
}

pub(super) fn record_observation_request_executed() {
    event_loop::record_observation_request_executed();
}

pub(super) fn record_degraded_draw_application() {
    event_loop::record_degraded_draw_application();
}

pub(super) fn record_stale_token_event() {
    event_loop::record_stale_token_event();
}

pub(super) fn record_timer_schedule_duration(duration_micros: u64) {
    event_loop::record_timer_schedule_duration(duration_micros);
}

pub(super) fn record_timer_fire_duration(duration_micros: u64) {
    event_loop::record_timer_fire_duration(duration_micros);
}

pub(super) fn record_scheduled_queue_depth(depth: usize) {
    event_loop::record_scheduled_queue_depth(depth);
}

pub(super) fn record_probe_duration(kind: ProbeKind, duration_micros: u64) {
    event_loop::record_probe_duration(kind, duration_micros);
}

pub(super) fn record_probe_refresh_retried(kind: ProbeKind) {
    event_loop::record_probe_refresh_retried(kind);
}

pub(super) fn record_probe_refresh_budget_exhausted(kind: ProbeKind) {
    event_loop::record_probe_refresh_budget_exhausted(kind);
}

pub(super) fn event_loop_diagnostics() -> event_loop::EventLoopDiagnostics {
    event_loop::diagnostics_snapshot()
}

pub(super) fn diagnostics_report() -> String {
    let loop_diag = event_loop_diagnostics();
    read_engine_state(|state| {
        let runtime = state.core_state.runtime();
        let core = state.core_state();
        let host_bridge_state = match state.shell.host_bridge_state() {
            HostBridgeState::Unverified => "unverified".to_string(),
            HostBridgeState::Verified { revision } => format!("verified:v{}", revision.get()),
        };

        let animation_phase = if !runtime.is_initialized() {
            "uninitialized"
        } else if runtime.is_settling() {
            "settling"
        } else if runtime.is_animating() {
            "animating"
        } else if runtime.is_draining() {
            "draining"
        } else {
            "idle"
        };

        format!(
            "smear_cursor enabled={} animation_phase={} core_lifecycle={:?} host_bridge={} ingress_received={} ingress_applied={} ingress_dropped={} ingress_coalesced={} observation_requests_executed={} degraded_draw_applications={} stale_token_events={} timer_schedule_samples={} timer_schedule_mean_ms={:.3} timer_schedule_max_ms={:.3} timer_fire_samples={} timer_fire_mean_ms={:.3} timer_fire_max_ms={:.3} scheduled_queue_depth_samples={} scheduled_queue_depth_mean={:.3} scheduled_queue_depth_max={} cursor_probe_samples={} cursor_probe_mean_ms={:.3} cursor_probe_max_ms={:.3} cursor_probe_refresh_retries={} cursor_probe_refresh_budget_exhausted={} background_probe_samples={} background_probe_mean_ms={:.3} background_probe_max_ms={:.3} background_probe_refresh_retries={} background_probe_refresh_budget_exhausted={} callback_ewma_ms={:.3} last_autocmd_ms={:.3} last_observation_request_ms={:.3}",
            runtime.is_enabled(),
            animation_phase,
            core.lifecycle(),
            host_bridge_state,
            loop_diag.metrics.ingress_received,
            loop_diag.metrics.ingress_applied,
            loop_diag.metrics.ingress_dropped,
            loop_diag.metrics.ingress_coalesced,
            loop_diag.metrics.observation_requests_executed,
            loop_diag.metrics.degraded_draw_applications,
            loop_diag.metrics.stale_token_events,
            loop_diag.metrics.timer_schedule.samples,
            loop_diag.metrics.timer_schedule.mean_ms(),
            loop_diag.metrics.timer_schedule.max_ms(),
            loop_diag.metrics.timer_fire.samples,
            loop_diag.metrics.timer_fire.mean_ms(),
            loop_diag.metrics.timer_fire.max_ms(),
            loop_diag.metrics.scheduled_queue_depth.samples,
            loop_diag.metrics.scheduled_queue_depth.mean_depth(),
            loop_diag.metrics.scheduled_queue_depth.max_depth,
            loop_diag.metrics.cursor_color_probe.duration.samples,
            loop_diag.metrics.cursor_color_probe.duration.mean_ms(),
            loop_diag.metrics.cursor_color_probe.duration.max_ms(),
            loop_diag.metrics.cursor_color_probe.refresh_retries,
            loop_diag
                .metrics
                .cursor_color_probe
                .refresh_budget_exhausted,
            loop_diag.metrics.background_probe.duration.samples,
            loop_diag.metrics.background_probe.duration.mean_ms(),
            loop_diag.metrics.background_probe.duration.max_ms(),
            loop_diag.metrics.background_probe.refresh_retries,
            loop_diag.metrics.background_probe.refresh_budget_exhausted,
            loop_diag.callback_duration_ewma_ms,
            loop_diag.last_autocmd_event_ms,
            loop_diag.last_observation_request_ms,
        )
    })
}

fn reset_transient_event_state_with_policy() {
    clear_all_core_timer_handles();
    super::handlers::reset_scheduled_effect_queue();
    mutate_engine_state(|state| {
        state.shell.reset_probe_caches();
    });
    clear_autocmd_event_timestamp();
    clear_observation_request_timestamp();
    clear_cursor_callback_duration_estimate();
    reset_core_state();
}

pub(super) fn reset_transient_event_state() {
    reset_transient_event_state_with_policy();
}

pub(super) fn reset_transient_event_state_without_generation_bump() {
    reset_transient_event_state_with_policy();
}

#[derive(Debug, Clone, Copy, Default)]
struct ShellRecoveryState {
    namespace_id: Option<u32>,
    host_bridge_state: HostBridgeState,
}

fn recover_engine_state(state: &mut EngineState) -> Option<u32> {
    let recovery_state = ShellRecoveryState {
        namespace_id: state.shell.namespace_id(),
        host_bridge_state: state.shell.host_bridge_state(),
    };
    *state = EngineState::default();
    state.shell.host_bridge_state = recovery_state.host_bridge_state;
    recovery_state.namespace_id
}

fn post_engine_state_recovery(namespace_id: Option<u32>) {
    set_log_level(RuntimeConfig::default().logging_level);
    warn("engine state panicked while borrowed; resetting runtime state");
    if let Some(namespace_id) = namespace_id {
        let _ = recover_all_namespaces(namespace_id);
    }
    reset_transient_event_state_without_generation_bump();
}

fn with_engine_state_access<R>(accessor: impl FnOnce(&mut EngineState) -> R) -> R {
    let mut state = ENGINE_CONTEXT.with(EngineContext::take_state);
    match catch_unwind(AssertUnwindSafe(|| accessor(&mut state))) {
        Ok(output) => {
            ENGINE_CONTEXT.with(|context| context.restore_state(state));
            output
        }
        Err(panic_payload) => {
            let namespace_id = recover_engine_state(&mut state);
            ENGINE_CONTEXT.with(|context| context.restore_state(state));
            post_engine_state_recovery(namespace_id);
            resume_unwind(panic_payload);
        }
    }
}

pub(super) fn read_engine_state<R>(reader: impl FnOnce(&EngineState) -> R) -> R {
    with_engine_state_access(|state| reader(state))
}

pub(super) fn mutate_engine_state<R>(mutator: impl FnOnce(&mut EngineState) -> R) -> R {
    with_engine_state_access(mutator)
}

pub(super) fn is_enabled() -> bool {
    read_engine_state(|state| state.core_state.runtime().is_enabled())
}

pub(super) fn ingress_read_snapshot() -> IngressReadSnapshot {
    IngressReadSnapshot::capture()
}

pub(super) fn core_state() -> CoreState {
    read_engine_state(EngineState::core_state)
}

pub(super) fn set_core_state(next_state: CoreState) {
    mutate_engine_state(|state| {
        state.set_core_state(next_state);
    });
}

pub(super) fn cursor_color_colorscheme_generation() -> Generation {
    read_engine_state(|state| state.shell.cursor_color_colorscheme_generation())
}

pub(super) fn cached_cursor_color_sample(
    witness: &CursorColorProbeWitness,
) -> CursorColorCacheLookup {
    read_engine_state(|state| state.shell.cached_cursor_color_sample(witness))
}

pub(super) fn store_cursor_color_sample(
    witness: CursorColorProbeWitness,
    sample: Option<CursorColorSample>,
) {
    mutate_engine_state(|state| {
        state.shell.store_cursor_color_sample(witness, sample);
    });
}

pub(super) fn note_cursor_color_colorscheme_change() {
    mutate_engine_state(|state| {
        state.shell.note_cursor_color_colorscheme_change();
    });
}

pub(crate) fn record_effect_failure(source: EffectFailureSource, context: &'static str) {
    let observed_at = to_core_millis(now_ms());
    trace_lazy(|| {
        format!(
            "effect_failure_recorded source={source:?} context={context} observed_at={}",
            observed_at.value(),
        )
    });
    super::handlers::dispatch_core_event_with_default_scheduler(Event::EffectFailed(
        EffectFailedEvent {
            proposal_id: None,
            observed_at,
        },
    ));
}

pub(super) fn dispatch_shell_timer_fired(shell_timer_id: NvimTimerId) {
    let started_at = Instant::now();
    let Some(handle) = take_core_timer_handle_by_shell_timer_id(shell_timer_id) else {
        trace_lazy(|| {
            format!(
                "timer_fire_ignored shell_timer_id={} reason=missing_handle",
                shell_timer_id.get(),
            )
        });
        return;
    };

    let observed_at = to_core_millis(now_ms());
    trace_lazy(|| {
        format!(
            "timer_fire kind={} token={} shell_timer_id={} observed_at={}",
            timer_kind_name(TimerKind::from_timer_id(handle.token.id())),
            timer_token_summary(handle.token),
            shell_timer_id.get(),
            observed_at.value(),
        )
    });

    let event = Event::TimerFiredWithToken(TimerFiredWithTokenEvent {
        token: handle.token,
        observed_at,
    });

    super::handlers::dispatch_core_event_with_default_scheduler(event);
    record_timer_fire_duration(duration_to_micros(started_at.elapsed()));
}

fn reset_core_state() {
    mutate_engine_state(|state| {
        let runtime = state.core_state.runtime().clone();
        state.set_core_state(CoreState::default().with_runtime(runtime));
    });
}

pub(super) fn to_core_millis(value_ms: f64) -> Millis {
    if !value_ms.is_finite() || value_ms <= 0.0 {
        return Millis::new(0);
    }
    let Ok(duration) = Duration::try_from_secs_f64(value_ms / 1000.0) else {
        return Millis::new(u64::MAX);
    };
    Millis::new(u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
}

fn duration_to_micros(duration: Duration) -> u64 {
    u64::try_from(duration.as_micros()).unwrap_or(u64::MAX)
}

fn schedule_core_timer_effect(
    host_bridge: InstalledHostBridge,
    token: TimerToken,
    delay_ms: u64,
    requested_at: Millis,
) -> Vec<Event> {
    let kind = TimerKind::from_timer_id(token.id());
    clear_core_timer_handle(token.id());
    let timeout = Duration::from_millis(delay_ms);
    let timer_schedule_summary = format!(
        "kind={} token={} delay_ms={} requested_at={}",
        timer_kind_name(kind),
        timer_token_summary(token),
        delay_ms,
        requested_at.value(),
    );
    let schedule_started_at = Instant::now();
    let schedule_outcome = start_timer_once(host_bridge, timeout);
    record_timer_schedule_duration(duration_to_micros(schedule_started_at.elapsed()));
    match schedule_outcome {
        Ok(shell_timer_id) => {
            trace_lazy(|| {
                format!(
                    "timer_schedule {} shell_timer_id={}",
                    timer_schedule_summary,
                    shell_timer_id.get(),
                )
            });
            set_core_timer_handle(CoreTimerHandle {
                shell_timer_id,
                token,
            });
            Vec::new()
        }
        Err(err) => {
            trace_lazy(|| format!("timer_schedule_failed {timer_schedule_summary} error={err}"));
            warn(&format!("failed to schedule core timer: {err}"));
            vec![Event::TimerLostWithToken(TimerLostWithTokenEvent {
                token,
                observed_at: requested_at,
            })]
        }
    }
}

fn resolved_timer_delay_ms(kind: TimerKind, delay: DelayBudgetMs) -> u64 {
    if kind == TimerKind::Animation && delay == DelayBudgetMs::DEFAULT_ANIMATION {
        return read_engine_state(|state| {
            let configured_interval_ms = state.core_state.runtime().config.time_interval;
            as_delay_ms(configured_interval_ms).max(1)
        });
    }
    delay.value()
}

pub(super) trait EffectExecutor {
    fn execute_effect(&mut self, effect: Effect) -> Result<Vec<Event>>;
}

#[derive(Debug, Clone, Copy)]
pub(super) struct NeovimEffectExecutor {
    host_bridge: InstalledHostBridge,
}

impl NeovimEffectExecutor {
    pub(super) fn new() -> Result<Self> {
        Ok(Self {
            host_bridge: installed_host_bridge()?,
        })
    }
}

impl EffectExecutor for NeovimEffectExecutor {
    fn execute_effect(&mut self, effect: Effect) -> Result<Vec<Event>> {
        match effect {
            Effect::ScheduleTimer(payload) => Ok(schedule_core_timer_effect(
                self.host_bridge,
                payload.token,
                resolved_timer_delay_ms(
                    TimerKind::from_timer_id(payload.token.id()),
                    payload.delay,
                ),
                payload.requested_at,
            )),
            Effect::RequestObservationBase(payload) => {
                note_observation_request_now();
                record_observation_request_executed();
                super::handlers::execute_core_request_observation_base_effect(payload)
            }
            Effect::RequestProbe(payload) => {
                let kind = payload.kind;
                let started_at = Instant::now();
                let result = super::handlers::execute_core_request_probe_effect(&payload);
                record_probe_duration(kind, duration_to_micros(started_at.elapsed()));
                Ok(result)
            }
            Effect::RequestRenderPlan(payload) => Ok(
                super::handlers::execute_core_request_render_plan_effect(payload.as_ref()),
            ),
            Effect::ApplyProposal(payload) => Ok(
                super::handlers::execute_core_apply_proposal_effect(*payload),
            ),
            Effect::ApplyRenderCleanup(payload) => Ok(
                super::handlers::execute_core_apply_render_cleanup_effect(payload),
            ),
            Effect::ApplyIngressCursorPresentation(payload) => {
                super::handlers::apply_ingress_cursor_presentation_effect(payload);
                Ok(Vec::new())
            }
            Effect::RecordEventLoopMetric(metric) => {
                match metric {
                    EventLoopMetricEffect::IngressCoalesced => record_ingress_coalesced(),
                    EventLoopMetricEffect::StaleToken => record_stale_token_event(),
                    EventLoopMetricEffect::ProbeRefreshRetried(kind) => {
                        record_probe_refresh_retried(kind);
                    }
                    EventLoopMetricEffect::ProbeRefreshBudgetExhausted(kind) => {
                        record_probe_refresh_budget_exhausted(kind);
                    }
                }
                Ok(Vec::new())
            }
            Effect::RedrawCmdline => {
                super::handlers::execute_redraw_cmdline_effect();
                Ok(Vec::new())
            }
        }
    }
}

#[cfg(test)]
mod ingress_snapshot_tests {
    use super::{IngressModePolicySnapshot, IngressReadSnapshot};
    use crate::types::Point;
    use std::sync::Arc;

    #[test]
    fn ingress_mode_policy_rejects_insert_composite_modes_without_insert_flag() {
        let policy = IngressModePolicySnapshot::from_mode_flags([false, true, false, true]);
        assert!(!policy.mode_allowed("ic"));
    }

    #[test]
    fn ingress_mode_policy_accepts_replace_visual_modes_with_replace_flag() {
        let policy = IngressModePolicySnapshot::from_mode_flags([false, true, false, true]);
        assert!(policy.mode_allowed("Rv"));
    }

    #[test]
    fn ingress_mode_policy_rejects_terminal_pending_modes_without_terminal_flag() {
        let policy = IngressModePolicySnapshot::from_mode_flags([false, true, false, true]);
        assert!(!policy.mode_allowed("ntT"));
    }

    #[test]
    fn ingress_mode_policy_accepts_cmdline_visual_modes_with_cmdline_flag() {
        let policy = IngressModePolicySnapshot::from_mode_flags([false, true, false, true]);
        assert!(policy.mode_allowed("cv"));
    }

    #[test]
    fn ingress_mode_policy_keeps_normal_mode_enabled() {
        let policy = IngressModePolicySnapshot::from_mode_flags([false, true, false, true]);
        assert!(policy.mode_allowed("n"));
    }

    #[test]
    fn ingress_snapshot_filetype_filter_matches_exact_entries() {
        let snapshot = IngressReadSnapshot::new_for_test(
            true,
            false,
            7,
            [Point { row: 1.0, col: 2.0 }; 4],
            (true, true, true, true),
            vec!["lua".to_string(), "rust".to_string()],
        );

        assert!(snapshot.has_disabled_filetypes());
        assert!(snapshot.filetype_disabled("lua"));
        assert!(snapshot.filetype_disabled("rust"));
        assert!(!snapshot.filetype_disabled("nix"));
    }

    #[test]
    fn ingress_read_snapshot_can_share_disabled_filetypes_arc() {
        let filetypes_disabled: Arc<[String]> = vec!["lua".to_string(), "rust".to_string()].into();
        let snapshot = IngressReadSnapshot {
            enabled: true,
            needs_initialize: false,
            key_delay_ms: 0,
            current_corners: [Point::ZERO; 4],
            mode_policy: IngressModePolicySnapshot::from_mode_flags([true, true, true, true]),
            filetypes_disabled: Arc::clone(&filetypes_disabled),
        };

        assert!(Arc::ptr_eq(
            &snapshot.filetypes_disabled,
            &filetypes_disabled
        ));
    }
}

pub(super) fn now_ms() -> f64 {
    let duration = match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration,
        Err(err) => err.duration(),
    };
    duration.as_secs_f64() * 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::TimerGeneration;

    fn shell_timer_id(value: i64) -> NvimTimerId {
        NvimTimerId::try_new(value).expect("test shell timer id must be positive")
    }

    fn handle(value: i64, timer_id: TimerId, generation: u64) -> CoreTimerHandle {
        CoreTimerHandle {
            shell_timer_id: shell_timer_id(value),
            token: TimerToken::new(timer_id, TimerGeneration::new(generation)),
        }
    }

    #[test]
    fn core_timer_handles_replace_is_slot_scoped() {
        let mut handles = CoreTimerHandles::default();
        let animation = handle(11, TimerId::Animation, 1);
        let ingress = handle(12, TimerId::Ingress, 2);
        let replacement = handle(13, TimerId::Animation, 3);

        assert_eq!(handles.replace(animation), None);
        assert_eq!(handles.replace(ingress), None);
        assert_eq!(handles.replace(replacement), Some(animation));
        assert_eq!(handles.animation, Some(replacement));
        assert_eq!(handles.ingress, Some(ingress));
    }

    #[test]
    fn core_timer_handles_take_by_shell_timer_id_is_slot_scoped() {
        let mut handles = CoreTimerHandles::default();
        let animation = handle(11, TimerId::Animation, 1);
        let ingress = handle(12, TimerId::Ingress, 2);

        let _ = handles.replace(animation);
        let _ = handles.replace(ingress);

        assert_eq!(
            handles.take_by_shell_timer_id(shell_timer_id(11)),
            Some(animation)
        );
        assert_eq!(handles.animation, None);
        assert_eq!(handles.ingress, Some(ingress));
        assert_eq!(handles.take_by_shell_timer_id(shell_timer_id(99)), None);
        assert_eq!(handles.ingress, Some(ingress));
    }

    #[test]
    fn core_timer_handles_clear_all_drains_every_active_timer() {
        let mut handles = CoreTimerHandles::default();
        let animation = handle(21, TimerId::Animation, 1);
        let recovery = handle(22, TimerId::Recovery, 2);

        let _ = handles.replace(animation);
        let _ = handles.replace(recovery);

        let drained = handles.clear_all();

        assert_eq!(drained, [Some(animation), None, Some(recovery), None]);
        assert_eq!(
            handles.take_by_shell_timer_id(animation.shell_timer_id),
            None
        );
        assert_eq!(
            handles.take_by_shell_timer_id(recovery.shell_timer_id),
            None
        );
    }
}
