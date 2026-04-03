use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PlannerCompileModeOverride {
    Auto,
    Reference,
    LocalQuery,
}

impl PlannerCompileModeOverride {
    fn parse(raw: &str) -> Option<Self> {
        match raw {
            "auto" => Some(Self::Auto),
            "reference" => Some(Self::Reference),
            "local_query" => Some(Self::LocalQuery),
            _ => None,
        }
    }
}

fn planner_compile_mode_override() -> PlannerCompileModeOverride {
    static OVERRIDE: std::sync::OnceLock<PlannerCompileModeOverride> = std::sync::OnceLock::new();

    *OVERRIDE.get_or_init(|| {
        // PERF: the headless planner harness uses this env override to compare
        // full-field compilation against the local-query path without shipping a
        // public config knob.
        std::env::var("SMEAR_PLANNER_COMPILE_MODE")
            .ok()
            .as_deref()
            .and_then(PlannerCompileModeOverride::parse)
            .unwrap_or(PlannerCompileModeOverride::Auto)
    })
}

fn query_bounds_for_compile_mode(
    compile_mode: PlannerCompileModeOverride,
    centerline: &[CenterSample],
    previous_cells: &std::collections::BTreeMap<(i64, i64), DecodedCellState>,
    frame: &RenderFrame,
) -> Option<SliceSearchBounds> {
    match compile_mode {
        PlannerCompileModeOverride::Auto | PlannerCompileModeOverride::LocalQuery => {
            fast_path_query_bounds(centerline, previous_cells, frame, PREVIOUS_CELL_HALO_CELLS)
        }
        PlannerCompileModeOverride::Reference => None,
    }
}

pub(super) fn compile_render_frame(
    frame: &RenderFrame,
    state: PlannerState,
) -> CompiledPlannerFrame {
    let mut next_state = state;
    stage_deposited_samples(&mut next_state, frame);
    populate_resampled_centerline_with_scratch(
        &next_state.center_history,
        RIBBON_SAMPLE_SPACING_CELLS,
        frame.block_aspect_ratio,
        &mut next_state.decode_scratch,
    );
    let query_bounds = query_bounds_for_compile_mode(
        planner_compile_mode_override(),
        &next_state.decode_scratch.centerline,
        &next_state.previous_cells,
        frame,
    );
    let compiled = compiled_field_for_state(&mut next_state, query_bounds);

    CompiledPlannerFrame {
        next_state,
        compiled,
        query_bounds,
    }
}

fn ensure_latent_cache_current(state: &mut PlannerState) {
    if state.latent_cache.latest_step() != state.step_index {
        debug_assert!(
            state.latent_cache.latest_step().value() <= state.step_index.value(),
            "latent cache should only advance forward with planner state"
        );
        state.latent_cache.advance_to(state.step_index);
    }
}

#[cfg(test)]
pub(super) fn compile_field_reference_for_state(
    state: &mut PlannerState,
) -> std::collections::BTreeMap<(i64, i64), CompiledCell> {
    ensure_latent_cache_current(state);
    latent_field::compile_field_reference(&state.latent_cache)
}

#[cfg(test)]
pub(super) fn compile_render_frame_reference(
    frame: &RenderFrame,
    state: PlannerState,
) -> std::collections::BTreeMap<(i64, i64), CompiledCell> {
    let mut next_state = state;
    stage_deposited_samples(&mut next_state, frame);
    compile_field_reference_for_state(&mut next_state)
}

pub(super) fn compiled_field_for_state(
    state: &mut PlannerState,
    query_bounds: Option<SliceSearchBounds>,
) -> std::sync::Arc<CompiledField> {
    ensure_latent_cache_current(state);

    let cache = &state.compiled_cache;
    let cache_is_current = cache.latest_step == Some(state.step_index)
        && cache.latent_revision == state.latent_cache.revision()
        && cache.query_bounds == query_bounds;
    if cache_is_current {
        return std::sync::Arc::clone(&cache.field);
    }

    let compiled = std::sync::Arc::new(match query_bounds {
        Some(bounds) => {
            crate::events::record_planner_local_query_compile();
            CompiledField::Rows(latent_field::compile_field_in_bounds_rows_with_scratch(
                &state.latent_cache,
                bounds,
                &mut state.compiled_cache.scratch,
            ))
        }
        None => {
            crate::events::record_planner_reference_compile();
            CompiledField::Reference(latent_field::compile_field_reference_with_scratch(
                &state.latent_cache,
                &mut state.compiled_cache.scratch,
            ))
        }
    });
    crate::events::record_planner_compiled_cells_emitted_count(compiled.len());
    let scratch = std::mem::take(&mut state.compiled_cache.scratch);
    state.compiled_cache = CompiledFieldCache {
        latest_step: Some(state.step_index),
        latent_revision: state.latent_cache.revision(),
        query_bounds,
        field: std::sync::Arc::clone(&compiled),
        scratch,
    };
    compiled
}

#[cfg(test)]
mod tests {
    use super::PlannerCompileModeOverride;
    use pretty_assertions::assert_eq;

    #[test]
    fn planner_compile_mode_override_parse_accepts_known_modes() {
        assert_eq!(
            PlannerCompileModeOverride::parse("auto"),
            Some(PlannerCompileModeOverride::Auto)
        );
        assert_eq!(
            PlannerCompileModeOverride::parse("reference"),
            Some(PlannerCompileModeOverride::Reference)
        );
        assert_eq!(
            PlannerCompileModeOverride::parse("local_query"),
            Some(PlannerCompileModeOverride::LocalQuery)
        );
    }

    #[test]
    fn planner_compile_mode_override_parse_rejects_unknown_modes() {
        assert_eq!(PlannerCompileModeOverride::parse("bogus"), None);
    }
}
