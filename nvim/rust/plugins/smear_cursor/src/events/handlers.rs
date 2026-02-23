use super::cursor::{
    cursor_position_for_mode, line_value, mode_string, smear_outside_cmd_row,
    update_tracked_cursor_color,
};
use super::geometry::{current_cursor_snapshot, frame_reaches_target_cell, maybe_scroll_shift};
use super::logging::{debug, hide_real_cursor, log_slow_callback, unhide_real_cursor, warn};
use super::policy::{
    BufferEventPolicy, current_buffer_event_policy, remaining_throttle_delay_ms,
    skip_current_buffer_events,
};
use super::runtime::{
    clear_animation_timer, clear_external_event_timer, clear_external_trigger_pending,
    clear_key_event_timer, cursor_callback_duration_estimate_ms,
    elapsed_ms_since_last_autocmd_event, elapsed_ms_since_last_external_dispatch, is_enabled,
    mark_external_trigger_pending_if_idle, note_autocmd_event_now, note_external_dispatch_now,
    now_ms, record_cursor_callback_duration, seed_from_clock, state_lock,
};
use super::timers::{
    apply_render_cleanup_action, ensure_namespace_id, schedule_animation_tick,
    schedule_external_event_timer, schedule_external_throttle_timer, schedule_key_event_timer,
};
use crate::draw::{
    clear_active_render_windows, clear_highlight_cache, draw_current, draw_target_hack_block,
};
use crate::reducer::{
    CursorCommand, CursorEventContext, EventSource, RenderAction, as_delay_ms, build_render_frame,
    external_settle_delay_ms, next_animation_delay_ms, reduce_cursor_event,
};
use crate::state::CursorLocation;
use crate::types::Point;
use nvim_oxi::api::types::AutocmdCallbackArgs;
use nvim_oxi::{Result, api, schedule};
use nvim_utils::mode::is_cmdline_mode;

fn render_action_perf_details(render_action: &RenderAction) -> String {
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
                "action=draw area_cells={area_cells} height_cells={height_cells} width_cells={width_cells} particles={} vertical_bar={} max_kept_windows={}",
                frame.particles.len(),
                frame.vertical_bar,
                frame.max_kept_windows
            )
        }
        RenderAction::ClearAll => "action=clear_all".to_string(),
        RenderAction::Noop => "action=noop".to_string(),
    }
}

pub(super) fn on_cursor_event_impl(source: EventSource) -> Result<()> {
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
    let buffer_event_policy = current_buffer_event_policy(&buffer)?;
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
        // Match upstream behavior: ignore transient cursor-position misses instead of
        // clearing the current smear state.
        return Ok(());
    };

    let current_win_handle = i64::from(window.handle());
    let current_buf_handle = i64::from(buffer.handle());
    let current_top_row = line_value("w0")?;
    let current_line = line_value(".")?;
    let current_location = CursorLocation::new(
        current_win_handle,
        current_buf_handle,
        current_top_row,
        current_line,
    );
    let (scroll_buffer_space, previous_location, current_corners) = {
        let state = state_lock();
        (
            state.config.scroll_buffer_space,
            state.tracked_location(),
            state.current_corners(),
        )
    };
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
    let perf_render_details = render_action_perf_details(&render_decision.render_action);
    let perf_cleanup_details = format!("cleanup={:?}", render_decision.render_cleanup_action);

    if notify_delay_disabled
        && let Err(err) = api::command(
            "lua vim.notify(\"Smear cursor disabled in the current buffer due to high delay.\")",
        )
    {
        warn(&format!("delay-disabled notify failed: {err}"));
    }

    apply_render_action(namespace_id, &mode, render_decision.render_action)?;

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
        &format!("{perf_render_details} {perf_cleanup_details}"),
    );
    let should_schedule = match source {
        EventSource::AnimationTick => true,
        EventSource::External => command.is_some(),
    };
    let maybe_delay = {
        let mut state = state_lock();
        if should_schedule
            && state.is_enabled()
            && state.is_animating()
            && !state.is_delay_disabled()
        {
            let step_interval_ms = command
                .map(|value| match value {
                    CursorCommand::StepIntervalMs(step_interval_ms) => step_interval_ms,
                })
                .unwrap_or(state.config.time_interval);
            Some(next_animation_delay_ms(
                &mut state,
                step_interval_ms,
                callback_duration_ms,
            ))
        } else {
            None
        }
    };
    match (maybe_delay, source) {
        (Some(delay), _) => schedule_animation_tick(delay.max(animation_delay_floor_ms)),
        (None, EventSource::AnimationTick) => clear_animation_timer(),
        (None, EventSource::External) => {}
    }

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
    if !policy.should_use_debounced_external_settle() {
        return on_throttled_ui_external_event_trigger(policy);
    }

    let mode = mode_string();
    let (smear_to_cmd, delay_ms) = {
        let state = state_lock();
        (
            state.config.smear_to_cmd,
            external_settle_delay_ms(state.config.delay_event_to_smear),
        )
    };
    if is_cmdline_mode(&mode) && !smear_to_cmd {
        clear_external_event_timer();
        let mut state = state_lock();
        state.clear_pending_external_event();
        return Ok(());
    }

    let snapshot = current_cursor_snapshot(smear_to_cmd)?;
    let Some(snapshot) = snapshot else {
        {
            let mut state = state_lock();
            state.clear_pending_external_event();
        }
        return Ok(());
    };

    {
        let mut state = state_lock();
        state.set_pending_external_event(Some(snapshot));
    }
    schedule_external_event_timer(delay_ms);
    Ok(())
}

