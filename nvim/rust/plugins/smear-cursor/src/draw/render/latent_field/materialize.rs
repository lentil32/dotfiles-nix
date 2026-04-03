use super::EPSILON;
use super::MICRO_H;
use super::MICRO_W;
use super::MicroTile;
use super::Pose;
use super::SAMPLE_Q12_SCALE;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug)]
enum AxisSampleProjection {
    Invalid,
    Stationary { distance: f64 },
    Moving { center_t: f64, inv_abs_delta: f64 },
}

impl AxisSampleProjection {
    fn for_sample(sample: f64, start: f64, end: f64) -> Self {
        if !sample.is_finite() || !start.is_finite() || !end.is_finite() {
            return Self::Invalid;
        }

        let delta = end - start;
        if delta.abs() <= EPSILON {
            return Self::Stationary {
                distance: (sample - start).abs(),
            };
        }

        Self::Moving {
            center_t: (sample - start) / delta,
            inv_abs_delta: delta.abs().recip(),
        }
    }

    fn interval(self, half_extent: f64) -> Option<(f64, f64)> {
        if !half_extent.is_finite() || half_extent <= 0.0 {
            return None;
        }

        match self {
            Self::Invalid => None,
            Self::Stationary { distance } => (distance <= half_extent).then_some((0.0, 1.0)),
            Self::Moving {
                center_t,
                inv_abs_delta,
            } => {
                let radius = half_extent * inv_abs_delta;
                let lo = (center_t - radius).clamp(0.0, 1.0);
                let hi = (center_t + radius).clamp(0.0, 1.0);
                (hi > lo).then_some((lo, hi))
            }
        }
    }
}

#[derive(Clone, Debug, Default)]
pub(in super::super) struct SweptOccupancyGeometry {
    row_projections: Vec<(i64, SampleProjectionRow)>,
    col_projections: Vec<(i64, SampleProjectionCol)>,
    safe_aspect_ratio: f64,
    base_half_height: f64,
    base_half_width: f64,
    max_half_height: f64,
    max_half_width: f64,
}

#[derive(Debug, Default)]
pub(in super::super) struct SweepMaterializeScratch {
    row_intervals: Vec<(i64, SampleIntervals<MICRO_H>)>,
    col_intervals: Vec<(i64, SampleIntervals<MICRO_W>)>,
}

type SampleProjectionRow = [AxisSampleProjection; MICRO_H];
type SampleProjectionCol = [AxisSampleProjection; MICRO_W];

fn sample_offset(sample_index: usize, samples_per_axis: usize) -> f64 {
    (sample_index as f64 + 0.5) / samples_per_axis as f64
}

fn sample_center_cell_span(
    start: f64,
    end: f64,
    half_extent: f64,
    samples_per_axis: usize,
) -> Option<(i64, i64)> {
    if !start.is_finite() || !end.is_finite() || !half_extent.is_finite() || samples_per_axis == 0 {
        return None;
    }

    let first_offset = sample_offset(0, samples_per_axis);
    let last_offset = sample_offset(samples_per_axis.saturating_sub(1), samples_per_axis);
    let min_center = start.min(end) - half_extent;
    let max_center = start.max(end) + half_extent;
    let min_cell = (min_center - last_offset).ceil() as i64;
    let max_cell = (max_center - first_offset).floor() as i64;
    (min_cell <= max_cell).then_some((min_cell, max_cell))
}

type AxisInterval = (f64, f64);
type SampleIntervals<const N: usize> = [Option<AxisInterval>; N];
type SampleProjection<const N: usize> = [AxisSampleProjection; N];

fn axis_projections_for_cells<const N: usize>(
    min_cell: i64,
    max_cell: i64,
    sample_scale: f64,
    start: f64,
    end: f64,
    max_half_extent: f64,
) -> Vec<(i64, SampleProjection<N>)> {
    let cell_count =
        usize::try_from(max_cell.saturating_sub(min_cell).saturating_add(1)).unwrap_or_default();
    let mut cells = Vec::with_capacity(cell_count);

    for cell in min_cell..=max_cell {
        let mut projections = [AxisSampleProjection::Invalid; N];
        let mut any_coverage = false;
        for (sample_index, projection_slot) in projections.iter_mut().enumerate() {
            let sample = (cell as f64 + sample_offset(sample_index, N)) * sample_scale;
            let projection = AxisSampleProjection::for_sample(sample, start, end);
            any_coverage |= projection.interval(max_half_extent).is_some();
            *projection_slot = projection;
        }

        if any_coverage {
            cells.push((cell, projections));
        }
    }

    cells
}

