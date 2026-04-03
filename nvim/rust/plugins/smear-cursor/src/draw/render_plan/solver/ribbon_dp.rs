use super::ribbon_state_space::build_slice_states;
use super::ribbon_state_space::cell_cost_for_state_with_peak;
use super::ribbon_state_space::slice_peak_highlight_level;
use super::ribbon_state_space::state_for_slice_cell;
use super::*;

pub(in super::super) fn overlap_penalty(
    previous: Option<DecodedCellState>,
    next: Option<DecodedCellState>,
) -> u64 {
    match (previous, next) {
        (None, None) => 0,
        (None, Some(_)) | (Some(_), None) => PENALTY_OVERLAP_FLIP,
        (Some(prev), Some(next_state)) if prev == next_state => 0,
        (Some(prev), Some(next_state)) if prev.glyph == next_state.glyph => {
            u64::from(prev.level.abs_diff(next_state.level)).saturating_mul(90)
        }
        (Some(prev), Some(next_state)) => PENALTY_OVERLAP_SHAPE
            .saturating_add(u64::from(prev.level.abs_diff(next_state.level)).saturating_mul(120)),
    }
}

#[derive(Clone, Copy)]
struct PreparedSliceState {
    state: SliceState,
    width_cells: f64,
    center_q16: Option<i32>,
}

impl PreparedSliceState {
    fn new(slice: &RibbonSlice, state: SliceState) -> Self {
        let projected_span = slice.run_projected_span_q16(state);
        Self {
            state,
            width_cells: projected_span.map_or(0.0, ProjectedSpanQ16::width_cells),
            center_q16: projected_span.map(ProjectedSpanQ16::center_q16),
        }
    }
}

fn prepare_slice_states(slice: &RibbonSlice, spatial_weight_q10: u32) -> Vec<PreparedSliceState> {
    build_slice_states(slice, spatial_weight_q10)
        .into_iter()
        .map(|state| PreparedSliceState::new(slice, state))
        .collect::<Vec<_>>()
}

fn build_slice_overlap_pairs(
    previous_slice: &RibbonSlice,
    next_slice: &RibbonSlice,
) -> Box<[(usize, usize)]> {
    let previous_indices = previous_slice
        .cells
        .iter()
        .enumerate()
        .map(|(index, cell)| (cell.coord, index))
        .collect::<BTreeMap<_, _>>();
    let mut overlap_pairs = Vec::<(usize, usize)>::with_capacity(
        previous_slice.cells.len().min(next_slice.cells.len()),
    );

    // Match coordinates once per slice pair so the DP only walks true overlaps inside the
    // state-pair loop instead of rescanning the previous slice for every compared cell.
    for (next_index, next_cell) in next_slice.cells.iter().enumerate() {
        if let Some(&previous_index) = previous_indices.get(&next_cell.coord) {
            overlap_pairs.push((previous_index, next_index));
        }
    }

    overlap_pairs.into_boxed_slice()
}