fn on_throttled_ui_external_event_trigger(policy: BufferEventPolicy) -> Result<()> {
    let throttle_interval_ms = policy.settle_delay_floor_ms();
    let elapsed_ms = elapsed_ms_since_last_external_dispatch(now_ms());
    let remaining_delay_ms = remaining_throttle_delay_ms(throttle_interval_ms, elapsed_ms);

    if remaining_delay_ms == 0 {
        on_cursor_event_impl(EventSource::External)?;
        note_external_dispatch_now();
        return Ok(());
    }

    schedule_external_throttle_timer(remaining_delay_ms);
    Ok(())
}

pub(super) fn schedule_external_event_trigger() {
    if !mark_external_trigger_pending_if_idle() {
        return;
    }

    schedule(|_| {
        clear_external_trigger_pending();
        if let Err(err) = on_external_event_trigger() {
            warn(&format!("external event trigger failed: {err}"));
        }
    });
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
    if !policy.use_key_fallback() {
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
    if elapsed_ms_since_last_autocmd_event(now) <= delay_after_key_ms {
        return Ok(());
    }
    if !policy.should_use_debounced_external_settle() {
        schedule_external_event_trigger();
        return Ok(());
    }
    schedule_key_event_timer(key_delay_ms);
    Ok(())
}

fn replace_real_cursor_from_event(only_hide_real_cursor: bool) -> Result<()> {
    let mode = mode_string();
    let namespace_id = ensure_namespace_id();

    let maybe_frame = {
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

        let vertical_bar = state.config.cursor_is_vertical_bar(&mode);
        let mut frame = build_render_frame(
            &state,
            &mode,
            current_corners,
            Point {
                row: -1.0,
                col: -1.0,
            },
            vertical_bar,
            None,
        );
        frame.particles.clear();
        Some(frame)
    };

    if let Some(frame) = maybe_frame {
        draw_current(namespace_id, &frame)?;
    }

    Ok(())
}

fn apply_render_action(namespace_id: u32, mode: &str, render_action: RenderAction) -> Result<()> {
    let cmdline_mode = is_cmdline_mode(mode);
    match render_action {
        RenderAction::Draw(frame) => {
            let redraw_cmd = cmdline_mode && smear_outside_cmd_row(&frame.corners)?;
            draw_current(namespace_id, &frame)?;
            if redraw_cmd && let Err(err) = api::command("redraw") {
                debug(&format!("redraw after draw failed: {err}"));
            }

            let should_unhide_while_animating =
                cmdline_mode || (frame.never_draw_over_target && frame_reaches_target_cell(&frame));
            if !cmdline_mode && !should_unhide_while_animating && frame.hide_target_hack {
                draw_target_hack_block(namespace_id, &frame)?;
            }
            let mut state = state_lock();
            if should_unhide_while_animating {
                if state.is_cursor_hidden() {
                    if !state.config.hide_target_hack {
                        unhide_real_cursor();
                    }
                    state.set_cursor_hidden(false);
                }
            } else if !state.is_cursor_hidden() && !cmdline_mode {
                if !state.config.hide_target_hack {
                    hide_real_cursor();
                }
                state.set_cursor_hidden(true);
            }
        }
        RenderAction::ClearAll => {
            let max_kept_windows = {
                let state = state_lock();
                state.config.max_kept_windows
            };
            clear_active_render_windows(namespace_id, max_kept_windows);
            if cmdline_mode && let Err(err) = api::command("redraw") {
                debug(&format!("redraw after clear failed: {err}"));
            }
            let mut state = state_lock();
            if state.is_cursor_hidden() {
                if !state.config.hide_target_hack {
                    unhide_real_cursor();
                }
                state.set_cursor_hidden(false);
            }
        }
        RenderAction::Noop => {}
    }

    Ok(())
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
