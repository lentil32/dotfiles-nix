use super::cursor::{
    cursor_position_for_mode, line_value, mode_string, smear_outside_cmd_row,
    update_tracked_cursor_color,
};
use super::geometry::{current_cursor_snapshot, maybe_scroll_shift};
use super::logging::{debug, hide_real_cursor, log_slow_callback, unhide_real_cursor, warn};
use super::policy::{
    BufferEventPolicy, current_buffer_event_policy, remaining_throttle_delay_ms,
    skip_current_buffer_events,
};
use super::runtime::{
    bump_render_generation, clear_animation_timer, clear_cmdline_redraw_pending,
    clear_external_event_timer, clear_key_event_timer, complete_external_trigger_dispatch,
    current_render_generation, cursor_callback_duration_estimate_ms,
    elapsed_ms_since_last_autocmd_event, elapsed_ms_since_last_external_dispatch, is_enabled,
    mark_cmdline_redraw_pending_if_idle, mark_external_trigger_pending_if_idle,
    note_autocmd_event_now, note_external_dispatch_now, now_ms, record_cursor_callback_duration,
    seed_from_clock, state_lock,
};
use super::timers::{
    apply_render_cleanup_action, ensure_namespace_id, schedule_animation_tick,
    schedule_external_event_timer, schedule_external_throttle_timer, schedule_key_event_timer,
};
use crate::draw::{
    AllocationPolicy, clear_active_render_windows, clear_highlight_cache,
    clear_prepaint_for_current_tab, draw_current, draw_target_hack_block,
    notify_delay_disabled_warning, prepaint_cursor_block, redraw,
};
use crate::reducer::{
    AnimationScheduleAction, AnimationScheduleInput, CursorEventContext, CursorVisibilityEffect,
    EventSource, RenderAction, RenderAllocationPolicy, as_delay_ms, decide_animation_schedule,
    external_settle_delay_ms, reduce_cursor_event,
};
use crate::state::{CursorLocation, CursorSnapshot};
use crate::types::ScreenCell;
use nvim_oxi::api::types::AutocmdCallbackArgs;
use nvim_oxi::{Result, api, schedule};
use nvim_utils::mode::is_cmdline_mode;

// Event handlers are orchestration-only: pure decision helpers compute explicit actions,
// then small apply functions execute runtime side effects.

#[derive(Clone, Copy, Debug, Default)]
struct RenderExecutionMetrics {
    ops_planned: usize,
    ops_applied: usize,
    ops_skipped_capacity: usize,
    windows_created: usize,
    windows_reused: usize,
    reuse_failed_missing_window: usize,
    reuse_failed_reconfigure: usize,
    reuse_failed_missing_buffer: usize,
    windows_pruned: usize,
    windows_hidden: usize,
    windows_invalid_removed: usize,
    windows_recovered: usize,
    pool_total_windows: usize,
    pool_available_windows: usize,
    pool_in_use_windows: usize,
    pool_cached_budget: usize,
    pool_last_frame_demand: usize,
}

impl RenderExecutionMetrics {
    fn merge_apply_metrics(
        &mut self,
        planned_ops: usize,
        applied_ops: usize,
        skipped_ops_capacity: usize,
        created_windows: usize,
        reused_windows: usize,
        reuse_failed_missing_window: usize,
        reuse_failed_reconfigure: usize,
        reuse_failed_missing_buffer: usize,
        pruned_windows: usize,
        recovered_windows: usize,
        pool_snapshot: Option<crate::draw::TabPoolSnapshot>,
    ) {
        self.ops_planned = self.ops_planned.saturating_add(planned_ops);
        self.ops_applied = self.ops_applied.saturating_add(applied_ops);
        self.ops_skipped_capacity = self
            .ops_skipped_capacity
            .saturating_add(skipped_ops_capacity);
        self.windows_created = self.windows_created.saturating_add(created_windows);
        self.windows_reused = self.windows_reused.saturating_add(reused_windows);
        self.reuse_failed_missing_window = self
            .reuse_failed_missing_window
            .saturating_add(reuse_failed_missing_window);
        self.reuse_failed_reconfigure = self
            .reuse_failed_reconfigure
            .saturating_add(reuse_failed_reconfigure);
        self.reuse_failed_missing_buffer = self
            .reuse_failed_missing_buffer
            .saturating_add(reuse_failed_missing_buffer);
        self.windows_pruned = self.windows_pruned.saturating_add(pruned_windows);
        self.windows_recovered = self.windows_recovered.saturating_add(recovered_windows);
        if let Some(snapshot) = pool_snapshot {
            self.pool_total_windows = snapshot.total_windows;
            self.pool_available_windows = snapshot.available_windows;
            self.pool_in_use_windows = snapshot.in_use_windows;
            self.pool_cached_budget = snapshot.cached_budget;
            self.pool_last_frame_demand = snapshot.last_frame_demand;
        }
    }

