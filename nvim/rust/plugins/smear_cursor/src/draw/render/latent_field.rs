use crate::core::types::{ArcLenQ16, StepIndex, StrokeId};
use crate::types::{BASE_TIME_INTERVAL, Point};
use std::collections::{BTreeMap, HashMap, VecDeque};

pub(super) const MICRO_W: usize = 8;
pub(super) const MICRO_H: usize = 8;
pub(super) const MICRO_TILE_SAMPLES: usize = MICRO_W * MICRO_H;

const SAMPLE_Q12_SCALE: u32 = 4095;
const WEIGHT_Q16_SCALE: u64 = 65_536;
const MIN_COMPILED_SAMPLE_Q12: u16 = 4;
const EPSILON: f64 = 1.0e-9;
const DEFAULT_TAIL_DURATION_MS: f64 = 198.0;
const DURATION_SCALE_MIN: f64 = 0.40;
const DURATION_SCALE_MAX: f64 = 2.50;
const DURATION_SCALE_EXPONENT: f64 = 0.85;
const SHEATH_BASE_LIFETIME_MS: f64 = 40.0;
const CORE_BASE_LIFETIME_MS: f64 = 112.0;
const FILAMENT_BASE_LIFETIME_MS: f64 = 252.0;
const SHEATH_MIN_SUPPORT_STEPS: usize = 2;
const CORE_MIN_SUPPORT_STEPS: usize = 4;
const FILAMENT_MIN_SUPPORT_STEPS: usize = 7;
const SHEATH_WIDTH_SCALE: f64 = 1.18;
const CORE_WIDTH_SCALE: f64 = 0.58;
const FILAMENT_WIDTH_SCALE: f64 = 0.28;
const SHEATH_INTENSITY: f64 = 0.90;
const CORE_INTENSITY: f64 = 0.80;
const FILAMENT_INTENSITY: f64 = 0.78;
const TAIL_WEIGHT_EXPONENT: f64 = 0.90;
const COMBINED_HEAD_MIX: f64 = 0.20;
const COMBINED_TAIL_MIX: f64 = 0.80;
const RECENT_HEAD_MIX: f64 = 0.82;
const RECENT_TAIL_MIX: f64 = 0.18;

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

