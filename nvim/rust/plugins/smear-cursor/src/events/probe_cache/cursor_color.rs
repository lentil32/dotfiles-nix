use super::super::lru_cache::LruCache;
use crate::core::effect::ProbePolicy;
use crate::core::state::CursorColorProbeWitness;
use crate::core::state::CursorColorSample;
use crate::core::state::ProbeReuse;
use crate::host::BufferHandle;

pub(super) const CURSOR_COLOR_CACHE_CAPACITY: usize = 16;
const CURSOR_COLOR_MOTION_CACHE_CAPACITY: usize = 16;

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) enum CursorColorCacheLookup {
    Miss,
    Hit(Option<CursorColorSample>),
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct CachedCursorColorProbeSample {
    reuse: ProbeReuse,
    sample: Option<CursorColorSample>,
}

impl CachedCursorColorProbeSample {
    const fn new(reuse: ProbeReuse, sample: Option<CursorColorSample>) -> Self {
        Self { reuse, sample }
    }

    pub(crate) const fn reuse(self) -> ProbeReuse {
        self.reuse
    }

    pub(crate) const fn sample(self) -> Option<CursorColorSample> {
        self.sample
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
struct CursorColorMotionCacheKey {
    window_handle: i64,
    buffer_handle: BufferHandle,
    changedtick: u64,
    mode: String,
    line: i64,
    colorscheme_generation: crate::core::types::Generation,
    cache_generation: crate::core::types::Generation,
}

impl CursorColorMotionCacheKey {
    fn from_witness(witness: &CursorColorProbeWitness) -> Option<Self> {
        // Mirror `CompatibleWithinLine`: motion-cache reuse collapses only
        // column drift within the same line, never cross-line movement.
        let line = witness.cursor_position()?.row();
        Some(Self {
            window_handle: witness.window_handle(),
            buffer_handle: witness.buffer_handle(),
            changedtick: witness.changedtick(),
            mode: witness.mode().to_owned(),
            line,
            colorscheme_generation: witness.colorscheme_generation(),
            cache_generation: witness.cache_generation(),
        })
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct CursorColorProbeCache {
    exact: LruCache<CursorColorProbeWitness, Option<CursorColorSample>>,
    motion: LruCache<CursorColorMotionCacheKey, Option<CursorColorSample>>,
}

impl Default for CursorColorProbeCache {
    fn default() -> Self {
        Self {
            exact: LruCache::new(CURSOR_COLOR_CACHE_CAPACITY),
            motion: LruCache::new(CURSOR_COLOR_MOTION_CACHE_CAPACITY),
        }
    }
}

impl CursorColorProbeCache {
    pub(super) fn cached_sample(
        &mut self,
        witness: &CursorColorProbeWitness,
    ) -> CursorColorCacheLookup {
        self.exact
            .get_copy(witness)
            .map_or(CursorColorCacheLookup::Miss, CursorColorCacheLookup::Hit)
    }

    pub(super) fn cached_sample_for_probe(
        &mut self,
        witness: &CursorColorProbeWitness,
        probe_policy: ProbePolicy,
        reuse: ProbeReuse,
    ) -> Option<CachedCursorColorProbeSample> {
        if let CursorColorCacheLookup::Hit(sample) = self.cached_sample(witness) {
            return Some(CachedCursorColorProbeSample::new(reuse, sample));
        }

        if reuse != ProbeReuse::Compatible || !probe_policy.allows_compatible_cursor_color_reuse() {
            return None;
        }

        let motion_key = CursorColorMotionCacheKey::from_witness(witness)?;
        self.motion
            .get_copy(&motion_key)
            .map(|sample| CachedCursorColorProbeSample::new(ProbeReuse::Compatible, sample))
    }

    pub(super) fn store_sample(
        &mut self,
        witness: CursorColorProbeWitness,
        sample: Option<CursorColorSample>,
    ) {
        let motion_key = CursorColorMotionCacheKey::from_witness(&witness);
        self.exact.insert(witness, sample);
        if let Some(motion_key) = motion_key {
            self.motion.insert(motion_key, sample);
        }
    }

    pub(super) fn clear(&mut self) {
        self.exact.clear();
        self.motion.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::CachedCursorColorProbeSample;
    use super::CursorColorCacheLookup;
    use super::CursorColorProbeCache;
    use crate::core::effect::CursorColorFallbackMode;
    use crate::core::effect::CursorColorReuseMode;
    use crate::core::effect::CursorPositionProbeMode;
    use crate::core::effect::ProbePolicy;
    use crate::core::effect::ProbeQuality;
    use crate::core::state::CursorColorSample;
    use crate::core::state::ProbeReuse;
    use crate::test_support::cursor;
    use crate::test_support::cursor_color_probe_witness_with_cache_generation as witness_with_cache_generation;
    use crate::test_support::proptest::mode_case;
    use crate::test_support::proptest::pure_config;
    use proptest::prelude::*;

    fn sample_strategy() -> BoxedStrategy<Option<CursorColorSample>> {
        proptest::option::of(any::<u32>().prop_map(CursorColorSample::new)).boxed()
    }

    fn different_mode(mode: &str) -> &'static str {
        match mode {
            "n" | "no" => "i",
            "i" | "ic" => "R",
            "R" | "Rc" => "t",
            _ => "n",
        }
    }

    proptest! {
        #![proptest_config(pure_config())]

        #[test]
        fn prop_cursor_color_probe_cache_exact_lookup_tracks_each_witness_field(
            window_handle in any::<i64>(),
            buffer_handle in any::<i64>(),
            changedtick in any::<u64>(),
            mode in mode_case(),
            row in 1_u32..256,
            col in 1_u32..256,
            colorscheme_generation in any::<u64>(),
            cache_generation in any::<u64>(),
            sample in sample_strategy(),
            mutated_field in 0_usize..8,
        ) {
            let mut cache = CursorColorProbeCache::default();
            let base = witness_with_cache_generation(
                window_handle,
                buffer_handle,
                changedtick,
                mode.mode(),
                Some(cursor(row, col)),
                colorscheme_generation,
                cache_generation,
            );
            cache.store_sample(base.clone(), sample);

            prop_assert_eq!(cache.cached_sample(&base), CursorColorCacheLookup::Hit(sample));

            let mutated = match mutated_field {
                0 => witness_with_cache_generation(
                    window_handle.wrapping_add(1),
                    buffer_handle,
                    changedtick,
                    mode.mode(),
                    Some(cursor(row, col)),
                    colorscheme_generation,
                    cache_generation,
                ),
                1 => witness_with_cache_generation(
                    window_handle,
                    buffer_handle.wrapping_add(1),
                    changedtick,
                    mode.mode(),
                    Some(cursor(row, col)),
                    colorscheme_generation,
                    cache_generation,
                ),
                2 => witness_with_cache_generation(
                    window_handle,
                    buffer_handle,
                    changedtick.wrapping_add(1),
                    mode.mode(),
                    Some(cursor(row, col)),
                    colorscheme_generation,
                    cache_generation,
                ),
                3 => witness_with_cache_generation(
                    window_handle,
                    buffer_handle,
                    changedtick,
                    different_mode(mode.mode()),
                    Some(cursor(row, col)),
                    colorscheme_generation,
                    cache_generation,
                ),
                4 => witness_with_cache_generation(
                    window_handle,
                    buffer_handle,
                    changedtick,
                    mode.mode(),
                    Some(cursor(row, col.saturating_add(1))),
                    colorscheme_generation,
                    cache_generation,
                ),
                5 => witness_with_cache_generation(
                    window_handle,
                    buffer_handle,
                    changedtick,
                    mode.mode(),
                    Some(cursor(row.saturating_add(1), col)),
                    colorscheme_generation,
                    cache_generation,
                ),
                6 => witness_with_cache_generation(
                    window_handle,
                    buffer_handle,
                    changedtick,
                    mode.mode(),
                    Some(cursor(row, col)),
                    colorscheme_generation.wrapping_add(1),
                    cache_generation,
                ),
                7 => witness_with_cache_generation(
                    window_handle,
                    buffer_handle,
                    changedtick,
                    mode.mode(),
                    Some(cursor(row, col)),
                    colorscheme_generation,
                    cache_generation.wrapping_add(1),
                ),
                _ => unreachable!(),
            };

            prop_assert_eq!(cache.cached_sample(&mutated), CursorColorCacheLookup::Miss);
        }

        #[test]
        fn prop_cursor_color_probe_cache_compatible_reuse_is_same_line_and_policy_gated(
            window_handle in any::<i64>(),
            buffer_handle in any::<i64>(),
            changedtick in any::<u64>(),
            mode in mode_case(),
            row in 1_u32..256,
            col in 1_u32..256,
            colorscheme_generation in any::<u64>(),
            cache_generation in any::<u64>(),
            sample in sample_strategy(),
            column_delta in 1_u32..32,
        ) {
            let mut cache = CursorColorProbeCache::default();
            let base = witness_with_cache_generation(
                window_handle,
                buffer_handle,
                changedtick,
                mode.mode(),
                Some(cursor(row, col)),
                colorscheme_generation,
                cache_generation,
            );
            let moved = witness_with_cache_generation(
                window_handle,
                buffer_handle,
                changedtick,
                mode.mode(),
                Some(cursor(row, col.saturating_add(column_delta))),
                colorscheme_generation,
                cache_generation,
            );
            let exact_compatible_policy = ProbePolicy::from_modes(
                CursorPositionProbeMode::Exact,
                CursorColorReuseMode::CompatibleWithinLine,
                CursorColorFallbackMode::SyntaxThenExtmarks,
            );
            cache.store_sample(base, sample);

            prop_assert_eq!(cache.cached_sample(&moved), CursorColorCacheLookup::Miss);
            prop_assert_eq!(
                cache.cached_sample_for_probe(
                    &moved,
                    ProbePolicy::new(ProbeQuality::Exact),
                    ProbeReuse::Compatible,
                ),
                None,
            );
            prop_assert_eq!(
                cache.cached_sample_for_probe(&moved, exact_compatible_policy, ProbeReuse::Exact),
                None,
            );
            prop_assert_eq!(
                cache.cached_sample_for_probe(
                    &moved,
                    exact_compatible_policy,
                    ProbeReuse::Compatible,
                ),
                Some(CachedCursorColorProbeSample {
                    reuse: ProbeReuse::Compatible,
                    sample,
                }),
            );
        }

        #[test]
        fn prop_cursor_color_probe_cache_motion_lookup_tracks_reduced_motion_key(
            window_handle in any::<i64>(),
            buffer_handle in any::<i64>(),
            changedtick in any::<u64>(),
            mode in mode_case(),
            row in 1_u32..256,
            col in 1_u32..256,
            colorscheme_generation in any::<u64>(),
            cache_generation in any::<u64>(),
            sample in sample_strategy(),
            mutated_field in 0_usize..7,
            column_delta in 1_u32..32,
        ) {
            let mut cache = CursorColorProbeCache::default();
            let base = witness_with_cache_generation(
                window_handle,
                buffer_handle,
                changedtick,
                mode.mode(),
                Some(cursor(row, col)),
                colorscheme_generation,
                cache_generation,
            );
            cache.store_sample(base, sample);

            let compatible_policy = ProbePolicy::new(ProbeQuality::FastMotion);
            let same_line = witness_with_cache_generation(
                window_handle,
                buffer_handle,
                changedtick,
                mode.mode(),
                Some(cursor(row, col.saturating_add(column_delta))),
                colorscheme_generation,
                cache_generation,
            );
            prop_assert_eq!(
                cache.cached_sample_for_probe(
                    &same_line,
                    compatible_policy,
                    ProbeReuse::Compatible,
                ),
                Some(CachedCursorColorProbeSample {
                    reuse: ProbeReuse::Compatible,
                    sample,
                }),
            );

            let mutated = match mutated_field {
                0 => witness_with_cache_generation(
                    window_handle.wrapping_add(1),
                    buffer_handle,
                    changedtick,
                    mode.mode(),
                    Some(cursor(row, col.saturating_add(column_delta))),
                    colorscheme_generation,
                    cache_generation,
                ),
                1 => witness_with_cache_generation(
                    window_handle,
                    buffer_handle.wrapping_add(1),
                    changedtick,
                    mode.mode(),
                    Some(cursor(row, col.saturating_add(column_delta))),
                    colorscheme_generation,
                    cache_generation,
                ),
                2 => witness_with_cache_generation(
                    window_handle,
                    buffer_handle,
                    changedtick.wrapping_add(1),
                    mode.mode(),
                    Some(cursor(row, col.saturating_add(column_delta))),
                    colorscheme_generation,
                    cache_generation,
                ),
                3 => witness_with_cache_generation(
                    window_handle,
                    buffer_handle,
                    changedtick,
                    different_mode(mode.mode()),
                    Some(cursor(row, col.saturating_add(column_delta))),
                    colorscheme_generation,
                    cache_generation,
                ),
                4 => witness_with_cache_generation(
                    window_handle,
                    buffer_handle,
                    changedtick,
                    mode.mode(),
                    Some(cursor(row.saturating_add(1), col)),
                    colorscheme_generation,
                    cache_generation,
                ),
                5 => witness_with_cache_generation(
                    window_handle,
                    buffer_handle,
                    changedtick,
                    mode.mode(),
                    Some(cursor(row, col.saturating_add(column_delta))),
                    colorscheme_generation.wrapping_add(1),
                    cache_generation,
                ),
                6 => witness_with_cache_generation(
                    window_handle,
                    buffer_handle,
                    changedtick,
                    mode.mode(),
                    Some(cursor(row, col.saturating_add(column_delta))),
                    colorscheme_generation,
                    cache_generation.wrapping_add(1),
                ),
                _ => unreachable!(),
            };

            prop_assert_eq!(
                cache.cached_sample_for_probe(
                    &mutated,
                    compatible_policy,
                    ProbeReuse::Compatible,
                ),
                None,
            );
        }

        #[test]
        fn prop_cursor_color_probe_cache_clear_resets_exact_and_motion_entries(
            window_handle in any::<i64>(),
            buffer_handle in any::<i64>(),
            changedtick in any::<u64>(),
            mode in mode_case(),
            row in 1_u32..256,
            col in 1_u32..256,
            colorscheme_generation in any::<u64>(),
            cache_generation in any::<u64>(),
            sample in sample_strategy(),
            column_delta in 1_u32..32,
        ) {
            let mut cache = CursorColorProbeCache::default();
            let original = witness_with_cache_generation(
                window_handle,
                buffer_handle,
                changedtick,
                mode.mode(),
                Some(cursor(row, col)),
                colorscheme_generation,
                cache_generation,
            );
            let moved = witness_with_cache_generation(
                window_handle,
                buffer_handle,
                changedtick,
                mode.mode(),
                Some(cursor(row, col.saturating_add(column_delta))),
                colorscheme_generation,
                cache_generation,
            );
            cache.store_sample(original.clone(), sample);

            cache.clear();

            prop_assert_eq!(cache.cached_sample(&original), CursorColorCacheLookup::Miss);
            prop_assert_eq!(
                cache.cached_sample_for_probe(
                    &moved,
                    ProbePolicy::new(ProbeQuality::FastMotion),
                    ProbeReuse::Compatible,
                ),
                None,
            );
        }
    }
}
