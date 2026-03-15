fn normalize_direction_display(
    row_delta: f64,
    col_delta: f64,
    block_aspect_ratio: f64,
) -> (f64, f64) {
    let safe_aspect = crate::types::display_metric_row_scale(block_aspect_ratio);
    let row = row_delta * safe_aspect;
    let col = col_delta;
    let length = (row * row + col * col).sqrt();
    if !length.is_finite() || length <= f64::EPSILON {
        return (0.0, 1.0);
    }
    (row / length, col / length)
}

fn to_q16(value: f64) -> i32 {
    if !value.is_finite() {
        return 0;
    }
    let scaled = (value * Q16_SCALE as f64).round();
    scaled.clamp(i32::MIN as f64, i32::MAX as f64) as i32
}

fn scale_penalty(base: u64, spatial_weight_q10: u32) -> u64 {
    base.saturating_mul(u64::from(spatial_weight_q10)) / 1024
}

fn linear_cells_penalty(delta_cells: f64, weight: u64) -> u64 {
    if !delta_cells.is_finite() || delta_cells <= 0.0 {
        return 0;
    }
    let delta_q10 = (delta_cells * WIDTH_Q10_SCALE_U64 as f64)
        .round()
        .clamp(0.0, u64::MAX as f64) as u64;
    let weighted = u128::from(delta_q10).saturating_mul(u128::from(weight));
    (weighted / u128::from(WIDTH_Q10_SCALE_U64)).min(u128::from(u64::MAX)) as u64
}

fn squared_cells_penalty(delta_cells: f64, weight: u64) -> u64 {
    if !delta_cells.is_finite() || delta_cells <= 0.0 {
        return 0;
    }
    let delta_q10 = (delta_cells * WIDTH_Q10_SCALE_U64 as f64)
        .round()
        .clamp(0.0, u64::MAX as f64) as u64;
    let weighted = u128::from(delta_q10)
        .saturating_mul(u128::from(delta_q10))
        .saturating_mul(u128::from(weight));
    let denom = u128::from(WIDTH_Q10_SCALE_U64).saturating_mul(u128::from(WIDTH_Q10_SCALE_U64));
    (weighted / denom).min(u128::from(u64::MAX)) as u64
}

