use super::*;

pub(in super::super) fn state_for_slice_cell(
    slice: &RibbonSlice,
    state: SliceState,
    cell_index: usize,
) -> Option<DecodedCellState> {
    let candidate_index = state.candidate_offset_for(cell_index)?;
    slice.cells[cell_index]
        .non_empty_candidates
        .get(candidate_index)
        .copied()
        .and_then(|candidate| candidate.state)
}

pub(in super::super) fn cell_cost_for_state_with_peak(
    slice: &RibbonSlice,
    state: SliceState,
    cell_index: usize,
    peak_highlight_level: u32,
) -> u64 {
    let Some(candidate_index) = state.candidate_offset_for(cell_index) else {
        return slice.cells[cell_index].empty_cost;
    };
    slice.cells[cell_index]
        .non_empty_candidates
        .get(candidate_index)
        .map_or(u64::MAX / 4, |candidate| {
            adjusted_candidate_cost(slice, peak_highlight_level, *candidate)
        })
}

pub(in super::super) fn run_width_cells(slice: &RibbonSlice, state: SliceState) -> f64 {
    slice
        .run_projected_span_q16(state)
        .map_or(0.0, ProjectedSpanQ16::width_cells)
}

pub(in super::super) fn state_local_prior(
    slice: &RibbonSlice,
    state: SliceState,
    spatial_weight_q10: u32,
) -> u64 {
    let width_cells = run_width_cells(slice, state);
    let taper = squared_cells_penalty(
        (width_cells - slice.target_width_cells).abs(),
        COMET_TAPER_WEIGHT,
    );
    let tip = if slice.tail_u >= COMET_TIP_ZONE_START {
        squared_cells_penalty(
            (width_cells - slice.tip_width_cap_cells).max(0.0),
            COMET_TIP_WEIGHT,
        )
    } else {
        0
    };
    let transverse_factor_q10 = (slice.transverse_width_penalty * 1024.0)
        .round()
        .clamp(0.0, 1024.0) as u32;
    let transverse = scale_penalty(
        squared_cells_penalty(width_cells, COMET_TRANSVERSE_WEIGHT),
        transverse_factor_q10,
    );
    scale_penalty(
        taper.saturating_add(tip).saturating_add(transverse),
        spatial_weight_q10,
    )
}

pub(in super::super) fn slice_peak_highlight_level(slice: &RibbonSlice) -> u32 {
    slice
        .cells
        .iter()
        .flat_map(|cell| cell.non_empty_candidates.iter().copied())
        .filter_map(|candidate| candidate.state.map(|decoded| decoded.level.value()))
        .max()
        .unwrap_or(0)
}

fn destination_catch_penalty(
    slice: &RibbonSlice,
    peak_highlight_level: u32,
    state: Option<DecodedCellState>,
) -> u64 {
    let Some(decoded) = state else {
        return 0;
    };

    let headward_factor_q10 = (smoothstep01(1.0 - slice.tail_u) * 1024.0)
        .round()
        .clamp(0.0, 1024.0) as u32;
    let dim_delta = peak_highlight_level.saturating_sub(decoded.level.value());
    // head-side catch slices need an explicit dim-state penalty or a long bridge can
    // flatten into a uniformly dull filament whenever the brighter destination option costs a bit
    // more than continuing the tail.
    scale_penalty(
        u64::from(dim_delta).saturating_mul(CATCH_SALIENCE_DIM_PENALTY),
        headward_factor_q10,
    )
}

pub(in super::super) fn adjusted_candidate_cost(
    slice: &RibbonSlice,
    peak_highlight_level: u32,
    candidate: CellCandidate,
) -> u64 {
    candidate
        .unary_cost
        .saturating_add(destination_catch_penalty(
            slice,
            peak_highlight_level,
            candidate.state,
        ))
}

