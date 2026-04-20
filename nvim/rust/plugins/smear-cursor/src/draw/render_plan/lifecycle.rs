/// Hashes the stable frame inputs that decide whether planner output can be
/// reused across draw passes.
pub(crate) fn frame_draw_signature(frame: &RenderFrame) -> Option<u64> {
    // Projection reuse already gates on policy equality and planner clock continuity.
    // The residual signature only needs the cheap dynamic inputs that can change the
    // retained trail raster when the planner itself is not advancing.
    let mut hasher = DefaultHasher::new();
    frame.mode.hash(&mut hasher);
    frame.vertical_bar.hash(&mut hasher);
    frame.trail_stroke_id.hash(&mut hasher);
    frame.retarget_epoch.hash(&mut hasher);

    hash_f64(&mut hasher, frame.target.row);
    hash_f64(&mut hasher, frame.target.col);

    for corner in &frame.corners {
        hash_f64(&mut hasher, corner.row);
        hash_f64(&mut hasher, corner.col);
    }

    Some(hasher.finish())
}

/// Hashes the particle-overlay inputs that can change independently from the
/// trail planner output.
pub(crate) fn frame_particle_overlay_signature(frame: &RenderFrame) -> Option<u64> {
    if frame.aggregated_particle_cells().is_empty() {
        return None;
    }

    // Policy equality already validates the particle rendering configuration. The
    // overlay refresh key only needs the retained shared aggregate identity so reuse
    // can avoid walking every packed sample on each projection.
    let mut hasher = DefaultHasher::new();
    std::ptr::hash(
        std::sync::Arc::as_ptr(&frame.aggregated_particle_cells),
        &mut hasher,
    );

    Some(hasher.finish())
}

/// Reserves the target cell for cursor punch-through when the render frame is
/// allowed to hide the target directly.
pub(crate) fn plan_target_cell_overlay(
    frame: &RenderFrame,
    viewport: ViewportBounds,
    shape: crate::types::CursorCellShape,
) -> Option<TargetCellOverlay> {
    if !frame.hide_target_hack {
        return None;
    }

    let row = frame.target.row.round() as i64;
    let col = frame.target.col.round() as i64;
    if row < 1 || row > viewport.max_row() || col < 1 || col > viewport.max_col() {
        return None;
    }

    Some(TargetCellOverlay {
        row,
        col,
        zindex: frame.windows_zindex,
        shape,
        level: HighlightLevel::from_raw_clamped(frame.color_levels),
    })
}

#[cfg(test)]
pub(crate) fn render_frame_to_plan(
    frame: &RenderFrame,
    state: PlannerState,
    viewport: ViewportBounds,
) -> PlannerOutput {
    render_frame_to_plan_with_signature(frame, state, viewport, frame_draw_signature(frame))
}

/// Compiles and decodes one render frame into the next planner output.
pub(crate) fn render_frame_to_plan_with_signature(
    frame: &RenderFrame,
    state: PlannerState,
    viewport: ViewportBounds,
    maybe_signature: Option<u64>,
) -> PlannerOutput {
    let compiled = compile_render_frame(frame, state);
    decode_compiled_frame(frame, compiled, viewport, maybe_signature)
}

#[cfg(test)]
pub(crate) fn particle_overlay_plan(frame: &RenderFrame, viewport: ViewportBounds) -> RenderPlan {
    let target_row = frame.target.row.round() as i64;
    let target_col = frame.target.col.round() as i64;
    let mut builder =
        PlanBuilder::with_capacity(viewport, 0, frame.aggregated_particle_cells().len());
    builder.set_punch_through_cell(target_row, target_col);

    {
        let mut resources = PlanResources {
            builder: &mut builder,
            windows_zindex: frame.windows_zindex,
            particle_zindex: frame.windows_zindex.saturating_sub(PARTICLE_ZINDEX_OFFSET),
        };

        draw_particles(&mut resources, frame, target_row, target_col);
    }

    builder.finish(None, None)
}

pub(in crate::draw::render_plan) fn decode_compiled_frame(
    frame: &RenderFrame,
    compiled_frame: CompiledPlannerFrame,
    viewport: ViewportBounds,
    maybe_signature: Option<u64>,
) -> PlannerOutput {
    let CompiledPlannerFrame {
        mut next_state,
        compiled,
        query_bounds,
    } = compiled_frame;
    let temporal_weight = sanitize_temporal_weight(frame);
    if let Some(bounds) = query_bounds {
        populate_cell_candidates_in_bounds_with_scratch(
            &compiled,
            &next_state.previous_cells,
            frame.color_levels,
            temporal_weight,
            sanitize_top_k(frame),
            bounds,
            &mut next_state.decode_scratch,
        );
    } else {
        populate_cell_candidates_with_scratch(
            &compiled,
            &next_state.previous_cells,
            frame.color_levels,
            temporal_weight,
            sanitize_top_k(frame),
            &mut next_state.decode_scratch,
        );
    }
    let next_cells = {
        let PlannerDecodeScratch {
            cell_candidates,
            centerline,
            solver,
            ..
        } = &mut next_state.decode_scratch;
        decode_compiled_field_with_compiled_and_scratch(
            &compiled,
            cell_candidates,
            centerline,
            frame,
            solver,
        )
    };

    let target_row = frame.target.row.round() as i64;
    let target_col = frame.target.col.round() as i64;

    let mut builder = PlanBuilder::with_capacity(
        viewport,
        next_cells.len(),
        frame.aggregated_particle_cells().len(),
    );
    builder.set_punch_through_cell(target_row, target_col);

    {
        let mut resources = PlanResources {
            builder: &mut builder,
            windows_zindex: frame.windows_zindex,
            particle_zindex: frame.windows_zindex.saturating_sub(PARTICLE_ZINDEX_OFFSET),
        };

        draw_particles(&mut resources, frame, target_row, target_col);

        for ((row, col), decoded) in &next_cells {
            push_decoded_cell(&mut resources, *row, *col, *decoded);
        }
    }

    next_state.previous_cells = std::sync::Arc::new(next_cells);

    let plan = builder.finish(
        Some(ClearOp {
            max_kept_windows: frame.max_kept_windows,
        }),
        None,
    );

    #[cfg(test)]
    {
        PlannerOutput {
            plan,
            next_state,
            signature: maybe_signature,
        }
    }

    #[cfg(not(test))]
    {
        let _ = maybe_signature;
        PlannerOutput { plan, next_state }
    }
}
