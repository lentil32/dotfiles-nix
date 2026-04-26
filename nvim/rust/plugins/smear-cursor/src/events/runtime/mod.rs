use super::RuntimeAccessError;

mod cell;
mod diagnostics;
mod diagnostics_lane;
mod dispatch_queue;
mod editor_viewport;
mod effects;
mod engine;
mod host_capabilities;
mod ingress_snapshot;
mod recovery;
mod shell;
mod telemetry;
mod timer_bridge;
mod timers;

pub(super) use editor_viewport::EditorViewportCache;
pub(crate) use editor_viewport::EditorViewportSnapshot;
pub(crate) use ingress_snapshot::IngressReadSnapshot;
#[cfg(test)]
pub(crate) use ingress_snapshot::IngressReadSnapshotTestInput;

pub(super) type RuntimeAccessResult<T> = std::result::Result<T, RuntimeAccessError>;

#[cfg(test)]
pub(crate) use cell::clear_runtime_draw_context_for_test;
pub(crate) use cell::flush_redraw_capability;
pub(super) use cell::read_event_loop_state;
pub(crate) use cell::restore_draw_prepaint_by_tab;
pub(crate) use cell::restore_draw_render_tabs;
#[cfg(test)]
pub(crate) use cell::runtime_render_tab_handles_for_test;
pub(crate) use cell::set_flush_redraw_capability;
pub(super) use cell::set_runtime_log_level;
pub(super) use cell::should_runtime_log;
pub(crate) use cell::take_draw_prepaint_by_tab;
pub(crate) use cell::take_draw_render_tabs;
pub(crate) use cell::tracked_runtime_draw_tab_handles;
pub(super) use cell::with_dispatch_queue;
pub(super) use cell::with_event_loop_state;
#[cfg(test)]
pub(super) use cell::with_event_loop_state_for_test;
pub(super) use cell::with_runtime_log_file_handle;
pub(crate) use cell::with_runtime_palette_lane;
pub(super) use diagnostics::diagnostics_report;
pub(super) use diagnostics::reset_transient_event_state;
pub(super) use diagnostics::validation_counters_report;
pub(super) use dispatch_queue::MAX_CHAINED_SCHEDULED_WORK_ITEMS_PER_EDGE;
pub(super) use dispatch_queue::MAX_SCHEDULED_WORK_ITEMS_PER_EDGE;
pub(super) use dispatch_queue::MIN_SCHEDULED_WORK_ITEMS_PER_EDGE;
pub(super) use dispatch_queue::PendingMetricEffects;
pub(super) use dispatch_queue::ScheduledEffectDrainEntry;
pub(super) use dispatch_queue::ScheduledEffectQueueState;
pub(super) use dispatch_queue::ScheduledWorkItem;
pub(super) use dispatch_queue::ScheduledWorkUnit;
pub(super) use dispatch_queue::ShellOnlyStep;
pub(super) use effects::EffectExecutor;
pub(super) use effects::NeovimEffectExecutor;
pub(crate) use effects::record_effect_failure;
pub(super) use engine::apply_core_setup_options;
#[cfg(test)]
pub(super) use engine::core_state;
pub(super) use engine::disable_core_runtime;
#[cfg(not(test))]
pub(super) use engine::ingress_read_snapshot;
pub(super) use engine::ingress_read_snapshot_with_current_buffer;
#[cfg(test)]
pub(super) use engine::set_core_state;
pub(super) use engine::sync_core_runtime_to_current_cursor;
pub(super) use engine::toggle_core_runtime;
pub(super) use engine::with_core_read;
pub(super) use engine::with_core_transition;
pub(crate) use host_capabilities::FlushRedrawCapability;
pub(super) use shell::advance_buffer_text_revision;
pub(super) use shell::buffer_text_revision;
pub(super) use shell::cached_conceal_delta;
pub(super) use shell::cached_conceal_regions;
pub(super) use shell::cached_conceal_screen_cell;
pub(super) use shell::cached_cursor_color_sample_for_probe;
pub(super) use shell::cached_cursor_text_context;
pub(super) use shell::clear_real_cursor_visibility;
pub(super) use shell::cursor_color_cache_generation;
pub(super) use shell::cursor_color_colorscheme_generation;
pub(crate) use shell::editor_viewport_for_bounds;
pub(crate) use shell::editor_viewport_for_command_row;
pub(super) use shell::host_bridge_state;
pub(super) use shell::invalidate_buffer_local_caches;
pub(super) use shell::invalidate_buffer_local_probe_caches;
pub(super) use shell::invalidate_buffer_metadata;
pub(super) use shell::invalidate_conceal_probe_caches;
#[cfg(test)]
pub(in crate::events) use shell::mutate_shell_state;
pub(super) use shell::namespace_id;
pub(super) use shell::note_conceal_read_boundary;
pub(super) use shell::note_cursor_color_colorscheme_change;
pub(super) use shell::note_cursor_color_observation_boundary;
pub(super) use shell::note_host_bridge_verified;
pub(super) use shell::note_real_cursor_visibility;
#[cfg(test)]
pub(in crate::events) use shell::read_shell_state;
pub(super) use shell::real_cursor_visibility_matches;
pub(super) use shell::reclaim_background_probe_request_scratch;
pub(super) use shell::reclaim_conceal_regions_scratch;
pub(super) use shell::refresh_editor_viewport_cache;
pub(super) use shell::release_cleanup_cold_shell_storage;
#[cfg(test)]
pub(super) use shell::reset_transient_shell_caches;
#[cfg(test)]
use shell::resolve_buffer_event_policy_for_metadata;
pub(super) use shell::resolved_current_buffer_event_policy;
pub(super) use shell::set_namespace_id;
pub(super) use shell::store_conceal_delta;
pub(super) use shell::store_conceal_regions;
pub(super) use shell::store_conceal_screen_cell;
pub(super) use shell::store_cursor_color_sample;
pub(super) use shell::store_cursor_text_context;
pub(super) use shell::take_background_probe_request_scratch;
pub(super) use shell::take_conceal_regions_scratch;
#[cfg(test)]
pub(super) use telemetry::clear_cursor_callback_duration_estimate;
pub(super) use telemetry::cursor_callback_duration_estimate_ms;
pub(super) use telemetry::note_autocmd_event_now;
pub(super) use telemetry::record_buffer_metadata_read;
pub(crate) use telemetry::record_compiled_field_cache_hit;
pub(crate) use telemetry::record_compiled_field_cache_miss;
pub(super) use telemetry::record_conceal_deferred_projection;
pub(super) use telemetry::record_conceal_full_scan;
pub(super) use telemetry::record_conceal_region_cache_hit;
pub(super) use telemetry::record_conceal_region_cache_miss;
pub(super) use telemetry::record_conceal_screen_cell_cache_hit;
pub(super) use telemetry::record_conceal_screen_cell_cache_miss;
pub(super) use telemetry::record_cursor_autocmd_fast_path_continued;
pub(super) use telemetry::record_cursor_autocmd_fast_path_dropped;
pub(super) use telemetry::record_cursor_callback_duration;
pub(super) use telemetry::record_cursor_color_cache_hit;
pub(super) use telemetry::record_cursor_color_cache_miss;
pub(super) use telemetry::record_cursor_color_probe_reuse;
pub(super) use telemetry::record_degraded_draw_application;
pub(super) use telemetry::record_delayed_ingress_pending_update;
pub(super) use telemetry::record_delayed_ingress_pending_update_count;
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
pub(crate) use telemetry::record_planning_preview_copied_particles;
#[cfg(feature = "perf-counters")]
pub(crate) use telemetry::record_planning_preview_copy;
pub(crate) use telemetry::record_planning_preview_invocation;
pub(super) use telemetry::record_post_burst_convergence;
pub(super) use telemetry::record_probe_extmark_fallback;
pub(super) use telemetry::record_probe_refresh_budget_exhausted_count;
pub(super) use telemetry::record_probe_refresh_retried_count;
pub(crate) use telemetry::record_projection_reuse_hit;
pub(crate) use telemetry::record_projection_reuse_miss;
pub(super) use telemetry::record_scheduled_drain_items;
pub(super) use telemetry::record_scheduled_drain_items_for_thermal;
pub(super) use telemetry::record_scheduled_drain_reschedule;
pub(super) use telemetry::record_scheduled_drain_reschedule_for_thermal;
pub(super) use telemetry::record_scheduled_queue_depth;
pub(super) use telemetry::record_scheduled_queue_depth_for_thermal;
pub(super) use telemetry::record_stale_token_event_count;
#[cfg(test)]
pub(super) use timer_bridge::CoreTimerHandle;
pub(super) use timers::dispatch_core_timer_fired;
pub(super) use timers::now_ms;
pub(super) use timers::to_core_millis;

#[cfg(test)]
use super::event_loop::reset_for_test as reset_event_loop_for_test;
#[cfg(test)]
use diagnostics::perf_diagnostics_report as test_perf_diagnostics_report;
#[cfg(test)]
use diagnostics::validation_counters_report as test_validation_counters_report;

#[cfg(test)]
mod tests;
