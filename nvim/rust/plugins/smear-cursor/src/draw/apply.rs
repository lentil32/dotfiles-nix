use super::constants::EXTMARK_ID;
use super::context::log_draw_error;
use super::context::with_render_tab;
use super::palette::HighlightGroupNames;
use super::palette::highlight_group_names;
use super::render_plan::HighlightRef;
use super::window_pool::AcquireError;
use super::window_pool::AcquireKind;
use super::window_pool::AcquiredWindow;
use super::window_pool::AllocationPolicy;
use super::window_pool::TabPoolSnapshot;
use super::window_pool::WindowPlacement;
use super::window_pool::{self};
use crate::config::normalize_color_levels;
use crate::core::realization::RealizationProjection;
use crate::core::realization::RealizationSpan;
use crate::events::editor_viewport_for_bounds;
use crate::position::ViewportBounds;
use nvim_oxi::Array;
use nvim_oxi::Dictionary;
use nvim_oxi::Object;
use nvim_oxi::Result;
use nvim_oxi::api;
use nvim_oxi::api::opts::SetExtmarkOpts;
use nvim_oxi::api::types::ExtmarkVirtTextPosition;
use std::cell::Cell;

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
    pub(crate) hidden_windows: usize,
    pub(crate) invalid_removed_windows: usize,
    pub(crate) recovered_windows: usize,
    pub(crate) pool_snapshot: Option<TabPoolSnapshot>,
}

impl ApplyMetrics {
    pub(crate) const fn requires_shell_redraw(self) -> bool {
        self.hidden_windows > 0 || self.invalid_removed_windows > 0
    }
}

pub(crate) fn editor_bounds() -> Result<ViewportBounds> {
    let viewport = editor_viewport_for_bounds()?;
    viewport.bounds().ok_or_else(|| {
        api::Error::Other("editor viewport snapshot produced invalid viewport bounds".into()).into()
    })
}

pub(crate) fn current_tab_handle() -> i32 {
    api::get_current_tabpage().handle()
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum FlushRedrawCapability {
    #[default]
    Unknown,
    ApiAvailable,
    FallbackOnly,
}

thread_local! {
    static FLUSH_REDRAW_CAPABILITY: Cell<FlushRedrawCapability> =
        const { Cell::new(FlushRedrawCapability::Unknown) };
}

fn set_flush_redraw_capability(capability: FlushRedrawCapability) {
    FLUSH_REDRAW_CAPABILITY.with(|slot| slot.set(capability));
}

fn flush_redraw_capability() -> FlushRedrawCapability {
    FLUSH_REDRAW_CAPABILITY.with(Cell::get)
}

fn flush_redraw_capability_from_exists_result(exists_result: i64) -> FlushRedrawCapability {
    if exists_result > 0 {
        FlushRedrawCapability::ApiAvailable
    } else {
        FlushRedrawCapability::FallbackOnly
    }
}

fn flush_redraw_via_api() -> Result<()> {
    let mut opts = Dictionary::new();
    opts.insert("cursor", true);
    opts.insert("valid", true);
    opts.insert("flush", true);
    let _: Object = api::call_function("nvim__redraw", Array::from_iter([Object::from(opts)]))?;
    Ok(())
}

pub(crate) fn refresh_redraw_capability() -> Result<()> {
    let exists_result: i64 =
        api::call_function("exists", Array::from_iter([Object::from("*nvim__redraw")]))?;
    set_flush_redraw_capability(flush_redraw_capability_from_exists_result(exists_result));
    Ok(())
}

pub(crate) fn redraw() -> Result<()> {
    let capability = match flush_redraw_capability() {
        FlushRedrawCapability::Unknown => {
            refresh_redraw_capability()?;
            flush_redraw_capability()
        }
        known => known,
    };

    if matches!(capability, FlushRedrawCapability::ApiAvailable) {
        match flush_redraw_via_api() {
            Ok(()) => return Ok(()),
            Err(_) => set_flush_redraw_capability(FlushRedrawCapability::FallbackOnly),
        }
    }

    Ok(api::command("redraw!")?)
}

pub(crate) fn clear_namespace_in_buffer(buffer: &mut api::Buffer, namespace_id: u32) -> bool {
    match buffer.clear_namespace(namespace_id, 0..) {
        Ok(()) => true,
        Err(err) => {
            log_draw_error("clear render namespace", &err);
            false
        }
    }
}

pub(crate) fn clear_namespace_all_buffers(namespace_id: u32) -> usize {
    let mut cleared_buffers = 0_usize;
    for mut buffer in api::list_bufs() {
        if buffer.is_valid() && clear_namespace_in_buffer(&mut buffer, namespace_id) {
            cleared_buffers = cleared_buffers.saturating_add(1);
        }
    }
    cleared_buffers
}

fn highlight_group(group_names: &HighlightGroupNames, reference: HighlightRef) -> &str {
    match reference {
        HighlightRef::Normal(level) => group_names.normal_name(level),
    }
}

#[derive(Debug)]
pub(crate) struct PreparedApplyPlan<'a> {
    group_names: HighlightGroupNames,
    planned_ops: usize,
    clears_existing_frame: bool,
    projection: &'a RealizationProjection,
}

