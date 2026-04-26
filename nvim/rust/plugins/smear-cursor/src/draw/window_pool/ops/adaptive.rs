#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct AdaptiveBudgetState {
    ewma_demand_milli: u64,
    cached_budget: usize,
}

fn ceil_div_u64(lhs: u64, rhs: u64) -> u64 {
    if rhs == 0 {
        return 0;
    }
    lhs.div_ceil(rhs)
}

fn next_adaptive_budget(previous: AdaptiveBudgetState, frame_demand: usize) -> AdaptiveBudgetState {
    let demand_milli = u64::try_from(frame_demand)
        .unwrap_or(u64::MAX)
        .saturating_mul(ADAPTIVE_POOL_EWMA_SCALE);
    let weighted_prev = previous
        .ewma_demand_milli
        .saturating_mul(ADAPTIVE_POOL_EWMA_PREV_WEIGHT);
    let weighted_new = demand_milli.saturating_mul(ADAPTIVE_POOL_EWMA_NEW_WEIGHT);
    let denominator = ADAPTIVE_POOL_EWMA_PREV_WEIGHT.saturating_add(ADAPTIVE_POOL_EWMA_NEW_WEIGHT);
    let next_ewma = if previous.ewma_demand_milli == 0 {
        demand_milli
    } else {
        weighted_prev
            .saturating_add(weighted_new)
            .saturating_add(denominator.saturating_sub(1))
            / denominator.max(1)
    };
    let ewma_demand =
        usize::try_from(ceil_div_u64(next_ewma, ADAPTIVE_POOL_EWMA_SCALE)).unwrap_or(usize::MAX);
    let target_budget = ewma_demand
        .saturating_add(ADAPTIVE_POOL_BUDGET_MARGIN)
        .clamp(ADAPTIVE_POOL_MIN_BUDGET, ADAPTIVE_POOL_HARD_MAX_BUDGET);
    let next_budget = if target_budget >= previous.cached_budget {
        target_budget
    } else {
        previous
            .cached_budget
            .saturating_sub(ADAPTIVE_POOL_BUDGET_MARGIN)
            .max(target_budget)
            .max(ADAPTIVE_POOL_MIN_BUDGET)
    };

    AdaptiveBudgetState {
        ewma_demand_milli: next_ewma,
        cached_budget: next_budget,
    }
}

fn effective_keep_budget(adaptive_budget: usize, max_kept_windows: usize) -> usize {
    adaptive_budget.min(max_kept_windows)
}

pub(crate) fn tab_has_visible_windows(tab_windows: &TabWindows) -> bool {
    tab_windows.visible_window_count() > 0
}

pub(crate) fn tab_has_pending_clear_work(
    tab_windows: &TabWindows,
    max_kept_windows: usize,
) -> bool {
    if tab_has_visible_windows(tab_windows) {
        return true;
    }

    let keep_budget = effective_keep_budget(tab_windows.cached_budget, max_kept_windows);
    if tab_windows.windows.len() > keep_budget {
        return true;
    }

    tab_windows.has_invalid_windows()
}

#[cfg(test)]
pub(crate) fn has_pending_clear_work(
    tabs: &HashMap<TabHandle, TabWindows>,
    max_kept_windows: usize,
) -> bool {
    tabs.values()
        .any(|tab_windows| tab_has_pending_clear_work(tab_windows, max_kept_windows))
}

fn lru_prune_indices(windows: &[CachedRenderWindow], keep_count: usize) -> Vec<usize> {
    let available: Vec<(usize, FrameEpoch)> = windows
        .iter()
        .enumerate()
        .filter_map(|(index, cached)| cached.available_epoch().map(|epoch| (index, epoch)))
        .collect();
    if available.len() <= keep_count {
        return Vec::new();
    }

    if keep_count == 0 {
        let mut remove_indices: Vec<usize> =
            available.into_iter().map(|(index, _)| index).collect();
        remove_indices.sort_unstable();
        return remove_indices;
    }

    let mut keep_heap: BinaryHeap<std::cmp::Reverse<(FrameEpoch, usize)>> = BinaryHeap::new();
    for (index, epoch) in available.iter().copied() {
        if keep_heap.len() < keep_count {
            keep_heap.push(std::cmp::Reverse((epoch, index)));
            continue;
        }

        let Some(std::cmp::Reverse((oldest_kept_epoch, oldest_kept_index))) =
            keep_heap.peek().copied()
        else {
            continue;
        };
        if (epoch, index) > (oldest_kept_epoch, oldest_kept_index) {
            keep_heap.pop();
            keep_heap.push(std::cmp::Reverse((epoch, index)));
        }
    }

    let keep_indices: HashSet<usize> = keep_heap
        .into_iter()
        .map(|std::cmp::Reverse((_, index))| index)
        .collect();
    let mut remove_indices: Vec<usize> = available
        .into_iter()
        .filter_map(|(index, _)| (!keep_indices.contains(&index)).then_some(index))
        .collect();
    remove_indices.sort_unstable();
    remove_indices
}