#[derive(Clone, Debug, PartialEq)]
pub(super) struct DepositedSlice {
    pub(super) stroke_id: StrokeId,
    pub(super) step_index: StepIndex,
    pub(super) dt_ms_q16: u32,
    pub(super) arc_len_q16: ArcLenQ16,
    pub(super) bbox: CellRect,
    pub(super) band: TailBand,
    pub(super) support_steps: usize,
    pub(super) intensity_q16: u32,
    pub(super) microtiles: BTreeMap<(i64, i64), MicroTile>,
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

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct WeightCurveKey {
    support_steps: usize,
    intensity_q16: u32,
    dt_ms_q16: u32,
}

impl WeightCurveKey {
    fn from_slice(slice: &DepositedSlice) -> Self {
        Self {
            support_steps: slice.support_steps.max(1),
            intensity_q16: slice.intensity_q16,
            dt_ms_q16: slice.dt_ms_q16,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct AccumulatorCell {
    weighted_samples: [u64; MICRO_TILE_SAMPLES],
    weighted_total_mass: u64,
    weighted_recent_mass: u64,
}

impl Default for AccumulatorCell {
    fn default() -> Self {
        Self {
            weighted_samples: [0_u64; MICRO_TILE_SAMPLES],
            weighted_total_mass: 0,
            weighted_recent_mass: 0,
        }
    }
}

impl AccumulatorCell {
    fn survives(self) -> bool {
        self.weighted_total_mass
            >= u64::from(MIN_COMPILED_SAMPLE_Q12).saturating_mul(WEIGHT_Q16_SCALE)
    }
}

#[derive(Debug, Default)]
pub(super) struct CompileScratch {
    accum: HashMap<(i64, i64), AccumulatorCell>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct BucketCell {
    // Each curve/age bucket currently receives at most one slice per render step, so `u32`
    // leaves ample headroom while cutting the retained latent-cache footprint roughly in half.
    samples_q12_sum: [u32; MICRO_TILE_SAMPLES],
    total_mass_q12: u32,
}

impl BucketCell {
    fn add_tile(&mut self, tile: &MicroTile) {
        let mut total_mass_q12 = 0_u32;
        for (index, sample) in tile.samples_q12.iter().copied().enumerate() {
            let sample_u32 = u32::from(sample);
            self.samples_q12_sum[index] = self.samples_q12_sum[index].saturating_add(sample_u32);
            total_mass_q12 = total_mass_q12.saturating_add(sample_u32);
        }
        self.total_mass_q12 = self.total_mass_q12.saturating_add(total_mass_q12);
    }

    fn remove_tile(&mut self, tile: &MicroTile) {
        let mut total_mass_q12 = 0_u32;
        for (index, sample) in tile.samples_q12.iter().copied().enumerate() {
            let sample_u32 = u32::from(sample);
            // removal should mirror prior insertion exactly; saturating arithmetic keeps the
            // derived cache non-panicking if that invariant is ever violated.
            debug_assert!(self.samples_q12_sum[index] >= sample_u32);
            self.samples_q12_sum[index] = self.samples_q12_sum[index].saturating_sub(sample_u32);
            total_mass_q12 = total_mass_q12.saturating_add(sample_u32);
        }
        debug_assert!(self.total_mass_q12 >= total_mass_q12);
        self.total_mass_q12 = self.total_mass_q12.saturating_sub(total_mass_q12);
    }

    fn is_empty(&self) -> bool {
        self.total_mass_q12 == 0
    }
}

impl Default for BucketCell {
    fn default() -> Self {
        Self {
            samples_q12_sum: [0_u32; MICRO_TILE_SAMPLES],
            total_mass_q12: 0,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct WeightCurveBuckets {
    age_zero_slot: usize,
    buckets: Vec<BTreeMap<(i64, i64), BucketCell>>,
}

impl WeightCurveBuckets {
    fn with_support_steps(support_steps: usize) -> Self {
        Self {
            age_zero_slot: 0,
            buckets: vec![BTreeMap::new(); support_steps.max(1)],
        }
    }

    fn bucket_index(&self, age_steps: u64) -> Option<usize> {
        let age_index = usize::try_from(age_steps).ok()?;
        if age_index >= self.buckets.len() {
            return None;
        }
        Some((self.age_zero_slot + age_index) % self.buckets.len())
    }

    fn bucket_for_age(&self, age_steps: u64) -> Option<&BTreeMap<(i64, i64), BucketCell>> {
        let index = self.bucket_index(age_steps)?;
        self.buckets.get(index)
    }

    fn bucket_for_age_mut(
        &mut self,
        age_steps: u64,
    ) -> Option<&mut BTreeMap<(i64, i64), BucketCell>> {
        let index = self.bucket_index(age_steps)?;
        self.buckets.get_mut(index)
    }

    fn advance_steps(&mut self, step_delta: u64) {
        if self.buckets.is_empty() || step_delta == 0 {
            return;
        }

        let len = self.buckets.len();
        let len_u64 = u64::try_from(len).unwrap_or(u64::MAX);
        if step_delta >= len_u64 {
            self.clear_all();
            return;
        }

        let rotate = usize::try_from(step_delta).unwrap_or(len);
        self.age_zero_slot = (self.age_zero_slot + len - (rotate % len)) % len;
        for age_index in 0..rotate {
            let slot = (self.age_zero_slot + age_index) % len;
            self.buckets[slot].clear();
        }
    }

    fn clear_all(&mut self) {
        self.age_zero_slot = 0;
        for bucket in &mut self.buckets {
            bucket.clear();
        }
    }

    fn is_empty(&self) -> bool {
        self.buckets.iter().all(BTreeMap::is_empty)
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct LatentFieldCache {
    latest_step: StepIndex,
    revision: u64,
    curves: BTreeMap<WeightCurveKey, WeightCurveBuckets>,
}

impl LatentFieldCache {
    pub(super) const fn latest_step(&self) -> StepIndex {
        self.latest_step
    }

    pub(super) const fn revision(&self) -> u64 {
        self.revision
    }

    #[cfg(test)]
    pub(super) fn rebuild(
        history: &VecDeque<DepositedSlice>,
        latest_step: StepIndex,
        revision: u64,
    ) -> Self {
        let mut cache = Self {
            latest_step,
            revision,
            curves: BTreeMap::new(),
        };
        for slice in history {
            cache.insert_slice(slice);
        }
        cache.revision = revision;
        cache
    }

    pub(super) fn advance_to(&mut self, latest_step: StepIndex) {
        let step_delta = latest_step.value().saturating_sub(self.latest_step.value());
        if step_delta == 0 {
            return;
        }

        for buckets in self.curves.values_mut() {
            buckets.advance_steps(step_delta);
        }
        self.curves.retain(|_, buckets| !buckets.is_empty());
        self.latest_step = latest_step;
    }

    pub(super) fn insert_slice(&mut self, slice: &DepositedSlice) {
        let key = WeightCurveKey::from_slice(slice);
        let age_steps = self
            .latest_step
            .value()
            .saturating_sub(slice.step_index.value());
        let buckets = self
            .curves
            .entry(key)
            .or_insert_with(|| WeightCurveBuckets::with_support_steps(key.support_steps));
        let Some(bucket) = buckets.bucket_for_age_mut(age_steps) else {
            return;
        };

        for (coord, tile) in &slice.microtiles {
            bucket.entry(*coord).or_default().add_tile(tile);
        }
        self.revision = self.revision.saturating_add(1);
    }

    #[cfg(test)]
    pub(super) fn remove_slice(&mut self, slice: &DepositedSlice) {
        let key = WeightCurveKey::from_slice(slice);
        let age_steps = self
            .latest_step
            .value()
            .saturating_sub(slice.step_index.value());
        let mut changed = false;
        let mut remove_curve = false;

        if let Some(buckets) = self.curves.get_mut(&key) {
            if let Some(bucket) = buckets.bucket_for_age_mut(age_steps) {
                for (coord, tile) in &slice.microtiles {
                    let remove_cell = bucket.get_mut(coord).is_some_and(|cell| {
                        cell.remove_tile(tile);
                        cell.is_empty()
                    });
                    changed |= remove_cell || bucket.contains_key(coord);
                    if remove_cell {
                        let _ = bucket.remove(coord);
                    }
                }
            }
            remove_curve = buckets.is_empty();
        }

        if remove_curve {
            let _ = self.curves.remove(&key);
        }
        if changed {
            self.revision = self.revision.saturating_add(1);
        }
    }
}

fn axis_interval(sample: f64, start: f64, end: f64, half_extent: f64) -> Option<(f64, f64)> {
    if !sample.is_finite() || !start.is_finite() || !end.is_finite() || !half_extent.is_finite() {
        return None;
    }
    if half_extent <= 0.0 {
        return None;
    }

    let delta = end - start;
    if delta.abs() <= EPSILON {
        if (sample - start).abs() <= half_extent {
            return Some((0.0, 1.0));
        }
        return None;
    }

    let left = (sample - half_extent - start) / delta;
    let right = (sample + half_extent - start) / delta;
    let lo = left.min(right).clamp(0.0, 1.0);
    let hi = left.max(right).clamp(0.0, 1.0);
    if hi <= lo {
        return None;
    }
    Some((lo, hi))
}

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

fn axis_intervals_for_cells<const N: usize>(
    min_cell: i64,
    max_cell: i64,
    sample_scale: f64,
    start: f64,
    end: f64,
    half_extent: f64,
) -> Vec<(i64, [Option<(f64, f64)>; N])> {
    let cell_count =
        usize::try_from(max_cell.saturating_sub(min_cell).saturating_add(1)).unwrap_or_default();
    let mut cells = Vec::with_capacity(cell_count);

    for cell in min_cell..=max_cell {
        let mut intervals = [None; N];
        let mut any_coverage = false;
        for sample_index in 0..N {
            let sample = (cell as f64 + sample_offset(sample_index, N)) * sample_scale;
            let interval = axis_interval(sample, start, end, half_extent);
            any_coverage |= interval.is_some();
            intervals[sample_index] = interval;
        }

        if any_coverage {
            cells.push((cell, intervals));
        }
    }

    cells
}

pub(super) fn tail_support_steps(tail_duration_ms: f64, simulation_hz: f64) -> usize {
    let safe_duration_ms = if tail_duration_ms.is_finite() {
        tail_duration_ms.max(1.0)
    } else {
        180.0
    };
    let step_ms = simulation_step_ms(simulation_hz);

    ((safe_duration_ms / step_ms).round() as usize).max(1)
}

pub(super) fn simulation_step_ms(simulation_hz: f64) -> f64 {
    let safe_hz = if simulation_hz.is_finite() {
        simulation_hz.max(1.0)
    } else {
        120.0
    };
    1000.0 / safe_hz
}

pub(super) fn q16_from_non_negative(value: f64) -> u32 {
    if !value.is_finite() {
        return 0;
    }

    let scaled = (value.max(0.0) * f64::from(1_u32 << 16)).round();
    scaled.clamp(0.0, f64::from(u32::MAX)) as u32
}

fn reference_step_weight_q16(dt_ms_q16: u32) -> u64 {
    let reference_dt_q16 = u64::from(q16_from_non_negative(BASE_TIME_INTERVAL));
    if reference_dt_q16 == 0 {
        return WEIGHT_Q16_SCALE;
    }

    u64::from(dt_ms_q16)
        .saturating_mul(WEIGHT_Q16_SCALE)
        .saturating_div(reference_dt_q16)
}

fn scale_weight_by_step_dt(weight_q16: u64, dt_ms_q16: u32) -> u64 {
    let dt_weight_q16 = reference_step_weight_q16(dt_ms_q16);
    let scaled = u128::from(weight_q16)
        .saturating_mul(u128::from(dt_weight_q16))
        .saturating_div(u128::from(WEIGHT_Q16_SCALE));
    scaled.min(u128::from(u64::MAX)) as u64
}

pub(super) fn intensity_q16(intensity: f64) -> u32 {
    if !intensity.is_finite() {
        return 0;
    }
    (intensity.clamp(0.0, 1.0) * WEIGHT_Q16_SCALE as f64).round() as u32
}

fn smoothstep01(value: f64) -> f64 {
    let x = value.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

fn band_intensity_factor(intensity_q16: u32) -> f64 {
    (f64::from(intensity_q16) / WEIGHT_Q16_SCALE as f64).max(0.0)
}

fn age_weights(age_steps: u64, support_steps: usize) -> Option<(f64, f64)> {
    let support_steps_u64 = u64::try_from(support_steps).unwrap_or(u64::MAX).max(1);
    if age_steps >= support_steps_u64 {
        return None;
    }

    let normalized_age = age_steps as f64 / support_steps_u64 as f64;
    let age = normalized_age.clamp(0.0, 1.0);
    let head_weight = (1.0 - age).clamp(0.0, 1.0);
    // Keep intensity decay smoother near support expiry to reduce one-frame aging pops.
    let age_smooth = smoothstep01(age);
    let tail_weight = (1.0 - age_smooth).powf(TAIL_WEIGHT_EXPONENT);
    Some((head_weight, tail_weight))
}

fn weight_q16_for_curve(key: WeightCurveKey, age_steps: u64) -> u64 {
    let Some((head_weight, tail_weight)) = age_weights(age_steps, key.support_steps) else {
        return 0;
    };

    let combined_weight = ((COMBINED_HEAD_MIX * head_weight + COMBINED_TAIL_MIX * tail_weight)
        * band_intensity_factor(key.intensity_q16))
    .clamp(0.0, 1.0);
    scale_weight_by_step_dt(
        (combined_weight * WEIGHT_Q16_SCALE as f64).round() as u64,
        key.dt_ms_q16,
    )
}

fn recent_weight_q16_for_curve(key: WeightCurveKey, age_steps: u64) -> u64 {
    let Some((head_weight, tail_weight)) = age_weights(age_steps, key.support_steps) else {
        return 0;
    };

    let recent_weight = ((RECENT_HEAD_MIX * head_weight + RECENT_TAIL_MIX * tail_weight)
        * band_intensity_factor(key.intensity_q16))
    .clamp(0.0, 1.0);
    scale_weight_by_step_dt(
        (recent_weight * WEIGHT_Q16_SCALE as f64).round() as u64,
        key.dt_ms_q16,
    )
}

#[cfg(test)]
pub(super) fn slice_weight_q16(slice: &DepositedSlice, age_steps: u64) -> u64 {
    weight_q16_for_curve(WeightCurveKey::from_slice(slice), age_steps)
}

#[cfg(test)]
pub(super) fn slice_recent_weight_q16(slice: &DepositedSlice, age_steps: u64) -> u64 {
    recent_weight_q16_for_curve(WeightCurveKey::from_slice(slice), age_steps)
}

pub(super) fn comet_tail_profiles(tail_duration_ms: f64) -> [TailBandProfile; 3] {
    let duration_ratio = if tail_duration_ms.is_finite() {
        (tail_duration_ms / DEFAULT_TAIL_DURATION_MS).clamp(DURATION_SCALE_MIN, DURATION_SCALE_MAX)
    } else {
        1.0
    };
    let support_scale = duration_ratio.powf(DURATION_SCALE_EXPONENT);

    [
        TailBandProfile {
            band: TailBand::Sheath,
            width_scale: SHEATH_WIDTH_SCALE,
            lifetime_ms: SHEATH_BASE_LIFETIME_MS * support_scale,
            min_support_steps: SHEATH_MIN_SUPPORT_STEPS,
            intensity: SHEATH_INTENSITY,
        },
        TailBandProfile {
            band: TailBand::Core,
            width_scale: CORE_WIDTH_SCALE,
            lifetime_ms: CORE_BASE_LIFETIME_MS * support_scale,
            min_support_steps: CORE_MIN_SUPPORT_STEPS,
            intensity: CORE_INTENSITY,
        },
        TailBandProfile {
            band: TailBand::Filament,
            width_scale: FILAMENT_WIDTH_SCALE,
            lifetime_ms: FILAMENT_BASE_LIFETIME_MS * support_scale,
            min_support_steps: FILAMENT_MIN_SUPPORT_STEPS,
            intensity: FILAMENT_INTENSITY,
        },
    ]
}

pub(super) fn max_comet_support_steps(tail_duration_ms: f64, simulation_hz: f64) -> usize {
    comet_tail_profiles(tail_duration_ms)
        .into_iter()
        .map(|profile| profile.support_steps(simulation_hz))
        .max()
        .unwrap_or(1)
}

pub(super) fn deposit_swept_occupancy(
    start: Pose,
    end: Pose,
    block_aspect_ratio: f64,
    thickness_y: f64,
    thickness_x: f64,
) -> BTreeMap<(i64, i64), MicroTile> {
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
    let half_width = (start.half_width.max(end.half_width) * width_scale).max(EPSILON);
    let half_height = (start.half_height.max(end.half_height) * height_scale).max(EPSILON);

    let Some((min_row, max_row)) =
        sample_center_cell_span(start.center.row, end.center.row, half_height, MICRO_H)
    else {
        return BTreeMap::new();
    };
    let Some((min_col, max_col)) =
        sample_center_cell_span(start.center.col, end.center.col, half_width, MICRO_W)
    else {
        return BTreeMap::new();
    };

    let row_intervals = axis_intervals_for_cells::<MICRO_H>(
        min_row,
        max_row,
        safe_aspect_ratio,
        start.center.row * safe_aspect_ratio,
        end.center.row * safe_aspect_ratio,
        half_height * safe_aspect_ratio,
    );
    let col_intervals = axis_intervals_for_cells::<MICRO_W>(
        min_col,
        max_col,
        1.0,
        start.center.col,
        end.center.col,
        half_width,
    );
    if row_intervals.is_empty() || col_intervals.is_empty() {
        return BTreeMap::new();
    }

    let mut tiles = BTreeMap::<(i64, i64), MicroTile>::new();

    for (row, row_intervals) in &row_intervals {
        for (col, col_intervals) in &col_intervals {
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
pub(super) fn prune_history(
    history: &mut VecDeque<DepositedSlice>,
    latest_step: StepIndex,
    support_steps: usize,
) {
    let support_steps_u64 = u64::try_from(support_steps).unwrap_or(u64::MAX);
    while history.front().is_some_and(|slice| {
        latest_step.value().saturating_sub(slice.step_index.value()) >= support_steps_u64
    }) {
        let _ = history.pop_front();
    }
}

fn accumulate_weighted_mass(
    cell: &mut AccumulatorCell,
    total_mass_q12: u32,
    weight_q16: u64,
    recent_weight_q16: u64,
) {
    cell.weighted_total_mass = cell
        .weighted_total_mass
        .saturating_add(u64::from(total_mass_q12).saturating_mul(weight_q16));
    if recent_weight_q16 > 0 {
        cell.weighted_recent_mass = cell
            .weighted_recent_mass
            .saturating_add(u64::from(total_mass_q12).saturating_mul(recent_weight_q16));
    }
}

fn accumulate_bucket_weighted_samples(
    cell: &mut AccumulatorCell,
    samples_q12_sum: &[u32; MICRO_TILE_SAMPLES],
    weight_q16: u64,
) {
    for (index, sample_sum) in samples_q12_sum.iter().copied().enumerate() {
        let weighted = u64::from(sample_sum).saturating_mul(weight_q16);
        cell.weighted_samples[index] = cell.weighted_samples[index].saturating_add(weighted);
    }
}

fn accumulate_bucket_weighted_cell(
    cell: &mut AccumulatorCell,
    bucket_cell: &BucketCell,
    weight_q16: u64,
    recent_weight_q16: u64,
) {
    accumulate_weighted_mass(
        cell,
        bucket_cell.total_mass_q12,
        weight_q16,
        recent_weight_q16,
    );
    accumulate_bucket_weighted_samples(cell, &bucket_cell.samples_q12_sum, weight_q16);
}

#[cfg(test)]
fn accumulate_microtile_weighted_samples(
    cell: &mut AccumulatorCell,
    tile: &MicroTile,
    weight_q16: u64,
) {
    for (index, sample) in tile.samples_q12.iter().copied().enumerate() {
        let weighted = u64::from(sample).saturating_mul(weight_q16);
        cell.weighted_samples[index] = cell.weighted_samples[index].saturating_add(weighted);
    }
}

fn finalize_compiled_cells(
    accum: &HashMap<(i64, i64), AccumulatorCell>,
) -> BTreeMap<(i64, i64), CompiledCell> {
    let mut compiled = BTreeMap::<(i64, i64), CompiledCell>::new();

    for (coord, cell) in accum {
        if !cell.survives() {
            continue;
        }

        let mut tile = MicroTile::default();
        for (index, weighted_sample) in cell.weighted_samples.iter().copied().enumerate() {
            let normalized = (weighted_sample / WEIGHT_Q16_SCALE).min(u64::from(SAMPLE_Q12_SCALE));
            tile.samples_q12[index] = normalized as u16;
        }

        if tile.max_sample_q12() < MIN_COMPILED_SAMPLE_Q12 {
            continue;
        }

        let total_mass_q12 =
            (cell.weighted_total_mass / WEIGHT_Q16_SCALE).min(u64::from(u32::MAX)) as u32;
        let recent_mass_q12 =
            (cell.weighted_recent_mass / WEIGHT_Q16_SCALE).min(u64::from(u32::MAX)) as u32;

        compiled.insert(
            *coord,
            CompiledCell {
                tile,
                age: AgeMoment {
                    total_mass_q12,
                    recent_mass_q12,
                },
            },
        );
    }

    compiled
}

fn for_each_weighted_bucket<F>(cache: &LatentFieldCache, mut visit: F)
where
    F: FnMut(u64, u64, &BTreeMap<(i64, i64), BucketCell>),
{
    for (key, curve) in &cache.curves {
        for age_steps in 0..curve.buckets.len() {
            let age_steps_u64 = u64::try_from(age_steps).unwrap_or(u64::MAX);
            let weight_q16 = weight_q16_for_curve(*key, age_steps_u64);
            if weight_q16 == 0 {
                continue;
            }

            let Some(bucket) = curve.bucket_for_age(age_steps_u64) else {
                continue;
            };
            visit(
                weight_q16,
                recent_weight_q16_for_curve(*key, age_steps_u64),
                bucket,
            );
        }
    }
}

pub(super) fn compile_field_from_cache(
    cache: &LatentFieldCache,
) -> BTreeMap<(i64, i64), CompiledCell> {
    let mut scratch = CompileScratch::default();
    compile_field_from_cache_with_scratch(cache, &mut scratch)
}

pub(super) fn compile_field_from_cache_with_scratch(
    cache: &LatentFieldCache,
    scratch: &mut CompileScratch,
) -> BTreeMap<(i64, i64), CompiledCell> {
    scratch.accum.clear();

    // CONTEXT: active motion invalidates the compiled revision every frame once fresh slices land.
    // Reuse the accumulator allocation and fold mass/sample accumulation into one retained-cache
    // walk so hot frames do not also pay a second bucket scan plus HashMap growth churn.
    for_each_weighted_bucket(cache, |weight_q16, recent_weight_q16, bucket| {
        for (coord, cell) in bucket {
            accumulate_bucket_weighted_cell(
                scratch.accum.entry(*coord).or_default(),
                cell,
                weight_q16,
                recent_weight_q16,
            );
        }
    });

    if scratch.accum.is_empty() {
        return BTreeMap::new();
    }

    finalize_compiled_cells(&scratch.accum)
}

#[cfg(test)]
pub(super) fn compile_field(
    history: &VecDeque<DepositedSlice>,
    latest_step: StepIndex,
) -> BTreeMap<(i64, i64), CompiledCell> {
    let mut accum = HashMap::<(i64, i64), AccumulatorCell>::new();

    for slice in history {
        let age_steps = latest_step.value().saturating_sub(slice.step_index.value());
        let weight_q16 = slice_weight_q16(slice, age_steps);
        if weight_q16 == 0 {
            continue;
        }
        let recent_weight_q16 = slice_recent_weight_q16(slice, age_steps);

        for (coord, tile) in &slice.microtiles {
            let accum_cell = accum.entry(*coord).or_default();
            accumulate_weighted_mass(
                accum_cell,
                tile.total_mass_q12(),
                weight_q16,
                recent_weight_q16,
            );
            accumulate_microtile_weighted_samples(accum_cell, tile, weight_q16);
        }
    }

    if accum.is_empty() {
        return BTreeMap::new();
    }
    finalize_compiled_cells(&accum)
}

#[cfg(test)]
mod tests {
    use super::{
        CellRect, LatentFieldCache, MICRO_TILE_SAMPLES, Pose, SAMPLE_Q12_SCALE, TailBand,
        comet_tail_profiles, compile_field, compile_field_from_cache,
        compile_field_from_cache_with_scratch, deposit_swept_occupancy, intensity_q16,
        max_comet_support_steps, prune_history, q16_from_non_negative, simulation_step_ms,
        tail_support_steps,
    };
    use crate::core::types::{ArcLenQ16, StepIndex, StrokeId};
    use crate::types::Point;
    use std::collections::{BTreeMap, VecDeque};

    #[test]
    fn deposit_static_pose_produces_non_empty_tile() {
        let pose = Pose {
            center: Point {
                row: 10.5,
                col: 20.5,
            },
            half_height: 0.5,
            half_width: 0.5,
        };
        let tiles = deposit_swept_occupancy(pose, pose, 2.0, 1.0, 1.0);

        assert!(!tiles.is_empty());
        let center = tiles.get(&(10, 20)).expect("center cell should be present");
        assert!(center.samples_q12.iter().any(|value| *value > 0));
    }

    #[test]
    fn deposit_static_pose_on_exact_cell_boundary_skips_empty_border_tiles() {
        let pose = Pose {
            center: Point {
                row: 10.5,
                col: 20.5,
            },
            half_height: 0.5,
            half_width: 0.5,
        };

        let tiles = deposit_swept_occupancy(pose, pose, 2.0, 1.0, 1.0);
        assert_eq!(tiles.keys().copied().collect::<Vec<_>>(), vec![(10, 20)]);
    }

    #[test]
    fn deposit_sweep_spans_multiple_cells() {
        let start = Pose {
            center: Point {
                row: 10.5,
                col: 10.5,
            },
            half_height: 0.5,
            half_width: 0.5,
        };
        let end = Pose {
            center: Point {
                row: 10.5,
                col: 14.5,
            },
            half_height: 0.5,
            half_width: 0.5,
        };
        let tiles = deposit_swept_occupancy(start, end, 2.0, 1.0, 1.0);
        assert!(tiles.contains_key(&(10, 10)));
        assert!(tiles.contains_key(&(10, 14)));
    }

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
    fn prune_history_keeps_recent_window() {
        let mut history = VecDeque::new();
        for step in 0_u64..10_u64 {
            history.push_back(super::DepositedSlice {
                stroke_id: StrokeId::new(1),
                step_index: StepIndex::new(step),
                dt_ms_q16: q16_from_non_negative(simulation_step_ms(120.0)),
                arc_len_q16: ArcLenQ16::ZERO,
                bbox: CellRect::new(0, 0, 0, 0),
                band: TailBand::Core,
                support_steps: 3,
                intensity_q16: intensity_q16(1.0),
                microtiles: Default::default(),
            });
        }

        prune_history(&mut history, StepIndex::new(9), 3);
        assert!(
            history
                .front()
                .is_some_and(|slice| slice.step_index.value() >= 7)
        );
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
    fn compile_field_from_cache_matches_direct_replay() {
        let support_duration_ms = 180.0;
        let (history, latest_step) = stationary_history_for_rate(120.0, support_duration_ms);
        let cache = LatentFieldCache::rebuild(&history, latest_step, 7);

        assert_eq!(
            compile_field_from_cache(&cache),
            compile_field(&history, latest_step)
        );
    }

    #[test]
    fn compile_field_from_cache_with_scratch_reuses_accumulator_capacity() {
        let support_duration_ms = 180.0;
        let (history, latest_step) = stationary_history_for_rate(120.0, support_duration_ms);
        let cache = LatentFieldCache::rebuild(&history, latest_step, 7);
        let mut scratch = super::CompileScratch::default();

        let first = compile_field_from_cache_with_scratch(&cache, &mut scratch);
        let first_capacity = scratch.accum.capacity();
        let second = compile_field_from_cache_with_scratch(&cache, &mut scratch);

        assert_eq!(first, second);
        assert!(
            first_capacity > 0,
            "first compile should reserve accumulator storage for retained cells"
        );
        assert_eq!(
            scratch.accum.capacity(),
            first_capacity,
            "scratch-backed recompiles should keep the existing accumulator allocation"
        );
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
        assert!(compile_field_from_cache(&cache).is_empty());
    }

    #[test]
    fn latent_bucket_cell_stays_compact() {
        assert!(
            std::mem::size_of::<super::BucketCell>() <= 260,
            "bucket cache cell regressed in size: {} bytes",
            std::mem::size_of::<super::BucketCell>()
        );
    }

    #[test]
    fn latent_cache_incremental_updates_match_direct_replay() {
        let tile = fully_covered_tile();
        let bbox = CellRect::new(4, 4, 5, 5);
        let make_slice = |step: u64| super::DepositedSlice {
            stroke_id: StrokeId::new(1),
            step_index: StepIndex::new(step),
            dt_ms_q16: q16_from_non_negative(simulation_step_ms(120.0)),
            arc_len_q16: ArcLenQ16::ZERO,
            bbox,
            band: TailBand::Core,
            support_steps: 4,
            intensity_q16: intensity_q16(1.0),
            microtiles: BTreeMap::from([((4_i64, 5_i64), tile)]),
        };

        let mut history = VecDeque::from([make_slice(1), make_slice(2)]);
        let mut cache = LatentFieldCache::rebuild(&history, StepIndex::new(2), 11);
        assert_eq!(
            compile_field_from_cache(&cache),
            compile_field(&history, StepIndex::new(2))
        );

        cache.advance_to(StepIndex::new(3));
        assert_eq!(
            compile_field_from_cache(&cache),
            compile_field(&history, StepIndex::new(3))
        );

        let slice = make_slice(3);
        cache.insert_slice(&slice);
        history.push_back(slice);
        assert_eq!(
            compile_field_from_cache(&cache),
            compile_field(&history, StepIndex::new(3))
        );

        cache.advance_to(StepIndex::new(5));
        assert_eq!(
            compile_field_from_cache(&cache),
            compile_field(&history, StepIndex::new(5))
        );

        let removed = history
            .pop_front()
            .expect("history should contain oldest slice");
        cache.remove_slice(&removed);
        assert_eq!(
            compile_field_from_cache(&cache),
            compile_field(&history, StepIndex::new(5))
        );
    }
}
