use crate::types::{RenderFrame, ScreenCell};
use nvim_oxi::Result;
use nvim_oxi::api;
use nvim_oxi::api::opts::{OptionOpts, OptionScope, SetExtmarkOpts};
use nvim_oxi::api::types::{ExtmarkVirtTextPosition, WindowConfig, WindowRelativeTo, WindowStyle};
use nvim_oxi_utils::handles;
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

mod apply;
mod palette;
mod render_plan;
mod window_pool;
pub(crate) use apply::ApplyMetrics;
pub(crate) use window_pool::AllocationPolicy;
pub(crate) use window_pool::{GlobalPoolSnapshot, TabPoolSnapshot};

pub(crate) const EXTMARK_ID: u32 = 999;
const PREPAINT_EXTMARK_ID: u32 = 1001;
const PREPAINT_BUFFER_FILETYPE: &str = "smear-cursor-prepaint";
const PREPAINT_CHARACTER: &str = "█";
const PREPAINT_HIGHLIGHT_GROUP: &str = "Cursor";
pub(crate) const BRAILLE_CODE_MIN: i64 = 0x2800;
pub(crate) const BRAILLE_CODE_MAX: i64 = 0x28FF;
pub(crate) const OCTANT_CODE_MIN: i64 = 0x1CD00;
pub(crate) const OCTANT_CODE_MAX: i64 = 0x1CDE7;
pub(crate) const PARTICLE_ZINDEX_OFFSET: u32 = 1;
pub(crate) const BLOCK_ASPECT_RATIO: f64 = 2.0;

pub(crate) const BOTTOM_BLOCKS: [&str; 9] = ["█", "▇", "▆", "▅", "▄", "▃", "▂", "▁", " "];
pub(crate) const LEFT_BLOCKS: [&str; 9] = [" ", "▏", "▎", "▍", "▌", "▋", "▊", "▉", "█"];
pub(crate) const MATRIX_CHARACTERS: [&str; 16] = [
    "", "▘", "▝", "▀", "▖", "▌", "▞", "▛", "▗", "▚", "▐", "▜", "▄", "▙", "▟", "█",
];

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct ClearActiveRenderWindowsSummary {
    pub(crate) had_visible_windows_before_clear: bool,
    pub(crate) pruned_windows: usize,
    pub(crate) hidden_windows: usize,
    pub(crate) invalid_removed_windows: usize,
}

