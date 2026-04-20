use crate::position::RenderPoint;
#[cfg(test)]
use std::collections::BTreeMap;

#[path = "latent_field/materialize.rs"]
mod materialize;
#[path = "latent_field/reference_compile.rs"]
mod reference_compile;
#[path = "latent_field/spatial_index.rs"]
mod spatial_index;
#[path = "latent_field/store.rs"]
mod store;
#[path = "latent_field/weights.rs"]
mod weights;

pub(super) use materialize::SweepMaterializeScratch;
#[cfg(test)]
pub(super) use materialize::deposit_swept_occupancy;
pub(super) use materialize::materialize_swept_occupancy_with_scratch;
pub(super) use materialize::prepare_swept_occupancy_geometry;
pub(super) use reference_compile::CompileScratch;
#[cfg(test)]
pub(super) use reference_compile::compile_field;
pub(super) use reference_compile::compile_field_in_bounds_rows_with_scratch;
#[cfg(test)]
pub(super) use reference_compile::compile_field_in_bounds_with_scratch;
pub(super) use reference_compile::compile_field_reference_with_scratch;
pub(super) use spatial_index::BorrowedCellRows;
pub(super) use spatial_index::BorrowedCellRowsScratch;
pub(super) use spatial_index::CellRowQueryStats;
pub(super) use spatial_index::CellRows;
#[cfg(test)]
pub(super) use store::DepositedSlice;
pub(super) use store::LatentFieldCache;
pub(super) use weights::comet_tail_profiles;
pub(super) use weights::intensity_q16;
pub(super) use weights::max_comet_support_steps;
pub(super) use weights::q16_from_non_negative;
pub(super) use weights::simulation_step_ms;
#[cfg(test)]
pub(super) use weights::slice_recent_weight_q16;
#[cfg(test)]
pub(super) use weights::slice_weight_q16;
pub(super) use weights::tail_support_steps;

pub(super) const MICRO_W: usize = 8;
pub(super) const MICRO_H: usize = 8;
pub(super) const MICRO_TILE_SAMPLES: usize = MICRO_W * MICRO_H;

const SAMPLE_Q12_SCALE: u32 = 4095;
const MIN_COMPILED_SAMPLE_Q12: u16 = 4;
const EPSILON: f64 = 1.0e-9;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct MicroTile {
    pub(super) samples_q12: [u16; MICRO_TILE_SAMPLES],
}

impl Default for MicroTile {
    fn default() -> Self {
        Self {
            samples_q12: [0_u16; MICRO_TILE_SAMPLES],
        }
    }
}

impl MicroTile {
    pub(super) fn max_sample_q12(self) -> u16 {
        self.samples_q12.iter().copied().max().unwrap_or(0)
    }

