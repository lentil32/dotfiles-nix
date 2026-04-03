use super::ENGINE_CONTEXT;
use super::EngineAccessError;
use super::EngineContext;
use super::EngineState;
use super::HostBridgeState;
use super::event_loop;
use super::host_bridge::InstalledHostBridge;
use super::host_bridge::installed_host_bridge;
use super::logging::set_log_level;
use super::logging::trace_lazy;
use super::logging::warn;
use super::policy::BufferEventPolicy;
use super::policy::current_buffer_event_policy;
use super::probe_cache::CachedCursorColorProbeSample;
use super::probe_cache::ConcealCacheKey;
use super::probe_cache::ConcealCacheLookup;
use super::probe_cache::ConcealDeltaCacheKey;
use super::probe_cache::ConcealDeltaCacheLookup;
use super::probe_cache::ConcealRegion;
use super::probe_cache::ConcealScreenCell;
use super::probe_cache::ConcealScreenCellCacheKey;
use super::probe_cache::ConcealScreenCellCacheLookup;
use super::probe_cache::CursorTextContextCacheKey;
use super::probe_cache::CursorTextContextCacheLookup;
use super::timers::NvimTimerId;
use super::timers::start_timer_once;
use super::timers::stop_timer;
use super::trace::timer_kind_name;
use super::trace::timer_token_summary;
use crate::config::RuntimeConfig;
use crate::core::effect::Effect;
use crate::core::effect::EventLoopMetricEffect;
use crate::core::effect::ProbePolicy;
use crate::core::effect::RequestProbeEffect;
use crate::core::effect::TimerKind;
use crate::core::event::EffectFailedEvent;
use crate::core::event::EffectFailureSource;
use crate::core::event::Event;
use crate::core::event::TimerFiredWithTokenEvent;
use crate::core::event::TimerLostWithTokenEvent;
use crate::core::runtime_reducer::as_delay_ms;
use crate::core::state::CoreState;
use crate::core::state::CursorColorProbeWitness;
use crate::core::state::CursorColorSample;
use crate::core::state::CursorTextContext;
use crate::core::state::ExternalDemandKind;
use crate::core::state::ObservationSnapshot;
use crate::core::state::ProbeKind;
use crate::core::state::ProbeReuse;
use crate::core::state::RenderThermalState;
use crate::core::types::DelayBudgetMs;
use crate::core::types::Generation;
use crate::core::types::Millis;
use crate::core::types::TimerToken;
use crate::draw::recover_all_namespaces;
use crate::draw::render_pool_diagnostics;
use nvim_oxi::Result;
use nvim_oxi::api;
use std::cell::RefCell;
use std::panic::AssertUnwindSafe;
use std::panic::catch_unwind;
use std::panic::resume_unwind;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

mod ingress_snapshot;

pub(crate) use ingress_snapshot::IngressReadSnapshot;

pub(super) type EngineAccessResult<T> = std::result::Result<T, EngineAccessError>;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
struct CoreTimerHandle {
    shell_timer_id: NvimTimerId,
    token: TimerToken,
}

#[derive(Default)]
struct CoreTimerHandles {
    handles: Vec<CoreTimerHandle>,
}

impl CoreTimerHandles {
    fn replace(&mut self, handle: CoreTimerHandle) -> Option<CoreTimerHandle> {
        let timer_id = handle.token.id();
        if let Some(index) = self
            .handles
            .iter()
            .position(|existing| existing.token.id() == timer_id)
        {
            let displaced = std::mem::replace(&mut self.handles[index], handle);
            Some(displaced)
        } else {
            self.handles.push(handle);
            None
        }
    }

    #[cfg(test)]
    fn has_outstanding_timer_id(&self, timer_id: crate::core::types::TimerId) -> bool {
        self.handles
            .iter()
            .any(|handle| handle.token.id() == timer_id)
    }

    fn clear_all(&mut self) -> Vec<CoreTimerHandle> {
        self.handles.drain(..).collect()
    }

    fn take_by_shell_timer_id(&mut self, shell_timer_id: NvimTimerId) -> Option<CoreTimerHandle> {
        self.handles
            .iter()
            .position(|handle| handle.shell_timer_id == shell_timer_id)
            .map(|index| self.handles.swap_remove(index))
    }
}

thread_local! {
    // Reducer token generations define which timer edge is live. The runtime keeps a single
    // outstanding shell timer per TimerId, replacing the prior host timer when a newer token is
    // scheduled for the same kind.
    static CORE_TIMER_HANDLES: RefCell<CoreTimerHandles> =
        RefCell::new(CoreTimerHandles::default());
}