impl ClearActiveRenderWindowsSummary {
    pub(crate) fn had_visual_change(self) -> bool {
        self.had_visible_windows_before_clear
            && (self.pruned_windows > 0
                || self.hidden_windows > 0
                || self.invalid_removed_windows > 0)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PrepaintPlacement {
    cell: ScreenCell,
    zindex: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PrepaintOverlay {
    window_id: i32,
    buffer_id: i32,
    placement: Option<PrepaintPlacement>,
}

#[derive(Debug)]
pub(crate) struct DrawState {
    pub(crate) tabs: HashMap<i32, window_pool::TabWindows>,
    planner_state: render_plan::PlannerState,
    prepaint_by_tab: HashMap<i32, PrepaintOverlay>,
    span_arena: apply::SpanArena,
}

impl Default for DrawState {
    fn default() -> Self {
        Self {
            tabs: HashMap::with_capacity(4),
            planner_state: render_plan::PlannerState::default(),
            prepaint_by_tab: HashMap::with_capacity(2),
            span_arena: apply::SpanArena::default(),
        }
    }
}

#[derive(Debug)]
struct DrawContext {
    draw_state: Mutex<DrawState>,
}

impl DrawContext {
    fn new() -> Self {
        Self {
            draw_state: Mutex::new(DrawState::default()),
        }
    }
}

static DRAW_CONTEXT: LazyLock<DrawContext> = LazyLock::new(DrawContext::new);

pub(crate) fn log_draw_error(context: &str, err: &impl std::fmt::Display) {
    apply::err_writeln(&format!("[smear_cursor][draw] {context} failed: {err}"));
}

fn draw_state_lock() -> std::sync::MutexGuard<'static, DrawState> {
    loop {
        match DRAW_CONTEXT.draw_state.lock() {
            Ok(guard) => return guard,
            Err(poisoned) => {
                let mut guard = poisoned.into_inner();
                *guard = DrawState::default();
                drop(guard);
                DRAW_CONTEXT.draw_state.clear_poison();
            }
        }
    }
}

pub(crate) fn clear_highlight_cache() {
    palette::clear_highlight_cache();
}

pub(crate) fn redraw() -> Result<()> {
    apply::redraw()
}

pub(crate) fn notify_delay_disabled_warning() -> Result<()> {
    apply::notify_delay_disabled_warning()
}

pub(crate) fn draw_target_hack_block(
    namespace_id: u32,
    frame: &RenderFrame,
    allocation_policy: AllocationPolicy,
) -> Result<ApplyMetrics> {
    if namespace_id == 0 {
        return Ok(ApplyMetrics::default());
    }

    palette::ensure_highlight_palette(frame)?;
    let viewport = apply::editor_bounds()?;
    let Some(target_hack) = render_plan::plan_target_hack(frame, viewport) else {
        return Ok(ApplyMetrics::default());
    };

    let tab_handle = apply::current_tab_handle();
    let mut draw_state = draw_state_lock();
    let plan = render_plan::RenderPlan {
        clear: None,
        cell_ops: Vec::new(),
        particle_ops: Vec::new(),
        target_hack: Some(target_hack),
    };

    apply::apply_plan(
        &mut draw_state,
        namespace_id,
        tab_handle,
        frame,
        viewport,
        &plan,
        allocation_policy,
    )
}

pub(crate) fn draw_current(
    namespace_id: u32,
    frame: &RenderFrame,
    allocation_policy: AllocationPolicy,
) -> Result<ApplyMetrics> {
    if namespace_id == 0 {
        return Ok(ApplyMetrics::default());
    }

    palette::ensure_highlight_palette(frame)?;

    let viewport = apply::editor_bounds()?;
    let tab_handle = apply::current_tab_handle();
    let mut draw_state = draw_state_lock();

    let maybe_signature = render_plan::frame_draw_signature(frame);
    if let Some(signature) = maybe_signature
        && window_pool::last_draw_signature(&draw_state.tabs, tab_handle)
            .is_some_and(|previous| previous == signature)
    {
        return Ok(ApplyMetrics::default());
    }

    let planner_output =
        render_plan::render_frame_to_plan(frame, draw_state.planner_state, viewport);
    draw_state.planner_state = planner_output.next_state;
    let draw_result = apply::apply_plan(
        &mut draw_state,
        namespace_id,
        tab_handle,
        frame,
        viewport,
        &planner_output.plan,
        allocation_policy,
    );

    window_pool::set_last_draw_signature(
        &mut draw_state.tabs,
        tab_handle,
        if draw_result.is_ok() {
            planner_output.signature
        } else {
            None
        },
    );

    draw_result
}

fn prepaint_open_window_config(placement: PrepaintPlacement, hidden: bool) -> WindowConfig {
    let mut builder = WindowConfig::builder();
    builder
        .relative(WindowRelativeTo::Editor)
        .row(placement.cell.row() as f64 - 1.0)
        .col(placement.cell.col() as f64 - 1.0)
        .width(1)
        .height(1)
        .focusable(false)
        .style(WindowStyle::Minimal)
        .noautocmd(true)
        .hide(hidden)
        .zindex(placement.zindex);
    builder.build()
}

fn prepaint_reconfigure_window_config(placement: PrepaintPlacement, hidden: bool) -> WindowConfig {
    let mut builder = WindowConfig::builder();
    builder
        .relative(WindowRelativeTo::Editor)
        .row(placement.cell.row() as f64 - 1.0)
        .col(placement.cell.col() as f64 - 1.0)
        .width(1)
        .height(1)
        .focusable(false)
        .style(WindowStyle::Minimal)
        .hide(hidden)
        .zindex(placement.zindex);
    builder.build()
}

fn hidden_prepaint_window_config() -> WindowConfig {
    let mut builder = WindowConfig::builder();
    builder.hide(true);
    builder.build()
}

fn set_existing_window_config(window: &mut api::Window, mut config: WindowConfig) -> Result<()> {
    // nvim_win_set_config rejects the `noautocmd` key for existing windows.
    config.noautocmd = None;
    window.set_config(&config)?;
    Ok(())
}

fn initialize_prepaint_buffer_options(buffer: &api::Buffer) -> Result<()> {
    let opts = OptionOpts::builder().buf(buffer.clone()).build();
    api::set_option_value("buftype", "nofile", &opts)?;
    api::set_option_value("filetype", PREPAINT_BUFFER_FILETYPE, &opts)?;
    api::set_option_value("bufhidden", "wipe", &opts)?;
    api::set_option_value("swapfile", false, &opts)?;
    Ok(())
}

fn initialize_prepaint_window_options(window: &api::Window) -> Result<()> {
    let opts = OptionOpts::builder()
        .scope(OptionScope::Local)
        .win(window.clone())
        .build();
    api::set_option_value("winhighlight", "NormalFloat:Normal", &opts)?;
    api::set_option_value("winblend", 100_i64, &opts)?;
    Ok(())
}

fn close_prepaint_overlay(namespace_id: u32, overlay: PrepaintOverlay) {
    if let Some(mut buffer) = handles::valid_buffer(i64::from(overlay.buffer_id))
        && let Err(err) = buffer.clear_namespace(namespace_id, 0..)
    {
        log_draw_error("clear prepaint namespace", &err);
    }
    if let Some(window) = handles::valid_window(i64::from(overlay.window_id))
        && let Err(err) = window.close(true)
    {
        log_draw_error("close prepaint overlay window", &err);
    }
}

fn valid_prepaint_handles(overlay: PrepaintOverlay) -> Option<(api::Window, api::Buffer)> {
    let window = handles::valid_window(i64::from(overlay.window_id))?;
    let buffer = handles::valid_buffer(i64::from(overlay.buffer_id))?;
    Some((window, buffer))
}

fn create_prepaint_overlay(
    placement: PrepaintPlacement,
) -> Result<(PrepaintOverlay, api::Window, api::Buffer)> {
    let buffer = api::create_buf(false, true)?;
    initialize_prepaint_buffer_options(&buffer)?;

    let config = prepaint_open_window_config(placement, false);
    let window = api::open_win(&buffer, false, &config)?;
    initialize_prepaint_window_options(&window)?;

    let overlay = PrepaintOverlay {
        window_id: window.handle(),
        buffer_id: buffer.handle(),
        placement: Some(placement),
    };
    Ok((overlay, window, buffer))
}

fn hide_prepaint_overlay(overlay: &mut PrepaintOverlay) -> bool {
    let Some(mut window) = handles::valid_window(i64::from(overlay.window_id)) else {
        return false;
    };

    if let Err(err) = set_existing_window_config(&mut window, hidden_prepaint_window_config()) {
        log_draw_error("hide prepaint overlay window", &err);
        return false;
    }
    overlay.placement = None;
    true
}

pub(crate) fn prepaint_cursor_block(
    namespace_id: u32,
    cell: ScreenCell,
    zindex: u32,
) -> Result<()> {
    if namespace_id == 0 {
        return Ok(());
    }

    let tab_handle = apply::current_tab_handle();
    let mut draw_state = draw_state_lock();
    let requested_placement = PrepaintPlacement { cell, zindex };

    let previous = draw_state.prepaint_by_tab.remove(&tab_handle);
    let mut overlay = previous;
    let mut handles_pair = previous.and_then(valid_prepaint_handles);

    if handles_pair.is_none() {
        if let Some(stale) = previous {
            close_prepaint_overlay(namespace_id, stale);
        }

        match create_prepaint_overlay(requested_placement) {
            Ok((created_overlay, window, buffer)) => {
                overlay = Some(created_overlay);
                handles_pair = Some((window, buffer));
            }
            Err(err) => {
                // Prepaint is non-critical: keep cursor callback non-fatal.
                log_draw_error("create prepaint overlay", &err);
                return Ok(());
            }
        }
    }

    let Some((mut window, mut buffer)) = handles_pair else {
        return Ok(());
    };

    if overlay.is_some_and(|entry| entry.placement != Some(requested_placement))
        && let Err(err) = set_existing_window_config(
            &mut window,
            prepaint_reconfigure_window_config(requested_placement, false),
        )
    {
        log_draw_error("reconfigure prepaint overlay window", &err);

        if let Some(stale) = overlay {
            close_prepaint_overlay(namespace_id, stale);
        }
        match create_prepaint_overlay(requested_placement) {
            Ok((created_overlay, _recreated_window, recreated_buffer)) => {
                overlay = Some(created_overlay);
                buffer = recreated_buffer;
            }
            Err(recreate_err) => {
                log_draw_error(
                    "recreate prepaint overlay after reconfigure failure",
                    &recreate_err,
                );
                return Ok(());
            }
        }
    }

    let extmark_opts = SetExtmarkOpts::builder()
        .id(PREPAINT_EXTMARK_ID)
        .virt_text([(PREPAINT_CHARACTER, PREPAINT_HIGHLIGHT_GROUP)])
        .virt_text_pos(ExtmarkVirtTextPosition::Overlay)
        .virt_text_win_col(0)
        .build();
    if let Err(err) = buffer.set_extmark(namespace_id, 0, 0, &extmark_opts) {
        log_draw_error("set prepaint overlay payload", &err);
        return Ok(());
    }

    if let Some(mut entry) = overlay {
        entry.placement = Some(requested_placement);
        draw_state.prepaint_by_tab.insert(tab_handle, entry);
    }
    Ok(())
}

pub(crate) fn clear_prepaint_for_current_tab(namespace_id: u32) {
    if namespace_id == 0 {
        return;
    }

    let tab_handle = apply::current_tab_handle();
    let mut draw_state = draw_state_lock();
    let Some(entry) = draw_state.prepaint_by_tab.get_mut(&tab_handle) else {
        return;
    };
    if !hide_prepaint_overlay(entry) {
        let _ = draw_state.prepaint_by_tab.remove(&tab_handle);
    }
}

fn clear_all_prepaint_locked(draw_state: &mut DrawState, namespace_id: u32) {
    let prepaint_by_tab = std::mem::take(&mut draw_state.prepaint_by_tab);
    for overlay in prepaint_by_tab.values().copied() {
        close_prepaint_overlay(namespace_id, overlay);
    }
}

pub(crate) fn global_pool_snapshot() -> GlobalPoolSnapshot {
    let draw_state = draw_state_lock();
    window_pool::global_pool_snapshot(&draw_state.tabs)
}

pub(crate) fn clear_active_render_windows(
    namespace_id: u32,
    max_kept_windows: usize,
) -> ClearActiveRenderWindowsSummary {
    let mut draw_state = draw_state_lock();
    let had_visible_windows_before_clear = window_pool::has_visible_windows(&draw_state.tabs);
    if !window_pool::has_pending_clear_work(&draw_state.tabs, max_kept_windows) {
        return ClearActiveRenderWindowsSummary {
            had_visible_windows_before_clear,
            ..ClearActiveRenderWindowsSummary::default()
        };
    }

    window_pool::begin_frame(&mut draw_state.tabs);
    let pruned_windows = window_pool::prune(&mut draw_state.tabs, namespace_id, max_kept_windows);
    let release_summary = window_pool::release_unused(&mut draw_state.tabs, namespace_id);
    ClearActiveRenderWindowsSummary {
        had_visible_windows_before_clear,
        pruned_windows,
        hidden_windows: release_summary.hidden_windows,
        invalid_removed_windows: release_summary.invalid_removed_windows,
    }
}

pub(crate) fn purge_render_windows(namespace_id: u32) {
    let mut draw_state = draw_state_lock();
    clear_all_prepaint_locked(&mut draw_state, namespace_id);
    window_pool::purge(&mut draw_state.tabs, namespace_id);
    draw_state.planner_state = render_plan::PlannerState::default();
}

pub(crate) fn clear_all_namespaces(namespace_id: u32) {
    {
        let mut draw_state = draw_state_lock();
        clear_all_prepaint_locked(&mut draw_state, namespace_id);
        window_pool::purge(&mut draw_state.tabs, namespace_id);
        draw_state.planner_state = render_plan::PlannerState::default();
    }
    apply::clear_namespace_all_buffers(namespace_id);
}