pub(in super::super) fn slice_state_cmp(lhs: SliceState, rhs: SliceState) -> std::cmp::Ordering {
    lhs.local_cost
        .cmp(&rhs.local_cost)
        .then_with(|| lhs.tie_break_key().cmp(&rhs.tie_break_key()))
}

// CONTEXT: Keep slice-state enumeration bounded to the post-truncation top-k so the ribbon solver
// never materializes the full combinatorial frontier before sorting it back down.
struct SliceStateCollector {
    states: Vec<SliceState>,
    #[cfg(test)]
    peak_len: usize,
}

impl SliceStateCollector {
    fn new(seed: SliceState) -> Self {
        Self {
            states: vec![seed],
            #[cfg(test)]
            peak_len: 1,
        }
    }

    fn insert(&mut self, state: SliceState) {
        let insert_at = self
            .states
            .partition_point(|existing| slice_state_cmp(*existing, state).is_le());
        if self.states.len() >= RIBBON_MAX_STATES_PER_SLICE && insert_at == self.states.len() {
            return;
        }

        self.states.insert(insert_at, state);
        if self.states.len() > RIBBON_MAX_STATES_PER_SLICE {
            let _ = self.states.pop();
        }
        #[cfg(test)]
        {
            self.peak_len = self.peak_len.max(self.states.len());
        }
    }

    fn should_prune_branch(&self, optimistic_cost: u64) -> bool {
        self.states.len() >= RIBBON_MAX_STATES_PER_SLICE
            && self
                .states
                .last()
                .is_some_and(|worst| optimistic_cost > worst.local_cost)
    }

    fn finish(self) -> Vec<SliceState> {
        self.states
    }

    #[cfg(test)]
    fn peak_len(&self) -> usize {
        self.peak_len
    }
}

#[derive(Clone, Copy)]
struct RunEnumerationBounds {
    remaining_empty_costs: [u64; RIBBON_MAX_RUN_LENGTH + 1],
    remaining_min_adjusted_costs: [u64; RIBBON_MAX_RUN_LENGTH + 1],
}

impl RunEnumerationBounds {
    fn try_new(slice: &RibbonSlice, run: RunSpan, peak_highlight_level: u32) -> Option<Self> {
        let mut bounds = Self {
            remaining_empty_costs: [0; RIBBON_MAX_RUN_LENGTH + 1],
            remaining_min_adjusted_costs: [0; RIBBON_MAX_RUN_LENGTH + 1],
        };
        let run_len = run.end.saturating_sub(run.start).saturating_add(1);
        for offset in (0..run_len).rev() {
            let cell = &slice.cells[run.start + offset];
            let min_adjusted_cost = cell
                .non_empty_candidates
                .iter()
                .copied()
                .map(|candidate| adjusted_candidate_cost(slice, peak_highlight_level, candidate))
                .min()?;
            bounds.remaining_empty_costs[offset] =
                bounds.remaining_empty_costs[offset + 1].saturating_add(cell.empty_cost);
            bounds.remaining_min_adjusted_costs[offset] =
                bounds.remaining_min_adjusted_costs[offset + 1].saturating_add(min_adjusted_cost);
        }
        Some(bounds)
    }

    fn optimistic_cost(self, running_cost: u64, offset: usize) -> u64 {
        running_cost
            .saturating_sub(self.remaining_empty_costs[offset])
            .saturating_add(self.remaining_min_adjusted_costs[offset])
    }
}

