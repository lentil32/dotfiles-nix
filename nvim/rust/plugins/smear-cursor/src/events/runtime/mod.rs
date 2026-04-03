use super::EngineAccessError;

mod diagnostics;
mod editor_viewport;
mod effects;
mod engine;
mod ingress_snapshot;
mod telemetry;
mod timers;

pub(crate) use editor_viewport::EditorViewport;
pub(super) use editor_viewport::EditorViewportCache;
pub(crate) use ingress_snapshot::IngressReadSnapshot;
#[cfg(test)]
pub(crate) use ingress_snapshot::IngressReadSnapshotTestInput;

pub(super) type EngineAccessResult<T> = std::result::Result<T, EngineAccessError>;

pub(super) use diagnostics::diagnostics_report;
pub(super) use diagnostics::reset_transient_event_state;
pub(super) use diagnostics::validation_counters_report;
pub(super) use effects::EffectExecutor;
pub(super) use effects::NeovimEffectExecutor;
pub(crate) use effects::record_effect_failure;
pub(super) use engine::cached_conceal_delta;
pub(super) use engine::cached_conceal_regions;
pub(super) use engine::cached_conceal_screen_cell;
pub(super) use engine::cached_cursor_color_sample_for_probe;
pub(super) use engine::cached_cursor_text_context;
#[cfg(test)]
pub(super) use engine::core_state;
pub(super) use engine::cursor_color_cache_generation;
pub(super) use engine::cursor_color_colorscheme_generation;
pub(crate) use engine::editor_viewport_for_bounds;
pub(crate) use engine::editor_viewport_for_command_row;
pub(super) use engine::ingress_read_snapshot;
pub(super) use engine::mutate_engine_state;
pub(super) use engine::note_conceal_read_boundary;
pub(super) use engine::note_cursor_color_colorscheme_change;
pub(super) use engine::note_cursor_color_observation_boundary;
pub(super) use engine::read_engine_state;
pub(super) use engine::refresh_editor_viewport_cache;
pub(super) use engine::resolved_current_buffer_event_policy;
#[cfg(test)]
pub(super) use engine::set_core_state;
pub(super) use engine::store_conceal_delta;
pub(super) use engine::store_conceal_regions;
pub(super) use engine::store_conceal_screen_cell;
pub(super) use engine::store_cursor_color_sample;
pub(super) use engine::store_cursor_text_context;
pub(super) use telemetry::clear_autocmd_event_timestamp;
pub(super) use telemetry::clear_cursor_callback_duration_estimate;
pub(super) use telemetry::clear_observation_request_timestamp;
pub(super) use telemetry::cursor_callback_duration_estimate_ms;
pub(super) use telemetry::note_autocmd_event_now;
pub(super) use telemetry::record_buffer_metadata_read;
pub(crate) use telemetry::record_command_row_read;
pub(super) use telemetry::record_conceal_full_scan;
pub(super) use telemetry::record_conceal_raw_screenpos_fallback;
pub(super) use telemetry::record_conceal_region_cache_hit;
pub(super) use telemetry::record_conceal_region_cache_miss;
pub(super) use telemetry::record_conceal_screen_cell_cache_hit;
pub(super) use telemetry::record_conceal_screen_cell_cache_miss;
pub(super) use telemetry::record_current_buffer_changedtick_read;
pub(super) use telemetry::record_cursor_callback_duration;
pub(super) use telemetry::record_cursor_color_cache_hit;
pub(super) use telemetry::record_cursor_color_cache_miss;
pub(super) use telemetry::record_cursor_color_probe_reuse;
pub(super) use telemetry::record_degraded_draw_application;
pub(super) use telemetry::record_delayed_ingress_pending_update;
pub(super) use telemetry::record_delayed_ingress_pending_update_count;
pub(crate) use telemetry::record_editor_bounds_read;
pub(super) use telemetry::record_ingress_applied;
pub(super) use telemetry::record_ingress_coalesced;
pub(super) use telemetry::record_ingress_coalesced_count;
pub(super) use telemetry::record_ingress_dropped;
pub(super) use telemetry::record_ingress_received;
pub(crate) use telemetry::record_particle_aggregation;
pub(crate) use telemetry::record_particle_overlay_refresh;
pub(crate) use telemetry::record_particle_simulation_step;
pub(crate) use telemetry::record_planner_candidate_cells_built_count;
pub(crate) use telemetry::record_planner_candidate_query_cells_count;
pub(crate) use telemetry::record_planner_compiled_cells_emitted_count;
pub(crate) use telemetry::record_planner_compiled_query_cells_count;
pub(crate) use telemetry::record_planner_local_query;
pub(crate) use telemetry::record_planner_local_query_compile;
pub(crate) use telemetry::record_planner_local_query_envelope_area_cells;
pub(crate) use telemetry::record_planner_reference_compile;
pub(super) use telemetry::record_post_burst_convergence;
pub(super) use telemetry::record_probe_extmark_fallback;
pub(super) use telemetry::record_probe_refresh_budget_exhausted_count;
pub(super) use telemetry::record_probe_refresh_retried_count;
pub(super) use telemetry::record_scheduled_drain_items;
pub(super) use telemetry::record_scheduled_drain_items_for_thermal;
pub(super) use telemetry::record_scheduled_drain_reschedule;
pub(super) use telemetry::record_scheduled_drain_reschedule_for_thermal;
pub(super) use telemetry::record_scheduled_queue_depth;
pub(super) use telemetry::record_scheduled_queue_depth_for_thermal;
pub(super) use telemetry::record_stale_token_event_count;
pub(super) use timers::dispatch_shell_timer_fired;
pub(super) use timers::now_ms;
pub(super) use timers::to_core_millis;

#[cfg(test)]
use super::event_loop::reset_for_test as reset_event_loop_for_test;
#[cfg(test)]
use diagnostics::perf_diagnostics_report as test_perf_diagnostics_report;
#[cfg(test)]
use diagnostics::validation_counters_report as test_validation_counters_report;
#[cfg(test)]
use engine::resolve_buffer_event_policy_for_metadata;
#[cfg(test)]
use timers::CoreTimerHandle;
#[cfg(test)]
use timers::CoreTimerHandles;

#[cfg(test)]
mod tests;