fn with_core_timer_handles<R>(f: impl FnOnce(&mut CoreTimerHandles) -> R) -> R {
    CORE_TIMER_HANDLES.with(|handles| {
        let mut handles = handles.borrow_mut();
        f(&mut handles)
    })
}

fn set_core_timer_handle(handle: CoreTimerHandle) -> bool {
    let displaced = with_core_timer_handles(|handles| handles.replace(handle));
    if let Some(displaced) = displaced {
        stop_core_timer_handle(displaced, "replace");
    }
    displaced.is_some()
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

fn clear_all_core_timer_handles() {
    let drained = with_core_timer_handles(CoreTimerHandles::clear_all);
    for handle in drained {
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

fn current_buffer_handle() -> Option<i64> {
    let buffer = api::get_current_buf();
    buffer.is_valid().then(|| i64::from(buffer.handle()))
}

fn record_current_buffer_perf_telemetry(update: impl FnOnce(&mut super::ShellState, i64)) {
    let Some(buffer_handle) = current_buffer_handle() else {
        return;
    };
    let _ = mutate_engine_state(|state| update(&mut state.shell, buffer_handle));
}

pub(super) fn record_cursor_callback_duration(duration_ms: f64) {
    event_loop::record_cursor_callback_duration(duration_ms);
    record_current_buffer_perf_telemetry(|shell, buffer_handle| {
        shell.record_buffer_callback_duration(buffer_handle, duration_ms);
    });
}

pub(super) fn clear_cursor_callback_duration_estimate() {
    event_loop::clear_cursor_callback_duration_estimate();
}

pub(super) fn cursor_callback_duration_estimate_ms() -> f64 {
    let local_estimate = current_buffer_handle().and_then(|buffer_handle| {
        read_engine_state(|state| {
            state
                .shell
                .buffer_perf_telemetry(buffer_handle)
                .map(|telemetry| telemetry.callback_duration_estimate_ms())
        })
        .ok()
        .flatten()
    });
    local_estimate.unwrap_or_else(event_loop::cursor_callback_duration_estimate_ms)
}

pub(super) fn record_ingress_received() {
    event_loop::record_ingress_received();
}

pub(super) fn record_ingress_coalesced() {
    event_loop::record_ingress_coalesced();
}

pub(super) fn record_ingress_coalesced_count(count: usize) {
    event_loop::record_ingress_coalesced_count(count);
}

pub(super) fn record_delayed_ingress_pending_update() {
    event_loop::record_delayed_ingress_pending_update();
}

pub(super) fn record_delayed_ingress_pending_update_count(count: usize) {
    event_loop::record_delayed_ingress_pending_update_count(count);
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

pub(super) fn record_stale_token_event_count(count: usize) {
    event_loop::record_stale_token_event_count(count);
}

pub(super) fn record_timer_schedule_duration(duration_micros: u64) {
    event_loop::record_timer_schedule_duration(duration_micros);
}

pub(super) fn record_timer_fire_duration(duration_micros: u64) {
    event_loop::record_timer_fire_duration(duration_micros);
}

pub(super) fn record_host_timer_rearm(kind: TimerKind) {
    event_loop::record_host_timer_rearm(kind);
}

pub(super) fn record_post_burst_convergence(started_at: Millis, converged_at: Millis) {
    event_loop::record_post_burst_convergence(started_at, converged_at);
}

pub(super) fn record_scheduled_queue_depth(depth: usize) {
    event_loop::record_scheduled_queue_depth(depth);
}

pub(super) fn record_scheduled_queue_depth_for_thermal(thermal: RenderThermalState, depth: usize) {
    event_loop::record_scheduled_queue_depth_for_thermal(thermal, depth);
}

pub(super) fn record_scheduled_drain_items(drained_items: usize) {
    event_loop::record_scheduled_drain_items(drained_items);
}

pub(super) fn record_scheduled_drain_items_for_thermal(
    thermal: RenderThermalState,
    drained_items: usize,
) {
    event_loop::record_scheduled_drain_items_for_thermal(thermal, drained_items);
}

pub(super) fn record_scheduled_drain_reschedule() {
    event_loop::record_scheduled_drain_reschedule();
}

pub(super) fn record_scheduled_drain_reschedule_for_thermal(thermal: RenderThermalState) {
    event_loop::record_scheduled_drain_reschedule_for_thermal(thermal);
}

pub(super) fn record_probe_duration(kind: ProbeKind, duration_micros: u64) {
    event_loop::record_probe_duration(kind, duration_micros);
}

pub(super) fn record_probe_refresh_retried(kind: ProbeKind) {
    event_loop::record_probe_refresh_retried(kind);
}

pub(super) fn record_probe_refresh_retried_count(kind: ProbeKind, count: usize) {
    event_loop::record_probe_refresh_retried_count(kind, count);
}

pub(super) fn record_probe_refresh_budget_exhausted(kind: ProbeKind) {
    event_loop::record_probe_refresh_budget_exhausted(kind);
}

pub(super) fn record_probe_refresh_budget_exhausted_count(kind: ProbeKind, count: usize) {
    event_loop::record_probe_refresh_budget_exhausted_count(kind, count);
}

pub(super) fn record_probe_extmark_fallback(kind: ProbeKind) {
    event_loop::record_probe_extmark_fallback(kind);
    if matches!(kind, ProbeKind::CursorColor) {
        let observed_at_ms = now_ms();
        record_current_buffer_perf_telemetry(|shell, buffer_handle| {
            shell.record_buffer_cursor_color_extmark_fallback(buffer_handle, observed_at_ms);
        });
    }
}

pub(super) fn record_cursor_color_cache_hit() {
    event_loop::record_cursor_color_cache_hit();
}

pub(super) fn record_cursor_color_cache_miss() {
    event_loop::record_cursor_color_cache_miss();
}

pub(super) fn record_cursor_color_probe_reuse(reuse: ProbeReuse) {
    event_loop::record_cursor_color_reuse(reuse);
}

pub(super) fn record_conceal_region_cache_hit() {
    event_loop::record_conceal_region_cache_hit();
}

pub(super) fn record_conceal_region_cache_miss() {
    event_loop::record_conceal_region_cache_miss();
}

pub(super) fn record_conceal_screen_cell_cache_hit() {
    event_loop::record_conceal_screen_cell_cache_hit();
}

pub(super) fn record_conceal_screen_cell_cache_miss() {
    event_loop::record_conceal_screen_cell_cache_miss();
}

pub(super) fn record_conceal_full_scan() {
    event_loop::record_conceal_full_scan();
    let observed_at_ms = now_ms();
    record_current_buffer_perf_telemetry(|shell, buffer_handle| {
        shell.record_buffer_conceal_full_scan(buffer_handle, observed_at_ms);
    });
}

pub(super) fn record_conceal_raw_screenpos_fallback() {
    event_loop::record_conceal_raw_screenpos_fallback();
    let observed_at_ms = now_ms();
    record_current_buffer_perf_telemetry(|shell, buffer_handle| {
        shell.record_buffer_conceal_raw_screenpos_fallback(buffer_handle, observed_at_ms);
    });
}

pub(crate) fn record_planner_local_query(
    bucket_maps_scanned: usize,
    bucket_cells_scanned: usize,
    local_query_cells: usize,
) {
    event_loop::record_planner_local_query(
        bucket_maps_scanned,
        bucket_cells_scanned,
        local_query_cells,
    );
}

pub(crate) fn record_planner_local_query_envelope_area_cells(area_cells: u64) {
    event_loop::record_planner_local_query_envelope_area_cells(area_cells);
}

pub(crate) fn record_planner_compiled_query_cells_count(count: usize) {
    event_loop::record_planner_compiled_query_cells_count(count);
}

pub(crate) fn record_planner_candidate_query_cells_count(count: usize) {
    event_loop::record_planner_candidate_query_cells_count(count);
}

pub(crate) fn record_planner_compiled_cells_emitted_count(count: usize) {
    event_loop::record_planner_compiled_cells_emitted_count(count);
}

pub(crate) fn record_planner_reference_compile() {
    event_loop::record_planner_reference_compile();
}

pub(crate) fn record_planner_local_query_compile() {
    event_loop::record_planner_local_query_compile();
}

pub(crate) fn record_planner_candidate_cells_built_count(count: usize) {
    event_loop::record_planner_candidate_cells_built_count(count);
}

pub(super) fn event_loop_diagnostics() -> event_loop::EventLoopDiagnostics {
    event_loop::diagnostics_snapshot()
}

#[cfg(not(test))]
fn current_buffer_perf_policy() -> Result<Option<BufferEventPolicy>> {
    Ok(ingress_read_snapshot()?.current_buffer_event_policy())
}

#[cfg(test)]
fn current_buffer_perf_policy() -> Result<Option<BufferEventPolicy>> {
    Ok(None)
}

pub(super) fn diagnostics_report() -> String {
    perf_diagnostics_report()
}

fn compact_float_value(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        format!("{value:.1}")
    }
}

pub(super) fn perf_diagnostics_report() -> String {
    let loop_diag = event_loop_diagnostics();
    let buffer_perf_policy = current_buffer_perf_policy().ok().flatten();
    match read_engine_state(|state| {
        let core = state.core_state();
        let runtime = core.runtime();
        let cleanup = core.render_cleanup();
        let ingress_policy = core.ingress_policy();
        let demand_queue = core.demand_queue();
        let pool = render_pool_diagnostics();
        let _delayed_ingress_due_at = ingress_policy.pending_delay_until();
        let queue_cursor_pending = demand_queue.latest_cursor().is_some();
        let queue_ordered_backlog = demand_queue.ordered().len();
        let queue_total_backlog =
            queue_ordered_backlog.saturating_add(usize::from(queue_cursor_pending));
        let post_burst_convergence_last_ms = if loop_diag.metrics.post_burst_convergence.samples > 0
        {
            Some(loop_diag.metrics.post_burst_convergence.last_ms)
        } else {
            None
        };
        let has_retained_cursor_color = core
            .retained_observation()
            .and_then(ObservationSnapshot::cursor_color)
            .is_some();
        let probe_policy = core
            .active_observation_request()
            .map(|request| {
                ProbePolicy::for_demand(
                    request.demand().kind(),
                    request.demand().buffer_perf_class(),
                    has_retained_cursor_color,
                )
            })
            .or_else(|| {
                buffer_perf_policy.map(|policy| {
                    ProbePolicy::for_demand(
                        ExternalDemandKind::ExternalCursor,
                        policy.core_perf_class(),
                        has_retained_cursor_color,
                    )
                })
            });
        let configured_perf_mode = runtime.config.buffer_perf_mode;

        // Surprising: the Lua-visible `diagnostics()` payload truncates around 1 KiB through the
        // plugin bridge, so perf automation uses this compact reducer-owned subset instead.
        [
            "smear_cursor".to_string(),
            format!(
                "perf_class={}",
                buffer_perf_policy.map_or("na", BufferEventPolicy::diagnostic_class_name)
            ),
            format!("perf_mode={}", configured_perf_mode.option_name()),
            format!(
                "perf_effective_mode={}",
                buffer_perf_policy.map_or("na", |policy| {
                    policy.diagnostic_effective_mode_name(configured_perf_mode)
                })
            ),
            format!(
                "buffer_line_count={}",
                buffer_perf_policy.map_or(0, BufferEventPolicy::line_count)
            ),
            format!(
                "callback_ewma_ms={}",
                compact_float_value(buffer_perf_policy.map_or(
                    loop_diag.callback_duration_ewma_ms,
                    BufferEventPolicy::callback_duration_estimate_ms,
                ))
            ),
            format!(
                "probe_policy={}",
                probe_policy.map_or("na", ProbePolicy::diagnostic_name)
            ),
            format!(
                "perf_reason_bits={}",
                buffer_perf_policy.map_or(0, BufferEventPolicy::observed_reason_bits)
            ),
            format!(
                "planner_bms={}",
                loop_diag.metrics.planner.bucket_maps_scanned
            ),
            format!(
                "planner_bcs={}",
                loop_diag.metrics.planner.bucket_cells_scanned
            ),
            format!(
                "planner_lqea={}",
                loop_diag.metrics.planner.local_query_envelope_area_cells
            ),
            format!(
                "planner_local_query_cells={}",
                loop_diag.metrics.planner.local_query_cells
            ),
            format!(
                "planner_compq={}",
                loop_diag.metrics.planner.compiled_query_cells
            ),
            format!(
                "planner_candq={}",
                loop_diag.metrics.planner.candidate_query_cells
            ),
            format!(
                "planner_compiled_cells_emitted={}",
                loop_diag.metrics.planner.compiled_cells_emitted
            ),
            format!(
                "planner_candidate_cells_built={}",
                loop_diag.metrics.planner.candidate_cells_built
            ),
            // Keep these keys abbreviated so the reducer payload stays below
            // the ~1 KiB bridge budget used by the perf harness.
            format!(
                "planner_rc={}",
                loop_diag.metrics.planner.reference_compiles
            ),
            format!(
                "planner_lqc={}",
                loop_diag.metrics.planner.local_query_compiles
            ),
            format!(
                "cursor_color_extmark_fallback_calls={}",
                loop_diag.metrics.cursor_color_probe.extmark_fallback_calls
            ),
            format!(
                "cursor_color_cache_hit={}",
                loop_diag.metrics.cursor_color_cache.hits
            ),
            format!(
                "cursor_color_cache_miss={}",
                loop_diag.metrics.cursor_color_cache.misses
            ),
            format!(
                "cursor_color_reuse_exact={}",
                loop_diag.metrics.cursor_color_reuse.exact
            ),
            format!(
                "cursor_color_reuse_compatible={}",
                loop_diag.metrics.cursor_color_reuse.compatible
            ),
            format!(
                "cursor_color_reuse_refresh_required={}",
                loop_diag.metrics.cursor_color_reuse.refresh_required
            ),
            format!(
                "conceal_region_cache_hit={}",
                loop_diag.metrics.conceal_probe.region_cache.hits
            ),
            format!(
                "conceal_region_cache_miss={}",
                loop_diag.metrics.conceal_probe.region_cache.misses
            ),
            format!(
                "conceal_screen_cell_cache_hit={}",
                loop_diag.metrics.conceal_probe.screen_cell_cache.hits
            ),
            format!(
                "conceal_screen_cell_cache_miss={}",
                loop_diag.metrics.conceal_probe.screen_cell_cache.misses
            ),
            format!(
                "conceal_full_scan_calls={}",
                loop_diag.metrics.conceal_probe.full_scan_calls
            ),
            format!(
                "conceal_raw_screenpos_fallback_calls={}",
                loop_diag.metrics.conceal_probe.raw_screenpos_fallback_calls
            ),
            format!(
                "perf_reasons={}",
                buffer_perf_policy.map_or_else(
                    || "na".to_string(),
                    BufferEventPolicy::diagnostic_observed_reason_summary,
                )
            ),
            format!(
                "cleanup_thermal={}",
                cleanup_thermal_name(cleanup.thermal())
            ),
            format!("pool_total_windows={}", pool.total_windows),
            format!("pool_cached_budget={}", pool.cached_budget),
            format!("pool_peak_requested={}", pool.peak_requested_capacity),
            format!("pool_cap_hits={}", pool.capacity_cap_hits),
            format!("max_kept_windows={}", runtime.config.max_kept_windows),
            format!(
                "delayed_ingress_pending_updates={}",
                loop_diag.metrics.delayed_ingress_pending_updates
            ),
            format!("queue_total_backlog={queue_total_backlog}"),
            format!(
                "post_burst_convergence_last_ms={}",
                optional_u64_value(post_burst_convergence_last_ms)
            ),
            format!(
                "host_timer_rearms_ingress={}",
                loop_diag.metrics.host_timer_rearms_by_kind.ingress
            ),
            format!(
                "scheduled_drain_reschedules_cooling={}",
                loop_diag
                    .metrics
                    .scheduled_drain_reschedules_by_thermal
                    .cooling
            ),
        ]
        .join(" ")
    }) {
        Ok(report) => report,
        Err(err) => format!("smear_cursor error={err}"),
    }
}

fn cleanup_thermal_name(thermal: RenderThermalState) -> &'static str {
    match thermal {
        RenderThermalState::Hot => "hot",
        RenderThermalState::Cooling => "cooling",
        RenderThermalState::Cold => "cold",
    }
}

fn optional_u64_value(value: Option<u64>) -> String {
    value.map_or_else(|| "none".to_string(), |value| value.to_string())
}

fn reset_transient_event_state_with_policy() {
    clear_all_core_timer_handles();
    super::handlers::reset_scheduled_effect_queue();
    if let Err(err) = mutate_engine_state(|state| {
        state.shell.reset_probe_caches();
        state.shell.reset_buffer_event_policies();
    }) {
        warn(&format!(
            "engine state re-entered during transient reset; skipping shell cache reset: {err}"
        ));
    }
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

fn with_engine_state_access<R>(
    accessor: impl FnOnce(&mut EngineState) -> R,
) -> EngineAccessResult<R> {
    let mut state = ENGINE_CONTEXT.with(EngineContext::take_state)?;
    match catch_unwind(AssertUnwindSafe(|| accessor(&mut state))) {
        Ok(output) => {
            ENGINE_CONTEXT.with(|context| context.restore_state(state));
            Ok(output)
        }
        Err(panic_payload) => {
            let namespace_id = recover_engine_state(&mut state);
            ENGINE_CONTEXT.with(|context| context.restore_state(state));
            post_engine_state_recovery(namespace_id);
            resume_unwind(panic_payload);
        }
    }
}

pub(super) fn read_engine_state<R>(
    reader: impl FnOnce(&EngineState) -> R,
) -> EngineAccessResult<R> {
    with_engine_state_access(|state| reader(state))
}

pub(super) fn mutate_engine_state<R>(
    mutator: impl FnOnce(&mut EngineState) -> R,
) -> EngineAccessResult<R> {
    with_engine_state_access(mutator)
}

pub(super) fn ingress_read_snapshot() -> EngineAccessResult<IngressReadSnapshot> {
    IngressReadSnapshot::capture()
}

pub(super) fn resolved_current_buffer_event_policy(
    snapshot: &IngressReadSnapshot,
    buffer: &api::Buffer,
) -> Result<BufferEventPolicy> {
    let buffer_handle = i64::from(buffer.handle());
    let observed_at_ms = now_ms();
    let (previous, telemetry) = read_engine_state(|state| {
        (
            state.shell.cached_buffer_event_policy(buffer_handle),
            state
                .shell
                .buffer_perf_telemetry(buffer_handle)
                .unwrap_or_default(),
        )
    })
    .map_err(nvim_oxi::Error::from)?;
    let policy =
        current_buffer_event_policy(snapshot, buffer, previous, telemetry, observed_at_ms)?;
    mutate_engine_state(|state| {
        state.shell.store_buffer_event_policy(buffer_handle, policy);
    })
    .map_err(nvim_oxi::Error::from)?;
    Ok(policy)
}

#[cfg(test)]
pub(super) fn core_state() -> EngineAccessResult<CoreState> {
    read_engine_state(EngineState::clone_core_state)
}

#[cfg(test)]
pub(super) fn set_core_state(next_state: CoreState) -> EngineAccessResult<()> {
    mutate_engine_state(|state| {
        state.set_core_state(next_state);
    })
}

pub(super) fn cursor_color_colorscheme_generation() -> EngineAccessResult<Generation> {
    read_engine_state(|state| state.shell.cursor_color_colorscheme_generation())
}

pub(super) fn cached_cursor_color_sample_for_probe(
    witness: &CursorColorProbeWitness,
    probe_policy: ProbePolicy,
    reuse: ProbeReuse,
) -> EngineAccessResult<Option<CachedCursorColorProbeSample>> {
    mutate_engine_state(|state| {
        state
            .shell
            .cached_cursor_color_sample_for_probe(witness, probe_policy, reuse)
    })
}

pub(super) fn store_cursor_color_sample(
    witness: CursorColorProbeWitness,
    sample: Option<CursorColorSample>,
) -> EngineAccessResult<()> {
    mutate_engine_state(|state| {
        state.shell.store_cursor_color_sample(witness, sample);
    })
}

pub(super) fn cached_cursor_text_context(
    key: &CursorTextContextCacheKey,
) -> EngineAccessResult<CursorTextContextCacheLookup> {
    mutate_engine_state(|state| state.shell.cached_cursor_text_context(key))
}

pub(super) fn store_cursor_text_context(
    key: CursorTextContextCacheKey,
    context: Option<CursorTextContext>,
) -> EngineAccessResult<()> {
    mutate_engine_state(|state| {
        state.shell.store_cursor_text_context(key, context);
    })
}

pub(super) fn cached_conceal_regions(
    key: &ConcealCacheKey,
) -> EngineAccessResult<ConcealCacheLookup> {
    mutate_engine_state(|state| state.shell.cached_conceal_regions(key))
}

pub(super) fn cached_conceal_delta(
    key: &ConcealDeltaCacheKey,
) -> EngineAccessResult<ConcealDeltaCacheLookup> {
    mutate_engine_state(|state| state.shell.cached_conceal_delta(key))
}

pub(super) fn store_conceal_regions(
    key: ConcealCacheKey,
    scanned_to_col1: i64,
    regions: Arc<[ConcealRegion]>,
) -> EngineAccessResult<()> {
    mutate_engine_state(|state| {
        state
            .shell
            .store_conceal_regions(key, scanned_to_col1, regions);
    })
}

pub(super) fn store_conceal_delta(
    key: ConcealDeltaCacheKey,
    current_col1: i64,
    delta: i64,
) -> EngineAccessResult<()> {
    mutate_engine_state(|state| {
        state.shell.store_conceal_delta(key, current_col1, delta);
    })
}

pub(super) fn cached_conceal_screen_cell(
    key: &ConcealScreenCellCacheKey,
) -> EngineAccessResult<ConcealScreenCellCacheLookup> {
    mutate_engine_state(|state| state.shell.cached_conceal_screen_cell(key))
}

pub(super) fn store_conceal_screen_cell(
    key: ConcealScreenCellCacheKey,
    cell: Option<ConcealScreenCell>,
) -> EngineAccessResult<()> {
    mutate_engine_state(|state| {
        state.shell.store_conceal_screen_cell(key, cell);
    })
}

pub(super) fn note_cursor_color_colorscheme_change() -> EngineAccessResult<()> {
    mutate_engine_state(|state| {
        state.shell.note_cursor_color_colorscheme_change();
    })
}

pub(crate) fn record_effect_failure(source: EffectFailureSource, context: &'static str) {
    let observed_at = to_core_millis(now_ms());
    trace_lazy(|| {
        format!(
            "effect_failure_recorded source={source:?} context={context} observed_at={}",
            observed_at.value(),
        )
    });
    super::handlers::stage_core_event_with_default_scheduler(Event::EffectFailed(
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

    if let Err(err) = super::handlers::dispatch_core_event_with_default_scheduler(event.clone()) {
        warn(&format!(
            "engine state re-entered while dispatching timer event; re-staging for recovery: {err}"
        ));
        super::handlers::stage_core_event_with_default_scheduler(event);
    }
    record_timer_fire_duration(duration_to_micros(started_at.elapsed()));
}

fn reset_core_state() {
    if let Err(err) = mutate_engine_state(|state| {
        let runtime = state.core_state_mut().take_runtime();
        state.set_core_state(CoreState::default().with_runtime(runtime));
    }) {
        warn(&format!(
            "engine state re-entered during core reset; keeping existing state: {err}"
        ));
    }
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
            let rearmed = set_core_timer_handle(CoreTimerHandle {
                shell_timer_id,
                token,
            });
            if rearmed {
                record_host_timer_rearm(kind);
            }
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
        return match read_engine_state(|state| {
            let configured_interval_ms = state.core_state.runtime().config.time_interval;
            as_delay_ms(configured_interval_ms).max(1)
        }) {
            Ok(delay_ms) => delay_ms,
            Err(err) => {
                warn(&format!(
                    "engine state re-entered while resolving animation delay; using default timer budget: {err}"
                ));
                delay.value()
            }
        };
    }
    delay.value()
}

pub(super) trait EffectExecutor {
    fn execute_effect(&mut self, effect: Effect) -> Result<Vec<Event>>;

    fn execute_probe_effect(
        &mut self,
        payload: RequestProbeEffect,
        _same_reducer_wave: bool,
    ) -> Result<Vec<Event>> {
        self.execute_effect(Effect::RequestProbe(payload))
    }
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
    fn execute_probe_effect(
        &mut self,
        payload: RequestProbeEffect,
        same_reducer_wave: bool,
    ) -> Result<Vec<Event>> {
        let kind = payload.kind;
        let started_at = Instant::now();
        let result = if same_reducer_wave {
            super::handlers::execute_core_request_probe_effect_same_reducer_wave(&payload)
        } else {
            super::handlers::execute_core_request_probe_effect(&payload)
        };
        record_probe_duration(kind, duration_to_micros(started_at.elapsed()));
        Ok(result)
    }

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
            Effect::RequestProbe(payload) => self.execute_probe_effect(payload, false),
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
                    EventLoopMetricEffect::DelayedIngressPendingUpdated => {
                        record_delayed_ingress_pending_update();
                    }
                    EventLoopMetricEffect::CleanupConvergedToCold {
                        started_at,
                        converged_at,
                    } => {
                        record_post_burst_convergence(started_at, converged_at);
                    }
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
    use crate::core::types::TimerId;
    use insta::assert_snapshot;

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
    fn core_timer_handles_replace_keeps_one_live_timer_per_id() {
        let mut handles = CoreTimerHandles::default();
        let animation = handle(11, TimerId::Animation, 1);
        let ingress = handle(12, TimerId::Ingress, 2);
        let rearmed_animation = handle(13, TimerId::Animation, 3);

        assert_eq!(handles.replace(animation), None);
        assert_eq!(handles.replace(ingress), None);
        assert_eq!(handles.replace(rearmed_animation), Some(animation));

        assert_eq!(
            handles.take_by_shell_timer_id(animation.shell_timer_id),
            None
        );
        assert_eq!(
            handles.take_by_shell_timer_id(ingress.shell_timer_id),
            Some(ingress)
        );
        assert_eq!(
            handles.take_by_shell_timer_id(rearmed_animation.shell_timer_id),
            Some(rearmed_animation)
        );
    }

    #[test]
    fn core_timer_handles_take_by_shell_timer_id_is_exact() {
        let mut handles = CoreTimerHandles::default();
        let animation = handle(11, TimerId::Animation, 1);
        let ingress = handle(12, TimerId::Ingress, 2);

        assert_eq!(handles.replace(animation), None);
        assert_eq!(handles.replace(ingress), None);

        assert_eq!(
            handles.take_by_shell_timer_id(shell_timer_id(11)),
            Some(animation)
        );
        assert_eq!(handles.take_by_shell_timer_id(shell_timer_id(99)), None);
        assert_eq!(
            handles.take_by_shell_timer_id(shell_timer_id(12)),
            Some(ingress)
        );
    }

    #[test]
    fn core_timer_handles_clear_all_drains_every_active_timer() {
        let mut handles = CoreTimerHandles::default();
        let animation = handle(21, TimerId::Animation, 1);
        let recovery = handle(22, TimerId::Recovery, 2);

        assert_eq!(handles.replace(animation), None);
        assert_eq!(handles.replace(recovery), None);

        let drained = handles.clear_all();

        assert_eq!(drained, vec![animation, recovery]);
        assert_eq!(
            handles.take_by_shell_timer_id(animation.shell_timer_id),
            None
        );
        assert_eq!(
            handles.take_by_shell_timer_id(recovery.shell_timer_id),
            None
        );
    }

    #[test]
    fn core_timer_handles_detect_outstanding_kind_for_rearm_tracking() {
        let mut handles = CoreTimerHandles::default();
        assert_eq!(handles.replace(handle(41, TimerId::Ingress, 1)), None);

        assert!(handles.has_outstanding_timer_id(TimerId::Ingress));
        assert!(!handles.has_outstanding_timer_id(TimerId::Cleanup));
    }

    #[test]
    fn core_timer_handles_replace_updates_the_live_shell_timer_for_the_same_id() {
        let mut handles = CoreTimerHandles::default();
        let first = handle(51, TimerId::Cleanup, 1);
        let second = handle(52, TimerId::Cleanup, 2);

        assert_eq!(handles.replace(first), None);
        assert_eq!(handles.replace(second), Some(first));

        assert_eq!(handles.take_by_shell_timer_id(first.shell_timer_id), None);
        assert_eq!(
            handles.take_by_shell_timer_id(second.shell_timer_id),
            Some(second)
        );
    }

    #[test]
    fn nested_engine_state_access_returns_reentry_error_and_preserves_state() {
        let nested = mutate_engine_state(|state| {
            state.shell.set_namespace_id(77);
            read_engine_state(|inner| inner.shell.namespace_id())
        });

        assert_eq!(nested, Ok(Err(EngineAccessError::Reentered)));
        assert_eq!(
            read_engine_state(|state| state.shell.namespace_id()),
            Ok(Some(77))
        );
    }

    #[test]
    fn perf_diagnostics_report_includes_recovery_fields_within_bridge_budget() {
        let report = perf_diagnostics_report();

        assert!(report.starts_with("smear_cursor "));
        assert!(
            report.len() < 1024,
            "perf diagnostics report exceeded bridge budget: {} bytes",
            report.len()
        );
        assert!(report.contains("perf_class="));
        assert!(report.contains("perf_mode="));
        assert!(report.contains("perf_effective_mode="));
        assert!(report.contains("buffer_line_count="));
        assert!(report.contains("callback_ewma_ms="));
        assert!(report.contains("probe_policy="));
        assert!(report.contains("perf_reason_bits="));
        assert!(report.contains("planner_bms="));
        assert!(report.contains("planner_bcs="));
        assert!(report.contains("planner_lqea="));
        assert!(report.contains("planner_local_query_cells="));
        assert!(report.contains("planner_compq="));
        assert!(report.contains("planner_candq="));
        assert!(report.contains("planner_compiled_cells_emitted="));
        assert!(report.contains("planner_candidate_cells_built="));
        assert!(report.contains("planner_rc="));
        assert!(report.contains("planner_lqc="));
        assert!(report.contains("cursor_color_extmark_fallback_calls="));
        assert!(report.contains("cursor_color_cache_hit="));
        assert!(report.contains("cursor_color_cache_miss="));
        assert!(report.contains("cursor_color_reuse_exact="));
        assert!(report.contains("cursor_color_reuse_compatible="));
        assert!(report.contains("cursor_color_reuse_refresh_required="));
        assert!(report.contains("conceal_region_cache_hit="));
        assert!(report.contains("conceal_region_cache_miss="));
        assert!(report.contains("conceal_screen_cell_cache_hit="));
        assert!(report.contains("conceal_screen_cell_cache_miss="));
        assert!(report.contains("conceal_full_scan_calls="));
        assert!(report.contains("conceal_raw_screenpos_fallback_calls="));
        assert!(report.contains("perf_reasons="));
        assert!(report.contains("cleanup_thermal="));
        assert!(report.contains("pool_total_windows="));
        assert!(report.contains("pool_cached_budget="));
        assert!(report.contains("pool_peak_requested="));
        assert!(report.contains("pool_cap_hits="));
        assert!(report.contains("max_kept_windows="));
        assert!(report.contains("queue_total_backlog="));
        assert!(report.contains("delayed_ingress_pending_updates="));
        assert!(report.contains("post_burst_convergence_last_ms="));
        assert!(report.contains("host_timer_rearms_ingress="));
        assert!(report.contains("scheduled_drain_reschedules_cooling="));
        assert!(report.len() < 1000);
    }

    #[test]
    fn perf_diagnostics_report_snapshot_renders_stable_field_order() {
        assert_snapshot!(perf_diagnostics_report());
    }
}