impl PreparedApplyPlan<'_> {
    fn group_names(&self) -> &HighlightGroupNames {
        &self.group_names
    }

    fn planned_ops(&self) -> usize {
        self.planned_ops
    }

    fn clears_existing_frame(&self) -> bool {
        self.clears_existing_frame
    }

    fn spans(&self) -> impl Iterator<Item = &RealizationSpan> + Clone {
        self.projection.spans()
    }
}

pub(crate) fn prepare_apply_plan<'a>(
    color_levels: u32,
    projection: &'a RealizationProjection,
) -> PreparedApplyPlan<'a> {
    let group_names = highlight_group_names(normalize_color_levels(color_levels));

    PreparedApplyPlan {
        group_names,
        planned_ops: projection.span_count(),
        clears_existing_frame: projection.clear().is_some(),
        projection,
    }
}

fn acquire_window_for_span(
    tab_windows: &mut window_pool::TabWindows,
    namespace_id: u32,
    placement: WindowPlacement,
    allocation_policy: AllocationPolicy,
    metrics: &mut ApplyMetrics,
) -> Result<Option<AcquiredWindow>> {
    let acquired =
        window_pool::acquire_in_tab(tab_windows, namespace_id, placement, allocation_policy);

    match acquired {
        Ok(acquired) => Ok(Some(acquired)),
        Err(AcquireError::Exhausted {
            allocation_policy: AllocationPolicy::ReuseOnly,
        }) => {
            // Frame-start prewarm owns reuse-only capacity growth. Keep the span loop allocation
            // free so missed capacity invariants degrade by dropping spans instead of adding a
            // second synchronous create/open window spike to the hot path.
            metrics.skipped_ops_capacity = metrics.skipped_ops_capacity.saturating_add(1);
            Ok(None)
        }
        Err(AcquireError::Exhausted {
            allocation_policy: AllocationPolicy::BootstrapIfPoolEmpty,
        }) => Err(nvim_oxi::Error::Api(nvim_oxi::api::Error::Other(
            "render window acquire exhausted after frame-start bootstrap prewarm".into(),
        ))),
    }
}

fn mark_span_satisfied(metrics: &mut ApplyMetrics) {
    metrics.applied_ops = metrics.applied_ops.saturating_add(1);
}

enum SpanApplyDecision {
    Dropped,
    AlreadySatisfied,
    Apply {
        window_id: i32,
        buffer: api::Buffer,
        payload_hash: u64,
    },
}