    #[cfg(test)]
    fn total_mass_q12(&self) -> u32 {
        self.samples_q12.iter().copied().map(u32::from).sum()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct MaterializedTile {
    pub(super) coord: (i64, i64),
    pub(super) tile: MicroTile,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Pose {
    pub(super) center: RenderPoint,
    pub(super) half_height: f64,
    pub(super) half_width: f64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct CellRect {
    pub(super) min_row: i64,
    pub(super) max_row: i64,
    pub(super) min_col: i64,
    pub(super) max_col: i64,
}

impl CellRect {
    pub(super) const fn new(min_row: i64, max_row: i64, min_col: i64, max_col: i64) -> Self {
        Self {
            min_row,
            max_row,
            min_col,
            max_col,
        }
    }

    #[cfg(test)]
    pub(super) fn from_microtiles(tiles: &BTreeMap<(i64, i64), MicroTile>) -> Option<Self> {
        let mut coords = tiles.keys().copied();
        let (first_row, first_col) = coords.next()?;
        let mut min_row = first_row;
        let mut max_row = first_row;
        let mut min_col = first_col;
        let mut max_col = first_col;

        for (row, col) in coords {
            min_row = min_row.min(row);
            max_row = max_row.max(row);
            min_col = min_col.min(col);
            max_col = max_col.max(col);
        }

        Some(Self::new(min_row, max_row, min_col, max_col))
    }

    #[cfg(test)]
    pub(super) fn contains(self, coord: (i64, i64)) -> bool {
        coord.0 >= self.min_row
            && coord.0 <= self.max_row
            && coord.1 >= self.min_col
            && coord.1 <= self.max_col
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum TailBand {
    Sheath,
    Core,
    Filament,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct TailBandProfile {
    pub(super) band: TailBand,
    pub(super) width_scale: f64,
    pub(super) lifetime_ms: f64,
    pub(super) min_support_steps: usize,
    pub(super) intensity: f64,
}

impl TailBandProfile {
    pub(super) fn support_steps(self, simulation_hz: f64) -> usize {
        tail_support_steps(self.lifetime_ms, simulation_hz).max(self.min_support_steps)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct AgeMoment {
    pub(super) total_mass_q12: u32,
    pub(super) recent_mass_q12: u32,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct CompiledCell {
    pub(super) tile: MicroTile,
    pub(super) age: AgeMoment,
}

#[cfg(test)]
mod tests {
    use super::CellRect;
    use super::CompileScratch;
    use super::CompiledCell;
    use super::DepositedSlice;
    use super::LatentFieldCache;
    use super::MICRO_TILE_SAMPLES;
    use super::MIN_COMPILED_SAMPLE_Q12;
    use super::Pose;
    use super::SAMPLE_Q12_SCALE;
    use super::TailBand;
    use super::comet_tail_profiles;
    use super::compile_field;
    use super::compile_field_in_bounds_rows_with_scratch;
    use super::compile_field_in_bounds_with_scratch;
    use super::compile_field_reference_with_scratch;
    use super::deposit_swept_occupancy;
    use super::intensity_q16;
    use super::max_comet_support_steps;
    use super::q16_from_non_negative;
    use super::simulation_step_ms;
    use super::slice_recent_weight_q16;
    use super::slice_weight_q16;
    use super::tail_support_steps;
    use super::weights::WEIGHT_Q16_SCALE;
    use crate::core::types::ArcLenQ16;
    use crate::core::types::StepIndex;
    use crate::core::types::StrokeId;
    use crate::position::RenderPoint;
    use crate::test_support::proptest::pure_config;
    use pretty_assertions::assert_eq;
    use proptest::collection::vec;
    use proptest::prelude::*;
    use std::collections::BTreeMap;
    use std::collections::VecDeque;

    #[derive(Clone, Debug)]
    struct LatentHistoryFixture {
        history: VecDeque<DepositedSlice>,
        latest_step: StepIndex,
        bounds: CellRect,
    }

    fn compile_field_reference(cache: &LatentFieldCache) -> BTreeMap<(i64, i64), CompiledCell> {
        let mut scratch = CompileScratch::default();
        compile_field_reference_with_scratch(cache, &mut scratch)
    }

    fn compile_field_in_bounds(
        cache: &LatentFieldCache,
        bounds: CellRect,
    ) -> BTreeMap<(i64, i64), CompiledCell> {
        let mut scratch = CompileScratch::default();
        compile_field_in_bounds_with_scratch(cache, bounds, &mut scratch)
    }

    #[test]
    fn comet_support_steps_scale_consistently_with_duration_and_hz() {
        assert_eq!(tail_support_steps(250.0, 120.0), 30);
        assert_eq!(tail_support_steps(f64::NAN, f64::NAN), 22);

        for (duration_ms, simulation_hz) in [(120.0, 60.0), (198.0, 120.0), (420.0, 144.0)] {
            let expected = comet_tail_profiles(duration_ms)
                .into_iter()
                .map(|profile| profile.support_steps(simulation_hz))
                .max()
                .unwrap_or(1);
            assert_eq!(
                max_comet_support_steps(duration_ms, simulation_hz),
                expected,
                "max support mismatch for duration_ms={duration_ms} simulation_hz={simulation_hz}"
            );
        }
    }

    fn fully_covered_tile() -> super::MicroTile {
        super::MicroTile {
            samples_q12: [SAMPLE_Q12_SCALE as u16; MICRO_TILE_SAMPLES],
        }
    }

    fn aligned_pose() -> BoxedStrategy<Pose> {
        (-12_i64..=12_i64, -12_i64..=12_i64, 2_u8..=8_u8, 2_u8..=8_u8)
            .prop_map(|(row, col, half_height_steps, half_width_steps)| Pose {
                center: RenderPoint {
                    row: row as f64 + 0.5,
                    col: col as f64 + 0.5,
                },
                half_height: f64::from(half_height_steps) / 8.0,
                half_width: f64::from(half_width_steps) / 8.0,
            })
            .boxed()
    }

    fn tail_band_case() -> BoxedStrategy<TailBand> {
        prop_oneof![
            Just(TailBand::Sheath),
            Just(TailBand::Core),
            Just(TailBand::Filament),
        ]
        .boxed()
    }

    fn supported_simulation_hz() -> BoxedStrategy<f64> {
        prop_oneof![
            Just(48.0),
            Just(60.0),
            Just(72.0),
            Just(90.0),
            Just(120.0),
            Just(144.0),
        ]
        .boxed()
    }

    fn doubled_simulation_hz_pair() -> BoxedStrategy<(f64, f64)> {
        prop_oneof![
            Just((48.0, 96.0)),
            Just((60.0, 120.0)),
            Just((72.0, 144.0)),
            Just((90.0, 180.0)),
        ]
        .boxed()
    }

    fn latent_history_fixture() -> BoxedStrategy<LatentHistoryFixture> {
        (
            vec(
                (
                    aligned_pose(),
                    aligned_pose(),
                    1_usize..=12_usize,
                    supported_simulation_hz(),
                    0.25_f64..=1.0_f64,
                    0.5_f64..=1.5_f64,
                    0.5_f64..=1.5_f64,
                    tail_band_case(),
                ),
                1..=6,
            ),
            -2_i64..=2_i64,
            -2_i64..=2_i64,
            -2_i64..=2_i64,
            -2_i64..=2_i64,
        )
            .prop_map(
                |(slice_specs, min_row_delta, max_row_delta, min_col_delta, max_col_delta)| {
                    let mut history = VecDeque::with_capacity(slice_specs.len());
                    let mut overall_bounds = None::<CellRect>;

                    for (
                        index,
                        (
                            start,
                            end,
                            support_steps,
                            simulation_hz,
                            intensity,
                            thickness_y,
                            thickness_x,
                            band,
                        ),
                    ) in slice_specs.into_iter().enumerate()
                    {
                        let microtiles =
                            deposit_swept_occupancy(start, end, 2.0, thickness_y, thickness_x);
                        let bbox = CellRect::from_microtiles(&microtiles)
                            .expect("aligned poses with positive thickness should materialize");
                        overall_bounds = Some(match overall_bounds {
                            Some(existing) => CellRect::new(
                                existing.min_row.min(bbox.min_row),
                                existing.max_row.max(bbox.max_row),
                                existing.min_col.min(bbox.min_col),
                                existing.max_col.max(bbox.max_col),
                            ),
                            None => bbox,
                        });

                        history.push_back(DepositedSlice {
                            stroke_id: StrokeId::new(u64::try_from(index + 1).unwrap_or(u64::MAX)),
                            step_index: StepIndex::new(
                                u64::try_from(index + 1).unwrap_or(u64::MAX),
                            ),
                            dt_ms_q16: q16_from_non_negative(simulation_step_ms(simulation_hz)),
                            arc_len_q16: ArcLenQ16::ZERO,
                            bbox,
                            band,
                            support_steps,
                            intensity_q16: intensity_q16(intensity),
                            microtiles,
                        });
                    }

                    let overall_bounds = overall_bounds
                        .expect("fixture generation should produce at least one slice");
                    let mut min_row = overall_bounds.min_row + min_row_delta;
                    let mut max_row = overall_bounds.max_row + max_row_delta;
                    let mut min_col = overall_bounds.min_col + min_col_delta;
                    let mut max_col = overall_bounds.max_col + max_col_delta;
                    if min_row > max_row {
                        std::mem::swap(&mut min_row, &mut max_row);
                    }
                    if min_col > max_col {
                        std::mem::swap(&mut min_col, &mut max_col);
                    }

                    LatentHistoryFixture {
                        latest_step: StepIndex::new(
                            u64::try_from(history.len()).unwrap_or(u64::MAX),
                        ),
                        history,
                        bounds: CellRect::new(min_row, max_row, min_col, max_col),
                    }
                },
            )
            .boxed()
    }

    fn compiled_cell_for_age(age_steps: u64, support_steps: usize) -> Option<CompiledCell> {
        let latest_step = StepIndex::new(100_u64 + age_steps);
        let mut history = VecDeque::new();
        history.push_back(DepositedSlice {
            stroke_id: StrokeId::new(1),
            step_index: StepIndex::new(100),
            dt_ms_q16: q16_from_non_negative(simulation_step_ms(120.0)),
            arc_len_q16: ArcLenQ16::ZERO,
            bbox: CellRect::new(4, 4, 5, 5),
            band: TailBand::Core,
            support_steps,
            intensity_q16: intensity_q16(1.0),
            microtiles: BTreeMap::from([((4_i64, 5_i64), fully_covered_tile())]),
        });
        compile_field(&history, latest_step)
            .get(&(4_i64, 5_i64))
            .copied()
    }

    fn stationary_history_for_rate(
        simulation_hz: f64,
        support_duration_ms: f64,
    ) -> (VecDeque<DepositedSlice>, StepIndex) {
        let support_steps = tail_support_steps(support_duration_ms, simulation_hz);
        let latest_step = StepIndex::new(u64::try_from(support_steps).unwrap_or(u64::MAX));
        let dt_ms_q16 = q16_from_non_negative(simulation_step_ms(simulation_hz));
        let tile = fully_covered_tile();
        let bbox = CellRect::new(4, 4, 5, 5);
        let history = (1..=support_steps)
            .map(|step| DepositedSlice {
                stroke_id: StrokeId::new(1),
                step_index: StepIndex::new(u64::try_from(step).unwrap_or(u64::MAX)),
                dt_ms_q16,
                arc_len_q16: ArcLenQ16::ZERO,
                bbox,
                band: TailBand::Core,
                support_steps,
                intensity_q16: intensity_q16(1.0),
                microtiles: BTreeMap::from([((4_i64, 5_i64), tile)]),
            })
            .collect::<VecDeque<_>>();
        (history, latest_step)
    }

    fn mass_similarity_holds(lhs: u32, rhs: u32, tolerance_divisor: u64) -> bool {
        let diff = u64::from(lhs.abs_diff(rhs));
        let tolerance = (u64::from(lhs.max(rhs)) / tolerance_divisor).max(1);
        diff <= tolerance
    }

    fn peak_similarity_holds(lhs: u16, rhs: u16, tolerance_divisor: u16) -> bool {
        let diff = lhs.abs_diff(rhs);
        let tolerance = (lhs.max(rhs) / tolerance_divisor).max(1);
        diff <= tolerance
    }

    proptest! {
        #![proptest_config(pure_config())]

        #[test]
        fn prop_compile_decay_matches_curve_weights_and_ages_out(
            support_steps in 1_usize..64_usize,
            age_steps in 0_u64..96_u64,
        ) {
            let tile = fully_covered_tile();
            let slice = DepositedSlice {
                stroke_id: StrokeId::new(1),
                step_index: StepIndex::new(100),
                dt_ms_q16: q16_from_non_negative(simulation_step_ms(120.0)),
                arc_len_q16: ArcLenQ16::ZERO,
                bbox: CellRect::new(4, 4, 5, 5),
                band: TailBand::Core,
                support_steps,
                intensity_q16: intensity_q16(1.0),
                microtiles: BTreeMap::from([((4_i64, 5_i64), tile)]),
            };
            let weighted_sample_q16 =
                u64::from(SAMPLE_Q12_SCALE).saturating_mul(slice_weight_q16(&slice, age_steps));
            let weighted_total_mass_q16 = u64::from(tile.total_mass_q12())
                .saturating_mul(slice_weight_q16(&slice, age_steps));
            let expected_sample_q12 =
                u16::try_from(weighted_sample_q16 / WEIGHT_Q16_SCALE).unwrap_or(u16::MAX);
            let expected_total_mass_q12 =
                u32::try_from(weighted_total_mass_q16 / WEIGHT_Q16_SCALE).unwrap_or(u32::MAX);
            let expected_recent_mass_q12 = u32::try_from(
                u64::from(tile.total_mass_q12())
                    .saturating_mul(slice_recent_weight_q16(&slice, age_steps))
                    / WEIGHT_Q16_SCALE,
            )
            .unwrap_or(u32::MAX);
            let survives = weighted_total_mass_q16
                >= u64::from(MIN_COMPILED_SAMPLE_Q12).saturating_mul(WEIGHT_Q16_SCALE)
                && expected_sample_q12 >= MIN_COMPILED_SAMPLE_Q12;
            let compiled = compiled_cell_for_age(age_steps, support_steps);

            if survives {
                let compiled = compiled.expect("non-zero weight should compile");
                prop_assert_eq!(compiled.age.total_mass_q12, expected_total_mass_q12);
                prop_assert_eq!(compiled.age.recent_mass_q12, expected_recent_mass_q12);
                prop_assert!(compiled
                    .tile
                    .samples_q12
                    .iter()
                    .copied()
                    .all(|sample| sample == expected_sample_q12));
                if age_steps == 0 {
                    prop_assert_eq!(compiled.age.recent_mass_q12, compiled.age.total_mass_q12);
                }
            } else {
                prop_assert_eq!(compiled, None);
            }
        }

        #[test]
        fn prop_cell_rect_tracks_microtile_extent(
            coords in vec((-12_i64..=12_i64, -12_i64..=12_i64), 1..=16),
        ) {
            let tiles = coords
                .into_iter()
                .map(|coord| (coord, fully_covered_tile()))
                .collect::<BTreeMap<_, _>>();
            let bbox = CellRect::from_microtiles(&tiles)
                .expect("non-empty tiles should have an enclosing bounding box");
            let mut keys = tiles.keys().copied();
            let first = keys.next().expect("tiles map should stay non-empty");
            let expected = keys.fold(
                CellRect::new(first.0, first.0, first.1, first.1),
                |bounds, (row, col)| {
                    CellRect::new(
                        bounds.min_row.min(row),
                        bounds.max_row.max(row),
                        bounds.min_col.min(col),
                        bounds.max_col.max(col),
                    )
                },
            );

            prop_assert_eq!(bbox, expected);
            for coord in tiles.keys().copied() {
                prop_assert!(bbox.contains(coord));
            }
        }

        #[test]
        fn prop_compile_field_keeps_visible_tail_intensity_similar_across_doubled_rates(
            support_steps_at_base_rate in 4_usize..24_usize,
            (base_hz, doubled_hz) in doubled_simulation_hz_pair(),
        ) {
            let support_duration_ms =
                support_steps_at_base_rate as f64 * simulation_step_ms(base_hz);
            let (history_base, latest_base) =
                stationary_history_for_rate(base_hz, support_duration_ms);
            let (history_doubled, latest_doubled) =
                stationary_history_for_rate(doubled_hz, support_duration_ms);
            let cell_base = compile_field(&history_base, latest_base)
                .get(&(4_i64, 5_i64))
                .copied()
                .expect("stationary field at the base rate should compile");
            let cell_doubled = compile_field(&history_doubled, latest_doubled)
                .get(&(4_i64, 5_i64))
                .copied()
                .expect("stationary field at the doubled rate should compile");

            prop_assert!(
                mass_similarity_holds(
                    cell_base.age.total_mass_q12,
                    cell_doubled.age.total_mass_q12,
                    10,
                ),
                "total mass diverged too far across doubled rates: base={} doubled={}",
                cell_base.age.total_mass_q12,
                cell_doubled.age.total_mass_q12,
            );
            prop_assert!(
                mass_similarity_holds(
                    cell_base.age.recent_mass_q12,
                    cell_doubled.age.recent_mass_q12,
                    10,
                ),
                "recent mass diverged too far across doubled rates: base={} doubled={}",
                cell_base.age.recent_mass_q12,
                cell_doubled.age.recent_mass_q12,
            );
            prop_assert!(
                peak_similarity_holds(cell_base.tile.max_sample_q12(), cell_doubled.tile.max_sample_q12(), 10),
                "peak intensity diverged too far across doubled rates: base={} doubled={}",
                cell_base.tile.max_sample_q12(),
                cell_doubled.tile.max_sample_q12(),
            );
        }

        #[test]
        fn prop_compile_field_cache_projection_matches_direct_replay_and_bounds(
            fixture in latent_history_fixture(),
        ) {
            let cache = LatentFieldCache::rebuild(&fixture.history, fixture.latest_step, 17);
            let direct = compile_field(&fixture.history, fixture.latest_step);
            let reference = compile_field_reference(&cache);

            prop_assert_eq!(&reference, &direct);

            let mut reference_scratch = CompileScratch::default();
            let first_reference = compile_field_reference_with_scratch(&cache, &mut reference_scratch);
            let first_reference_capacity = reference_scratch.accumulator_capacity();
            let second_reference = compile_field_reference_with_scratch(&cache, &mut reference_scratch);

            prop_assert_eq!(&first_reference, &reference);
            prop_assert_eq!(&second_reference, &reference);
            prop_assert_eq!(
                reference_scratch.accumulator_capacity(),
                first_reference_capacity,
            );

            let filtered_reference = reference
                .iter()
                .filter(|(coord, _)| fixture.bounds.contains(**coord))
                .map(|(coord, value)| (*coord, *value))
                .collect::<BTreeMap<_, _>>();
            let bounded = compile_field_in_bounds(&cache, fixture.bounds);

            prop_assert_eq!(&bounded, &filtered_reference);

            let mut bounded_scratch = CompileScratch::default();
            let first_bounded =
                compile_field_in_bounds_with_scratch(&cache, fixture.bounds, &mut bounded_scratch);
            let first_bounded_capacity = bounded_scratch.accumulator_capacity();
            let second_bounded =
                compile_field_in_bounds_with_scratch(&cache, fixture.bounds, &mut bounded_scratch);

            prop_assert_eq!(&first_bounded, &filtered_reference);
            prop_assert_eq!(&second_bounded, &filtered_reference);
            prop_assert_eq!(
                bounded_scratch.accumulator_capacity(),
                first_bounded_capacity,
            );

            let mut rows_scratch = CompileScratch::default();
            let rows =
                compile_field_in_bounds_rows_with_scratch(&cache, fixture.bounds, &mut rows_scratch);
            let rows_as_map = rows
                .iter()
                .map(|(coord, value)| (coord, *value))
                .collect::<BTreeMap<_, _>>();

            prop_assert_eq!(&rows_as_map, &filtered_reference);
        }

        #[test]
        fn prop_compile_field_discards_cells_below_compiled_visibility_threshold(
            sample_q12 in 0_u16..MIN_COMPILED_SAMPLE_Q12,
        ) {
            let tile = super::MicroTile {
                samples_q12: [sample_q12; MICRO_TILE_SAMPLES],
            };
            let history = VecDeque::from([DepositedSlice {
                stroke_id: StrokeId::new(1),
                step_index: StepIndex::new(1),
                dt_ms_q16: q16_from_non_negative(simulation_step_ms(120.0)),
                arc_len_q16: ArcLenQ16::ZERO,
                bbox: CellRect::new(4, 4, 5, 5),
                band: TailBand::Core,
                support_steps: 4,
                intensity_q16: intensity_q16(1.0),
                microtiles: BTreeMap::from([((4_i64, 5_i64), tile)]),
            }]);
            let cache = LatentFieldCache::rebuild(&history, StepIndex::new(1), 17);

            prop_assert!(compile_field(&history, StepIndex::new(1)).is_empty());
            prop_assert!(compile_field_reference(&cache).is_empty());
        }
    }
}