    fn to_perf_details(self) -> String {
        format!(
            "ops_planned={} ops_applied={} ops_skipped_capacity={} windows_created={} windows_reused={} reuse_failed_missing_window={} reuse_failed_reconfigure={} reuse_failed_missing_buffer={} windows_pruned={} windows_hidden={} windows_invalid_removed={} windows_recovered={} pool_total_windows={} pool_available_windows={} pool_in_use_windows={} pool_cached_budget={} pool_last_frame_demand={}",
            self.ops_planned,
            self.ops_applied,
            self.ops_skipped_capacity,
            self.windows_created,
            self.windows_reused,
            self.reuse_failed_missing_window,
            self.reuse_failed_reconfigure,
            self.reuse_failed_missing_buffer,
            self.windows_pruned,
            self.windows_hidden,
            self.windows_invalid_removed,
            self.windows_recovered,
            self.pool_total_windows,
            self.pool_available_windows,
            self.pool_in_use_windows,
            self.pool_cached_budget,
            self.pool_last_frame_demand
        )
    }
}

fn to_draw_allocation_policy(effect: RenderAllocationPolicy) -> AllocationPolicy {
    match effect {
        RenderAllocationPolicy::ReuseOnly => AllocationPolicy::ReuseOnly,
        RenderAllocationPolicy::BootstrapIfPoolEmpty => AllocationPolicy::BootstrapIfPoolEmpty,
    }
}

