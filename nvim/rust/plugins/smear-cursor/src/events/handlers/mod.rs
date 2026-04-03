mod core_dispatch;
mod ingress_router;
mod labels;
mod observation;
mod render_apply;
mod render_bridge;
mod render_plan;
mod source_selection;
mod viewport;

// Handler pipeline:
// 1) ingress_router maps raw ingress -> typed core events.
// 2) core_dispatch runs deterministic reducer transitions and stages shell-effect batches.
// 3) observation collects a coherent shell snapshot for core-owned planning.
// 4) render_plan computes a typed plan artifact for a specific proposal token.
// 5) render_bridge realizes state-owned proposals and applies draw side effects.
#[cfg(test)]
pub(super) use crate::core::runtime_reducer::EventSource;
pub(super) use core_dispatch::dispatch_core_event_with_default_scheduler;
pub(super) use core_dispatch::reset_scheduled_effect_queue;
pub(super) use core_dispatch::stage_core_event_with_default_scheduler;
pub(crate) use ingress_router::on_autocmd_event;
pub(super) use observation::execute_core_request_observation_base_effect;
pub(super) use observation::execute_core_request_probe_effect;
pub(super) use observation::execute_core_request_probe_effect_same_reducer_wave;
pub(super) use render_apply::apply_ingress_cursor_presentation_effect;
pub(super) use render_apply::execute_core_apply_render_cleanup_effect;
pub(super) use render_apply::execute_redraw_cmdline_effect;
pub(super) use render_bridge::execute_core_apply_proposal_effect;
pub(super) use render_plan::execute_core_request_render_plan_effect;
#[cfg(test)]
pub(super) use source_selection::select_core_event_source;
#[cfg(test)]
pub(super) use source_selection::should_request_observation_for_autocmd;

#[cfg(test)]
mod tests;
