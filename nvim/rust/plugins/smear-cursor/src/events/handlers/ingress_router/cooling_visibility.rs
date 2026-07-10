//! Current-tab visual fence while inactive cleanup remains incrementally Cooling.

use super::IngressDispatchOutcome;
use crate::core::state::RenderThermalState;
use crate::events::logging::warn;
use crate::events::runtime::namespace_id;
use crate::events::runtime::with_core_read;
use nvim_oxi::Result;

fn should_fence_current_tab(thermal: RenderThermalState) -> bool {
    thermal == RenderThermalState::Cooling
}

pub(super) fn on_current_tab_ingress() -> Result<IngressDispatchOutcome> {
    let max_kept_windows = match with_core_read(|state| {
        should_fence_current_tab(state.render_cleanup().thermal())
            .then_some(state.runtime().config.max_kept_windows)
    }) {
        Ok(Some(max_kept_windows)) => max_kept_windows,
        Ok(None) => return Ok(IngressDispatchOutcome::Dropped),
        Err(err) => {
            warn(&format!(
                "runtime lane re-entered while checking the cooling tab fence: {err}"
            ));
            return Ok(IngressDispatchOutcome::Dropped);
        }
    };
    let namespace_id = match namespace_id() {
        Ok(Some(namespace_id)) => namespace_id,
        Ok(None) => return Ok(IngressDispatchOutcome::Dropped),
        Err(err) => {
            warn(&format!(
                "runtime lane re-entered while reading the cooling tab-fence namespace: {err}"
            ));
            return Ok(IngressDispatchOutcome::Dropped);
        }
    };

    let (render, prepaint) =
        crate::draw::clear_current_tab_render_artifacts(namespace_id, max_kept_windows);
    if (render.had_visual_change() || prepaint.had_visual_change())
        && let Err(err) = crate::draw::redraw()
    {
        warn(&format!("cooling tab-fence redraw failed: {err}"));
    }
    Ok(IngressDispatchOutcome::Applied)
}

#[cfg(test)]
mod tests {
    use super::should_fence_current_tab;
    use crate::core::state::RenderThermalState;
    use pretty_assertions::assert_eq;

    #[test]
    fn current_tab_visibility_fence_runs_only_while_cleanup_is_cooling() {
        assert_eq!(
            [
                RenderThermalState::Hot,
                RenderThermalState::Cooling,
                RenderThermalState::Cold,
            ]
            .map(should_fence_current_tab),
            [false, true, false],
        );
    }
}
