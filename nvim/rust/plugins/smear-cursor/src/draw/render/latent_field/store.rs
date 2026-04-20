#[cfg(test)]
use super::CellRect;
use super::MICRO_TILE_SAMPLES;
use super::MaterializedTile;
use super::MicroTile;
#[cfg(test)]
use super::TailBand;
use super::spatial_index::CellRows;
#[cfg(test)]
use crate::core::types::ArcLenQ16;
use crate::core::types::StepIndex;
#[cfg(test)]
use crate::core::types::StrokeId;
use std::collections::BTreeMap;
#[cfg(test)]
use std::collections::VecDeque;

#[cfg(test)]
#[derive(Clone, Debug, PartialEq)]
pub(in super::super) struct DepositedSlice {
    pub(in super::super) stroke_id: StrokeId,
    pub(in super::super) step_index: StepIndex,
    pub(in super::super) dt_ms_q16: u32,
    pub(in super::super) arc_len_q16: ArcLenQ16,
    pub(in super::super) bbox: CellRect,
    pub(in super::super) band: TailBand,
    pub(in super::super) support_steps: usize,
    pub(in super::super) intensity_q16: u32,
    pub(in super::super) microtiles: BTreeMap<(i64, i64), MicroTile>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct WeightCurveKey {
    pub(super) support_steps: usize,
    pub(super) intensity_q16: u32,
    pub(super) dt_ms_q16: u32,
}

impl WeightCurveKey {
    fn new(support_steps: usize, intensity_q16: u32, dt_ms_q16: u32) -> Self {
        Self {
            support_steps: support_steps.max(1),
            intensity_q16,
            dt_ms_q16,
        }
    }

