use super::log_draw_error;
use super::palette::{HighlightGroupNames, highlight_group_names};
use super::render_plan::{CellOp, HighlightRef, RenderPlan, Viewport};
use super::window_pool::{self, AcquireKind, WindowPlacement};
use super::{
    BRAILLE_CODE_MAX, BRAILLE_CODE_MIN, DrawState, EXTMARK_ID, OCTANT_CODE_MAX, OCTANT_CODE_MIN,
};
use crate::lua::i64_from_object;
use crate::types::RenderFrame;
use nvim_oxi::Result;
use nvim_oxi::api;
use nvim_oxi::api::opts::{OptionOpts, SetExtmarkOpts};
use nvim_oxi::api::types::ExtmarkVirtTextPosition;
use nvim_oxi::{Array, Object};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct ApplyMetrics {
    pub(crate) planned_ops: usize,
    pub(crate) applied_ops: usize,
    pub(crate) created_windows: usize,
    pub(crate) reused_windows: usize,
    pub(crate) reuse_failed_missing_window: usize,
    pub(crate) reuse_failed_reconfigure: usize,
    pub(crate) reuse_failed_missing_buffer: usize,
    pub(crate) pruned_windows: usize,
    pub(crate) recovered_windows: usize,
}

pub(crate) fn editor_bounds() -> Result<Viewport> {
    let opts = OptionOpts::builder().build();
    let lines: i64 = api::get_option_value("lines", &opts)?;
    let cmdheight: i64 = api::get_option_value("cmdheight", &opts)?;
    let columns: i64 = api::get_option_value("columns", &opts)?;
    let max_row = (lines - cmdheight).max(1);
    let max_col = columns.max(1);
    Ok(Viewport { max_row, max_col })
}

pub(crate) fn current_tab_handle() -> i32 {
    api::get_current_tabpage().handle()
}

pub(crate) fn redraw() -> Result<()> {
    Ok(api::command("redraw")?)
}

pub(crate) fn notify_delay_disabled_warning() -> Result<()> {
    Ok(api::command(
        "lua vim.notify(\"Smear cursor disabled in the current buffer due to high delay.\")",
    )?)
}

pub(crate) fn err_writeln(message: &str) {
    api::err_writeln(message);
}

pub(crate) fn clear_namespace_in_buffer(buffer: &mut api::Buffer, namespace_id: u32) {
    if let Err(err) = buffer.clear_namespace(namespace_id, 0..) {
        log_draw_error("clear render namespace", &err);
    }
}

pub(crate) fn clear_namespace_all_buffers(namespace_id: u32) {
    for mut buffer in api::list_bufs() {
        if buffer.is_valid() {
            clear_namespace_in_buffer(&mut buffer, namespace_id);
        }
    }
}

fn highlight_group<'a>(group_names: &'a HighlightGroupNames, reference: HighlightRef) -> &'a str {
    match reference {
        HighlightRef::Normal(level) => {
            let level_index = usize::try_from(level)
                .unwrap_or(0)
                .min(group_names.normal.len().saturating_sub(1));
            group_names
                .normal
                .get(level_index)
                .map(String::as_str)
                .unwrap_or("SmearCursor1")
        }
        HighlightRef::Inverted(level) => {
            let level_index = usize::try_from(level)
                .unwrap_or(0)
                .min(group_names.inverted.len().saturating_sub(1));
            group_names
                .inverted
                .get(level_index)
                .map(String::as_str)
                .unwrap_or("SmearCursorInverted1")
        }
    }
}

fn draw_character(
    draw_state: &mut DrawState,
    tab_handle: i32,
    namespace_id: u32,
    viewport: Viewport,
    op: &CellOp,
    hl_group: &str,
    metrics: &mut ApplyMetrics,
) -> Result<()> {
    if op.row < 1 || op.row > viewport.max_row || op.col < 1 || op.col > viewport.max_col {
        return Ok(());
    }

    let placement = WindowPlacement {
        row: op.row,
        col: op.col,
        zindex: op.zindex,
    };
    let acquired = window_pool::acquire(&mut draw_state.tabs, namespace_id, tab_handle, placement)?;
    metrics.reuse_failed_missing_window = metrics
        .reuse_failed_missing_window
        .saturating_add(acquired.reuse_failures.missing_window);
    metrics.reuse_failed_reconfigure = metrics
        .reuse_failed_reconfigure
        .saturating_add(acquired.reuse_failures.reconfigure_failed);
    metrics.reuse_failed_missing_buffer = metrics
        .reuse_failed_missing_buffer
        .saturating_add(acquired.reuse_failures.missing_buffer);
    match acquired.kind {
        AcquireKind::Created => {
            metrics.created_windows = metrics.created_windows.saturating_add(1);
        }
        AcquireKind::Reused => {
            metrics.reused_windows = metrics.reused_windows.saturating_add(1);
        }
    }

    let window_id = acquired.window_id;
    let mut buffer = acquired.buffer;
    let payload_matches = draw_state.tabs.get(&tab_handle).is_some_and(|tab_windows| {
        tab_windows.cached_payload_matches(window_id, op.character.as_str(), hl_group)
    });
    if payload_matches {
        return Ok(());
    }

    let extmark_opts = SetExtmarkOpts::builder()
        .id(EXTMARK_ID)
        .virt_text([(op.character.as_str(), hl_group)])
        .virt_text_pos(ExtmarkVirtTextPosition::Overlay)
        .virt_text_win_col(0)
        .build();

    if let Err(err) = buffer.set_extmark(namespace_id, 0, 0, &extmark_opts) {
        let recovered = window_pool::recover_invalid_window(
            &mut draw_state.tabs,
            namespace_id,
            tab_handle,
            window_id,
        );
        if recovered {
            metrics.recovered_windows = metrics.recovered_windows.saturating_add(1);
            log_draw_error("recover invalid render window", &err);
            return Ok(());
        }
        return Err(nvim_oxi::Error::Api(err));
    }

    metrics.applied_ops = metrics.applied_ops.saturating_add(1);
    if let Some(tab_windows) = draw_state.tabs.get_mut(&tab_handle) {
        tab_windows.cache_payload(window_id, op.character.as_str(), hl_group);
    }
    Ok(())
}