fn populate_axis_intervals_from_projections<const N: usize>(
    projections: &[(i64, SampleProjection<N>)],
    half_extent: f64,
    target: &mut Vec<(i64, SampleIntervals<N>)>,
) {
    target.clear();

    for (cell, samples) in projections {
        let mut intervals = [None; N];
        let mut any_coverage = false;
        for (interval_slot, projection) in intervals.iter_mut().zip(samples.iter().copied()) {
            let interval = projection.interval(half_extent);
            any_coverage |= interval.is_some();
            *interval_slot = interval;
        }

        if any_coverage {
            target.push((*cell, intervals));
        }
    }
}

pub(in super::super) fn prepare_swept_occupancy_geometry(
    start: Pose,
    end: Pose,
    block_aspect_ratio: f64,
    thickness_y: f64,
    thickness_x: f64,
) -> SweptOccupancyGeometry {
    let safe_aspect_ratio = if block_aspect_ratio.is_finite() {
        block_aspect_ratio.max(EPSILON)
    } else {
        1.0
    };
    let width_scale = if thickness_x.is_finite() {
        thickness_x.max(EPSILON)
    } else {
        1.0
    };
    let height_scale = if thickness_y.is_finite() {
        thickness_y.max(EPSILON)
    } else {
        1.0
    };
    let base_half_width = start.half_width.max(end.half_width);
    let base_half_height = start.half_height.max(end.half_height);
    let max_half_width = (base_half_width * width_scale).max(EPSILON);
    let max_half_height = (base_half_height * height_scale).max(EPSILON);

    let mut geometry = SweptOccupancyGeometry {
        safe_aspect_ratio,
        base_half_height,
        base_half_width,
        max_half_height,
        max_half_width,
        ..SweptOccupancyGeometry::default()
    };

    let Some((min_row, max_row)) =
        sample_center_cell_span(start.center.row, end.center.row, max_half_height, MICRO_H)
    else {
        return geometry;
    };
    let Some((min_col, max_col)) =
        sample_center_cell_span(start.center.col, end.center.col, max_half_width, MICRO_W)
    else {
        return geometry;
    };

    geometry.row_projections = axis_projections_for_cells::<MICRO_H>(
        min_row,
        max_row,
        safe_aspect_ratio,
        start.center.row * safe_aspect_ratio,
        end.center.row * safe_aspect_ratio,
        max_half_height * safe_aspect_ratio,
    );
    geometry.col_projections = axis_projections_for_cells::<MICRO_W>(
        min_col,
        max_col,
        1.0,
        start.center.col,
        end.center.col,
        max_half_width,
    );
    geometry
}

