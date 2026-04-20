use crate::core::realization::PaletteSpec;
use crate::core::state::ShellProjection;
use nvim_oxi::Result;

mod apply;
mod cleanup;
mod constants;
mod context;
mod floating_windows;
mod palette;
mod prepaint;
pub(crate) mod render_plan;
mod resource_guard;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;
mod window_pool;

pub(crate) use apply::ApplyMetrics;
pub(crate) use apply::editor_bounds;
pub(crate) use apply::redraw;
pub(crate) use apply::refresh_redraw_capability as initialize_runtime_capabilities;
pub(crate) use cleanup::ClearActiveRenderWindowsSummary;
pub(crate) use cleanup::PurgeRenderWindowsSummary;
pub(crate) use cleanup::clear_active_render_windows;
pub(crate) use cleanup::compact_render_windows;
pub(crate) use cleanup::purge_render_windows;
pub(crate) use cleanup::recover_all_namespaces;
pub(crate) use constants::BRAILLE_CODE_MAX;
pub(crate) use constants::BRAILLE_CODE_MIN;
pub(crate) use constants::OCTANT_CODE_MAX;
pub(crate) use constants::OCTANT_CODE_MIN;
pub(crate) use constants::PARTICLE_ZINDEX_OFFSET;
pub(crate) use constants::PREPAINT_BUFFER_FILETYPE;
pub(crate) use constants::PREPAINT_BUFFER_TYPE;
pub(crate) use context::log_draw_error;
pub(crate) use context::render_pool_diagnostics;
pub(crate) use floating_windows::FloatingWindowPlacement;
pub(crate) use floating_windows::clear_namespace_and_hide_floating_window;
pub(crate) use floating_windows::delete_floating_buffer;
pub(crate) use floating_windows::initialize_floating_buffer_options;
pub(crate) use floating_windows::initialize_floating_window_options;
pub(crate) use floating_windows::open_hidden_floating_window_config;
pub(crate) use floating_windows::reconfigure_floating_window_config;
pub(crate) use floating_windows::set_existing_floating_window_config;
pub(crate) use palette::clear_highlight_cache;
pub(crate) use palette::ensure_highlight_palette_for_spec as ensure_palette;
pub(crate) use prepaint::ClearPrepaintOverlaysSummary;
pub(crate) use prepaint::clear_all_prepaint_overlays;
pub(crate) use prepaint::clear_prepaint_for_current_tab;
pub(crate) use prepaint::prepaint_cursor_cell;
pub(crate) use resource_guard::StagedFloatingWindow;
pub(crate) use window_pool::AllocationPolicy;
pub(crate) use window_pool::CompactRenderWindowsSummary;
#[cfg(test)]
pub(crate) use window_pool::TabPoolSnapshot;

pub(crate) fn draw_current(
    namespace_id: u32,
    palette: &PaletteSpec,
    projection: ShellProjection<'_>,
    max_kept_windows: usize,
    allocation_policy: AllocationPolicy,
) -> Result<ApplyMetrics> {
    if namespace_id == 0 {
        return Ok(ApplyMetrics::default());
    }

    ensure_palette(palette)?;

    let tab_handle = apply::current_tab_handle();
    let prepared_apply =
        apply::prepare_apply_plan(palette.color_levels(), projection.realization());
    apply::apply_plan(
        namespace_id,
        tab_handle,
        max_kept_windows,
        &prepared_apply,
        allocation_policy,
    )
}