fn screen_char_code(row: i64, col: i64) -> Option<i64> {
    let args = Array::from_iter([Object::from(row), Object::from(col)]);
    let value = api::call_function("screenchar", args).ok()?;
    i64_from_object("screenchar", value).ok()
}

fn particle_can_draw_at(row: i64, col: i64) -> bool {
    let Some(bg_char_code) = screen_char_code(row, col) else {
        return false;
    };

    let is_space = bg_char_code == 32;
    let is_braille = (BRAILLE_CODE_MIN..=BRAILLE_CODE_MAX).contains(&bg_char_code);
    let is_octant = (OCTANT_CODE_MIN..=OCTANT_CODE_MAX).contains(&bg_char_code);
    is_space || is_braille || is_octant
}

fn apply_particle_op(
    draw_state: &mut DrawState,
    tab_handle: i32,
    namespace_id: u32,
    viewport: Viewport,
    group_names: &HighlightGroupNames,
    op: &super::render_plan::ParticleOp,
    metrics: &mut ApplyMetrics,
) -> Result<()> {
    if op.requires_background_probe && !particle_can_draw_at(op.cell.row, op.cell.col) {
        return Ok(());
    }

    let hl_group = highlight_group(group_names, op.cell.highlight);
    draw_character(
        draw_state,
        tab_handle,
        namespace_id,
        viewport,
        &op.cell,
        hl_group,
        metrics,
    )
}

fn apply_target_hack(
    draw_state: &mut DrawState,
    tab_handle: i32,
    namespace_id: u32,
    viewport: Viewport,
    group_names: &HighlightGroupNames,
    target_hack: super::render_plan::TargetHackOp,
    metrics: &mut ApplyMetrics,
) -> Result<()> {
    let op = CellOp {
        row: target_hack.row,
        col: target_hack.col,
        zindex: target_hack.zindex,
        character: "█".to_string(),
        highlight: HighlightRef::Normal(target_hack.level),
    };
    let hl_group = highlight_group(group_names, op.highlight);
    draw_character(
        draw_state,
        tab_handle,
        namespace_id,
        viewport,
        &op,
        hl_group,
        metrics,
    )
}

pub(crate) fn apply_plan(
    draw_state: &mut DrawState,
    namespace_id: u32,
    tab_handle: i32,
    frame: &RenderFrame,
    viewport: Viewport,
    plan: &RenderPlan,
) -> Result<ApplyMetrics> {
    let mut metrics = ApplyMetrics {
        planned_ops: plan
            .cell_ops
            .len()
            .saturating_add(plan.particle_ops.len())
            .saturating_add(usize::from(plan.target_hack.is_some())),
        ..ApplyMetrics::default()
    };

    if let Some(clear) = plan.clear {
        window_pool::begin_frame(&mut draw_state.tabs);
        metrics.pruned_windows = metrics.pruned_windows.saturating_add(window_pool::prune(
            &mut draw_state.tabs,
            namespace_id,
            clear.max_kept_windows,
        ));
    }

    let color_levels = frame.color_levels.max(1);
    let group_names = highlight_group_names(color_levels);

    let apply_result = (|| -> Result<()> {
        for op in &plan.particle_ops {
            apply_particle_op(
                draw_state,
                tab_handle,
                namespace_id,
                viewport,
                &group_names,
                op,
                &mut metrics,
            )?;
        }

        for op in &plan.cell_ops {
            let hl_group = highlight_group(&group_names, op.highlight);
            draw_character(
                draw_state,
                tab_handle,
                namespace_id,
                viewport,
                op,
                hl_group,
                &mut metrics,
            )?;
        }

        if let Some(target_hack) = plan.target_hack {
            apply_target_hack(
                draw_state,
                tab_handle,
                namespace_id,
                viewport,
                &group_names,
                target_hack,
                &mut metrics,
            )?;
        }

        Ok(())
    })();

    window_pool::release_unused(&mut draw_state.tabs, namespace_id);
    apply_result.map(|()| metrics)
}
