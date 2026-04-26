use super::ribbon_dp::merge_ribbon_assignments;
use super::ribbon_dp::solve_ribbon_dp;
use super::*;

pub(in super::super) fn ribbon_support_is_oversized(slices: &[RibbonSlice]) -> bool {
    slices
        .iter()
        .any(|slice| slice.cells.len() > RIBBON_MAX_CROSS_SECTION_CELLS)
}

fn chebyshev_cell_distance(lhs: (i64, i64), rhs: (i64, i64)) -> u64 {
    lhs.0.abs_diff(rhs.0).max(lhs.1.abs_diff(rhs.1))
}

fn ribbon_support_preserves_sparse_bridge_continuity(slices: &[RibbonSlice]) -> bool {
    if slices.len() < 2 {
        return false;
    }

    slices.windows(2).all(|pair| {
        let previous = &pair[0];
        let next = &pair[1];
        previous.cells.iter().any(|lhs| {
            next.cells
                .iter()
                .any(|rhs| chebyshev_cell_distance(lhs.coord, rhs.coord) <= 2)
        })
    })
}

#[cfg(test)]
pub(in super::super) struct DecodedFieldTrace {
    pub(in super::super) cells: BTreeMap<(i64, i64), DecodedCellState>,
    pub(in super::super) path: DecodePathTrace,
}

pub(in super::super) fn select_decode_path(
    baseline: &BTreeMap<(i64, i64), DecodedCellState>,
    slices: &[RibbonSlice],
    spatial_weight_q10: u32,
) -> DecodePathTrace {
    if spatial_weight_q10 == 0 || baseline.is_empty() {
        return DecodePathTrace::Baseline;
    }
    if active_support_is_disconnected(baseline)
        && !ribbon_support_preserves_sparse_bridge_continuity(slices)
    {
        // sparse separator/empty cells can drop out of the local baseline entirely while
        // the ribbon slices still encode one coherent bridge corridor. Keep those cases on the DP
        // path so the comet stays legible across undecodable gaps instead of snapping to fallback.
        return DecodePathTrace::PairwiseFallbackDisconnected;
    }
    if slices.len() < 2 {
        return DecodePathTrace::Baseline;
    }
    if ribbon_support_is_oversized(slices) {
        return DecodePathTrace::PairwiseFallbackOversized;
    }
    DecodePathTrace::RibbonDp
}

fn spatial_distance(previous: Option<DecodedCellState>, next: Option<DecodedCellState>) -> u64 {
    u64::from(temporal_transition_distance(previous, next))
}

pub(in super::super) fn active_support_is_disconnected(
    active_cells: &BTreeMap<(i64, i64), DecodedCellState>,
) -> bool {
    if active_cells.len() > FALLBACK_COMPONENT_THRESHOLD {
        return true;
    }
    if active_cells.len() < 3 {
        return false;
    }

    let mut unvisited = active_cells
        .keys()
        .copied()
        .collect::<BTreeSet<(i64, i64)>>();
    let neighbors = |coord: (i64, i64)| -> [(i64, i64); 8] {
        [
            (coord.0 - 1, coord.1 - 1),
            (coord.0 - 1, coord.1),
            (coord.0 - 1, coord.1 + 1),
            (coord.0, coord.1 - 1),
            (coord.0, coord.1 + 1),
            (coord.0 + 1, coord.1 - 1),
            (coord.0 + 1, coord.1),
            (coord.0 + 1, coord.1 + 1),
        ]
    };

    let mut component_count = 0_usize;
    while let Some(seed) = unvisited.iter().next().copied() {
        component_count += 1;
        let mut queue = VecDeque::from([seed]);
        let _ = unvisited.remove(&seed);
        while let Some(coord) = queue.pop_front() {
            for neighbor in neighbors(coord) {
                if unvisited.remove(&neighbor) {
                    queue.push_back(neighbor);
                }
            }
        }
    }

    // straight row/column motion often decodes locally as a thick single component before
    // ribbon solve refines it. Treating every high-degree cell as "non-ribbon" sends those cases
    // down the fallback path and skips the comet taper that already works for diagonals. The
    // fallback is still reserved for truly disconnected support.
    component_count > 1
}

