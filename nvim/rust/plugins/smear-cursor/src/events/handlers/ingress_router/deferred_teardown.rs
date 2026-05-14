use super::teardown_autocmd::DeferredTeardownEffect;
use crate::draw::prune_closed_window_resources;
use crate::draw::prune_stale_tab_resources;
use crate::events::runtime::now_ms;
use crate::events::runtime::to_core_millis;
use crate::events::schedule_guarded;
use nvim_oxi::Result;

pub(super) fn schedule_deferred_teardown_effect(effect: DeferredTeardownEffect) {
    let schedule_context = match effect {
        DeferredTeardownEffect::ClosedTab { .. } => "deferred TabClosed draw resource cleanup",
        DeferredTeardownEffect::ClosedWindow { .. } => "deferred WinClosed draw resource cleanup",
    };
    schedule_guarded(schedule_context, move || {
        if let Err(err) = apply_deferred_teardown_effect(effect) {
            let error_context = match effect {
                DeferredTeardownEffect::ClosedTab { .. } => "deferred TabClosed resource cleanup",
                DeferredTeardownEffect::ClosedWindow { .. } => {
                    "deferred WinClosed resource cleanup"
                }
            };
            crate::events::warn(&format!("{error_context} failed: {err}"));
        }
    });
}

fn apply_deferred_teardown_effect(effect: DeferredTeardownEffect) -> Result<()> {
    match effect {
        DeferredTeardownEffect::ClosedTab { tab_handle } => {
            cleanup_closed_tab_resources(tab_handle)
        }
        DeferredTeardownEffect::ClosedWindow { window_id } => {
            cleanup_closed_window_resources(window_id)
        }
    }
}

fn schedule_retained_resource_cleanup_retry(retained_resources: usize) -> Result<()> {
    let Some(event) = super::super::retained_resource_cleanup_retry_event(
        retained_resources,
        to_core_millis(now_ms()),
    ) else {
        return Ok(());
    };

    super::super::dispatch_core_event_with_default_scheduler(event)
}

fn cleanup_closed_tab_resources(tab_handle: crate::host::TabHandle) -> Result<()> {
    let namespace_id =
        super::super::super::host_bridge::ensure_namespace_id().map_err(nvim_oxi::Error::from)?;
    let summary = prune_stale_tab_resources(namespace_id, &[tab_handle]);
    schedule_retained_resource_cleanup_retry(summary.retained_resources())
}

fn cleanup_closed_window_resources(window_id: i32) -> Result<()> {
    let namespace_id =
        super::super::super::host_bridge::ensure_namespace_id().map_err(nvim_oxi::Error::from)?;
    let summary = prune_closed_window_resources(namespace_id, window_id);
    schedule_retained_resource_cleanup_retry(summary.retained_resources())
}