pub(in super::super) fn materialize_swept_occupancy_with_scratch(
    geometry: &SweptOccupancyGeometry,
    thickness_y: f64,
    thickness_x: f64,
    scratch: &mut SweepMaterializeScratch,
) -> BTreeMap<(i64, i64), MicroTile> {
    if geometry.row_projections.is_empty() || geometry.col_projections.is_empty() {
        return BTreeMap::new();
    }

    let width_scale = if thickness_x.is_finite() {
        thickness_x.max(EPSILON)
    } else {
        1.0
    };
    let height_scale = if thickness_y.is_finite() {
        thickness_y.max(EPSILON)
    } else {
        1.0
    };
    let half_width = (geometry.base_half_width * width_scale).max(EPSILON);
    let half_height = (geometry.base_half_height * height_scale).max(EPSILON);
    debug_assert!(
        half_width <= geometry.max_half_width + EPSILON
            && half_height <= geometry.max_half_height + EPSILON,
        "sweep materialization should stay within the prepared max extents"
    );

    populate_axis_intervals_from_projections(
        &geometry.row_projections,
        half_height * geometry.safe_aspect_ratio,
        &mut scratch.row_intervals,
    );
    populate_axis_intervals_from_projections(
        &geometry.col_projections,
        half_width,
        &mut scratch.col_intervals,
    );
    if scratch.row_intervals.is_empty() || scratch.col_intervals.is_empty() {
        return BTreeMap::new();
    }

    let mut tiles = BTreeMap::<(i64, i64), MicroTile>::new();

    for (row, row_intervals) in &scratch.row_intervals {
        for (col, col_intervals) in &scratch.col_intervals {
            let mut tile = MicroTile::default();
            let mut any_coverage = false;

            for (sample_row, y_interval) in row_intervals.iter().copied().enumerate() {
                let Some((y_lo, y_hi)) = y_interval else {
                    continue;
                };

                for (sample_col, x_interval) in col_intervals.iter().copied().enumerate() {
                    let Some((x_lo, x_hi)) = x_interval else {
                        continue;
                    };

                    let index = sample_row * MICRO_W + sample_col;
                    let occupancy = (x_hi.min(y_hi) - x_lo.max(y_lo)).clamp(0.0, 1.0);
                    let sample_q12 = (occupancy * SAMPLE_Q12_SCALE as f64).round() as u16;
                    tile.samples_q12[index] = sample_q12;
                    any_coverage |= sample_q12 > 0;
                }
            }

            if any_coverage {
                tiles.insert((*row, *col), tile);
            }
        }
    }

    tiles
}

#[cfg(test)]
pub(in super::super) fn deposit_swept_occupancy(
    start: Pose,
    end: Pose,
    block_aspect_ratio: f64,
    thickness_y: f64,
    thickness_x: f64,
) -> BTreeMap<(i64, i64), MicroTile> {
    let geometry =
        prepare_swept_occupancy_geometry(start, end, block_aspect_ratio, thickness_y, thickness_x);
    let mut scratch = SweepMaterializeScratch::default();
    materialize_swept_occupancy_with_scratch(&geometry, thickness_y, thickness_x, &mut scratch)
}

#[cfg(test)]
mod tests {
    use super::super::CellRect;
    use super::super::TailBand;
    use super::super::comet_tail_profiles;
    use super::Pose;
    use super::SweepMaterializeScratch;
    use super::deposit_swept_occupancy;
    use super::materialize_swept_occupancy_with_scratch;
    use super::prepare_swept_occupancy_geometry;
    use crate::test_support::proptest::positive_aspect_ratio;
    use crate::test_support::proptest::pure_config;
    use crate::types::Point;
    use proptest::prelude::*;

    #[derive(Clone, Debug)]
    struct MaterializeFixture {
        start: Pose,
        end: Pose,
        block_aspect_ratio: f64,
        band_thicknesses: Vec<(TailBand, f64, f64)>,
        max_thickness_y: f64,
        max_thickness_x: f64,
    }

    fn aligned_pose() -> BoxedStrategy<Pose> {
        (-10_i64..=10_i64, -10_i64..=10_i64, 2_u8..=8_u8, 2_u8..=8_u8)
            .prop_map(|(row, col, half_height_steps, half_width_steps)| Pose {
                center: Point {
                    row: row as f64 + 0.5,
                    col: col as f64 + 0.5,
                },
                half_height: f64::from(half_height_steps) / 8.0,
                half_width: f64::from(half_width_steps) / 8.0,
            })
            .boxed()
    }