    #[cfg(test)]
    pub(super) fn from_slice(slice: &DepositedSlice) -> Self {
        Self::new(slice.support_steps, slice.intensity_q16, slice.dt_ms_q16)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct BucketCell {
    // Each curve/age bucket currently receives at most one slice per render step, so `u32`
    // leaves ample headroom while cutting the retained latent-cache footprint roughly in half.
    pub(super) samples_q12_sum: [u32; MICRO_TILE_SAMPLES],
    pub(super) total_mass_q12: u32,
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

    #[cfg(test)]
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

    #[cfg(test)]
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
pub(super) struct WeightCurveBuckets {
    age_zero_slot: usize,
    pub(super) buckets: Vec<CellRows<BucketCell>>,
}

impl WeightCurveBuckets {
    fn with_support_steps(support_steps: usize) -> Self {
        Self {
            age_zero_slot: 0,
            buckets: vec![CellRows::default(); support_steps.max(1)],
        }
    }

    fn bucket_index(&self, age_steps: u64) -> Option<usize> {
        let age_index = usize::try_from(age_steps).ok()?;
        if age_index >= self.buckets.len() {
            return None;
        }
        Some((self.age_zero_slot + age_index) % self.buckets.len())
    }

    pub(super) fn bucket_for_age(&self, age_steps: u64) -> Option<&CellRows<BucketCell>> {
        let index = self.bucket_index(age_steps)?;
        self.buckets.get(index)
    }

    fn bucket_for_age_mut(&mut self, age_steps: u64) -> Option<&mut CellRows<BucketCell>> {
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
        self.buckets.iter().all(CellRows::is_empty)
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(in super::super) struct LatentFieldCache {
    latest_step: StepIndex,
    revision: u64,
    pub(super) curves: BTreeMap<WeightCurveKey, WeightCurveBuckets>,
}

impl LatentFieldCache {
    pub(in super::super) const fn latest_step(&self) -> StepIndex {
        self.latest_step
    }

    pub(in super::super) const fn revision(&self) -> u64 {
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

    pub(in super::super) fn advance_to(&mut self, latest_step: StepIndex) {
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

    fn insert_tiles<'a, I>(
        &mut self,
        step_index: StepIndex,
        dt_ms_q16: u32,
        support_steps: usize,
        intensity_q16: u32,
        tiles: I,
    ) where
        I: IntoIterator<Item = ((i64, i64), &'a MicroTile)>,
    {
        let key = WeightCurveKey::new(support_steps, intensity_q16, dt_ms_q16);
        let age_steps = self.latest_step.value().saturating_sub(step_index.value());
        let buckets = self
            .curves
            .entry(key)
            .or_insert_with(|| WeightCurveBuckets::with_support_steps(key.support_steps));
        let Some(bucket) = buckets.bucket_for_age_mut(age_steps) else {
            return;
        };

        let mut changed = false;
        for (coord, tile) in tiles {
            bucket.entry_mut(coord).add_tile(tile);
            changed = true;
        }
        if changed {
            self.revision = self.revision.saturating_add(1);
        }
    }

    pub(in super::super) fn insert_materialized_slice(
        &mut self,
        step_index: StepIndex,
        dt_ms_q16: u32,
        support_steps: usize,
        intensity_q16: u32,
        tiles: &[MaterializedTile],
    ) {
        self.insert_tiles(
            step_index,
            dt_ms_q16,
            support_steps,
            intensity_q16,
            tiles.iter().map(|tile| (tile.coord, &tile.tile)),
        );
    }

    #[cfg(test)]
    pub(in super::super) fn insert_slice(&mut self, slice: &DepositedSlice) {
        self.insert_tiles(
            slice.step_index,
            slice.dt_ms_q16,
            slice.support_steps,
            slice.intensity_q16,
            slice.microtiles.iter().map(|(&coord, tile)| (coord, tile)),
        );
    }

    #[cfg(test)]
    fn remove_slice(&mut self, slice: &DepositedSlice) {
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
                    let mut remove_cell = false;
                    let existed = bucket.get_mut(*coord).is_some_and(|cell| {
                        cell.remove_tile(tile);
                        remove_cell = cell.is_empty();
                        true
                    });
                    changed |= existed;
                    if remove_cell {
                        let _ = bucket.remove(*coord);
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

#[cfg(test)]
mod tests {
    use super::super::CellRect;
    use super::super::MICRO_TILE_SAMPLES;
    use super::super::MaterializedTile;
    use super::super::MicroTile;
    use super::super::SAMPLE_Q12_SCALE;
    use super::super::TailBand;
    use super::super::compile_field;
    use super::super::compile_field_reference_with_scratch;
    use super::super::intensity_q16;
    use super::super::q16_from_non_negative;
    use super::super::simulation_step_ms;
    use super::BucketCell;
    use super::DepositedSlice;
    use super::LatentFieldCache;
    use super::prune_history;
    use crate::core::types::ArcLenQ16;
    use crate::core::types::StepIndex;
    use crate::core::types::StrokeId;
    use pretty_assertions::assert_eq;
    use std::collections::BTreeMap;
    use std::collections::VecDeque;

    fn compile_field_reference(
        cache: &LatentFieldCache,
    ) -> BTreeMap<(i64, i64), super::super::CompiledCell> {
        let mut scratch = super::super::CompileScratch::default();
        compile_field_reference_with_scratch(cache, &mut scratch)
    }

    fn fully_covered_tile() -> MicroTile {
        MicroTile {
            samples_q12: [SAMPLE_Q12_SCALE as u16; MICRO_TILE_SAMPLES],
        }
    }

    #[test]
    fn prune_history_keeps_recent_window() {
        let mut history = VecDeque::new();
        for step in 0_u64..10_u64 {
            history.push_back(DepositedSlice {
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
    fn latent_bucket_cell_stays_compact() {
        assert!(
            std::mem::size_of::<BucketCell>() <= 260,
            "bucket cache cell regressed in size: {} bytes",
            std::mem::size_of::<BucketCell>()
        );
    }

    #[test]
    fn latent_cache_incremental_updates_match_direct_replay() {
        let tile = fully_covered_tile();
        let bbox = CellRect::new(4, 6, 5, 7);
        let make_slice = |step: u64| DepositedSlice {
            stroke_id: StrokeId::new(1),
            step_index: StepIndex::new(step),
            dt_ms_q16: q16_from_non_negative(simulation_step_ms(120.0)),
            arc_len_q16: ArcLenQ16::ZERO,
            bbox,
            band: TailBand::Core,
            support_steps: 4,
            intensity_q16: intensity_q16(1.0),
            microtiles: BTreeMap::from([
                ((4_i64, 5_i64), tile),
                ((4_i64, 7_i64), tile),
                ((6_i64, 6_i64), tile),
            ]),
        };

        let mut history = VecDeque::from([make_slice(1), make_slice(2)]);
        let mut cache = LatentFieldCache::rebuild(&history, StepIndex::new(2), 11);
        assert_eq!(
            compile_field_reference(&cache),
            compile_field(&history, StepIndex::new(2))
        );

        cache.advance_to(StepIndex::new(3));
        assert_eq!(
            compile_field_reference(&cache),
            compile_field(&history, StepIndex::new(3))
        );

        let slice = make_slice(3);
        cache.insert_slice(&slice);
        history.push_back(slice);
        assert_eq!(
            compile_field_reference(&cache),
            compile_field(&history, StepIndex::new(3))
        );

        cache.advance_to(StepIndex::new(5));
        assert_eq!(
            compile_field_reference(&cache),
            compile_field(&history, StepIndex::new(5))
        );

        let removed = history
            .pop_front()
            .expect("history should contain oldest slice");
        cache.remove_slice(&removed);
        assert_eq!(
            compile_field_reference(&cache),
            compile_field(&history, StepIndex::new(5))
        );
    }

    #[test]
    fn insert_materialized_slice_matches_slice_replay() {
        let tile = fully_covered_tile();
        let bbox = CellRect::new(4, 6, 5, 7);
        let step_index = StepIndex::new(3);
        let dt_ms_q16 = q16_from_non_negative(simulation_step_ms(120.0));
        let support_steps = 4;
        let intensity_q16 = intensity_q16(1.0);
        let materialized_tiles = [
            MaterializedTile {
                coord: (4, 5),
                tile,
            },
            MaterializedTile {
                coord: (4, 7),
                tile,
            },
            MaterializedTile {
                coord: (6, 6),
                tile,
            },
        ];
        let slice = DepositedSlice {
            stroke_id: StrokeId::new(1),
            step_index,
            dt_ms_q16,
            arc_len_q16: ArcLenQ16::ZERO,
            bbox,
            band: TailBand::Core,
            support_steps,
            intensity_q16,
            microtiles: BTreeMap::from(materialized_tiles.map(|tile| (tile.coord, tile.tile))),
        };

        let mut materialized_cache = LatentFieldCache::default();
        materialized_cache.advance_to(step_index);
        materialized_cache.insert_materialized_slice(
            step_index,
            dt_ms_q16,
            support_steps,
            intensity_q16,
            &materialized_tiles,
        );

        let mut replay_cache = LatentFieldCache::default();
        replay_cache.advance_to(step_index);
        replay_cache.insert_slice(&slice);

        assert_eq!(
            compile_field_reference(&materialized_cache),
            compile_field_reference(&replay_cache)
        );
        assert_eq!(materialized_cache.revision(), replay_cache.revision());
    }
}
