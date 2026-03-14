use crate::core::realization::{PaletteSpec, realize_logical_raster};
use crate::core::state::ProjectionSnapshot;
use crate::core::types::RenderOutcome;
use crate::mutex::lock_with_poison_recovery;
use crate::types::ScreenCell;
use nvim_oxi::Result;
use nvim_oxi::api;
use nvim_oxi::api::opts::{OptionOpts, OptionScope, SetExtmarkOpts};
use nvim_oxi::api::types::{ExtmarkVirtTextPosition, WindowConfig, WindowRelativeTo, WindowStyle};
use nvim_oxi_utils::handles;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

mod apply;
mod palette;
pub(crate) mod render_plan;
mod window_pool;
pub(crate) use apply::ApplyMetrics;
pub(crate) use window_pool::AllocationPolicy;

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

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct PurgeRenderWindowsSummary {
    pub(crate) had_visible_render_windows_before_purge: bool,
    pub(crate) had_visible_prepaint_before_purge: bool,
    pub(crate) purged_windows: usize,
    pub(crate) cleared_prepaint_overlays: usize,
}

impl PurgeRenderWindowsSummary {
    pub(crate) fn had_visual_change(self) -> bool {
        (self.had_visible_render_windows_before_purge && self.purged_windows > 0)
            || (self.had_visible_prepaint_before_purge && self.cleared_prepaint_overlays > 0)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct RecoveryNamespaceCleanupSummary {
    pub(crate) purge: PurgeRenderWindowsSummary,
    pub(crate) orphan_render_windows_closed: usize,
    pub(crate) cleared_buffer_namespaces: usize,
}

impl RecoveryNamespaceCleanupSummary {
    #[cfg(test)]
    pub(crate) fn had_visual_change(self) -> bool {
        self.purge.had_visual_change() || self.orphan_render_windows_closed > 0
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct FloatingWindowPlacement {
    pub(crate) row: i64,
    pub(crate) col: i64,
    pub(crate) width: u32,
    pub(crate) zindex: u32,
}

fn build_floating_window_config(
    placement: FloatingWindowPlacement,
    relative_to: WindowRelativeTo,
    style: WindowStyle,
    hidden: bool,
    include_noautocmd: bool,
) -> WindowConfig {
    let mut builder = WindowConfig::builder();
    builder
        .relative(relative_to)
        .row(placement.row as f64 - 1.0)
        .col(placement.col as f64 - 1.0)
        .width(placement.width.max(1))
        .height(1)
        .focusable(false)
        .style(style)
        .hide(hidden)
        .zindex(placement.zindex);
    if include_noautocmd {
        builder.noautocmd(true);
    }
    builder.build()
}

pub(crate) fn open_floating_window_config(
    placement: FloatingWindowPlacement,
    hidden: bool,
    relative_to: WindowRelativeTo,
    style: WindowStyle,
) -> WindowConfig {
    build_floating_window_config(placement, relative_to, style, hidden, true)
}

pub(crate) fn reconfigure_floating_window_config(
    placement: FloatingWindowPlacement,
    hidden: bool,
    relative_to: WindowRelativeTo,
    style: WindowStyle,
) -> WindowConfig {
    build_floating_window_config(placement, relative_to, style, hidden, false)
}

pub(crate) fn open_hidden_floating_window_config(
    zindex: u32,
    relative_to: WindowRelativeTo,
    style: WindowStyle,
) -> WindowConfig {
    let mut builder = WindowConfig::builder();
    builder
        .relative(relative_to)
        .row(0.0)
        .col(0.0)
        .width(1)
        .height(1)
        .focusable(false)
        .style(style)
        .noautocmd(true)
        .hide(true)
        .zindex(zindex);
    builder.build()
}

pub(crate) fn hide_floating_window_config() -> WindowConfig {
    let mut builder = WindowConfig::builder();
    builder.hide(true);
    builder.build()
}

pub(crate) fn set_existing_floating_window_config(
    window: &mut api::Window,
    mut config: WindowConfig,
) -> Result<()> {
    // nvim_win_set_config rejects the `noautocmd` key for existing windows.
    config.noautocmd = None;
    window.set_config(&config)?;
    Ok(())
}

pub(crate) fn set_existing_floating_window_config_ref(
    window: &mut api::Window,
    config: &WindowConfig,
) -> Result<()> {
    if config.noautocmd.is_some() {
        return set_existing_floating_window_config(window, config.clone());
    }
    window.set_config(config)?;
    Ok(())
}

pub(crate) fn initialize_floating_buffer_options(
    buffer: &api::Buffer,
    buftype: &str,
    filetype: &str,
) -> Result<()> {
    let opts = OptionOpts::builder().buf(buffer.clone()).build();
    api::set_option_value("buftype", buftype, &opts)?;
    api::set_option_value("filetype", filetype, &opts)?;
    api::set_option_value("bufhidden", "wipe", &opts)?;
    api::set_option_value("swapfile", false, &opts)?;
    Ok(())
}

pub(crate) fn initialize_floating_window_options(
    window: &api::Window,
    scope: OptionScope,
) -> Result<()> {
    let opts = OptionOpts::builder()
        .scope(scope)
        .win(window.clone())
        .build();
    api::set_option_value("winhighlight", "NormalFloat:Normal", &opts)?;
    api::set_option_value("winblend", 100_i64, &opts)?;
    Ok(())
}

#[derive(Debug)]
struct DrawContext {
    render_tabs: Mutex<HashMap<i32, Arc<Mutex<window_pool::TabWindows>>>>,
    prepaint_by_tab: Mutex<HashMap<i32, PrepaintOverlay>>,
}

impl DrawContext {
    fn new() -> Self {
        Self {
            render_tabs: Mutex::new(HashMap::with_capacity(4)),
            prepaint_by_tab: Mutex::new(HashMap::with_capacity(2)),
        }
    }
}

static DRAW_CONTEXT: LazyLock<DrawContext> = LazyLock::new(DrawContext::new);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct DrawApplyResult {
    pub(crate) metrics: ApplyMetrics,
    pub(crate) outcome: RenderOutcome,
}

fn classify_draw_outcome(metrics: &ApplyMetrics) -> RenderOutcome {
    let is_fully_applied = metrics.skipped_ops_capacity == 0
        && metrics.recovered_windows == 0
        && metrics.reuse_failed_missing_window == 0
        && metrics.reuse_failed_reconfigure == 0
        && metrics.reuse_failed_missing_buffer == 0;
    if is_fully_applied {
        RenderOutcome::AppliedFully
    } else {
        RenderOutcome::Degraded
    }
}

pub(crate) fn log_draw_error(context: &str, err: &impl std::fmt::Display) {
    apply::err_writeln(&format!("[smear_cursor][draw] {context} failed: {err}"));
}

fn render_tabs_lock()
-> std::sync::MutexGuard<'static, HashMap<i32, Arc<Mutex<window_pool::TabWindows>>>> {
    lock_with_poison_recovery(
        &DRAW_CONTEXT.render_tabs,
        |render_tabs| {
            *render_tabs = HashMap::with_capacity(4);
        },
        |_| {},
    )
}

fn prepaint_lock() -> std::sync::MutexGuard<'static, HashMap<i32, PrepaintOverlay>> {
    lock_with_poison_recovery(
        &DRAW_CONTEXT.prepaint_by_tab,
        |prepaint_by_tab| {
            *prepaint_by_tab = HashMap::with_capacity(2);
        },
        |_| {},
    )
}

fn render_tab_state(tab_handle: i32) -> Arc<Mutex<window_pool::TabWindows>> {
    let mut render_tabs = render_tabs_lock();
    Arc::clone(
        render_tabs
            .entry(tab_handle)
            .or_insert_with(|| Arc::new(Mutex::new(window_pool::TabWindows::default()))),
    )
}

fn lock_render_tab(
    render_tab: &Arc<Mutex<window_pool::TabWindows>>,
) -> std::sync::MutexGuard<'_, window_pool::TabWindows> {
    lock_with_poison_recovery(
        render_tab.as_ref(),
        |tab_windows| {
            *tab_windows = window_pool::TabWindows::default();
        },
        |_| {},
    )
}

pub(crate) fn with_render_tab<T>(
    tab_handle: i32,
    mutator: impl FnOnce(&mut window_pool::TabWindows) -> T,
) -> T {
    let render_tab = render_tab_state(tab_handle);
    let mut tab_windows = lock_render_tab(&render_tab);
    mutator(&mut tab_windows)
}

fn render_tab_states() -> Vec<(i32, Arc<Mutex<window_pool::TabWindows>>)> {
    let mut render_tabs = render_tabs_lock()
        .iter()
        .map(|(tab_handle, render_tab)| (*tab_handle, Arc::clone(render_tab)))
        .collect::<Vec<_>>();
    render_tabs.sort_unstable_by_key(|(tab_handle, _)| *tab_handle);
    render_tabs
}

fn drain_render_tab_states() -> Vec<(i32, Arc<Mutex<window_pool::TabWindows>>)> {
    let mut render_tabs = render_tabs_lock()
        .drain()
        .collect::<Vec<(i32, Arc<Mutex<window_pool::TabWindows>>)>>();
    render_tabs.sort_unstable_by_key(|(tab_handle, _)| *tab_handle);
    render_tabs
}

pub(crate) fn clear_highlight_cache() {
    palette::clear_highlight_cache();
}

pub(crate) fn ensure_palette(palette: &PaletteSpec) -> Result<()> {
    palette::ensure_highlight_palette_for_spec(palette)
}

pub(crate) fn redraw() -> Result<()> {
    apply::redraw()
}

pub(crate) fn editor_bounds() -> Result<render_plan::Viewport> {
    apply::editor_bounds()
}

pub(crate) fn draw_current(
    namespace_id: u32,
    palette: &PaletteSpec,
    projection: &ProjectionSnapshot,
    max_kept_windows: usize,
    allocation_policy: AllocationPolicy,
) -> Result<DrawApplyResult> {
    if namespace_id == 0 {
        return Ok(DrawApplyResult {
            metrics: ApplyMetrics::default(),
            outcome: RenderOutcome::AppliedFully,
        });
    }

    ensure_palette(palette)?;

    let tab_handle = apply::current_tab_handle();
    // Surprising: the old shell-local draw ack was keyed by proposal id, so it could not
    // suppress future proposals. The core realization ledger is now the only apply authority.
    let realization = realize_logical_raster(projection.logical_raster());
    let prepared_apply = apply::prepare_apply_plan(palette.color_levels(), &realization);
    let draw_result = apply::apply_plan(
        namespace_id,
        tab_handle,
        max_kept_windows,
        &prepared_apply,
        allocation_policy,
    );
    let outcome = draw_result
        .as_ref()
        .map(classify_draw_outcome)
        .unwrap_or(RenderOutcome::Failed);

    draw_result.map(|metrics| DrawApplyResult { metrics, outcome })
}

fn prepaint_open_window_config(placement: PrepaintPlacement, hidden: bool) -> WindowConfig {
    open_floating_window_config(
        FloatingWindowPlacement {
            row: placement.cell.row(),
            col: placement.cell.col(),
            width: 1,
            zindex: placement.zindex,
        },
        hidden,
        WindowRelativeTo::Editor,
        WindowStyle::Minimal,
    )
}

fn prepaint_reconfigure_window_config(placement: PrepaintPlacement, hidden: bool) -> WindowConfig {
    reconfigure_floating_window_config(
        FloatingWindowPlacement {
            row: placement.cell.row(),
            col: placement.cell.col(),
            width: 1,
            zindex: placement.zindex,
        },
        hidden,
        WindowRelativeTo::Editor,
        WindowStyle::Minimal,
    )
}

fn hidden_prepaint_window_config() -> WindowConfig {
    hide_floating_window_config()
}

fn initialize_prepaint_buffer_options(buffer: &api::Buffer) -> Result<()> {
    initialize_floating_buffer_options(buffer, "nofile", PREPAINT_BUFFER_FILETYPE)
}

fn initialize_prepaint_window_options(window: &api::Window) -> Result<()> {
    initialize_floating_window_options(window, OptionScope::Local)
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

fn hide_prepaint_overlay(namespace_id: u32, overlay: &mut PrepaintOverlay) -> bool {
    if overlay.placement.is_none() {
        return true;
    }

    let Some(mut buffer) = handles::valid_buffer(i64::from(overlay.buffer_id)) else {
        return false;
    };
    let Some(mut window) = handles::valid_window(i64::from(overlay.window_id)) else {
        return false;
    };

    // Surprising: some UIs can keep showing the last composed float contents until a later
    // repaint even after the window is hidden. Blank the prepaint payload first so the reused
    // overlay does not leave a stale cursor block behind.
    if let Err(err) = buffer.clear_namespace(namespace_id, 0..) {
        log_draw_error("clear prepaint overlay namespace before hide", &err);
        return false;
    }

    if let Err(err) =
        set_existing_floating_window_config(&mut window, hidden_prepaint_window_config())
    {
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
    let mut prepaint_by_tab = prepaint_lock();
    let requested_placement = PrepaintPlacement { cell, zindex };

    let previous = prepaint_by_tab.remove(&tab_handle);
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
        && let Err(err) = set_existing_floating_window_config(
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
        prepaint_by_tab.insert(tab_handle, entry);
    }
    Ok(())
}

pub(crate) fn clear_prepaint_for_current_tab(namespace_id: u32) {
    if namespace_id == 0 {
        return;
    }

    let tab_handle = apply::current_tab_handle();
    let mut prepaint_by_tab = prepaint_lock();
    let Some(entry) = prepaint_by_tab.get_mut(&tab_handle) else {
        return;
    };
    if !hide_prepaint_overlay(namespace_id, entry) {
        let _ = prepaint_by_tab.remove(&tab_handle);
    }
}

fn clear_all_prepaint_locked(
    prepaint_by_tab: &mut HashMap<i32, PrepaintOverlay>,
    namespace_id: u32,
) -> (usize, bool) {
    let prepaint_by_tab = std::mem::take(prepaint_by_tab);
    let had_visible_overlays = prepaint_by_tab
        .values()
        .any(|overlay| overlay.placement.is_some());
    let cleared_prepaint_overlays = prepaint_by_tab.len();
    for overlay in prepaint_by_tab.values().copied() {
        close_prepaint_overlay(namespace_id, overlay);
    }
    (cleared_prepaint_overlays, had_visible_overlays)
}

fn summarize_tracked_purge_state(
    render_tabs: &[(i32, Arc<Mutex<window_pool::TabWindows>>)],
    prepaint_by_tab: &HashMap<i32, PrepaintOverlay>,
) -> PurgeRenderWindowsSummary {
    let mut summary = PurgeRenderWindowsSummary {
        had_visible_prepaint_before_purge: prepaint_by_tab
            .values()
            .any(|overlay| overlay.placement.is_some()),
        cleared_prepaint_overlays: prepaint_by_tab.len(),
        ..PurgeRenderWindowsSummary::default()
    };

    for (_tab_handle, render_tab) in render_tabs {
        let tab_windows = lock_render_tab(render_tab);
        summary.had_visible_render_windows_before_purge = summary
            .had_visible_render_windows_before_purge
            || window_pool::tab_has_visible_windows(&tab_windows);
        summary.purged_windows = summary
            .purged_windows
            .saturating_add(window_pool::tab_pool_snapshot_from_tab(&tab_windows).total_windows);
    }

    summary
}

pub(crate) fn clear_active_render_windows(
    namespace_id: u32,
    max_kept_windows: usize,
) -> ClearActiveRenderWindowsSummary {
    let mut summary = ClearActiveRenderWindowsSummary::default();

    for (_tab_handle, render_tab) in render_tab_states() {
        let tab_summary = {
            let mut tab_windows = lock_render_tab(&render_tab);
            let had_visible_windows_before_clear =
                window_pool::tab_has_visible_windows(&tab_windows);
            if !window_pool::tab_has_pending_clear_work(&tab_windows, max_kept_windows) {
                ClearActiveRenderWindowsSummary {
                    had_visible_windows_before_clear,
                    ..ClearActiveRenderWindowsSummary::default()
                }
            } else {
                window_pool::begin_cleanup_frame(&mut tab_windows);
                let pruned_windows =
                    window_pool::prune_tab(&mut tab_windows, namespace_id, max_kept_windows);
                // Surprising: some frontends can keep compositing a just-hidden float until later UI
                // activity. Cleanup must therefore close shell-visible smear windows authoritatively instead
                // of only transitioning pooled lifecycle state to hidden.
                let closed_windows = window_pool::close_visible_tab(&mut tab_windows, namespace_id);
                let release_summary =
                    window_pool::release_unused_in_tab(&mut tab_windows, namespace_id);
                ClearActiveRenderWindowsSummary {
                    had_visible_windows_before_clear,
                    pruned_windows: pruned_windows.saturating_add(closed_windows),
                    hidden_windows: release_summary.hidden_windows,
                    invalid_removed_windows: release_summary.invalid_removed_windows,
                }
            }
        };
        summary.had_visible_windows_before_clear = summary.had_visible_windows_before_clear
            || tab_summary.had_visible_windows_before_clear;
        summary.pruned_windows = summary
            .pruned_windows
            .saturating_add(tab_summary.pruned_windows);
        summary.hidden_windows = summary
            .hidden_windows
            .saturating_add(tab_summary.hidden_windows);
        summary.invalid_removed_windows = summary
            .invalid_removed_windows
            .saturating_add(tab_summary.invalid_removed_windows);
    }

    summary
}

pub(crate) fn purge_render_windows(namespace_id: u32) -> PurgeRenderWindowsSummary {
    let summary = {
        let prepaint_by_tab = prepaint_lock();
        let render_tabs = render_tab_states();
        summarize_tracked_purge_state(render_tabs.as_slice(), &prepaint_by_tab)
    };
    {
        let mut prepaint_by_tab = prepaint_lock();
        let _ = clear_all_prepaint_locked(&mut prepaint_by_tab, namespace_id);
    }

    for (_tab_handle, render_tab) in drain_render_tab_states() {
        let mut tab_windows = lock_render_tab(&render_tab);
        window_pool::purge_tab(&mut tab_windows, namespace_id);
    }
    summary
}

fn summarize_recovery_namespace_cleanup(
    purge: PurgeRenderWindowsSummary,
    orphan_render_windows_closed: usize,
    cleared_buffer_namespaces: usize,
) -> RecoveryNamespaceCleanupSummary {
    RecoveryNamespaceCleanupSummary {
        purge,
        orphan_render_windows_closed,
        cleared_buffer_namespaces,
    }
}

pub(crate) fn recover_all_namespaces(namespace_id: u32) -> RecoveryNamespaceCleanupSummary {
    let purge = purge_render_windows(namespace_id);
    let orphan_render_windows_closed = window_pool::close_orphan_render_windows(namespace_id);
    let cleared_buffer_namespaces = apply::clear_namespace_all_buffers(namespace_id);
    summarize_recovery_namespace_cleanup(
        purge,
        orphan_render_windows_closed,
        cleared_buffer_namespaces,
    )
}

#[cfg(test)]
mod tests {
    use super::{
        ApplyMetrics, PrepaintOverlay, PurgeRenderWindowsSummary, RecoveryNamespaceCleanupSummary,
        RenderOutcome, classify_draw_outcome, clear_active_render_windows,
        clear_all_prepaint_locked, drain_render_tab_states, lock_render_tab, prepaint_lock,
        render_tab_states, summarize_recovery_namespace_cleanup, summarize_tracked_purge_state,
        with_render_tab,
    };
    use crate::core::types::StrokeId;
    use crate::draw::window_pool::WindowBufferHandle;
    use crate::types::{
        BASE_TIME_INTERVAL, Point, RenderFrame, RenderStepSample, StaticRenderConfig,
    };
    use std::sync::{Arc, LazyLock, Mutex};

    static DRAW_TEST_MUTEX: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    fn reset_draw_context_for_test() {
        prepaint_lock().clear();
        let _ = drain_render_tab_states();
    }

    fn base_frame() -> RenderFrame {
        let corners = [
            Point {
                row: 10.0,
                col: 10.0,
            },
            Point {
                row: 10.0,
                col: 11.0,
            },
            Point {
                row: 11.0,
                col: 11.0,
            },
            Point {
                row: 11.0,
                col: 10.0,
            },
        ];
        RenderFrame {
            mode: "n".to_string(),
            corners,
            step_samples: vec![RenderStepSample::new(corners, BASE_TIME_INTERVAL)],
            planner_idle_steps: 0,
            target: Point {
                row: 10.0,
                col: 10.0,
            },
            target_corners: corners,
            vertical_bar: false,
            trail_stroke_id: StrokeId::new(1),
            retarget_epoch: 0,
            particles: Vec::new(),
            color_at_cursor: None,
            static_config: Arc::new(StaticRenderConfig {
                cursor_color: None,
                cursor_color_insert_mode: None,
                normal_bg: None,
                transparent_bg_fallback_color: "#303030".to_string(),
                cterm_cursor_colors: None,
                cterm_bg: None,
                hide_target_hack: false,
                max_kept_windows: 32,
                never_draw_over_target: false,
                particle_max_lifetime: 1.0,
                particle_switch_octant_braille: 0.3,
                particles_over_text: true,
                color_levels: 16,
                gamma: 2.2,
                block_aspect_ratio: crate::config::DEFAULT_BLOCK_ASPECT_RATIO,
                tail_duration_ms: 180.0,
                simulation_hz: 120.0,
                trail_thickness: 1.0,
                trail_thickness_x: 1.0,
                spatial_coherence_weight: 1.0,
                temporal_stability_weight: 0.12,
                top_k_per_cell: 5,
                windows_zindex: 200,
            }),
        }
    }

    #[test]
    fn classify_draw_outcome_marks_clean_metrics_as_fully_applied() {
        let metrics = ApplyMetrics {
            planned_ops: 2,
            applied_ops: 2,
            ..ApplyMetrics::default()
        };
        assert_eq!(classify_draw_outcome(&metrics), RenderOutcome::AppliedFully);
    }

    #[test]
    fn classify_draw_outcome_marks_capacity_skips_as_degraded() {
        let metrics = ApplyMetrics {
            skipped_ops_capacity: 1,
            ..ApplyMetrics::default()
        };
        assert_eq!(classify_draw_outcome(&metrics), RenderOutcome::Degraded);
    }

    #[test]
    fn classify_draw_outcome_marks_reuse_failures_as_degraded() {
        let metrics = ApplyMetrics {
            reuse_failed_reconfigure: 1,
            ..ApplyMetrics::default()
        };
        assert_eq!(classify_draw_outcome(&metrics), RenderOutcome::Degraded);
    }

    #[test]
    fn trail_stroke_change_changes_draw_signature() {
        let mut baseline = base_frame();
        baseline.retarget_epoch = 10;
        let baseline_signature = super::render_plan::frame_draw_signature(&baseline);

        let mut retargeted = baseline;
        retargeted.trail_stroke_id = StrokeId::new(2);
        let retargeted_signature = super::render_plan::frame_draw_signature(&retargeted);

        assert_ne!(baseline_signature, retargeted_signature);
    }

    #[test]
    fn render_tab_tracking_is_isolated_by_tab_handle() {
        let _guard = DRAW_TEST_MUTEX
            .lock()
            .expect("draw test mutex should not be poisoned");
        reset_draw_context_for_test();

        with_render_tab(11, |tab_windows| tab_windows.cache_payload(91, 111));
        with_render_tab(22, |tab_windows| tab_windows.cache_payload(91, 222));

        assert!(with_render_tab(11, |tab_windows| tab_windows
            .cached_payload_matches(91, 111)));
        assert!(!with_render_tab(11, |tab_windows| tab_windows
            .cached_payload_matches(91, 222)));
        assert!(with_render_tab(22, |tab_windows| tab_windows
            .cached_payload_matches(91, 222)));
        assert_eq!(
            render_tab_states()
                .into_iter()
                .map(|(tab_handle, _)| tab_handle)
                .collect::<Vec<_>>(),
            vec![11, 22]
        );

        reset_draw_context_for_test();
    }

    #[test]
    fn draining_render_tab_tracking_preserves_tab_owned_state_before_registry_clear() {
        let _guard = DRAW_TEST_MUTEX
            .lock()
            .expect("draw test mutex should not be poisoned");
        reset_draw_context_for_test();

        with_render_tab(9, |tab_windows| tab_windows.cache_payload(41, 401));
        with_render_tab(3, |tab_windows| tab_windows.cache_payload(42, 402));

        let drained = drain_render_tab_states();
        let drained_handles = drained
            .iter()
            .map(|(tab_handle, _)| *tab_handle)
            .collect::<Vec<_>>();
        assert_eq!(drained_handles, vec![3, 9]);

        let drained_payloads = drained
            .iter()
            .map(|(tab_handle, render_tab)| {
                let tab_windows = lock_render_tab(render_tab);
                let cached_payload = match *tab_handle {
                    3 => tab_windows.cached_payload_matches(42, 402),
                    9 => tab_windows.cached_payload_matches(41, 401),
                    other => panic!("unexpected tab handle in drained render tabs: {other}"),
                };
                (*tab_handle, cached_payload)
            })
            .collect::<Vec<_>>();
        assert_eq!(drained_payloads, vec![(3, true), (9, true)]);
        assert!(render_tab_states().is_empty());

        reset_draw_context_for_test();
    }

    #[test]
    fn clearing_all_prepaint_tracking_does_not_touch_render_tab_state() {
        let _guard = DRAW_TEST_MUTEX
            .lock()
            .expect("draw test mutex should not be poisoned");
        reset_draw_context_for_test();

        with_render_tab(17, |tab_windows| tab_windows.cache_payload(77, 707));
        {
            let mut prepaint_by_tab = prepaint_lock();
            prepaint_by_tab.insert(
                17,
                PrepaintOverlay {
                    window_id: -1,
                    buffer_id: -1,
                    placement: None,
                },
            );
            prepaint_by_tab.insert(
                23,
                PrepaintOverlay {
                    window_id: -2,
                    buffer_id: -2,
                    placement: None,
                },
            );
            clear_all_prepaint_locked(&mut prepaint_by_tab, 99);
            assert!(prepaint_by_tab.is_empty());
        }

        assert!(with_render_tab(17, |tab_windows| tab_windows
            .cached_payload_matches(77, 707)));
        assert_eq!(render_tab_states().len(), 1);

        reset_draw_context_for_test();
    }

    #[test]
    fn clear_active_render_windows_is_noop_without_tracked_state() {
        let _guard = DRAW_TEST_MUTEX
            .lock()
            .expect("draw test mutex should not be poisoned");
        reset_draw_context_for_test();

        assert_eq!(
            clear_active_render_windows(99, 32),
            super::ClearActiveRenderWindowsSummary::default()
        );

        reset_draw_context_for_test();
    }

    #[test]
    fn tracked_purge_summary_reads_visible_render_and_prepaint_state() {
        let _guard = DRAW_TEST_MUTEX
            .lock()
            .expect("draw test mutex should not be poisoned");
        reset_draw_context_for_test();

        with_render_tab(17, |tab_windows| {
            tab_windows.push_test_visible_window(
                WindowBufferHandle {
                    window_id: -17,
                    buffer_id: -117,
                },
                super::window_pool::WindowPlacement {
                    row: 3,
                    col: 4,
                    width: 2,
                    zindex: 120,
                },
                7,
            );
        });
        {
            let mut prepaint_by_tab = prepaint_lock();
            prepaint_by_tab.insert(
                17,
                PrepaintOverlay {
                    window_id: -19,
                    buffer_id: -119,
                    placement: Some(super::PrepaintPlacement {
                        cell: crate::types::ScreenCell::new(3, 4)
                            .expect("test prepaint cell should be in bounds"),
                        zindex: 120,
                    }),
                },
            );
        }

        let render_tabs = render_tab_states();
        let prepaint_by_tab = prepaint_lock();
        let summary = summarize_tracked_purge_state(render_tabs.as_slice(), &prepaint_by_tab);
        drop(prepaint_by_tab);

        assert_eq!(
            summary,
            PurgeRenderWindowsSummary {
                had_visible_render_windows_before_purge: true,
                had_visible_prepaint_before_purge: true,
                purged_windows: 1,
                cleared_prepaint_overlays: 1,
            }
        );
        assert!(summary.had_visual_change());

        reset_draw_context_for_test();
    }

    #[test]
    fn recovery_namespace_cleanup_summary_keeps_global_sweeps_explicit() {
        let summary =
            summarize_recovery_namespace_cleanup(PurgeRenderWindowsSummary::default(), 2, 5);

        assert_eq!(
            summary,
            RecoveryNamespaceCleanupSummary {
                purge: PurgeRenderWindowsSummary::default(),
                orphan_render_windows_closed: 2,
                cleared_buffer_namespaces: 5,
            }
        );
        assert!(summary.had_visual_change());
    }
}