    fn materialize_fixture() -> BoxedStrategy<MaterializeFixture> {
        (
            aligned_pose(),
            aligned_pose(),
            positive_aspect_ratio(),
            0.5_f64..=1.5_f64,
            0.5_f64..=1.5_f64,
            80.0_f64..=360.0_f64,
        )
            .prop_map(
                |(
                    start,
                    end,
                    block_aspect_ratio,
                    base_thickness_y,
                    base_thickness_x,
                    tail_duration_ms,
                )| {
                    let profiles = comet_tail_profiles(tail_duration_ms);
                    let band_thicknesses = profiles
                        .into_iter()
                        .map(|profile| {
                            (
                                profile.band,
                                base_thickness_y * profile.width_scale,
                                base_thickness_x * profile.width_scale,
                            )
                        })
                        .collect::<Vec<_>>();
                    let max_thickness_y = band_thicknesses
                        .iter()
                        .fold(0.0_f64, |max, (_, thickness_y, _)| max.max(*thickness_y));
                    let max_thickness_x = band_thicknesses
                        .iter()
                        .fold(0.0_f64, |max, (_, _, thickness_x)| max.max(*thickness_x));

                    MaterializeFixture {
                        start,
                        end,
                        block_aspect_ratio,
                        band_thicknesses,
                        max_thickness_y,
                        max_thickness_x,
                    }
                },
            )
            .boxed()
    }

    proptest! {
        #![proptest_config(pure_config())]

        #[test]
        fn prop_deposit_bounds_cover_endpoints_and_shared_geometry_matches_direct_materialization(
            fixture in materialize_fixture(),
        ) {
            let geometry = prepare_swept_occupancy_geometry(
                fixture.start,
                fixture.end,
                fixture.block_aspect_ratio,
                fixture.max_thickness_y,
                fixture.max_thickness_x,
            );
            let mut scratch = SweepMaterializeScratch::default();
            let start_cell = (
                fixture.start.center.row.floor() as i64,
                fixture.start.center.col.floor() as i64,
            );
            let end_cell = (
                fixture.end.center.row.floor() as i64,
                fixture.end.center.col.floor() as i64,
            );

            for (band, thickness_y, thickness_x) in &fixture.band_thicknesses {
                let direct = deposit_swept_occupancy(
                    fixture.start,
                    fixture.end,
                    fixture.block_aspect_ratio,
                    *thickness_y,
                    *thickness_x,
                );
                let shared = materialize_swept_occupancy_with_scratch(
                    &geometry,
                    *thickness_y,
                    *thickness_x,
                    &mut scratch,
                );
                prop_assert_eq!(
                    &shared,
                    &direct,
                    "shared sweep geometry should preserve direct deposition for {:?}",
                    band,
                );
                let Some(bbox) = CellRect::from_microtiles(&direct) else {
                    // Thin aligned sweeps can quantize away entirely when no microtile sample
                    // center falls inside the swept support for this band.
                    continue;
                };

                prop_assert!(bbox.contains(start_cell));
                prop_assert!(bbox.contains(end_cell));
            }
        }

        #[test]
        fn prop_shared_sweep_geometry_materialization_is_order_independent(
            fixture in materialize_fixture(),
        ) {
            let geometry = prepare_swept_occupancy_geometry(
                fixture.start,
                fixture.end,
                fixture.block_aspect_ratio,
                fixture.max_thickness_y,
                fixture.max_thickness_x,
            );
            let materialize_band_order =
                |ordered_band_thicknesses: &[(TailBand, f64, f64)]| {
                    let mut scratch = SweepMaterializeScratch::default();
                    ordered_band_thicknesses
                        .iter()
                        .map(|(band, thickness_y, thickness_x)| {
                            (
                                *band,
                                materialize_swept_occupancy_with_scratch(
                                    &geometry,
                                    *thickness_y,
                                    *thickness_x,
                                    &mut scratch,
                                ),
                            )
                        })
                        .collect::<Vec<_>>()
                };

            let forward = materialize_band_order(&fixture.band_thicknesses);
            let mut reversed_band_thicknesses = fixture.band_thicknesses;
            reversed_band_thicknesses.reverse();
            let reverse = materialize_band_order(&reversed_band_thicknesses);

            for band in [TailBand::Sheath, TailBand::Core, TailBand::Filament] {
                let forward_tiles = forward
                    .iter()
                    .find(|(candidate_band, _)| *candidate_band == band)
                    .map(|(_, tiles)| tiles)
                    .expect("forward order should include each tail band");
                let reverse_tiles = reverse
                    .iter()
                    .find(|(candidate_band, _)| *candidate_band == band)
                    .map(|(_, tiles)| tiles)
                    .expect("reverse order should include each tail band");

                prop_assert_eq!(forward_tiles, reverse_tiles);
            }
        }
    }
}