fn draw_span(
    namespace_id: u32,
    tab_handle: i32,
    allocation_policy: AllocationPolicy,
    group_names: &HighlightGroupNames,
    metrics: &mut ApplyMetrics,
    span: &RealizationSpan,
) -> Result<()> {
    if span.width() == 0 || span.chunks().is_empty() {
        return Ok(());
    }

    let placement = WindowPlacement {
        row: span.row(),
        col: span.col(),
        width: span.width(),
        zindex: span.zindex(),
    };
    let payload_hash = span.payload_hash();
    let decision = with_render_tab(tab_handle, |tab_windows| -> Result<SpanApplyDecision> {
        let Some(acquired) = acquire_window_for_span(
            tab_windows,
            namespace_id,
            placement,
            allocation_policy,
            metrics,
        )?
        else {
            return Ok(SpanApplyDecision::Dropped);
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

        if tab_windows.cached_payload_matches(acquired.window_id, payload_hash) {
            return Ok(SpanApplyDecision::AlreadySatisfied);
        }

        Ok(SpanApplyDecision::Apply {
            window_id: acquired.window_id,
            buffer: acquired.buffer,
            payload_hash,
        })
    })?;

    let (window_id, mut buffer, payload_hash) = match decision {
        SpanApplyDecision::Apply {
            window_id,
            buffer,
            payload_hash,
        } => (window_id, buffer, payload_hash),
        SpanApplyDecision::AlreadySatisfied => {
            // cached payload reuse already satisfies the planned span. Counting it as
            // unapplied pushes the shell into a false degraded-apply recovery loop on hot reuse
            // frames, which is exactly what the `gg` trace showed.
            mark_span_satisfied(metrics);
            return Ok(());
        }
        SpanApplyDecision::Dropped => {
            return Ok(());
        }
    };

    let extmark_opts = SetExtmarkOpts::builder()
        .id(EXTMARK_ID)
        .virt_text(span.chunks().iter().map(|chunk| {
            (
                chunk.glyph().as_str(),
                highlight_group(group_names, chunk.highlight()),
            )
        }))
        .virt_text_pos(ExtmarkVirtTextPosition::Overlay)
        .virt_text_win_col(0)
        .build();

    // the window pool reservation already lives in draw state. Apply the extmark outside
    // the global draw mutex so one long frame does not block unrelated prepaint/cleanup bookkeeping.
    if let Err(err) = buffer.set_extmark(namespace_id, 0, 0, &extmark_opts) {
        let recovered = with_render_tab(tab_handle, |tab_windows| {
            window_pool::recover_invalid_window_in_tab(tab_windows, namespace_id, window_id)
        });
        if recovered {
            metrics.recovered_windows = metrics.recovered_windows.saturating_add(1);
            log_draw_error("recover invalid render window", &err);
            return Ok(());
        }
        return Err(nvim_oxi::Error::Api(err));
    }

    mark_span_satisfied(metrics);
    with_render_tab(tab_handle, |tab_windows| {
        tab_windows.cache_payload(window_id, payload_hash);
    });
    Ok(())
}

fn prepare_frame_capacity(
    namespace_id: u32,
    tab_handle: i32,
    max_kept_windows: usize,
    prepared: &PreparedApplyPlan<'_>,
    allocation_policy: AllocationPolicy,
    metrics: &mut ApplyMetrics,
) -> Result<()> {
    with_render_tab(tab_handle, |tab_windows| -> Result<()> {
        if prepared.clears_existing_frame() {
            window_pool::begin_apply_frame(tab_windows, metrics.planned_ops);
        }

        let in_use_windows = window_pool::tab_in_use_window_count_from_tab(tab_windows);
        let capacity_target = window_pool::frame_capacity_target(
            in_use_windows,
            metrics.planned_ops,
            max_kept_windows,
            allocation_policy,
        );
        window_pool::record_frame_capacity_target(tab_windows, capacity_target);
        metrics.created_windows =
            metrics
                .created_windows
                .saturating_add(window_pool::ensure_capacity_in_tab(
                    tab_windows,
                    namespace_id,
                    capacity_target.target_capacity,
                    max_kept_windows,
                )?);
        Ok(())
    })
}

fn finalize_apply_metrics(namespace_id: u32, tab_handle: i32, metrics: &mut ApplyMetrics) {
    let (release_summary, pool_snapshot) = with_render_tab(tab_handle, |tab_windows| {
        (
            window_pool::release_unused_in_tab(tab_windows, namespace_id),
            Some(window_pool::tab_pool_snapshot_from_tab(tab_windows)),
        )
    });
    record_release_summary(metrics, release_summary);
    metrics.pool_snapshot = pool_snapshot;
}

pub(crate) fn apply_plan(
    namespace_id: u32,
    tab_handle: i32,
    max_kept_windows: usize,
    prepared: &PreparedApplyPlan<'_>,
    allocation_policy: AllocationPolicy,
) -> Result<ApplyMetrics> {
    let mut metrics = ApplyMetrics {
        planned_ops: prepared.planned_ops(),
        ..ApplyMetrics::default()
    };
    prepare_frame_capacity(
        namespace_id,
        tab_handle,
        max_kept_windows,
        prepared,
        allocation_policy,
        &mut metrics,
    )?;

    let apply_result = (|| -> Result<()> {
        for span in prepared.spans() {
            draw_span(
                namespace_id,
                tab_handle,
                allocation_policy,
                prepared.group_names(),
                &mut metrics,
                span,
            )?;
        }
        Ok(())
    })();

    finalize_apply_metrics(namespace_id, tab_handle, &mut metrics);
    apply_result.map(|()| metrics)
}

fn record_release_summary(
    metrics: &mut ApplyMetrics,
    release_summary: window_pool::ReleaseUnusedSummary,
) {
    metrics.hidden_windows = metrics
        .hidden_windows
        .saturating_add(release_summary.hidden_windows);
    metrics.invalid_removed_windows = metrics
        .invalid_removed_windows
        .saturating_add(release_summary.invalid_removed_windows);
}

#[cfg(test)]
mod tests {
    use super::ApplyMetrics;
    use super::FlushRedrawCapability;
    use super::flush_redraw_capability_from_exists_result;
    use super::mark_span_satisfied;
    use super::prepare_apply_plan;
    use super::record_release_summary;
    use crate::config::MAX_COLOR_LEVELS;
    use crate::core::realization::LogicalRaster;
    use crate::core::realization::realize_logical_raster;
    use crate::draw::render_plan::CellOp;
    use crate::draw::render_plan::ClearOp;
    use crate::draw::render_plan::Glyph;
    use crate::draw::render_plan::HighlightLevel;
    use crate::draw::render_plan::HighlightRef;
    use crate::draw::window_pool::ReleaseUnusedSummary;
    use std::sync::Arc;

    #[test]
    fn record_release_summary_tracks_hidden_and_invalid_removed_windows() {
        let mut metrics = ApplyMetrics::default();
        record_release_summary(
            &mut metrics,
            ReleaseUnusedSummary {
                hidden_windows: 2,
                invalid_removed_windows: 1,
            },
        );

        assert_eq!(metrics.hidden_windows, 2);
        assert_eq!(metrics.invalid_removed_windows, 1);
    }

    #[test]
    fn apply_metrics_require_shell_redraw_when_hidden_windows_change() {
        let metrics = ApplyMetrics {
            hidden_windows: 1,
            ..ApplyMetrics::default()
        };

        assert!(metrics.requires_shell_redraw());
    }

    #[test]
    fn apply_metrics_require_shell_redraw_when_invalid_windows_are_removed() {
        let metrics = ApplyMetrics {
            invalid_removed_windows: 1,
            ..ApplyMetrics::default()
        };

        assert!(metrics.requires_shell_redraw());
    }

    #[test]
    fn mark_span_satisfied_counts_payload_reuse_as_applied() {
        let mut metrics = ApplyMetrics::default();

        mark_span_satisfied(&mut metrics);
        mark_span_satisfied(&mut metrics);

        assert_eq!(metrics.applied_ops, 2);
    }

    #[test]
    fn flush_redraw_capability_maps_exists_results_to_api_or_fallback() {
        assert_eq!(
            flush_redraw_capability_from_exists_result(1),
            FlushRedrawCapability::ApiAvailable
        );
        assert_eq!(
            flush_redraw_capability_from_exists_result(0),
            FlushRedrawCapability::FallbackOnly
        );
    }

    #[test]
    fn prepare_apply_plan_materializes_read_only_span_inputs() {
        let projection = realize_logical_raster(&LogicalRaster::new(
            Some(ClearOp {
                max_kept_windows: 3,
            }),
            Arc::from(vec![
                CellOp {
                    row: 4,
                    col: 7,
                    zindex: 99,
                    glyph: Glyph::BLOCK,
                    highlight: HighlightRef::Normal(HighlightLevel::from_raw_clamped(1)),
                },
                CellOp {
                    row: 4,
                    col: 8,
                    zindex: 99,
                    glyph: Glyph::Static("x"),
                    highlight: HighlightRef::Normal(HighlightLevel::from_raw_clamped(2)),
                },
            ]),
        ));
        let prepared = prepare_apply_plan(4, &projection);
        let prepared_spans = prepared.spans().collect::<Vec<_>>();
        let projection_spans = projection.spans().collect::<Vec<_>>();

        assert_eq!(prepared.planned_ops(), 1);
        assert!(prepared.clears_existing_frame());
        assert_eq!(prepared_spans.len(), 1);
        assert_eq!(prepared_spans, projection_spans);
        assert_eq!(
            prepared_spans[0].payload_hash(),
            projection_spans[0].payload_hash()
        );
        assert_eq!(
            prepared
                .group_names()
                .normal_name(HighlightLevel::from_raw_clamped(4)),
            "SmearCursor4"
        );
    }

    #[test]
    fn prepare_apply_plan_clamps_color_levels_to_the_palette_cap() {
        let projection = realize_logical_raster(&LogicalRaster::new(None, Arc::default()));
        let prepared = prepare_apply_plan(MAX_COLOR_LEVELS.saturating_add(32), &projection);

        assert_eq!(
            prepared
                .group_names()
                .normal_name(HighlightLevel::from_raw_clamped(MAX_COLOR_LEVELS)),
            format!("SmearCursor{MAX_COLOR_LEVELS}")
        );
        assert_eq!(
            prepared
                .group_names()
                .normal_name(HighlightLevel::from_raw_clamped(
                    MAX_COLOR_LEVELS.saturating_add(1)
                )),
            format!("SmearCursor{MAX_COLOR_LEVELS}")
        );
    }
}