fn prepare_fallback_assignment(
    cell_candidates: &BTreeMap<(i64, i64), Vec<CellCandidate>>,
    scratch: &mut SolverScratch,
) {
    let SolverScratch {
        fallback_coords,
        fallback_coord_index,
        fallback_assignment,
        ..
    } = scratch;
    fallback_coords.clear();
    fallback_coords.reserve(
        cell_candidates
            .len()
            .saturating_sub(fallback_coords.capacity()),
    );
    fallback_coord_index.clear();
    fallback_coord_index.reserve(
        cell_candidates
            .len()
            .saturating_sub(fallback_coord_index.capacity()),
    );
    fallback_assignment.clear();
    fallback_assignment.reserve(
        cell_candidates
            .len()
            .saturating_sub(fallback_assignment.capacity()),
    );

    for (index, (&coord, candidates)) in cell_candidates.iter().enumerate() {
        fallback_coords.push(coord);
        fallback_coord_index.insert(coord, index);
        fallback_assignment.push(candidates.first().and_then(|candidate| candidate.state));
    }
}

pub(in super::super) fn solve_pairwise_fallback_with_scratch(
    cell_candidates: &BTreeMap<(i64, i64), Vec<CellCandidate>>,
    spatial_weight_q10: u32,
    scratch: &mut SolverScratch,
) -> BTreeMap<(i64, i64), DecodedCellState> {
    prepare_fallback_assignment(cell_candidates, scratch);
    let SolverScratch {
        fallback_coords,
        fallback_coord_index,
        fallback_assignment,
        ..
    } = scratch;

    if fallback_coords.is_empty() {
        return BTreeMap::new();
    }

    for _ in 0..FALLBACK_ITERATIONS {
        let mut changed = false;
        for (cell_index, (coord, candidates)) in cell_candidates.iter().enumerate() {
            let neighbors = [
                (coord.0 - 1, coord.1),
                (coord.0 + 1, coord.1),
                (coord.0, coord.1 - 1),
                (coord.0, coord.1 + 1),
            ];
            let mut best = CellCandidate {
                state: None,
                unary_cost: u64::MAX,
            };
            let mut best_total = u64::MAX;

            for candidate in candidates.iter().copied() {
                let pairwise = neighbors.into_iter().fold(0_u64, |acc, neighbor| {
                    let Some(&neighbor_index) = fallback_coord_index.get(&neighbor) else {
                        return acc;
                    };
                    let neighbor_state = fallback_assignment[neighbor_index];
                    let raw = spatial_distance(candidate.state, neighbor_state);
                    acc.saturating_add(scale_penalty(raw, spatial_weight_q10))
                });
                let total = candidate.unary_cost.saturating_add(pairwise);
                let should_replace = total < best_total
                    || (total == best_total
                        && state_sort_key(candidate.state) < state_sort_key(best.state));
                if should_replace {
                    best_total = total;
                    best = candidate;
                }
            }

            let current = fallback_assignment[cell_index];
            if current != best.state {
                changed = true;
                fallback_assignment[cell_index] = best.state;
            }
        }
        if !changed {
            break;
        }
    }

    fallback_coords
        .iter()
        .copied()
        .zip(fallback_assignment.iter().copied())
        .filter_map(|(coord, state)| state.map(|decoded| (coord, decoded)))
        .collect::<BTreeMap<_, _>>()
}

#[cfg(test)]
pub(in super::super) fn solve_pairwise_fallback(
    cell_candidates: &BTreeMap<(i64, i64), Vec<CellCandidate>>,
    spatial_weight_q10: u32,
) -> BTreeMap<(i64, i64), DecodedCellState> {
    let mut scratch = SolverScratch::default();
    solve_pairwise_fallback_with_scratch(cell_candidates, spatial_weight_q10, &mut scratch)
}

