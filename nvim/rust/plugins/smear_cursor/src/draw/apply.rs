use super::log_draw_error;
use super::palette::{HighlightGroupNames, highlight_group_names};
use super::render_plan::{CellOp, Glyph, HighlightRef, RenderPlan, Viewport};
use super::window_pool::{self, AcquireKind, AllocationPolicy, TabPoolSnapshot, WindowPlacement};
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
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct ApplyMetrics {
    pub(crate) planned_ops: usize,
    pub(crate) applied_ops: usize,
    pub(crate) skipped_ops_capacity: usize,
    pub(crate) created_windows: usize,
    pub(crate) reused_windows: usize,
    pub(crate) reuse_failed_missing_window: usize,
    pub(crate) reuse_failed_reconfigure: usize,
    pub(crate) reuse_failed_missing_buffer: usize,
    pub(crate) pruned_windows: usize,
    pub(crate) recovered_windows: usize,
    pub(crate) pool_snapshot: Option<TabPoolSnapshot>,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SpanChunk {
    glyph: Glyph,
    highlight: HighlightRef,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SpanOp {
    row: i64,
    col: i64,
    width: u32,
    zindex: u32,
    chunk_start: usize,
    chunk_len: usize,
}

#[derive(Debug, Default)]
pub(crate) struct SpanArena {
    spans: Vec<SpanOp>,
    chunks: Vec<SpanChunk>,
}

impl SpanArena {
    fn clear(&mut self) {
        self.spans.clear();
        self.chunks.clear();
    }

    fn flush_pending(&mut self, pending: &mut Option<SpanOp>) {
        if let Some(span) = pending.take() {
            self.spans.push(span);
        }
    }

    fn push_cell(&mut self, viewport: Viewport, op: &CellOp, pending: &mut Option<SpanOp>) {
        if op.row < 1 || op.row > viewport.max_row || op.col < 1 || op.col > viewport.max_col {
            return;
        }

        let can_append = pending.as_ref().is_some_and(|active| {
            active.row == op.row
                && active.zindex == op.zindex
                && op.col == active.col.saturating_add(i64::from(active.width))
        });
        if !can_append {
            self.flush_pending(pending);
            *pending = Some(SpanOp {
                row: op.row,
                col: op.col,
                width: 0,
                zindex: op.zindex,
                chunk_start: self.chunks.len(),
                chunk_len: 0,
            });
        }

        let Some(span) = pending.as_mut() else {
            return;
        };
        span.width = span.width.saturating_add(1);
        span.chunk_len = span.chunk_len.saturating_add(1);
        self.chunks.push(SpanChunk {
            glyph: op.glyph,
            highlight: op.highlight,
        });
    }
}

fn span_payload_hash(chunks: &[SpanChunk]) -> u64 {
    let mut hasher = DefaultHasher::new();
    for chunk in chunks {
        chunk.glyph.hash(&mut hasher);
        chunk.highlight.hash(&mut hasher);
    }
    hasher.finish()
}

fn draw_span(
    draw_state: &mut DrawState,
    tab_handle: i32,
    namespace_id: u32,
    allocation_policy: AllocationPolicy,
    span: SpanOp,
    span_chunks: &[SpanChunk],
    group_names: &HighlightGroupNames,
    metrics: &mut ApplyMetrics,
) -> Result<()> {
    if span.width == 0 || span_chunks.is_empty() {
        return Ok(());
    }

    let placement = WindowPlacement {
        row: span.row,
        col: span.col,
        width: span.width,
        zindex: span.zindex,
    };
    let acquired = window_pool::acquire(
        &mut draw_state.tabs,
        namespace_id,
        tab_handle,
        placement,
        allocation_policy,
    )?;
    let Some(acquired) = acquired else {
        metrics.skipped_ops_capacity = metrics.skipped_ops_capacity.saturating_add(1);
        return Ok(());
    };
    metrics.reuse_failed_missing_window = metrics
        .reuse_failed_missing_window
        .saturating_add(acquired.reuse_failures.missing_window);
    metrics.reuse_failed_reconfigure = metrics
        .reuse_failed_reconfigure
        .saturating_add(acquired.reuse_failures.reconfigure_failed);
    metrics.reuse_failed_missing_buffer = metrics
        .reuse_failed_missing_buffer
        .saturating_add(acquired.reuse_failures.missing_buffer);
    if matches!(acquired.kind, AcquireKind::Reused) {
        metrics.reused_windows = metrics.reused_windows.saturating_add(1);
    }

    let window_id = acquired.window_id;
    let payload_hash = span_payload_hash(span_chunks);
    let mut buffer = acquired.buffer;
    let payload_matches = draw_state
        .tabs
        .get(&tab_handle)
        .is_some_and(|tab_windows| tab_windows.cached_payload_matches(window_id, payload_hash));
    if payload_matches {
        return Ok(());
    }

    let extmark_opts = SetExtmarkOpts::builder()
        .id(EXTMARK_ID)
        .virt_text(span_chunks.iter().map(|chunk| {
            (
                chunk.glyph.as_str(),
                highlight_group(group_names, chunk.highlight),
            )
        }))
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
        tab_windows.cache_payload(window_id, payload_hash);
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

fn build_span_ops(plan: &RenderPlan, viewport: Viewport, arena: &mut SpanArena) {
    arena.clear();
    let mut pending: Option<SpanOp> = None;

    for op in &plan.particle_ops {
        if op.requires_background_probe && !particle_can_draw_at(op.cell.row, op.cell.col) {
            continue;
        }
        arena.push_cell(viewport, &op.cell, &mut pending);
    }

    for op in &plan.cell_ops {
        arena.push_cell(viewport, op, &mut pending);
    }

    if let Some(target_hack) = plan.target_hack {
        let target_cell = CellOp {
            row: target_hack.row,
            col: target_hack.col,
            zindex: target_hack.zindex,
            glyph: Glyph::BLOCK,
            highlight: HighlightRef::Normal(target_hack.level),
        };
        arena.push_cell(viewport, &target_cell, &mut pending);
    }
    arena.flush_pending(&mut pending);
}

pub(crate) fn apply_plan(
    draw_state: &mut DrawState,
    namespace_id: u32,
    tab_handle: i32,
    frame: &RenderFrame,
    viewport: Viewport,
    plan: &RenderPlan,
    _allocation_policy: AllocationPolicy,
) -> Result<ApplyMetrics> {
    let color_levels = frame.color_levels.max(1);
    let group_names = highlight_group_names(color_levels);
    let mut span_arena = std::mem::take(&mut draw_state.span_arena);
    build_span_ops(plan, viewport, &mut span_arena);

    let mut metrics = ApplyMetrics {
        planned_ops: span_arena.spans.len(),
        ..ApplyMetrics::default()
    };
    if plan.clear.is_some() {
        window_pool::begin_frame_for_tab(&mut draw_state.tabs, tab_handle, metrics.planned_ops);
    }

    let in_use_windows = window_pool::tab_pool_snapshot(&draw_state.tabs, tab_handle)
        .map_or(0, |snapshot| snapshot.in_use_windows);
    let required_capacity = in_use_windows
        .saturating_add(metrics.planned_ops)
        .min(frame.max_kept_windows);
    metrics.created_windows =
        metrics
            .created_windows
            .saturating_add(window_pool::ensure_tab_capacity(
                &mut draw_state.tabs,
                tab_handle,
                required_capacity,
                frame.max_kept_windows,
            )?);

    let apply_result = (|| -> Result<()> {
        for span in span_arena.spans.iter().copied() {
            let chunk_end = span.chunk_start.saturating_add(span.chunk_len);
            let Some(span_chunks) = span_arena.chunks.get(span.chunk_start..chunk_end) else {
                continue;
            };
            draw_span(
                draw_state,
                tab_handle,
                namespace_id,
                AllocationPolicy::ReuseOnly,
                span,
                span_chunks,
                &group_names,
                &mut metrics,
            )?;
        }
        Ok(())
    })();

    draw_state.span_arena = span_arena;
    let _ = window_pool::release_unused_tab(&mut draw_state.tabs, namespace_id, tab_handle);
    metrics.pool_snapshot = window_pool::tab_pool_snapshot(&draw_state.tabs, tab_handle);
    apply_result.map(|()| metrics)
}