fn smoothstep01(value: f64) -> f64 {
    let x = value.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

fn centerline_tail_u(sample_index: usize, sample_count: usize) -> f64 {
    if sample_count <= 1 {
        return 0.0;
    }
    // Surprising: centerline samples are oldest -> newest, so comet-tail distance from head
    // uses the reversed progress coordinate.
    let progress = sample_index as f64 / (sample_count.saturating_sub(1) as f64);
    (1.0 - progress).clamp(0.0, 1.0)
}

fn taper_progress(u: f64) -> f64 {
    if u <= COMET_NECK_FRACTION {
        return 0.0;
    }
    let v = ((u - COMET_NECK_FRACTION) / (1.0 - COMET_NECK_FRACTION)).clamp(0.0, 1.0);
    let s = smoothstep01(v);
    1.0 - (1.0 - s).powf(COMET_TAPER_EXPONENT)
}

fn filament_blend_factor(u: f64) -> f64 {
    if u <= COMET_NECK_FRACTION {
        return 0.0;
    }
    ((u - COMET_NECK_FRACTION) / (1.0 - COMET_NECK_FRACTION))
        .clamp(0.0, 1.0)
        .powf(1.6)
}

fn default_head_width_cells(frame: &RenderFrame) -> f64 {
    (slice_band_half_width(frame) * 2.0).clamp(1.0, RIBBON_MAX_RUN_LENGTH as f64)
}

fn comet_target_width_cells(head_width: f64, u: f64) -> f64 {
    let tip_width = (head_width * COMET_TIP_WIDTH_RATIO)
        .max(COMET_MIN_RESOLVABLE_WIDTH)
        .min(head_width);
    let progress = taper_progress(u);
    (head_width + (tip_width - head_width) * progress).max(COMET_MIN_RESOLVABLE_WIDTH)
}

fn centerline_curvature(centerline: &[CenterSample], index: usize, block_aspect_ratio: f64) -> f64 {
    if centerline.len() < 3 || index == 0 || index + 1 >= centerline.len() {
        return 0.0;
    }
    let prev = centerline[index - 1];
    let current = centerline[index];
    let next = centerline[index + 1];
    let dot = (prev.tangent_row * next.tangent_row + prev.tangent_col * next.tangent_col)
        .clamp(-1.0, 1.0);
    let cross = prev.tangent_row * next.tangent_col - prev.tangent_col * next.tangent_row;
    let turn_angle = cross.abs().atan2(dot).abs();
    let arc = (aspect_metric_distance(next.pos, current.pos, block_aspect_ratio)
        + aspect_metric_distance(current.pos, prev.pos, block_aspect_ratio))
    .max(1.0e-6);
    (turn_angle / arc).max(0.0)
}

fn centerline_curvature_smoothed(
    centerline: &[CenterSample],
    index: usize,
    block_aspect_ratio: f64,
) -> f64 {
    if centerline.is_empty() {
        return 0.0;
    }
    let current = centerline_curvature(centerline, index, block_aspect_ratio);
    let previous = if index > 0 {
        centerline_curvature(centerline, index - 1, block_aspect_ratio)
    } else {
        current
    };
    let next = if index + 1 < centerline.len() {
        centerline_curvature(centerline, index + 1, block_aspect_ratio)
    } else {
        current
    };

    (0.25 * previous + 0.50 * current + 0.25 * next).min(COMET_CURVATURE_KAPPA_CAP)
}

fn solve_slice_band_half_width(frame: &RenderFrame, u: f64, curvature: f64) -> f64 {
    let head_half_width = slice_band_half_width(frame);
    let filament_half_width = COMET_MIN_RESOLVABLE_WIDTH;
    let blend = filament_blend_factor(u);
    let blended = head_half_width * (1.0 - blend) + filament_half_width * blend;
    let excess = (blended - filament_half_width).max(0.0);
    let compressed_excess =
        excess / (1.0 + COMET_CURVATURE_COMPRESS_FACTOR * curvature.max(0.0) * excess);
    let compressed = filament_half_width + compressed_excess;
    compressed.clamp(filament_half_width, head_half_width)
}

fn resample_centerline(
    history: &VecDeque<CenterPathSample>,
    spacing: f64,
    block_aspect_ratio: f64,
) -> Vec<CenterSample> {
    if history.is_empty() {
        return Vec::new();
    }

    let mut points = Vec::<Point>::with_capacity(history.len());
    for sample in history {
        let should_push = points.last().is_none_or(|last| {
            aspect_metric_distance(*last, sample.pos, block_aspect_ratio) > 1.0e-3
        });
        if should_push {
            points.push(sample.pos);
        }
    }

    if points.is_empty() {
        return Vec::new();
    }
    if points.len() == 1 {
        return vec![CenterSample {
            pos: points[0],
            tangent_row: 0.0,
            tangent_col: 1.0,
        }];
    }

    let mut cumulative = Vec::<f64>::with_capacity(points.len());
    cumulative.push(0.0);
    for pair in points.windows(2) {
        let delta = aspect_metric_distance(pair[1], pair[0], block_aspect_ratio);
        let previous = cumulative.last().copied().unwrap_or(0.0);
        cumulative.push(previous + delta.max(0.0));
    }

    let total_len = cumulative.last().copied().unwrap_or(0.0);
    if total_len <= f64::EPSILON {
        return vec![CenterSample {
            pos: points[0],
            tangent_row: 0.0,
            tangent_col: 1.0,
        }];
    }

    let safe_spacing = if spacing.is_finite() {
        spacing.max(0.125)
    } else {
        RIBBON_SAMPLE_SPACING_CELLS
    };
    let sample_count = (total_len / safe_spacing).ceil() as usize + 1;

    let mut resampled = Vec::<CenterSample>::with_capacity(sample_count);
    let mut segment = 0_usize;
    for sample_index in 0..sample_count {
        let target_arc = (sample_index as f64 * safe_spacing).min(total_len);
        while segment + 2 < cumulative.len() && target_arc > cumulative[segment + 1] {
            segment = segment.saturating_add(1);
        }

        let start = points[segment];
        let end = points[segment + 1];
        let segment_start = cumulative[segment];
        let segment_end = cumulative[segment + 1];
        let segment_len = (segment_end - segment_start).max(f64::EPSILON);
        let t = ((target_arc - segment_start) / segment_len).clamp(0.0, 1.0);
        let pos = Point {
            row: start.row + (end.row - start.row) * t,
            col: start.col + (end.col - start.col) * t,
        };
        let (tangent_row, tangent_col) = normalize_direction_display(
            end.row - start.row,
            end.col - start.col,
            block_aspect_ratio,
        );
        resampled.push(CenterSample {
            pos,
            tangent_row,
            tangent_col,
        });
    }
    resampled
}

fn slice_band_half_width(frame: &RenderFrame) -> f64 {
    let thickness = frame.trail_thickness.max(frame.trail_thickness_x);
    let safe = if thickness.is_finite() {
        thickness.max(0.5)
    } else {
        1.0
    };
    (0.75 * safe + 0.5).clamp(0.75, 3.0)
}

#[derive(Clone, Copy)]
struct SliceSearchBounds {
    min_row: i64,
    max_row: i64,
    min_col: i64,
    max_col: i64,
}

fn measured_slice_support_width_cells(
    sample: CenterSample,
    normal_row: f64,
    normal_col: f64,
    band_half_width: f64,
    compiled: &BTreeMap<(i64, i64), CompiledCell>,
    frame: &RenderFrame,
    bounds: SliceSearchBounds,
) -> Option<f64> {
    if compiled.is_empty() {
        return None;
    }

    let safe_aspect = crate::types::display_metric_row_scale(frame.block_aspect_ratio);
    let sample_half_span_q16 = to_q16(
        0.5 * (normal_row.abs() * safe_aspect / MICRO_H as f64 + normal_col.abs() / MICRO_W as f64),
    );
    let mut support: Option<ProjectedSpanQ16> = None;

    for (coord, compiled_cell) in
        compiled.range((bounds.min_row, i64::MIN)..=(bounds.max_row, i64::MAX))
    {
        if coord.1 < bounds.min_col || coord.1 > bounds.max_col {
            continue;
        }

        for sample_row in 0..MICRO_H {
            for sample_col in 0..MICRO_W {
                let index = sample_row * MICRO_W + sample_col;
                if compiled_cell.tile.samples_q12[index] < MIN_VISIBLE_SAMPLE_Q12 {
                    continue;
                }

                let row_center = coord.0 as f64 + (sample_row as f64 + 0.5) / MICRO_H as f64;
                let col_center = coord.1 as f64 + (sample_col as f64 + 0.5) / MICRO_W as f64;
                let delta_row = row_center - sample.pos.row;
                let delta_col = col_center - sample.pos.col;
                let delta_row_display = delta_row * safe_aspect;
                let along = delta_row_display * sample.tangent_row + delta_col * sample.tangent_col;
                let across = delta_row_display * normal_row + delta_col * normal_col;

                if along.abs() > RIBBON_SLICE_HALF_SPAN || across.abs() > band_half_width {
                    continue;
                }

                let sample_span = ProjectedSpanQ16::from_center_and_half_span(
                    to_q16(across),
                    sample_half_span_q16,
                );
                support = Some(support.map_or(sample_span, |existing| existing.cover(sample_span)));
            }
        }
    }

    support.map(|span| {
        span.width_cells()
            .clamp(COMET_MIN_RESOLVABLE_WIDTH, RIBBON_MAX_RUN_LENGTH as f64)
    })
}

fn slice_head_width_cells(frame: &RenderFrame, measured_support_width_cells: Option<f64>) -> f64 {
    measured_support_width_cells
        .unwrap_or_else(|| default_head_width_cells(frame))
        .clamp(1.0, RIBBON_MAX_RUN_LENGTH as f64)
}

#[cfg(test)]
fn build_ribbon_slices(
    centerline: &[CenterSample],
    cell_candidates: &BTreeMap<(i64, i64), Vec<CellCandidate>>,
    frame: &RenderFrame,
) -> Vec<RibbonSlice> {
    build_ribbon_slices_with_compiled(centerline, &BTreeMap::new(), cell_candidates, frame)
}

fn build_ribbon_slices_with_compiled(
    centerline: &[CenterSample],
    compiled: &BTreeMap<(i64, i64), CompiledCell>,
    cell_candidates: &BTreeMap<(i64, i64), Vec<CellCandidate>>,
    frame: &RenderFrame,
) -> Vec<RibbonSlice> {
    if centerline.is_empty() || cell_candidates.is_empty() {
        return Vec::new();
    }

    let mut slices = Vec::<RibbonSlice>::new();
    let safe_aspect = crate::types::display_metric_row_scale(frame.block_aspect_ratio);

    for (sample_index, sample) in centerline.iter().copied().enumerate() {
        let tail_u = centerline_tail_u(sample_index, centerline.len());
        let curvature =
            centerline_curvature_smoothed(centerline, sample_index, frame.block_aspect_ratio);
        let band_half_width = solve_slice_band_half_width(frame, tail_u, curvature);
        let normal_row = -sample.tangent_col;
        let normal_col = sample.tangent_row;
        let cell_half_span_q16 = to_q16(0.5 * (normal_row.abs() * safe_aspect + normal_col.abs()));
        let mut cells = Vec::<SliceCell>::new();
        let row_display_bound =
            sample.tangent_row.abs() * RIBBON_SLICE_HALF_SPAN + normal_row.abs() * band_half_width;
        let col_bound =
            sample.tangent_col.abs() * RIBBON_SLICE_HALF_SPAN + normal_col.abs() * band_half_width;
        let row_bound = row_display_bound / safe_aspect;
        let bounds = SliceSearchBounds {
            min_row: (sample.pos.row - row_bound).floor() as i64 - 1,
            max_row: (sample.pos.row + row_bound).ceil() as i64,
            min_col: (sample.pos.col - col_bound).floor() as i64 - 1,
            max_col: (sample.pos.col + col_bound).ceil() as i64,
        };
        let measured_support_width_cells = measured_slice_support_width_cells(
            sample,
            normal_row,
            normal_col,
            band_half_width,
            compiled,
            frame,
            bounds,
        );
        let target_width_cells = comet_target_width_cells(
            slice_head_width_cells(frame, measured_support_width_cells),
            tail_u,
        );
        let tip_width_cap_cells =
            (COMET_MIN_RESOLVABLE_WIDTH * COMET_TIP_CAP_MULTIPLIER).min(target_width_cells);
        let transverse_width_penalty = filament_blend_factor(tail_u).clamp(0.0, 1.0);

        for (coord, candidates) in
            cell_candidates.range((bounds.min_row, i64::MIN)..=(bounds.max_row, i64::MAX))
        {
            if coord.1 < bounds.min_col || coord.1 > bounds.max_col {
                continue;
            }

            let row_center = coord.0 as f64 + 0.5;
            let col_center = coord.1 as f64 + 0.5;
            let delta_row = row_center - sample.pos.row;
            let delta_col = col_center - sample.pos.col;
            let delta_row_display = delta_row * safe_aspect;
            let along = delta_row_display * sample.tangent_row + delta_col * sample.tangent_col;
            let across = delta_row_display * normal_row + delta_col * normal_col;

            if along.abs() > RIBBON_SLICE_HALF_SPAN || across.abs() > band_half_width {
                continue;
            }

            let Some(empty_cost) = candidates
                .iter()
                .find_map(|candidate| (candidate.state.is_none()).then_some(candidate.unary_cost))
            else {
                continue;
            };
            let normal_q16 = to_q16(across);
            cells.push(SliceCell::new(
                *coord,
                normal_q16,
                cell_half_span_q16,
                empty_cost,
                non_empty_candidates(candidates),
            ));
        }

        if cells.is_empty() {
            continue;
        }

        cells.sort_by(|lhs, rhs| {
            lhs.normal_center_q16
                .cmp(&rhs.normal_center_q16)
                .then_with(|| lhs.coord.cmp(&rhs.coord))
        });

        let is_duplicate = slices.last().is_some_and(|slice| {
            slice.cells.len() == cells.len()
                && slice
                    .cells
                    .iter()
                    .zip(cells.iter())
                    .all(|(lhs, rhs)| lhs.coord == rhs.coord)
        });
        if is_duplicate {
            continue;
        }

        slices.push(RibbonSlice {
            cells,
            tail_u,
            target_width_cells,
            tip_width_cap_cells,
            transverse_width_penalty,
        });
    }

    slices
}

fn ribbon_support_is_oversized(slices: &[RibbonSlice]) -> bool {
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

struct DecodedField {
    cells: BTreeMap<(i64, i64), DecodedCellState>,
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "decode path trace is exercised only by render-plan tests"
        )
    )]
    path: DecodePathTrace,
}