fn transition_cost_prepared(
    previous_slice: &RibbonSlice,
    previous_state: PreparedSliceState,
    next_slice: &RibbonSlice,
    next_state: PreparedSliceState,
    overlap_pairs: &[(usize, usize)],
    spatial_weight_q10: u32,
) -> u64 {
    let mut cost = 0_u64;
    for &(previous_index, next_index) in overlap_pairs {
        let previous_value =
            state_for_slice_cell(previous_slice, previous_state.state, previous_index);
        let next_value = state_for_slice_cell(next_slice, next_state.state, next_index);
        cost = cost.saturating_add(scale_penalty(
            overlap_penalty(previous_value, next_value),
            spatial_weight_q10,
        ));
    }

    let prev_width = previous_state.width_cells;
    let next_width = next_state.width_cells;
    cost = cost.saturating_add(scale_penalty(
        linear_cells_penalty((prev_width - next_width).abs(), PENALTY_THICKNESS_DELTA),
        spatial_weight_q10,
    ));
    let (headward_len, tailward_len) = if previous_slice.tail_u <= next_slice.tail_u {
        (prev_width, next_width)
    } else {
        (next_width, prev_width)
    };
    let mono_violation = (tailward_len - headward_len - COMET_MONO_EPSILON_CELLS).max(0.0);
    cost = cost.saturating_add(scale_penalty(
        squared_cells_penalty(mono_violation, COMET_MONO_WEIGHT),
        spatial_weight_q10,
    ));

    match (previous_state.center_q16, next_state.center_q16) {
        (Some(prev_center), Some(next_center)) => {
            let shift_q16 = prev_center.abs_diff(next_center);
            let shift_penalty =
                (u64::from(shift_q16).saturating_mul(PENALTY_CENTER_SHIFT)) / Q16_SCALE_U64;
            cost = cost.saturating_add(scale_penalty(shift_penalty, spatial_weight_q10));
            if shift_q16 > ((3 * Q16_SCALE) / 2) as u32 {
                cost = cost.saturating_add(scale_penalty(PENALTY_DISCONNECT, spatial_weight_q10));
            }
        }
        (None, Some(_)) | (Some(_), None) => {
            cost = cost.saturating_add(scale_penalty(PENALTY_EMPTY_TRANSITION, spatial_weight_q10));
        }
        (None, None) => {}
    }

    cost
}

pub(in super::super) fn solve_ribbon_dp(
    slices: &[RibbonSlice],
    spatial_weight_q10: u32,
) -> Option<Vec<SliceState>> {
    if slices.len() < 2 {
        return None;
    }

    let state_sets = slices
        .iter()
        .map(|slice| prepare_slice_states(slice, spatial_weight_q10))
        .collect::<Vec<_>>();
    if state_sets.iter().any(Vec::is_empty) {
        return None;
    }
    let overlap_pairs = slices
        .windows(2)
        .map(|pair| build_slice_overlap_pairs(&pair[0], &pair[1]))
        .collect::<Vec<_>>();

    let mut costs = Vec::<Vec<u64>>::with_capacity(state_sets.len());
    let mut backpointers = Vec::<Vec<usize>>::with_capacity(state_sets.len());

    costs.push(
        state_sets[0]
            .iter()
            .map(|state| state.state.local_cost)
            .collect::<Vec<_>>(),
    );
    backpointers.push(vec![0; state_sets[0].len()]);

    for slice_index in 1..state_sets.len() {
        let current_states = &state_sets[slice_index];
        let previous_states = &state_sets[slice_index - 1];
        let overlap_pairs = &overlap_pairs[slice_index - 1];
        let mut current_costs = vec![u64::MAX; current_states.len()];
        let mut current_back = vec![0_usize; current_states.len()];

        for (current_index, current_state) in current_states.iter().copied().enumerate() {
            let mut best_prev_index = 0_usize;
            let mut best_cost = u64::MAX;
            for (previous_index, previous_state) in previous_states.iter().copied().enumerate() {
                let trans = transition_cost_prepared(
                    &slices[slice_index - 1],
                    previous_state,
                    &slices[slice_index],
                    current_state,
                    overlap_pairs,
                    spatial_weight_q10,
                );
                let candidate = costs[slice_index - 1][previous_index]
                    .saturating_add(current_state.state.local_cost)
                    .saturating_add(trans);
                let should_replace = candidate < best_cost
                    || (candidate == best_cost && previous_index < best_prev_index)
                    || (candidate == best_cost
                        && previous_index == best_prev_index
                        && previous_state.state.tie_break_key()
                            < previous_states[best_prev_index].state.tie_break_key());
                if should_replace {
                    best_cost = candidate;
                    best_prev_index = previous_index;
                }
            }
            current_costs[current_index] = best_cost;
            current_back[current_index] = best_prev_index;
        }

        costs.push(current_costs);
        backpointers.push(current_back);
    }

    let last_index = state_sets.len() - 1;
    let mut best_state_index = 0_usize;
    let mut best_total = u64::MAX;
    for (index, total_cost) in costs[last_index].iter().copied().enumerate() {
        let should_replace = total_cost < best_total
            || (total_cost == best_total
                && state_sets[last_index][index].state.tie_break_key()
                    < state_sets[last_index][best_state_index]
                        .state
                        .tie_break_key())
            || (total_cost == best_total
                && state_sets[last_index][index].state.tie_break_key()
                    == state_sets[last_index][best_state_index]
                        .state
                        .tie_break_key()
                && index < best_state_index);
        if should_replace {
            best_total = total_cost;
            best_state_index = index;
        }
    }

    let mut path = vec![SliceState::empty(0); state_sets.len()];
    let mut cursor = best_state_index;
    for slice_index in (0..state_sets.len()).rev() {
        path[slice_index] = state_sets[slice_index][cursor].state;
        if slice_index > 0 {
            cursor = backpointers[slice_index][cursor];
        }
    }
    Some(path)
}

