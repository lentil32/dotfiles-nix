use crate::types::RenderFrame;
use nvim_oxi::Result;
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

mod apply;
mod palette;
mod render_plan;
mod window_pool;
pub(crate) use apply::ApplyMetrics;

pub(crate) const EXTMARK_ID: u32 = 999;
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

#[derive(Debug)]
pub(crate) struct DrawState {
    pub(crate) tabs: HashMap<i32, window_pool::TabWindows>,
    planner_state: render_plan::PlannerState,
}

impl Default for DrawState {
    fn default() -> Self {
        Self {
            tabs: HashMap::with_capacity(4),
            planner_state: render_plan::PlannerState::default(),
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
    )
}

pub(crate) fn draw_current(namespace_id: u32, frame: &RenderFrame) -> Result<ApplyMetrics> {
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

pub(crate) fn clear_active_render_windows(namespace_id: u32, max_kept_windows: usize) {
    let mut draw_state = draw_state_lock();
    window_pool::begin_frame(&mut draw_state.tabs);
    let _ = window_pool::prune(&mut draw_state.tabs, namespace_id, max_kept_windows);
    window_pool::release_unused(&mut draw_state.tabs, namespace_id);
}

pub(crate) fn purge_render_windows(namespace_id: u32) {
    let mut draw_state = draw_state_lock();
    window_pool::purge(&mut draw_state.tabs, namespace_id);
    draw_state.planner_state = render_plan::PlannerState::default();
}

pub(crate) fn clear_all_namespaces(namespace_id: u32) {
    {
        let mut draw_state = draw_state_lock();
        window_pool::purge(&mut draw_state.tabs, namespace_id);
        draw_state.planner_state = render_plan::PlannerState::default();
    }
    apply::clear_namespace_all_buffers(namespace_id);
}