fn decode_compiled_field_with_solver_and_path(
    compiled: &CompiledField,
    cell_candidates: &BTreeMap<(i64, i64), Vec<CellCandidate>>,
    centerline: &[CenterSample],
    frame: &RenderFrame,
    scratch: &mut SolverScratch,
    solve_ribbon: impl FnOnce(&[RibbonSlice], u32) -> Option<Vec<SliceState>>,
) -> (BTreeMap<(i64, i64), DecodedCellState>, DecodePathTrace) {
    let mut baseline = decode_locally(cell_candidates);
    let spatial_weight_q10 = sanitize_spatial_weight_q10(frame);
    let slices = build_ribbon_slices_with_compiled_and_scratch(
        centerline,
        compiled,
        cell_candidates,
        frame,
        scratch,
    );
    match select_decode_path(&baseline, &slices, spatial_weight_q10) {
        DecodePathTrace::Baseline => return (baseline, DecodePathTrace::Baseline),
        path @ (DecodePathTrace::PairwiseFallbackDisconnected
        | DecodePathTrace::PairwiseFallbackOversized) => {
            // preserve full slice support until decode-path selection so oversized ribbon
            // support can take the explicit fallback path instead of silently collapsing to local
            // decode.
            return (
                solve_pairwise_fallback_with_scratch(cell_candidates, spatial_weight_q10, scratch),
                path,
            );
        }
        DecodePathTrace::RibbonDp => {}
        DecodePathTrace::RibbonDpSolveFailed => {}
    }

    let Some(path) = solve_ribbon(&slices, spatial_weight_q10) else {
        return (
            solve_pairwise_fallback_with_scratch(cell_candidates, spatial_weight_q10, scratch),
            DecodePathTrace::RibbonDpSolveFailed,
        );
    };
    merge_ribbon_assignments(&mut baseline, &slices, &path, scratch);
    (baseline, DecodePathTrace::RibbonDp)
}

#[cfg(test)]
pub(in super::super) fn decode_compiled_field_trace(
    cell_candidates: &BTreeMap<(i64, i64), Vec<CellCandidate>>,
    centerline: &[CenterSample],
    frame: &RenderFrame,
) -> DecodedFieldTrace {
    let compiled = CompiledField::default();
    let mut scratch = SolverScratch::default();
    decode_compiled_field_trace_with_compiled_and_scratch(
        &compiled,
        cell_candidates,
        centerline,
        frame,
        &mut scratch,
    )
}

#[cfg(test)]
pub(in super::super) fn decode_compiled_field_trace_with_compiled_and_scratch(
    compiled: &CompiledField,
    cell_candidates: &BTreeMap<(i64, i64), Vec<CellCandidate>>,
    centerline: &[CenterSample],
    frame: &RenderFrame,
    scratch: &mut SolverScratch,
) -> DecodedFieldTrace {
    let (cells, path) = decode_compiled_field_with_solver_and_path(
        compiled,
        cell_candidates,
        centerline,
        frame,
        scratch,
        solve_ribbon_dp,
    );
    DecodedFieldTrace { cells, path }
}

pub(in super::super) fn decode_compiled_field_with_compiled_and_scratch(
    compiled: &CompiledField,
    cell_candidates: &BTreeMap<(i64, i64), Vec<CellCandidate>>,
    centerline: &[CenterSample],
    frame: &RenderFrame,
    scratch: &mut SolverScratch,
) -> BTreeMap<(i64, i64), DecodedCellState> {
    decode_compiled_field_with_solver_and_path(
        compiled,
        cell_candidates,
        centerline,
        frame,
        scratch,
        solve_ribbon_dp,
    )
    .0
}

#[cfg(test)]
pub(in super::super) fn decode_compiled_field(
    cell_candidates: &BTreeMap<(i64, i64), Vec<CellCandidate>>,
    centerline: &[CenterSample],
    frame: &RenderFrame,
) -> BTreeMap<(i64, i64), DecodedCellState> {
    decode_compiled_field_with_compiled_and_scratch(
        &CompiledField::default(),
        cell_candidates,
        centerline,
        frame,
        &mut SolverScratch::default(),
    )
}
