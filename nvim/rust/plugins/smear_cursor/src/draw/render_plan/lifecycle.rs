/// Hashes the stable frame inputs that decide whether planner output can be
/// reused across draw passes.
pub(crate) fn frame_draw_signature(frame: &RenderFrame) -> Option<u64> {
    if !frame.particles.is_empty() {
        return None;
    }

    let mut hasher = DefaultHasher::new();
    frame.mode.hash(&mut hasher);
    frame.vertical_bar.hash(&mut hasher);
    frame.trail_stroke_id.hash(&mut hasher);
    frame.retarget_epoch.hash(&mut hasher);
    frame.never_draw_over_target.hash(&mut hasher);
    frame.color_levels.hash(&mut hasher);
    frame.windows_zindex.hash(&mut hasher);
    hash_f64(&mut hasher, frame.block_aspect_ratio);
    hash_f64(&mut hasher, frame.tail_duration_ms);
    hash_f64(&mut hasher, frame.simulation_hz);
    hash_f64(&mut hasher, frame.trail_thickness);
    hash_f64(&mut hasher, frame.trail_thickness_x);
    hash_f64(&mut hasher, frame.spatial_coherence_weight);
    hash_f64(&mut hasher, frame.temporal_stability_weight);
    frame.top_k_per_cell.hash(&mut hasher);

    hash_f64(&mut hasher, frame.target.row);
    hash_f64(&mut hasher, frame.target.col);

    frame.planner_idle_steps.hash(&mut hasher);
    for corner in &frame.corners {
        hash_f64(&mut hasher, corner.row);
        hash_f64(&mut hasher, corner.col);
    }
    frame.step_samples.len().hash(&mut hasher);
    for sample in frame.step_samples.iter() {
        hash_f64(&mut hasher, sample.dt_ms);
        for corner in &sample.corners {
            hash_f64(&mut hasher, corner.row);
            hash_f64(&mut hasher, corner.col);
        }
    }

    Some(hasher.finish())
}

/// Reserves the target cell for cursor punch-through when the render frame is
/// allowed to hide the target directly.
pub(crate) fn plan_target_cell_overlay(
    frame: &RenderFrame,
    viewport: Viewport,
) -> Option<TargetCellOverlay> {
    if !frame.hide_target_hack || frame.vertical_bar {
        return None;
    }

    let row = frame.target.row.round() as i64;
    let col = frame.target.col.round() as i64;
    if row < 1 || row > viewport.max_row || col < 1 || col > viewport.max_col {
        return None;
    }

    Some(TargetCellOverlay {
        row,
        col,
        zindex: frame.windows_zindex,
        level: HighlightLevel::from_raw_clamped(frame.color_levels),
    })
}

#[cfg(test)]
pub(crate) fn render_frame_to_plan(
    frame: &RenderFrame,
    state: PlannerState,
    viewport: Viewport,
) -> PlannerOutput {
    render_frame_to_plan_with_signature(frame, state, viewport, frame_draw_signature(frame))
}

/// Compiles and decodes one render frame into the next planner output.
pub(crate) fn render_frame_to_plan_with_signature(
    frame: &RenderFrame,
    state: PlannerState,
    viewport: Viewport,
    maybe_signature: Option<u64>,
) -> PlannerOutput {
    let compiled = compile_render_frame(frame, state);
    decode_compiled_frame(frame, compiled, viewport, maybe_signature)
}

/// Compiles one render frame into the cached planner representation.
pub(crate) fn compile_render_frame(
    frame: &RenderFrame,
    state: PlannerState,
) -> CompiledPlannerFrame {
    let mut next_state = state;
    stage_deposited_samples(&mut next_state, frame);
    let compiled = compiled_field_for_state(&mut next_state);

    CompiledPlannerFrame {
        next_state,
        compiled,
    }
}

fn compiled_field_for_state(
    state: &mut PlannerState,
) -> std::sync::Arc<std::collections::BTreeMap<(i64, i64), CompiledCell>> {
    if state.latent_cache.latest_step() != state.step_index {
        debug_assert!(
            state.latent_cache.latest_step().value() <= state.step_index.value(),
            "latent cache should only advance forward with planner state"
        );
        state.latent_cache.advance_to(state.step_index);
    }

    let cache = &state.compiled_cache;
    let cache_is_current = cache.latest_step == Some(state.step_index)
        && cache.latent_revision == state.latent_cache.revision();
    if cache_is_current {
        return std::sync::Arc::clone(&cache.field);
    }

    let compiled = std::sync::Arc::new(latent_field::compile_field_from_cache_with_scratch(
        &state.latent_cache,
        &mut state.compiled_cache.scratch,
    ));
    let scratch = std::mem::take(&mut state.compiled_cache.scratch);
    state.compiled_cache = CompiledFieldCache {
        latest_step: Some(state.step_index),
        latent_revision: state.latent_cache.revision(),
        field: std::sync::Arc::clone(&compiled),
        scratch,
    };
    compiled
}

pub(crate) fn decode_compiled_frame(
    frame: &RenderFrame,
    compiled_frame: CompiledPlannerFrame,
    viewport: Viewport,
    maybe_signature: Option<u64>,
) -> PlannerOutput {
    let CompiledPlannerFrame {
        mut next_state,
        compiled,
    } = compiled_frame;
    let temporal_weight = sanitize_temporal_weight(frame);
    let cell_candidates = build_cell_candidates(
        &compiled,
        &next_state.previous_cells,
        frame.color_levels,
        temporal_weight,
        sanitize_top_k(frame),
    );
    let centerline = resample_centerline(
        &next_state.center_history,
        RIBBON_SAMPLE_SPACING_CELLS,
        frame.block_aspect_ratio,
    );
    let decoded =
        decode_compiled_field_trace_with_compiled(&compiled, &cell_candidates, &centerline, frame);
    let next_cells = decoded.cells;

    let target_row = frame.target.row.round() as i64;
    let target_col = frame.target.col.round() as i64;

    let mut builder = PlanBuilder::with_capacity(viewport, next_cells.len(), frame.particles.len());
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

    next_state.previous_cells = next_cells;

    PlannerOutput {
        plan: builder.finish(
            Some(ClearOp {
                max_kept_windows: frame.max_kept_windows,
            }),
            None,
        ),
        next_state,
        signature: maybe_signature,
    }
}
