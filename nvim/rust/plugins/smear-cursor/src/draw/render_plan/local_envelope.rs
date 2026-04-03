use super::*;

pub(super) type SliceSearchBounds = latent_field::CellRect;
#[cfg(test)]
pub(super) type CellRowIndexScratch<T> = latent_field::BorrowedCellRowsScratch<T>;
#[cfg(test)]
pub(super) type CellRowIndex<'a, T> = latent_field::BorrowedCellRows<'a, T>;

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

pub(super) fn to_q16(value: f64) -> i32 {
    if !value.is_finite() {
        return 0;
    }
    let scaled = (value * Q16_SCALE as f64).round();
    scaled.clamp(i32::MIN as f64, i32::MAX as f64) as i32
}

pub(super) fn centerline_tail_u(sample_index: usize, sample_count: usize) -> f64 {
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

pub(super) fn default_head_width_cells(frame: &RenderFrame) -> f64 {
    (slice_band_half_width(frame) * 2.0).clamp(1.0, RIBBON_MAX_RUN_LENGTH as f64)
}

pub(super) fn comet_target_width_cells(head_width: f64, u: f64) -> f64 {
    let tip_width = (head_width * COMET_TIP_WIDTH_RATIO)
        .max(COMET_MIN_RESOLVABLE_WIDTH)
        .min(head_width);
    let progress = taper_progress(u);
    (head_width + (tip_width - head_width) * progress).max(COMET_MIN_RESOLVABLE_WIDTH)
}

pub(super) fn centerline_curvature(
    centerline: &[CenterSample],
    index: usize,
    block_aspect_ratio: f64,
) -> f64 {
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

fn slice_band_half_width(frame: &RenderFrame) -> f64 {
    let thickness = frame.trail_thickness.max(frame.trail_thickness_x);
    let safe = if thickness.is_finite() {
        thickness.max(0.5)
    } else {
        1.0
    };
    (0.75 * safe + 0.5).clamp(0.75, 3.0)
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

pub(super) fn populate_resampled_centerline_with_scratch(
    history: &VecDeque<CenterPathSample>,
    spacing: f64,
    block_aspect_ratio: f64,
    scratch: &mut PlannerDecodeScratch,
) {
    let PlannerDecodeScratch {
        centerline_points,
        centerline_cumulative,
        centerline,
        ..
    } = scratch;
    centerline_points.clear();
    centerline_cumulative.clear();
    centerline.clear();

    if history.is_empty() {
        return;
    }

    centerline_points.reserve(history.len().saturating_sub(centerline_points.capacity()));
    for sample in history {
        let should_push = centerline_points.last().is_none_or(|last| {
            aspect_metric_distance(*last, sample.pos, block_aspect_ratio) > 1.0e-3
        });
        if should_push {
            centerline_points.push(sample.pos);
        }
    }

    if centerline_points.is_empty() {
        return;
    }
    if centerline_points.len() == 1 {
        centerline.push(CenterSample {
            pos: centerline_points[0],
            tangent_row: 0.0,
            tangent_col: 1.0,
        });
        return;
    }

    centerline_cumulative.reserve(
        centerline_points
            .len()
            .saturating_sub(centerline_cumulative.capacity()),
    );
    centerline_cumulative.push(0.0);
    for pair in centerline_points.windows(2) {
        let delta = aspect_metric_distance(pair[1], pair[0], block_aspect_ratio);
        let previous = centerline_cumulative.last().copied().unwrap_or(0.0);
        centerline_cumulative.push(previous + delta.max(0.0));
    }

    let total_len = centerline_cumulative.last().copied().unwrap_or(0.0);
    if total_len <= f64::EPSILON {
        centerline.push(CenterSample {
            pos: centerline_points[0],
            tangent_row: 0.0,
            tangent_col: 1.0,
        });
        return;
    }

    let safe_spacing = if spacing.is_finite() {
        spacing.max(0.125)
    } else {
        RIBBON_SAMPLE_SPACING_CELLS
    };
    let sample_count = (total_len / safe_spacing).ceil() as usize + 1;

    centerline.reserve(sample_count.saturating_sub(centerline.capacity()));
    let mut segment = 0_usize;
    for sample_index in 0..sample_count {
        let target_arc = (sample_index as f64 * safe_spacing).min(total_len);
        while segment + 2 < centerline_cumulative.len()
            && target_arc > centerline_cumulative[segment + 1]
        {
            segment = segment.saturating_add(1);
        }

        let start = centerline_points[segment];
        let end = centerline_points[segment + 1];
        let segment_start = centerline_cumulative[segment];
        let segment_end = centerline_cumulative[segment + 1];
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
        centerline.push(CenterSample {
            pos,
            tangent_row,
            tangent_col,
        });
    }
}

#[cfg(test)]
pub(super) fn resample_centerline(
    history: &VecDeque<CenterPathSample>,
    spacing: f64,
    block_aspect_ratio: f64,
) -> Vec<CenterSample> {
    let mut scratch = PlannerDecodeScratch::default();
    populate_resampled_centerline_with_scratch(history, spacing, block_aspect_ratio, &mut scratch);
    scratch.centerline
}

pub(super) fn ribbon_slice_search_bounds(
    sample: CenterSample,
    frame: &RenderFrame,
    tail_u: f64,
    curvature: f64,
) -> SliceSearchBounds {
    let band_half_width = solve_slice_band_half_width(frame, tail_u, curvature);
    let safe_aspect = crate::types::display_metric_row_scale(frame.block_aspect_ratio);
    let normal_row = -sample.tangent_col;
    let normal_col = sample.tangent_row;
    let row_display_bound =
        sample.tangent_row.abs() * RIBBON_SLICE_HALF_SPAN + normal_row.abs() * band_half_width;
    let col_bound =
        sample.tangent_col.abs() * RIBBON_SLICE_HALF_SPAN + normal_col.abs() * band_half_width;
    let row_bound = row_display_bound / safe_aspect;

    SliceSearchBounds {
        min_row: (sample.pos.row - row_bound).floor() as i64 - 1,
        max_row: (sample.pos.row + row_bound).ceil() as i64,
        min_col: (sample.pos.col - col_bound).floor() as i64 - 1,
        max_col: (sample.pos.col + col_bound).ceil() as i64,
    }
}

pub(super) fn compute_local_query_envelope(
    centerline: &[CenterSample],
    previous_cells: &BTreeMap<(i64, i64), DecodedCellState>,
    frame: &RenderFrame,
    previous_cell_halo: i64,
) -> Option<SliceSearchBounds> {
    let mut envelope: Option<SliceSearchBounds> = None;

    for (sample_index, sample) in centerline.iter().copied().enumerate() {
        let bounds = ribbon_slice_search_bounds(
            sample,
            frame,
            centerline_tail_u(sample_index, centerline.len()),
            centerline_curvature_smoothed(centerline, sample_index, frame.block_aspect_ratio),
        );
        if let Some(existing) = &mut envelope {
            existing.min_row = existing.min_row.min(bounds.min_row);
            existing.max_row = existing.max_row.max(bounds.max_row);
            existing.min_col = existing.min_col.min(bounds.min_col);
            existing.max_col = existing.max_col.max(bounds.max_col);
        } else {
            envelope = Some(bounds);
        }
    }

    let halo = previous_cell_halo.max(0);
    let mut previous_coords = previous_cells.keys().copied();
    if let Some((first_row, first_col)) = previous_coords.next() {
        let mut previous_bounds =
            SliceSearchBounds::new(first_row, first_row, first_col, first_col);
        for (row, col) in previous_coords {
            previous_bounds.min_row = previous_bounds.min_row.min(row);
            previous_bounds.max_row = previous_bounds.max_row.max(row);
            previous_bounds.min_col = previous_bounds.min_col.min(col);
            previous_bounds.max_col = previous_bounds.max_col.max(col);
        }
        previous_bounds.min_row = previous_bounds.min_row.saturating_sub(halo);
        previous_bounds.max_row = previous_bounds.max_row.saturating_add(halo);
        previous_bounds.min_col = previous_bounds.min_col.saturating_sub(halo);
        previous_bounds.max_col = previous_bounds.max_col.saturating_add(halo);

        if let Some(existing) = &mut envelope {
            existing.min_row = existing.min_row.min(previous_bounds.min_row);
            existing.max_row = existing.max_row.max(previous_bounds.max_row);
            existing.min_col = existing.min_col.min(previous_bounds.min_col);
            existing.max_col = existing.max_col.max(previous_bounds.max_col);
        } else {
            envelope = Some(previous_bounds);
        }
    }

    envelope
}

fn local_query_envelope_area_cells(bounds: SliceSearchBounds) -> Option<u64> {
    let row_span =
        u64::try_from(i128::from(bounds.max_row) - i128::from(bounds.min_row) + 1).ok()?;
    let col_span =
        u64::try_from(i128::from(bounds.max_col) - i128::from(bounds.min_col) + 1).ok()?;
    row_span.checked_mul(col_span)
}

pub(super) fn fast_path_query_bounds(
    centerline: &[CenterSample],
    previous_cells: &BTreeMap<(i64, i64), DecodedCellState>,
    frame: &RenderFrame,
    previous_cell_halo: i64,
) -> Option<SliceSearchBounds> {
    let bounds =
        compute_local_query_envelope(centerline, previous_cells, frame, previous_cell_halo)?;
    let area_cells = local_query_envelope_area_cells(bounds).unwrap_or(u64::MAX);
    crate::events::record_planner_local_query_envelope_area_cells(area_cells);
    if area_cells > LOCAL_QUERY_ENVELOPE_FAST_PATH_MAX_AREA_CELLS {
        return None;
    }

    Some(bounds)
}

enum CompiledCellQueryView<'a> {
    Borrowed(latent_field::BorrowedCellRows<'a, CompiledCell>),
    Owned(&'a latent_field::CellRows<CompiledCell>),
}

impl<'a> CompiledCellQueryView<'a> {
    fn is_empty(&self) -> bool {
        match self {
            Self::Borrowed(rows) => rows.is_empty(),
            Self::Owned(rows) => rows.is_empty(),
        }
    }

    fn for_each_in_bounds(
        &self,
        bounds: SliceSearchBounds,
        visit: impl FnMut((i64, i64), &CompiledCell),
    ) -> latent_field::CellRowQueryStats {
        match self {
            Self::Borrowed(rows) => rows.for_each_in_bounds(bounds, visit),
            Self::Owned(rows) => rows.for_each_in_bounds(bounds, visit),
        }
    }
}

fn measured_slice_support_width_cells(
    sample: CenterSample,
    normal_row: f64,
    normal_col: f64,
    band_half_width: f64,
    compiled: &CompiledCellQueryView<'_>,
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

    let query_stats = compiled.for_each_in_bounds(bounds, |coord, compiled_cell| {
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
    });
    crate::events::record_planner_local_query(
        query_stats.bucket_maps_scanned,
        query_stats.bucket_cells_scanned,
        query_stats.local_query_cells,
    );
    crate::events::record_planner_compiled_query_cells_count(query_stats.local_query_cells);

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
pub(super) fn build_ribbon_slices(
    centerline: &[CenterSample],
    cell_candidates: &BTreeMap<(i64, i64), Vec<CellCandidate>>,
    frame: &RenderFrame,
) -> Vec<RibbonSlice> {
    let compiled = CompiledField::default();
    let mut scratch = SolverScratch::default();
    build_ribbon_slices_with_compiled_and_scratch(
        centerline,
        &compiled,
        cell_candidates,
        frame,
        &mut scratch,
    )
}

#[cfg(test)]
pub(super) fn build_ribbon_slices_with_compiled(
    centerline: &[CenterSample],
    compiled: &BTreeMap<(i64, i64), CompiledCell>,
    cell_candidates: &BTreeMap<(i64, i64), Vec<CellCandidate>>,
    frame: &RenderFrame,
) -> Vec<RibbonSlice> {
    let compiled = CompiledField::Reference(compiled.clone());
    let mut scratch = SolverScratch::default();
    build_ribbon_slices_with_compiled_and_scratch(
        centerline,
        &compiled,
        cell_candidates,
        frame,
        &mut scratch,
    )
}

pub(super) fn build_ribbon_slices_with_compiled_and_scratch(
    centerline: &[CenterSample],
    compiled: &CompiledField,
    cell_candidates: &BTreeMap<(i64, i64), Vec<CellCandidate>>,
    frame: &RenderFrame,
    scratch: &mut SolverScratch,
) -> Vec<RibbonSlice> {
    if centerline.is_empty() || cell_candidates.is_empty() {
        return Vec::new();
    }

    let SolverScratch {
        compiled_row_index,
        candidate_row_index,
        ..
    } = scratch;
    let compiled_index = match compiled {
        CompiledField::Reference(compiled) => CompiledCellQueryView::Borrowed(
            latent_field::BorrowedCellRows::build(compiled, compiled_row_index),
        ),
        CompiledField::Rows(compiled) => CompiledCellQueryView::Owned(compiled),
    };
    let candidate_index =
        latent_field::BorrowedCellRows::build(cell_candidates, candidate_row_index);
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
        let bounds = ribbon_slice_search_bounds(sample, frame, tail_u, curvature);
        let measured_support_width_cells = measured_slice_support_width_cells(
            sample,
            normal_row,
            normal_col,
            band_half_width,
            &compiled_index,
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

        let query_stats = candidate_index.for_each_in_bounds(bounds, |coord, candidates| {
            let row_center = coord.0 as f64 + 0.5;
            let col_center = coord.1 as f64 + 0.5;
            let delta_row = row_center - sample.pos.row;
            let delta_col = col_center - sample.pos.col;
            let delta_row_display = delta_row * safe_aspect;
            let along = delta_row_display * sample.tangent_row + delta_col * sample.tangent_col;
            let across = delta_row_display * normal_row + delta_col * normal_col;

            if along.abs() > RIBBON_SLICE_HALF_SPAN || across.abs() > band_half_width {
                return;
            }

            let Some(empty_cost) = candidates
                .iter()
                .find_map(|candidate| (candidate.state.is_none()).then_some(candidate.unary_cost))
            else {
                return;
            };
            let normal_q16 = to_q16(across);
            cells.push(SliceCell::new(
                coord,
                normal_q16,
                cell_half_span_q16,
                empty_cost,
                non_empty_candidates(candidates),
            ));
        });
        crate::events::record_planner_local_query(
            query_stats.bucket_maps_scanned,
            query_stats.bucket_cells_scanned,
            query_stats.local_query_cells,
        );
        crate::events::record_planner_candidate_query_cells_count(query_stats.local_query_cells);

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