fn build_slice_state_collector(
    slice: &RibbonSlice,
    spatial_weight_q10: u32,
) -> SliceStateCollector {
    let baseline = slice
        .cells
        .iter()
        .fold(0_u64, |acc, cell| acc.saturating_add(cell.empty_cost));
    let peak_highlight_level = slice_peak_highlight_level(slice);
    let empty_state = SliceState::empty(0);
    let mut states = SliceStateCollector::new(SliceState::empty(
        baseline.saturating_add(state_local_prior(slice, empty_state, spatial_weight_q10)),
    ));

    for start in 0..slice.cells.len() {
        let max_end = (start + RIBBON_MAX_RUN_LENGTH).min(slice.cells.len());
        for end_exclusive in (start + 1)..=max_end {
            let end = end_exclusive - 1;
            let Some(run) = RunSpan::try_new(start, end) else {
                continue;
            };
            let Some(bounds) = RunEnumerationBounds::try_new(slice, run, peak_highlight_level)
            else {
                continue;
            };
            let context = RunEnumerationContext {
                input: RunEnumerationInput {
                    slice,
                    run,
                    spatial_weight_q10,
                    peak_highlight_level,
                },
                bounds,
            };
            enumerate_run_candidate_states(
                &context,
                RunEnumerationCursor {
                    cell_index: start,
                    running_cost: baseline,
                },
                &mut [0; RIBBON_MAX_RUN_LENGTH],
                &mut states,
            );
        }
    }

    states
}

pub(in super::super) fn build_slice_states(
    slice: &RibbonSlice,
    spatial_weight_q10: u32,
) -> Vec<SliceState> {
    build_slice_state_collector(slice, spatial_weight_q10).finish()
}

#[cfg(test)]
pub(in super::super) fn build_slice_states_with_peak_working_set(
    slice: &RibbonSlice,
    spatial_weight_q10: u32,
) -> (Vec<SliceState>, usize) {
    let collector = build_slice_state_collector(slice, spatial_weight_q10);
    let peak_len = collector.peak_len();
    (collector.finish(), peak_len)
}

#[derive(Clone, Copy)]
pub(in super::super) struct RunEnumerationInput<'a> {
    pub(in super::super) slice: &'a RibbonSlice,
    pub(in super::super) run: RunSpan,
    pub(in super::super) spatial_weight_q10: u32,
    pub(in super::super) peak_highlight_level: u32,
}

#[derive(Clone, Copy)]
struct RunEnumerationContext<'a> {
    input: RunEnumerationInput<'a>,
    bounds: RunEnumerationBounds,
}

#[derive(Clone, Copy)]
pub(in super::super) struct RunEnumerationCursor {
    pub(in super::super) cell_index: usize,
    pub(in super::super) running_cost: u64,
}

fn enumerate_run_candidate_states(
    context: &RunEnumerationContext<'_>,
    cursor: RunEnumerationCursor,
    candidate_offsets: &mut [u8; RIBBON_MAX_RUN_LENGTH],
    states: &mut SliceStateCollector,
) {
    let input = context.input;
    if cursor.cell_index > input.run.end {
        let state = SliceState::with_run(input.run, *candidate_offsets, 0);
        states.insert(SliceState::with_run(
            input.run,
            *candidate_offsets,
            cursor.running_cost.saturating_add(state_local_prior(
                input.slice,
                state,
                input.spatial_weight_q10,
            )),
        ));
        return;
    }

    let offset = cursor.cell_index - input.run.start;
    let cell = &input.slice.cells[cursor.cell_index];
    if cell.non_empty_candidates.is_empty() {
        return;
    }
    if states.should_prune_branch(context.bounds.optimistic_cost(cursor.running_cost, offset)) {
        return;
    }

    for (candidate_index, candidate) in cell.non_empty_candidates.iter().copied().enumerate() {
        let Ok(candidate_index) = u8::try_from(candidate_index) else {
            continue;
        };
        candidate_offsets[offset] = candidate_index;
        let next_cost = cursor
            .running_cost
            .saturating_sub(cell.empty_cost)
            .saturating_add(adjusted_candidate_cost(
                input.slice,
                input.peak_highlight_level,
                candidate,
            ));
        enumerate_run_candidate_states(
            context,
            RunEnumerationCursor {
                cell_index: cursor.cell_index + 1,
                running_cost: next_cost,
            },
            candidate_offsets,
            states,
        );
    }
}