fn select_decode_path(
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

fn state_for_slice_cell(
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

fn cell_cost_for_state(slice: &RibbonSlice, state: SliceState, cell_index: usize) -> u64 {
    let Some(candidate_index) = state.candidate_offset_for(cell_index) else {
        return slice.cells[cell_index].empty_cost;
    };
    slice.cells[cell_index]
        .non_empty_candidates
        .get(candidate_index)
        .map_or(u64::MAX / 4, |candidate| {
            adjusted_candidate_cost(slice, *candidate)
        })
}

fn run_width_cells(slice: &RibbonSlice, state: SliceState) -> f64 {
    slice
        .run_projected_span_q16(state)
        .map_or(0.0, ProjectedSpanQ16::width_cells)
}

fn state_local_prior(slice: &RibbonSlice, state: SliceState, spatial_weight_q10: u32) -> u64 {
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

fn slice_peak_highlight_level(slice: &RibbonSlice) -> u32 {
    slice
        .cells
        .iter()
        .flat_map(|cell| cell.non_empty_candidates.iter().copied())
        .filter_map(|candidate| candidate.state.map(|decoded| decoded.level.value()))
        .max()
        .unwrap_or(0)
}

fn destination_catch_penalty(slice: &RibbonSlice, state: Option<DecodedCellState>) -> u64 {
    let Some(decoded) = state else {
        return 0;
    };

    let headward_factor_q10 = (smoothstep01(1.0 - slice.tail_u) * 1024.0)
        .round()
        .clamp(0.0, 1024.0) as u32;
    let dim_delta = slice_peak_highlight_level(slice).saturating_sub(decoded.level.value());
    // head-side catch slices need an explicit dim-state penalty or a long bridge can
    // flatten into a uniformly dull filament whenever the brighter destination option costs a bit
    // more than continuing the tail.
    scale_penalty(
        u64::from(dim_delta).saturating_mul(CATCH_SALIENCE_DIM_PENALTY),
        headward_factor_q10,
    )
}

fn adjusted_candidate_cost(slice: &RibbonSlice, candidate: CellCandidate) -> u64 {
    candidate
        .unary_cost
        .saturating_add(destination_catch_penalty(slice, candidate.state))
}

fn build_slice_states(slice: &RibbonSlice, spatial_weight_q10: u32) -> Vec<SliceState> {
    let baseline = slice
        .cells
        .iter()
        .fold(0_u64, |acc, cell| acc.saturating_add(cell.empty_cost));
    let empty_state = SliceState::empty(0);
    let mut states = vec![SliceState::empty(
        baseline.saturating_add(state_local_prior(slice, empty_state, spatial_weight_q10)),
    )];

    for start in 0..slice.cells.len() {
        let max_end = (start + RIBBON_MAX_RUN_LENGTH).min(slice.cells.len());
        for end_exclusive in (start + 1)..=max_end {
            let end = end_exclusive - 1;
            let Some(run) = RunSpan::try_new(start, end) else {
                continue;
            };
            enumerate_run_candidate_states(
                slice,
                run,
                spatial_weight_q10,
                start,
                baseline,
                &mut [0; RIBBON_MAX_RUN_LENGTH],
                &mut states,
            );
        }
    }

    states.sort_by(|lhs, rhs| {
        lhs.local_cost
            .cmp(&rhs.local_cost)
            .then_with(|| lhs.tie_break_key().cmp(&rhs.tie_break_key()))
    });
    states.truncate(RIBBON_MAX_STATES_PER_SLICE);
    states
}

fn enumerate_run_candidate_states(
    slice: &RibbonSlice,
    run: RunSpan,
    spatial_weight_q10: u32,
    cell_index: usize,
    running_cost: u64,
    candidate_offsets: &mut [u8; RIBBON_MAX_RUN_LENGTH],
    states: &mut Vec<SliceState>,
) {
    if cell_index > run.end {
        let state = SliceState::with_run(run, *candidate_offsets, 0);
        states.push(SliceState::with_run(
            run,
            *candidate_offsets,
            running_cost.saturating_add(state_local_prior(slice, state, spatial_weight_q10)),
        ));
        return;
    }

    let offset = cell_index - run.start;
    let cell = &slice.cells[cell_index];
    if cell.non_empty_candidates.is_empty() {
        return;
    }

    for (candidate_index, candidate) in cell.non_empty_candidates.iter().copied().enumerate() {
        let Ok(candidate_index) = u8::try_from(candidate_index) else {
            continue;
        };
        candidate_offsets[offset] = candidate_index;
        let next_cost = running_cost
            .saturating_sub(cell.empty_cost)
            .saturating_add(adjusted_candidate_cost(slice, candidate));
        enumerate_run_candidate_states(
            slice,
            run,
            spatial_weight_q10,
            cell_index + 1,
            next_cost,
            candidate_offsets,
            states,
        );
    }
}

fn overlap_penalty(previous: Option<DecodedCellState>, next: Option<DecodedCellState>) -> u64 {
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

fn run_center_q16(slice: &RibbonSlice, state: SliceState) -> Option<i32> {
    slice
        .run_projected_span_q16(state)
        .map(ProjectedSpanQ16::center_q16)
}

fn transition_cost(
    previous_slice: &RibbonSlice,
    previous_state: SliceState,
    next_slice: &RibbonSlice,
    next_state: SliceState,
    spatial_weight_q10: u32,
) -> u64 {
    let mut cost = 0_u64;
    for (next_index, next_cell) in next_slice.cells.iter().enumerate() {
        let next_value = state_for_slice_cell(next_slice, next_state, next_index);
        if let Some(previous_index) = previous_slice
            .cells
            .iter()
            .position(|previous_cell| previous_cell.coord == next_cell.coord)
        {
            let previous_value =
                state_for_slice_cell(previous_slice, previous_state, previous_index);
            cost = cost.saturating_add(scale_penalty(
                overlap_penalty(previous_value, next_value),
                spatial_weight_q10,
            ));
        }
    }

    let prev_width = run_width_cells(previous_slice, previous_state);
    let next_width = run_width_cells(next_slice, next_state);
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

    match (
        run_center_q16(previous_slice, previous_state),
        run_center_q16(next_slice, next_state),
    ) {
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

fn solve_ribbon_dp(slices: &[RibbonSlice], spatial_weight_q10: u32) -> Option<Vec<SliceState>> {
    if slices.len() < 2 {
        return None;
    }

    let state_sets = slices
        .iter()
        .map(|slice| build_slice_states(slice, spatial_weight_q10))
        .collect::<Vec<Vec<SliceState>>>();
    if state_sets.iter().any(Vec::is_empty) {
        return None;
    }

    let mut costs = Vec::<Vec<u64>>::with_capacity(state_sets.len());
    let mut backpointers = Vec::<Vec<usize>>::with_capacity(state_sets.len());

    costs.push(
        state_sets[0]
            .iter()
            .map(|state| state.local_cost)
            .collect::<Vec<_>>(),
    );
    backpointers.push(vec![0; state_sets[0].len()]);

    for slice_index in 1..state_sets.len() {
        let current_states = &state_sets[slice_index];
        let previous_states = &state_sets[slice_index - 1];
        let mut current_costs = vec![u64::MAX; current_states.len()];
        let mut current_back = vec![0_usize; current_states.len()];

        for (current_index, current_state) in current_states.iter().copied().enumerate() {
            let mut best_prev_index = 0_usize;
            let mut best_cost = u64::MAX;
            for (previous_index, previous_state) in previous_states.iter().copied().enumerate() {
                let trans = transition_cost(
                    &slices[slice_index - 1],
                    previous_state,
                    &slices[slice_index],
                    current_state,
                    spatial_weight_q10,
                );
                let candidate = costs[slice_index - 1][previous_index]
                    .saturating_add(current_state.local_cost)
                    .saturating_add(trans);
                let should_replace = candidate < best_cost
                    || (candidate == best_cost && previous_index < best_prev_index)
                    || (candidate == best_cost
                        && previous_index == best_prev_index
                        && previous_state.tie_break_key()
                            < previous_states[best_prev_index].tie_break_key());
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
                && state_sets[last_index][index].tie_break_key()
                    < state_sets[last_index][best_state_index].tie_break_key())
            || (total_cost == best_total
                && state_sets[last_index][index].tie_break_key()
                    == state_sets[last_index][best_state_index].tie_break_key()
                && index < best_state_index);
        if should_replace {
            best_total = total_cost;
            best_state_index = index;
        }
    }

    let mut path = vec![SliceState::empty(0); state_sets.len()];
    let mut cursor = best_state_index;
    for slice_index in (0..state_sets.len()).rev() {
        path[slice_index] = state_sets[slice_index][cursor];
        if slice_index > 0 {
            cursor = backpointers[slice_index][cursor];
        }
    }
    Some(path)
}

fn merge_ribbon_assignments(
    baseline: &mut BTreeMap<(i64, i64), DecodedCellState>,
    slices: &[RibbonSlice],
    solved_states: &[SliceState],
) {
    let mut votes = BTreeMap::<(i64, i64), BTreeMap<Option<DecodedCellState>, VoteStats>>::new();

    for (slice, solved_state) in slices.iter().zip(solved_states.iter().copied()) {
        for cell_index in 0..slice.cells.len() {
            let state = state_for_slice_cell(slice, solved_state, cell_index);
            let cost = cell_cost_for_state(slice, solved_state, cell_index);
            let bucket = votes
                .entry(slice.cells[cell_index].coord)
                .or_default()
                .entry(state)
                .or_default();
            bucket.count = bucket.count.saturating_add(1);
            bucket.total_cost = bucket.total_cost.saturating_add(cost);
        }
    }

    for (coord, per_state_votes) in votes {
        let mut best: Option<(Option<DecodedCellState>, VoteStats)> = None;
        for (state, stats) in per_state_votes {
            let should_replace = match best {
                None => true,
                Some((best_state, best_stats)) => {
                    stats.count > best_stats.count
                        || (stats.count == best_stats.count
                            && stats.total_cost < best_stats.total_cost)
                        || (stats.count == best_stats.count
                            && stats.total_cost == best_stats.total_cost
                            && state_sort_key(state) < state_sort_key(best_state))
                }
            };
            if should_replace {
                best = Some((state, stats));
            }
        }

        if let Some((state, _)) = best {
            match state {
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

fn spatial_distance(previous: Option<DecodedCellState>, next: Option<DecodedCellState>) -> u64 {
    u64::from(temporal_transition_distance(previous, next))
}

fn active_support_is_disconnected(active_cells: &BTreeMap<(i64, i64), DecodedCellState>) -> bool {
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

fn solve_pairwise_fallback(
    cell_candidates: &BTreeMap<(i64, i64), Vec<CellCandidate>>,
    spatial_weight_q10: u32,
) -> BTreeMap<(i64, i64), DecodedCellState> {
    let mut assignment = BTreeMap::<(i64, i64), Option<DecodedCellState>>::new();
    for (&coord, candidates) in cell_candidates {
        let state = candidates.first().and_then(|candidate| candidate.state);
        assignment.insert(coord, state);
    }

    for _ in 0..FALLBACK_ITERATIONS {
        let mut changed = false;
        for (&coord, candidates) in cell_candidates {
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
                    if !cell_candidates.contains_key(&neighbor) {
                        return acc;
                    }
                    let neighbor_state = assignment.get(&neighbor).copied().flatten();
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

            let current = assignment.get(&coord).copied().flatten();
            if current != best.state {
                changed = true;
                assignment.insert(coord, best.state);
            }
        }
        if !changed {
            break;
        }
    }

    assignment
        .into_iter()
        .filter_map(|(coord, state)| state.map(|decoded| (coord, decoded)))
        .collect::<BTreeMap<_, _>>()
}

fn decode_compiled_field_with_solver(
    compiled: &BTreeMap<(i64, i64), CompiledCell>,
    cell_candidates: &BTreeMap<(i64, i64), Vec<CellCandidate>>,
    centerline: &[CenterSample],
    frame: &RenderFrame,
    solve_ribbon: impl FnOnce(&[RibbonSlice], u32) -> Option<Vec<SliceState>>,
) -> DecodedField {
    let mut baseline = decode_locally(cell_candidates);
    let spatial_weight_q10 = sanitize_spatial_weight_q10(frame);
    let slices = build_ribbon_slices_with_compiled(centerline, compiled, cell_candidates, frame);
    match select_decode_path(&baseline, &slices, spatial_weight_q10) {
        DecodePathTrace::Baseline => {
            return DecodedField {
                cells: baseline,
                path: DecodePathTrace::Baseline,
            };
        }
        path @ (DecodePathTrace::PairwiseFallbackDisconnected
        | DecodePathTrace::PairwiseFallbackOversized) => {
            // preserve full slice support until decode-path selection so oversized ribbon
            // support can take the explicit fallback path instead of silently collapsing to local
            // decode.
            return DecodedField {
                cells: solve_pairwise_fallback(cell_candidates, spatial_weight_q10),
                path,
            };
        }
        DecodePathTrace::RibbonDp => {}
        DecodePathTrace::RibbonDpSolveFailed => {}
    }

    let Some(path) = solve_ribbon(&slices, spatial_weight_q10) else {
        return DecodedField {
            cells: solve_pairwise_fallback(cell_candidates, spatial_weight_q10),
            path: DecodePathTrace::RibbonDpSolveFailed,
        };
    };
    merge_ribbon_assignments(&mut baseline, &slices, &path);
    DecodedField {
        cells: baseline,
        path: DecodePathTrace::RibbonDp,
    }
}

#[cfg(test)]
fn decode_compiled_field_trace(
    cell_candidates: &BTreeMap<(i64, i64), Vec<CellCandidate>>,
    centerline: &[CenterSample],
    frame: &RenderFrame,
) -> DecodedField {
    decode_compiled_field_trace_with_compiled(&BTreeMap::new(), cell_candidates, centerline, frame)
}

fn decode_compiled_field_trace_with_compiled(
    compiled: &BTreeMap<(i64, i64), CompiledCell>,
    cell_candidates: &BTreeMap<(i64, i64), Vec<CellCandidate>>,
    centerline: &[CenterSample],
    frame: &RenderFrame,
) -> DecodedField {
    decode_compiled_field_with_solver(
        compiled,
        cell_candidates,
        centerline,
        frame,
        solve_ribbon_dp,
    )
}

#[cfg(test)]
fn decode_compiled_field(
    cell_candidates: &BTreeMap<(i64, i64), Vec<CellCandidate>>,
    centerline: &[CenterSample],
    frame: &RenderFrame,
) -> BTreeMap<(i64, i64), DecodedCellState> {
    decode_compiled_field_trace(cell_candidates, centerline, frame).cells
}

fn decoded_glyph_to_render(glyph: DecodedGlyph) -> Option<Glyph> {
    match glyph {
        DecodedGlyph::Block => Some(Glyph::BLOCK),
        DecodedGlyph::Matrix(mask) => {
            let index = usize::from(mask.min(0x0F));
            let character = MATRIX_CHARACTERS.get(index).copied().unwrap_or("");
            if character.is_empty() {
                None
            } else {
                Some(Glyph::Static(character))
            }
        }
        DecodedGlyph::Octant(mask) => {
            let index = usize::from(mask.saturating_sub(1));
            let character = OCTANT_CHARACTERS.get(index).copied();
            character.map(Glyph::Static)
        }
    }
}

fn push_decoded_cell(
    resources: &mut PlanResources<'_>,
    row: i64,
    col: i64,
    state: DecodedCellState,
) {
    let Some(glyph) = decoded_glyph_to_render(state.glyph) else {
        return;
    };
    let _ = resources.builder.push_cell(
        row,
        col,
        resources.windows_zindex,
        glyph,
        HighlightRef::Normal(state.level),
    );
}

fn sanitize_temporal_weight(frame: &RenderFrame) -> f64 {
    if frame.temporal_stability_weight.is_finite() {
        frame.temporal_stability_weight.clamp(0.0, 3.0)
    } else {
        0.12
    }
}

fn sanitize_spatial_weight_q10(frame: &RenderFrame) -> u32 {
    let weight = if frame.spatial_coherence_weight.is_finite() {
        frame.spatial_coherence_weight.clamp(0.0, 4.0)
    } else {
        1.0
    };
    (weight * 1024.0).round() as u32
}

fn sanitize_top_k(frame: &RenderFrame) -> usize {
    usize::from(frame.top_k_per_cell.clamp(2, 8))
}

fn aspect_metric_distance(start: Point, end: Point, block_aspect_ratio: f64) -> f64 {
    start.display_distance(end, block_aspect_ratio)
}

fn arc_len_q16_delta(start: Point, end: Point, block_aspect_ratio: f64) -> ArcLenQ16 {
    ArcLenQ16::new(latent_field::q16_from_non_negative(aspect_metric_distance(
        start,
        end,
        block_aspect_ratio,
    )))
}

fn speed_gain(speed_cps: f64, start_cps: f64, full_cps: f64, min_gain: f64) -> f64 {
    if !speed_cps.is_finite() {
        return min_gain.clamp(0.0, 1.0);
    }
    let denom = (full_cps - start_cps).max(1.0e-6);
    let t = ((speed_cps - start_cps) / denom).clamp(0.0, 1.0);
    let eased = smoothstep01(t);
    min_gain + (1.0 - min_gain) * eased
}

fn band_speed_gains(
    start_pose: latent_field::Pose,
    end_pose: latent_field::Pose,
    block_aspect_ratio: f64,
    dt_ms: f64,
) -> (f64, f64) {
    let safe_dt_ms = if dt_ms.is_finite() {
        dt_ms.max(1.0)
    } else {
        latent_field::simulation_step_ms(120.0)
    };
    let dt_seconds = safe_dt_ms / 1000.0;
    let speed_cps =
        aspect_metric_distance(start_pose.center, end_pose.center, block_aspect_ratio) / dt_seconds;
    let sheath = speed_gain(
        speed_cps,
        SPEED_SHEATH_START_CPS,
        SPEED_SHEATH_FULL_CPS,
        SPEED_SHEATH_MIN_GAIN,
    );
    let core = speed_gain(
        speed_cps,
        SPEED_CORE_START_CPS,
        SPEED_CORE_FULL_CPS,
        SPEED_CORE_MIN_GAIN,
    );
    (sheath, core)
}

fn handle_stroke_transition(state: &mut PlannerState, frame: &RenderFrame) {
    let stroke_changed = state
        .last_trail_stroke_id
        .is_some_and(|stroke_id| stroke_id != frame.trail_stroke_id);
    if stroke_changed {
        state.arc_len_q16 = ArcLenQ16::ZERO;
        state.last_pose = None;
        state.center_history.clear();
    }
    state.last_trail_stroke_id = Some(frame.trail_stroke_id);
}

fn ensure_latent_cache_current(state: &mut PlannerState) {
    let cache_is_current = state.latent_cache.latest_step() == state.step_index
        && state.latent_cache.history_revision() == state.history_revision;
    if cache_is_current {
        return;
    }

    state.latent_cache = latent_field::LatentFieldCache::rebuild(
        &state.history,
        state.step_index,
        state.history_revision,
    );
}

fn stage_deposited_samples(state: &mut PlannerState, frame: &RenderFrame) {
    handle_stroke_transition(state, frame);
    ensure_latent_cache_current(state);

    let mut latest_pose = state.last_pose;
    let mut history_changed = false;

    for (sample, current_pose) in frame.step_samples.iter().zip(frame_sample_poses(frame)) {
        let start_pose = latest_pose.unwrap_or(current_pose);
        let arc_len_delta_q16 = arc_len_q16_delta(
            start_pose.center,
            current_pose.center,
            frame.block_aspect_ratio,
        );

        state.step_index = state.step_index.next();
        state.latent_cache.advance_to(state.step_index);
        state.arc_len_q16 = state.arc_len_q16.saturating_add(arc_len_delta_q16);
        let (sheath_gain, core_gain) = band_speed_gains(
            start_pose,
            current_pose,
            frame.block_aspect_ratio,
            sample.dt_ms,
        );
        for profile in latent_field::comet_tail_profiles(frame.tail_duration_ms) {
            let band_gain = match profile.band {
                TailBand::Sheath => sheath_gain,
                TailBand::Core => core_gain,
                TailBand::Filament => 1.0,
            };
            let band_intensity = profile.intensity * band_gain;
            let intensity_q16 = latent_field::intensity_q16(band_intensity);
            if intensity_q16 == 0 {
                continue;
            }

            let microtiles = latent_field::deposit_swept_occupancy(
                start_pose,
                current_pose,
                frame.block_aspect_ratio,
                frame.trail_thickness * profile.width_scale,
                frame.trail_thickness_x * profile.width_scale,
            );
            if microtiles.is_empty() {
                continue;
            }
            let Some(bbox) = latent_field::CellRect::from_microtiles(&microtiles) else {
                continue;
            };

            let slice = DepositedSlice {
                stroke_id: frame.trail_stroke_id,
                step_index: state.step_index,
                dt_ms_q16: latent_field::q16_from_non_negative(sample.dt_ms),
                arc_len_q16: state.arc_len_q16,
                bbox,
                band: profile.band,
                support_steps: profile.support_steps(frame.simulation_hz),
                intensity_q16,
                microtiles,
            };
            state.latent_cache.insert_slice(&slice);
            state.history.push_back(slice);
            history_changed = true;
        }
        state.center_history.push_back(CenterPathSample {
            step_index: state.step_index,
            pos: current_pose.center,
        });
        latest_pose = Some(current_pose);
    }

    // presentation ticks can advance latent support windows even when the motion
    // reducer has stopped depositing new samples. That keeps settle-time disappearance inside
    // the normal render pipeline instead of falling back to shell-side cleanup truth.
    for _ in 0..frame.planner_idle_steps {
        state.step_index = state.step_index.next();
        state.latent_cache.advance_to(state.step_index);
    }

    let support_steps =
        latent_field::max_comet_support_steps(frame.tail_duration_ms, frame.simulation_hz);
    let support_steps_u64 = u64::try_from(support_steps).unwrap_or(u64::MAX);
    while state.history.front().is_some_and(|slice| {
        state
            .step_index
            .value()
            .saturating_sub(slice.step_index.value())
            >= support_steps_u64
    }) {
        if let Some(removed_slice) = state.history.pop_front() {
            state.latent_cache.remove_slice(&removed_slice);
            history_changed = true;
        }
    }
    let support_steps_u64 = u64::try_from(support_steps).unwrap_or(u64::MAX);
    while state.center_history.front().is_some_and(|sample| {
        state
            .step_index
            .value()
            .saturating_sub(sample.step_index.value())
            >= support_steps_u64
    }) {
        let _ = state.center_history.pop_front();
    }
    if let Some(latest_pose) = latest_pose {
        state.last_pose = Some(latest_pose);
    }
    if history_changed {
        state.history_revision = state.history_revision.saturating_add(1);
        state
            .latent_cache
            .set_history_revision(state.history_revision);
    }
}
