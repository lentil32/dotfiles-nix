use crate::types::Point;
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
#[cfg(test)]
pub(super) use reference_compile::compile_field_in_bounds;
pub(super) use reference_compile::compile_field_in_bounds_rows_with_scratch;
pub(super) use reference_compile::compile_field_in_bounds_with_scratch;
pub(super) use reference_compile::compile_field_reference;
pub(super) use reference_compile::compile_field_reference_with_scratch;
pub(super) use spatial_index::BorrowedCellRows;
pub(super) use spatial_index::BorrowedCellRowsScratch;
pub(super) use spatial_index::CellRowQueryStats;
pub(super) use spatial_index::CellRows;
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

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Pose {
    pub(super) center: Point,
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
    use super::LatentFieldCache;
    use super::MICRO_TILE_SAMPLES;
    use super::Pose;
    use super::SAMPLE_Q12_SCALE;
    use super::TailBand;
    use super::comet_tail_profiles;
    use super::compile_field;
    use super::compile_field_in_bounds;
    use super::compile_field_in_bounds_rows_with_scratch;
    use super::compile_field_in_bounds_with_scratch;
    use super::compile_field_reference;
    use super::compile_field_reference_with_scratch;
    use super::deposit_swept_occupancy;
    use super::intensity_q16;
    use super::max_comet_support_steps;
    use super::q16_from_non_negative;
    use super::simulation_step_ms;
    use super::tail_support_steps;
    use crate::core::types::ArcLenQ16;
    use crate::core::types::StepIndex;
    use crate::core::types::StrokeId;
    use crate::types::Point;
    use pretty_assertions::assert_eq;
    use std::collections::BTreeMap;
    use std::collections::VecDeque;

    #[test]
    fn compile_field_ages_out_old_slices() {
        let pose = Pose {
            center: Point { row: 4.5, col: 5.5 },
            half_height: 0.5,
            half_width: 0.5,
        };
        let tiles = deposit_swept_occupancy(pose, pose, 2.0, 1.0, 1.0);
        let mut history = VecDeque::new();
        history.push_back(super::DepositedSlice {
            stroke_id: StrokeId::new(1),
            step_index: StepIndex::new(1),
            dt_ms_q16: q16_from_non_negative(simulation_step_ms(120.0)),
            arc_len_q16: ArcLenQ16::ZERO,
            bbox: CellRect::new(4, 4, 5, 5),
            band: TailBand::Core,
            support_steps: 4,
            intensity_q16: intensity_q16(1.0),
            microtiles: tiles,
        });

        let empty = compile_field(&history, StepIndex::new(12));
        assert!(empty.is_empty());
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

    fn compiled_cell_for_age(age_steps: u64, support_steps: usize) -> Option<super::CompiledCell> {
        let latest_step = StepIndex::new(100_u64 + age_steps);
        let mut history = VecDeque::new();
        history.push_back(super::DepositedSlice {
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

    #[test]
    fn compile_decay_matches_normalized_age_endpoints_and_midpoint() {
        let support_steps = 10_usize;

        let head = compiled_cell_for_age(0, support_steps).expect("a=0 should contribute");
        let midpoint =
            compiled_cell_for_age(5, support_steps).expect("a=0.5 should still contribute");
        let tip = compiled_cell_for_age(10, support_steps);

        assert_eq!(head.age.recent_mass_q12, head.age.total_mass_q12);
        assert!(midpoint.age.total_mass_q12 < head.age.total_mass_q12);
        assert!(
            midpoint.age.total_mass_q12 > head.age.total_mass_q12 / 2,
            "midpoint mass should reflect head_weight=1-a, not fixed head window"
        );
        assert!(
            u64::from(midpoint.age.recent_mass_q12).saturating_mul(5)
                >= u64::from(midpoint.age.total_mass_q12).saturating_mul(4),
            "midpoint recent mass should stay close to total mass"
        );
        assert!(tip.is_none(), "a=1 should fully age out");
    }

    #[test]
    fn cell_rect_tracks_microtile_extent() {
        let tiles = BTreeMap::from([
            ((4_i64, 5_i64), fully_covered_tile()),
            ((6_i64, 9_i64), fully_covered_tile()),
            ((5_i64, 7_i64), fully_covered_tile()),
        ]);

        let bbox = CellRect::from_microtiles(&tiles).expect("bbox should exist for non-empty map");
        assert_eq!(bbox, CellRect::new(4, 6, 5, 9));
        assert!(bbox.contains((5, 7)));
        assert!(!bbox.contains((7, 7)));
    }

    fn stationary_history_for_rate(
        simulation_hz: f64,
        support_duration_ms: f64,
    ) -> (VecDeque<super::DepositedSlice>, StepIndex) {
        let support_steps = tail_support_steps(support_duration_ms, simulation_hz);
        let latest_step = StepIndex::new(u64::try_from(support_steps).unwrap_or(u64::MAX));
        let dt_ms_q16 = q16_from_non_negative(simulation_step_ms(simulation_hz));
        let tile = fully_covered_tile();
        let bbox = CellRect::new(4, 4, 5, 5);
        let history = (1..=support_steps)
            .map(|step| super::DepositedSlice {
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

    #[test]
    fn compile_field_scales_slice_weight_by_dt_for_rate_stability() {
        let support_duration_ms = 180.0;
        let (history_60hz, latest_60hz) = stationary_history_for_rate(60.0, support_duration_ms);
        let (history_120hz, latest_120hz) = stationary_history_for_rate(120.0, support_duration_ms);

        let cell_60hz = compile_field(&history_60hz, latest_60hz)
            .get(&(4_i64, 5_i64))
            .copied()
            .expect("60Hz history should compile");
        let cell_120hz = compile_field(&history_120hz, latest_120hz)
            .get(&(4_i64, 5_i64))
            .copied()
            .expect("120Hz history should compile");

        let lhs = i64::from(cell_60hz.age.total_mass_q12);
        let rhs = i64::from(cell_120hz.age.total_mass_q12);
        let diff = (lhs - rhs).abs();
        let tolerance = (lhs.max(rhs) / 12).max(1);
        assert!(
            diff <= tolerance,
            "dt scaling should keep total mass stable across rates: lhs={lhs} rhs={rhs} diff={diff} tolerance={tolerance}"
        );
    }

    #[test]
    fn compile_field_keeps_visible_tail_intensity_similar_across_60hz_and_120hz() {
        let support_duration_ms = 180.0;
        let (history_60hz, latest_60hz) = stationary_history_for_rate(60.0, support_duration_ms);
        let (history_120hz, latest_120hz) = stationary_history_for_rate(120.0, support_duration_ms);

        let cell_60hz = compile_field(&history_60hz, latest_60hz)
            .get(&(4_i64, 5_i64))
            .copied()
            .expect("60Hz history should compile");
        let cell_120hz = compile_field(&history_120hz, latest_120hz)
            .get(&(4_i64, 5_i64))
            .copied()
            .expect("120Hz history should compile");

        let total_diff = i64::from(cell_60hz.age.total_mass_q12)
            .abs_diff(i64::from(cell_120hz.age.total_mass_q12));
        let total_tolerance = (u64::from(
            cell_60hz
                .age
                .total_mass_q12
                .max(cell_120hz.age.total_mass_q12),
        ) / 12)
            .max(1);
        assert!(
            total_diff <= total_tolerance,
            "visible total mass should stay similar across rates: 60Hz={} 120Hz={} diff={} tolerance={}",
            cell_60hz.age.total_mass_q12,
            cell_120hz.age.total_mass_q12,
            total_diff,
            total_tolerance,
        );

        let recent_diff = i64::from(cell_60hz.age.recent_mass_q12)
            .abs_diff(i64::from(cell_120hz.age.recent_mass_q12));
        let recent_tolerance = (u64::from(
            cell_60hz
                .age
                .recent_mass_q12
                .max(cell_120hz.age.recent_mass_q12),
        ) / 12)
            .max(1);
        assert!(
            recent_diff <= recent_tolerance,
            "visible recent mass should stay similar across rates: 60Hz={} 120Hz={} diff={} tolerance={}",
            cell_60hz.age.recent_mass_q12,
            cell_120hz.age.recent_mass_q12,
            recent_diff,
            recent_tolerance,
        );

        let peak_diff = cell_60hz
            .tile
            .max_sample_q12()
            .abs_diff(cell_120hz.tile.max_sample_q12());
        let peak_tolerance = (u16::max(
            cell_60hz.tile.max_sample_q12(),
            cell_120hz.tile.max_sample_q12(),
        ) / 12)
            .max(1);
        assert!(
            peak_diff <= peak_tolerance,
            "visible peak intensity should stay similar across rates: 60Hz={} 120Hz={} diff={} tolerance={}",
            cell_60hz.tile.max_sample_q12(),
            cell_120hz.tile.max_sample_q12(),
            peak_diff,
            peak_tolerance,
        );
    }

    #[test]
    fn compile_field_reference_matches_direct_replay() {
        let support_duration_ms = 180.0;
        let (history, latest_step) = stationary_history_for_rate(120.0, support_duration_ms);
        let cache = LatentFieldCache::rebuild(&history, latest_step, 7);

        assert_eq!(
            compile_field_reference(&cache),
            compile_field(&history, latest_step)
        );
    }

    #[test]
    fn compile_field_reference_with_scratch_reuses_accumulator_capacity() {
        let support_duration_ms = 180.0;
        let (history, latest_step) = stationary_history_for_rate(120.0, support_duration_ms);
        let cache = LatentFieldCache::rebuild(&history, latest_step, 7);
        let mut scratch = super::CompileScratch::default();

        let first = compile_field_reference_with_scratch(&cache, &mut scratch);
        let first_capacity = scratch.accumulator_capacity();
        let second = compile_field_reference_with_scratch(&cache, &mut scratch);

        assert_eq!(first, second);
        assert!(
            first_capacity > 0,
            "first compile should reserve accumulator storage for retained cells"
        );
        assert_eq!(
            scratch.accumulator_capacity(),
            first_capacity,
            "scratch-backed recompiles should keep the existing accumulator allocation"
        );
    }

    #[test]
    fn compile_field_in_bounds_matches_reference_filtered_to_same_rect() {
        let support_duration_ms = 180.0;
        let (history, latest_step) = stationary_history_for_rate(120.0, support_duration_ms);
        let cache = LatentFieldCache::rebuild(&history, latest_step, 7);
        let bounds = CellRect::new(4, 5, 5, 6);

        let bounded = compile_field_in_bounds(&cache, bounds);
        let filtered_reference = compile_field_reference(&cache)
            .into_iter()
            .filter(|(coord, _)| bounds.contains(*coord))
            .collect::<BTreeMap<_, _>>();

        assert_eq!(bounded, filtered_reference);
    }

    #[test]
    fn compile_field_in_bounds_with_scratch_reuses_accumulator_capacity() {
        let support_duration_ms = 180.0;
        let (history, latest_step) = stationary_history_for_rate(120.0, support_duration_ms);
        let cache = LatentFieldCache::rebuild(&history, latest_step, 7);
        let bounds = CellRect::new(4, 5, 5, 6);
        let mut scratch = super::CompileScratch::default();

        let first = compile_field_in_bounds_with_scratch(&cache, bounds, &mut scratch);
        let first_capacity = scratch.accumulator_capacity();
        let second = compile_field_in_bounds_with_scratch(&cache, bounds, &mut scratch);

        assert_eq!(first, second);
        assert!(
            first_capacity > 0,
            "bounded compile should reserve accumulator storage for the queried cells"
        );
        assert_eq!(
            scratch.accumulator_capacity(),
            first_capacity,
            "bounded scratch-backed recompiles should keep the existing accumulator allocation"
        );
    }

    #[test]
    fn compile_field_in_bounds_rows_matches_reference_filtered_to_same_rect() {
        let support_duration_ms = 180.0;
        let (history, latest_step) = stationary_history_for_rate(120.0, support_duration_ms);
        let cache = LatentFieldCache::rebuild(&history, latest_step, 7);
        let bounds = CellRect::new(4, 5, 5, 6);
        let mut scratch = super::CompileScratch::default();

        let rows = compile_field_in_bounds_rows_with_scratch(&cache, bounds, &mut scratch);
        let filtered_reference = compile_field_reference(&cache)
            .into_iter()
            .filter(|(coord, _)| bounds.contains(*coord))
            .collect::<BTreeMap<_, _>>();
        let rows_as_map = rows
            .iter()
            .map(|(coord, value)| (coord, *value))
            .collect::<BTreeMap<_, _>>();

        assert_eq!(rows_as_map, filtered_reference);
    }

    #[test]
    fn compile_field_discards_cells_below_compiled_visibility_threshold() {
        let mut tile = super::MicroTile::default();
        tile.samples_q12[0] = 3;

        let history = VecDeque::from([super::DepositedSlice {
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

        assert!(compile_field(&history, StepIndex::new(1)).is_empty());
        assert!(compile_field_reference(&cache).is_empty());
    }
}