fn render_action_perf_details(
    render_action: &RenderAction,
    render_allocation_policy: RenderAllocationPolicy,
) -> String {
    match render_action {
        RenderAction::Draw(frame) => {
            let (min_row, max_row) = frame
                .corners
                .iter()
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(min, max), point| {
                    (min.min(point.row), max.max(point.row))
                });
            let (min_col, max_col) = frame
                .corners
                .iter()
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(min, max), point| {
                    (min.min(point.col), max.max(point.col))
                });
            let height_cells = (max_row.ceil() - min_row.floor()).max(0.0) as i64;
            let width_cells = (max_col.ceil() - min_col.floor()).max(0.0) as i64;
            let area_cells = height_cells.saturating_mul(width_cells);
            format!(
                "action=draw area_cells={area_cells} height_cells={height_cells} width_cells={width_cells} particles={} vertical_bar={} max_kept_windows={} allocation={render_allocation_policy:?}",
                frame.particles.len(),
                frame.vertical_bar,
                frame.max_kept_windows
            )
        }
        RenderAction::ClearAll => "action=clear_all".to_string(),
        RenderAction::Noop => "action=noop".to_string(),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum ExternalTriggerAction {
    ClearPending {
        clear_timer: bool,
    },
    DispatchNow,
    ScheduleSettle {
        delay_ms: u64,
        snapshot: CursorSnapshot,
    },
    ScheduleThrottle {
        delay_ms: u64,
    },
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(super) enum KeyEventAction {
    None,
    TriggerExternal,
    ScheduleKeyTimer { delay_ms: u64 },
}

pub(super) fn decide_external_trigger_action(
    policy: BufferEventPolicy,
    mode: &str,
    smear_to_cmd: bool,
    settle_delay_ms: u64,
    snapshot: Option<CursorSnapshot>,
    elapsed_since_external_dispatch_ms: f64,
) -> ExternalTriggerAction {
    if !policy.should_use_debounced_external_settle() {
        let throttle_interval_ms = policy.settle_delay_floor_ms();
        let remaining_delay_ms =
            remaining_throttle_delay_ms(throttle_interval_ms, elapsed_since_external_dispatch_ms);
        if remaining_delay_ms == 0 {
            return ExternalTriggerAction::DispatchNow;
        }
        return ExternalTriggerAction::ScheduleThrottle {
            delay_ms: remaining_delay_ms,
        };
    }

    if is_cmdline_mode(mode) && !smear_to_cmd {
        return ExternalTriggerAction::ClearPending { clear_timer: true };
    }

    match snapshot {
        Some(snapshot) => ExternalTriggerAction::ScheduleSettle {
            delay_ms: settle_delay_ms,
            snapshot,
        },
        None => ExternalTriggerAction::ClearPending { clear_timer: false },
    }
}

fn apply_external_trigger_action(action: ExternalTriggerAction) -> Result<()> {
    match action {
        ExternalTriggerAction::ClearPending { clear_timer } => {
            if clear_timer {
                clear_external_event_timer();
            }
            let mut state = state_lock();
            state.clear_pending_external_event();
            Ok(())
        }
        ExternalTriggerAction::DispatchNow => {
            on_cursor_event_impl(EventSource::External)?;
            note_external_dispatch_now();
            Ok(())
        }
        ExternalTriggerAction::ScheduleSettle { delay_ms, snapshot } => {
            let mut state = state_lock();
            state.set_pending_external_event(Some(snapshot));
            drop(state);
            schedule_external_event_timer(delay_ms);
            Ok(())
        }
        ExternalTriggerAction::ScheduleThrottle { delay_ms } => {
            schedule_external_throttle_timer(delay_ms);
            Ok(())
        }
    }
}

fn apply_animation_schedule(action: AnimationScheduleAction) {
    match action {
        AnimationScheduleAction::None => {}
        AnimationScheduleAction::Clear => clear_animation_timer(),
        AnimationScheduleAction::Schedule {
            delay_ms,
            generation,
        } => schedule_animation_tick(delay_ms, generation),
    }
}

pub(super) fn decide_key_event_action(
    policy: BufferEventPolicy,
    key_delay_ms: u64,
    delay_after_key_ms: f64,
    elapsed_since_last_autocmd_ms: f64,
) -> KeyEventAction {
    if !policy.use_key_fallback() || elapsed_since_last_autocmd_ms <= delay_after_key_ms {
        return KeyEventAction::None;
    }
    if !policy.should_use_debounced_external_settle() {
        return KeyEventAction::TriggerExternal;
    }
    KeyEventAction::ScheduleKeyTimer {
        delay_ms: key_delay_ms,
    }
}

pub(super) fn should_bump_render_generation(source: EventSource, is_animating: bool) -> bool {
    matches!(source, EventSource::External) && !is_animating
}

pub(super) fn on_cursor_event_impl(source: EventSource) -> Result<()> {
    let render_generation = match source {
        EventSource::AnimationTick => current_render_generation(),
        EventSource::External => {
            let is_animating = {
                let state = state_lock();
                state.is_animating()
            };
            if should_bump_render_generation(source, is_animating) {
                bump_render_generation()
            } else {
                current_render_generation()
            }
        }
    };

    let mode = mode_string();
    let namespace_id = ensure_namespace_id();
    let smear_to_cmd = {
        let state = state_lock();
        state.config.smear_to_cmd
    };

    let window = api::get_current_win();
    if !window.is_valid() {
        return Ok(());
    }

    let buffer = api::get_current_buf();
    if !buffer.is_valid() {
        return Ok(());
    }
    let buffer_event_policy = match source {
        EventSource::AnimationTick => BufferEventPolicy::Normal,
        EventSource::External => current_buffer_event_policy(&buffer)?,
    };
    let animation_delay_floor_ms = buffer_event_policy.animation_delay_floor_ms();

    let fallback_target_position = match source {
        EventSource::AnimationTick => {
            let state = state_lock();
            if state.is_initialized() {
                let target = state.target_position();
                Some((target.row, target.col))
            } else {
                None
            }
        }
        EventSource::External => None,
    };

    let maybe_cursor_position = if let Some(position) = fallback_target_position {
        Some(position)
    } else {
        cursor_position_for_mode(&window, &mode, smear_to_cmd)?
    };

    let Some((cursor_row, cursor_col)) = maybe_cursor_position else {
        // Ignore transient cursor-position misses instead of clearing active smear state.
        return Ok(());
    };

    let current_win_handle = i64::from(window.handle());
    let current_buf_handle = i64::from(buffer.handle());
    let (scroll_buffer_space, previous_location, current_corners) = {
        let state = state_lock();
        (
            state.config.scroll_buffer_space,
            state.tracked_location(),
            state.current_corners(),
        )
    };
    let (current_top_row, current_line) = match source {
        EventSource::AnimationTick => previous_location
            .filter(|tracked| {
                tracked.window_handle == current_win_handle
                    && tracked.buffer_handle == current_buf_handle
            })
            .map(|tracked| (tracked.top_row, tracked.line))
            .unwrap_or((line_value("w0")?, line_value(".")?)),
        EventSource::External => (line_value("w0")?, line_value(".")?),
    };
    let current_location = CursorLocation::new(
        current_win_handle,
        current_buf_handle,
        current_top_row,
        current_line,
    );
    let scroll_shift = match source {
        EventSource::External => maybe_scroll_shift(
            &window,
            scroll_buffer_space,
            &current_corners,
            previous_location,
            current_location,
        )?,
        EventSource::AnimationTick => None,
    };

    let event_now_ms = now_ms();
    let event = CursorEventContext {
        row: cursor_row,
        col: cursor_col,
        now_ms: event_now_ms,
        seed: seed_from_clock(),
        cursor_location: current_location,
        scroll_shift,
    };

    let transition = {
        let mut state = state_lock();
        state.set_namespace_id(namespace_id);
        reduce_cursor_event(&mut state, &mode, event, source)
    };
    let crate::reducer::CursorTransition {
        render_decision,
        notify_delay_disabled,
        command,
    } = transition;
    let perf_render_details = render_action_perf_details(
        &render_decision.render_action,
        render_decision.render_allocation_policy,
    );
    let perf_cleanup_details = format!("cleanup={:?}", render_decision.render_cleanup_action);

    if notify_delay_disabled && let Err(err) = notify_delay_disabled_warning() {
        warn(&format!("delay-disabled notify failed: {err}"));
    }

    let execution_metrics = apply_render_action(
        namespace_id,
        render_decision.render_action,
        render_decision.render_allocation_policy,
        render_decision.render_side_effects,
    )?;

    apply_render_cleanup_action(namespace_id, render_decision.render_cleanup_action);

    let callback_duration_ms = (now_ms() - event_now_ms).max(0.0);
    record_cursor_callback_duration(callback_duration_ms);
    let callback_duration_estimate_ms = cursor_callback_duration_estimate_ms();
    let source_name = match source {
        EventSource::AnimationTick => "animation_tick",
        EventSource::External => "external",
    };
    log_slow_callback(
        source_name,
        &mode,
        callback_duration_ms,
        callback_duration_estimate_ms,
        &format!(
            "{perf_render_details} {perf_cleanup_details} {}",
            execution_metrics.to_perf_details()
        ),
    );
    let animation_schedule_action = {
        let mut state = state_lock();
        decide_animation_schedule(
            &mut state,
            AnimationScheduleInput {
                source,
                command,
                callback_duration_ms,
                callback_duration_estimate_ms,
                animation_delay_floor_ms,
                render_generation,
                in_cmdline_mode: is_cmdline_mode(&mode),
            },
        )
    };
    apply_animation_schedule(animation_schedule_action);

    Ok(())
}

pub(super) fn on_external_event_trigger() -> Result<()> {
    if !is_enabled() {
        let mut state = state_lock();
        state.clear_pending_external_event();
        return Ok(());
    }

    let buffer = api::get_current_buf();
    if !buffer.is_valid() {
        return Ok(());
    }
    let policy = current_buffer_event_policy(&buffer)?;
    if skip_current_buffer_events(&buffer)? {
        let mut state = state_lock();
        state.clear_pending_external_event();
        return Ok(());
    }
    let mode = mode_string();
    let (smear_to_cmd, settle_delay_ms) = {
        let state = state_lock();
        (
            state.config.smear_to_cmd,
            external_settle_delay_ms(state.config.delay_event_to_smear),
        )
    };
    let snapshot = if policy.should_use_debounced_external_settle()
        && (!is_cmdline_mode(&mode) || smear_to_cmd)
    {
        current_cursor_snapshot(smear_to_cmd)?
    } else {
        None
    };
    let elapsed_since_external_dispatch_ms = elapsed_ms_since_last_external_dispatch(now_ms());
    let action = decide_external_trigger_action(
        policy,
        &mode,
        smear_to_cmd,
        settle_delay_ms,
        snapshot,
        elapsed_since_external_dispatch_ms,
    );
    apply_external_trigger_action(action)
}

fn schedule_external_event_trigger_dispatch() {
    schedule(|_| {
        if let Err(err) = on_external_event_trigger() {
            warn(&format!("external event trigger failed: {err}"));
        }
        if complete_external_trigger_dispatch() {
            schedule_external_event_trigger_dispatch();
        }
    });
}

fn schedule_cmdline_redraw() {
    if !mark_cmdline_redraw_pending_if_idle() {
        return;
    }

    schedule(|_| {
        let redraw_result = redraw();
        clear_cmdline_redraw_pending();
        if let Err(err) = redraw_result {
            debug(&format!("redraw failed: {err}"));
        }
    });
}

pub(super) fn schedule_external_event_trigger() {
    if !mark_external_trigger_pending_if_idle() {
        return;
    }
    schedule_external_event_trigger_dispatch();
}

pub(crate) fn on_key_event() -> Result<()> {
    if !is_enabled() {
        return Ok(());
    }

    let buffer = api::get_current_buf();
    if !buffer.is_valid() {
        return Ok(());
    }
    let policy = current_buffer_event_policy(&buffer)?;
    if skip_current_buffer_events(&buffer)? {
        return Ok(());
    }

    let now = now_ms();
    let (key_delay_ms, delay_after_key_ms) = {
        let state = state_lock();
        (
            as_delay_ms(state.config.delay_after_key),
            state.config.delay_after_key.max(0.0),
        )
    };
    let elapsed_since_last_autocmd_ms = elapsed_ms_since_last_autocmd_event(now);
    let action = decide_key_event_action(
        policy,
        key_delay_ms,
        delay_after_key_ms,
        elapsed_since_last_autocmd_ms,
    );
    match action {
        KeyEventAction::None => {}
        KeyEventAction::TriggerExternal => schedule_external_event_trigger(),
        KeyEventAction::ScheduleKeyTimer { delay_ms } => schedule_key_event_timer(delay_ms),
    }

    Ok(())
}

fn replace_real_cursor_from_event(only_hide_real_cursor: bool) -> Result<()> {
    let mode = mode_string();
    let namespace_id = ensure_namespace_id();

    let maybe_prepaint_cell = {
        let mut state = state_lock();
        state.set_namespace_id(namespace_id);
        let current_corners = state.current_corners();

        if !state.is_enabled()
            || state.is_animating()
            || state.config.hide_target_hack
            || !state.config.mode_allowed(&mode)
            || !smear_outside_cmd_row(&current_corners)?
        {
            return Ok(());
        }

        if !state.is_cursor_hidden() {
            hide_real_cursor();
            state.set_cursor_hidden(true);
        }

        if only_hide_real_cursor {
            return Ok(());
        }

        ScreenCell::from_rounded_point(current_corners[0])
            .map(|cell| (cell, state.config.windows_zindex))
    };

    if let Some((cell, zindex)) = maybe_prepaint_cell {
        prepaint_cursor_block(namespace_id, cell, zindex)?;
    }

    Ok(())
}

fn apply_cursor_visibility_effect(effect: CursorVisibilityEffect) -> bool {
    let mut state = state_lock();
    match effect {
        CursorVisibilityEffect::Keep => false,
        CursorVisibilityEffect::Hide => {
            if state.is_cursor_hidden() {
                return false;
            }
            if !state.config.hide_target_hack {
                hide_real_cursor();
            }
            state.set_cursor_hidden(true);
            true
        }
        CursorVisibilityEffect::Show => {
            if !state.is_cursor_hidden() {
                return false;
            }
            if !state.config.hide_target_hack {
                unhide_real_cursor();
            }
            state.set_cursor_hidden(false);
            true
        }
    }
}

fn apply_render_action(
    namespace_id: u32,
    render_action: RenderAction,
    render_allocation_policy: RenderAllocationPolicy,
    render_side_effects: crate::reducer::RenderSideEffects,
) -> Result<RenderExecutionMetrics> {
    clear_prepaint_for_current_tab(namespace_id);
    let mut metrics = RenderExecutionMetrics::default();
    let allocation_policy = to_draw_allocation_policy(render_allocation_policy);
    match render_action {
        RenderAction::Draw(frame) => {
            let redraw_cmd = render_side_effects.redraw_after_draw_if_cmdline
                && smear_outside_cmd_row(&frame.corners)?;
            let draw_metrics = draw_current(namespace_id, &frame, allocation_policy)?;
            metrics.merge_apply_metrics(
                draw_metrics.planned_ops,
                draw_metrics.applied_ops,
                draw_metrics.skipped_ops_capacity,
                draw_metrics.created_windows,
                draw_metrics.reused_windows,
                draw_metrics.reuse_failed_missing_window,
                draw_metrics.reuse_failed_reconfigure,
                draw_metrics.reuse_failed_missing_buffer,
                draw_metrics.pruned_windows,
                draw_metrics.recovered_windows,
                draw_metrics.pool_snapshot,
            );

            if render_side_effects.draw_target_hack_after_draw {
                match draw_target_hack_block(namespace_id, &frame, allocation_policy) {
                    Ok(target_metrics) => {
                        metrics.merge_apply_metrics(
                            target_metrics.planned_ops,
                            target_metrics.applied_ops,
                            target_metrics.skipped_ops_capacity,
                            target_metrics.created_windows,
                            target_metrics.reused_windows,
                            target_metrics.reuse_failed_missing_window,
                            target_metrics.reuse_failed_reconfigure,
                            target_metrics.reuse_failed_missing_buffer,
                            target_metrics.pruned_windows,
                            target_metrics.recovered_windows,
                            target_metrics.pool_snapshot,
                        );
                    }
                    Err(err) => debug(&format!("target-hack draw failed: {err}")),
                }
            }
            let cursor_visibility_changed =
                apply_cursor_visibility_effect(render_side_effects.cursor_visibility);
            if redraw_cmd && (metrics.ops_applied > 0 || cursor_visibility_changed) {
                schedule_cmdline_redraw();
            }
        }
        RenderAction::ClearAll => {
            let max_kept_windows = {
                let state = state_lock();
                state.config.max_kept_windows
            };
            let clear_summary = clear_active_render_windows(namespace_id, max_kept_windows);
            metrics.windows_pruned = metrics
                .windows_pruned
                .saturating_add(clear_summary.pruned_windows);
            metrics.windows_hidden = metrics
                .windows_hidden
                .saturating_add(clear_summary.hidden_windows);
            metrics.windows_invalid_removed = metrics
                .windows_invalid_removed
                .saturating_add(clear_summary.invalid_removed_windows);
            let cursor_visibility_changed =
                apply_cursor_visibility_effect(render_side_effects.cursor_visibility);
            if render_side_effects.redraw_after_clear_if_cmdline
                && (clear_summary.had_visual_change() || cursor_visibility_changed)
            {
                schedule_cmdline_redraw();
            }
        }
        RenderAction::Noop => {}
    }

    Ok(metrics)
}

pub(super) fn on_cursor_event(args: AutocmdCallbackArgs) -> bool {
    let result = (|| -> Result<()> {
        if !is_enabled() {
            return Ok(());
        }

        let buffer = api::get_current_buf();
        if !buffer.is_valid() {
            return Ok(());
        }
        let policy = current_buffer_event_policy(&buffer)?;
        if skip_current_buffer_events(&buffer)? {
            return Ok(());
        }

        note_autocmd_event_now();

        // `vim.on_key` is a fallback for cursor-affecting actions that do not emit
        // movement/window autocmds. Once an autocmd arrived, discard any pending
        // key-triggered callback to avoid duplicate smears after key sequences.
        clear_key_event_timer();

        if args.event == "CursorMoved" || args.event == "CursorMovedI" {
            let mut state = state_lock();
            update_tracked_cursor_color(&mut state);
        }
        if policy.should_prepaint_cursor() {
            replace_real_cursor_from_event(false)?;
        }
        schedule_external_event_trigger();
        Ok(())
    })();

    match result {
        Ok(()) => false,
        Err(err) => {
            warn(&format!("cursor event failed: {err}"));
            false
        }
    }
}

pub(super) fn on_buf_enter(_args: AutocmdCallbackArgs) -> bool {
    let mut state = state_lock();
    state.set_delay_disabled(false);
    false
}

pub(super) fn on_colorscheme(_args: AutocmdCallbackArgs) -> bool {
    clear_highlight_cache();
    false
}
