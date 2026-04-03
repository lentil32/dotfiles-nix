use super::AgeMoment;
use super::CellRect;
use super::CellRows;
use super::CompiledCell;
use super::MICRO_TILE_SAMPLES;
use super::MIN_COMPILED_SAMPLE_Q12;
use super::MicroTile;
use super::SAMPLE_Q12_SCALE;
#[cfg(test)]
use super::slice_recent_weight_q16;
#[cfg(test)]
use super::slice_weight_q16;
use super::store::BucketCell;
#[cfg(test)]
use super::store::DepositedSlice;
use super::store::LatentFieldCache;
use super::weights::WEIGHT_Q16_SCALE;
use super::weights::recent_weight_q16_for_curve;
use super::weights::weight_q16_for_curve;
#[cfg(test)]
use crate::core::types::StepIndex;
use std::collections::BTreeMap;
use std::collections::HashMap;
#[cfg(test)]
use std::collections::VecDeque;

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
pub(in super::super) struct CompileScratch {
    accum: HashMap<(i64, i64), AccumulatorCell>,
}

impl CompileScratch {
    #[cfg(test)]
    pub(in super::super) fn accumulator_capacity(&self) -> usize {
        self.accum.capacity()
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

fn accumulate_weighted_bucket(
    scratch: &mut CompileScratch,
    bucket: &CellRows<BucketCell>,
    weight_q16: u64,
    recent_weight_q16: u64,
    bounds: Option<CellRect>,
) {
    let mut accumulate = |coord, bucket_cell: &BucketCell| {
        accumulate_bucket_weighted_cell(
            scratch.accum.entry(coord).or_default(),
            bucket_cell,
            weight_q16,
            recent_weight_q16,
        );
    };
    if let Some(bounds) = bounds {
        bucket.for_each_in_bounds(bounds, &mut accumulate);
    } else {
        bucket.for_each(&mut accumulate);
    }
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

fn finalize_compiled_cell(cell: &AccumulatorCell) -> Option<CompiledCell> {
    if !cell.survives() {
        return None;
    }

    let mut tile = MicroTile::default();
    for (index, weighted_sample) in cell.weighted_samples.iter().copied().enumerate() {
        let normalized = (weighted_sample / WEIGHT_Q16_SCALE).min(u64::from(SAMPLE_Q12_SCALE));
        tile.samples_q12[index] = normalized as u16;
    }

    if tile.max_sample_q12() < MIN_COMPILED_SAMPLE_Q12 {
        return None;
    }

    let total_mass_q12 =
        (cell.weighted_total_mass / WEIGHT_Q16_SCALE).min(u64::from(u32::MAX)) as u32;
    let recent_mass_q12 =
        (cell.weighted_recent_mass / WEIGHT_Q16_SCALE).min(u64::from(u32::MAX)) as u32;

    Some(CompiledCell {
        tile,
        age: AgeMoment {
            total_mass_q12,
            recent_mass_q12,
        },
    })
}

fn finalize_compiled_cells(
    accum: &HashMap<(i64, i64), AccumulatorCell>,
) -> BTreeMap<(i64, i64), CompiledCell> {
    let mut compiled = BTreeMap::<(i64, i64), CompiledCell>::new();
    for (coord, cell) in accum {
        if let Some(compiled_cell) = finalize_compiled_cell(cell) {
            compiled.insert(*coord, compiled_cell);
        }
    }
    compiled
}

fn finalize_compiled_cell_rows(
    accum: &HashMap<(i64, i64), AccumulatorCell>,
) -> CellRows<CompiledCell> {
    let mut compiled = CellRows::<CompiledCell>::default();
    for (coord, cell) in accum {
        if let Some(compiled_cell) = finalize_compiled_cell(cell) {
            let _ = compiled.insert(*coord, compiled_cell);
        }
    }
    compiled
}

fn for_each_weighted_bucket<F>(cache: &LatentFieldCache, mut visit: F)
where
    F: FnMut(u64, u64, &CellRows<BucketCell>),
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

#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the explicit reference compiler stays available for future differential tests"
    )
)]
pub(in super::super) fn compile_field_reference(
    cache: &LatentFieldCache,
) -> BTreeMap<(i64, i64), CompiledCell> {
    let mut scratch = CompileScratch::default();
    compile_field_reference_with_scratch(cache, &mut scratch)
}

#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the BTreeMap-based bounded compiler remains available for differential tests"
    )
)]
pub(in super::super) fn compile_field_in_bounds(
    cache: &LatentFieldCache,
    bounds: CellRect,
) -> BTreeMap<(i64, i64), CompiledCell> {
    let mut scratch = CompileScratch::default();
    compile_field_in_bounds_with_scratch(cache, bounds, &mut scratch)
}

pub(in super::super) fn compile_field_reference_with_scratch(
    cache: &LatentFieldCache,
    scratch: &mut CompileScratch,
) -> BTreeMap<(i64, i64), CompiledCell> {
    compile_field_reference_in_bounds_impl_map(cache, None, scratch)
}

pub(in super::super) fn compile_field_in_bounds_with_scratch(
    cache: &LatentFieldCache,
    bounds: CellRect,
    scratch: &mut CompileScratch,
) -> BTreeMap<(i64, i64), CompiledCell> {
    compile_field_reference_in_bounds_impl_map(cache, Some(bounds), scratch)
}

fn compile_field_reference_in_bounds_impl(
    cache: &LatentFieldCache,
    bounds: Option<CellRect>,
    scratch: &mut CompileScratch,
) {
    scratch.accum.clear();

    // Reference path: materialize the full compiled field from the retained latent cache. The
    // planner still uses a reusable scratch map here so differential tests do not regress from
    // obvious allocation churn while the optimized path is being built separately.
    for_each_weighted_bucket(cache, |weight_q16, recent_weight_q16, bucket| {
        accumulate_weighted_bucket(scratch, bucket, weight_q16, recent_weight_q16, bounds);
    });
}

pub(in super::super) fn compile_field_in_bounds_rows_with_scratch(
    cache: &LatentFieldCache,
    bounds: CellRect,
    scratch: &mut CompileScratch,
) -> CellRows<CompiledCell> {
    compile_field_reference_in_bounds_impl(cache, Some(bounds), scratch);
    if scratch.accum.is_empty() {
        return CellRows::default();
    }

    finalize_compiled_cell_rows(&scratch.accum)
}

fn compile_field_reference_in_bounds_impl_map(
    cache: &LatentFieldCache,
    bounds: Option<CellRect>,
    scratch: &mut CompileScratch,
) -> BTreeMap<(i64, i64), CompiledCell> {
    compile_field_reference_in_bounds_impl(cache, bounds, scratch);
    if scratch.accum.is_empty() {
        return BTreeMap::new();
    }

    finalize_compiled_cells(&scratch.accum)
}

#[cfg(test)]
pub(in super::super) fn compile_field(
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