pub(in super::super) fn merge_ribbon_assignments(
    baseline: &mut BTreeMap<(i64, i64), DecodedCellState>,
    slices: &[RibbonSlice],
    solved_states: &[SliceState],
    scratch: &mut SolverScratch,
) {
    let SolverScratch {
        vote_buckets,
        reusable_vote_lists,
        ..
    } = scratch;
    recycle_vote_lists(vote_buckets, reusable_vote_lists);

    for (slice, solved_state) in slices.iter().zip(solved_states.iter().copied()) {
        let peak_highlight_level = slice_peak_highlight_level(slice);
        for cell_index in 0..slice.cells.len() {
            let state = state_for_slice_cell(slice, solved_state, cell_index);
            let cost = cell_cost_for_state_with_peak(
                slice,
                solved_state,
                cell_index,
                peak_highlight_level,
            );
            let coord = slice.cells[cell_index].coord;
            let bucket = match vote_buckets.entry(coord) {
                std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert(reusable_vote_lists.pop().unwrap_or_default())
                }
            };
            record_state_vote(bucket, state, cost);
        }
    }

    for (&coord, per_state_votes) in vote_buckets.iter() {
        let mut best: Option<StateVote> = None;
        for vote in per_state_votes.iter().copied() {
            let should_replace = match best {
                None => true,
                Some(best_vote) => {
                    vote.stats.count > best_vote.stats.count
                        || (vote.stats.count == best_vote.stats.count
                            && vote.stats.total_cost < best_vote.stats.total_cost)
                        || (vote.stats.count == best_vote.stats.count
                            && vote.stats.total_cost == best_vote.stats.total_cost
                            && state_sort_key(vote.state) < state_sort_key(best_vote.state))
                }
            };
            if should_replace {
                best = Some(vote);
            }
        }

        if let Some(best_vote) = best {
            match best_vote.state {
                Some(decoded) => {
                    baseline.insert(coord, decoded);
                }
                None => {
                    baseline.remove(&coord);
                }
            }
        }
    }
}

fn recycle_vote_lists(
    vote_buckets: &mut std::collections::HashMap<(i64, i64), Vec<StateVote>>,
    reusable_vote_lists: &mut Vec<Vec<StateVote>>,
) {
    for (_, mut votes) in vote_buckets.drain() {
        votes.clear();
        reusable_vote_lists.push(votes);
    }
}

fn record_state_vote(votes: &mut Vec<StateVote>, state: Option<DecodedCellState>, cost: u64) {
    if let Some(existing) = votes.iter_mut().find(|vote| vote.state == state) {
        existing.stats.count = existing.stats.count.saturating_add(1);
        existing.stats.total_cost = existing.stats.total_cost.saturating_add(cost);
        return;
    }

    votes.push(StateVote {
        state,
        stats: VoteStats {
            count: 1,
            total_cost: cost,
        },
    });
}
